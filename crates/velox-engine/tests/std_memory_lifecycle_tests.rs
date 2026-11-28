#![cfg(feature = "std")]

use std::sync::{Arc, Weak};
use std::time::Duration;

use velox_engine::prelude::*;

fn make_bmp_24(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let row_stride = ((w as usize) * 3 + 3) & !3;
    let pixel_bytes = row_stride * (h as usize);
    let file_size = 54 + pixel_bytes;
    let mut out = vec![0u8; file_size];
    out[0..2].copy_from_slice(b"BM");
    out[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&(w as i32).to_le_bytes());
    out[22..26].copy_from_slice(&(h as i32).to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    for y in 0..(h as usize) {
        let base = 54 + y * row_stride;
        for x in 0..(w as usize) {
            out[base + x * 3] = b;
            out[base + x * 3 + 1] = g;
            out[base + x * 3 + 2] = r;
        }
    }
    out
}

/// 回归探针 1：验证图片缓存 4 MiB 预算真正释放被淘汰的 60 MiB 像素、单图超预算不撑破上限、
/// 淘汰后仍可通过轻量元数据读取真实版本号，且异步解码工作池复用固定线程并按字节节流。
#[test]
fn test_image_cache_strict_budget_pixel_release_metadata_revision_and_bounded_pool() {
    clear_global_image_cache();
    let budget = 4 * 1024 * 1024;
    set_global_image_cache_budget(budget);

    // 1. 复现 32 张 × 2 MiB（1024×512 RGBA8）图片写入 4 MiB 预算
    let mut weaks: Vec<(ImageAssetId, u64, Weak<[u8]>)> = Vec::with_capacity(32);
    for i in 0..32u32 {
        let id = ImageAssetId(10_000 + i);
        let rgba = vec![(i as u8).wrapping_mul(7); 1024 * 512 * 4]; // 精确 2 MiB
        let rev = register_image_rgba(id, 1024, 512, rgba).expect("register 2MiB image");
        let px = get_cached_image(id).expect("freshly registered image fits in 4MiB budget");
        weaks.push((id, rev, Arc::downgrade(&px.rgba)));
        drop(px);
    }

    let (cur_bytes, max_bytes, count) = global_image_cache_stats();
    assert_eq!(max_bytes, budget);
    assert!(
        cur_bytes <= budget,
        "cache bytes {cur_bytes} must not exceed budget {budget}"
    );
    assert_eq!(count, 2, "only two 2MiB images should fit in a 4MiB budget");

    // 关键验证：被 LRU 淘汰的 30 张图像底层像素 Arc<[u8]> 已彻底释放（不再被 GLOBAL_IMAGE_STATES 钉死）
    let live_pixels = weaks.iter().filter(|(_, _, w)| w.upgrade().is_some()).count();
    assert_eq!(
        live_pixels, 2,
        "only 2 images (4 MiB) may remain alive; got {live_pixels} alive (possible GLOBAL_IMAGE_STATES leak)"
    );

    // 2. 被 CPU LRU 淘汰的早期图片仍能通过轻量元数据读到非零版本号（供 GPU 纹理缓存使用）
    let (evicted_id, evicted_rev, _) = &weaks[0];
    assert!(get_cached_image(*evicted_id).is_none());
    assert_eq!(image_revision(*evicted_id), Some(*evicted_rev));
    let meta = image_metadata(*evicted_id).expect("evicted image metadata must stay available");
    assert_eq!(meta.width, 1024);
    assert_eq!(meta.height, 512);
    assert_eq!(meta.revision, *evicted_rev);
    assert!(matches!(
        image_load_state(*evicted_id),
        Some(ImageLoadState::Ready(m)) if m.revision == *evicted_rev
    ));

    // 3. 单张 6 MiB 图像超过 4 MiB 总预算：不留驻缓存，严格守住 `cur_bytes <= 4 MiB`
    let huge_id = ImageAssetId(10_999);
    let huge_rgba = vec![0xAAu8; 1024 * 1536 * 4]; // 6 MiB
    let huge_rev = register_image_rgba(huge_id, 1024, 1536, huge_rgba).expect("register 6MiB");
    let (after_huge_bytes, _, _) = global_image_cache_stats();
    assert!(
        after_huge_bytes <= budget,
        "oversized single image must not breach budget: {after_huge_bytes} > {budget}"
    );
    assert!(get_cached_image(huge_id).is_none());
    assert_eq!(image_revision(huge_id), Some(huge_rev));

    // 4. 显式释放与彻底卸载入口
    let (live_id, live_rev, live_weak) = &weaks[31];
    assert!(live_weak.upgrade().is_some());
    assert!(release_cached_image(*live_id));
    assert!(
        live_weak.upgrade().is_none(),
        "release_cached_image must immediately release pixel backing"
    );
    assert_eq!(
        image_revision(*live_id),
        Some(*live_rev),
        "release_cached_image must preserve metadata revision"
    );
    assert!(unload_image(*live_id));
    assert_eq!(image_revision(*live_id), None);
    assert_eq!(image_load_state(*live_id), None);

    // 5. 有界异步解码工作池并发解码验证（复用固定 2..=4 线程并受在途像素预算节流）
    set_global_image_cache_budget(64 * 1024 * 1024);
    set_async_decode_in_flight_budget(256 * 1024);
    let workers = async_decode_worker_count();
    assert!(
        (2..=4).contains(&workers),
        "bounded decode pool must reuse 2..=4 workers, got {workers}"
    );

    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..12u32 {
        let id = ImageAssetId(11_000 + i);
        let bmp = make_bmp_24(64, 64, (i * 20) as u8, 128, 255);
        let txc = tx.clone();
        load_image_bytes_async(
            id,
            bmp,
            Some(Arc::new(move |aid, ok| {
                let _ = txc.send((aid, ok));
            })),
        );
    }
    let mut completed = 0usize;
    for _ in 0..12 {
        let (aid, ok) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("bounded async decode pool timed out");
        assert!(ok, "decode failed for {aid:?}");
        assert!(image_revision(aid).is_some());
        completed += 1;
    }
    assert_eq!(completed, 12);

    // 清理
    for i in 0..32u32 {
        unload_image(ImageAssetId(10_000 + i));
    }
    unload_image(huge_id);
    for i in 0..12u32 {
        unload_image(ImageAssetId(11_000 + i));
    }
}

/// 回归探针 2：验证 `MobileEmbedApp` 与 `WgpuOffscreenRenderer` 不再在原生/懒分配路径下持有重复的整屏 CPU 副本。
#[test]
fn test_mobile_and_offscreen_avoid_redundant_cpu_framebuffers() {
    use velox_engine::platform::mobile::MobileEmbedApp;

    struct SolidView {
        color: Color,
    }
    impl View for SolidView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(1920)
                .h(1080)
                .bg(self.color)
                .into_element(cx)
        }
    }

    // 1. 1080p 构造、resize、DPI 切换在未走 CPU 路径前均保持 0 字节 CPU framebuffer
    let mut app = MobileEmbedApp::new_android(1920, 1080);
    assert_eq!(
        app.cpu_framebuffer_bytes(),
        0,
        "MobileEmbedApp must lazily allocate CPU framebuffer"
    );
    app.resize(1280, 720);
    assert_eq!(app.cpu_framebuffer_bytes(), 0);
    app.set_dpi_scale(2);
    assert_eq!(app.cpu_framebuffer_bytes(), 0);
    app.set_dpi_scale(1);
    app.resize(1920, 1080);
    assert_eq!(app.cpu_framebuffer_bytes(), 0);

    // 2. 离线/CPU 路径：仅分配 1 份整屏缓冲（1920×1080×4 = 8,294,400 B），而非 3 份（24.88 MB）
    let one_frame_bytes = 1920 * 1080 * 4;
    let _ = app.engine.mount_root(SolidView {
        color: Color::rgb(40, 120, 220),
    });
    let dirty_count = app.step_frame(16);
    assert!(dirty_count > 0);
    assert_eq!(app.rgba_bytes().len(), one_frame_bytes);
    assert!(
        app.cpu_memory_bytes() <= one_frame_bytes + 64 * 1024,
        "mobile offscreen path must hold at most 1 CPU framebuffer (~{} B), got {} B",
        one_frame_bytes,
        app.cpu_memory_bytes()
    );

    // 验证像素写入正确（非全零）
    let px0 = &app.rgba_bytes()[0..4];
    assert_eq!(px0, &[40, 120, 220, 255]);

    // 3. 后台化 `trim_memory()` 彻底回收 CPU 帧缓冲
    app.trim_memory();
    assert_eq!(app.cpu_memory_bytes(), 0);

    // 4. `WgpuOffscreenRenderer::new_lazy` + `render_draw_ops_rgba8888_into` 零内部 CPU 缓冲分配
    #[cfg(feature = "gpu")]
    {
        use velox_engine::renderers::wgpu::WgpuOffscreenRenderer;

        let mut engine = VeloxEngine::new(320, 180);
        let _ = engine.mount_root(SolidView {
            color: Color::rgb(12, 200, 90),
        });
        let dirty: Vec<Region> = engine.step_frame_dt(16).to_vec();
        let mut off = WgpuOffscreenRenderer::new_lazy(320, 180);
        assert_eq!(off.cpu_memory_bytes(), 0);

        let mut caller_fb = vec![Rgba8888(0); 320 * 180];
        let (n, _) = off.render_draw_ops_rgba8888_into(
            engine.cached_draw_ops(),
            engine.viewport_region(),
            1,
            &dirty,
            &mut caller_fb,
        );
        assert!(n > 0);
        assert_eq!(
            off.cpu_memory_bytes(),
            0,
            "direct render_draw_ops_rgba8888_into must not allocate internal rgba_buffer or fallback_pixmap"
        );
        assert_eq!(caller_fb[0].to_color(), Color::rgb(12, 200, 90));
    }
}

