//! 现代声明式动画架构升级回归测试套件：
//! 1. `MotionCache` 定点数二阶弹簧速度继承（中途打断无折角）与无障碍 `reduced_motion` 开关；
//! 2. 基于幽灵快照的零配置退场（`.exit_to` / `.auto_animate()`）与容器级 `.stagger()` 交错延迟；
//! 3. 合成层 FLIP 仿射变换 `Transform2D`、圆角逆缩放 `CornerRadii::inverse_scaled` 与嵌套祖先偏移扣除；
//! 4. `@use-gesture` 双曲橡皮筋阻尼、手势速度移交弹簧与惯性撞边临界阻尼回弹。

use velox_engine::motion::{DrawPayload, MotionCache, MotionState};
use velox_engine::paint::{compute_flip_transform, CornerRadii};
use velox_engine::prelude::*;

#[test]
fn test_motion_cache_second_order_spring_velocity_inheritance_on_interrupt() {
    let mut cache = MotionCache::new();
    let spec = TransitionSpec {
        duration_ms: 240,
        animate_bounds: true,
        easing: Easing::Spring,
        ..TransitionSpec::default()
    };

    // 1. 首帧定位在 x = 0
    cache.begin_frame();
    let s0 = cache.resolve(
        1,
        MotionState::new(Region::new(0, 10, 40, 40), Color::WHITE, Color::TRANSPARENT),
        Some(spec),
    );
    cache.end_frame();
    assert_eq!(s0.bounds.x, 0);
    assert!(!cache.has_active_animations());

    // 2. 目标切至 x = 200，推进 4 帧（64ms）积累显著正向瞬时速度
    cache.begin_frame();
    let _ = cache.resolve(
        1,
        MotionState::new(Region::new(200, 10, 40, 40), Color::WHITE, Color::TRANSPARENT),
        Some(spec),
    );
    cache.end_frame();
    assert!(cache.has_active_animations());

    for _ in 0..4 {
        assert!(cache.tick(16));
    }
    let mid_state = cache.current_state(1).unwrap();
    let (mid_vx, _) = cache.current_velocity(1).unwrap();
    assert!(
        mid_state.bounds.x > 20 && mid_state.bounds.x < 190,
        "spring should be mid-flight, got x={}",
        mid_state.bounds.x
    );
    assert!(
        mid_vx > 200,
        "spring must build substantial positive velocity, got vx={mid_vx} px/s"
    );

    // 3. 中途反向打断：目标切回 x = 20，瞬时速度必须严格继承（绝不突变归零！）
    cache.begin_frame();
    let _ = cache.resolve(
        1,
        MotionState::new(Region::new(20, 10, 40, 40), Color::WHITE, Color::TRANSPARENT),
        Some(spec),
    );
    cache.end_frame();
    let (after_interrupt_vx, _) = cache.current_velocity(1).unwrap();
    assert_eq!(
        after_interrupt_vx, mid_vx,
        "interrupting a spring transition must inherit instantaneous velocity without discontinuity"
    );

    // 推进足够多帧后必须精确收敛到 x = 20 并进入 0% CPU 休眠
    for _ in 0..200 {
        if !cache.tick(16) {
            break;
        }
    }
    assert!(!cache.has_active_animations(), "spring must eventually settle");
    assert_eq!(cache.current_state(1).unwrap().bounds.x, 20);

    // 4. 开启 `reduced_motion` 后单次位移瞬时到位
    cache.set_reduced_motion(true);
    cache.begin_frame();
    let snap = cache.resolve(
        1,
        MotionState::new(Region::new(300, 10, 40, 40), Color::WHITE, Color::TRANSPARENT),
        Some(spec),
    );
    cache.end_frame();
    assert_eq!(snap.bounds.x, 300);
    assert!(!cache.has_active_animations());
}

