//! `velox-core` 可插拔多调度器异步运行时（`velox_core::async_rt`）全面回归测试。
//!
//! 覆盖：
//! - **全平台 `#![no_std]` 零堆抽象层**（`--no-default-features` 下真实运行）：
//!   - 模拟 **Embassy** 调度器（`SchedulerClock` + `AsyncDriver` + 黏性中断唤醒信号）零堆驱动 `run_async_event_loop`；
//! - **非嵌入式 `std` 可插拔多调度器层**：
//!   1. `VeloxEngine::spawn_for` + `cx.background_spawn` 后台多线程卸载 + `Handle::update` L2 标脏闭环；
//!   2. `Task<T>` RAII `Drop` 自动取消回收、`.detach()` 脱离托管、`Task::ready` 与 `is_cancelled()`；
//!   3. `remove_view` 后复插同槽位 ABA `generation` 安全防护（返回 `None` 绝不误改新视图或 panic）；
//!   4. 交互回调 `acx.spawn` 触发、首帧 0 延迟跑到首个 `.await` 与跨子视图 `Signal<T>` 响应式联动；
//!   5. `acx.defer` / `acx.defer_self` 延迟 FIFO 执行与同视图重入 `acx.update` 安全返回 `None`；
//!   6. 并发 `Timer::after` 最小堆顺序唤醒 + `set_async_waker` 零轮询精准通知；
//!   7. 无 `Spinner` 静默异步任务等待期 `needs_redraw() == false`（0% CPU 休眠）并在到期时瞬间唤醒；
//!   8. `MobileEmbedApp` 与 `WebCanvasApp` 跨平台异步唤醒透传验证；
//!   9. `ManualScheduler` 确定性虚拟时钟（<5ms 内快进 10 分钟异步任务与取消）；
//!   10. 自定义第三方 4 方法 `SchedulerBackend` 运行时热切换与 RAII 取消验证；
//!   11. `StdAsyncDriver` + `run_async_event_loop` 休眠与外部唤醒闭环验证。

mod std_async_tests {
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use velox_core::platform::mobile::MobileEmbedApp;
use velox_core::platform::web::WebCanvasApp;
use velox_core::prelude::*;

struct CounterView {
    value: i32,
    worker_ran_off_ui: bool,
    active_task: Option<Task<()>>,
    signal: Option<Signal<i32>>,
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl CounterView {
    fn new(log: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            value: 0,
            worker_ran_off_ui: false,
            active_task: None,
            signal: None,
            log,
        }
    }
}

impl View for CounterView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let on_spawn = cx.listener(|this, _: ClickEvent, acx| {
            this.log.lock().unwrap().push("click");
            this.active_task = Some(acx.spawn(async move |handle: Handle<Self>, mut cx: AsyncApp| {
                let _ = handle.update(&mut cx, |v, _| {
                    v.log.lock().unwrap().push("fg-before-await");
                    v.value += 1;
                });
                let computed = cx
                    .background_spawn(async move {
                        Timer::after(Duration::from_millis(15)).await;
                        41i32
                    })
                    .await;
                let _ = handle.update(&mut cx, |v, _| {
                    v.log.lock().unwrap().push("fg-after-await");
                    v.value += computed;
                    if let Some(sig) = v.signal {
                        sig.set(v.value);
                    }
                    v.active_task = None;
                });
            }));
        });

        let on_defer = cx.listener(|this, _: ClickEvent, acx| {
            this.log.lock().unwrap().push("defer-click");
            // 验证当前视图内对自身的即时 acx.update 重入安全返回 None，绝不 panic
            let self_h = acx.handle::<Self>().expect("current handle must exist");
            assert!(
                acx.update(self_h, |_, _| ()).is_none(),
                "re-entrant immediate acx.update on active pod must safely return None"
            );
            acx.defer({
                let log = Arc::clone(&this.log);
                move |_acx| {
                    log.lock().unwrap().push("defer-1");
                }
            });
            acx.defer_self::<Self, _>(|v, _| {
                v.log.lock().unwrap().push("defer-self-2");
                v.value += 100;
            });
        });

        div()
            .id(1)
            .w(200)
            .h(120)
            .flex_col()
            .child(
                cx,
                div()
                    .id(10)
                    .w(180)
                    .h(40)
                    .on_click(cx, on_spawn)
                    .child(cx, text(fmt!(cx, "v={}", self.value))),
            )
            .child(
                cx,
                div()
                    .id(11)
                    .w(180)
                    .h(40)
                    .on_click(cx, on_defer)
                    .child(cx, text("Defer")),
            )
            .into_element(cx)
    }
}

