//! 动画系统测试：缓动曲线、不透明度过渡、入场/延迟、子树位移跟随、槽位回收、`Tween` / `Presence` 与引擎动画时钟。

use core::cell::Cell;
use velox_engine::solver::{solve_and_flatten, HitRegion};
use velox_engine::prelude::*;

/// 求解一次元素树并返回绘制指令（便于逐帧断言插值结果）。
fn solve<'a>(
    cx: &'a FrameScope,
    root: &'a Element<'a>,
    motion: &mut MotionCache,
) -> SmallVec<DrawOp<'a>, 64> {
    let mut ops: SmallVec<DrawOp<'_>, 64> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 64> = SmallVec::new();
    solve_and_flatten(cx.arena(), root, Region::new(0, 0, 200, 200), motion, &mut ops, &mut hits);
    ops
}

fn quad_bg(op: &DrawOp<'_>) -> Color {
    match op.payload {
        DrawPayload::Quad { bg, .. } => bg,
        _ => panic!("expected quad"),
    }
}

#[test]
fn test_easing_curves_endpoints_and_shapes() {
    let all = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Spring,
        Easing::EaseOutBack,
        Easing::Bounce,
        Easing::Elastic,
    ];
    for e in all {
        assert_eq!(Easing::from_bits(e as u8), e);
        assert!(e.apply_256(0).abs() <= 2, "{:?} start = {}", e, e.apply_256(0));
        assert_eq!(e.apply_256(256), 256, "{:?}", e);
    }
    // EaseOutBack 与 Elastic 存在过冲；Bounce 永不超出 256 且单调趋近
    assert!((0..256).any(|t| Easing::EaseOutBack.apply_256(t) > 256));
    assert!((0..256).any(|t| Easing::Elastic.apply_256(t) > 256));
    assert!((0..=256).all(|t| (0..=256).contains(&Easing::Bounce.apply_256(t))));
    // EaseIn 前段慢于线性，EaseOut 前段快于线性
    assert!(Easing::EaseIn.apply_256(64) < 64);
    assert!(Easing::EaseOut.apply_256(64) > 64);
    assert_eq!(Easing::Linear.sample_256(50, 100), 128);
}

#[test]
fn test_fade_in_enter_animation_and_delay() {
    let mut motion = MotionCache::new();
    let mut alphas = [0u8; 4];
    for (frame, alpha) in alphas.iter_mut().enumerate() {
        let cx = FrameScope::new();
        let root = div()
            .size_full()
            .child(
                &cx,
                div()
                    .id(7)
                    .w(50)
                    .h(50)
                    .bg(Color::WHITE)
                    .fade_in(Duration::from_millis(100))
                    .delay(Duration::from_millis(50))
                    .ease(Easing::Linear),
            )
            .into_element(&cx);
        let ops = solve(&cx, &root, &mut motion);
        *alpha = ops.iter().find(|o| o.id == Some(7)).map(|o| quad_bg(o).a()).unwrap_or(0);
        if frame < 3 {
            motion.tick(50);
        } else {
            motion.tick(0);
        }
    }
    // 帧 0：起始透明；帧 1：仍处于 50ms 延迟段；帧 2：进入动画中段；帧 3：完成
    assert_eq!(alphas[0], 0);
    assert_eq!(alphas[1], 0);
    assert!(alphas[2] > 100 && alphas[2] < 160, "mid alpha = {}", alphas[2]);
    assert_eq!(alphas[3], 255);
}

