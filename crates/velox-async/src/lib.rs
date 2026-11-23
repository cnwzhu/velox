//! # Velox Async (`velox-async`)
//!
//! 面向多异步运行时的统一调度器适配层 crate，通过 Cargo feature 按需开启（默认 0 外部异步运行时依赖）：
//! - `tokio` feature：启用 [`tokio::TokioScheduler`]、[`VeloxTokioExt`]、[`VeloxEngineTokioExt`]；
//! - `smol` feature：启用 [`smol::SmolScheduler`]、[`VeloxSmolExt`]、[`VeloxEngineSmolExt`]；
//! - 同时透传 [`velox_core::async_rt`] 的核心跨调度器抽象（`SchedulerBackend`、`AsyncDriver`、`run_async_event_loop` 等）。

#![forbid(unsafe_code)]


pub use velox_core::async_rt::*;

#[cfg(feature = "tokio")]
pub mod tokio;

#[cfg(feature = "smol")]
pub mod smol;

#[cfg(feature = "tokio")]
pub use crate::tokio::{TokioScheduler, VeloxEngineTokioExt, VeloxTokioExt};

#[cfg(feature = "smol")]
pub use crate::smol::{SmolScheduler, VeloxEngineSmolExt, VeloxSmolExt};
