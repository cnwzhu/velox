//! 可插拔动态调度器后端抽象 [`SchedulerBackend`]、跨线程黏性唤醒信号 [`UiWakeSignal`] 与标准异步驱动器 [`StdAsyncDriver`]。
//!
//! 通过将后台任务派生、定时器与单调时钟隔离在 [`SchedulerBackend`] trait 之后，
//! `VeloxEngine`、`ForegroundExecutor`、`AsyncApp` 与 `BackgroundExecutor` 彻底解耦底层调度实现，
//! 可在不修改任何业务视图代码的情况下自由切换：
//! - [`NativeStdScheduler`](super::NativeStdScheduler)：内置零外部依赖多线程池 + 最小堆定时器（默认）；
//! - [`ManualScheduler`](super::ManualScheduler)：确定性虚拟时钟 + 手动步进调度器（零真睡单元测试）；
//! - 外部自定义运行时适配（如未来的 Tokio / smol / Web Worker / 平台原生调度器）。

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use core::time::Duration;
use std::boxed::Box;
use std::sync::{Arc, Mutex};

use super::scheduler::{AsyncDriver, SchedulerCapabilities, SchedulerClock};
use super::std_scheduler::{NativeStdScheduler, Timer};

/// 堆装箱的 `Send + 'static` 异步任务类型别名（供 [`SchedulerBackend`] 后端实现接收）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 可插拔非嵌入式异步调度器后端抽象。
///
/// 框架内所有 `Task<T>` 的 RAII 取消、返回值传递与 `Waker` 包装均由上层统一负责，
/// 任意第三方或自定义调度器仅需实现本 trait 的 4 个核心方法即可完整接入 Velox。
pub trait SchedulerBackend: Send + Sync + 'static {
    /// 调度器名称（用于日志与能力诊断，如 `"velox-native-std"` / `"velox-manual"` / `"tokio"`）。
    fn name(&self) -> &'static str;

    /// 返回当前调度器的能力描述符。
    fn capabilities(&self) -> SchedulerCapabilities {
        SchedulerCapabilities {
            name: self.name(),
            supports_background_threads: true,
            supports_dynamic_spawn: true,
            deterministic_clock: false,
        }
    }

    /// 在后台执行器中派发一个无返回值的 `Send + 'static` 异步任务。
    fn spawn_background(&self, fut: BoxFuture<'static, ()>);

    /// 创建一个由当前调度器时钟驱动的非阻塞定时器 [`Timer`]。
    fn timer(&self, duration: Duration) -> Timer;

    /// 获取当前调度器单调时间戳（毫秒）。
    fn now_ms(&self) -> u64;
}

// ============================================================================
// 线程安全黏性唤醒信号 `UiWakeSignal`（零丢唤醒，可直连 `engine.set_async_waker`）
// ============================================================================

struct WakeSignalState {
    signaled: bool,
    waker: Option<Waker>,
}

/// 线程安全的跨线程黏性（latched）异步唤醒信号（语义对齐 Embassy `embassy_sync::signal::Signal<_, ()>`）。
///
/// - 若在 [`UiWakeSignal::wait`] 之前调用 [`UiWakeSignal::wake`]，信号会被锁存，下一次 `wait().await` 立即就绪绝不丢唤醒；
/// - 可通过 [`UiWakeSignal::notifier`] 生成 `Fn() + Send + Sync + 'static` 直接注入 [`crate::engine::VeloxEngine::set_async_waker`]。
#[derive(Clone)]
pub struct UiWakeSignal {
    state: Arc<Mutex<WakeSignalState>>,
}

impl Default for UiWakeSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl UiWakeSignal {
    /// 创建一个新的未触发唤醒信号。
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(WakeSignalState {
                signaled: false,
                waker: None,
            })),
        }
    }

    /// 触发唤醒信号：若存在挂起的等待协程则立即唤醒，否则锁存状态供下次 `wait().await` 消费。
    pub fn wake(&self) {
        let waker = {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            guard.signaled = true;
            guard.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }

    /// 构造一个可直接传给 [`crate::engine::VeloxEngine::set_async_waker`] 的线程安全唤醒闭包。
    pub fn notifier(&self) -> impl Fn() + Send + Sync + 'static {
        let signal = self.clone();
        move || signal.wake()
    }

    /// 异步挂起等待下一次唤醒信号（消费已锁存信号后立刻返回）。
    pub fn wait(&self) -> UiWakeFuture {
        UiWakeFuture {
            state: Arc::clone(&self.state),
        }
    }
}

/// [`UiWakeSignal::wait`] 返回的异步等待 [`Future`]。
pub struct UiWakeFuture {
    state: Arc<Mutex<WakeSignalState>>,
}

impl Unpin for UiWakeFuture {}

impl Future for UiWakeFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if guard.signaled {
            guard.signaled = false;
            return Poll::Ready(());
        }
        guard.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

// ============================================================================
// 基于任意 `SchedulerBackend` 的通用 `StdAsyncDriver`
// ============================================================================

/// 将任意 [`SchedulerBackend`] 与 [`UiWakeSignal`] 组合为 [`AsyncDriver`]，
/// 用于直接驱动 [`super::run_async_event_loop`]。
#[derive(Clone)]
pub struct StdAsyncDriver {
    backend: Arc<dyn SchedulerBackend>,
    wake_signal: UiWakeSignal,
}

impl Default for StdAsyncDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl StdAsyncDriver {
    /// 使用默认内置 [`NativeStdScheduler`] 创建标准异步主循环驱动器。
    pub fn new() -> Self {
        Self::with_backend(NativeStdScheduler::global())
    }

    /// 使用指定的 [`SchedulerBackend`] 创建异步主循环驱动器。
    pub fn with_backend(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self {
            backend,
            wake_signal: UiWakeSignal::new(),
        }
    }

    /// 获取关联的黏性唤醒信号句柄（用于绑定至 `engine.set_async_waker(driver.wake_signal().notifier())`）。
    #[inline]
    pub fn wake_signal(&self) -> &UiWakeSignal {
        &self.wake_signal
    }

    /// 触发一次外部事件唤醒。
    #[inline]
    pub fn wake(&self) {
        self.wake_signal.wake();
    }

    /// 获取底层 [`SchedulerBackend`] 引用。
    #[inline]
    pub fn backend(&self) -> &Arc<dyn SchedulerBackend> {
        &self.backend
    }
}

impl SchedulerClock for StdAsyncDriver {
    #[inline]
    fn now_ms(&self) -> u64 {
        self.backend.now_ms()
    }

    #[inline]
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        self.backend.timer(duration)
    }
}

impl AsyncDriver for StdAsyncDriver {
    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        self.backend.capabilities()
    }

    #[inline]
    fn wait_for_event(&self) -> impl Future<Output = ()> {
        self.wake_signal.wait()
    }
}
