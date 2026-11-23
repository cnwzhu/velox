//! `velox-core` 布局引擎、内存池与 Pass-4-Only 快路径测试套件。

use velox_core::prelude::*;

#[test]
fn test_style_size_is_24_bytes() {
    assert_eq!(core::mem::size_of::<Style>(), 24);
    assert!(core::mem::size_of::<Handle<u32>>() <= 4);
}

#[test]
fn test_scratch_arena_alloc_and_fmt() {
    let mut cx = FrameScope::new();
    {
        let slice = cx.arena().alloc_iter(0..5usize);
        assert_eq!(slice, &[0, 1, 2, 3, 4]);
        let s = fmt!(cx, "Temp: {}.{}°C", 23, 5);
        assert_eq!(s.as_str(), "Temp: 23.5°C");
        assert!(cx.arena().used_bytes() > 0);
    }
    cx.arena_mut().rewind();
    assert_eq!(cx.arena().used_bytes(), 0);
}

#[test]
fn test_flex_wrap_and_min_max_constraints() {
    use velox_core::solver::{solve_and_flatten, HitRegion};

    let cx = FrameScope::new();
    let root = div()
        .flex_row()
        .flex_wrap()
        .w(200)
        .gap(10)
        .child(&cx, div().id(1).min_w(80).h(30).bg(Color::WHITE))
        .child(&cx, div().id(2).min_w(80).h(30).bg(Color::WHITE))
        .child(&cx, div().id(3).min_w(80).h(30).bg(Color::WHITE))
        .child(&cx, div().id(4).min_w(80).h(30).bg(Color::WHITE))
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 32> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 64> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &root,
        Region::new(0, 0, 200, 200),
        &mut motion,
        &mut ops,
        &mut hits,
    );

    assert_eq!(ops.len(), 4);
    // 第 1 行：x = 0, 90; y = 0
    assert_eq!(ops[0].bounds, Region::new(0, 0, 80, 30));
    assert_eq!(ops[1].bounds, Region::new(90, 0, 80, 30));
    // 第 2 行（自动折行）：x = 0, 90; y = 40 (30 + gap 10)
    assert_eq!(ops[2].bounds, Region::new(0, 40, 80, 30));
    assert_eq!(ops[3].bounds, Region::new(90, 40, 80, 30));
}

#[test]
fn test_single_frame_strip_layout_cache() {
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

    let mut engine = VeloxEngine::new(320, 240);
    engine.mount_root(BreathingView);

    // 执行一帧布局与脏区计算
    let damage = engine.step_frame();
    assert_eq!(damage.len(), 1);
    assert_eq!(engine.layout_solve_count(), 1);

    // 模拟 MCU 15 个 16 行条带分批光栅化：应 100% 复用 ScratchArena 中的缓存 DrawOp，不重复求解布局
    let mut strip_buf = [Rgb565(0); 320 * 16];
    for strip_idx in 0..15 {
        let y = strip_idx * 16;
        engine.rasterize_strip(
            Region::new(0, y, 320, 16),
            0,
            y,
            320,
            &mut strip_buf,
        );
    }
    assert_eq!(engine.layout_solve_count(), 1);
}

#[test]
fn test_multiline_text_wrapping_and_ellipsis() {
    use velox_core::solver::{solve_and_flatten, HitRegion};

    let cx = FrameScope::new();
    // 宽度限制为 80px，长文本开启自动折行后高度应扩展为多行（>= 32px）
    let root = div()
        .flex_col()
        .w(80)
        .child(
            &cx,
            text("Hello Velox Embedded GUI Framework")
                .size(16)
                .text_center(),
        )
        .child(
            &cx,
            text("Single line with ellipsis truncation")
                .size(16)
                .nowrap()
                .ellipsis(),
        )
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 32> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 64> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &root,
        Region::new(0, 0, 80, 200),
        &mut motion,
        &mut ops,
        &mut hits,
    );

    assert_eq!(ops.len(), 2);
    assert!(ops[0].bounds.h >= 40);
    assert_eq!(ops[1].bounds.h, 20);

    let mut pixels = [Rgb565(0); 80 * 120];
    rasterize_region(ops.as_slice(), Region::new(0, 0, 80, 120), 0, 0, 80, &mut pixels);
    assert!(pixels.iter().any(|p| p.0 != 0));
}