#[test]
fn test_zero_config_ghost_exit_and_auto_animate_stagger() {
    struct ListView {
        items: [Option<(u32, &'static str)>; 3],
    }

    impl View for ListView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut list = div()
                .w(320)
                .h(240)
                .bg(Color::rgb(15, 20, 30))
                .flex_col()
                .gap(8)
                .auto_animate_with(Duration::from_millis(160))
                .stagger(Duration::from_millis(40));

            for item in self.items.iter().flatten() {
                let (id, label) = *item;
                list = list.child(
                    cx,
                    div()
                        .id(id)
                        .w(280)
                        .h(40)
                        .bg(Color::rgb(59, 130, 246))
                        .rounded(8)
                        .child(cx, text(label).color(Color::WHITE).size(14)),
                );
            }
            list.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 240);
    let root = engine.mount_root(ListView {
        items: [
            Some((101, "Alpha")),
            Some((102, "Beta")),
            Some((103, "Gamma")),
        ],
    });

    // 首帧与入场收敛（含 0ms / 40ms / 80ms stagger 交错延迟）
    let _ = engine.step_frame_dt(16);
    assert!(
        engine.needs_redraw(),
        "auto_animate children should animate on initial mount"
    );
    for _ in 0..80 {
        let _ = engine.step_frame_dt(16);
    }
    assert_eq!(engine.active_ghost_count(), 0);

    // 直接从状态移除第 2 项（102: "Beta"），View 零 Presence 样板代码
    engine.view_mut(root).unwrap().items[1] = None;
    let _ = engine.step_frame_dt(16);
    assert_eq!(
        engine.active_ghost_count(),
        1,
        "removed child under auto_animate() must automatically spawn a ghost exit snapshot"
    );
    assert!(engine.needs_redraw());

    // 推进至退场中段，验证幽灵 Quad 与附带标签 TextRun 均仍在 DrawOp 中渲染并淡出
    let _ = engine.step_frame_dt(64);
    let has_ghost_quad = engine
        .cached_draw_ops()
        .iter()
        .any(|op| op.id == Some(102 ^ 0x4748_0000));
    let has_ghost_label = engine
        .cached_draw_ops()
        .iter()
        .any(|op| op.id == Some(102 ^ 0x4749_0000));
    assert!(has_ghost_quad, "ghost exit Quad must be emitted mid-exit");
    assert!(has_ghost_label, "ghost exit TextRun label must be emitted mid-exit");

    // 推进超过 160ms，幽灵自动回收，无残留
    for _ in 0..20 {
        let _ = engine.step_frame_dt(16);
    }
    assert_eq!(
        engine.active_ghost_count(),
        0,
        "ghost exit snapshot must auto-reclaim after duration completes"
    );
}

#[test]
fn test_flip_layout_compositor_xform_inverse_radii_and_nested_compensation() {
    // 1. 单元测试圆角逆缩放与 FLIP 仿射变换计算
    let r = CornerRadii::all(12);
    let inv = r.inverse_scaled(0.5, 0.5);
    assert_eq!(inv, CornerRadii::all(24));

    let layout_b = Region::new(100, 100, 300, 150);
    let visual_b = Region::new(20, 40, 150, 75);
    let (xf, sx, sy) = compute_flip_transform(layout_b, visual_b).expect("must produce FLIP xform");
    assert!((sx - 0.5).abs() < 1e-4);
    assert!((sy - 0.5).abs() < 1e-4);
    assert!((xf.a - 0.5).abs() < 1e-4);
    assert!((xf.d - 0.5).abs() < 1e-4);

    // 2. 端到端 `.flip_layout()`：卡片尺寸展开时保持终点布局框，发射带缩放 xform 与逆缩放圆角的合成原语
    struct FlipCardView {
        expanded: bool,
    }
    impl View for FlipCardView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let (w, h) = if self.expanded { (320, 160) } else { (120, 60) };
            div()
                .w(400)
                .h(240)
                .bg(Color::rgb(10, 14, 22))
                .child(
                    cx,
                    div()
                        .id(501)
                        .w(w)
                        .h(h)
                        .bg(Color::rgb(37, 99, 235))
                        .rounded(12)
                        .flip_layout(Duration::from_millis(240)),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(400, 240);
    let root = engine.mount_root(FlipCardView { expanded: false });
    let _ = engine.step_frame_dt(16);

    // 展开卡片
    engine.view_mut(root).unwrap().expanded = true;
    let _ = engine.step_frame_dt(16);
    let _ = engine.step_frame_dt(32);

    // 验证发射的是携带非单位 FLIP `Transform2D` 且圆角被逆缩放补偿（> 12）的 `StyledQuad`，
    // 同时基底 `bounds` 保持终点尺寸 `320×160`（避免内部多行文本逐帧反复折行）
    let flip_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(501))
        .expect("FLIP card op must exist");
    assert_eq!(flip_op.bounds.w, 320);
    assert_eq!(flip_op.bounds.h, 160);
    match flip_op.payload {
        DrawPayload::StyledQuad(sq) => {
            let xf = sq.transform.expect("FLIP mid-flight must carry Transform2D");
            assert!(xf.a > 0.3 && xf.a < 0.99, "expected scale_x < 1, got {}", xf.a);
            assert!(
                sq.radii.tl > 12,
                "corner radii must be inverse-scaled during FLIP shrink, got {}",
                sq.radii.tl
            );
        }
        _ => panic!("expected StyledQuad for FLIP transform"),
    }

    // 收敛后不再需要 FLIP 仿射变换，自动回落普通快速路径
    for _ in 0..120 {
        let _ = engine.step_frame_dt(16);
    }
    let settled_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(501))
        .unwrap();
    assert!(matches!(settled_op.payload, DrawPayload::Quad { .. }));
}

