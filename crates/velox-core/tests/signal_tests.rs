//! `velox-core` 响应式信号图（三色 Push-Pull / Early Cutoff / Batch / 侵入式边池）测试套件。

use velox_core::prelude::*;

#[test]
fn test_nsh_signals_computed_memoization_and_effect() {
    use core::sync::atomic::{AtomicU32, Ordering};

    static EFFECT_OBSERVED: AtomicU32 = AtomicU32::new(0);
    EFFECT_OBSERVED.store(0, Ordering::SeqCst);

    let count = signal(10i32);
    let factor = signal(3i32);
    let total = computed(move || count.get() * factor.get());

    // 1. 验证 Computed 惰性求值与多次读取缓存命中（不重复执行计算闭包）
    assert_eq!(total.eval_count(), 0);
    assert_eq!(total.get(), 30);
    assert_eq!(total.eval_count(), 1);
    assert_eq!(total.get(), 30);
    assert_eq!(total.eval_count(), 1);

    // 2. 验证 Effect 创建时自动执行一次并追踪 Signal/Computed 依赖
    let _eff = effect(move || {
        EFFECT_OBSERVED.store(total.get() as u32, Ordering::SeqCst);
    });
    assert_eq!(EFFECT_OBSERVED.load(Ordering::SeqCst), 30);
    assert_eq!(total.eval_count(), 1);

    // 3. 更新上游 Signal：自动使 Computed 失效并立即触发 Effect 重跑
    count.set(20);
    assert_eq!(EFFECT_OBSERVED.load(Ordering::SeqCst), 60);
    assert_eq!(total.eval_count(), 2);
    assert_eq!(total.get(), 60);
    assert_eq!(total.eval_count(), 2);

    // 写入相同值（PartialEq 相等）不应触发重算
    count.set_if_changed(20);
    assert_eq!(total.eval_count(), 2);
}

#[test]
fn test_retained_subview_skip_and_fine_grained_signal_dirty() {
    struct StaticSidebarView;
    impl View for StaticSidebarView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(80)
                .h(200)
                .bg(Color::rgb(30, 41, 59))
                .child(cx, text("Sidebar"))
                .into_element(cx)
        }
    }

    struct ReactiveLeafView {
        sig: Signal<i32>,
    }
    impl View for ReactiveLeafView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let val = self.sig.get();
            let sig = self.sig;
            div()
                .id(77)
                .w(120)
                .h(40)
                .bg(Color::rgb(37, 99, 235))
                .on_click(
                    cx,
                    cx.listener(move |_this, _ev, _acx| {
                        // NSH 风格：直接修改 Signal，无需手动调用 acx.notify()
                        sig.update(|v| *v += 1);
                    }),
                )
                .child(cx, text(fmt!(cx, "Val={}", val)))
                .into_element(cx)
        }
    }

    struct ShellRootView {
        sidebar: Handle<StaticSidebarView>,
        leaf: Handle<ReactiveLeafView>,
    }
    impl View for ShellRootView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_row()
                .size_full()
                .child(cx, self.sidebar)
                .child(cx, self.leaf)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 200);
    let sig = signal(1i32);
    let sidebar = engine.insert_view(StaticSidebarView);
    let leaf = engine.insert_view(ReactiveLeafView { sig });
    let root = engine.mount_root(ShellRootView { sidebar, leaf });

    // 首帧：3 个视图各执行 1 次 render
    engine.step_frame();
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(sidebar), 1);
    assert_eq!(engine.pod_render_count(leaf), 1);
    assert_eq!(engine.subview_cache_hits(), 0);

    // 点击 leaf（位于 x=80..200, y=0..40）触发 Signal 更新（未调用 acx.notify）
    assert!(engine.dispatch_click(100, 20));
    assert_eq!(sig.get(), 2);
    assert!(engine.needs_redraw());

    // 第二帧：仅 ReactiveLeafView 重跑 render (1 -> 2)，ShellRootView 与 StaticSidebarView 100% 命中缓存跳过 render！
    let damage = engine.step_frame();
    assert!(!damage.is_empty());
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(sidebar), 1);
    assert_eq!(engine.pod_render_count(leaf), 2);
    assert!(engine.subview_cache_hits() >= 2);
}

