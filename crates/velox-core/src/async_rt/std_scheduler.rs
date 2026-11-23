//! 内置默认标准库调度器 [`NativeStdScheduler`]、统一定时器 [`Timer`]、可插拔 [`BackgroundExecutor`] 与轻量 [`block_on`]。
//!
//! 特点：
//! - 零外部异步运行时依赖（纯标准库 `Future` / `Waker` / `Condvar` / `BinaryHeap`）；
//! - 后台线程池与单常驻最小堆定时器线程均惰性启动，未调用前 0 线程、0 内存占用，空闲 0% CPU；
//! - [`BackgroundExecutor`] 持有 `Arc<dyn SchedulerBackend>`，并通过通用可取消包装自动为任意后端提供 RAII `Task::drop` 取消语义。

use core::cmp::Reverse;
use core::future::Future;
use core::pin::{pin, Pin};
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};
use core::time::Duration;
use std::boxed::Box;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::task::Wake;
use std::time::Instant;

use super::backend::{BoxFuture, SchedulerBackend};
use super::scheduler::SchedulerCapabilities;
use super::task::{Task, TaskShared};

/// 在当前线程阻塞驱动任意 [`Future`] 至完成（用于无头工具、CLI 演示与单元测试）。
pub fn block_on<F: Future>(fut: F) -> F::Output {
    struct ThreadWaker {
        ready: Mutex<bool>,
        cv: Condvar,
    }
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if let Ok(mut ready) = self.ready.lock() {
                *ready = true;
                self.cv.notify_one();
            }
        }
    }

    let signal = Arc::new(ThreadWaker {
        ready: Mutex::new(false),
        cv: Condvar::new(),
    });
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);

    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => {
                if let Ok(mut ready) = signal.ready.lock() {
                    while !*ready {
                        ready = match signal.cv.wait(ready) {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                    }
                    *ready = false;
                }
            }
        }
    }
}

// ============================================================================
// 全局单常驻线程最小堆定时器（NativeTimerDriver）
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
struct TimerEntry {
    deadline: Instant,
    seq: u64,
    waker: Waker,
}

#[cfg(not(target_arch = "wasm32"))]
impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.seq == other.seq
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Eq for TimerEntry {}

#[cfg(not(target_arch = "wasm32"))]
impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.deadline
            .cmp(&other.deadline)
            .then_with(|| self.seq.cmp(&other.seq))
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct TimerDriver {
    state: Mutex<(u64, BinaryHeap<Reverse<TimerEntry>>)>,
    cv: Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
static TIMER_DRIVER: OnceLock<Arc<TimerDriver>> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
fn timer_driver() -> &'static Arc<TimerDriver> {
    TIMER_DRIVER.get_or_init(|| {
        let driver = Arc::new(TimerDriver {
            state: Mutex::new((0, BinaryHeap::new())),
            cv: Condvar::new(),
        });
        let bg = Arc::clone(&driver);
        let _ = std::thread::Builder::new()
            .name(std::string::String::from("velox-async-timer"))
            .stack_size(128 * 1024)
            .spawn(move || loop {
                let waker_to_fire = {
                    let mut guard = bg.state.lock().unwrap_or_else(|e| e.into_inner());
                    loop {
                        let Some(Reverse(top)) = guard.1.peek() else {
                            guard = bg.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                            continue;
                        };
                        let now = Instant::now();
                        if top.deadline <= now {
                            let Reverse(entry) = guard.1.pop().unwrap();
                            break entry.waker;
                        }
                        let wait_dur = top.deadline.saturating_duration_since(now);
                        let (next_guard, _) = bg
                            .cv
                            .wait_timeout(guard, wait_dur)
                            .unwrap_or_else(|e| e.into_inner());
                        guard = next_guard;
                    }
                };
                waker_to_fire.wake();
            });
        driver
    })
}

struct NativeTimer {
    deadline: Instant,
    registered: bool,
}

impl NativeTimer {
    #[inline]
    fn after(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            registered: false,
        }
    }

    fn poll_native(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !self.registered {
                self.registered = true;
                let driver = timer_driver();
                let mut guard = driver.state.lock().unwrap_or_else(|e| e.into_inner());
                guard.0 = guard.0.wrapping_add(1);
                let seq = guard.0;
                let is_earlier = guard
                    .1
                    .peek()
                    .map(|Reverse(top)| self.deadline < top.deadline)
                    .unwrap_or(true);
                guard.1.push(Reverse(TimerEntry {
                    deadline: self.deadline,
                    seq,
                    waker: cx.waker().clone(),
                }));
                if is_earlier {
                    driver.cv.notify_one();
                }
            }
            Poll::Pending
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = cx;
            Poll::Ready(())
        }
    }
}

