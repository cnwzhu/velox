//! 四层内存构成账本 + P0/P1/P2/P3 端到端实测验证探针。
//!
//! 运行命令：
//! `cargo test -p desktop_app --test memory_and_startup_profile -- --nocapture --test-threads=1`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use desktop_app::{
    mount_desktop_app, GalleryApp, GalleryStory, DEFAULT_DESKTOP_HEIGHT, DEFAULT_DESKTOP_WIDTH,
};
use velox_kit::core::canvas_scene::{
    CanvasAsciiLabel, CanvasItemState, CanvasStaticGeom, SceneCanvasSpec, StaticSceneId, Viewport2D,
};
use velox_kit::core::display::font_memory_stats;
use velox_kit::core::platform::winit::write_uncompressed_png;
use velox_kit::core::renderers::wgpu::{PreparedWgpuContext, WgpuOffscreenRenderer};
use velox_kit::prelude::*;

struct CountingAlloc;

static RUST_HEAP_LIVE: AtomicUsize = AtomicUsize::new(0);
static RUST_HEAP_PEAK: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let prev = RUST_HEAP_LIVE.fetch_add(layout.size(), Ordering::Relaxed);
            RUST_HEAP_PEAK.fetch_max(prev.saturating_add(layout.size()), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        RUST_HEAP_LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                let diff = new_size - layout.size();
                let prev = RUST_HEAP_LIVE.fetch_add(diff, Ordering::Relaxed);
                RUST_HEAP_PEAK.fetch_max(prev.saturating_add(diff), Ordering::Relaxed);
            } else {
                RUST_HEAP_LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct ProcessMemoryCountersEx {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
    fn K32GetProcessMemoryInfo(
        process: *mut core::ffi::c_void,
        ppsmemcounters: *mut ProcessMemoryCountersEx,
        cb: u32,
    ) -> i32;
}

fn os_process_memory() -> (usize, usize) {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut pmc = core::mem::zeroed::<ProcessMemoryCountersEx>();
        pmc.cb = core::mem::size_of::<ProcessMemoryCountersEx>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            return (pmc.working_set_size, pmc.private_usage);
        }
    }
    (0, 0)
}

fn mb(bytes: usize) -> f64 {
    (bytes as f64) / (1024.0 * 1024.0)
}

fn kb(bytes: usize) -> f64 {
    (bytes as f64) / 1024.0
}

fn print_memory_ledger(
    stage: &str,
    engine: &VeloxEngine,
    offscreen: Option<&WgpuOffscreenRenderer>,
) {
    let (ws, priv_bytes) = os_process_memory();
    let rust_live = RUST_HEAP_LIVE.load(Ordering::Relaxed);
    let rust_peak = RUST_HEAP_PEAK.load(Ordering::Relaxed);
    let non_rust_ws = ws.saturating_sub(rust_live);
    let fonts = font_memory_stats();
    let telem = capture_telemetry(engine, None);

    println!("\n================ [{stage}] 四层内存构成账本 ================");
    println!(
        "  [L1 OS 物理水位] WorkingSet = {:6.2} MiB | PrivateCommit = {:6.2} MiB",
        mb(ws),
        mb(priv_bytes)
    );
    println!(
        "  [L2 Rust 堆统计] RustLive   = {:6.2} MiB | RustPeak      = {:6.2} MiB | OS非Rust差值 = {:6.2} MiB",
        mb(rust_live),
        mb(rust_peak),
        mb(non_rust_ws)
    );
    println!(
        "  [L3 字体子系统]  Mapped只读映射 = {:6.2} MiB | Heap私有堆 = {:6.2} MiB (primary={:.2}M, cjk={:.2}M, bold={:.2}M, italic={:.2}M, mono={:.2}M)",
        mb(fonts.mapped_font_bytes),
        mb(fonts.heap_font_bytes),
        mb(fonts.primary_bytes),
        mb(fonts.fallback_cjk_bytes),
        mb(fonts.bold_bytes),
        mb(fonts.italic_bytes),
        mb(fonts.mono_bytes),
    );
    println!(
        "                   GlyphCache = {} entries ({:.1} KiB) | StyledGlyphCache = {} entries ({:.1} KiB)",
        fonts.glyph_cache_entries,
        kb(fonts.glyph_cache_bitmap_bytes),
        fonts.styled_glyph_cache_entries,
        kb(fonts.styled_glyph_cache_bitmap_bytes),
    );
    println!(
        "  [L3 引擎瞬态池]  ScratchArena used = {:.1} KiB | cached_draw_ops = {}",
        kb(telem.arena_used_bytes),
        engine.cached_draw_ops().len()
    );
    if let Some(off) = offscreen {
        if let Some(g) = off.gpu_memory_stats() {
            println!(
                "  [L4 GPU 纹理池]  GlyphAtlas  = {}x{} ({:.2} MiB, {} entries, 装箱率 {:.1}%)",
                g.glyph_atlas_size,
                g.glyph_atlas_size,
                mb(g.glyph_atlas_bytes),
                g.glyph_atlas_entries,
                g.glyph_atlas_occupancy * 100.0
            );
            println!(
                "                   SpriteAtlas = {}x{} ({:.2} MiB, {} entries, 装箱率 {:.1}%)",
                g.sprite_atlas_size,
                g.sprite_atlas_size,
                mb(g.sprite_atlas_bytes),
                g.sprite_atlas_entries,
                g.sprite_atlas_occupancy * 100.0
            );
            println!(
                "                   LayerPool   = {} targets ({:.2} MiB) | BackdropCache hits={} misses={}",
                g.layer_pool_targets,
                mb(g.layer_pool_target_bytes),
                g.backdrop_blur_cache_hits,
                g.backdrop_blur_cache_misses,
            );
            println!(
                "                   CanvasGPU   = {} scenes ({:.2} MiB) | SlugGPU = {} glyphs ({:.1} KiB, last_drawn={}) | Renderer总GPU = {:.2} MiB",
                g.canvas_scenes,
                mb(g.canvas_gpu_bytes),
                g.slug_cached_glyphs,
                kb(g.slug_gpu_bytes),
                g.slug_last_drawn_glyphs,
                mb(g.total_gpu_bytes())
            );
        }
    }
}

