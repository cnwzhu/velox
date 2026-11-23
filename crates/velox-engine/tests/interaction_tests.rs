//! `velox-engine` 交互手势、焦点导航、作用域 ID、原生 IME 与无障碍增量树测试套件。

use velox_engine::prelude::*;

struct ChildCounter {
    count: u16,
}

impl View for ChildCounter {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .w(120)
            .h(40)
            .bg(Color::rgb(30, 64, 175))
            .on_click(
                cx,
                cx.listener(|this, _ev, acx| {
                    this.count += 1;
                    acx.notify();
                }),
            )
            .child(cx, text(fmt!(cx, "Count={}", self.count)))
            .into_element(cx)
    }
}

struct ParentApp {
    child: Handle<ChildCounter>,
}

impl View for ParentApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .flex_col()
            .size_full()
            .p(10)
            .child(cx, self.child)
            .into_element(cx)
    }
}

#[test]
fn test_nested_subview_rendering_and_click_isolation() {
    let mut engine = VeloxEngine::new(320, 240);
    let child_handle = engine.insert_view(ChildCounter { count: 0 });
    let _root_handle = engine.mount_root(ParentApp {
        child: child_handle,
    });

    // 首帧全屏渲染
    let d1 = engine.step_frame();
    assert_eq!(d1.len(), 1);
    assert_eq!(d1[0], Region::new(0, 0, 320, 240));

    // 静止帧 0 脏区
    assert!(engine.step_frame().is_empty());

    // 点击嵌套子视图 ChildCounter (位于 (10..130, 10..50))
    assert!(engine.dispatch_click(25, 25));
    assert_eq!(engine.view(child_handle).unwrap().count, 1);

    // 验证子视图状态变更仅触发子视图局部脏矩形重绘
    let d2 = engine.step_frame();
    assert!(!d2.is_empty());
    assert!(d2[0].w <= 120 && d2[0].h <= 40);
}

struct GestureAndFocusApp {
    slider_val: i32,
    btn_a_clicks: u16,
    btn_b_clicks: u16,
}

impl View for GestureAndFocusApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .flex_col()
            .size_full()
            .p(8)
            .gap(8)
            .child(
                cx,
                // 可拖拽滑块区域 (x: 8..208, y: 8..32)
                div()
                    .id(10)
                    .w(200)
                    .h(24)
                    .bg(Color::rgb(30, 41, 59))
                    .on_drag(
                        cx,
                        cx.listener(|this, ev: DragEvent, acx| {
                            this.slider_val =
                                ((ev.local_x as i32) * 100) / (ev.width.max(1) as i32);
                            acx.notify();
                        }),
                    ),
            )
            .child(
                cx,
                // 焦点按钮 A
                div()
                    .id(11)
                    .w(100)
                    .h(30)
                    .bg(Color::rgb(37, 99, 235))
                    .active_bg(Color::rgb(29, 78, 216))
                    .on_click(
                        cx,
                        cx.listener(|this, _ev, acx| {
                            this.btn_a_clicks += 1;
                            acx.notify();
                        }),
                    ),
            )
            .child(
                cx,
                // 焦点按钮 B
                div()
                    .id(12)
                    .w(100)
                    .h(30)
                    .bg(Color::rgb(16, 185, 129))
                    .on_click(
                        cx,
                        cx.listener(|this, _ev, acx| {
                            this.btn_b_clicks += 1;
                            acx.notify();
                        }),
                    ),
            )
            .into_element(cx)
    }
}