enum TimerInner {
    /// 默认原生最小堆定时器（零额外 `Box` 堆分配快路径）。
    Native(NativeTimer),
    /// 可插拔第三方或确定性调度器提供的自定义定时器 Future。
    Custom(BoxFuture<'static, ()>),
}

/// 跨调度器统一非阻塞异步定时器 [`Future`]。
///
/// - 使用默认 [`NativeStdScheduler`] 时走零 `Box` 快路径，由单常驻最小堆定时器线程驱动（空闲 0% CPU）；
/// - 注入自定义 [`SchedulerBackend`]（如 [`ManualScheduler`](super::ManualScheduler) 或未来的 Tokio 适配器）时，
///   自动透明切换至对应调度器的时钟实现，业务层无需修改任何代码。
pub struct Timer {
    inner: TimerInner,
}

impl Timer {
    /// 创建一个使用默认内置原生时钟驱动的异步定时器。
    #[inline]
    pub fn after(duration: Duration) -> Self {
        Self {
            inner: TimerInner::Native(NativeTimer::after(duration)),
        }
    }

    /// 将自定义调度器返回的任意 `Send + 'static` 定时 Future 包装为统一 [`Timer`]。
    #[inline]
    pub fn from_future(fut: BoxFuture<'static, ()>) -> Self {
        Self {
            inner: TimerInner::Custom(fut),
        }
    }
}

impl Unpin for Timer {}

impl Future for Timer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.get_mut().inner {
            TimerInner::Native(native) => native.poll_native(cx),
            TimerInner::Custom(custom) => custom.as_mut().poll(cx),
        }
    }
}

// ============================================================================
// 内置原生后台工作线程池（NativeStdScheduler）
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
struct BgJob {
    future: Mutex<Option<BoxFuture<'static, ()>>>,
    scheduled: AtomicBool,
}

#[cfg(not(target_arch = "wasm32"))]
impl BgJob {
    fn enqueue(self: &Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            let pool = bg_pool();
            let mut q = pool.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.push_back(Arc::clone(self));
            pool.cv.notify_one();
        }
    }

    fn run(self: &Arc<Self>) {
        self.scheduled.store(false, Ordering::Release);

        // 先移出 Future 到局部栈再 poll，避免 Future 内部同步触发 wake 造成重入锁死锁
        let mut fut = {
            let mut slot = self.future.lock().unwrap_or_else(|e| e.into_inner());
            let Some(f) = slot.take() else {
                return;
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
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Wake for BgJob {
    fn wake(self: Arc<Self>) {
        self.enqueue();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.enqueue();
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct BackgroundPool {
    queue: Mutex<VecDeque<Arc<BgJob>>>,
    cv: Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
static BG_POOL: OnceLock<Arc<BackgroundPool>> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
fn bg_pool() -> &'static Arc<BackgroundPool> {
    BG_POOL.get_or_init(|| {
        let pool = Arc::new(BackgroundPool {
            queue: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
        });
        let worker_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .clamp(2, 4);
        for idx in 0..worker_count {
            let p = Arc::clone(&pool);
            let _ = std::thread::Builder::new()
                .name(std::format!("velox-bg-{idx}"))
                .stack_size(256 * 1024)
                .spawn(move || loop {
                    let job = {
                        let mut q = p.queue.lock().unwrap_or_else(|e| e.into_inner());
                        while q.is_empty() {
                            q = p.cv.wait(q).unwrap_or_else(|e| e.into_inner());
                        }
                        q.pop_front()
                    };
                    if let Some(job) = job {
                        job.run();
                    }
                });
        }
        pool
    })
}

/// Velox 内置默认标准库调度器后端（零外部依赖：常驻多线程池 + 单常驻最小堆定时器线程）。
pub struct NativeStdScheduler {
    epoch: Instant,
}

static GLOBAL_NATIVE_SCHEDULER: OnceLock<Arc<dyn SchedulerBackend>> = OnceLock::new();

impl NativeStdScheduler {
    /// 创建一个新的原生标准库调度器实例（共享惰性后台线程池与定时器线程）。
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }

    /// 获取进程级共享默认 [`NativeStdScheduler`] 后端引用（零重复分配）。
    pub fn global() -> Arc<dyn SchedulerBackend> {
        Arc::clone(GLOBAL_NATIVE_SCHEDULER.get_or_init(|| Arc::new(Self::new())))
    }
}

impl Default for NativeStdScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerBackend for NativeStdScheduler {
    #[inline]
    fn name(&self) -> &'static str {
        "velox-native-std"
    }

    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        SchedulerCapabilities {
            name: self.name(),
            supports_background_threads: cfg!(not(target_arch = "wasm32")),
            supports_dynamic_spawn: true,
            deterministic_clock: false,
        }
    }

    fn spawn_background(&self, fut: BoxFuture<'static, ()>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let job = Arc::new(BgJob {
                future: Mutex::new(Some(fut)),
                scheduled: AtomicBool::new(false),
            });
            job.enqueue();
        }
        #[cfg(target_arch = "wasm32")]
        {
            block_on(fut);
        }
    }

    #[inline]
    fn timer(&self, duration: Duration) -> Timer {
        Timer::after(duration)
    }

    #[inline]
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

