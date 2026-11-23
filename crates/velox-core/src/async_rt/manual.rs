//! 确定性虚拟时钟与手动步进调度器 [`ManualScheduler`]（同时实现 [`SchedulerBackend`] 与 [`AsyncDriver`]）。
//!
//! 典型用途：
//! - **零真睡、零时间抖动单元测试**：直接通过 [`ManualScheduler::advance`] 在 <1ms 内瞬间快进数分钟异步定时器；
//! - **多调度器隔离验证**：证明同一套业务异步代码（`acx.spawn` / `cx.background_spawn` / `cx.timer`）可在不修改任何业务代码的前提下自由切换底层调度器。

use core::cmp::Reverse;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};
use core::time::Duration;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::{Arc, Mutex};
use std::task::Wake;
use std::vec::Vec;

use crate::engine::VeloxEngine;

use super::backend::{BoxFuture, SchedulerBackend, UiWakeSignal};
use super::scheduler::{AsyncDriver, SchedulerCapabilities, SchedulerClock};
use super::std_scheduler::Timer;

struct ManualTimerEntry {
    deadline_ms: u64,
    seq: u64,
    waker: Waker,
}

impl PartialEq for ManualTimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline_ms == other.deadline_ms && self.seq == other.seq
    }
}

impl Eq for ManualTimerEntry {}

impl PartialOrd for ManualTimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ManualTimerEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.deadline_ms
            .cmp(&other.deadline_ms)
            .then_with(|| self.seq.cmp(&other.seq))
    }
}

struct ManualBgJob {
    state: Arc<ManualState>,
    future: Mutex<Option<BoxFuture<'static, ()>>>,
    queued: AtomicBool,
}

impl ManualBgJob {
    fn enqueue(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let mut q = self
                .state
                .bg_queue
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            q.push_back(Arc::clone(self));
            drop(q);
            self.state.wake_signal.wake();
        }
    }

    fn poll_once(self: &Arc<Self>) -> bool {
        self.queued.store(false, Ordering::Release);
        let mut fut = {
            let mut slot = self.future.lock().unwrap_or_else(|e| e.into_inner());
            let Some(f) = slot.take() else {
                return false;
            };
            f
        };
        let waker = Waker::from(Arc::clone(self));
        let mut cx = Context::from_waker(&waker);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(()) => {}
            Poll::Pending => {
                let mut slot = self.future.lock().unwrap_or_else(|e| e.into_inner());
                *slot = Some(fut);
            }
        }
        true
    }
}

impl Wake for ManualBgJob {
    fn wake(self: Arc<Self>) {
        self.enqueue();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.enqueue();
    }
}

struct ManualState {
    now_ms: AtomicU64,
    timers: Mutex<(u64, BinaryHeap<Reverse<ManualTimerEntry>>)>,
    bg_queue: Mutex<VecDeque<Arc<ManualBgJob>>>,
    wake_signal: UiWakeSignal,
}

struct ManualTimerFuture {
    state: Arc<ManualState>,
    deadline_ms: u64,
    registered: bool,
}

impl Unpin for ManualTimerFuture {}

impl Future for ManualTimerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let now = this.state.now_ms.load(Ordering::Acquire);
        if now >= this.deadline_ms {
            return Poll::Ready(());
        }
        if !this.registered {
            this.registered = true;
            let mut guard = this.state.timers.lock().unwrap_or_else(|e| e.into_inner());
            guard.0 = guard.0.wrapping_add(1);
            let seq = guard.0;
            guard.1.push(Reverse(ManualTimerEntry {
                deadline_ms: this.deadline_ms,
                seq,
                waker: cx.waker().clone(),
            }));
        }
        Poll::Pending
    }
}

/// 确定性虚拟时钟与手动步进调度器（零真睡、零线程、100% 可复现）。
#[derive(Clone)]
pub struct ManualScheduler {
    state: Arc<ManualState>,
}