#[test]
fn test_rubberband_overscroll_and_boundary_spring_handoff() {
    // 1. 双曲橡皮筋数学不变量：次线性单调阻尼、不突破 dimension 渐近线
    let d1 = rubberband(50.0, 300.0, 0.55);
    let d2 = rubberband(500.0, 300.0, 0.55);
    assert!(d1 > 0.0 && d1 < 50.0);
    assert!(d2 > d1 && d2 < 300.0);

    let i1 = rubberband_i16(-100, 300, 140);
    let i2 = rubberband_i16(-600, 300, 140);
    assert!(i1 < 0 && i1 > -100);
    assert!(i2 < i1 && i2 > -300);

    // 2. `Spring::from_gesture_velocity` 瞬时速度注入
    let mut sp = Spring::from_gesture_velocity(10.0, 0.0, 800.0, SpringConfig::DEFAULT);
    assert!(sp.is_running());
    sp.step(16);
    assert!(sp.value() > 10.0, "positive gesture velocity must carry forward before returning");
    for _ in 0..200 {
        sp.step(16);
    }
    assert!(sp.is_settled());
    assert_eq!(sp.value_i32(), 0);

    // 3. 越界拖拽橡皮筋阻尼 + 松手临界阻尼弹簧回弹
    let mut cache = MotionCache::new();
    let scroll_id = 88u32;
    cache.apply_scroll_drag(scroll_id, -120, 400, 300);
    let over = cache.scroll_overscroll(scroll_id);
    assert!(
        over < -20 && over > -100,
        "dragging past top boundary must apply rubberband damping, got overscroll={over}"
    );
    assert_eq!(cache.scroll_offset(scroll_id), over);

    // 松手后自动回弹归零
    cache.release_scroll_drag(scroll_id, -10, 400);
    assert!(cache.has_active_animations());
    for _ in 0..120 {
        if !cache.tick(16) {
            break;
        }
    }
    assert_eq!(cache.scroll_overscroll(scroll_id), 0);
    assert_eq!(cache.scroll_offset(scroll_id), 0);
    assert!(!cache.has_active_animations());

    // 4. 惯性高速滚动撞击底边 max_scroll：动量无缝移交回弹弹簧（产生柔和过冲再回落到 max_scroll）
    cache.set_scroll_offset(scroll_id, 380);
    cache.set_scroll_inertia(scroll_id, 40, 400);
    let mut max_seen = 380i16;
    for _ in 0..160 {
        let _ = cache.tick(16);
        let cur = cache.scroll_offset(scroll_id);
        if cur > max_seen {
            max_seen = cur;
        }
        if !cache.has_active_animations() {
            break;
        }
    }
    assert!(
        max_seen > 400,
        "inertial momentum hitting bottom boundary should hand off to overscroll spring, max_seen={max_seen}"
    );
    assert_eq!(
        cache.scroll_offset(scroll_id),
        400,
        "after boundary bounce, scroll offset must settle cleanly at max_scroll"
    );
    assert!(!cache.has_active_animations());
}

