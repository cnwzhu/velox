//! 前沿渲染与动效专项验收测试：
//! 1. Motion 2.0 零膨胀 Q4（1/16 px）亚像素弹簧精度与快照变更检出；
//! 2. `Rgb565` 4×4 Bayer 有序抖动消除 16-bit 渐变色带（32-bit 格式零开销直通）；
//! 3. 可选 `swash` feature 零拷贝 OpenType GSUB/GPOS 塑形与可变字体轴验证。

use velox_engine::display::{PixelFormat, Rgb565, Rgba8888, Xrgb8888};
use velox_engine::motion::{
    pack_subpx_q4, unpack_subpx_q4, DrawOp, DrawPayload, MotionCache, MotionState,
};
use velox_engine::prelude::*;
use velox_engine::renderers::software::rasterize_region;
use velox_engine::solver::Region;

#[test]
fn test_subpx_q4_pack_unpack_symmetry_and_zero_bloat() {
    for dx in -8i8..=7 {
        for dy in -8i8..=7 {
            let packed = pack_subpx_q4(dx, dy);
            assert_eq!(unpack_subpx_q4(packed), (dx, dy));
        }
    }
    // 越界输入自动钳位至 [-8, 7]
    assert_eq!(unpack_subpx_q4(pack_subpx_q4(127, -128)), (7, -8));

    // `DrawOp::to_snapshot()` 必须能感知 1/16 px 亚像素位移变化，保证弹簧尾段触发增量提交
    let mut op = DrawOp {
        id: Some(7),
        bounds: Region::new(10, 20, 100, 40),
        clip: Region::new(0, 0, 400, 300),
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: Color::rgb(37, 99, 235),
            bg_grad: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            rounded: 8,
            border_width: 0,
        },
    };
    let snap0 = op.to_snapshot();
    op.subpx_q4 = pack_subpx_q4(3, -2);
    let snap1 = op.to_snapshot();
    assert_ne!(
        snap0, snap1,
        "1/16 px subpixel delta must alter DrawOpSnapshot hash"
    );

    // 零膨胀防线：MotionState 应保持紧凑（不因新增 subpx_q4 发生结构体膨胀）
    assert!(
        core::mem::size_of::<MotionState>() <= 24,
        "MotionState bloated to {} bytes",
        core::mem::size_of::<MotionState>()
    );
}

#[test]
fn test_spring_emits_nonzero_subpx_q4_and_settles_to_zero() {
    let mut cache = MotionCache::new();
    let spec = TransitionSpec {
        duration_ms: 240,
        animate_bounds: true,
        easing: Easing::Spring,
        ..TransitionSpec::default()
    };

    cache.begin_frame();
    let s0 = cache.resolve(
        901,
        MotionState::new(Region::new(10, 20, 80, 40), Color::WHITE, Color::TRANSPARENT),
        Some(spec),
    );
    cache.end_frame();
    assert_eq!(s0.subpx_q4, 0);

    // 切换到新目标并逐帧推进，验证飞行及尾段中持续输出非零 Q4 亚像素残差
    cache.begin_frame();
    let _ = cache.resolve(
        901,
        MotionState::new(Region::new(160, 20, 80, 40), Color::WHITE, Color::TRANSPARENT),
        Some(spec),
    );
    cache.end_frame();

    let mut saw_nonzero_subpx = false;
    for _ in 0..100 {
        let running = cache.tick(16);
        if let Some(st) = cache.current_state(901) {
            if st.subpx_q4 != 0 {
                saw_nonzero_subpx = true;
            }
        }
        if !running {
            break;
        }
    }
    assert!(
        saw_nonzero_subpx,
        "spring animation must emit nonzero Q4 subpixel residuals during flight/tail"
    );

    // 推进至完全收敛后，稳态亚像素残差必须归 0 且精确落在目标整数坐标
    for _ in 0..200 {
        if !cache.tick(16) {
            break;
        }
    }
    let settled = cache.current_state(901).expect("spring box state");
    assert_eq!(settled.bounds.x, 160);
    assert_eq!(settled.subpx_q4, 0);
}

#[test]
fn test_rgb565_bayer_dither_reduces_quantization_error_and_32bit_is_noop() {
    assert!(Rgb565::NEEDS_DITHER);
    assert!(!Xrgb8888::NEEDS_DITHER);
    assert!(!Rgba8888::NEEDS_DITHER);

    // 32-bit 格式必须零改动直通
    let sample = Color::rgb(123, 145, 167);
    assert_eq!(
        Xrgb8888::from_color_dithered(sample, 1, 3),
        Xrgb8888::from_color(sample)
    );
    assert_eq!(
        Rgba8888::from_color_dithered(sample, 2, 1),
        Rgba8888::from_color(sample)
    );

    // 选一个位于 Rgb565 5-bit 量化中点（R=100 介于 96 和 104 之间）的灰度色，
    // 在 4×4 Bayer 矩阵上的空间均值应大幅逼近原始值，且包含两种相邻量化码值
    let mid_gray = Color::rgb(100, 102, 100);
    let flat_raw = Rgb565::from_color(mid_gray);
    let mut sum_r = 0u32;
    let mut sum_g = 0u32;
    let mut distinct_values = std::collections::HashSet::new();
    for y in 0..4i16 {
        for x in 0..4i16 {
            let p = Rgb565::from_color_dithered(mid_gray, x, y);
            distinct_values.insert(p.0);
            let r5 = (p.0 >> 11) & 0x1F;
            let g6 = (p.0 >> 5) & 0x3F;
            sum_r += (r5 as u32 * 255 + 15) / 31;
            sum_g += (g6 as u32 * 255 + 31) / 63;
        }
    }
    assert!(
        distinct_values.len() >= 2,
        "4x4 Bayer dither should alternate adjacent RGB565 quantization levels for mid-step color"
    );
    let avg_r = (sum_r as f32) / 16.0;
    let avg_g = (sum_g as f32) / 16.0;
    let flat_r = (((flat_raw.0 >> 11) & 0x1F) as f32 * 255.0) / 31.0;
    assert!(
        (avg_r - 100.0).abs() <= (flat_r - 100.0).abs() + 0.5,
        "dithered red mean {avg_r:.2} should track target 100.0 better than flat {flat_r:.2}"
    );
    assert!(
        (avg_g - 102.0).abs() < 3.0,
        "dithered green mean {avg_g:.2} should stay close to target 102.0"
    );
}