#[test]
fn test_input_gesture_drag_and_key_focus_navigation() {
    let mut engine = VeloxEngine::new(320, 240);
    let handle = engine.mount_root(GestureAndFocusApp {
        slider_val: 0,
        btn_a_clicks: 0,
        btn_b_clicks: 0,
    });
    engine.step_frame();

    // 1. 测试指针按下并拖拽滑块 (从 x=28 拖到 x=108，即 local_x = 100 / 200 -> 50%)
    assert!(engine.dispatch_pointer_down(28, 16));
    assert!(engine.dispatch_pointer_move(108, 16));
    engine.dispatch_pointer_up(108, 16);
    assert_eq!(engine.view(handle).unwrap().slider_val, 50);

    // 2. 测试物理按键/旋钮焦点导航 (Tab 切换焦点 + Enter 触发点击)
    engine.set_focused_index(None);
    assert!(engine.dispatch_key(KeyCode::Tab));
    assert_eq!(engine.focused_index(), Some(0)); // 按钮 A
    assert!(engine.dispatch_key(KeyCode::Enter));
    assert_eq!(engine.view(handle).unwrap().btn_a_clicks, 1);

    assert!(engine.dispatch_key(KeyCode::Tab));
    assert_eq!(engine.focused_index(), Some(1)); // 按钮 B
    assert!(engine.dispatch_key(KeyCode::Enter));
    assert_eq!(engine.view(handle).unwrap().btn_b_clicks, 1);
}

struct SharedSettingsModel {
    volume: i32,
}

impl View for SharedSettingsModel {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div().into_element(cx)
    }
}

struct ParentDashboardView {
    settings: Handle<SharedSettingsModel>,
}

impl View for ParentDashboardView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let vol = cx.read(self.settings).map(|s| s.volume).unwrap_or(0);
        let settings_handle = self.settings;
        div()
            .size_full()
            .child(
                cx,
                div()
                    .id(10)
                    .w(100)
                    .h(40)
                    .bg(Color::rgb(37, 99, 235))
                    .on_click(
                        cx,
                        cx.listener(move |_this, _ev, acx| {
                            acx.update(settings_handle, |model, inner_acx| {
                                model.volume += 15;
                                inner_acx.notify();
                            });
                            acx.notify();
                        }),
                    )
                    .child(cx, text(velox_engine::fmt!(cx, "VOL: {}", vol))),
            )
            .into_element(cx)
    }
}

#[test]
fn test_cross_view_read_and_update() {
    let mut engine = VeloxEngine::new(200, 100);
    let settings = engine.insert_view(SharedSettingsModel { volume: 50 });
    engine.mount_root(ParentDashboardView { settings });

    engine.step_frame();
    assert_eq!(engine.view(settings).unwrap().volume, 50);

    assert!(engine.dispatch_click(30, 20));
    assert_eq!(engine.view(settings).unwrap().volume, 65);
    assert!(engine.needs_redraw());
}

struct OverlayAndHoldView {
    bg_clicks: u32,
    long_presses: u32,
    repeat_clicks: u32,
    show_overlay: bool,
}

impl View for OverlayAndHoldView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let mut root = div().size_full().flex_col();

        // 即使在树中先声明 overlay 节点，它也应在最后绘制（Z-order 置顶）并阻断底层点击穿透
        if self.show_overlay {
            root = root.child(
                cx,
                div()
                    .id(900)
                    .pos(0, 0)
                    .size_full()
                    .overlay()
                    .occlude_hit()
                    .bg(Color::rgba(0, 0, 0, 160)),
            );
        }

        root.child(
            cx,
            div()
                .id(1)
                .w(100)
                .h(40)
                .bg(Color::rgb(20, 180, 90))
                .auto_repeat()
                .on_click(
                    cx,
                    cx.listener(|this, _ev, acx| {
                        this.bg_clicks += 1;
                        this.repeat_clicks += 1;
                        acx.notify();
                    }),
                )
                .on_long_press(
                    cx,
                    cx.listener(|this, _ev, acx| {
                        this.long_presses += 1;
                        acx.notify();
                    }),
                ),
        )
        .into_element(cx)
    }
}