#[test]
fn test_symmetric_enter_exit_preserves_durations_and_nested_dialog_title_ghost() {
    struct ModalCardView {
        open: bool,
    }
    impl View for ModalCardView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut root = div().w(480).h(320).bg(Color::rgb(12, 16, 24));
            if self.open {
                root = root.child(
                    cx,
                    div()
                        .id(911)
                        .pos(80, 60)
                        .w(320)
                        .h(160)
                        .p(16)
                        .rounded(10)
                        .bg(Color::rgb(28, 34, 48))
                        .overlay()
                        .enter_from(255, 0, 14, Duration::from_millis(240))
                        .ease(Easing::EaseOutBack)
                        .exit_to(0, 0, 14, Duration::from_millis(160))
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .w_full()
                                .child(cx, text("Edit Component Tokens").size(15).color(Color::WHITE)),
                        ),
                );
            }
            root.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(480, 320);
    let root = engine.mount_root(ModalCardView { open: true });
    let _ = engine.step_frame_dt(16);
    // 推进至入场完成
    for _ in 0..25 {
        let _ = engine.step_frame_dt(16);
    }

    // 关闭弹窗：验证同时产出圆角卡片幽灵与嵌套在 header div 内的标题文字幽灵
    engine.view_mut(root).unwrap().open = false;
    let _ = engine.step_frame_dt(16);
    let _ = engine.step_frame_dt(48);

    let has_card_ghost = engine
        .cached_draw_ops()
        .iter()
        .any(|op| op.id == Some(911 ^ 0x4748_0000));
    let has_title_ghost = engine.cached_draw_ops().iter().any(|op| {
        op.id == Some(911 ^ 0x4749_0000)
            && matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "Edit Component Tokens")
    });
    assert!(has_card_ghost, "closing dialog must emit sliding card ghost");
    assert!(
        has_title_ghost,
        "closing dialog must capture nested header title text into ghost snapshot"
    );

    // 约 160ms 后退场完成并自动回收进入 0% CPU 休眠
    for _ in 0..16 {
        let _ = engine.step_frame_dt(16);
    }
    assert!(
        !engine.needs_redraw(),
        "dialog exit ghost must settle cleanly to idle"
    );
}

#[test]
fn test_ime_caret_tracks_exact_cursor_position_and_focal_radial_reflect() {
    struct EditorCaretView {
        caret_x: i16,
        caret_y: i16,
    }
    impl View for EditorCaretView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .child(
                    cx,
                    div()
                        .pos(40, 60)
                        .w(600)
                        .h(400)
                        .focusable()
                        .child(
                            cx,
                            div()
                                .pos(self.caret_x, self.caret_y)
                                .w(1)
                                .h(18)
                                .bg(Color::TRANSPARENT)
                                .ime_caret(),
                        ),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(800, 600);
    let root = engine.mount_root(EditorCaretView {
        caret_x: 120,
        caret_y: 84,
    });
    let _ = engine.step_frame_dt(16);
    assert_eq!(
        engine.ime_cursor_bounds(),
        Some(Region::new(160, 144, 1, 18)),
        "IME cursor bounds must track exact .ime_caret() screen rect instead of whole editor box"
    );

    // 移动光标后精确跟随
    {
        let v = engine.view_mut(root).unwrap();
        v.caret_x = 248;
        v.caret_y = 192;
    }
    let _ = engine.step_frame_dt(16);
    assert_eq!(
        engine.ime_cursor_bounds(),
        Some(Region::new(288, 252, 1, 18))
    );

    // 双圆焦点径向渐变在 Reflect 模式下对圆内焦点无 NaN/None 截断且选出正确的正根
    static STOPS: [ColorStop; 2] = [
        ColorStop::new(0.0, Color::WHITE),
        ColorStop::new(1.0, Color::BLACK),
    ];
    let p = Paint::RadialGradient {
        center: [0.5, 0.5],
        radius: 0.28,
        focal: [0.42, 0.40],
        focal_radius: 0.02,
        stops: &STOPS,
        extend: ExtendMode::Reflect,
        space: GradientSpace::Oklab,
    };
    for &u in &[0.05f32, 0.25, 0.42, 0.50, 0.75, 0.95] {
        for &v in &[0.05f32, 0.25, 0.40, 0.50, 0.75, 0.95] {
            let t = p.eval_gradient_t(u, v).expect("focal inside circle must never clip");
            assert!((0.0..=1.0).contains(&t), "reflected t out of range at ({u},{v}): {t}");
        }
    }
}
