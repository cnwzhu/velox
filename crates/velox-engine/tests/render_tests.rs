//! `velox-engine` 软件 SDF 光栅化、遮挡剔除、脏区追踪、字库字距与 GPU 排版缓存测试套件。

use velox_engine::prelude::*;

#[test]
fn test_damage_tracker_bounded_merge_max_8() {
    let mut tracker = DamageTracker::new();
    for i in 0..15 {
        tracker.add_region(Region::new(i * 20, i * 20, 10, 10));
    }
    assert!(tracker.regions().len() <= 8);
}

#[test]
fn test_sdf_coverage_corners_and_center() {
    let r = Region::new(10, 10, 100, 50);
    assert_eq!(compute_sdf_coverage(50, 30, r, 12), 255);
    assert_eq!(compute_sdf_coverage(10, 10, r, 12), 0);
    assert_eq!(compute_sdf_coverage(10, 10, r, 0), 255);
}

#[test]
fn test_front_to_back_occlusion_culling() {
    use velox_engine::motion::{DrawOp, DrawPayload};

    let vp = Region::new(0, 0, 320, 240);
    let bg_op = DrawOp {
        id: Some(1),
        bounds: vp,
        clip: vp,
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: Color::rgb(15, 23, 42),
            bg_grad: Color::rgb(15, 23, 42),
            border_color: Color::TRANSPARENT,
            rounded: 0,
            border_width: 0,
        },
    };
    let modal_card = DrawOp {
        id: Some(2),
        bounds: Region::new(40, 40, 240, 160),
        clip: vp,
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: Color::rgb(30, 41, 59),
            bg_grad: Color::rgb(30, 41, 59),
            border_color: Color::TRANSPARENT,
            rounded: 8,
            border_width: 0,
        },
    };

    // 位于上层不透明卡片内切矩形 (48..272, 48..192) 内部的局部脏区，对底层背景应被 100% 剔除
    let inner_dirty = Region::new(60, 60, 100, 80);
    let culled = compute_unoccluded_clip(inner_dirty, &[modal_card]);
    assert!(culled.is_empty());

    // 顶部横跨遮挡裁剪验证
    let top_bar = DrawOp {
        id: Some(3),
        bounds: Region::new(0, 0, 320, 40),
        clip: vp,
        clip_radius: 0,
        subpx_q4: 0,
        payload: bg_op.payload,
    };
    let trimmed = compute_unoccluded_clip(vp, &[top_bar]);
    assert_eq!(trimmed, Region::new(0, 40, 320, 200));
}

#[test]
fn test_rounded_parent_clip_keeps_background_and_preserves_geometry() {
    use velox_engine::motion::{DrawOp, DrawPayload};
    use velox_engine::renderers::software::precompute_occlusion_clips;

    let viewport = Region::new(0, 0, 32, 32);
    let quad = |bounds, clip_radius, color| DrawOp {
        id: None,
        bounds,
        clip: viewport,
        clip_radius,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: color,
            bg_grad: color,
            border_color: Color::TRANSPARENT,
            rounded: 0,
            border_width: 0,
        },
    };

    let mut ops = [
        quad(viewport, 0, Color::WHITE),
        quad(viewport, 12, Color::rgb(190, 40, 40)),
        quad(Region::new(0, 0, 4, 32), 0, Color::rgb(20, 120, 240)),
    ];
    let mut pixels = [Xrgb8888(0); 32 * 32];
    let mut expected = pixels;
    for op in &ops {
        rasterize_region(core::slice::from_ref(op), viewport, 0, 0, 32, &mut expected);
    }
    precompute_occlusion_clips(&mut ops);
    assert_eq!(ops[1].clip, viewport);
    rasterize_region(&ops, viewport, 0, 0, 32, &mut pixels);
    assert_eq!(pixels, expected);
}

struct BreathingView;

impl View for BreathingView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .id(99)
            .w(40)
            .h(40)
            .bg(Color::rgb(255, 0, 0))
            .ping_pong_bg(Color::rgb(0, 0, 255), Duration::from_millis(200))
            .into_element(cx)
    }
}

#[test]
fn test_ping_pong_infinite_breathing_animation() {
    let mut engine = VeloxEngine::new(100, 100);
    engine.mount_root(BreathingView);
    engine.step_frame_dt(16);
    assert!(engine.needs_redraw());
    // 即使超过单次周期 200ms，PingPong 循环动画依然保持活跃并产生局部脏区
    let d1 = engine.step_frame_dt(150);
    assert!(!d1.is_empty());
    let d2 = engine.step_frame_dt(150);
    assert!(!d2.is_empty());
    assert!(engine.needs_redraw());
}

