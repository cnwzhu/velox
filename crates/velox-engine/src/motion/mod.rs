//! 扁平绘制原语（`DrawOp`）、跨帧状态过渡插值池（`MotionCache`）与声明式动画原语（`Tween` / `Presence` / `Spring`）。

mod cache;
mod draw_op;
mod spring;
mod tween;

pub use cache::{MotionCache, MotionState};
pub use draw_op::{pack_subpx_q4, unpack_subpx_q4, DrawOp, DrawOpSnapshot, DrawPayload};
pub use spring::{rubberband, rubberband_i16, Spring, SpringConfig};
pub use tween::{Presence, Tween};