#[test]
fn test_overlay_z_order_occlude_and_long_press_auto_repeat() {
    let mut engine = VeloxEngine::new(200, 200);
    let handle = engine.mount_root(OverlayAndHoldView {
        bg_clicks: 0,
        long_presses: 0,
        repeat_clicks: 0,
        show_overlay: true,
    });
    engine.step_frame();

    // 1. 当 overlay 遮罩开启时，点击 (20, 20) 被 occlude_hit 拦截，底层按钮不触发
    assert!(!engine.dispatch_click(20, 20));
    assert_eq!(engine.view(handle).unwrap().bg_clicks, 0);

    // 2. 关闭遮罩后，按住 550ms 触发长按与按住连发
    engine.view_mut(handle).unwrap().show_overlay = false;
    engine.step_frame();

    assert!(engine.dispatch_pointer_down(20, 20));
    engine.step_frame_dt(300);
    assert_eq!(engine.view(handle).unwrap().long_presses, 0);

    // 累计按住 550ms：触发 1 次长按（>= 500ms）+ 1 次自动连发（>= 400ms 后满 100ms）
    engine.step_frame_dt(250);
    assert_eq!(engine.view(handle).unwrap().long_presses, 1);
    assert_eq!(engine.view(handle).unwrap().repeat_clicks, 1);

    // 再按住 200ms：再触发 2 次自动连发
    engine.step_frame_dt(200);
    assert_eq!(engine.view(handle).unwrap().repeat_clicks, 3);

    // 抬起手指：因已触发过长按，不再额外误触普通单次点击
    engine.dispatch_pointer_up(20, 20);
    assert_eq!(engine.view(handle).unwrap().repeat_clicks, 3);
}

#[test]
fn test_reentrant_borrow_safety_pod_remove_and_keyboard_safe_area() {
    use core::sync::atomic::{AtomicUsize, Ordering};

    static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct ViewA {
        b: Option<Handle<ViewB>>,
        val: u32,
    }
    impl Drop for ViewA {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl View for ViewA {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let b_handle = self.b.unwrap();
            div()
                .id(1)
                .w(100)
                .h(40)
                .bg(Color::WHITE)
                .on_click(
                    cx,
                    cx.listener(move |this, _ev, acx| {
                        this.val += 1;
                        acx.update(b_handle, |b_view, inner_acx| {
                            b_view.val += 10;
                            // 尝试 A -> B -> A 循环重入：必须安全返回 None，绝不产生双 &mut 别名 UB
                            let reentrant = b_view
                                .a
                                .and_then(|ah| inner_acx.update(ah, |a_again, _| a_again.val += 999));
                            assert!(reentrant.is_none());
                            assert!(b_view.a.and_then(|ah| inner_acx.read(ah)).is_none());
                        });
                        acx.notify();
                    }),
                )
                .into_element(cx)
        }
    }

    struct ViewB {
        a: Option<Handle<ViewA>>,
        val: u32,
    }
    impl View for ViewB {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div().into_element(cx)
        }
    }

    DROP_COUNT.store(0, Ordering::SeqCst);
    let mut engine = VeloxEngine::new(200, 400);
    let b = engine.insert_view(ViewB { a: None, val: 0 });
    let a = engine.mount_root(ViewA {
        b: Some(b),
        val: 0,
    });
    engine.view_mut(b).unwrap().a = Some(a);

    engine.step_frame();
    let solves_before = engine.layout_solve_count();
    // 命中测试应直接复用本帧缓存的 HitRegion，不再触发额外的整树 layout solve
    assert!(!engine.dispatch_pointer_move(10, 10));
    assert_eq!(engine.layout_solve_count(), solves_before);

    assert!(engine.dispatch_click(20, 20));
    assert_eq!(engine.view(a).unwrap().val, 1);
    assert_eq!(engine.view(b).unwrap().val, 10);

    // 验证 PodPool::remove 释放槽位与 Drop 调用
    let temp = engine.insert_view(ViewA { b: None, val: 42 });
    assert!(engine.remove_view(temp));
    assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 1);
    assert!(engine.view(temp).is_none());

    // 验证 > 127px 软键盘高度不发生 i8 截断
    let mut cx = FrameScope::new();
    cx.set_safe_area(SafeAreaInsets::new(48, 0, 34, 0));
    cx.set_keyboard_inset(280);
    let d = div().safe_area(&cx);
    assert_eq!(d.extra_padding.top, 48);
    assert_eq!(d.extra_padding.bottom, 314);
}

