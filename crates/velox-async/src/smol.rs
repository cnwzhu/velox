//! **smol (`async-executor` + `async-io`)** 轻量级异步调度器适配实现（由 `velox-async/smol` feature 启用）。
//!
//! - 与 GPUI 底层所采用的 `smol` / `async-task` 生态同源，依赖极简、编译迅速；
//! - 实现 [`velox_core::async_rt::SchedulerBackend`] 与 [`velox_core::async_rt::AsyncDriver`]；
//! - 后台多线程池驱动 `async_executor::Executor`，定时由 `async_io::Timer` 驱动（空闲 0% CPU）。

use core::future::Future;
use core::time::Duration;
use std::boxed::Box;
use std::format;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ::async_executor::Executor;
use velox_core::app::Application;
use velox_core::async_rt::{
    AsyncDriver, BoxFuture, SchedulerBackend, SchedulerCapabilities, SchedulerClock, Timer,
    UiWakeSignal,
};
use velox_core::VeloxEngine;

pub use ::async_executor as executor;
pub use ::async_io as io;

/// Velox 的 `smol` (`async-executor` + `async-io`) 轻量级多线程调度器适配实现。
pub struct SmolScheduler {
    executor: Arc<Executor<'static>>,
    epoch: Instant,
    wake_signal: UiWakeSignal,
}

static GLOBAL_SMOL_SCHEDULER: OnceLock<Arc<SmolScheduler>> = OnceLock::new();

impl SmolScheduler {
    /// 创建一个默认 2 个后台工作线程的 `SmolScheduler` 实例。
    pub fn new() -> Arc<Self> {
        Self::with_worker_threads(2)
    }

    /// 创建指定工作线程数的 `SmolScheduler` 实例。
    pub fn with_worker_threads(worker_threads: usize) -> Arc<Self> {
        let executor = Arc::new(Executor::new());
        let count = worker_threads.max(1);
        for idx in 0..count {
            let ex = Arc::clone(&executor);
            let _ = std::thread::Builder::new()
                .name(format!("velox-smol-{idx}"))
                .stack_size(256 * 1024)
                .spawn(move || {
                    ::async_io::block_on(ex.run(core::future::pending::<()>()));
                });
        }
        Arc::new(Self {
            executor,
            epoch: Instant::now(),
            wake_signal: UiWakeSignal::new(),
        })
    }

    /// 获取进程级共享 [`SmolScheduler`] 单例（惰性初始化）。
    pub fn global() -> Arc<Self> {
        Arc::clone(GLOBAL_SMOL_SCHEDULER.get_or_init(Self::new))
    }

    /// 获取底层 [`::async_executor::Executor`] 引用。
    #[inline]
    pub fn executor(&self) -> &Arc<Executor<'static>> {
        &self.executor
    }

    /// 获取关联的黏性唤醒信号句柄。
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

impl SchedulerBackend for SmolScheduler {
    #[inline]
    fn name(&self) -> &'static str {
        "smol"
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
        // 取消生命周期由上层 Velox `Task<T>` + `CancellableFuture` 统一托管，这里直接 detach 到执行器
        self.executor.spawn(fut).detach();
    }

    fn timer(&self, duration: Duration) -> Timer {
        Timer::from_future(Box::pin(async move {
            let _ = ::async_io::Timer::after(duration).await;
        }))
    }

    #[inline]
    fn now_ms(&self) -> u64 {
        SmolScheduler::now_ms(self)
    }
}

impl SchedulerClock for SmolScheduler {
    #[inline]
    fn now_ms(&self) -> u64 {
        SmolScheduler::now_ms(self)
    }

    #[inline]
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        <Self as SchedulerBackend>::timer(self, duration)
    }
}

impl AsyncDriver for SmolScheduler {
    #[inline]
    fn capabilities(&self) -> SchedulerCapabilities {
        <Self as SchedulerBackend>::capabilities(self)
    }

    #[inline]
    fn wait_for_event(&self) -> impl Future<Output = ()> {
        self.wake_signal.wait()
    }
}

/// 为 [`VeloxEngine`] 与 [`Application`] 提供一行启用 `smol` 调度器的链式扩展方法。
pub trait VeloxSmolExt: Sized {
    /// 链式启用共享全局 [`SmolScheduler`]。
    fn with_smol(self) -> Self;
}

impl VeloxSmolExt for VeloxEngine {
    #[inline]
    fn with_smol(self) -> Self {
        self.with_scheduler(SmolScheduler::global())
    }
}

impl VeloxSmolExt for Application {
    #[inline]
    fn with_smol(self) -> Self {
        self.with_scheduler(SmolScheduler::global())
    }
}

/// 为 [`VeloxEngine`] 提供运行时热切换 `smol` 调度器的扩展方法。
pub trait VeloxEngineSmolExt {
    /// 运行时将引擎调度器切换为共享全局 [`SmolScheduler`]。
    fn use_smol(&mut self);
}

impl VeloxEngineSmolExt for VeloxEngine {
    #[inline]
    fn use_smol(&mut self) {
        self.set_scheduler(SmolScheduler::global());
    }
}
