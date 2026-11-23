//! 双模内存管理原语：`ScratchArena`、`SmallVec<T, N>` 与 `StrRef<'a>`。

mod scratch;
mod small_vec;
mod str_ref;

pub use scratch::{ScratchArena, SCRATCH_CAPACITY};
pub use small_vec::{SmallString, SmallVec};
pub use str_ref::StrRef;