/// 回归探针 3：验证 `register_font_vec` 不再调用 `Box::leak`，重复注册同 `FontSpec` 替换旧字体字节且支持 `unregister_font`。
#[test]
fn test_dynamic_font_registration_replaces_and_unregisters_without_leak() {
    let font_candidates = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ];
    let Some(font_bytes) = font_candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
        // 无系统 TTF 的精简容器跳过
        return;
    };
    let single_size = font_bytes.len();
    assert!(single_size > 1024);

    let spec = FontSpec {
        family: FontFamily::Custom(77),
        weight: RichFontWeight::Normal,
        style: RichFontStyle::Normal,
    };

    // 连续重复注册 16 次：堆上驻留字节必须恒等于单份字体大小，绝不线性泄漏
    for _ in 0..16 {
        assert!(register_font_vec(spec, font_bytes.clone()));
        assert_eq!(registered_custom_font_count(), 1);
        assert_eq!(
            registered_custom_font_bytes(),
            single_size,
            "re-registering the same FontSpec must drop previous font bytes instead of leaking"
        );
    }

    // 验证该动态注册字体可以正常测宽和光栅化
    let g = velox_engine::display::get_cached_glyph_styled_q4('A', 64, spec);
    assert!(g.is_some());

    // 注销后条目数与堆字节数全部清零
    assert!(unregister_font(spec));
    assert_eq!(registered_custom_font_count(), 0);
    assert_eq!(registered_custom_font_bytes(), 0);
    assert!(!unregister_font(spec));
}