#[test]
fn test_signal_reclaim_generation_and_reentrant_update() {
    let old_ptr_like: Signal<u64> = Signal::new(0xDEAD_BEEF_CAFE_BABEu64);
    assert_eq!(old_ptr_like.get_untracked(), 0xDEAD_BEEF_CAFE_BABEu64);

    // 销毁释放槽位（必须递增代数，防止跨类型双关）
    assert!(old_ptr_like.dispose());

    let reused_u32: Signal<u32> = Signal::new(42);
    assert_eq!(reused_u32.get_untracked(), 42);
    // 旧句柄写入必须静默失效，绝不污染新槽位
    old_ptr_like.set(0x1111_2222_3333_4444);
    assert_eq!(reused_u32.get_untracked(), 42);

    // 验证 Signal::update 闭包内重入读取其他/同一信号不会产生双可变借用崩溃
    let helper: Signal<u32> = Signal::new(8);
    reused_u32.update(|v| {
        *v += helper.get_untracked();
    });
    assert_eq!(reused_u32.get_untracked(), 50);
    reused_u32.dispose();
    helper.dispose();
}

#[test]
fn test_std_heap_pod_drop_and_thread_local_signals() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DROPPED: AtomicUsize = AtomicUsize::new(0);

    // 超过 256B 内联阈值的堆溢出视图，验证 remove_view / 析构时无内存泄漏
    struct LargeHeapView {
        _payload: [u64; 40],
        _owned: std::string::String,
    }
    impl Drop for LargeHeapView {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl View for LargeHeapView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div().w(10).h(10).into_element(cx)
        }
    }

    DROPPED.store(0, Ordering::SeqCst);
    {
        let mut engine = VeloxEngine::new(100, 100);
        let mut handles = std::vec::Vec::new();
        for i in 0..70 {
            handles.push(engine.insert_view(LargeHeapView {
                _payload: [i as u64; 40],
                _owned: std::format!("view-{i}"),
            }));
        }
        for h in handles.drain(..35) {
            assert!(engine.remove_view(h));
        }
        assert_eq!(DROPPED.load(Ordering::SeqCst), 35);
    }
    // Engine 析构时剩余 35 个堆视图也必须全部 Drop
    assert_eq!(DROPPED.load(Ordering::SeqCst), 70);

    // 验证多线程下 Signal 线程局部隔离无数据竞争
    let main_sig = Signal::new(100i32);
    let t = std::thread::spawn(|| {
        let worker_sig = Signal::new(999i32);
        worker_sig.set(1000);
        worker_sig.get_untracked()
    });
    assert_eq!(t.join().unwrap(), 1000);
    assert_eq!(main_sig.get_untracked(), 100);
    main_sig.dispose();
}

