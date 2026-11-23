//! **Tokio** 异步调度器适配实现（由 `velox-async/tokio` feature 启用）。
//!
//! - 将 Tokio 多线程运行时适配为 [`velox_core::async_rt::SchedulerBackend`] 与 [`velox_core::async_rt::AsyncDriver`]；
//! - 内置跨线程安全 `TokioTimerFuture`（每次 `poll` 自动 `handle.enter()`），确保在 `winit` UI 主线程中调用 `cx.timer(...)` 永不触发 `no reactor running` panic；
//! - 可直接在 `cx.background_spawn(...)` 内无缝 `.await` `reqwest`、`hyper`、`sqlx`、`tokio::net`、`tokio::fs` 等完整 Tokio 生态库。

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;
use std::boxed::Box;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use velox_core::app::Application;
use velox_core::async_rt::{
    AsyncDriver, BoxFuture, SchedulerBackend, SchedulerCapabilities, SchedulerClock, Timer,
    UiWakeSignal,
};
use velox_core::VeloxEngine;

pub use ::tokio as runtime;

/// 跨线程安全的 `tokio::time::Sleep` 包装器：
/// 在每次 `poll` 期间通过 `handle.enter()` 绑定所属 Tokio 运行时上下文，
/// 保证在非 Tokio 线程（如桌面 `winit` UI 主线程）轮询定时器时 100% 安全。
struct TokioTimerFuture {
    handle: ::tokio::runtime::Handle,
    duration: Duration,
    sleep: Option<Pin<Box<::tokio::time::Sleep>>>,
}

impl Unpin for TokioTimerFuture {}

impl Future for TokioTimerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let _guard = this.handle.enter();
        let sleep = this
            .sleep
            .get_or_insert_with(|| Box::pin(::tokio::time::sleep(this.duration)));
        sleep.as_mut().poll(cx)
    }
}

/// Velox 的 Tokio 多线程调度器适配实现（同时实现 [`SchedulerBackend`] 与 [`AsyncDriver`]）。
pub struct TokioScheduler {
    _owned_runtime: Option<::tokio::runtime::Runtime>,
    handle: ::tokio::runtime::Handle,
    epoch: Instant,
    wake_signal: UiWakeSignal,
}

static GLOBAL_TOKIO_SCHEDULER: OnceLock<Arc<TokioScheduler>> = OnceLock::new();

impl TokioScheduler {
    /// 创建一个默认 2 个工作线程的 Tokio 调度器实例。
    pub fn new() -> Arc<Self> {
        Self::with_worker_threads(2)
    }

    /// 创建指定后台工作线程数的 Tokio 调度器实例。
    pub fn with_worker_threads(worker_threads: usize) -> Arc<Self> {
        let runtime = ::tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads.max(1))
            .enable_all()
            .thread_name("velox-tokio")
            .build()
            .expect("failed to build Tokio runtime for velox-async");
        let handle = runtime.handle().clone();
        Arc::new(Self {
            _owned_runtime: Some(runtime),
            handle,
            epoch: Instant::now(),
            wake_signal: UiWakeSignal::new(),
        })
    }

    /// 复用外部已存在的 [`::tokio::runtime::Handle`] 构造适配器（零额外线程池创建）。
    pub fn from_handle(handle: ::tokio::runtime::Handle) -> Arc<Self> {
        Arc::new(Self {
            _owned_runtime: None,
            handle,
            epoch: Instant::now(),
            wake_signal: UiWakeSignal::new(),
        })
    }

    /// 获取进程级共享 [`TokioScheduler`] 单例（惰性初始化，按需启动后台线程）。
    pub fn global() -> Arc<Self> {
        Arc::clone(GLOBAL_TOKIO_SCHEDULER.get_or_init(Self::new))
    }

    /// 获取底层 [`::tokio::runtime::Handle`] 引用。
    #[inline]
    pub fn handle(&self) -> &::tokio::runtime::Handle {
        &self.handle
    }

    /// 获取关联的黏性唤醒信号句柄（用于绑定至 `engine.set_async_waker(scheduler.wake_signal().notifier())`）。
    #[inline]
    pub fn wake_signal(&self) -> &UiWakeSignal {
        &self.wake_signal
    }

    /// 返回单调递增时间戳（毫秒）。
    #[inline]
    pub fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

impl SchedulerBackend for TokioScheduler {
    #[inline]
    fn name(&self) -> &'static str {
        "tokio"
    }

    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        SchedulerCapabilities {
            name: self.name(),
            supports_background_threads: true,
            supports_dynamic_spawn: true,
            deterministic_clock: false,
        }
    }

    fn spawn_background(&self, fut: BoxFuture<'static, ()>) {
        self.handle.spawn(fut);
    }

    fn timer(&self, duration: Duration) -> Timer {
        Timer::from_future(Box::pin(TokioTimerFuture {
            handle: self.handle.clone(),
            duration,
            sleep: None,
        }))
    }

    #[inline]
    fn now_ms(&self) -> u64 {
        TokioScheduler::now_ms(self)
    }
}

impl SchedulerClock for TokioScheduler {
    #[inline]
    fn now_ms(&self) -> u64 {
        TokioScheduler::now_ms(self)
    }

    #[inline]
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        <Self as SchedulerBackend>::timer(self, duration)
    }
}

impl AsyncDriver for TokioScheduler {
    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        <Self as SchedulerBackend>::capabilities(self)
    }

    #[inline]
    fn wait_for_event(&self) -> impl Future<Output = ()> {
        self.wake_signal.wait()
    }
}

/// 为 [`VeloxEngine`] 与 [`Application`] 提供一行启用 Tokio 调度器的链式扩展方法。
pub trait VeloxTokioExt: Sized {
    /// 链式启用共享全局 [`TokioScheduler`]。
    fn with_tokio(self) -> Self;
    /// 链式复用外部 [`::tokio::runtime::Handle`] 作为异步调度器。
    fn with_tokio_handle(self, handle: ::tokio::runtime::Handle) -> Self;
}

impl VeloxTokioExt for VeloxEngine {
    #[inline]
    fn with_tokio(self) -> Self {
        self.with_scheduler(TokioScheduler::global())
    }

    #[inline]
    fn with_tokio_handle(self, handle: ::tokio::runtime::Handle) -> Self {
        self.with_scheduler(TokioScheduler::from_handle(handle))
    }
}

impl VeloxTokioExt for Application {
    #[inline]
    fn with_tokio(self) -> Self {
        self.with_scheduler(TokioScheduler::global())
    }

    #[inline]
    fn with_tokio_handle(self, handle: ::tokio::runtime::Handle) -> Self {
        self.with_scheduler(TokioScheduler::from_handle(handle))
    }
}

/// 为 [`VeloxEngine`] 提供运行时热切换 Tokio 调度器的扩展方法。
pub trait VeloxEngineTokioExt {
    /// 运行时将引擎调度器切换为共享全局 [`TokioScheduler`]。
    fn use_tokio(&mut self);
    /// 运行时将引擎调度器切换为指定的 [`::tokio::runtime::Handle`]。
    fn use_tokio_handle(&mut self, handle: ::tokio::runtime::Handle);
}

impl VeloxEngineTokioExt for VeloxEngine {
    #[inline]
    fn use_tokio(&mut self) {
        self.set_scheduler(TokioScheduler::global());
    }

    #[inline]
    fn use_tokio_handle(&mut self, handle: ::tokio::runtime::Handle) {
        self.set_scheduler(TokioScheduler::from_handle(handle));
    }
}