#[test]
fn test_software_rgb565_gradient_uses_ordered_dither() {
    let mut pixels = vec![Rgb565::BLACK; 64 * 16];
    let dirty = Region::new(0, 0, 64, 16);
    // `border_width` 最高位 `0x80` 标识水平渐变
    let op = DrawOp {
        id: None,
        bounds: dirty,
        clip: dirty,
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: Color::rgb(96, 98, 96),
            bg_grad: Color::rgb(104, 106, 104),
            border_color: Color::TRANSPARENT,
            rounded: 0,
            border_width: 0x80,
        },
    };
    rasterize_region(&[op], dirty, 0, 0, 64, &mut pixels);

    // 水平渐变的同一列在垂直方向上，若无有序抖动所有行像素必然完全相同；
    // 启用 4×4 Bayer 抖动后，中间过渡列的不同行会出现平滑交替量化值。
    let mut vertical_variation_cols = 0usize;
    for x in 8..56 {
        let p0 = pixels[x];
        if (1..16).any(|y| pixels[y * 64 + x] != p0) {
            vertical_variation_cols += 1;
        }
    }
    assert!(
        vertical_variation_cols >= 8,
        "expected Bayer ordered dithering variation across gradient transition columns, got {vertical_variation_cols}"
    );
}

#[cfg(feature = "swash")]
#[test]
fn test_swash_feature_shaping_and_variable_axes_integration() {
    clear_swash_run_cache();
    let run = shape_text_run_swash(
        "Velox OpenType GSUB/GPOS AV fi fl",
        FontSpec::SANS,
        20.0,
        true,
        &[
            FontVariationAxis::weight(550.0),
            FontVariationAxis::optical_size(20.0),
        ],
    )
    .expect("swash shaping must succeed with system font");
    assert!(!run.glyphs.is_empty());
    assert!(run.total_advance > 100.0);

    let cached = shape_text_run_swash(
        "Velox OpenType GSUB/GPOS AV fi fl",
        FontSpec::SANS,
        20.0,
        true,
        &[
            FontVariationAxis::weight(550.0),
            FontVariationAxis::optical_size(20.0),
        ],
    )
    .expect("cached run");
    assert!(std::sync::Arc::ptr_eq(&run, &cached));
    assert!(swash_run_cache_stats().hits >= 1);

    // 多字体 Itemizer 回归：中英混排绝不能出现 .notdef (glyph_id == 0)
    let mixed = shape_text_run_swash("Hello 你好 Velox 世界", FontSpec::SANS, 20.0, true, &[])
        .expect("mixed script run");
    assert!(mixed.glyphs.len() >= 14);
    assert!(
        mixed.glyphs.iter().all(|g| g.glyph_id != 0),
        "mixed Latin+CJK run must not produce .notdef tofu glyphs"
    );
}