#[test]
fn test_grid_layout_and_soft_shadow_rasterization() {
    use velox_core::solver::{solve_and_flatten, HitRegion};

    let cx = FrameScope::new();
    let mut grid = div()
        .id(1)
        .grid(3)
        .w(100)
        .p(5)
        .gap(6)
        .shadow(4, 2, 2, Color::rgba(0, 0, 0, 180))
        .bg(Color::rgb(40, 40, 50));

    for i in 0..6u32 {
        grid = grid.child(&cx, div().id(10 + i).h(20).bg(Color::rgb(200, 100, 50)));
    }
    let root = grid.into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 16> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 64> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &root,
        Region::new(0, 0, 120, 100),
        &mut motion,
        &mut ops,
        &mut hits,
    );

    // 1 个软阴影 Quad + 1 个容器背景 Quad + 6 个网格子项 Quad = 8 个指令
    assert_eq!(ops.len(), 8);
    // 第 0 个是软阴影 Quad，第 1 个是容器本体，第 2..8 是 3 列 x 2 行网格项
    // inner_w = 100 - 10 = 90, total_gap = 12 => 3 列总宽 78 => 每列宽 26px
    assert_eq!(ops[2].bounds, Region::new(5, 5, 26, 20));
    assert_eq!(ops[3].bounds, Region::new(37, 5, 26, 20));
    assert_eq!(ops[4].bounds, Region::new(69, 5, 26, 20));
    // 第二行 y = 5 + 20 + 6 = 31
    assert_eq!(ops[5].bounds, Region::new(5, 31, 26, 20));

    let mut fb = [Xrgb8888(0); 120 * 100];
    rasterize_region(ops.as_slice(), Region::new(0, 0, 120, 100), 0, 0, 120, &mut fb);
    // 验证软阴影在容器右下方外部 (x=101, y=20) 产生了半透明衰减像素
    assert_ne!(fb[20 * 120 + 101].0, 0);
}

#[test]
fn test_pct_layout_z_index_auto_id_and_navigate_back() {
    use velox_core::solver::{solve_and_flatten, HitRegion};

    let cx = FrameScope::new();
    let root = div()
        .flex_row()
        .w(200)
        .h(100)
        // 子项 0：w_pct(50) -> 100px, h_pct(80) -> 80px, z_index = 2（应排在子项 1 之后绘制）
        .child(
            &cx,
            div()
                .key("left_pane")
                .w_pct(50)
                .h_pct(80)
                .z_index(2)
                .bg(Color::rgb(255, 0, 0)),
        )
        // 子项 1：w_pct(50) -> 100px, h_pct(50) -> 50px, z_index = 1（应先于子项 0 绘制）
        .child(
            &cx,
            div()
                .key("right_pane")
                .w_pct(50)
                .h_pct(50)
                .z_index(1)
                .bg(Color::rgb(0, 255, 0)),
        )
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 16> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 16> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &root,
        Region::new(0, 0, 200, 100),
        &mut motion,
        &mut ops,
        &mut hits,
    );

    assert_eq!(ops.len(), 2);
    // z_index = 1 的 right_pane 先绘制 (x=100, w=100, h=50)
    assert_eq!(ops[0].bounds, Region::new(100, 0, 100, 50));
    // z_index = 2 的 left_pane 后绘制（位于上层，x=0, w=100, h=80）
    assert_eq!(ops[1].bounds, Region::new(0, 0, 100, 80));

    // 验证 Scope::action 与 on_navigate_back 专属处理器（不污染屏幕中心右滑组件）
    struct BackAndActionApp {
        back_count: u32,
        swipe_count: u32,
        action_count: u32,
    }
    impl View for BackAndActionApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .on_navigate_back(cx.action(|this, acx| {
                    this.back_count += 1;
                    acx.stop_propagation();
                    acx.notify();
                }))
                .on_swipe(
                    cx,
                    cx.listener(|this, _dir, acx| {
                        this.swipe_count += 1;
                        acx.notify();
                    }),
                )
                .child(
                    cx,
                    div().w(60).h(30).bg(Color::WHITE).on_click_raw(
                        cx.action(|this, acx| {
                            this.action_count += 1;
                            acx.notify();
                        }),
                    ),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(200, 200);
    let h = engine.mount_root(BackAndActionApp {
        back_count: 0,
        swipe_count: 0,
        action_count: 0,
    });
    engine.step_frame();

    assert!(engine.dispatch_navigate_back());
    assert_eq!(engine.view(h).unwrap().back_count, 1);
    assert_eq!(engine.view(h).unwrap().swipe_count, 0);

    assert!(engine.dispatch_click(10, 10));
    assert_eq!(engine.view(h).unwrap().action_count, 1);
}