struct SignalObserverView {
    signal: Signal<i32>,
    last_seen: i32,
}

impl View for SignalObserverView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        self.last_seen = self.signal.get();
        div()
            .id(200)
            .w(180)
            .h(24)
            .child(cx, text(fmt!(cx, "obs={}", self.last_seen)))
            .into_element(cx)
    }
}

struct RootShellView {
    counter: Handle<CounterView>,
    observer: Handle<SignalObserverView>,
}

impl View for RootShellView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .id(500)
            .w(240)
            .h(200)
            .flex_col()
            .child(cx, self.counter)
            .child(cx, self.observer)
            .into_element(cx)
    }
}

#[test]
fn test_engine_spawn_background_and_handle_update_l2_dirty() {
    let mut engine = VeloxEngine::new(240, 200);
    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(Arc::clone(&log)));
    let _ = engine.step_frame();
    assert_eq!(engine.pod_render_count(root), 1);
    assert!(!engine.needs_redraw());

    let ui_thread = std::thread::current().id();
    let task = engine.spawn_for(root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
        let (sum, ran_off_ui) = cx
            .background_spawn(async move {
                let bg_thread = std::thread::current().id();
                Timer::after(Duration::from_millis(12)).await;
                (123i32, bg_thread != ui_thread)
            })
            .await;

        let updated = this
            .update(&mut cx, |view, _| {
                view.value = sum;
                view.worker_ran_off_ui = ran_off_ui;
                view.value
            })
            .expect("root view must still be alive");
        assert_eq!(updated, 123);
        let snap = this.read(&cx, |view| view.value).unwrap();
        assert_eq!(snap, 123);
    });
    task.detach();

    // 推进至后台线程完成并回写 UI
    let mut finished = false;
    for _ in 0..80 {
        let _ = engine.step_frame();
        if engine.view(root).unwrap().value == 123 && !engine.needs_redraw() {
            finished = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(finished, "background_spawn + Handle::update must complete and repaint");
    assert!(engine.view(root).unwrap().worker_ran_off_ui);
    assert_eq!(engine.pod_render_count(root), 2);
    assert_eq!(engine.active_async_task_count(), 0);
}

#[test]
fn test_task_drop_auto_cancels_and_detach_survives() {
    let ready = Task::ready(77u32);
    assert!(!ready.is_cancelled());
    assert_eq!(block_on(ready), 77);

    let mut engine = VeloxEngine::new(200, 120);
    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(Arc::clone(&log)));
    let _ = engine.step_frame();

    let after_await_hit = Arc::new(AtomicBool::new(false));
    let hit_clone = Arc::clone(&after_await_hit);
    let cancelled_task = engine.spawn(async move |cx: AsyncApp| {
        cx.timer(Duration::from_millis(30)).await;
        hit_clone.store(true, Ordering::Release);
    });

    // 推一次使协程跑到 Timer::after(...).await 并挂起
    assert!(engine.poll_async_tasks());
    assert_eq!(engine.active_async_task_count(), 1);

    // RAII Drop 立即取消并标记待回收
    drop(cancelled_task);
    let _ = engine.poll_async_tasks();
    assert_eq!(
        engine.active_async_task_count(),
        0,
        "dropped Task must be reclaimed from foreground executor"
    );

    // 另派生一个 detach 任务，即使句柄销毁也会照常执行完毕
    let detached_done = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&detached_done);
    engine
        .spawn_for(root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(12)).await;
            let _ = this.update(&mut cx, |v, _| v.value = 9);
            done_clone.store(true, Ordering::Release);
        })
        .detach();

    for _ in 0..50 {
        let _ = engine.poll_async_tasks();
        if detached_done.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    // 再多等一点确认被 Drop 取消的任务绝不会在 30ms 后误执行
    std::thread::sleep(Duration::from_millis(35));
    let _ = engine.poll_async_tasks();

    assert!(detached_done.load(Ordering::Acquire));
    assert_eq!(engine.view(root).unwrap().value, 9);
    assert!(
        !after_await_hit.load(Ordering::Acquire),
        "dropped Task must NOT execute code after its suspension point"
    );
    assert_eq!(engine.active_async_task_count(), 0);
}