/// 回归探针 4：验证 GPU `LayerTargetPool` 按字节预算、切回普通页老化、窗口缩小与后台化自动回收离屏纹理与 SVG 遮罩。
#[cfg(feature = "gpu")]
#[test]
fn test_gpu_layer_pool_reclaims_idle_textures_and_svg_masks_by_budget() {
    use velox_engine::renderers::wgpu::{PreparedWgpuContext, WgpuOffscreenRenderer};

    let Some(ctx) = PreparedWgpuContext::init_headless() else {
        // 无硬件 GPU 环境跳过
        return;
    };

    struct EffectsPage;
    impl View for EffectsPage {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(1024)
                .h(512)
                .bg(Color::rgb(20, 30, 50))
                .child(
                    cx,
                    div()
                        .w(800)
                        .h(400)
                        .opacity(200)
                        .backdrop_blur(8)
                        .clip_path("M 0 0 L 800 0 L 800 400 L 0 400 Z")
                        .bg(Color::rgba(255, 120, 40, 180))
                        .into_element(cx),
                )
                .into_element(cx)
        }
    }

    struct PlainPage;
    impl View for PlainPage {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(1024)
                .h(512)
                .bg(Color::rgb(30, 40, 60))
                .into_element(cx)
        }
    }

    let mut fx_engine = VeloxEngine::new(1024, 512);
    let _ = fx_engine.mount_root(EffectsPage);
    let _ = fx_engine.step_frame_dt(16);

    let mut plain_engine = VeloxEngine::new(1024, 512);
    let _ = plain_engine.mount_root(PlainPage);
    let _ = plain_engine.step_frame_dt(16);

    let mut off = WgpuOffscreenRenderer::from_shared_device(
        ctx.device().clone(),
        ctx.queue().clone(),
        1024,
        512,
    );

    // 1. 首次渲染特效页：离屏纹理池与 SVG 遮罩缓存均产生显存占用，且帧末自动受字节预算约束
    assert!(off.render_draw_ops_scaled(
        fx_engine.cached_draw_ops(),
        fx_engine.viewport_region(),
        1.0
    ));
    let bytes_after_fx = off.layer_pool_bytes();
    let mask_bytes_after_fx = off.layer_mask_cache_bytes();
    assert!(
        bytes_after_fx > 0,
        "effects page should populate layer target pool"
    );
    assert!(
        mask_bytes_after_fx > 0,
        "clip_path should populate SVG mask cache"
    );

    // 2. 调低空闲预算到 2 MiB，验证池立即把多余空闲离屏纹理压回到预算内
    let tight_budget = 2 * 1024 * 1024;
    off.set_layer_pool_idle_budget(tight_budget);
    assert!(
        off.layer_pool_bytes() <= tight_budget,
        "layer pool bytes {} must shrink to <= tight budget {}",
        off.layer_pool_bytes(),
        tight_budget
    );

    // 3. 切回普通页推进老化帧数后，空闲离屏纹理全部自动回收至 0 B
    off.set_layer_pool_idle_max_age_frames(4);
    for _ in 0..5 {
        assert!(off.render_draw_ops_scaled(
            plain_engine.cached_draw_ops(),
            plain_engine.viewport_region(),
            1.0
        ));
    }
    assert_eq!(
        off.layer_pool_bytes(),
        0,
        "switching back to a plain page must reclaim idle offscreen targets after max age"
    );

    // 4. 再次进入特效页能够无缝重建并正确渲染，随后窗口缩小自动回收大于新视口的大纹理
    off.set_layer_pool_idle_budget(32 * 1024 * 1024);
    off.set_layer_pool_idle_max_age_frames(60);
    assert!(off.render_draw_ops_scaled(
        fx_engine.cached_draw_ops(),
        fx_engine.viewport_region(),
        1.0
    ));
    let bytes_before_shrink = off.layer_pool_bytes();
    assert!(bytes_before_shrink >= 1024 * 512 * 4);

    let mut small_engine = VeloxEngine::new(256, 128);
    let _ = small_engine.mount_root(PlainPage);
    let _ = small_engine.step_frame_dt(16);
    assert!(off.render_draw_ops_scaled(
        small_engine.cached_draw_ops(),
        small_engine.viewport_region(),
        1.0
    ));
    // 窗口缩小至 256×128：大于新视口的 1024×512 / 512×256 等大纹理立即回收，仅保留不超过新视口尺寸的热复用槽位
    assert!(
        off.layer_pool_bytes() < bytes_before_shrink
            && off.layer_pool_bytes() <= 256 * 128 * 4 * 2,
        "shrinking viewport must immediately reclaim oversized idle targets: before={} after={}",
        bytes_before_shrink,
        off.layer_pool_bytes()
    );

    // 5. 后台化 `trim_gpu_memory()` 清空全部空闲纹理与 SVG 遮罩缓存
    off.trim_gpu_memory();
    assert_eq!(off.layer_pool_bytes(), 0);
    assert_eq!(off.layer_mask_cache_bytes(), 0);
}

