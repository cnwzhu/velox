//! 扁平绘制原语（`DrawOp`）、跨帧状态过渡插值池（`MotionCache`）与声明式动画原语（`Tween` / `Presence`）。

mod cache;
mod draw_op;
mod tween;

pub use cache::{MotionCache, MotionState};
pub use draw_op::{DrawOp, DrawOpSnapshot, DrawPayload};
pub use tween::{Presence, Tween};