#[test]
fn test_removed_view_aba_generation_safety_returns_none() {
    let mut engine = VeloxEngine::new(200, 120);
    let log = Arc::new(Mutex::new(Vec::new()));
    let _root = engine.mount_root(CounterView::new(Arc::clone(&log)));
    let victim = engine.insert_view(CounterView::new(Arc::clone(&log)));

    let update_was_none = Arc::new(AtomicBool::new(false));
    let read_was_none = Arc::new(AtomicBool::new(false));
    let u_flag = Arc::clone(&update_was_none);
    let r_flag = Arc::clone(&read_was_none);

    engine
        .spawn_for(victim, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(15)).await;
            r_flag.store(this.read(&cx, |v| v.value).is_none(), Ordering::Release);
            u_flag.store(
                this.update(&mut cx, |v, _| v.value = 999).is_none(),
                Ordering::Release,
            );
        })
        .detach();

    // 跑到挂起点
    assert!(engine.poll_async_tasks());
    // 卸载旧视图并立即插入同类型新视图复用同槽位（经典 ABA 场景）
    assert!(engine.remove_view(victim));
    let replacement = engine.insert_view(CounterView::new(Arc::clone(&log)));
    assert_eq!(
        replacement.pod_id(),
        victim.pod_id(),
        "PodPool should reuse freed slot to verify ABA generation protection"
    );
    assert_ne!(replacement.generation(), victim.generation());

    for _ in 0..60 {
        let _ = engine.poll_async_tasks();
        if update_was_none.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }

    assert!(read_was_none.load(Ordering::Acquire));
    assert!(update_was_none.load(Ordering::Acquire));
    assert_eq!(
        engine.view(replacement).unwrap().value,
        0,
        "stale Handle must never mutate replacement view sharing the same PodId"
    );
}