#[cfg(feature = "gpu")]
#[test]
fn test_small_text_subpixel_follows_spring_and_shared_slug_engine_with_canvas() {
    use velox_engine::canvas_scene::{
        CanvasAsciiLabel, CanvasItemState, CanvasStaticGeom, SceneCanvasSpec, StaticSceneId,
        Viewport2D,
    };
    use velox_engine::renderers::wgpu::WgpuOffscreenRenderer;

    let mut off = WgpuOffscreenRenderer::new_lazy(480, 200);
    if !off.ensure_gpu() {
        return;
    }

    // 1. 验证小字号（< 28px）在非零负 subpx_q4 偏移下正确保留浮点亚像素位移（无“框动字跳”）
    let vp = Region::new(0, 0, 480, 200);
    let make_ops = |subpx_q4: u8| {
        [
            DrawOp {
                id: None,
                bounds: vp,
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: Color::rgb(12, 16, 24),
                    bg_grad: Color::TRANSPARENT,
                    border_color: Color::TRANSPARENT,
                    rounded: 0,
                    border_width: 0,
                },
            },
            DrawOp {
                id: Some(11),
                bounds: Region::new(20, 20, 260, 28),
                clip: vp,
                clip_radius: 0,
                subpx_q4,
                payload: DrawPayload::TextRun {
                    text: StrRef::Static("Button Label 14px"),
                    color: Color::WHITE,
                    font_size: 14,
                    text_flags: 0,
                },
            },
        ]
    };
    let mut fb_zero = vec![Rgba8888(0); 480 * 200];
    let mut fb_sub = vec![Rgba8888(0); 480 * 200];
    let ops0 = make_ops(0);
    let ops_neg = make_ops(pack_subpx_q4(-4, 4)); // -0.25px X, +0.25px Y
    let _ = off.render_draw_ops_rgba8888_into(&ops0, vp, 1, &[vp], &mut fb_zero);
    let _ = off.render_draw_ops_rgba8888_into(&ops_neg, vp, 1, &[vp], &mut fb_sub);
    let diff_pixels = fb_zero
        .iter()
        .zip(fb_sub.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        diff_pixels > 20,
        "14px small text must smoothly shift with Q4 subpixel residual (got {diff_pixels} differing pixels)"
    );

    // 2. 验证同帧混用大字号 UI text() 与 SceneCanvas 标签时共享唯一 SlugTextEngine，不重复缓存曲线
    let geoms = [CanvasStaticGeom::rounded_rect_rotated(
        100.0, 50.0, 120.0, 40.0, 1.0, 0.0, 6.0,
    )];
    let states = [CanvasItemState::new(0)];
    let palette = [Color::rgb(30, 41, 59)];
    let labels = [CanvasAsciiLabel {
        wx: 100.0,
        wy: 50.0,
        text: "VELOX",
        font_px: 34.0,
        color: Color::WHITE,
    }];
    let spec = SceneCanvasSpec {
        id: 7001,
        width: 480,
        height: 120,
        opacity: 255,
        bg: Color::rgb(10, 14, 22),
        scene_id: StaticSceneId(7001),
        viewport: Viewport2D::new(0.0, 0.0, 1.0),
        geoms: &geoms,
        states: &states,
        geom_revision: 1,
        state_revision: 1,
        palette: &palette,
        mesh_verts: &[],
        mesh_indices: &[],
        mesh_revision: 0,
        visible_ranges: &[],
        labels: &labels,
        marquee: None,
        marquee_color: Color::TRANSPARENT,
        dirty_screen_rects: &[],
        zooming: false,
        bg_shaders: &[],
        fg_shaders: &[],
        software_bg_fallback: None,
        software_fg_fallback: None,
    };
    let mut shared_off = WgpuOffscreenRenderer::new_lazy(480, 200);
    assert!(shared_off.ensure_gpu());
    let shared_ops = [
        DrawOp {
            id: None,
            bounds: Region::new(0, 0, 480, 64),
            clip: vp,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::TextRun {
                text: StrRef::Static("VELOX"),
                color: Color::WHITE,
                font_size: 36,
                text_flags: 0,
            },
        },
        DrawOp {
            id: None,
            bounds: Region::new(0, 70, 480, 120),
            clip: vp,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::SceneCanvas(&spec),
        },
    ];
    let mut fb_shared = vec![Rgba8888(0); 480 * 200];
    let _ = shared_off.render_draw_ops_rgba8888_into(&shared_ops, vp, 1, &[vp], &mut fb_shared);
    let stats = shared_off.gpu_memory_stats().unwrap();
    // "VELOX" 仅 5 个独立字符：若 UI 与 Canvas 共享唯一 SlugTextEngine，缓存字形数必须精确为 5（而非双份 10！）
    assert_eq!(
        stats.slug_cached_glyphs, 5,
        "UI heading and SceneCanvas must share one SlugTextEngine curve cache (expected 5 unique glyphs, got {})",
        stats.slug_cached_glyphs
    );
    assert_eq!(
        stats.slug_last_drawn_glyphs, 10,
        "both UI heading (5) and SceneCanvas label (5) must draw in same frame"
    );

    // 3. 当启用 `swash` 特性时，验证 RichText + TextSpan::var_weight 可变字体轴直通共享 Slug 2.0 曲线渲染
    #[cfg(feature = "swash")]
    {
        let spans = [
            TextSpan::new("Spring ")
                .size(34)
                .var_weight(420.0)
                .color(Color::WHITE),
            TextSpan::new("Weight 你好")
                .size(34)
                .var_weight(760.0)
                .color(Color::rgb(96, 165, 250)),
        ];
        let rt_spec = RichTextSpec {
            spans: &spans,
            default_color: Color::WHITE,
            default_font_size: 34,
            line_height_pct: 130,
            align: TextAlign::Left,
            rtl_paragraph: false,
            max_lines: 1,
            nowrap: true,
        };
        let rt_ops = [
            DrawOp {
                id: None,
                bounds: vp,
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: Color::rgb(12, 16, 24),
                    bg_grad: Color::TRANSPARENT,
                    border_color: Color::TRANSPARENT,
                    rounded: 0,
                    border_width: 0,
                },
            },
            DrawOp {
                id: None,
                bounds: Region::new(16, 20, 440, 60),
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::RichText(&rt_spec),
            },
        ];
        let mut rt_off = WgpuOffscreenRenderer::new_lazy(480, 200);
        assert!(rt_off.ensure_gpu());
        let mut rt_fb = vec![Rgba8888(0); 480 * 200];
        let _ = rt_off.render_draw_ops_rgba8888_into(&rt_ops, vp, 1, &[vp], &mut rt_fb);
        let rt_stats = rt_off.gpu_memory_stats().unwrap();
        assert!(
            rt_stats.slug_last_drawn_glyphs >= 14,
            "RichText with variable font axes must route through Swash -> Slug 2.0, got {} drawn glyphs",
            rt_stats.slug_last_drawn_glyphs
        );
        let lit = rt_fb
            .iter()
            .filter(|px| {
                let c = px.to_color();
                c.r() > 140 || c.b() > 180
            })
            .count();
        assert!(lit > 400, "RichText variable-font Slug pass must render visible pixels, got {lit}");
    }
}