#[test]
fn test_signal_diamond_glitch_free_and_early_cutoff() {
    use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

    static LAST_DIAMOND: AtomicI32 = AtomicI32::new(0);
    static EFFECT_HITS: AtomicU32 = AtomicU32::new(0);
    LAST_DIAMOND.store(0, Ordering::SeqCst);
    EFFECT_HITS.store(0, Ordering::SeqCst);

    // 1. 菱形依赖 A -> B, A -> C, (B, C) -> D -> Effect，验证无 Glitch 且每节点仅重算一次
    let a = signal(1i32);
    let b = computed(move || a.get() + 1);
    let c = computed(move || a.get() * 10);
    let d = computed(move || b.get() + c.get());

    let eff = effect(move || {
        LAST_DIAMOND.store(d.get(), Ordering::SeqCst);
        EFFECT_HITS.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(LAST_DIAMOND.load(Ordering::SeqCst), 12);
    assert_eq!(b.eval_count(), 1);
    assert_eq!(c.eval_count(), 1);
    assert_eq!(d.eval_count(), 1);
    assert_eq!(eff.run_count(), 1);

    a.set(5);
    assert_eq!(LAST_DIAMOND.load(Ordering::SeqCst), 56);
    assert_eq!(b.eval_count(), 2);
    assert_eq!(c.eval_count(), 2);
    assert_eq!(d.eval_count(), 2);
    assert_eq!(eff.run_count(), 2);
    assert_eq!(EFFECT_HITS.load(Ordering::SeqCst), 2);

    eff.dispose();
    d.dispose();
    c.dispose();
    b.dispose();
    a.dispose();

    // 2. Early Cutoff 剪枝：A -> is_even -> code -> Effect
    static CUTOFF_VAL: AtomicI32 = AtomicI32::new(0);
    CUTOFF_VAL.store(0, Ordering::SeqCst);

    let raw = signal(2i32);
    let is_even = computed(move || raw.get() % 2 == 0);
    let code = computed(move || if is_even.get() { 100i32 } else { 200i32 });
    let eff2 = effect(move || {
        CUTOFF_VAL.store(code.get(), Ordering::SeqCst);
    });

    assert_eq!(CUTOFF_VAL.load(Ordering::SeqCst), 100);
    assert_eq!(is_even.eval_count(), 1);
    assert_eq!(code.eval_count(), 1);
    assert_eq!(eff2.run_count(), 1);

    // 2 -> 4：仍然是偶数，is_even 重算但值未变 -> Early Cutoff 阻止 code 重算与 eff2 重跑！
    raw.set(4);
    assert_eq!(is_even.eval_count(), 2);
    assert_eq!(code.eval_count(), 1);
    assert_eq!(eff2.run_count(), 1);
    assert_eq!(CUTOFF_VAL.load(Ordering::SeqCst), 100);

    // 4 -> 5：变为奇数 -> 沿链路精准触发重算与 Effect
    raw.set(5);
    assert_eq!(is_even.eval_count(), 3);
    assert_eq!(code.eval_count(), 2);
    assert_eq!(eff2.run_count(), 2);
    assert_eq!(CUTOFF_VAL.load(Ordering::SeqCst), 200);

    eff2.dispose();
    code.dispose();
    is_even.dispose();
    raw.dispose();

    // 3. 自定义 Copy + PartialEq 结构体通过 computed_eq 享受 Early Cutoff
    #[derive(Copy, Clone, PartialEq, Eq)]
    struct Bucket(u8, u8);

    let score = signal(82u8);
    let bucket = computed_eq(move || Bucket(score.get() / 10, 1));
    let grade = computed(move || bucket.get().0 as i32 * 10);
    assert_eq!(grade.get(), 80);
    assert_eq!(bucket.eval_count(), 1);
    assert_eq!(grade.eval_count(), 1);

    // 82 -> 89：仍在 80 分段，bucket 相等 -> grade 零重算
    score.set(89);
    assert_eq!(grade.get(), 80);
    assert_eq!(bucket.eval_count(), 2);
    assert_eq!(grade.eval_count(), 1);

    // 89 -> 91：跨段 -> grade 重算为 90
    score.set(91);
    assert_eq!(grade.get(), 90);
    assert_eq!(bucket.eval_count(), 3);
    assert_eq!(grade.eval_count(), 2);

    grade.dispose();
    bucket.dispose();
    score.dispose();
}

#[test]
fn test_signal_intrusive_edge_pool_unlimited_deps_and_zero_leak() {
    let edges_before = velox_core::signal::active_dep_edge_count();

    // 1. 验证 32 字节 InlineFnStorage 可直接捕获 6 个 Signal 句柄
    let f0 = signal(1i32);
    let f1 = signal(2i32);
    let f2 = signal(3i32);
    let f3 = signal(4i32);
    let f4 = signal(5i32);
    let f5 = signal(6i32);
    let six_sum = computed(move || f0.get() + f1.get() + f2.get() + f3.get() + f4.get() + f5.get());
    assert_eq!(six_sum.get(), 21);
    f5.set(16);
    assert_eq!(six_sum.get(), 31);
    assert!(six_sum.dispose());
    for s in [f0, f1, f2, f3, f4, f5] {
        assert!(s.dispose());
    }

    // 2. 构造 16 个源信号（远超旧版 MAX_DEPS_PER_NODE = 8 上限），验证单节点侵入式依赖边链表追踪全部 16 条边
    let sigs = [
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
        signal(1i32),
    ];
    let sigs_ptr = sigs.as_ptr() as usize;

    let sum_a = computed(move || {
        let slice = unsafe { core::slice::from_raw_parts(sigs_ptr as *const Signal<i32>, 16) };
        let mut total = 0i32;
        for s in slice {
            total += s.get();
        }
        total
    });
    assert_eq!(sum_a.get(), 16);

    // 修改第 16 个依赖（索引 15，旧版上限 8 会静默丢弃此依赖）
    sigs[15].set(100);
    assert_eq!(sum_a.get(), 115);
    assert_eq!(sum_a.eval_count(), 2);

    // 释放所有节点，验证侵入式边池精确回收零泄漏
    assert!(sum_a.dispose());
    for s in sigs {
        assert!(s.dispose());
    }

    assert_eq!(velox_core::signal::active_dep_edge_count(), edges_before);
}

#[test]
fn test_signal_batch_coalesces_effects() {
    use core::sync::atomic::{AtomicI32, Ordering};

    static BATCH_SUM: AtomicI32 = AtomicI32::new(0);
    BATCH_SUM.store(0, Ordering::SeqCst);

    let x = signal(1i32);
    let y = signal(2i32);
    let sum = computed(move || x.get() + y.get());
    let eff = effect(move || {
        BATCH_SUM.store(sum.get(), Ordering::SeqCst);
    });
    assert_eq!(eff.run_count(), 1);
    assert_eq!(BATCH_SUM.load(Ordering::SeqCst), 3);

    batch(|| {
        x.set(10);
        y.set(20);
        // 事务期间尚未触发 Effect
        assert_eq!(eff.run_count(), 1);
    });
    // 事务结束合并只执行 1 次
    assert_eq!(eff.run_count(), 2);
    assert_eq!(sum.eval_count(), 2);
    assert_eq!(BATCH_SUM.load(Ordering::SeqCst), 30);

    eff.dispose();
    sum.dispose();
    x.dispose();
    y.dispose();
}

#[test]
fn test_computed_transitively_subscribes_pod_view() {
    struct StaticSibling;
    impl View for StaticSibling {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div().w(60).h(40).into_element(cx)
        }
    }

    struct DerivedLeaf {
        derived: Computed<i32>,
    }
    impl View for DerivedLeaf {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let v = self.derived.get();
            div()
                .w(120)
                .h(40)
                .child(cx, text(fmt!(cx, "D={}", v)))
                .into_element(cx)
        }
    }

    struct RootShell {
        sibling: Handle<StaticSibling>,
        leaf: Handle<DerivedLeaf>,
    }
    impl View for RootShell {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_row()
                .size_full()
                .child(cx, self.sibling)
                .child(cx, self.leaf)
                .into_element(cx)
        }
    }

    let base = signal(5i32);
    let mid = computed(move || base.get() * 2);
    let top = computed(move || mid.get() + 1);

    let mut engine = VeloxEngine::new(240, 80);
    let sibling = engine.insert_view(StaticSibling);
    let leaf = engine.insert_view(DerivedLeaf { derived: top });
    let root = engine.mount_root(RootShell { sibling, leaf });

    engine.step_frame();
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(sibling), 1);
    assert_eq!(engine.pod_render_count(leaf), 1);

    base.set(9);
    assert!(engine.needs_redraw());
    engine.step_frame();
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(sibling), 1);
    assert_eq!(engine.pod_render_count(leaf), 2);
    assert_eq!(top.get(), 19);

    top.dispose();
    mid.dispose();
    base.dispose();
}