#[test]
fn test_action_scope_spawn_from_click_and_signal_sync() {
    let mut engine = VeloxEngine::new(240, 200);
    let log = Arc::new(Mutex::new(Vec::new()));
    let sig = Signal::new(0i32);

    let mut counter_view = CounterView::new(Arc::clone(&log));
    counter_view.signal = Some(sig);
    let counter_h = engine.insert_view(counter_view);
    let observer_h = engine.insert_view(SignalObserverView {
        signal: sig,
        last_seen: -1,
    });
    let root_h = engine.mount_root(RootShellView {
        counter: counter_h,
        observer: observer_h,
    });

    let _ = engine.step_frame();
    assert_eq!(engine.pod_render_count(root_h), 1);
    assert_eq!(engine.pod_render_count(observer_h), 1);

    // 点击按钮 id=10 触发 acx.spawn
    let btn = engine
        .hit_regions()
        .iter()
        .find(|h| h.id == Some(10))
        .expect("button id=10")
        .bounds;
    assert!(engine.dispatch_click(btn.x + btn.w / 2, btn.y + btn.h / 2));

    // flush_async_after_event 应在点击同次交互内立刻跑到首个 .await
    assert_eq!(
        *log.lock().unwrap(),
        vec!["click", "fg-before-await"],
        "acx.spawn should immediately progress up to the first .await after event dispatch"
    );
    assert_eq!(engine.view(counter_h).unwrap().value, 1);

    let mut done = false;
    for _ in 0..80 {
        let _ = engine.step_frame();
        if engine.view(counter_h).unwrap().value == 42 && !engine.needs_redraw() {
            done = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(done);
    assert_eq!(
        *log.lock().unwrap(),
        vec!["click", "fg-before-await", "fg-after-await"]
    );
    assert_eq!(engine.view(observer_h).unwrap().last_seen, 42);
    assert_eq!(
        engine.pod_render_count(root_h),
        1,
        "RootShellView must remain untouched (0 re-renders) via L2 SubView + Signal isolation"
    );
    assert_eq!(engine.active_async_task_count(), 0);
}

#[test]
fn test_defer_and_defer_self_reentrancy_safety() {
    let mut engine = VeloxEngine::new(240, 200);
    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(Arc::clone(&log)));
    let _ = engine.step_frame();

    let btn = engine
        .hit_regions()
        .iter()
        .find(|h| h.id == Some(11))
        .expect("defer button id=11")
        .bounds;
    assert!(engine.dispatch_click(btn.x + btn.w / 2, btn.y + btn.h / 2));

    // 同次点击事件结束后 defer 队列自动按 FIFO 顺序排空，且 defer_self 成功拿到 &mut Self
    assert_eq!(
        *log.lock().unwrap(),
        vec!["defer-click", "defer-1", "defer-self-2"]
    );
    assert_eq!(engine.view(root).unwrap().value, 100);
}

#[test]
fn test_concurrent_timers_ordering_and_zero_polling_waker() {
    let mut engine = VeloxEngine::new(200, 120);
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_clone = Arc::clone(&wake_count);
    engine.set_async_waker(move || {
        wake_clone.fetch_add(1, Ordering::Relaxed);
    });

    let order = Arc::new(Mutex::new(Vec::<u8>::new()));
    for (tag, ms) in [(3u8, 60u64), (1u8, 15u64), (2u8, 35u64)] {
        let ord = Arc::clone(&order);
        engine
            .spawn(async move |cx: AsyncApp| {
                cx.timer(Duration::from_millis(ms)).await;
                ord.lock().unwrap().push(tag);
            })
            .detach();
    }

    // 首次 poll 挂载 3 个定时器
    assert!(engine.poll_async_tasks());
    assert_eq!(engine.active_async_task_count(), 3);

    for _ in 0..100 {
        let _ = engine.poll_async_tasks();
        if order.lock().unwrap().len() == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }

    assert_eq!(*order.lock().unwrap(), vec![1, 2, 3]);
    assert_eq!(engine.active_async_task_count(), 0);
    assert!(
        wake_count.load(Ordering::Relaxed) >= 3,
        "waker callback must fire when background timers expire"
    );

    // 验证 BackgroundExecutor + block_on 直接协作
    let bg_val = block_on(engine.background_spawn(async {
        Timer::after(Duration::from_millis(5)).await;
        2026usize
    }));
    assert_eq!(bg_val, 2026);
}

#[test]
fn test_silent_async_task_zero_cpu_idle_while_waiting() {
    let mut engine = VeloxEngine::new(240, 200);
    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(Arc::clone(&log)));

    let woken = Arc::new(AtomicUsize::new(0));
    let woken_c = Arc::clone(&woken);
    engine.set_async_waker(move || {
        woken_c.fetch_add(1, Ordering::AcqRel);
    });

    let _ = engine.step_frame();
    assert!(!engine.needs_redraw());

    // 静默异步任务（视图不含 Spinner，不调 request_frame）
    engine
        .spawn_for(root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(40)).await;
            let _ = this.update(&mut cx, |v, _| v.value = 55);
        })
        .detach();

    // 步进一帧挂起定时器
    let _ = engine.step_frame();
    let wakes_before = woken.load(Ordering::Acquire);

    // 关键验证：在等待 40ms 定时器的空闲窗口内，needs_redraw() 必须为 false（桌面端彻底进入 ControlFlow::Wait 0% CPU）
    assert!(
        !engine.needs_redraw(),
        "while waiting on silent async timer, engine.needs_redraw() must be false for 0% CPU sleep"
    );
    assert_eq!(engine.active_async_task_count(), 1);

    // 等待 wake_ui 被单常驻定时器线程精准唤醒（完全无需每帧轮询）
    let mut woke_up = false;
    for _ in 0..80 {
        if woken.load(Ordering::Acquire) > wakes_before {
            woke_up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(woke_up, "timer expiry must invoke async waker to wake sleeping UI loop");
    assert!(
        engine.needs_redraw(),
        "after async waker fires, engine.needs_redraw() must immediately become true"
    );

    let dmg = engine.step_frame();
    assert!(!dmg.is_empty(), "single frame after wakeup must repaint updated view");
    assert_eq!(engine.view(root).unwrap().value, 55);
    assert!(!engine.needs_redraw(), "must immediately settle back to 0% CPU idle");
    assert_eq!(engine.active_async_task_count(), 0);
}

#[test]
fn test_mobile_and_web_platform_async_waker_passthrough() {
    let mut mobile = MobileEmbedApp::new_android(320, 480);
    let log_m = Arc::new(Mutex::new(Vec::new()));
    let m_root = mobile.engine.mount_root(CounterView::new(log_m));
    let m_wakes = Arc::new(AtomicUsize::new(0));
    let m_wakes_c = Arc::clone(&m_wakes);
    mobile.set_async_waker(move || {
        m_wakes_c.fetch_add(1, Ordering::Relaxed);
    });
    let _ = mobile.step_frame(16);
    assert!(!mobile.needs_redraw());

    mobile
        .engine
        .spawn_for(m_root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(10)).await;
            let _ = this.update(&mut cx, |v, _| v.value = 11);
        })
        .detach();
    let _ = mobile.poll_async_tasks();
    for _ in 0..60 {
        if mobile.needs_redraw() {
            let _ = mobile.step_frame(16);
            if mobile.engine.view(m_root).unwrap().value == 11 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(mobile.engine.view(m_root).unwrap().value, 11);
    assert!(m_wakes.load(Ordering::Relaxed) >= 1);

    let mut web = WebCanvasApp::new(320, 240);
    let log_w = Arc::new(Mutex::new(Vec::new()));
    let w_root = web.engine.mount_root(CounterView::new(log_w));
    let w_wakes = Arc::new(AtomicUsize::new(0));
    let w_wakes_c = Arc::clone(&w_wakes);
    web.set_async_waker(move || {
        w_wakes_c.fetch_add(1, Ordering::Relaxed);
    });
    let _ = web.step_frame(16);
    assert!(!web.needs_redraw());

    web.engine
        .spawn_for(w_root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(10)).await;
            let _ = this.update(&mut cx, |v, _| v.value = 22);
        })
        .detach();
    let _ = web.poll_async_tasks();
    for _ in 0..60 {
        if web.needs_redraw() {
            let _ = web.step_frame(16);
            if web.engine.view(w_root).unwrap().value == 22 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(web.engine.view(w_root).unwrap().value, 22);
    assert!(w_wakes.load(Ordering::Relaxed) >= 1);
}

#[test]
fn test_pluggable_manual_scheduler_deterministic_virtual_clock() {
    let manual = ManualScheduler::new();
    let mut engine = VeloxEngine::new(240, 200).with_scheduler(manual.clone());
    assert_eq!(engine.scheduler_name(), "velox-manual");
    assert!(engine.scheduler_capabilities().deterministic_clock);
    assert!(!engine.scheduler_capabilities().supports_background_threads);

    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(Arc::clone(&log)));
    let _ = engine.step_frame();

    let real_start = Instant::now();

    // 启动一个跨 10 分钟（2 × 300s）的两阶段后台异步工作流，全程使用引擎当前调度器的虚拟时钟
    let long_task = engine.spawn_for(root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
        assert_eq!(cx.scheduler_name(), "velox-manual");
        let bg = cx.background_executor();
        let stage1 = bg
            .spawn({
                let bg = bg.clone();
                async move {
                    bg.timer(Duration::from_secs(300)).await;
                    10i32
                }
            })
            .await;
        let _ = this.update(&mut cx, |v, _| v.value += stage1);

        // 前景定时再等 5 分钟
        cx.timer(Duration::from_secs(300)).await;
        let _ = this.update(&mut cx, |v, _| v.value += 32);
    });

    // 同时派生一个 1 小时后才会触发、但中途将被 Drop 取消的任务
    let cancelled_hit = Arc::new(AtomicBool::new(false));
    let hit_flag = Arc::clone(&cancelled_hit);
    let one_hour_task = engine.spawn(async move |cx: AsyncApp| {
        cx.timer(Duration::from_secs(3600)).await;
        hit_flag.store(true, Ordering::Release);
    });

    // 推进前台 + 后台至全部挂起在虚拟定时器上
    manual.drive_engine(&mut engine);
    assert_eq!(manual.pending_timer_count(), 2);
    assert_eq!(engine.view(root).unwrap().value, 0);

    // 瞬间快进 5 分钟：阶段 1 完成，value 变为 10，进入阶段 2（再挂起 5 分钟定时器）
    assert_eq!(manual.advance(Duration::from_secs(300)), 1);
    manual.drive_engine(&mut engine);
    assert_eq!(engine.view(root).unwrap().value, 10);

    // Drop 取消 1 小时任务，立即回收
    drop(one_hour_task);
    manual.drive_engine(&mut engine);
    assert_eq!(engine.active_async_task_count(), 1);

    // 再快进 5 分钟：阶段 2 完成，value 变为 42
    assert_eq!(manual.advance(Duration::from_secs(300)), 1);
    manual.drive_engine(&mut engine);
    assert_eq!(engine.view(root).unwrap().value, 42);
    drop(long_task);
    assert_eq!(engine.active_async_task_count(), 0);

    // 再快进 1 小时确认已取消任务绝不会执行
    let _ = manual.advance(Duration::from_secs(3600));
    manual.drive_engine(&mut engine);
    assert!(!cancelled_hit.load(Ordering::Acquire));

    // 整套 70 分钟虚拟时间流程在真实墙钟下应瞬时完成（< 500ms）
    assert!(
        real_start.elapsed() < Duration::from_millis(500),
        "ManualScheduler virtual clock must run without real sleeping"
    );
}