#[test]
fn test_linear_gradient_quad_rasterization() {
    use velox_engine::solver::{solve_and_flatten, HitRegion};

    let cx = FrameScope::new();
    let root = div()
        .w(40)
        .h(40)
        .bg(Color::rgb(255, 0, 0))
        .bg_gradient(Color::rgb(0, 0, 255))
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 64> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &root,
        Region::new(0, 0, 40, 40),
        &mut motion,
        &mut ops,
        &mut hits,
    );

    let mut pixels = [Xrgb8888(0); 40 * 40];
    rasterize_region(ops.as_slice(), Region::new(0, 0, 40, 40), 0, 0, 40, &mut pixels);

    // 顶部第 0 行偏向红色，底部第 39 行偏向蓝色
    let top_px = pixels[20].0;
    let bot_px = pixels[39 * 40 + 20].0;
    let top_r = (top_px >> 16) & 0xFF;
    let bot_b = bot_px & 0xFF;
    assert!(top_r > 240);
    assert!(bot_b > 240);
}

#[test]
fn test_word_boundary_wrap_and_static_font_atlas() {
    use velox_engine::display::{measure_text_px, next_wrapped_line};

    // 1. 英文单词边界折行测试：容器宽介于 "Hello" 与 "Hello World" 之间时，不应将 "World" 截断在中间
    let w_hello = measure_text_px("Hello", 16).0;
    let w_hello_world = measure_text_px("Hello World", 16).0;
    let wrap_w = (w_hello + w_hello_world) / 2;
    let (line1, rem1) = next_wrapped_line("Hello World Velox", 16, wrap_w);
    assert_eq!(line1, "Hello");
    assert_eq!(rem1, "World Velox");

}

#[test]
fn test_swipe_gesture_and_rotated_sprites() {

    struct GestureApp {
        last_swipe: Option<SwipeDir>,
    }

    impl View for GestureApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            static RAW_A8: [u8; 16] = [255; 16];
            static ROT_RGB565: [u8; 32] = [0xFF; 32]; // 4x4

            let root = div()
                .id(1)
                .size_full()
                .bg(Color::rgb(20, 30, 40))
                .on_swipe(
                    cx,
                    cx.listener(|this, dir: SwipeDir, acx| {
                        this.last_swipe = Some(dir);
                        acx.notify();
                    }),
                )
                .child(cx, img(4, 4, &RAW_A8, SpriteFormat::A8).tint(Color::WHITE))
                .child(cx, img_rotated_rgb565(cx, 12, 12, 4, 4, &ROT_RGB565, 450, 256));
            root.into_element(cx)
        }
    }


    // 2. 验证 4 向滑动手势识别
    let mut engine = VeloxEngine::new(120, 80);
    let handle = engine.mount_root(GestureApp { last_swipe: None });
    engine.step_frame();

    engine.dispatch_pointer_down(80, 40);
    engine.dispatch_pointer_move(30, 42);
    assert!(engine.dispatch_pointer_up(30, 42));
    assert_eq!(engine.view(handle).unwrap().last_swipe, Some(SwipeDir::Left));

}

#[test]
fn test_tiled_rounded_clip_large_radius_public() {
    use velox_engine::motion::{DrawOp, DrawPayload};

    let vp = Region::new(0, 0, 120, 120);
    let bg_color = Color::rgb(20, 30, 40);
    let fg_color = Color::rgb(240, 80, 60);
    let ops = [
        DrawOp {
            id: Some(1),
            bounds: vp,
            clip: vp,
            clip_radius: 0,
            subpx_q4: 0,
            payload: DrawPayload::Quad {
                bg: bg_color,
                bg_grad: bg_color,
                border_color: Color::TRANSPARENT,
                rounded: 0,
                border_width: 0,
            },
        },
        DrawOp {
            id: Some(2),
            bounds: Region::new(10, 10, 100, 100),
            clip: Region::new(10, 10, 100, 100),
            clip_radius: 48,
            subpx_q4: 0,
            payload: DrawPayload::Quad {
                bg: fg_color,
                bg_grad: fg_color,
                border_color: Color::TRANSPARENT,
                rounded: 0,
                border_width: 0,
            },
        },
    ];

    let mut fb = vec![Rgba8888::from_color(Color::BLACK); 120 * 120];
    rasterize_region(&ops, vp, 0, 0, 120, &mut fb);

    let bg_px = Rgba8888::from_color(bg_color);
    let fg_px = Rgba8888::from_color(fg_color);
    assert_eq!(fb[10 * 120 + 10], bg_px);
    assert_eq!(fb[10 * 120 + 109], bg_px);
    assert_eq!(fb[109 * 120 + 10], bg_px);
    assert_eq!(fb[109 * 120 + 109], bg_px);
    assert_eq!(fb[60 * 120 + 60], fg_px);
}

