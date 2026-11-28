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