#[test]
fn test_custom_third_party_scheduler_backend_adapter() {
    /// 模拟最小外部第三方调度器适配（例如未来接入 Tokio / smol），仅实现 4 个 trait 方法。
    struct CustomRuntimeAdapter {
        spawn_calls: AtomicUsize,
        timer_calls: AtomicUsize,
        delegate: Arc<ManualScheduler>,
    }

    impl SchedulerBackend for CustomRuntimeAdapter {
        fn name(&self) -> &'static str {
            "custom-third-party"
        }

        fn spawn_background(&self, fut: BoxFuture<'static, ()>) {
            self.spawn_calls.fetch_add(1, Ordering::Relaxed);
            self.delegate.spawn_background(fut);
        }

        fn timer(&self, duration: Duration) -> Timer {
            self.timer_calls.fetch_add(1, Ordering::Relaxed);
            self.delegate.timer(duration)
        }

        fn now_ms(&self) -> u64 {
            self.delegate.now_ms()
        }
    }

    let manual = ManualScheduler::new();
    let custom = Arc::new(CustomRuntimeAdapter {
        spawn_calls: AtomicUsize::new(0),
        timer_calls: AtomicUsize::new(0),
        delegate: Arc::clone(&manual),
    });

    let mut engine = VeloxEngine::new(200, 120);
    assert_eq!(engine.scheduler_name(), "velox-native-std");
    // 运行时热切换调度器后端
    engine.set_scheduler(custom.clone());
    assert_eq!(engine.scheduler_name(), "custom-third-party");

    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(log));

    let bg_dropped = Arc::new(AtomicBool::new(false));
    struct DropGuard(Arc<AtomicBool>);
    impl Drop for DropGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    // 1. 正常执行经过自定义适配器
    engine
        .spawn_for(root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            let bg = cx.background_executor();
            assert_eq!(bg.scheduler_name(), "custom-third-party");
            let v = bg
                .spawn({
                    let bg = bg.clone();
                    async move {
                        bg.timer(Duration::from_millis(50)).await;
                        88i32
                    }
                })
                .await;
            let _ = this.update(&mut cx, |view, _| view.value = v);
        })
        .detach();

    // 2. 验证第三方调度器上的后台任务也原生支持 RAII Task::drop 自动取消与资源释放
    let bg_cancel_task = engine.background_spawn({
        let flag = Arc::clone(&bg_dropped);
        let timer = engine.timer(Duration::from_secs(10));
        async move {
            let _guard = DropGuard(flag);
            timer.await;
        }
    });

    manual.drive_engine(&mut engine);
    assert_eq!(custom.spawn_calls.load(Ordering::Relaxed), 2);
    assert_eq!(custom.timer_calls.load(Ordering::Relaxed), 2);
    assert!(!bg_dropped.load(Ordering::Acquire));

    // Drop 后台 Task -> CancelWaker 唤醒第三方调度器立即 poll 并释放 Future 中的 DropGuard
    drop(bg_cancel_task);
    manual.drive_engine(&mut engine);
    assert!(
        bg_dropped.load(Ordering::Acquire),
        "RAII Task::drop must cancel and release Future on arbitrary SchedulerBackend"
    );

    manual.advance(Duration::from_millis(50));
    manual.drive_engine(&mut engine);
    assert_eq!(engine.view(root).unwrap().value, 88);
}