#[test]
fn test_nested_overlay_vertical_layout_and_z_index_isolation() {
    use velox_core::solver::{solve_and_flatten, HitRegion};

    let cx = FrameScope::new();
    let root = div()
        .flex_col()
        .w(200)
        .h(200)
        // 普通子项 0：设置 z_index(5)，但不应隐式变成 overlay（不得脱离常规流）
        .child(&cx, div().w(100).h(40).z_index(5).bg(Color::rgb(255, 0, 0)))
        // 普通子项 1：紧随子项 0 之后，y 应为 40（证明子项 0 未脱离文档流）
        .child(&cx, div().w(100).h(40).z_index(1).bg(Color::rgb(0, 255, 0)))
        // Overlay 弹窗：内部包含垂直排列的两个子元素（验证 C1 修复：overlay 内部多层嵌套子节点的 y/h 正确计算）
        .child(
            &cx,
            div()
                .overlay()
                .flex_col()
                .w(120)
                .h(80)
                .gap(10)
                .bg(Color::rgb(20, 20, 30))
                .child(&cx, div().w(100).h(25).bg(Color::rgb(0, 0, 255)))
                .child(&cx, div().w(100).h(30).bg(Color::rgb(255, 255, 0))),
        )
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 16> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 16> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &root,
        Region::new(0, 0, 200, 200),
        &mut motion,
        &mut ops,
        &mut hits,
    );

    // 应有 5 个绘制指令：2 个普通流子项 + 1 个 overlay 容器 + 2 个 overlay 内部子项
    assert_eq!(ops.len(), 5);
    // 普通流中 z_index(1) 的第二个子项 y=40，先于 z_index(5) 的第一个子项绘制
    assert_eq!(ops[0].bounds, Region::new(0, 40, 100, 40));
    // 普通流中 z_index(5) 的第一个子项 y=0，仍在文档流内
    assert_eq!(ops[1].bounds, Region::new(0, 0, 100, 40));
    // Overlay 容器及其内部两个子项（验证 C1 修复：内部子项 y=80, h=25 与 y=115, h=30 均被正确赋值而非 h=0）
    assert_eq!(ops[2].bounds, Region::new(0, 80, 120, 80));
    assert_eq!(ops[3].bounds, Region::new(0, 80, 100, 25));
    assert_eq!(ops[4].bounds, Region::new(0, 115, 100, 30));
}


#[test]
fn test_pod_pool_generation_aba_and_flex_shrink_and_row_scroll() {
    use velox_core::solver::{solve_and_flatten, HitRegion};

    struct LeafView(u32);
    impl View for LeafView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div().w(40).h(20).into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(200, 100);
    let h1 = engine.insert_view(LeafView(1));
    assert_eq!(engine.view(h1).unwrap().0, 1);
    assert!(engine.remove_view(h1));
    // 复用同一槽位创建新视图，旧句柄 h1 因 generation 不匹配必须返回 None（防止 ABA 悬垂访问）
    let h2 = engine.insert_view(LeafView(2));
    assert_eq!(h1.pod_id(), h2.pod_id());
    assert_ne!(h1.generation(), h2.generation());
    assert!(engine.view(h1).is_none());
    assert_eq!(engine.view(h2).unwrap().0, 2);

    // 1) 验证非滚动 Row 的 Flex-Shrink：在 100px 宽的 Row 中放入两个 w(80).min_w(20) 的子项，自动按可缩减空间等比收缩为 50px + 50px
    let cx = FrameScope::new();
    let shrink_root = div()
        .flex_row()
        .w(100)
        .h(40)
        .child(&cx, div().w(80).min_w(20).h(40).bg(Color::rgb(255, 0, 0)))
        .child(&cx, div().w(80).min_w(20).h(40).bg(Color::rgb(0, 0, 255)))
        .into_element(&cx);

    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 8> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &shrink_root,
        Region::new(0, 0, 100, 40),
        &mut motion,
        &mut ops,
        &mut hits,
    );
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].bounds, Region::new(0, 0, 50, 40));
    assert_eq!(ops[1].bounds, Region::new(50, 0, 50, 40));

    // 2) 验证可滚动 Row 横向滚动偏移（scrollable + set_scroll_offset(99, 20) 不收缩子项并平移 -20px）
    let scroll_root = div()
        .id(99)
        .flex_row()
        .w(100)
        .h(40)
        .scrollable(100)
        .child(&cx, div().w(80).h(40).bg(Color::rgb(255, 0, 0)))
        .child(&cx, div().w(80).h(40).bg(Color::rgb(0, 0, 255)))
        .into_element(&cx);

    motion.set_scroll_offset(99, 20);
    let mut ops2: SmallVec<DrawOp<'_>, 8> = SmallVec::new();
    let mut hits2: SmallVec<HitRegion<'_>, 8> = SmallVec::new();
    solve_and_flatten(
        cx.arena(),
        &scroll_root,
        Region::new(0, 0, 100, 40),
        &mut motion,
        &mut ops2,
        &mut hits2,
    );

    assert_eq!(ops2.len(), 2);
    assert_eq!(ops2[0].bounds, Region::new(-20, 0, 80, 40));
    assert_eq!(ops2[1].bounds, Region::new(60, 0, 80, 40));
}

