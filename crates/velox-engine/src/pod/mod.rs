//! `PodPool` 状态槽位池、`Handle<T>` 强类型句柄、输入事件与 `Scope` / `FrameScope` 上下文。

mod events;
mod handle;
mod layout_cache;
mod pool;
mod scope;
mod theme;

pub use events::{
    ClickEvent, CursorShape, DragEvent, DragPayload, EventPhase, ImePreedit, InputEvent, KeyCode,
    KeyModifiers, PointerButton, PointerKind, SafeAreaInsets, SwipeDir,
};
pub use handle::{Handle, PodId};
pub use pool::{PodPool, PodReadGuard, MAX_PODS, POD_SLOT_BYTES};
pub use scope::{derive_scoped_id, ActionScope, FrameScope, Scope};
pub use theme::CoreTheme;