#[test]
fn test_horizontal_row_drag_scroll_scroll_x_and_pod_read_guard() {
    struct HorizScrollView;
    impl View for HorizScrollView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .id(600)
                .flex_row()
                .w(100)
                .h(40)
                .scrollable(120)
                .child(cx, div().id(601).w(100).h(40).bg(Color::rgb(255, 0, 0)))
                .child(cx, div().id(602).w(100).h(40).bg(Color::rgb(0, 0, 255)))
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(100, 40);
    engine.mount_root(HorizScrollView);
    engine.step_frame();

    // 1. 验证水平滚轮 ScrollX 驱动 FlexDir::Row 可滚动容器
    assert!(engine.dispatch_event(InputEvent::ScrollX {
        x: 50,
        y: 20,
        delta_x: 30,
    }));
    let d1 = engine.step_frame();
    assert!(!d1.is_empty());
    assert_eq!(engine.cached_draw_ops()[0].bounds.x, -30);
    assert_eq!(engine.cached_draw_ops()[1].bounds.x, 70);

    // 2. 验证水平指针拖拽（dx = -25 -> scroll +25 => 55）驱动 FlexDir::Row 容器跟手滚动，且抬手后触发惯性滚动（+25 => 80）
    engine.dispatch_pointer_down(80, 20);
    assert!(engine.dispatch_pointer_move(55, 20));
    let _ = engine.step_frame();
    assert_eq!(engine.cached_draw_ops()[0].bounds.x, -55);
    assert_eq!(engine.cached_draw_ops()[1].bounds.x, 45);
    engine.dispatch_pointer_up(55, 20);
    let _ = engine.step_frame();
    assert_eq!(engine.cached_draw_ops()[0].bounds.x, -80);
    assert_eq!(engine.cached_draw_ops()[1].bounds.x, 20);

    // 3. 验证 PodReadGuard 活跃读锁：守卫存活期间阻止对同一槽位的重入可变渲染
    let mut pool = PodPool::new();
    let h = pool.insert(SharedSettingsModel { volume: 77 });
    {
        let guard = pool.read_guarded(h).expect("read_guarded should succeed");
        assert_eq!(guard.volume, 77);
        let mut fs = FrameScope::new();
        assert!(
            pool.render_pod_in_arena(h.pod_id(), &mut fs).is_none(),
            "Active PodReadGuard must block mutable render of the same slot"
        );
    }
    // 守卫释放后可正常获取可变借用、渲染与移除
    let mut fs = FrameScope::new();
    assert!(pool.render_pod_in_arena(h.pod_id(), &mut fs).is_some());
    assert!(pool.get_mut(h).is_some());
    assert!(pool.remove(h));
}