#[test]
fn test_deep_subview_chain_without_depth_limit_and_cycle_guard() {
    struct ChainNode {
        next: Option<Handle<ChainNode>>,
        leaf_color: Option<Color>,
    }

    impl View for ChainNode {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut d = div().size_full();
            if let Some(col) = self.leaf_color {
                d = d.bg(col);
            }
            if let Some(next_h) = self.next {
                d = d.child(cx, next_h);
            }
            d.into_element(cx)
        }
    }

    // 构造 12 层嵌套 SubView（超过原先硬编码的 8 层上限），验证最底层叶节点依然完整渲染
    let mut engine = VeloxEngine::new(64, 64);
    let target_color = Color::rgb(12, 234, 56);
    let mut current = engine.insert_view(ChainNode {
        next: None,
        leaf_color: Some(target_color),
    });
    for _ in 0..11 {
        current = engine.insert_view(ChainNode {
            next: Some(current),
            leaf_color: None,
        });
    }
    let root = ChainNode {
        next: Some(current),
        leaf_color: None,
    };
    let root_handle = engine.mount_root(root);
    let _ = engine.step_frame();
    let mut fb = [Rgb565(0); 64 * 64];
    engine.rasterize_region(Region::new(0, 0, 64, 64), &mut fb);
    assert_eq!(
        fb[0],
        Rgb565::from_color(target_color),
        "12-layer deep SubView chain must render leaf node without depth truncation"
    );

    // 验证环形 SubView 引用 (root -> current -> ... -> root) 由 expanding 标记拦截而不会无限递归
    let leaf_check = engine.insert_view(ChainNode {
        next: Some(root_handle),
        leaf_color: Some(Color::rgb(200, 10, 20)),
    });
    if let Some(r) = engine.view_mut(root_handle) {
        r.next = Some(leaf_check);
    }
    let _ = engine.step_frame();
    engine.rasterize_region(Region::new(0, 0, 64, 64), &mut fb);
    assert_eq!(fb[0], Rgb565::from_color(Color::rgb(200, 10, 20)));
}