impl ManualScheduler {
    /// 创建一个从 `0ms` 开始计时的确定性调度器（返回 `Arc<Self>` 便于同时传给引擎与测试代码）。
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(ManualState {
                now_ms: AtomicU64::new(0),
                timers: Mutex::new((0, BinaryHeap::new())),
                bg_queue: Mutex::new(VecDeque::new()),
                wake_signal: UiWakeSignal::new(),
            }),
        })
    }

    /// 返回当前虚拟单调时间戳（毫秒）。
    #[inline]
    pub fn current_ms(&self) -> u64 {
        self.state.now_ms.load(Ordering::Acquire)
    }

    /// 返回当前虚拟单调时间戳（毫秒，`current_ms` 别名，避免同时导入多个调度 trait 时的调用二义性）。
    #[inline]
    pub fn now_ms(&self) -> u64 {
        self.current_ms()
    }

    /// 将虚拟时钟向前推进 `duration`，并按截止时间先后顺序唤醒全部已到期定时器，返回唤醒数量。
    pub fn advance(&self, duration: Duration) -> usize {
        let delta_ms = duration.as_millis().min(u64::MAX as u128) as u64;
        let target_ms = self
            .state
            .now_ms
            .fetch_add(delta_ms, Ordering::AcqRel)
            .saturating_add(delta_ms);

        let due_wakers: Vec<Waker> = {
            let mut guard = self.state.timers.lock().unwrap_or_else(|e| e.into_inner());
            let mut out = Vec::new();
            while let Some(Reverse(top)) = guard.1.peek() {
                if top.deadline_ms <= target_ms {
                    let Reverse(entry) = guard.1.pop().unwrap();
                    out.push(entry.waker);
                } else {
                    break;
                }
            }
            out
        };

        let count = due_wakers.len();
        for waker in due_wakers {
            waker.wake();
        }
        if count > 0 {
            self.state.wake_signal.wake();
        }
        count
    }

    /// 排空当前已就绪的后台任务队列直至全部挂起或完成，返回实际 poll 的任务次数。
    pub fn run_until_stalled(&self) -> usize {
        let mut polls = 0usize;
        for _ in 0..256 {
            let batch: Vec<Arc<ManualBgJob>> = {
                let mut q = self
                    .state
                    .bg_queue
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                q.drain(..).collect()
            };
            if batch.is_empty() {
                break;
            }
            for job in batch {
                if job.poll_once() {
                    polls += 1;
                }
            }
        }
        polls
    }

    /// 交替驱动后台任务与 [`VeloxEngine`] 前景协程，直至两者均进入静默等待，返回推进轮数。
    pub fn drive_engine(&self, engine: &mut VeloxEngine) -> usize {
        let mut progress = 0usize;
        for _ in 0..128 {
            let bg = self.run_until_stalled();
            let fg = usize::from(engine.poll_async_tasks());
            if bg == 0 && fg == 0 {
                break;
            }
            progress += bg + fg;
        }
        progress
    }

    /// 返回当前仍在等待虚拟时钟推进的定时器数量。
    #[inline]
    pub fn pending_timer_count(&self) -> usize {
        self.state
            .timers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .len()
    }

    /// 返回当前排队待执行的后台任务数量。
    #[inline]
    pub fn pending_background_count(&self) -> usize {
        self.state
            .bg_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// 获取关联的黏性唤醒信号句柄。
    #[inline]
    pub fn wake_signal(&self) -> &UiWakeSignal {
        &self.state.wake_signal
    }

    /// 主动触发一次异步事件循环唤醒。
    #[inline]
    pub fn notify_event(&self) {
        self.state.wake_signal.wake();
    }
}

impl SchedulerBackend for ManualScheduler {
    #[inline]
    fn name(&self) -> &'static str {
        "velox-manual"
    }

    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        SchedulerCapabilities {
            name: self.name(),
            supports_background_threads: false,
            supports_dynamic_spawn: true,
            deterministic_clock: true,
        }
    }

    fn spawn_background(&self, fut: BoxFuture<'static, ()>) {
        let job = Arc::new(ManualBgJob {
            state: Arc::clone(&self.state),
            future: Mutex::new(Some(fut)),
            queued: AtomicBool::new(false),
        });
        job.enqueue();
    }

    fn timer(&self, duration: Duration) -> Timer {
        let delta_ms = duration.as_millis().min(u64::MAX as u128) as u64;
        let deadline_ms = self.current_ms().saturating_add(delta_ms);
        Timer::from_future(std::boxed::Box::pin(ManualTimerFuture {
            state: Arc::clone(&self.state),
            deadline_ms,
            registered: false,
        }))
    }

    #[inline]
    fn now_ms(&self) -> u64 {
        self.current_ms()
    }
}

impl SchedulerClock for ManualScheduler {
    #[inline]
    fn now_ms(&self) -> u64 {
        self.current_ms()
    }

    #[inline]
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        <Self as SchedulerBackend>::timer(self, duration)
    }
}

impl AsyncDriver for ManualScheduler {
    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        <Self as SchedulerBackend>::capabilities(self)
    }

    #[inline]
    fn wait_for_event(&self) -> impl Future<Output = ()> {
        self.state.wake_signal.wait()
    }
}