#[test]
fn test_align_self_items_baseline_scrollable_auto_and_a11y_tree() {
    use velox_engine::a11y::A11yRole;
    use velox_engine::solver::{solve_and_flatten, HitRegion};

    // 1. 验证 align_self (self_start / self_center / self_end / self_stretch) 在 FlexDir::Col 下的交叉轴覆盖
    let cx = FrameScope::new();
    let col_root = div()
        .flex_col()
        .w(200)
        .h(160)
        .items_start()
        .child(&cx, div().w(60).h(20).self_center().bg(Color::rgb(255, 0, 0)))
        .child(&cx, div().w(60).h(20).self_end().bg(Color::rgb(0, 255, 0)))
        .child(&cx, div().h(20).self_stretch().bg(Color::rgb(0, 0, 255)))
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 8> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &col_root,
        Region::new(0, 0, 200, 160),
        &mut motion,
        &mut ops,
        &mut hits,
    );
    assert_eq!(ops.len(), 3);
    // self_center: x = (200 - 60) / 2 = 70
    assert_eq!(ops[0].bounds, Region::new(70, 0, 60, 20));
    // self_end: x = 200 - 60 = 140
    assert_eq!(ops[1].bounds, Region::new(140, 20, 60, 20));
    // self_stretch: w = 200
    assert_eq!(ops[2].bounds, Region::new(0, 40, 200, 20));

    // 2. 验证 items_baseline 在 FlexDir::Row 下的基线对齐
    let base_root = div()
        .flex_row()
        .w(200)
        .h(60)
        .items_baseline()
        .child(&cx, text("Big").size(24).color(Color::WHITE))
        .child(&cx, text("Small").size(12).color(Color::WHITE))
        .into_element(&cx);
    let mut ops_base: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
    let mut hits_base: SmallVec<HitRegion<'_>, 8> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &base_root,
        Region::new(0, 0, 200, 60),
        &mut motion,
        &mut ops_base,
        &mut hits_base,
    );
    assert_eq!(ops_base.len(), 2);
    // 小字号文本在基线对齐下应自动向下偏移 (y > 0) 以与大字号基线对齐
    assert!(
        ops_base[1].bounds.y > ops_base[0].bounds.y,
        "Smaller text in items_baseline row should have positive y offset to align baseline"
    );

    // 3. 验证 scrollable_auto() 自动测量子项总高度并计算最大滚动范围（无需硬编码 max_scroll）
    struct AutoScrollA11yView;
    impl View for AutoScrollA11yView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .id(700)
                .flex_col()
                .w(120)
                .h(60)
                .scrollable_auto()
                .child(
                    cx,
                    div()
                        .id(701)
                        .w(100)
                        .h(50)
                        .focusable()
                        .bg(Color::rgb(37, 99, 235))
                        .on_click_raw(cx.action(|_this, acx| acx.notify()))
                        .child(cx, text("Submit")),
                )
                .child(cx, div().id(702).w(100).h(50).bg(Color::rgb(16, 185, 129)))
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(120, 60);
    engine.mount_root(AutoScrollA11yView);
    engine.step_frame();
    // 子项总高 50 + 50 = 100，视口高 60 -> 自动计算 max_scroll = 40
    // 尝试向下滚动 999px，应被精确钳制在 40px（首个子项 y = -40）
    assert!(engine.dispatch_event(InputEvent::Scroll {
        x: 20,
        y: 20,
        delta_y: 999,
    }));
    let _ = engine.step_frame();
    assert_eq!(engine.cached_draw_ops()[0].bounds.y, -40);

    // 4. 验证无障碍语义树（Accessibility Semantic Tree）构建
    let a11y_nodes = engine.build_a11y_tree::<16>();
    assert!(a11y_nodes.iter().any(|n| n.role == A11yRole::Window));
    assert!(a11y_nodes
        .iter()
        .any(|n| n.role == A11yRole::ScrollRegion && n.id == 700));
    assert!(a11y_nodes
        .iter()
        .any(|n| n.role == A11yRole::Button && n.id == 701 && n.focusable));
    assert!(a11y_nodes
        .iter()
        .any(|n| n.role == A11yRole::Text && n.label.map(|s| s.as_str()) == Some("Submit")));
    assert_eq!(PointerButton::default(), PointerButton::Left);
    assert_eq!(CursorShape::default(), CursorShape::Default);
}