#[test]
fn test_pass4_only_fast_path_for_hover_scroll_and_animation() {
    struct InteractiveScrollApp;
    impl View for InteractiveScrollApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .id(500)
                .flex_col()
                .size_full()
                .scrollable(120)
                .child(
                    cx,
                    div()
                        .id(501)
                        .w(120)
                        .h(40)
                        .bg(Color::rgb(30, 64, 175))
                        .hover_bg(Color::rgb(59, 130, 246))
                        .active_bg(Color::rgb(29, 78, 216)),
                )
                .child(
                    cx,
                    div()
                        .id(502)
                        .w(120)
                        .h(40)
                        .bg(Color::rgb(16, 185, 129))
                        .ping_pong_bg(Color::rgb(245, 158, 11), Duration::from_millis(200)),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 160);
    let root = engine.mount_root(InteractiveScrollApp);

    // 首帧执行完整布局求解 (Pass 1~4)
    engine.step_frame();
    assert_eq!(engine.layout_solve_count(), 1);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.fast_paint_pass_count(), 0);

    // 1. 指针移入触发 hover_bg 变色：仅走 Pass-4 快径，0 次 View::render，0 次 Pass 1~3
    engine.dispatch_pointer_move(20, 20);
    let d_hover = engine.step_frame();
    assert!(!d_hover.is_empty());
    assert_eq!(engine.layout_solve_count(), 1);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.fast_paint_pass_count(), 1);

    // 2. 滚轮滚动触发偏移变更：仅走 Pass-4 快径
    engine.scroll(500, 24);
    let d_scroll = engine.step_frame();
    assert!(!d_scroll.is_empty());
    assert_eq!(engine.layout_solve_count(), 1);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.fast_paint_pass_count(), 2);

    // 3. Ping-Pong 呼吸动画插值帧：仅走 Pass-4 快径
    let d_anim = engine.step_frame_dt(50);
    assert!(!d_anim.is_empty());
    assert_eq!(engine.layout_solve_count(), 1);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.fast_paint_pass_count(), 3);

    // 4. PointerLeave 事件清除悬停高亮：仅走 Pass-4 快径（滚动 24px 后 501 位于 y=-24..16，先移入 (20, 10) 再触发 PointerLeave）
    engine.dispatch_pointer_move(20, 10);
    let _ = engine.step_frame();
    let solves_before_leave = engine.layout_solve_count();
    let fast_before_leave = engine.fast_paint_pass_count();
    assert!(engine.dispatch_event(InputEvent::PointerLeave));
    let d_leave = engine.step_frame();
    assert!(!d_leave.is_empty());
    assert_eq!(engine.layout_solve_count(), solves_before_leave);
    assert_eq!(engine.fast_paint_pass_count(), fast_before_leave + 1);
}