#[test]
fn test_opacity_transition_interpolates_subtree() {
    let mut motion = MotionCache::new();
    let render = |motion: &mut MotionCache, opacity: u8| -> (u8, u8) {
        let cx = FrameScope::new();
        let root = div()
            .id(1)
            .w(100)
            .h(100)
            .bg(Color::WHITE)
            .opacity(opacity)
            .animate_opacity(Duration::from_millis(100))
            .ease(Easing::Linear)
            .child(&cx, div().id(2).w(20).h(20).bg(Color::BLACK))
            .into_element(&cx);
        let ops = solve(&cx, &root, motion);
        // 完全透明的 Quad 会被求解器剔除，视为 alpha = 0
        let alpha_of = |id: u32| ops.iter().find(|o| o.id == Some(id)).map(|o| quad_bg(o).a()).unwrap_or(0);
        (alpha_of(1), alpha_of(2))
    };
    assert_eq!(render(&mut motion, 255), (255, 255));
    // 切换为全透明：首帧仍为起点，推进一半后父子同步处于半透明
    assert_eq!(render(&mut motion, 0), (255, 255));
    motion.tick(50);
    let (p, c) = render(&mut motion, 0);
    assert!(p > 100 && p < 160, "parent alpha = {}", p);
    assert_eq!(p, c);
    motion.tick(60);
    assert_eq!(render(&mut motion, 0), (0, 0));
}

#[test]
fn test_slide_in_parent_translates_children() {
    let mut motion = MotionCache::new();
    let cx = FrameScope::new();
    let root = div()
        .size_full()
        .child(
            &cx,
            div()
                .id(10)
                .w(80)
                .h(80)
                .p(10)
                .bg(Color::WHITE)
                .enter_from(255, 0, 40, Duration::from_millis(100))
                .child(&cx, div().id(11).w(20).h(20).bg(Color::BLACK)),
        )
        .into_element(&cx);
    let ops = solve(&cx, &root, &mut motion);
    let parent = ops.iter().find(|o| o.id == Some(10)).unwrap().bounds;
    let child = ops.iter().find(|o| o.id == Some(11)).unwrap().bounds;
    // 入场起点：父容器下移 40px，子节点同步跟随（保持 10px 内边距相对位置）
    assert_eq!(parent.y, 40);
    assert_eq!(child.y - parent.y, 10);
    assert_eq!(child.x - parent.x, 10);

    motion.tick(200);
    let cx2 = FrameScope::new();
    let root2 = div()
        .size_full()
        .child(
            &cx2,
            div()
                .id(10)
                .w(80)
                .h(80)
                .p(10)
                .bg(Color::WHITE)
                .enter_from(255, 0, 40, Duration::from_millis(100))
                .child(&cx2, div().id(11).w(20).h(20).bg(Color::BLACK)),
        )
        .into_element(&cx2);
    let ops2 = solve(&cx2, &root2, &mut motion);
    assert_eq!(ops2.iter().find(|o| o.id == Some(10)).unwrap().bounds.y, 0);
    assert_eq!(ops2.iter().find(|o| o.id == Some(11)).unwrap().bounds.y, 10);
}

#[test]
fn test_interrupted_bounds_transition_is_continuous() {
    let mut motion = MotionCache::new();
    let spec = Some(TransitionSpec {
        duration_ms: 100,
        animate_bounds: true,
        easing: Easing::Linear,
        ..TransitionSpec::default()
    });
    let at = |x: i16| MotionState::new(Region::new(x, 0, 10, 10), Color::WHITE, Color::TRANSPARENT);
    motion.resolve(1, at(0), spec);
    motion.resolve(1, at(100), spec);
    motion.tick(50);
    let mid = motion.resolve(1, at(100), spec).bounds.x;
    assert_eq!(mid, 50);
    // 中途反向打断：新动画从当前插值位置出发，首帧无跳变
    let restarted = motion.resolve(1, at(0), spec).bounds.x;
    assert_eq!(restarted, mid);
    motion.tick(100);
    assert_eq!(motion.resolve(1, at(0), spec).bounds.x, 0);
}

#[test]
fn test_stale_transition_slots_are_reclaimed() {
    let mut motion = MotionCache::new();
    {
        let cx = FrameScope::new();
        let mut root = div().flex_col().size_full();
        for i in 0..12u32 {
            root = root.child(
                &cx,
                div().id(100 + i).w(10).h(10).bg(Color::WHITE).animate_color(Duration::from_millis(100)),
            );
        }
        let root = root.into_element(&cx);
        solve(&cx, &root, &mut motion);
    }
    assert_eq!(motion.transition_count(), 12);
    {
        let cx = FrameScope::new();
        let root = div()
            .size_full()
            .child(&cx, div().id(100).w(10).h(10).bg(Color::WHITE).animate_color(Duration::from_millis(100)))
            // 仅声明 id 而无过渡配置的节点不应占用槽位
            .child(&cx, div().id(500).w(10).h(10).bg(Color::WHITE))
            .into_element(&cx);
        solve(&cx, &root, &mut motion);
    }
    assert_eq!(motion.transition_count(), 1);
}

