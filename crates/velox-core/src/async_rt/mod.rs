//! 跨平台异步调度与运行时抽象层 `velox_core::async_rt`。
//!
//! 采用严格分层隔离设计，使框架核心、UI 业务代码与底层调度器完全解耦：
//!
//! 1. **全平台 `#![no_std]` 零堆抽象层（含嵌入式 Embassy / RTIC / 裸机 MCU）**：
//!    - [`SchedulerCapabilities`]：描述当前调度器能力特征；
//!    - [`SchedulerClock`] / [`AsyncDriver`]：抽象单调时钟、非阻塞 `sleep`、`yield_now` 与空闲 `wait_for_event`（Embassy 可直接桥接 `embassy_time` 与 `embassy_sync::signal::Signal` 实现零堆 `WFI` 深度休眠）；
//!    - [`run_async_event_loop`] / [`AsyncLoopConfig`] / [`AsyncLoopReport`]：全平台通用的零堆异步主循环驱动函数。
//!
//! 2. **非嵌入式 `#[cfg(feature = "std")]` 可插拔多调度器与双执行器层**：
//!    - [`SchedulerBackend`]：可插拔后台任务与时钟后端 trait，仅需实现 4 个方法即可接入任意调度器（未来如 Tokio / smol / Web Worker）；
//!    - [`NativeStdScheduler`]：内置默认零外部依赖多线程池 + 单常驻最小堆定时器；
//!    - [`ManualScheduler`]：确定性虚拟时钟 + 手动步进调度器（零真睡、0ms 快进测试）；
//!    - [`ForegroundExecutor`] / [`AsyncApp`] / [`BackgroundExecutor`] / [`Task<T>`] / [`Timer`] / [`UiWakeSignal`] / [`StdAsyncDriver`] / [`block_on`]。

mod scheduler;

pub use scheduler::{
    run_async_event_loop, AsyncDriver, AsyncLoopConfig, AsyncLoopReport, SchedulerCapabilities,
    SchedulerClock, YieldNow,
};

mod backend;
mod foreground;
mod manual;
mod std_scheduler;
mod task;

pub use backend::{BoxFuture, SchedulerBackend, StdAsyncDriver, UiWakeFuture, UiWakeSignal};
pub use foreground::{AsyncApp, ForegroundExecutor};
pub use manual::ManualScheduler;
pub use std_scheduler::{block_on, BackgroundExecutor, NativeStdScheduler, Timer};
pub use task::Task;