#[test]
fn test_subview_relayout_boundary_in_place_and_clean_pass_cache() {
    struct StaticSidebarView;
    impl View for StaticSidebarView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .id(801)
                .w(60)
                .h_full()
                .bg(Color::rgb(30, 41, 59))
                .child(cx, text("NAV").size(12).color(Color::WHITE))
                .into_element(cx)
        }
    }

    struct AutoBadgeView {
        expanded: Signal<bool>,
    }
    impl View for AutoBadgeView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let bw = if self.expanded.get() { 80 } else { 40 };
            div()
                .id(802)
                .w(bw)
                .h(30)
                .bg(Color::rgb(16, 185, 129))
                .into_element(cx)
        }
    }

    struct FixedCardView {
        counter: Signal<i32>,
    }
    impl View for FixedCardView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let val = self.counter.get();
            let bar_w = (20 + (val * 10)).clamp(10, 90) as i16;
            div()
                .id(803)
                .flex_col()
                .w(100)
                .h(50)
                .p(4)
                .bg(Color::rgb(59, 130, 246))
                .child(
                    cx,
                    div()
                        .id(804)
                        .w(bar_w)
                        .h(12)
                        .bg(Color::rgb(250, 204, 21)),
                )
                .child(
                    cx,
                    text(fmt!(cx, "V={}", val))
                        .size(12)
                        .color(Color::WHITE),
                )
                .into_element(cx)
        }
    }

    struct FullContentView {
        mode: Signal<i32>,
    }
    impl View for FullContentView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let m = self.mode.get();
            div()
                .id(805)
                .w_full()
                .h_full()
                .bg(Color::rgb(15, 23, 42))
                .child(
                    cx,
                    div()
                        .id(806)
                        .w(if m == 0 { 30 } else { 70 })
                        .h(20)
                        .bg(Color::rgb(236, 72, 153)),
                )
                .into_element(cx)
        }
    }

    struct WorkspaceRoot {
        sidebar: Handle<StaticSidebarView>,
        badge: Handle<AutoBadgeView>,
        card: Handle<FixedCardView>,
        content: Handle<FullContentView>,
    }
    impl View for WorkspaceRoot {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_row()
                .w_full()
                .h_full()
                .child(cx, self.sidebar)
                .child(cx, self.badge)
                .child(cx, self.card)
                .child(cx, self.content)
                .into_element(cx)
        }
    }

    let expanded = Signal::new(false);
    let counter = Signal::new(1);
    let mode = Signal::new(0);

    let mut engine = VeloxEngine::new(320, 120);
    let sidebar = engine.insert_view(StaticSidebarView);
    let badge = engine.insert_view(AutoBadgeView { expanded });
    let card = engine.insert_view(FixedCardView { counter });
    let content = engine.insert_view(FullContentView { mode });
    let root = engine.mount_root(WorkspaceRoot {
        sidebar,
        badge,
        card,
        content,
    });

    // 第 1 帧：全量渲染与求解
    let _ = engine.step_frame();
    assert_eq!(engine.layout_solve_count(), 1);
    assert_eq!(engine.partial_relayout_count(), 0);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(sidebar), 1);
    assert_eq!(engine.pod_render_count(card), 1);

    let bar_op_f1 = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(804))
        .expect("bar op 804 should exist")
        .bounds;
    assert_eq!(bar_op_f1.x, 60 + 40 + 4);
    assert_eq!(bar_op_f1.w, 30);

    // 第 2 帧：仅修改具有固定宽高 w(100).h(50) 的 FixedCardView 内部状态
    // 应命中 Relayout Boundary 原地局部重排（跳过全局 build_layout_tree 与全局 Pass 1~3）
    counter.set(4);
    let _ = engine.step_frame();
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(sidebar), 1);
    assert_eq!(engine.pod_render_count(badge), 1);
    assert_eq!(engine.pod_render_count(card), 2);
    assert_eq!(engine.pod_render_count(content), 1);
    assert_eq!(
        engine.layout_solve_count(),
        1,
        "Fixed-size SubView update should not trigger full layout solve"
    );
    assert_eq!(
        engine.partial_relayout_count(),
        1,
        "Fixed-size SubView update should execute in-place partial relayout"
    );
    assert!(engine.relayout_boundary_hits() >= 1);

    let bar_op_f2 = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(804))
        .unwrap()
        .bounds;
    assert_eq!(bar_op_f2.x, 60 + 40 + 4);
    assert_eq!(bar_op_f2.w, 60);

    // 第 3 帧：仅修改具有 Length::Full 宽高的 FullContentView 内部状态
    // 同样应命中 Relayout Boundary 原地局部重排
    mode.set(1);
    let _ = engine.step_frame();
    assert_eq!(engine.layout_solve_count(), 1);
    assert_eq!(engine.partial_relayout_count(), 2);
    let inner_content_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(806))
        .unwrap()
        .bounds;
    assert_eq!(inner_content_op.w, 70);

    // 第 4 帧：修改 AutoBadgeView 使其外部宽度从 40px 变为 80px（破坏局部边界）
    // 应自动回退到全局布局，但未脏的 sidebar 与 card 应复用其缓存的 LayoutNode 子树并平移 card 的 x 坐标 (+40px)
    let hits_before = engine.relayout_boundary_hits();
    expanded.set(true);
    let _ = engine.step_frame();
    assert_eq!(engine.layout_solve_count(), 2);
    assert_eq!(engine.pod_render_count(sidebar), 1);
    assert_eq!(engine.pod_render_count(card), 2);
    assert!(
        engine.relayout_boundary_hits() >= hits_before + 2,
        "Clean sibling SubViews (sidebar & card) should hit Pass 1~3 layout cache during full solve"
    );

    let bar_op_f4 = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(804))
        .unwrap()
        .bounds;
    assert_eq!(
        bar_op_f4.x,
        60 + 80 + 4,
        "Clean FixedCardView subtree x should be shifted by +40px when sibling AutoBadgeView expands"
    );
    assert_eq!(bar_op_f4.w, 60);
}