#[test]
fn test_tween_and_presence() {
    let mut tw = Tween::new(0).with_duration(100).with_easing(Easing::Linear);
    assert!(!tw.is_running(0));
    tw.animate_to(1000, 200);
    assert_eq!(tw.value(1000), 0);
    assert_eq!(tw.value(1050), 100);
    assert!(tw.is_running(1050));
    // 中途重定目标保持连续
    tw.animate_to(1050, 0);
    assert_eq!(tw.value(1050), 100);
    assert_eq!(tw.value(1150), 0);
    assert!(!tw.is_running(1150));

    let mut p = Presence::new(false, 100).with_easing(Easing::Linear);
    assert!(!p.is_mounted(0));
    p.show(0);
    assert!(p.is_mounted(0));
    assert_eq!(p.opacity(50), 127);
    assert_eq!(p.offset(50, 20), 10);
    assert_eq!(p.opacity(100), 255);
    p.hide(200);
    // 退场过渡期间仍保持挂载，完成后卸载
    assert!(p.is_mounted(250));
    assert!(p.is_animating(250));
    assert!(!p.is_mounted(300));
}

struct ClockProbe {
    last_now: Cell<u32>,
    animate: bool,
}

impl View for ClockProbe {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        self.last_now.set(cx.now_ms());
        if self.animate {
            cx.request_frame();
        }
        div().w(10).h(10).bg(Color::WHITE).into_element(cx)
    }
}

#[test]
fn test_engine_clock_and_request_frame() {
    let mut engine = VeloxEngine::new(64, 64);
    let h = engine.mount_root(ClockProbe {
        last_now: Cell::new(0),
        animate: true,
    });
    engine.step_frame_dt(16);
    assert_eq!(engine.view(h).unwrap().last_now.get(), 16);
    assert!(engine.needs_redraw());
    engine.step_frame_dt(20);
    assert_eq!(engine.view(h).unwrap().last_now.get(), 36);
    assert_eq!(engine.clock_ms(), 36);

    // 停止请求后引擎回到空闲
    engine.view_mut(h).unwrap().animate = false;
    engine.step_frame_dt(16);
    assert!(!engine.needs_redraw());
    let before = engine.view(h).unwrap().last_now.get();
    engine.step_frame_dt(16);
    assert_eq!(engine.view(h).unwrap().last_now.get(), before);
}

struct ModalHost;

impl View for ModalHost {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .size_full()
            .child(
                cx,
                div()
                    .id(42)
                    .pos(0, 0)
                    .size_full()
                    .overlay()
                    .bg(Color::rgba(0, 0, 0, 160))
                    .fade_in(Duration::from_millis(100)),
            )
            .into_element(cx)
    }
}

#[test]
fn test_overlay_enter_animation_runs_to_completion() {
    let mut engine = VeloxEngine::new(64, 64);
    engine.mount_root(ModalHost);
    engine.step_frame_dt(16);
    assert!(engine.needs_redraw());
    let mut frames = 0;
    while engine.needs_redraw() && frames < 20 {
        let d = engine.step_frame_dt(16);
        if frames == 0 {
            assert!(!d.is_empty(), "fade-in frame should produce damage");
        }
        frames += 1;
    }
    assert!(frames < 20, "enter animation never settled");
    assert!(engine.step_frame_dt(16).is_empty());
}

struct ComplexScene;

impl View for ComplexScene {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let mut root = div()
            .flex_col()
            .size_full()
            .bg(Color::rgb(18, 22, 34))
            .bg_gradient(Color::rgb(28, 36, 54))
            .p(12)
            .gap(8);