#[test]
fn test_scoped_id_stack_isolation_and_orthogonal_namespace() {
    let cx = FrameScope::new();
    let root_a = cx.scoped_id(1);
    let root_b = cx.scoped_id(2);
    assert_ne!(root_a, root_b);
    // 全部派生 ID 落在高 bit=1 命名空间，与用户显式小整数 ID 正交零碰撞
    assert!(root_a >= 0x8000_0000);
    assert!(root_b >= 0x8000_0000);

    let id_row0 = cx.with_id(10, |cx| cx.scoped_id(1));
    let id_row1 = cx.with_id(11, |cx| cx.scoped_id(1));
    assert_ne!(id_row0, id_row1);
    assert_ne!(id_row0, root_a);

    // 嵌套作用域顺序敏感且弹栈后完整还原外层作用域
    let nested_ab = cx.with_id(10, |cx| cx.with_id(20, |cx| cx.scoped_id(1)));
    let nested_ba = cx.with_id(20, |cx| cx.with_id(10, |cx| cx.scoped_id(1)));
    assert_ne!(nested_ab, nested_ba);
    assert_eq!(cx.scoped_id(1), root_a);

    // 验证 Div::scoped_id 渲染两个同局部 ID 模板互不撞车且可分别响应点击
    struct ScopedListView {
        clicks: [u32; 2],
    }
    impl View for ScopedListView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut list = div().flex_row().size_full();
            for row in 0..2usize {
                let item = cx.with_id(row as u32, |cx| {
                    div()
                        .scoped_id(cx, 1)
                        .w(80)
                        .h(40)
                        .bg(Color::rgb(40, 80, 120))
                        .on_click(
                            cx,
                            cx.listener(move |this, _ev, _acx| {
                                this.clicks[row] += 1;
                            }),
                        )
                        .into_element(cx)
                });
                list = list.child(cx, item);
            }
            list.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(200, 60);
    let root = engine.mount_root(ScopedListView { clicks: [0, 0] });
    engine.step_frame();
    assert!(engine.dispatch_click(20, 20));
    assert!(engine.dispatch_click(100, 20));
    assert!(engine.dispatch_click(100, 20));
    let st = engine.view(root).unwrap();
    assert_eq!(st.clicks, [1, 2]);
}

#[test]
fn test_ime_preedit_commit_and_focused_bounds() {
    struct ImeEditorView {
        buf: [char; 32],
        len: usize,
        last_seen_preedit_len: usize,
    }
    impl View for ImeEditorView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            self.last_seen_preedit_len = cx.ime_preedit().map(|s| s.len()).unwrap_or(0);
            div()
                .flex_col()
                .size_full()
                .p(10)
                .child(
                    cx,
                    div()
                        .id(401)
                        .w(180)
                        .h(32)
                        .focusable()
                        .bg(Color::rgb(30, 41, 59))
                        .on_key_input(cx.listener(|this, key, _acx| {
                            if let KeyCode::Char(ch) = key {
                                if this.len < this.buf.len() {
                                    this.buf[this.len] = ch;
                                    this.len += 1;
                                }
                            }
                        }))
                        .child(cx, text(cx.ime_preedit().unwrap_or(""))),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 80);
    let root = engine.mount_root(ImeEditorView {
        buf: ['\0'; 32],
        len: 0,
        last_seen_preedit_len: 0,
    });
    engine.step_frame();

    // 焦点热区边界可用于平台定位 IME 候选框
    assert!(engine.has_focused_text_input());
    let bounds = engine.focused_bounds().expect("expected focused input bounds");
    assert_eq!((bounds.x, bounds.y, bounds.w, bounds.h), (10, 10, 180, 32));

    // 1. IME 激活与预编辑（拼音组合串）同步到 FrameScope
    engine.dispatch_event(InputEvent::ImeEnabled);
    assert!(engine.ime_enabled());
    assert!(engine.dispatch_ime_preedit("zhongwen", Some((0, 8))));
    assert_eq!(engine.ime_preedit(), "zhongwen");
    assert_eq!(engine.ime_preedit_state().cursor(), Some((0, 8)));
    engine.step_frame();
    assert_eq!(engine.view(root).unwrap().last_seen_preedit_len, 8);

    // 2. 提交中文词组：清空 preedit 并逐字送入聚焦输入框
    assert!(engine.dispatch_ime_commit("你好世界"));
    assert_eq!(engine.ime_preedit(), "");
    let st = engine.view(root).unwrap();
    assert_eq!(st.len, 4);
    assert_eq!(&st.buf[..4], &['你', '好', '世', '界']);

    // 3. 超长多字节 UTF-8 预编辑安全截断在有效字符边界（不 panic）
    let long_cjk = "一二三四五六七八九十一二三四五六七八九十一二三四五";
    let p = ImePreedit::from_str(long_cjk, Some((0, 999)));
    assert!(p.as_str().len() <= 64);
    assert!(p.as_str().chars().count() >= 21);

    // 4. IME 关闭清空残留 preedit
    assert!(engine.dispatch_ime_preedit("abc", None));
    assert!(engine.dispatch_event(InputEvent::ImeDisabled));
    assert!(!engine.ime_enabled());
    assert_eq!(engine.ime_preedit(), "");
}