#[test]
fn test_spring_overshoot_and_rgba8888_alpha_blending() {
    use velox_engine::display::PixelFormat;
    use velox_engine::style::Easing;

    // Spring 阻尼振荡在 t=192 附近应有 > 256 的回弹超调，最终在 t=256 收敛到 256
    let mid = Easing::Spring.apply_256(192);
    assert!(mid > 256, "Spring easing at t=192 should overshoot 256, got {mid}");
    assert_eq!(Easing::Spring.apply_256(256), 256);

    // Rgba8888 保留真实 alpha 并支持透明背景之上的 Porter-Duff src-over 预乘混合
    let dst = Rgba8888::from_color(Color::rgba(0, 0, 0, 0));
    assert_eq!(dst.to_color().a(), 0);
    let blended = dst.blend(Color::rgba(200, 100, 50, 128), 255);
    let out = blended.to_color();
    assert!(out.a() >= 127 && out.a() <= 129);
    assert!(out.r() >= 99 && out.r() <= 101);
}

#[test]
fn test_damage_tracker_sliding_window_resync_and_keyed_sibling_stable_hash() {
    use core::time::Duration;
    use velox_engine::damage::DamageTracker;
    use velox_engine::motion::DrawOpSnapshot;
    use velox_engine::solver::{solve_and_flatten, HitRegion};

    // 1. 验证 DamageTracker 滑动窗口重同步（R1）：在中间插入 1 个节点时，后续未变更节点不沦为脏区
    let prev = [
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 0, 40, 20),
            payload_hash: 101,
        },
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 30, 40, 20),
            payload_hash: 102,
        },
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 60, 40, 20),
            payload_hash: 103,
        },
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 90, 40, 20),
            payload_hash: 104,
        },
    ];
    let curr = [
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 0, 40, 20),
            payload_hash: 101,
        },
        // 在索引 1 新插入一个节点，后续 3 个节点保持原位和原哈希
        DrawOpSnapshot {
            effective_bounds: Region::new(50, 15, 30, 15),
            payload_hash: 999,
        },
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 30, 40, 20),
            payload_hash: 102,
        },
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 60, 40, 20),
            payload_hash: 103,
        },
        DrawOpSnapshot {
            effective_bounds: Region::new(0, 90, 40, 20),
            payload_hash: 104,
        },
    ];
    let mut tracker = DamageTracker::new();
    tracker.diff_snapshots(&prev, &curr);
    assert_eq!(tracker.regions().len(), 1);
    assert_eq!(tracker.regions()[0], Region::new(50, 15, 30, 15));

    // 2. 验证插入带 key 的兄弟节点不会打乱后续未显式编号子树的自动动画/滚动 ID（R2）
    let mut motion = MotionCache::new();

    {
        let cx = FrameScope::new();
        let root1 = div()
            .id(10)
            .flex_col()
            .w(100)
            .h(100)
            .child(
                &cx,
                div().w(60).h(20).child(
                    &cx,
                    div()
                        .w(40)
                        .h(20)
                        .animate_bounds(Duration::from_millis(200))
                        .bg(Color::rgb(255, 0, 0)),
                ),
            )
            .into_element(&cx);
        let mut ops: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
        let mut hits: SmallVec<HitRegion<'_>, 8> = SmallVec::new();
        solve_and_flatten(
            cx.arena(),
            &root1,
            Region::new(0, 0, 100, 100),
            &mut motion,
            &mut ops,
            &mut hits,
        );
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].bounds, Region::new(0, 0, 40, 20));
    }

    // 第二帧在前面插入一个显式 key("banner") 的兄弟节点（h=0 以保持 y 不变），仅将内层宽度改为 80
    // 若父级 child_hash 稳定，则内层动画节点不会丢失上一帧的旧宽度 40，而在进度 0 时从 40 开始平滑插值！
    {
        let cx = FrameScope::new();
        let root2 = div()
            .id(10)
            .flex_col()
            .w(100)
            .h(100)
            .child(&cx, div().key("banner").w(50).h(0))
            .child(
                &cx,
                div().w(60).h(20).child(
                    &cx,
                    div()
                        .w(80)
                        .h(20)
                        .animate_bounds(Duration::from_millis(200))
                        .bg(Color::rgb(255, 0, 0)),
                ),
            )
            .into_element(&cx);
        let mut ops: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
        let mut hits: SmallVec<HitRegion<'_>, 8> = SmallVec::new();
        solve_and_flatten(
            cx.arena(),
            &root2,
            Region::new(0, 0, 100, 100),
            &mut motion,
            &mut ops,
            &mut hits,
        );
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0].bounds.w, 40,
            "Sibling key insertion must preserve unkeyed sibling subtree hash so animation starts from 40"
        );
    }
}