#[test]
fn test_std_async_driver_run_async_event_loop_idle_and_wakeup() {
    struct StepAsyncPlatform {
        target: Handle<CounterView>,
        frames: u32,
    }

    impl AsyncPlatform for StepAsyncPlatform {
        async fn step_platform_async(
            &mut self,
            engine: &mut VeloxEngine,
            dt_ms: u16,
        ) -> PlatformControlFlow {
            self.frames += 1;
            let _ = engine.step_frame_dt(dt_ms);
            if engine.view(self.target).unwrap().value == 77 {
                PlatformControlFlow::Exit
            } else if engine.needs_redraw() {
                PlatformControlFlow::Poll
            } else {
                PlatformControlFlow::Wait
            }
        }
    }

    let driver = StdAsyncDriver::new();
    let mut engine = VeloxEngine::new(200, 120);
    engine.set_async_waker(driver.wake_signal().notifier());

    let log = Arc::new(Mutex::new(Vec::new()));
    let root = engine.mount_root(CounterView::new(log));

    // 静默异步任务：25ms 后写入 value = 77 并由 UiWakeSignal 唤醒处于 Wait 休眠的事件循环
    engine
        .spawn_for(root, async move |this: Handle<CounterView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(25)).await;
            let _ = this.update(&mut cx, |v, _| v.value = 77);
        })
        .detach();

    let mut platform = StepAsyncPlatform {
        target: root,
        frames: 0,
    };
    let report = block_on(run_async_event_loop(
        &mut platform,
        &mut engine,
        &driver,
        AsyncLoopConfig::new(Duration::from_millis(16)).with_max_steps(30),
    ));

    assert_eq!(engine.view(root).unwrap().value, 77);
    assert_eq!(report.last_control_flow, Some(PlatformControlFlow::Exit));
    assert!(
        report.idle_waits >= 1,
        "loop must enter Wait sleep before timer wakeup (got {:?})",
        report
    );
    assert!(
        report.steps <= 5,
        "zero-polling wakeup should finish in just a few frames, got {}",
        report.steps
    );
}
}