#[test]
fn test_a11y_hierarchy_and_incremental_diff() {
    struct A11yApp {
        show_extra: bool,
    }
    impl View for A11yApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut list = div()
                .id(800)
                .flex_col()
                .w(200)
                .h(120)
                .scrollable(120)
                .child(
                    cx,
                    div()
                        .id(801)
                        .w(120)
                        .h(32)
                        .focusable()
                        .on_click(cx, cx.listener(|_this, _ev, _acx| {}))
                        .child(cx, text("Save")),
                );
            if self.show_extra {
                list = list.child(
                    cx,
                    div()
                        .id(802)
                        .w(120)
                        .h(32)
                        .focusable()
                        .on_click(cx, cx.listener(|_this, _ev, _acx| {}))
                        .child(cx, text("Cancel")),
                );
            }
            div().size_full().child(cx, list).into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 160);
    let root = engine.mount_root(A11yApp { show_extra: true });
    engine.step_frame();

    let snap1 = {
        let tree1 = engine.build_a11y_tree::<16>();
        let win = tree1.iter().find(|n| n.role == A11yRole::Window).unwrap();
        let scroll = tree1.iter().find(|n| n.id == 800).unwrap();
        let btn1 = tree1.iter().find(|n| n.id == 801).unwrap();
        let txt_save = tree1
            .iter()
            .find(|n| n.role == A11yRole::Text && n.label.map(|s| s.as_str()) == Some("Save"))
            .unwrap();

        // 1. 验证四层父子归属关系：Window(0) -> ScrollRegion(800) -> Button(801) -> Text("Save")
        assert_eq!(win.parent_id, None);
        assert_eq!(scroll.parent_id, Some(0));
        assert_eq!(btn1.parent_id, Some(800));
        assert_eq!(txt_save.parent_id, Some(801));
        assert_eq!(children_of(tree1.as_slice(), 800).count(), 2);

        snapshot_a11y_tree::<16>(tree1.as_slice())
    };

    // 2. 静息帧跨帧增量 Diff 必须为空（零 IPC 噪声）
    {
        let tree1_again = engine.build_a11y_tree::<16>();
        let upd_idle = diff_a11y_snapshot::<16>(snap1.as_slice(), tree1_again.as_slice());
        assert!(upd_idle.is_empty());
    }

    // 3. 按 Tab 切换焦点至 Button(801)，验证增量 Diff 精确捕捉焦点迁移
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame();
    let snap2 = {
        let tree2 = engine.build_a11y_tree::<16>();
        let upd_focus = diff_a11y_snapshot::<16>(snap1.as_slice(), tree2.as_slice());
        assert!(!upd_focus.is_empty());
        assert!(upd_focus.focus_changed);
        assert_eq!(upd_focus.focus_id, Some(801));
        assert!(upd_focus.updated_nodes.iter().any(|n| n.id == 801 && n.focused));
        assert!(upd_focus.removed_ids.is_empty());

        snapshot_a11y_tree::<16>(tree2.as_slice())
    };

    // 4. 移除 Button(802)，验证 removed_ids 精确报告 802
    engine.update_view(root, |v| v.show_extra = false);
    engine.step_frame();
    {
        let tree3 = engine.build_a11y_tree::<16>();
        let upd_remove = diff_a11y_snapshot::<16>(snap2.as_slice(), tree3.as_slice());
        assert!(upd_remove.removed_ids.as_slice().contains(&802));
    }
}