#[test]
fn test_animated_subview_request_frame_preserves_clean_root_and_siblings() {
    struct StaticHeaderView;
    impl View for StaticHeaderView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(240)
                .h(32)
                .bg(Color::rgb(24, 24, 27))
                .child(cx, text("StaticHeader"))
                .into_element(cx)
        }
    }

    struct BlinkingLeafView {
        active: Signal<bool>,
    }
    impl View for BlinkingLeafView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let active = self.active.get();
            if active {
                cx.request_frame();
            }
            let tick = (cx.now_ms() / 16) % 2;
            div()
                .id(88)
                .w(240)
                .h(32)
                .bg(if active && tick == 1 {
                    Color::rgb(59, 130, 246)
                } else {
                    Color::rgb(39, 39, 42)
                })
                .child(cx, text(fmt!(cx, "Tick={}", tick)))
                .into_element(cx)
        }
    }

    struct ShellView {
        header: Handle<StaticHeaderView>,
        blink: Handle<BlinkingLeafView>,
    }
    impl View for ShellView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_col()
                .size_full()
                .child(cx, self.header)
                .child(cx, self.blink)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 100);
    let active = signal(true);
    let header = engine.insert_view(StaticHeaderView);
    let blink = engine.insert_view(BlinkingLeafView { active });
    let root = engine.mount_root(ShellView { header, blink });

    // 首帧：全部 3 个视图各渲染 1 次
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(header), 1);
    assert_eq!(engine.pod_render_count(blink), 1);
    assert!(engine.needs_redraw());

    // 连续推进 3 帧时钟动画：仅 BlinkingLeafView 重渲染，ShellView 与 StaticHeaderView 0 次重跑且命中 L2 原地重排！
    for expected in 2..=4 {
        let _ = engine.step_frame_dt(16);
        assert_eq!(engine.pod_render_count(root), 1);
        assert_eq!(engine.pod_render_count(header), 1);
        assert_eq!(engine.pod_render_count(blink), expected);
    }
    assert!(engine.partial_relayout_count() >= 3);
    assert!(engine.subview_cache_hits() >= 3);

    // 停止动画后下一帧渲染完成并自动回到 L0 0% CPU 休眠
    active.set(false);
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(header), 1);
    assert_eq!(engine.pod_render_count(blink), 5);
    assert!(!engine.needs_redraw(), "must return to L0 idle sleep once request_frame stops");
    active.dispose();
}