// ============================================================================
// 全平台 `#![no_std]` 零堆 Embassy 风格调度器隔离验证（`--no-default-features` 下真实运行）
// ============================================================================

mod no_std_embassy_contract_tests {
    use core::cell::Cell;
    use core::future::Future;
    use core::pin::{pin, Pin};
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    use core::time::Duration;

    use velox_core::prelude::*;

    struct NoStdCounterView {
        sensor_temp_c: i16,
    }

    impl View for NoStdCounterView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .id(1)
                .w(128)
                .h(64)
                .bg(Color::BLACK)
                .child(cx, text(fmt!(cx, "T={}C", self.sensor_temp_c)).color(Color::WHITE))
                .into_element(cx)
        }
    }

    /// 零堆、零 `std` 模拟 **Embassy** 调度器驱动器（精确对应 `embassy_time` + `embassy_sync::signal::Signal` 模型）。
    struct MockEmbassyDriver {
        now_ms: Cell<u64>,
        irq_pending: Cell<bool>,
        wfi_sleeps: Cell<u32>,
    }

    impl MockEmbassyDriver {
        const fn new() -> Self {
            Self {
                now_ms: Cell::new(0),
                irq_pending: Cell::new(false),
                wfi_sleeps: Cell::new(0),
            }
        }

        /// 模拟 MCU 外部中断/同核其他 `#[embassy_executor::task]` 触发 UI 唤醒。
        fn trigger_irq(&self, elapsed_ms: u64) {
            self.now_ms.set(self.now_ms.get().saturating_add(elapsed_ms));
            self.irq_pending.set(true);
        }
    }

    struct MockEmbassySleep<'a> {
        driver: &'a MockEmbassyDriver,
        delta_ms: u64,
        done: bool,
    }

    impl Future for MockEmbassySleep<'_> {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            if self.done {
                Poll::Ready(())
            } else {
                self.done = true;
                self.driver
                    .now_ms
                    .set(self.driver.now_ms.get().saturating_add(self.delta_ms));
                Poll::Pending
            }
        }
    }

    struct MockEmbassyWaitIrq<'a> {
        driver: &'a MockEmbassyDriver,
        counted: bool,
    }

    impl Future for MockEmbassyWaitIrq<'_> {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            if self.driver.irq_pending.replace(false) {
                Poll::Ready(())
            } else {
                if !self.counted {
                    self.counted = true;
                    self.driver.wfi_sleeps.set(self.driver.wfi_sleeps.get() + 1);
                }
                Poll::Pending
            }
        }
    }

    impl SchedulerClock for MockEmbassyDriver {
        fn now_ms(&self) -> u64 {
            self.now_ms.get()
        }

        fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
            MockEmbassySleep {
                driver: self,
                delta_ms: duration.as_millis() as u64,
                done: false,
            }
        }
    }

    impl AsyncDriver for MockEmbassyDriver {
        fn capabilities(&self) -> SchedulerCapabilities {
            SchedulerCapabilities {
                name: "mock-async-driver",
                supports_background_threads: false,
                supports_dynamic_spawn: false,
                deterministic_clock: true,
            }
        }

        fn wait_for_event(&self) -> impl Future<Output = ()> {
            MockEmbassyWaitIrq {
                driver: self,
                counted: false,
            }
        }
    }

    /// 零堆异步 SPI DMA 显示表面模拟（每次刷屏仅耗费 1 个异步挂起点，无堆分配）。
    struct MockEmbassyDisplay<'a> {
        view: Handle<NoStdCounterView>,
        sensor_mailbox: &'a Cell<Option<i16>>,
        flushed_frames: u32,
        exit_after_temp: i16,
    }

    impl AsyncPlatform for MockEmbassyDisplay<'_> {
        async fn step_platform_async(
            &mut self,
            engine: &mut VeloxEngine,
            dt_ms: u16,
        ) -> PlatformControlFlow {
            // 模拟从外部静态 Embassy 通道拉取最新传感器读数并更新 UI
            if let Some(temp) = self.sensor_mailbox.take() {
                if let Some(v) = engine.view_mut(self.view) {
                    v.sensor_temp_c = temp;
                }
            }
            let dirty = engine.step_frame_dt(dt_ms);
            if !dirty.is_empty() {
                self.flushed_frames += 1;
                // 模拟非阻塞 SPI DMA 传输期间让出 CPU
                YieldNow::new().await;
            }
            if engine.view(self.view).unwrap().sensor_temp_c == self.exit_after_temp {
                PlatformControlFlow::Exit
            } else if engine.needs_redraw() {
                PlatformControlFlow::Poll
            } else {
                PlatformControlFlow::Wait
            }
        }
    }

    fn noop_waker() -> Waker {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |p| RawWaker::new(p, &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }

    #[test]
    fn test_no_std_embassy_style_zero_heap_driver_and_wfi_sleep_loop() {
        let mut engine = VeloxEngine::new(128, 64);
        let root = engine.mount_root(NoStdCounterView { sensor_temp_c: 24 });
        let driver = MockEmbassyDriver::new();
    assert_eq!(driver.capabilities().name, "mock-async-driver");
        assert!(!driver.capabilities().supports_dynamic_spawn);

        let sensor_mailbox = Cell::new(None);
        let mut display = MockEmbassyDisplay {
            view: root,
            sensor_mailbox: &sensor_mailbox,
            flushed_frames: 0,
            exit_after_temp: 29,
        };

        let report = {
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);
            let mut loop_fut = pin!(run_async_event_loop(
                &mut display,
                &mut engine,
                &driver,
                AsyncLoopConfig::new(Duration::from_millis(16)).with_max_steps(20),
            ));

            // 1. 推进至首帧 DMA 完成并进入 WFI 休眠（driver.wfi_sleeps == 1）
            for _ in 0..8 {
                assert!(matches!(loop_fut.as_mut().poll(&mut cx), Poll::Pending));
                if driver.wfi_sleeps.get() == 1 {
                    break;
                }
            }
            assert_eq!(
                driver.wfi_sleeps.get(),
                1,
                "MCU must enter WFI Wait sleep after first paint"
            );

            // 2. 模拟 5 秒后 I2C 传感器 Embassy 任务推送温度 29°C 并触发中断唤醒
            sensor_mailbox.set(Some(29));
            driver.trigger_irq(5_000);

            let mut final_report = None;
            for _ in 0..8 {
                if let Poll::Ready(rep) = loop_fut.as_mut().poll(&mut cx) {
                    final_report = Some(rep);
                    break;
                }
            }
            final_report.expect("Embassy loop must wake from WFI, repaint, and exit")
        };

        assert_eq!(report.idle_waits, 1);
        assert_eq!(report.last_control_flow, Some(PlatformControlFlow::Exit));
        assert_eq!(
            display.flushed_frames, 2,
            "must repaint exactly first frame + IRQ update frame"
        );
        assert_eq!(engine.view(root).unwrap().sensor_temp_c, 29);
    }
}