#[test]
fn test_mouse_drag_does_not_scroll_page_while_touch_and_on_drag_work() {
    struct ScrollAndSliderView {
        slider_drag_count: u32,
        back_count: u32,
    }

    impl View for ScrollAndSliderView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_drag = cx.listener(|this, _ev: DragEvent, _| {
                this.slider_drag_count += 1;
            });
            let on_back = cx.action(|this, _| {
                this.back_count += 1;
            });
            div()
                .id(1)
                .w(240)
                .h(160)
                .flex_col()
                .on_navigate_back(on_back)
                .child(
                    cx,
                    div()
                        .id(20)
                        .w(240)
                        .h(30)
                        .bg(Color::rgb(64, 64, 64))
                        .on_drag(cx, on_drag),
                )
                .child(
                    cx,
                    div()
                        .id(30)
                        .w(240)
                        .h(130)
                        .flex_col()
                        .scrollable(200)
                        .overflow_hidden()
                        .child(cx, div().id(31).w(240).h(320).bg(Color::BLACK)),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 160);
    let root = engine.mount_root(ScrollAndSliderView {
        slider_drag_count: 0,
        back_count: 0,
    });
    engine.step_frame();

    // 1. 非触摸鼠标：在可滚动页面空白区按住左键上下拖动，绝不应拖动页面也不应产生惯性滚动
    assert!(!engine.gesture_config().mouse_drag_scroll);
    engine.dispatch_mouse_down(120, 100);
    assert!(!engine.dispatch_pointer_move(120, 50));
    assert!(!engine.dispatch_pointer_up(120, 50));
    assert_eq!(
        engine.motion_mut().scroll_offset(30),
        0,
        "Mouse left-button drag on blank scrollable page must NOT scroll the page"
    );

    // 2. 非触摸鼠标：从窗口左边缘向右拖拽，绝不应误触发移动端边缘返回手势
    engine.dispatch_mouse_down(4, 100);
    let _ = engine.dispatch_pointer_move(80, 100);
    let _ = engine.dispatch_pointer_up(80, 100);
    assert_eq!(
        engine.view(root).unwrap().back_count,
        0,
        "Mouse edge drag must NOT trigger mobile edge-back gesture"
    );

    // 3. 非触摸鼠标：在显式注册了 `.on_drag(...)` 的滑块控件上拖拽必须正常工作
    assert!(engine.dispatch_mouse_down(60, 15));
    assert!(engine.dispatch_pointer_move(120, 15));
    let _ = engine.dispatch_pointer_up(120, 15);
    assert!(engine.view(root).unwrap().slider_drag_count >= 2);

    // 4. 非触摸鼠标：滚轮滚动必须正常工作
    assert!(engine.dispatch_event(InputEvent::Scroll {
        x: 120,
        y: 100,
        delta_y: 40,
    }));
    assert_eq!(engine.motion_mut().scroll_offset(30), 40);

    // 5. 触控屏（Touch）：按住拖动可滚动页面正常跟手滚动，且左边缘右滑返回正常触发
    engine.dispatch_touch_down(120, 100);
    assert!(engine.dispatch_pointer_move(120, 70));
    assert_eq!(engine.motion_mut().scroll_offset(30), 70);
    let _ = engine.dispatch_pointer_up(120, 70);

    engine.dispatch_touch_down(4, 100);
    let _ = engine.dispatch_pointer_move(80, 100);
    assert!(engine.dispatch_pointer_up(80, 100));
    assert_eq!(engine.view(root).unwrap().back_count, 1);

    // 6. 显式开启 `mouse_drag_scroll = true` 时（如地图/白板视图），鼠标按住拖动亦可按需启用
    let mut cfg = engine.gesture_config();
    cfg.mouse_drag_scroll = true;
    engine.set_gesture_config(cfg);
    engine.dispatch_mouse_down(120, 100);
    assert!(engine.dispatch_pointer_move(120, 80));
    assert_eq!(engine.motion_mut().scroll_offset(30), 90);
    let _ = engine.dispatch_pointer_up(120, 80);
}
