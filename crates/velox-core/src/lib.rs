//! # Velox Core
//!
//! 跨平台桌面、移动端与 WebAssembly 轻量 UI 引擎内核。
//!
//! 提供：
//! - 帧级瞬态内存管理（`ScratchArena`、`SmallVec`、`StrRef`、`fmt!`）；
//! - 状态槽位与局部重绘边界（`PodPool`、`Handle<T>`、`View`、`Widget`）；
//! - `div()` + Tailwind 风格 24 字节紧凑样式构建器（`Style`、`Styled`）；
//! - `i16` 四步一维解耦线性弹性布局求解器（`Two-Pass Axis Solver`）；
//! - 跨帧过渡动画插值池（`MotionCache`）与定长 8 槽位脏矩形合并器（`DamageTracker`）；
//! - 跨平台窗口与事件层（[`platform`]：`testing`、`wayland`、`winit`、`mobile`、`web`）；
//! - 双渲染器层（[`renderers`]：内置多核 `software` 与默认 `gpu` 自研轻量级 `wgpu` 2D SDF 渲染器）。

pub mod a11y;
pub mod app;
pub mod arena;
pub mod async_rt;
pub mod damage;
pub mod display;
pub mod element;
pub mod engine;
pub mod i18n;
pub mod motion;
pub mod platform;
pub mod pod;
pub mod prelude;
pub mod renderers;
pub mod sdf;
pub mod signal;
pub mod solver;
pub mod style;
pub mod style_ext;
pub mod svg;
pub mod theme;

pub use engine::{GestureConfig, VeloxEngine, MAX_DRAW_OPS, MAX_HIT_REGIONS};
pub use renderers::software as raster;
pub use signal::{batch, computed, computed_eq, effect, signal, Computed, Effect, Signal};
pub use theme::{CoreTheme, Theme};