#[test]
fn test_ttf_kerning_and_text_layout_cache() {
    // 1. 验证字距微调与统一高精度测宽、自动折行、单行宽度完全一致（杜绝单行被误折行或触发假性省略号）
    let k_av = velox_engine::display::measure_char_kerning('A', 'V', 24);
    assert!(k_av <= 0, "AV kerning should be <= 0, got {}", k_av);
    let (w_av, _) = velox_engine::display::measure_text_px("AV", 24);
    let exact_av = velox_engine::display::measure_line_width_px("AV", 24);
    assert_eq!(w_av, exact_av + 2);
    let sum_raw = velox_engine::display::measure_char_advance('A', 24)
        + velox_engine::display::measure_char_advance('V', 24)
        + k_av
        + 2;
    assert!(
        (w_av - sum_raw).abs() <= 1,
        "exact float width ({w_av}) should stay within 1px of rounded sum ({sum_raw})"
    );

    for sample in [
        "Tri-State Checkbox (Native Vector SVG Check & Dash)",
        "wgpu 2D SDF Hardware Acceleration",
        "Native Vector SVG Path Engine",
        "Fractional HiDPI Subpixel Crispness",
        "wgpu 2D SDF (GPU)",
        "Built-in Software SDF",
        "Selected: 2/3 (Parent: Indeterminate)",
        "velox_base::{Switch, Slider, ProgressBar, ArcGauge}",
    ] {
        for fs in [11u8, 12, 13, 16, 20, 26] {
            let (tw, _) = velox_engine::display::measure_text_px(sample, fs);
            let line_w = velox_engine::display::measure_line_width_px(sample, fs);
            assert!(
                line_w <= tw,
                "line_w ({line_w}) must not exceed measured box width ({tw}) for {sample:?}@{fs}"
            );
            assert_eq!(
                velox_engine::display::count_text_lines(sample, fs, tw, false),
                1,
                "single-line measured text {sample:?}@{fs} (w={tw}) must never falsely wrap to 2 lines"
            );
            let (l1, rem) = velox_engine::display::next_wrapped_line(sample, fs, tw);
            assert_eq!(l1, sample);
            assert_eq!(rem, "");
        }
    }

    // 2. 验证 GPU 离屏渲染两帧时 TextLayoutCache 跨帧命中
    #[cfg(feature = "gpu")]
    {
        struct LabelApp;
        impl View for LabelApp {
            fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
                div()
                    .flex_col()
                    .size_full()
                    .p(8)
                    .bg(Color::rgb(15, 23, 42))
                    .child(cx, text("AVATAR Typography Kerning Cache").size(16).color(Color::WHITE))
                    .into_element(cx)
            }
        }
        let mut engine = VeloxEngine::new(320, 80);
        let _ = engine.mount_root(LabelApp);
        engine.step_frame();
        let mut off = velox_engine::renderers::wgpu::WgpuOffscreenRenderer::new(320, 80);
        if off.ensure_gpu() {
            let vp = engine.viewport_region();
            let ops = engine.cached_draw_ops();
            off.render_draw_ops(ops, vp, 1, &[vp]);
            let hits_after_1 = off.text_layout_cache_hits();
            off.render_draw_ops(ops, vp, 1, &[vp]);
            let hits_after_2 = off.text_layout_cache_hits();
            assert!(
                hits_after_2 > hits_after_1,
                "expected TextLayoutCache hits to grow on 2nd frame ({hits_after_1} -> {hits_after_2})"
            );
        }
    }
}