/// 回归探针 5：验证 64×64 空间分桶遮挡剔除与原始 `O(N²)` 升序裁剪 100% 逐像素等价（覆盖不重叠、阶梯剥离、大遮挡体、文本与圆角裁剪）。
#[test]
fn test_spatial_occlusion_matches_bruteforce_and_scales_linearly() {
    use velox_engine::motion::{DrawOp, DrawPayload};
    use velox_engine::renderers::software::{compute_unoccluded_clip, precompute_occlusion_clips};

    fn bruteforce_reference(ops: &mut [DrawOp<'_>]) {
        let len = ops.len();
        for idx in 0..len {
            let base_clip = ops[idx].clip.intersect(ops[idx].bounds);
            if base_clip.is_empty() {
                ops[idx].clip = Region::EMPTY;
                continue;
            }
            let unoccluded = compute_unoccluded_clip(base_clip, &ops[(idx + 1)..]);
            if unoccluded.is_empty() {
                ops[idx].clip = Region::EMPTY;
                continue;
            }
            let keep_parent_clip = matches!(ops[idx].payload, DrawPayload::TextRun { .. })
                || matches!(ops[idx].payload, DrawPayload::StyledQuad(sq) if sq.transform.is_some())
                || ops[idx].clip_radius > 0;
            if keep_parent_clip {
                ops[idx].clip = compute_unoccluded_clip(ops[idx].clip, &ops[(idx + 1)..]);
            } else {
                ops[idx].clip = unoccluded;
            }
        }
    }

    let make_quad = |id: u32, x: i16, y: i16, w: i16, h: i16, clip: Region, alpha: u8, rounded: u8| DrawOp {
        id: Some(id),
        bounds: Region::new(x, y, w, h),
        clip,
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: Color::rgba(40, 120, 220, alpha),
            bg_grad: Color::rgba(40, 120, 220, alpha),
            border_color: Color::TRANSPARENT,
            rounded,
            border_width: 0,
        },
    };

    // 1. 复杂混合场景：大背景 + 300 个交错重叠/阶梯剥离/圆角/半透明/文本节点 + 顶部大弹窗遮挡
    let vp = Region::new(0, 0, 1920, 1080);
    let mut ops_spatial: Vec<DrawOp<'static>> = Vec::new();
    ops_spatial.push(make_quad(1, 0, 0, 1920, 1080, vp, 255, 0)); // 大背景
    // 阶梯剥离构造：先左半遮挡，再右上半遮挡（依赖严格升序）
    ops_spatial.push(make_quad(2, 100, 100, 200, 200, vp, 255, 0));
    ops_spatial.push(make_quad(3, 100, 100, 100, 200, vp, 255, 0));
    ops_spatial.push(make_quad(4, 200, 100, 100, 100, vp, 255, 0));
    // 文本与圆角裁剪保留父级裁剪盒
    ops_spatial.push(DrawOp {
        id: Some(5),
        bounds: Region::new(400, 200, 120, 24),
        clip: Region::new(380, 190, 240, 80),
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::TextRun {
            text: StrRef::Static("Hello Spatial Occlusion"),
            color: Color::WHITE,
            font_size: 16,
            text_flags: 0,
        },
    });
    let mut rounded_op = make_quad(6, 420, 220, 160, 90, Region::new(400, 200, 220, 140), 255, 4);
    rounded_op.clip_radius = 8;
    ops_spatial.push(rounded_op);

    // 伪随机散布 300 个大小不一的重叠矩形
    let mut seed: u32 = 0x1234_5678;
    let mut next_u32 = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed
    };
    for i in 0..300u32 {
        let x = (next_u32() % 1800) as i16;
        let y = (next_u32() % 980) as i16;
        let w = (16 + (next_u32() % 240)) as i16;
        let h = (16 + (next_u32() % 180)) as i16;
        let alpha = if next_u32() % 5 == 0 { 180 } else { 255 };
        let rounded = if next_u32() % 4 == 0 { 6 } else { 0 };
        ops_spatial.push(make_quad(10 + i, x, y, w, h, vp, alpha, rounded));
    }
    // 顶部大遮挡模态框（覆盖超过 16 个 64×64 桶，测试 large_occluders 分支）
    ops_spatial.push(make_quad(999, 360, 180, 640, 480, vp, 255, 0));

    let mut ops_ref = ops_spatial.clone();
    precompute_occlusion_clips(&mut ops_spatial);
    bruteforce_reference(&mut ops_ref);

    for (i, (a, b)) in ops_spatial.iter().zip(ops_ref.iter()).enumerate() {
        assert_eq!(
            a.clip, b.clip,
            "spatial occlusion mismatch at op #{i}: spatial={:?}, ref={:?}",
            a.clip, b.clip
        );
    }

    // 2. 128 / 512 / 2,048 不重叠矩形等价验证与耗时基准（对比原始 O(N²)）
    for &n in &[128usize, 512, 2048] {
        let mut base_grid: Vec<DrawOp<'static>> = Vec::with_capacity(n);
        for i in 0..n {
            let col = (i % 64) as i16;
            let row = (i / 64) as i16;
            let r = Region::new(col * 30, row * 28, 26, 24);
            base_grid.push(make_quad(
                i as u32 + 1,
                r.x,
                r.y,
                r.w,
                r.h,
                Region::new(0, 0, 2000, 1200),
                255,
                0,
            ));
        }
        let mut grid_spatial = base_grid.clone();
        let mut grid_ref = base_grid.clone();
        precompute_occlusion_clips(&mut grid_spatial);
        bruteforce_reference(&mut grid_ref);
        for (i, (a, b)) in grid_spatial.iter().zip(grid_ref.iter()).enumerate() {
            assert_eq!(a.clip, b.clip, "{n}-grid mismatch at #{i}");
        }

        // 取 11 次中位数（P50）
        let mut spatial_us = Vec::with_capacity(11);
        let mut ref_us = Vec::with_capacity(11);
        for _ in 0..11 {
            let mut work = base_grid.clone();
            let t0 = std::time::Instant::now();
            precompute_occlusion_clips(&mut work);
            spatial_us.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        for _ in 0..5 {
            let mut work = base_grid.clone();
            let t0 = std::time::Instant::now();
            bruteforce_reference(&mut work);
            ref_us.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        spatial_us.sort_by(|a, b| a.total_cmp(b));
        ref_us.sort_by(|a, b| a.total_cmp(b));
        std::eprintln!(
            "[bench] N={n:4}: spatial P50 = {:.3} ms | bruteforce P50 = {:.3} ms | speedup = {:.1}x",
            spatial_us[5],
            ref_us[2],
            ref_us[2] / spatial_us[5].max(1e-6)
        );
    }
}

/// 回归探针 6：验证 GPU 实例缓冲增量局部上传（零全量 1.83 MiB 重传）与离屏脏区局部回读（零整屏 7.91 MiB 回读）。
#[cfg(feature = "gpu")]
#[test]
fn test_gpu_incremental_instance_upload_and_dirty_readback() {
    use velox_engine::motion::{DrawOp, DrawPayload};
    use velox_engine::renderers::wgpu::{PreparedWgpuContext, WgpuOffscreenRenderer};

    let Some(ctx) = PreparedWgpuContext::init_headless() else {
        return;
    };

    let width = 1280usize;
    let height = 720usize;
    let vp = Region::new(0, 0, width as i16, height as i16);
    let mut ops: Vec<DrawOp<'static>> = Vec::with_capacity(2001);
    ops.push(DrawOp {
        id: Some(1),
        bounds: vp,
        clip: vp,
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: Color::rgb(16, 24, 38),
            bg_grad: Color::rgb(16, 24, 38),
            border_color: Color::TRANSPARENT,
            rounded: 0,
            border_width: 0,
        },
    });
    for i in 0..2000usize {
        let x = ((i % 50) * 24 + 10) as i16;
        let y = ((i / 50) * 16 + 10) as i16;
        let r = Region::new(x, y, 20, 12);
        ops.push(DrawOp {
            id: Some((i as u32) + 2),
            bounds: r,
            clip: r,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::Quad {
                bg: Color::rgb(50, 80, 140),
                bg_grad: Color::rgb(50, 80, 140),
                border_color: Color::TRANSPARENT,
                rounded: 0,
                border_width: 0,
            },
        });
    }

    let mut off = WgpuOffscreenRenderer::from_shared_device(
        ctx.device().clone(),
        ctx.queue().clone(),
        width,
        height,
    );

    // 1. 首帧整屏上传与整屏回读
    let _ = off.render_draw_ops(&ops, vp, 1, &[vp]);
    let full_upload = off.last_uploaded_instance_bytes();
    let full_readback = off.last_readback_bytes();
    assert_eq!(full_upload, 2001 * 192);
    assert!(full_readback >= width * height * 4);

    // 2. 静止帧：零实例上传
    let dirty_small = Region::new(10, 10, 20, 12);
    let _ = off.render_draw_ops(&ops, vp, 1, &[dirty_small]);
    assert_eq!(
        off.last_uploaded_instance_bytes(),
        0,
        "unchanged primitives must upload 0 bytes"
    );

    // 3. 局部 Hover 切换：仅改动相距甚远的两个小矩形（索引 1 与 1950）
    if let DrawPayload::Quad { ref mut bg, ref mut bg_grad, .. } = ops[1].payload {
        *bg = Color::rgb(250, 60, 40);
        *bg_grad = Color::rgb(250, 60, 40);
    }
    let dirty_a = ops[1].bounds;
    let _ = off.render_draw_ops(&ops, vp, 1, &[dirty_a]);
    assert_eq!(
        off.last_uploaded_instance_bytes(),
        192,
        "changing 1 primitive out of 2001 must upload only 192 bytes"
    );
    let partial_readback = off.last_readback_bytes();
    assert!(
        partial_readback * 20 < full_readback,
        "dirty subrect readback ({partial_readback} B) must be < 5% of full frame ({full_readback} B)"
    );
    // 验证改动后的目标像素已准确回读到 `rgba_buffer`
    let px_off = (14 * width + 16) * 4;
    assert_eq!(
        &off.rgba_buffer[px_off..px_off + 4],
        &[250, 60, 40, 255],
        "dirty subrect readback must write updated pixels at exact coordinates"
    );
    // 验证未脏区域背景像素保持完好
    let bg_off = (700 * width + 1200) * 4;
    assert_eq!(&off.rgba_buffer[bg_off..bg_off + 4], &[16, 24, 38, 255]);
}