#[test]
fn test_hidpi_multistrip_precomputed_text_matches_single_strip_exact() {
    struct MultiLineHiDpiView;
    impl View for MultiLineHiDpiView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_col()
                .size_full()
                .bg(Color::rgb(16, 20, 30))
                .p(12)
                .gap(8)
                .child(
                    cx,
                    div()
                        .w(360)
                        .h(44)
                        .rounded(6)
                        .bg(Color::rgb(28, 36, 52))
                        .p(6)
                        .child(cx, text("Velox HiDPI 2.0x Precomputed Text Metrics").size(16).color(Color::WHITE)),
                )
                .child(
                    cx,
                    div()
                        .w(360)
                        .h(120)
                        .rounded(6)
                        .bg(Color::rgb(24, 32, 46))
                        .p(8)
                        .child(
                            cx,
                            text("Multi-line paragraph across several 16px parallel raster strips with CJK 中英文混排折行与行裁剪验证 1234567890")
                                .size(14)
                                .color(Color::rgb(210, 224, 240)),
                        ),
                )
                .into_element(cx)
        }
    }

    for sf in [1.5f32, 2.0f32] {
        let log_w: i16 = 400;
        let log_h: i16 = 240;
        let vp = Region::new(0, 0, log_w, log_h);
        let phys_w = ((log_w as f32) * sf).ceil() as usize;
        let phys_h = ((log_h as f32) * sf).ceil() as usize;

        let mut engine = VeloxEngine::new(log_w, log_h);
        engine.set_scale_factor(sf);
        engine.mount_root(MultiLineHiDpiView);
        let _ = engine.step_frame();

        // 整屏触发并行 16px 多条带路径（含 precompute_scaled_text_boxes）
        let mut fb_parallel = vec![Xrgb8888(0); phys_w * phys_h];
        engine.rasterize_strip(vp, 0, 0, phys_w, &mut fb_parallel);

        // 逐 12px 逻辑小块（面积 < 65_536，不触发并行池，走单线程保守裁剪路径）对比逐像素等价性
        let mut fb_serial = vec![Xrgb8888(0); phys_w * phys_h];
        for ly in (0..log_h).step_by(12) {
            let lh = (log_h - ly).min(12);
            engine.rasterize_strip(Region::new(0, ly, log_w, lh), 0, 0, phys_w, &mut fb_serial);
        }

        assert_eq!(
            fb_parallel, fb_serial,
            "HiDPI parallel precomputed text strips must match serial raster pixel-for-pixel at sf={sf}"
        );
    }
}