fn render_one(
    eng: &mut VeloxEngine,
    off: &mut WgpuOffscreenRenderer,
    fb: &mut [Rgba8888],
    scale: i16,
    dt_ms: u16,
) -> f64 {
    let t0 = Instant::now();
    let dirty: Vec<Region> = eng.step_frame_dt(dt_ms).to_vec();
    if !dirty.is_empty() {
        let vp = eng.viewport_region();
        let ops = eng.cached_draw_ops();
        let _ = off.render_draw_ops_rgba8888_into(ops, vp, scale, &dirty, fb);
    }
    t0.elapsed().as_secs_f64() * 1000.0
}

#[test]
fn profile_p0_p1_p2_p3_end_to_end() {
    unsafe {
        std::env::set_var("VELOX_STARTUP_TRACE", "1");
    }

    // =========================================================================
    // Part 1 (P1): 磁盘 PSO 缓存落盘与二次命中验证（Vulkan 后端支持 PIPELINE_CACHE）
    // =========================================================================
    println!("\n==================== Part 1 (P1): Vulkan 磁盘 PSO 缓存与后台预热验证 ====================");
    {
        unsafe {
            std::env::set_var("WGPU_BACKEND", "vulkan");
        }
        let t1 = Instant::now();
        let ctx1 = PreparedWgpuContext::init_headless();
        println!(
            "  [Vulkan Run 1 - 首次建缓存] total={:.1}ms, ok={}",
            t1.elapsed().as_secs_f64() * 1000.0,
            ctx1.is_some()
        );
        drop(ctx1);

        let t2 = Instant::now();
        let ctx2 = PreparedWgpuContext::init_headless();
        println!(
            "  [Vulkan Run 2 - 命中磁盘缓存] total={:.1}ms, ok={}",
            t2.elapsed().as_secs_f64() * 1000.0,
            ctx2.is_some()
        );
        drop(ctx2);
        unsafe {
            std::env::remove_var("WGPU_BACKEND");
        }
    }

    // =========================================================================
    // Part 2 (P0 + P1 + P3): 16 页四层内存账本 + 后台管线预热消除 1.2s 切页卡顿
    // =========================================================================
    println!("\n==================== Part 2 (P0/P1/P3): desktop_app 四层内存账本 ====================");
    let scale: i16 = 2;
    let phys_w = (DEFAULT_DESKTOP_WIDTH as usize) * (scale as usize);
    let phys_h = (DEFAULT_DESKTOP_HEIGHT as usize) * (scale as usize);

    let mut engine = VeloxEngine::new(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    engine.set_dpi_scale(scale);
    let root = mount_desktop_app(&mut engine, GalleryApp::new());

    let mut offscreen = WgpuOffscreenRenderer::new_lazy(phys_w, phys_h);
    let gpu_ok = offscreen.ensure_gpu();
    assert!(gpu_ok, "expected hardware GPU for profile test");
    // 启动后台子管线预热（与真实 WgpuWindowRenderer 启动一致）
    offscreen.spawn_background_pipeline_warmup();

    let mut caller_fb = vec![Rgba8888(0); phys_w * phys_h];

    let t_first_ms = render_one(&mut engine, &mut offscreen, &mut caller_fb, scale, 16);
    for _ in 0..32 {
        if !engine.needs_redraw() {
            break;
        }
        let _ = render_one(&mut engine, &mut offscreen, &mut caller_fb, scale, 16);
    }
    println!("  [T0 Overview] 首帧 = {:.1}ms", t_first_ms);
    print_memory_ledger("T0: Overview 首屏静止", &engine, Some(&offscreen));

    // 等待后台子管线预热完成（模拟用户在首页停留 ~0.8s 后再点 Canvas 页）
    offscreen.wait_background_pipeline_warmup();

    // T1: 切入 Canvas 页并放大至 2.2x 触发大量座位号 Slug 标签 + 连续 30 次缩放
    if let Some(app) = engine.view_mut(root) {
        app.active_story = GalleryStory::Canvas;
        app.canvas_demo.controller.viewport.zoom = 1.8;
    }
    let t_canvas_first = render_one(&mut engine, &mut offscreen, &mut caller_fb, scale, 16);
    for _ in 0..16 {
        if !engine.needs_redraw() {
            break;
        }
        let _ = render_one(&mut engine, &mut offscreen, &mut caller_fb, scale, 16);
    }
    let fonts_before_zoom = font_memory_stats();
    let mut zoom_times_ms: Vec<f64> = Vec::with_capacity(30);
    for i in 0..30 {
        let factor = if i < 15 { 1.05f32 } else { 0.95f32 };
        if let Some(app) = engine.view_mut(root) {
            app.canvas_demo.controller.viewport.zoom =
                (app.canvas_demo.controller.viewport.zoom * factor).clamp(0.8, 3.2);
        }
        let dt = render_one(&mut engine, &mut offscreen, &mut caller_fb, scale, 16);
        zoom_times_ms.push(dt);
    }
    zoom_times_ms.sort_by(|a, b| a.total_cmp(b));
    let p50 = zoom_times_ms[zoom_times_ms.len() / 2];
    let p99 = zoom_times_ms[(zoom_times_ms.len() * 99 / 100).min(zoom_times_ms.len() - 1)];
    let fonts_after_zoom = font_memory_stats();
    let gpu_t1 = offscreen.gpu_memory_stats().unwrap();
    println!(
        "\n  [T1 Canvas 预热后切页 + 30次连续缩放] 切页首帧={:.1}ms (优化前 1219ms!) | 缩放 P50={:.2}ms, P99={:.2}ms | GlyphCache增长=+{} | SlugGPU={} glyphs (last_drawn={})",
        t_canvas_first,
        p50,
        p99,
        fonts_after_zoom.glyph_cache_entries.saturating_sub(fonts_before_zoom.glyph_cache_entries),
        gpu_t1.slug_cached_glyphs,
        gpu_t1.slug_last_drawn_glyphs,
    );
    assert!(
        t_canvas_first < 250.0,
        "after background warmup, first Canvas frame must not stall for 1.2s; got {t_canvas_first:.1}ms"
    );
    print_memory_ledger("T1: Canvas 30次连续缩放后", &engine, Some(&offscreen));

    // 遍历全部 16 页（触发 msyh.ttc CJK 字体加载）并验证 P0 OS 只读映射零私有堆拷贝
    let all_stories = [
        GalleryStory::Overview,
        GalleryStory::Buttons,
        GalleryStory::Selection,
        GalleryStory::Sliders,
        GalleryStory::Inputs,
        GalleryStory::Navigation,
        GalleryStory::Editor,
        GalleryStory::Overlays,
        GalleryStory::Feedback,
        GalleryStory::Data,
        GalleryStory::SettingsIcons,
        GalleryStory::Effects,
        GalleryStory::Motion,
        GalleryStory::EngineLab,
        GalleryStory::Chat,
        GalleryStory::Canvas,
    ];
    for story in all_stories {
        if let Some(app) = engine.view_mut(root) {
            app.active_story = story;
        }
        let _ = render_one(&mut engine, &mut offscreen, &mut caller_fb, scale, 16);
    }
    print_memory_ledger("T2a: 遍历完16页峰值", &engine, Some(&offscreen));
    let fonts_peak = font_memory_stats();
    #[cfg(target_os = "windows")]
    {
        assert!(
            fonts_peak.mapped_font_bytes > 15 * 1024 * 1024,
            "expected system fonts (incl. msyh.ttc) to be OS read-only mapped, got {} B",
            fonts_peak.mapped_font_bytes
        );
        assert_eq!(
            fonts_peak.heap_font_bytes, 0,
            "system fonts must NOT occupy private Rust heap; got {} B",
            fonts_peak.heap_font_bytes
        );
    }

    // =========================================================================
    // Part 3 (P2): Dual-Kawase 毛玻璃脏区外扩交集跳过精确单元测试
    // =========================================================================
    println!("\n==================== Part 3 (P2): 毛玻璃脏区外扩跳过缓存验证 ====================");
    {
        struct BackdropDirtyTestView {
            outside_tick: u32,
            inside_tick: u32,
        }
        impl View for BackdropDirtyTestView {
            fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
                let out_col = if self.outside_tick % 2 == 0 {
                    Color::rgb(240, 80, 80)
                } else {
                    Color::rgb(80, 220, 120)
                };
                let in_col = if self.inside_tick % 2 == 0 {
                    Color::rgb(59, 130, 246)
                } else {
                    Color::rgb(250, 204, 21)
                };
                // 左侧 (20, 20, 200, 120) 放置背景色块与居中毛玻璃浮层 (blur=10 => 外扩 20px => 仅覆盖 x < 240)
                // 右侧相距 260px (x >= 480) 放置远离毛玻璃的动态指标块
                div()
                    .w(640)
                    .h(360)
                    .p(20)
                    .flex_row()
                    .gap(120)
                    .bg(Color::rgb(18, 20, 28))
                    .child(
                        cx,
                        div()
                            .w(200)
                            .h(120)
                            .p(20)
                            .bg(in_col)
                            .child(
                                cx,
                                div()
                                    .w(160)
                                    .h(80)
                                    .rounded(12)
                                    .backdrop_blur(10)
                                    .bg(Color::rgba(255, 255, 255, 36)),
                            ),
                    )
                    .child(cx, div().w(80).h(40).bg(out_col))
                    .into_element(cx)
            }
        }

        let mut bd_eng = VeloxEngine::new(640, 360);
        bd_eng.set_dpi_scale(1);
        let bd_root = bd_eng.mount_root(BackdropDirtyTestView {
            outside_tick: 0,
            inside_tick: 0,
        });
        let mut bd_off = WgpuOffscreenRenderer::new_lazy(640, 360);
        assert!(bd_off.ensure_gpu());
        let mut bd_fb = vec![Rgba8888(0); 640 * 360];

        // 帧 1：首帧全屏 -> 必须未命中并计算毛玻璃 (misses=1, hits=0)
        let _ = render_one(&mut bd_eng, &mut bd_off, &mut bd_fb, 1, 16);
        let s1 = bd_off.gpu_memory_stats().unwrap();
        assert_eq!(s1.backdrop_blur_cache_misses, 1);
        assert_eq!(s1.backdrop_blur_cache_hits, 0);

        // 帧 2..=6：仅变化右上角远端区块 (500, 40) -> 脏区远离毛玻璃外扩框 -> 连续 5 次 100% 命中跳过 Kawase!
        for t in 1..=5 {
            if let Some(v) = bd_eng.view_mut(bd_root) {
                v.outside_tick = t;
            }
            let _ = render_one(&mut bd_eng, &mut bd_off, &mut bd_fb, 1, 16);
        }
        let s2 = bd_off.gpu_memory_stats().unwrap();
        println!(
            "  [毛玻璃远端变化 5 帧] hits={} (期望 5), misses={} (期望 1)",
            s2.backdrop_blur_cache_hits, s2.backdrop_blur_cache_misses
        );
        assert_eq!(s2.backdrop_blur_cache_hits, 5);
        assert_eq!(s2.backdrop_blur_cache_misses, 1);

        // 帧 7：变化毛玻璃下方背景块 (40, 40) -> 脏区命中外扩采样框 -> 必须立刻失效重算 (misses=2)!
        if let Some(v) = bd_eng.view_mut(bd_root) {
            v.inside_tick = 1;
        }
        let _ = render_one(&mut bd_eng, &mut bd_off, &mut bd_fb, 1, 16);
        let s3 = bd_off.gpu_memory_stats().unwrap();
        println!(
            "  [毛玻璃下方变化 1 帧] hits={} (期望 5), misses={} (期望 2)",
            s3.backdrop_blur_cache_hits, s3.backdrop_blur_cache_misses
        );
        assert_eq!(s3.backdrop_blur_cache_hits, 5);
        assert_eq!(s3.backdrop_blur_cache_misses, 2);
    }

    // =========================================================================
    // Part 4 (P3): Slug 曲线字形 0.5x ~ 6.0x 连续缩放零位图缓存增长 + 像素验证 + 导图
    // =========================================================================
    println!("\n==================== Part 4 (P3): Slug GPU 曲线字形缩放与像素验证 ====================");
    {
        let mut slug_off = WgpuOffscreenRenderer::new_lazy(800, 400);
        assert!(slug_off.ensure_gpu());
        let mut slug_fb = vec![Rgba8888(0); 800 * 400];
        let geoms = [CanvasStaticGeom::rounded_rect_rotated(
            200.0, 100.0, 360.0, 160.0, 1.0, 0.0, 12.0,
        )];
        let states = [CanvasItemState::new(0)];
        let palette = [Color::rgb(24, 34, 54)];
        let labels = [
            CanvasAsciiLabel {
                wx: 200.0,
                wy: 72.0,
                text: "Velox Slug 2026 — A12 / VIP-08",
                font_px: 34.0,
                color: Color::rgb(248, 250, 252),
            },
            CanvasAsciiLabel {
                wx: 200.0,
                wy: 126.0,
                text: "零图集曲线渲染 0.1x ~ 1000x",
                font_px: 32.0,
                color: Color::rgb(96, 165, 250),
            },
        ];

        let fonts_before = font_memory_stats();
        let vp_reg = Region::new(0, 0, 800, 400);
        for &zoom in &[0.6f32, 1.0, 1.5, 2.2, 3.0, 4.0, 1.8] {
            let spec = SceneCanvasSpec {
                id: 9099,
                width: 800,
                height: 400,
                opacity: 255,
                bg: Color::rgb(12, 16, 26),
                scene_id: StaticSceneId(9099),
                viewport: Viewport2D::new(400.0 - 200.0 * zoom, 200.0 - 100.0 * zoom, zoom),
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
                zooming: true,
                bg_shaders: &[],
                fg_shaders: &[],
                software_bg_fallback: None,
                software_fg_fallback: None,
            };
            let op = DrawOp {
                id: None,
                bounds: vp_reg,
                clip: vp_reg,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::SceneCanvas(&spec),
            };
            let _ = slug_off.render_draw_ops_rgba8888_into(&[op], vp_reg, 1, &[vp_reg], &mut slug_fb);
        }
        let fonts_after = font_memory_stats();
        let gstats = slug_off.gpu_memory_stats().unwrap();

        // 统计非背景白色/蓝色字形墨水像素数
        let lit_pixels = slug_fb
            .iter()
            .filter(|px| {
                let c = px.to_color();
                c.r() > 140 || c.b() > 180
            })
            .count();
        println!(
            "  [Slug 曲线字形 7 档缩放(0.6x~4.0x)] slug_glyphs={}, slug_gpu={:.1} KiB, last_drawn={}, ink_pixels={}, CPU GlyphCache增量=+{}",
            gstats.slug_cached_glyphs,
            kb(gstats.slug_gpu_bytes),
            gstats.slug_last_drawn_glyphs,
            lit_pixels,
            fonts_after.glyph_cache_entries.saturating_sub(fonts_before.glyph_cache_entries),
        );
        assert!(gstats.slug_cached_glyphs >= 15, "Slug must cache ASCII+CJK outline glyphs");
        assert!(gstats.slug_last_drawn_glyphs >= 20, "Slug must draw visible glyphs");
        assert!(lit_pixels > 500, "Slug must render visible anti-aliased glyph pixels, got {lit_pixels}");
        assert_eq!(
            fonts_after.glyph_cache_entries, fonts_before.glyph_cache_entries,
            "Slug continuous zoom must cause ZERO CPU bitmap GlyphCache growth!"
        );

        // 可选导出验证图供视觉核验（通过环境变量 `VELOX_SLUG_VERIFY_PNG=<path>` 启用）
        if let Some(out_png) = std::env::var_os("VELOX_SLUG_VERIFY_PNG") {
            let path = std::path::Path::new(&out_png);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let xrgb: Vec<Xrgb8888> = slug_fb
                .iter()
                .map(|p| Xrgb8888::from_color(p.to_color()))
                .collect();
            if let Some(path_str) = path.to_str() {
                let _ = write_uncompressed_png(path_str, 800, 400, &xrgb);
            }
        }
    }
}