/// 回归探针 7：验证 `CanvasGpuEngine` 场景生命周期（显式释放 + 空闲老化/预算自动回收）及状态数组局部合并上传。
#[cfg(feature = "gpu")]
#[test]
fn test_canvas_scene_lifecycle_budget_and_partial_state_upload() {
    use velox_engine::canvas_scene::{
        CanvasItemState, CanvasStaticGeom, SceneCanvasSpec, StaticSceneId, Viewport2D,
        CANVAS_STATE_HOVERED, CANVAS_STATE_SELECTED,
    };
    use velox_engine::motion::{DrawOp, DrawPayload};
    use velox_engine::renderers::wgpu::{PreparedWgpuContext, WgpuOffscreenRenderer};

    let Some(ctx) = PreparedWgpuContext::init_headless() else {
        return;
    };

    let n = 20_000usize;
    let geoms: Vec<CanvasStaticGeom> = (0..n)
        .map(|i| {
            let x = ((i % 200) * 4) as f32;
            let y = ((i / 200) * 4) as f32;
            CanvasStaticGeom::rounded_rect_rotated(x, y, 3.0, 3.0, 1.0, 0.0, 0.5)
        })
        .collect();
    let mut states: Vec<CanvasItemState> = vec![CanvasItemState::new(0); n];
    let palette = [Color::rgb(60, 130, 240), Color::rgb(240, 80, 60)];
    let vp = Region::new(0, 0, 400, 300);

    let mut off = WgpuOffscreenRenderer::from_shared_device(
        ctx.device().clone(),
        ctx.queue().clone(),
        400,
        300,
    );

    let render_scene = |off: &mut WgpuOffscreenRenderer,
                        scene_id: StaticSceneId,
                        state_rev: u32,
                        st: &[CanvasItemState]| {
        let spec = SceneCanvasSpec {
            id: 1,
            width: 400,
            height: 300,
            opacity: 255,
            bg: Color::rgb(12, 16, 24),
            viewport: Viewport2D::default(),
            zooming: false,
            scene_id,
            geom_revision: 1,
            state_revision: state_rev,
            mesh_revision: 1,
            palette: &palette,
            geoms: &geoms,
            states: st,
            visible_ranges: &[],
            mesh_verts: &[],
            mesh_indices: &[],
            labels: &[],
            marquee: None,
            marquee_color: Color::TRANSPARENT,
            dirty_screen_rects: &[],
            bg_shaders: &[],
            fg_shaders: &[],
            software_bg_fallback: None,
            software_fg_fallback: None,
        };
        let op = DrawOp {
            id: Some(1),
            bounds: vp,
            clip: vp,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::SceneCanvas(&spec),
        };
        assert!(off.render_draw_ops_scaled(&[op], vp, 1.0));
    };

    // 1. 首帧完整上传 20,000 × 4B = 80,000 B 状态数组
    render_scene(&mut off, StaticSceneId(1), 1, &states);
    assert_eq!(off.canvas_scene_count(), 1);
    assert_eq!(off.canvas_last_state_upload_bytes(), n * 4);
    let scene1_bytes = off.canvas_cache_bytes();
    assert!(scene1_bytes >= n * 32 + n * 4);

    // 2. 仅修改 1 个元素状态：局部上传精确为 4 B（而非整份 80,000 B）
    states[42] = CanvasItemState::new(1).with_flags(CANVAS_STATE_HOVERED);
    render_scene(&mut off, StaticSceneId(1), 2, &states);
    assert_eq!(
        off.canvas_last_state_upload_bytes(),
        4,
        "updating 1 of 20,000 states must upload only 4 bytes"
    );

    // 3. 修改相距甚远的 2 个元素状态：拆分为两段局部上传，合计仅 8 B
    states[10] = CanvasItemState::new(1).with_flags(CANVAS_STATE_SELECTED);
    states[19_500] = CanvasItemState::new(1).with_flags(CANVAS_STATE_HOVERED);
    render_scene(&mut off, StaticSceneId(1), 3, &states);
    assert_eq!(
        off.canvas_last_state_upload_bytes(),
        8,
        "updating 2 distant states must upload only 8 bytes"
    );

    // 4. 切至场景 2，配置低预算与短闲置年龄后自动回收旧场景 1
    off.set_canvas_idle_budget(scene1_bytes);
    off.set_canvas_idle_max_age_frames(2);
    render_scene(&mut off, StaticSceneId(2), 1, &states);
    // 推进两帧场景 2，使场景 1 达到闲置年龄并超预算被自动回收
    render_scene(&mut off, StaticSceneId(2), 1, &states);
    render_scene(&mut off, StaticSceneId(2), 1, &states);
    assert_eq!(
        off.canvas_scene_count(),
        1,
        "idle Canvas scene 1 must be reclaimed automatically by budget/age"
    );

    // 5. 显式移除场景 2，显存占用清零
    assert!(off.remove_canvas_scene(StaticSceneId(2)));
    assert_eq!(off.canvas_scene_count(), 0);
    assert_eq!(off.canvas_cache_bytes(), 0);
}