        for row in 0..4 {
            let mut r = div().flex_row().w_full().h(56).gap(8);
            for col in 0..3 {
                let id = (row * 10 + col + 1) as u32;
                r = r.child(
                    cx,
                    div()
                        .id(id)
                        .flex_grow(1)
                        .h(52)
                        .rounded(10)
                        .shadow(6, 0, 2, Color::rgba(0, 0, 0, 120))
                        .bg(Color::rgb(42, 64, 110))
                        .bg_gradient_x(Color::rgb(68, 98, 160))
                        .border(1, Color::rgba(255, 255, 255, 90))
                        .p(8)
                        .child(
                            cx,
                            text(velox_engine::fmt!(cx, "Card #{}", id))
                                .size(14)
                                .color(Color::WHITE),
                        ),
                );
            }
            root = root.child(cx, r);
        }
        root.into_element(cx)
    }
}

#[test]
fn test_parallel_strip_rasterization_pixel_exact() {
    let mut engine = VeloxEngine::new(360, 280);
    engine.mount_root(ComplexScene);
    let _ = engine.step_frame_dt(16);
    let viewport = engine.viewport_region();
    let ops = engine.cached_draw_ops();

    let mut single_fb = [Xrgb8888(0); 360 * 280];
    let mut parallel_fb = [Xrgb8888(0); 360 * 280];

    rasterize_region(ops, viewport, 0, 0, 360, &mut single_fb);
    rasterize_region_parallel(ops, viewport, 0, 0, 360, &mut parallel_fb);

    assert_eq!(
        &single_fb[..],
        &parallel_fb[..],
        "Multi-threaded strip rasterization must be 100% pixel-identical to single-threaded rasterization"
    );
    assert!(rasterize_parallel_threads() >= 1);
}

#[test]
fn test_desktop_default_gpu_backend_and_offscreen_compositor() {
    use velox_engine::platform::shm_driver::{WaylandSurfaceDriver, WlSurface};
    use velox_engine::platform::wayland::WaylandPlatform;
    use velox_engine::platform::winit::WinitPlatform;

    // 1. 验证桌面端与移动端默认渲染后端选择并支持动态切换
    let default_backend = DesktopRenderBackend::default_desktop();
    #[cfg(feature = "gpu")]
    {
        assert_eq!(
            default_backend,
            DesktopRenderBackend::Wgpu,
            "Desktop should default to Wgpu (lightweight 2D SDF GPU renderer) when gpu is enabled"
        );
        assert!(default_backend.is_gpu());
    }

    let mut driver = WaylandSurfaceDriver::new(240, 160);
    assert_eq!(driver.backend(), default_backend);
    let mut wl_surf = WlSurface::new();
    let mut eng = VeloxEngine::new(240, 160);
    eng.mount_root(ComplexScene);
    assert!(driver.flush_frame_dt(&mut eng, &mut wl_surf, 16));
    assert!(driver.front_pixels().iter().any(|p| p.0 != 0));
    #[cfg(feature = "gpu")]
    {
        let gscene = driver.gpu_scene().expect("gpu_scene should exist");
        assert!(gscene.quad_count > 0);
        use velox_engine::renderers::wgpu::WgpuOffscreenRenderer;
        let mut gpu_renderer = WgpuOffscreenRenderer::new(240, 160);
        assert_eq!(gpu_renderer.name(), "wgpu");
        let mut eng_gpu = VeloxEngine::new(240, 160);
        eng_gpu.mount_root(ComplexScene);
        let (rects, px) = gpu_renderer.render_frame_dt(&mut eng_gpu, 16);
        assert_eq!(rects, 1);
        assert_eq!(px, 240 * 160);
        assert!(gpu_renderer.primitive_count() > 0);
        assert!(gpu_renderer.rgba_buffer.iter().any(|&b| b != 0));

        // 若宿主存在真实硬件 GPU，验证真实 wgpu 2D SDF 离屏管线能够渲染出非空像素
        if gpu_renderer.ensure_gpu() {
            let mut eng_hw = VeloxEngine::new(240, 160);
            eng_hw.mount_root(ComplexScene);
            gpu_renderer.rgba_buffer.fill(0);
            let (hw_rects, hw_px) = gpu_renderer.render_frame_dt(&mut eng_hw, 16);
            assert_eq!(hw_rects, 1);
            assert_eq!(hw_px, 240 * 160);
            assert!(gpu_renderer.primitive_count() > 0);
            assert!(
                gpu_renderer.rgba_buffer.iter().any(|&b| b != 0),
                "wgpu 2D SDF hardware offscreen render should produce non-empty pixels"
            );
        }
    }

    let winit_plat = WinitPlatform::new("GPU Default Test", 240, 160)
        .with_dpi_scale(1)
        .with_backend(DesktopRenderBackend::Software);
    assert_eq!(winit_plat.backend(), DesktopRenderBackend::Software);

    let wl_plat = WaylandPlatform::new_xdg_toplevel("App", "velox.app", 240, 160)
        .with_backend(DesktopRenderBackend::Software);
    assert_eq!(wl_plat.backend(), DesktopRenderBackend::Software);

    // 2. 验证 OffscreenCompositor 整屏转场合成：页面仅光栅化 1 次，后续 10 帧通过离屏缓冲平移合成
    let mut comp: OffscreenCompositor<Xrgb8888> = OffscreenCompositor::new(240, 160);
    let mut page_a = VeloxEngine::new(240, 160);
    page_a.mount_root(ComplexScene);
    let mut page_b = VeloxEngine::new(240, 160);
    page_b.mount_root(ModalHost);

    let _ = comp.cache_engine_page(1, false, &mut page_a);
    let _ = comp.cache_engine_page(2, false, &mut page_b);
    assert_eq!(comp.total_rasterizations(), 2);

    // 连续 10 帧转场动画期间再次请求缓存不会触发重光栅化
    let mut out_fb = std::vec![Xrgb8888(0); 240 * 160];
    for step in 1..=10 {
        let _ = comp.cache_engine_page(1, false, &mut page_a);
        let _ = comp.cache_engine_page(2, false, &mut page_b);
        let ox = ((10 - step) * 24) as i16;
        comp.composite_transition(
            Some((1, ox - 240, 0, 255)),
            (2, ox, 0, 255),
            Color::BLACK,
            &mut out_fb,
        );
    }
    assert_eq!(
        comp.total_rasterizations(),
        2,
        "OffscreenCompositor must reuse cached page buffers across all transition frames"
    );
    assert!(out_fb.iter().any(|p| p.0 != 0));
}