// ============================================================================
// 通用可取消后台任务包装 + 可插拔 `BackgroundExecutor`
// ============================================================================

struct CancellableFuture<T, Fut> {
    fut: Option<Pin<Box<Fut>>>,
    shared: Arc<TaskShared<T>>,
    cancel_waker: Arc<Mutex<Option<Waker>>>,
}

impl<T, Fut> Unpin for CancellableFuture<T, Fut> {}

impl<T, Fut> Future for CancellableFuture<T, Fut>
where
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.shared.is_cancelled() {
            this.fut = None;
            let mut slot = this.cancel_waker.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
            return Poll::Ready(());
        }

        {
            let mut slot = this.cancel_waker.lock().unwrap_or_else(|e| e.into_inner());
            let should_update = slot
                .as_ref()
                .map(|prev| !prev.will_wake(cx.waker()))
                .unwrap_or(true);
            if should_update {
                *slot = Some(cx.waker().clone());
            }
        }

        let Some(fut) = this.fut.as_mut() else {
            return Poll::Ready(());
        };

        match fut.as_mut().poll(cx) {
            Poll::Ready(output) => {
                this.fut = None;
                {
                    let mut slot = this.cancel_waker.lock().unwrap_or_else(|e| e.into_inner());
                    *slot = None;
                }
                if !this.shared.is_cancelled() {
                    this.shared.complete(output);
                }
                Poll::Ready(())
            }
            Poll::Pending => {
                if this.shared.is_cancelled() {
                    this.fut = None;
                    let mut slot = this.cancel_waker.lock().unwrap_or_else(|e| e.into_inner());
                    *slot = None;
                    return Poll::Ready(());
                }
                Poll::Pending
            }
        }
    }
}

/// 可插拔后台异步执行器句柄（轻量 `Clone`，面向 CPU 密集型计算、阻塞 I/O 与跨调度器定时）。
///
/// 内部持有 `Arc<dyn SchedulerBackend>`：默认指向 [`NativeStdScheduler::global`]，
/// 也可通过 [`BackgroundExecutor::with_backend`] 或 [`crate::engine::VeloxEngine::with_scheduler`] 切换至任意外部调度器。
#[derive(Clone)]
pub struct BackgroundExecutor {
    backend: Arc<dyn SchedulerBackend>,
}

impl Default for BackgroundExecutor {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for BackgroundExecutor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BackgroundExecutor")
            .field("scheduler", &self.backend.name())
            .finish()
    }
}

impl BackgroundExecutor {
    /// 创建一个绑定默认内置 [`NativeStdScheduler`] 的后台执行器句柄。
    #[inline]
    pub fn new() -> Self {
        Self {
            backend: NativeStdScheduler::global(),
        }
    }

    /// 创建一个绑定指定 [`SchedulerBackend`] 的后台执行器句柄。
    #[inline]
    pub fn with_backend(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self { backend }
    }

    /// 获取底层 [`SchedulerBackend`] 引用。
    #[inline]
    pub fn backend(&self) -> &Arc<dyn SchedulerBackend> {
        &self.backend
    }

    /// 返回当前绑定的调度器名称。
    #[inline]
    pub fn scheduler_name(&self) -> &'static str {
        self.backend.name()
    }

    /// 返回当前绑定的调度器能力元信息。
    #[inline]
    pub fn capabilities(&self) -> SchedulerCapabilities {
        self.backend.capabilities()
    }

    /// 返回当前调度器单调时钟（毫秒）。
    #[inline]
    pub fn now_ms(&self) -> u64 {
        self.backend.now_ms()
    }

    /// 在所配置的后台调度器中派生 `Send + 'static` 异步任务并返回支持 RAII Drop 自动取消的 [`Task<T>`]。
    pub fn spawn<T, Fut>(&self, fut: Fut) -> Task<T>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let cancel_waker_cb = Arc::clone(&cancel_waker);
        let on_cancel: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let waker = {
                let mut slot = cancel_waker_cb.lock().unwrap_or_else(|e| e.into_inner());
                slot.take()
            };
            if let Some(w) = waker {
                w.wake();
            }
        });
        let shared = TaskShared::new(cancelled, Some(on_cancel));
        let wrapped = CancellableFuture {
            fut: Some(Box::pin(fut)),
            shared: Arc::clone(&shared),
            cancel_waker,
        };
        self.backend.spawn_background(Box::pin(wrapped));
        Task::from_shared(shared)
    }

    /// 创建由当前调度器时钟驱动的非阻塞异步定时器（与 GPUI `cx.background_executor().timer(dur)` 对齐）。
    #[inline]
    pub fn timer(&self, duration: Duration) -> Timer {
        self.backend.timer(duration)
    }
}