#[cfg(all(feature = "std", feature = "gpu"))]
#[test]
fn test_wgpu_dirty_scissor_filtered_with_copy_matches_full_frame_exact() {
    use velox_engine::renderers::wgpu::{PreparedWgpuContext, WgpuRenderer};

    let Some(prepared) = PreparedWgpuContext::init_headless() else {
        return;
    };
    let device = prepared.device().clone();
    let queue = prepared.queue().clone();
    let w = 256u32;
    let h = 160u32;
    let vp = Region::new(0, 0, w as i16, h as i16);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = WgpuRenderer::new(&device, format);

    let make_scene = |btn_color: Color, label: &'static str| -> Vec<DrawOp<'static>> {
        vec![
            DrawOp {
                id: Some(1),
                bounds: vp,
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: Color::rgb(18, 22, 32),
                    bg_grad: Color::rgb(18, 22, 32),
                    border_color: Color::TRANSPARENT,
                    rounded: 0,
                    border_width: 0,
                },
            },
            // 远端静态卡片（位于脏区外，应被脏区过滤跳过编码且完整保留于持久纹理）
            DrawOp {
                id: Some(2),
                bounds: Region::new(10, 10, 80, 40),
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: Color::rgb(40, 140, 90),
                    bg_grad: Color::rgb(40, 140, 90),
                    border_color: Color::TRANSPARENT,
                    rounded: 6,
                    border_width: 0,
                },
            },
            // 局部变更按钮（位于脏区内）
            DrawOp {
                id: Some(3),
                bounds: Region::new(130, 80, 100, 44),
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: btn_color,
                    bg_grad: btn_color,
                    border_color: Color::TRANSPARENT,
                    rounded: 8,
                    border_width: 0,
                },
            },
            DrawOp {
                id: Some(4),
                bounds: Region::new(140, 90, 80, 24),
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::TextRun {
                    text: StrRef::Static(label),
                    color: Color::WHITE,
                    font_size: 14,
                    text_flags: 0,
                },
            },
        ]
    };

    let mk_tex = |usage: wgpu::TextureUsages| {
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let v = t.create_view(&wgpu::TextureViewDescriptor::default());
        (t, v)
    };
    let (persistent_tex, persistent_view) = mk_tex(
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let (swap_tex, _swap_view) = mk_tex(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let (ref_tex, ref_view) = mk_tex(
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );

    let read_rgba = |tex: &wgpu::Texture| -> Vec<u8> {
        let row_bytes = (w * 4) as usize;
        let padded = row_bytes.div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (padded * h as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().unwrap().unwrap();
        let mapped = slice.get_mapped_range().unwrap();
        let mut out = Vec::with_capacity(row_bytes * h as usize);
        for y in 0..(h as usize) {
            out.extend_from_slice(&mapped[y * padded..y * padded + row_bytes]);
        }
        drop(mapped);
        buf.unmap();
        out
    };

    let clear = wgpu::Color::BLACK;
    let ops_f1 = make_scene(Color::rgb(50, 100, 220), "IDLE");
    let ops_f2 = make_scene(Color::rgb(230, 80, 50), "HOT!");
    let dirty_btn = Region::new(130, 80, 100, 44);

    // 帧 1：整屏建持久目标
    renderer.render_to_view(&device, &queue, &ops_f1, vp, 1.0, &persistent_view, w, h, clear);
    // 帧 2：局部 Scissor 重绘 + 单 Encoder 拷贝到 swap_tex
    let used_dirty = renderer.render_to_view_dirty_with_copy(
        &device,
        &queue,
        &ops_f2,
        vp,
        1.0,
        &persistent_view,
        w,
        h,
        &[dirty_btn],
        clear,
        Some((&persistent_tex, &swap_tex)),
    );
    assert!(used_dirty, "persistent target dirty path must activate");
    // 参考帧：整屏清屏重画帧 2
    renderer.render_to_view(&device, &queue, &ops_f2, vp, 1.0, &ref_view, w, h, clear);

    let got = read_rgba(&swap_tex);
    let expected = read_rgba(&ref_tex);
    assert_eq!(
        got, expected,
        "persistent target dirty scissor + single-encoder copy must match full-frame render pixel-for-pixel"
    );

    // 回归验证 P1-1：2x HiDPI 下窄布局框的长 nowrap 文本横向扩展至 parent_clip，
    // 当局部脏区落在超出 op.bounds 的文本扩展区间内时，局刷绝不可擦除或丢失任何文字像素！
    let w2 = 512u32;
    let h2 = 160u32;
    let vp2 = Region::new(0, 0, 256, 80); // sf = 2.0 -> 512x160 物理像素
    let mk_tex2 = |usage: wgpu::TextureUsages| {
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w2,
                height: h2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let v = t.create_view(&wgpu::TextureViewDescriptor::default());
        (t, v)
    };
    let (p2_tex, p2_view) = mk_tex2(
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let (s2_tex, _) = mk_tex2(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let (r2_tex, r2_view) = mk_tex2(
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let read_rgba2 = |tex: &wgpu::Texture| -> Vec<u8> {
        let row_bytes = (w2 * 4) as usize;
        let padded = row_bytes.div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (padded * h2 as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(h2),
                },
            },
            wgpu::Extent3d {
                width: w2,
                height: h2,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().unwrap().unwrap();
        let mapped = slice.get_mapped_range().unwrap();
        let mut out = Vec::with_capacity(row_bytes * h2 as usize);
        for y in 0..(h2 as usize) {
            out.extend_from_slice(&mapped[y * padded..y * padded + row_bytes]);
        }
        drop(mapped);
        buf.unmap();
        out
    };

    let long_text_ops = vec![
        DrawOp {
            id: Some(1),
            bounds: vp2,
            clip: vp2,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::Quad {
                bg: Color::rgb(16, 20, 28),
                bg_grad: Color::rgb(16, 20, 28),
                border_color: Color::TRANSPARENT,
                rounded: 0,
                border_width: 0,
            },
        },
        // 布局框极窄（w=32），但文本为 nowrap 且父裁剪框为整屏 vp2：在 2x HiDPI 下横向扩展到 x=240+
        DrawOp {
            id: Some(2),
            bounds: Region::new(8, 24, 32, 20),
            clip: vp2,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::TextRun {
                text: StrRef::Static("Velox HiDPI 2x Long Nowrap Overflow Text Preservation"),
                color: Color::WHITE,
                font_size: 14,
                text_flags: 0x08, // nowrap
            },
        },
    ];
    // 脏区刻意选在 x=100..220（完全位于 op.bounds [8..40] + 16px 之外，但正处于长文本扩展绘制区！）
    let overflow_dirty = Region::new(100, 16, 120, 36);
    renderer.render_to_view(&device, &queue, &long_text_ops, vp2, 2.0, &p2_view, w2, h2, clear);
    let used2 = renderer.render_to_view_dirty_with_copy(
        &device,
        &queue,
        &long_text_ops,
        vp2,
        2.0,
        &p2_view,
        w2,
        h2,
        &[overflow_dirty],
        clear,
        Some((&p2_tex, &s2_tex)),
    );
    assert!(used2);
    renderer.render_to_view(&device, &queue, &long_text_ops, vp2, 2.0, &r2_view, w2, h2, clear);

    let got2 = read_rgba2(&s2_tex);
    let exp2 = read_rgba2(&r2_tex);
    let diff2 = got2
        .chunks_exact(4)
        .zip(exp2.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        diff2, 0,
        "2x HiDPI dirty repaint over overflowed nowrap text must preserve every pixel (diff={diff2})"
    );

    // 回归验证 P1-2：2x HiDPI 下通过真实 VeloxEngine + DamageTracker 删除溢出布局框的长 nowrap 文本，
    // 引擎产出的脏区必须覆盖全部扩展像素，局刷后 0 残留残影！
    struct RemovableOverflowTextView {
        show_text: bool,
    }
    impl View for RemovableOverflowTextView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut root = div().w(256).h(80).bg(Color::rgb(16, 20, 28));
            if self.show_text {
                root = root.child(
                    cx,
                    div()
                        .id(42)
                        .mt(24)
                        .ml(8)
                        .w(12)
                        .h(20)
                        .child(
                            cx,
                            text("Velox HiDPI 2x Delete Overflow Text Zero Ghosting")
                                .size(14)
                                .nowrap()
                                .color(Color::WHITE),
                        ),
                );
            }
            root.into_element(cx)
        }
    }

    let mut eng = VeloxEngine::new(256, 80);
    eng.set_scale_factor(2.0);
    let view_h = eng.mount_root(RemovableOverflowTextView { show_text: true });
    let _ = eng.step_frame();
    // 帧 1：将带溢出长文本的完整画面写入持久纹理 p2_view
    renderer.render_to_view(
        &device,
        &queue,
        eng.cached_draw_ops(),
        vp2,
        2.0,
        &p2_view,
        w2,
        h2,
        clear,
    );

    // 帧 2：删除溢出长文本，获取引擎 DamageTracker 产出的真实脏区
    eng.view_mut(view_h).unwrap().show_text = false;
    let del_damage: Vec<Region> = eng.step_frame().to_vec();
    assert!(!del_damage.is_empty(), "deleting overflowed text must emit damage");
    assert!(
        del_damage.iter().any(|r| r.w > 100),
        "damage for deleted overflowed nowrap text must expand beyond narrow 12px layout box, got {del_damage:?}"
    );

    let used_del = renderer.render_to_view_dirty_with_copy(
        &device,
        &queue,
        eng.cached_draw_ops(),
        vp2,
        2.0,
        &p2_view,
        w2,
        h2,
        &del_damage,
        clear,
        Some((&p2_tex, &s2_tex)),
    );
    assert!(used_del);
    renderer.render_to_view(
        &device,
        &queue,
        eng.cached_draw_ops(),
        vp2,
        2.0,
        &r2_view,
        w2,
        h2,
        clear,
    );

    let got_del = read_rgba2(&s2_tex);
    let exp_del = read_rgba2(&r2_tex);
    let ghost_pixels = got_del
        .chunks_exact(4)
        .zip(exp_del.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        ghost_pixels, 0,
        "deleting overflowed nowrap text at 2x HiDPI must leave 0 ghost pixels after dirty repaint (got {ghost_pixels})"
    );

    // 回归验证 P1-3：125% 与 150% 分数 DPI 下删除与移动 1px 细线（绘制占 2 列物理像素），
    // 物理 Scissor 必须完整覆盖细线扩宽与像素取整列，局刷后 0 残留像素！
    for sf in [1.25f32, 1.5f32] {
        let log_w = 160i16;
        let log_h = 80i16;
        let vp_s = Region::new(0, 0, log_w, log_h);
        let pw_s = ((log_w as f32) * sf).round() as u32;
        let ph_s = ((log_h as f32) * sf).round() as u32;
        let mk_tex_s = |usage: wgpu::TextureUsages| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: pw_s,
                    height: ph_s,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            (t, v)
        };
        let (ps_tex, ps_view) =
            mk_tex_s(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let (ss_tex, _) = mk_tex_s(
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
        );
        let (rs_tex, rs_view) =
            mk_tex_s(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let read_rgba_s = |tex: &wgpu::Texture| -> Vec<u8> {
            let row_bytes = (pw_s * 4) as usize;
            let padded = row_bytes.div_ceil(256) * 256;
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (padded * ph_s as usize) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.copy_texture_to_buffer(
                tex.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded as u32),
                        rows_per_image: Some(ph_s),
                    },
                },
                wgpu::Extent3d {
                    width: pw_s,
                    height: ph_s,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([enc.finish()]);
            let slice = buf.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            rx.recv().unwrap().unwrap();
            let mapped = slice.get_mapped_range().unwrap();
            let mut out = Vec::with_capacity(row_bytes * ph_s as usize);
            for y in 0..(ph_s as usize) {
                out.extend_from_slice(&mapped[y * padded..y * padded + row_bytes]);
            }
            drop(mapped);
            buf.unmap();
            out
        };

        struct HairlineView {
            x: Option<i16>,
        }
        impl View for HairlineView {
            fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
                let mut root = div().w(160).h(80).bg(Color::rgb(16, 20, 28));
                if let Some(lx) = self.x {
                    root = root.child(
                        cx,
                        div()
                            .id(77)
                            .ml(lx)
                            .mt(15)
                            .w(1)
                            .h(30)
                            .bg(Color::rgb(255, 220, 80)),
                    );
                }
                root.into_element(cx)
            }
        }

        let mut eng_s = VeloxEngine::new(log_w, log_h);
        eng_s.set_scale_factor(sf);
        let h_s = eng_s.mount_root(HairlineView { x: Some(11) }); // 11 * 1.5 = 16.5 -> round=17, w=2 -> [17, 19)
        let _ = eng_s.step_frame();
        renderer.render_to_view(
            &device,
            &queue,
            eng_s.cached_draw_ops(),
            vp_s,
            sf,
            &ps_view,
            pw_s,
            ph_s,
            clear,
        );

        // 步骤 A：将 1px 细线从 x=11 移动到 x=43
        eng_s.view_mut(h_s).unwrap().x = Some(43);
        let move_dmg: Vec<Region> = eng_s.step_frame().to_vec();
        let used_move = renderer.render_to_view_dirty_with_copy(
            &device,
            &queue,
            eng_s.cached_draw_ops(),
            vp_s,
            sf,
            &ps_view,
            pw_s,
            ph_s,
            &move_dmg,
            clear,
            Some((&ps_tex, &ss_tex)),
        );
        assert!(used_move);
        renderer.render_to_view(
            &device,
            &queue,
            eng_s.cached_draw_ops(),
            vp_s,
            sf,
            &rs_view,
            pw_s,
            ph_s,
            clear,
        );
        let move_diff = read_rgba_s(&ss_tex)
            .chunks_exact(4)
            .zip(read_rgba_s(&rs_tex).chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            move_diff, 0,
            "moving 1px hairline at sf={sf} must leave 0 ghost pixels (diff={move_diff})"
        );

        // 步骤 B：删除 1px 细线
        eng_s.view_mut(h_s).unwrap().x = None;
        let del_dmg: Vec<Region> = eng_s.step_frame().to_vec();
        let used_del_line = renderer.render_to_view_dirty_with_copy(
            &device,
            &queue,
            eng_s.cached_draw_ops(),
            vp_s,
            sf,
            &ps_view,
            pw_s,
            ph_s,
            &del_dmg,
            clear,
            Some((&ps_tex, &ss_tex)),
        );
        assert!(used_del_line);
        renderer.render_to_view(
            &device,
            &queue,
            eng_s.cached_draw_ops(),
            vp_s,
            sf,
            &rs_view,
            pw_s,
            ph_s,
            clear,
        );
        let del_diff = read_rgba_s(&ss_tex)
            .chunks_exact(4)
            .zip(read_rgba_s(&rs_tex).chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            del_diff, 0,
            "deleting 1px hairline at sf={sf} must leave 0 ghost pixels (diff={del_diff})"
        );
    }

    // 回归验证 P1-4：1x 下圆角父裁剪（clip_radius > 0）中的 nowrap 文本溢出布局框，
    // 更新文本溢出段下方背景后，局刷绝不可误剔除该文本！
    let rounded_clip = Region::new(8, 8, 240, 64);
    let make_rounded_parent_ops = |under_bg: Color| -> Vec<DrawOp<'static>> {
        vec![
            DrawOp {
                id: Some(1),
                bounds: vp,
                clip: vp,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: Color::rgb(16, 20, 28),
                    bg_grad: Color::rgb(16, 20, 28),
                    border_color: Color::TRANSPARENT,
                    rounded: 0,
                    border_width: 0,
                },
            },
            // 位于文字溢出段（x=70..190）下方的可变背景块
            DrawOp {
                id: Some(2),
                bounds: Region::new(70, 18, 120, 32),
                clip: rounded_clip,
                clip_radius: 12,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: under_bg,
                    bg_grad: under_bg,
                    border_color: Color::TRANSPARENT,
                    rounded: 4,
                    border_width: 0,
                },
            },
            // 窄布局框（w=24）的 1x nowrap 文本，位于圆角父裁剪 clip_radius=12 内，字形绘制延伸至 x=220+
            DrawOp {
                id: Some(3),
                bounds: Region::new(16, 24, 24, 20),
                clip: rounded_clip,
                clip_radius: 12,
                subpx_q4: 0,
                payload: DrawPayload::TextRun {
                    text: StrRef::Static("Velox 1x Rounded Parent Clip Nowrap Text"),
                    color: Color::WHITE,
                    font_size: 14,
                    text_flags: 0x08, // nowrap
                },
            },
        ]
    };
    let rp_ops1 = make_rounded_parent_ops(Color::rgb(35, 55, 95));
    let rp_ops2 = make_rounded_parent_ops(Color::rgb(95, 45, 55));
    let under_dirty = Region::new(70, 18, 120, 32);
    renderer.render_to_view(&device, &queue, &rp_ops1, vp, 1.0, &persistent_view, w, h, clear);
    let used_rp = renderer.render_to_view_dirty_with_copy(
        &device,
        &queue,
        &rp_ops2,
        vp,
        1.0,
        &persistent_view,
        w,
        h,
        &[under_dirty],
        clear,
        Some((&persistent_tex, &swap_tex)),
    );
    assert!(used_rp);
    renderer.render_to_view(&device, &queue, &rp_ops2, vp, 1.0, &ref_view, w, h, clear);
    let rp_diff = read_rgba(&swap_tex)
        .chunks_exact(4)
        .zip(read_rgba(&ref_tex).chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        rp_diff, 0,
        "1x dirty repaint under overflowed nowrap text inside rounded parent clip must match full frame (diff={rp_diff})"
    );

    // 回归验证 P1-5：350% 与 400% 高倍 DPI 下删除带最大 Q4 亚像素偏移（±7/16 逻辑 px -> ±1.75 物理 px）的 1px 细线，
    // 按 sf 动态计算的物理 Scissor 余量必须完整覆盖偏移后的细线，局刷后 0 残留像素！
    for (sf, subpx_q4) in [
        (3.5f32, pack_subpx_q4(7, 7)),
        (4.0f32, pack_subpx_q4(7, -7)),
        (4.0f32, pack_subpx_q4(-7, 7)),
    ] {
        let log_w = 120i16;
        let log_h = 60i16;
        let vp_hi = Region::new(0, 0, log_w, log_h);
        let pw_hi = ((log_w as f32) * sf).round() as u32;
        let ph_hi = ((log_h as f32) * sf).round() as u32;
        let mk_tex_hi = |usage: wgpu::TextureUsages| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: pw_hi,
                    height: ph_hi,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            (t, v)
        };
        let (ph_tex, ph_view) =
            mk_tex_hi(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let (sh_tex, _) = mk_tex_hi(
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
        );
        let (rh_tex, rh_view) =
            mk_tex_hi(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let read_rgba_hi = |tex: &wgpu::Texture| -> Vec<u8> {
            let row_bytes = (pw_hi * 4) as usize;
            let padded = row_bytes.div_ceil(256) * 256;
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (padded * ph_hi as usize) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.copy_texture_to_buffer(
                tex.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded as u32),
                        rows_per_image: Some(ph_hi),
                    },
                },
                wgpu::Extent3d {
                    width: pw_hi,
                    height: ph_hi,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([enc.finish()]);
            let slice = buf.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            rx.recv().unwrap().unwrap();
            let mapped = slice.get_mapped_range().unwrap();
            let mut out = Vec::with_capacity(row_bytes * ph_hi as usize);
            for y in 0..(ph_hi as usize) {
                out.extend_from_slice(&mapped[y * padded..y * padded + row_bytes]);
            }
            drop(mapped);
            buf.unmap();
            out
        };

        let bg_op = DrawOp {
            id: Some(1),
            bounds: vp_hi,
            clip: vp_hi,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::Quad {
                bg: Color::rgb(16, 20, 28),
                bg_grad: Color::rgb(16, 20, 28),
                border_color: Color::TRANSPARENT,
                rounded: 0,
                border_width: 0,
            },
        };
        let line_op = DrawOp {
            id: Some(2),
            bounds: Region::new(25, 10, 1, 20),
            clip: vp_hi,
            clip_radius: 0,
            subpx_q4,
            payload: DrawPayload::Quad {
                bg: Color::rgb(255, 220, 80),
                bg_grad: Color::rgb(255, 220, 80),
                border_color: Color::TRANSPARENT,
                rounded: 0,
                border_width: 0,
            },
        };
        let ops_with_line = vec![bg_op, line_op];
        let ops_deleted = vec![bg_op];

        // 使用仅含原始逻辑细线框 Region(25, 10, 1, 20) 作为脏区，验证按 sf 计算的物理余量能否彻底清除 ±1.75px 偏移残影
        let raw_line_dirty = Region::new(25, 10, 1, 20);
        renderer.render_to_view(
            &device,
            &queue,
            &ops_with_line,
            vp_hi,
            sf,
            &ph_view,
            pw_hi,
            ph_hi,
            clear,
        );
        let used_hi = renderer.render_to_view_dirty_with_copy(
            &device,
            &queue,
            &ops_deleted,
            vp_hi,
            sf,
            &ph_view,
            pw_hi,
            ph_hi,
            &[raw_line_dirty],
            clear,
            Some((&ph_tex, &sh_tex)),
        );
        assert!(used_hi);
        renderer.render_to_view(
            &device,
            &queue,
            &ops_deleted,
            vp_hi,
            sf,
            &rh_view,
            pw_hi,
            ph_hi,
            clear,
        );
        let hi_diff = read_rgba_hi(&sh_tex)
            .chunks_exact(4)
            .zip(read_rgba_hi(&rh_tex).chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            hi_diff, 0,
            "deleting 1px hairline with subpx_q4={subpx_q4:#x} at sf={sf} must leave 0 ghost pixels (diff={hi_diff})"
        );
    }

    // 回归验证 P1-6：1x DPI 下删除圆角父容器中溢出布局框的 3 行多行文本，
    // DamageTracker 生成的脏区必须覆盖全部 3 行溢出字形，局刷后 0 残留像素！
    struct RoundedMultilineDeleteView {
        show_text: bool,
    }
    impl View for RoundedMultilineDeleteView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut card = div()
                .id(80)
                .mt(8)
                .ml(8)
                .w(240)
                .h(120)
                .rounded(12)
                .overflow_hidden()
                .bg(Color::rgb(26, 34, 48))
                .p(8);
            if self.show_text {
                // 刻意给文本节点极矮布局框 h=16、w=90，使其折成 3+ 行并纵向溢出布局框，同时被圆角父容器裁剪
                card = card.child(
                    cx,
                    div().id(81).w(90).h(16).child(
                        cx,
                        text("First line wraps into second line and third overflow line inside rounded clip")
                            .size(14)
                            .color(Color::WHITE),
                    ),
                );
            }
            div()
                .w(256)
                .h(160)
                .bg(Color::rgb(16, 20, 28))
                .child(cx, card)
                .into_element(cx)
        }
    }

    let mut eng_ml = VeloxEngine::new(w as i16, h as i16);
    eng_ml.set_scale_factor(1.0);
    let h_ml = eng_ml.mount_root(RoundedMultilineDeleteView { show_text: true });
    let _ = eng_ml.step_frame();
    renderer.render_to_view(
        &device,
        &queue,
        eng_ml.cached_draw_ops(),
        vp,
        1.0,
        &persistent_view,
        w,
        h,
        clear,
    );

    eng_ml.view_mut(h_ml).unwrap().show_text = false;
    let ml_dmg: Vec<Region> = eng_ml.step_frame().to_vec();
    assert!(!ml_dmg.is_empty());
    assert!(
        ml_dmg.iter().any(|r| r.h > 40),
        "deleting overflowing 3-line text inside rounded clip must emit tall damage covering all lines, got {ml_dmg:?}"
    );
    let used_ml = renderer.render_to_view_dirty_with_copy(
        &device,
        &queue,
        eng_ml.cached_draw_ops(),
        vp,
        1.0,
        &persistent_view,
        w,
        h,
        &ml_dmg,
        clear,
        Some((&persistent_tex, &swap_tex)),
    );
    assert!(used_ml);
    renderer.render_to_view(
        &device,
        &queue,
        eng_ml.cached_draw_ops(),
        vp,
        1.0,
        &ref_view,
        w,
        h,
        clear,
    );
    let ml_diff = read_rgba(&swap_tex)
        .chunks_exact(4)
        .zip(read_rgba(&ref_tex).chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        ml_diff, 0,
        "deleting overflowing multiline text inside rounded parent clip at 1x must leave 0 ghost pixels (diff={ml_diff})"
    );
}