#[test]
fn test_duplicate_id_in_different_containers_does_not_loop_forever() {
    struct CollidingIdsView {
        bar_w: i16,
    }
    impl View for CollidingIdsView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            // 左侧无动画容器子项与右侧开启动画的进度条故意使用相同 id(302)，
            // 验证 MotionCache 基于 (id, path_hash) 作用域隔离，绝不因跨容器同帧冲突导致无限重绘死循环。
            let left = div()
                .id(10)
                .w(80)
                .h(120)
                .child(cx, div().id(302).w(60).h(24).bg(Color::rgb(40, 50, 60)));
            let right = div().id(20).w(160).h(120).child(
                cx,
                div()
                    .id(302)
                    .w(self.bar_w)
                    .h(10)
                    .bg(Color::rgb(56, 189, 248))
                    .animate_bounds(Duration::from_millis(160)),
            );
            div()
                .size_full()
                .flex_row()
                .child(cx, left)
                .child(cx, right)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 120);
    let root = engine.mount_root(CollidingIdsView { bar_w: 40 });
    let _ = engine.step_frame_dt(16);
    assert!(!engine.needs_redraw());

    // 触发右侧进度条宽度变化（40 -> 120），应平滑过渡并在 ~10 帧后自动进入 0% CPU 休眠
    engine.update_view(root, |v| v.bar_w = 120);
    let mut active_frames = 0usize;
    let mut settled = false;
    for _ in 0..30 {
        let _ = engine.step_frame_dt(16);
        if engine.needs_redraw() {
            active_frames += 1;
        } else {
            settled = true;
            break;
        }
    }
    assert!(
        active_frames >= 5 && active_frames <= 15 && settled,
        "Expected bounded transition frames before idle sleep, got active_frames={active_frames}, settled={settled}"
    );
}