#[test]
fn test_pass4_only_no_arena_leak_and_scroll_zero_clamp_and_gesture_dedup() {
    static DRAG_FIRES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    static SUBVIEW_RAW_CLICKS: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);

    struct SubBtn {
        count: u32,
    }
    impl View for SubBtn {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let c = self.count;
            div()
                .id(910)
                .w(60)
                .h(24)
                .bg(if c % 2 == 0 {
                    Color::rgb(40, 80, 120)
                } else {
                    Color::rgb(120, 80, 40)
                })
                .on_click_raw(cx.arena().alloc(|acx: &mut ActionScope| {
                    SUBVIEW_RAW_CLICKS.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
                    acx.notify();
                }))
                .into_element(cx)
        }
    }

    struct AuditView {
        sub: Handle<SubBtn>,
    }
    impl View for AuditView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_col()
                .w(240)
                .h(200)
                .gap(4)
                .child(
                    cx,
                    // 1. 内容小于视口 (30 < 80) 的 scrollable_auto 容器：max_scroll == 0
                    div()
                        .id(901)
                        .flex_col()
                        .w(200)
                        .h(80)
                        .scrollable_auto()
                        .child(cx, div().id(902).w(180).h(30).bg(Color::rgb(30, 160, 90))),
                )
                .child(
                    cx,
                    // 2. 绑定 on_drag 的滑块热区：按下+抬起轻点只触发 1 次 on_drag（不会在 PointerUp 重复回退触发第 2 次）
                    div()
                        .id(903)
                        .w(120)
                        .h(24)
                        .hover_bg(Color::rgb(60, 60, 80))
                        .active_bg(Color::rgb(80, 80, 120))
                        .on_drag(
                            cx,
                            cx.listener(|_this, _ev, _acx| {
                                DRAG_FIRES.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
                            }),
                        ),
                )
                .child(cx, self.sub)
                .into_element(cx)
        }
    }

    DRAG_FIRES.store(0, core::sync::atomic::Ordering::SeqCst);
    SUBVIEW_RAW_CLICKS.store(0, core::sync::atomic::Ordering::SeqCst);

    let mut engine = VeloxEngine::new(240, 200);
    let sub = engine.insert_view(SubBtn { count: 0 });
    engine.mount_root(AuditView { sub });
    let _ = engine.step_frame();
    let baseline_arena = engine.last_arena_used_bytes();

    // 连续触发 2000 帧 Pass-4-Only 交互重绘，验证竞技场逻辑占用零无限增长
    for i in 0..2000 {
        let x = if i % 2 == 0 { 20 } else { 200 };
        engine.dispatch_event(InputEvent::PointerMove { x, y: 95 });
        let _ = engine.step_frame();
    }
    assert_eq!(engine.last_arena_used_bytes(), baseline_arena);

    // 验证 max_scroll == 0 时滚轮与拖拽绝不会把子元素滚出原位
    let orig_y = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(902))
        .unwrap()
        .bounds
        .y;
    engine.dispatch_event(InputEvent::Scroll {
        x: 40,
        y: 20,
        delta_y: -120,
    });
    let _ = engine.step_frame();
    let after_scroll_y = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(902))
        .unwrap()
        .bounds
        .y;
    assert_eq!(after_scroll_y, orig_y);

    // 验证绑定 on_drag 的滑块热区轻点时只触发 on_drag 1 次（修复此前 PointerDown + PointerUp 双触发），而直接 dispatch_click 仍可回退触发
    engine.dispatch_event(InputEvent::PointerDown { x: 30, y: 95 });
    engine.dispatch_event(InputEvent::PointerUp { x: 30, y: 95 });
    assert_eq!(DRAG_FIRES.load(core::sync::atomic::Ordering::SeqCst), 1);
    assert!(engine.dispatch_click(30, 95));
    assert_eq!(DRAG_FIRES.load(core::sync::atomic::Ordering::SeqCst), 2);

    // 验证在按钮边缘按下但滑出按钮边界抬起（位移 5px <= slop 8px）不会误触发点击，而在按钮内点击 SubView 会将 acx.notify() 精准路由到子视图而非根视图
    let sub_renders_before = engine.pod_render_count(sub);
    engine.dispatch_event(InputEvent::PointerDown { x: 58, y: 125 });
    engine.dispatch_event(InputEvent::PointerUp { x: 63, y: 125 });
    assert_eq!(
        SUBVIEW_RAW_CLICKS.load(core::sync::atomic::Ordering::SeqCst),
        0
    );

    engine.dispatch_event(InputEvent::PointerDown { x: 20, y: 125 });
    engine.dispatch_event(InputEvent::PointerUp { x: 20, y: 125 });
    assert_eq!(
        SUBVIEW_RAW_CLICKS.load(core::sync::atomic::Ordering::SeqCst),
        1
    );
    let _ = engine.step_frame();
    assert_eq!(engine.pod_render_count(sub), sub_renders_before + 1);
}