#[test]
fn test_animate_bounds_does_not_drift_on_vertical_scroll_or_parent_shift() {
    struct ScrollSwitchView {
        sidebar_w: i16,
        checked: bool,
    }

    impl View for ScrollSwitchView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut track = div()
                .id(701)
                .flex_row()
                .items_center()
                .w(36)
                .h(20)
                .p(2)
                .bg(Color::rgb(59, 130, 246));
            if self.checked {
                track.style_mut().set_justify(Justify::End);
            } else {
                track.style_mut().set_justify(Justify::Start);
            }
            let thumb = div()
                .id(702)
                .w(16)
                .h(16)
                .bg(Color::WHITE)
                .animate_bounds(Duration::from_millis(180));

            let content = div()
                .id(700)
                .flex_col()
                .flex_grow(1)
                .h_full()
                .p(16)
                .gap(12)
                .scrollable(300)
                .overflow_hidden()
                .child(cx, div().w(120).h(80).bg(Color::rgb(30, 41, 59)))
                .child(cx, track.child(cx, thumb))
                .child(cx, div().w(120).h(320).bg(Color::rgb(30, 41, 59)));

            div()
                .size_full()
                .flex_row()
                .child(cx, div().w(self.sidebar_w).h_full().bg(Color::rgb(15, 23, 42)))
                .child(cx, content)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(400, 220);
    let root = engine.mount_root(ScrollSwitchView {
        sidebar_w: 160,
        checked: false,
    });
    let _ = engine.step_frame_dt(16);
    assert!(!engine.needs_redraw());

    let find_bounds = |eng: &VeloxEngine, id: u32| -> Region {
        eng.cached_draw_ops()
            .iter()
            .find(|op| op.id == Some(id))
            .expect("op must exist")
            .bounds
    };

    let t0 = find_bounds(&engine, 701);
    let k0 = find_bounds(&engine, 702);
    assert_eq!(k0.x - t0.x, 2);
    assert_eq!(k0.y - t0.y, 2);

    // 1. 垂直滚轮滚动：同一帧内滑块必须 0 延迟死锁在轨道正中，且绝不误触发 animate_bounds 动画
    engine.scroll(700, 48);
    let _ = engine.step_frame_dt(16);
    let t1 = find_bounds(&engine, 701);
    let k1 = find_bounds(&engine, 702);
    assert_eq!(t1.y, t0.y - 48, "track should scroll up by 48px");
    assert_eq!(
        k1.y - t1.y,
        2,
        "thumb must stay vertically locked inside track during scroll (track={t1:?}, thumb={k1:?})"
    );
    assert_eq!(k1.x - t1.x, 2);
    assert!(
        !engine.needs_redraw(),
        "scrolling must not falsely trigger animate_bounds animation"
    );

    // 2. 侧边栏折叠导致父容器横向刚性平移 -110px：滑块仍必须 0 延迟锁定在轨道内
    engine.update_view(root, |v| v.sidebar_w = 50);
    let _ = engine.step_frame_dt(16);
    let t2 = find_bounds(&engine, 701);
    let k2 = find_bounds(&engine, 702);
    assert_eq!(t2.x, t1.x - 110);
    assert_eq!(
        k2.x - t2.x,
        2,
        "thumb must stay horizontally locked inside track when parent shifts (track={t2:?}, thumb={k2:?})"
    );
    assert_eq!(k2.y - t2.y, 2);
    assert!(
        !engine.needs_redraw(),
        "parent rigid shift must not falsely trigger child animate_bounds"
    );

    // 3. 真正切换开关 checked=true：触发正常水平平滑过渡并最终落位在轨道右侧（x + 18）
    engine.update_view(root, |v| v.checked = true);
    let _ = engine.step_frame_dt(16);
    assert!(engine.needs_redraw(), "toggling switch must trigger thumb slide animation");
    for _ in 0..20 {
        let _ = engine.step_frame_dt(16);
        if !engine.needs_redraw() {
            break;
        }
    }
    let t3 = find_bounds(&engine, 701);
    let k3 = find_bounds(&engine, 702);
    assert_eq!(k3.x - t3.x, 18);
    assert_eq!(k3.y - t3.y, 2);
    assert!(!engine.needs_redraw());
}
