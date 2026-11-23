//! 参照 `pop-os/cosmic-text/tree/main/src` 重构的 Velox 多行文本排版与代码编辑引擎（完全兼容 `#![no_std]` 与 `std`）。
//!
//! 模块映射对照 `pop-os/cosmic-text/src/`：
//! - [`line_ending`]（`line_ending.rs`）：行尾约定 [`LineEnding`] 与零分配多行切分；
//! - [`attrs`]（`attrs.rs`）：富文本/语法属性 [`Attrs`] 与单行稀疏区间表 [`AttrsList`]；
//! - [`cursor`]（`cursor.rs`）：逻辑光标 [`Cursor`]、软折行亲和方向 [`Affinity`]、视觉光标 [`LayoutCursor`]、20 种导航原语 [`Motion`] 与滚动模型 [`Scroll`]；
//! - [`shape`]（`shape.rs`）：排版度量 [`Metrics`]、软折行策略 [`Wrap`]、对齐 [`Align`] 与排版产出 [`LayoutGlyph`] / [`LayoutLine`] / [`LayoutRun`]；
//! - [`buffer_line`]（`buffer_line.rs`）：单逻辑行 [`BufferLine`] 与惰性单行视觉折行缓存；
//! - [`buffer`]（`buffer.rs`）：多行缓冲区 [`TextDocBuffer`]（别名 [`Buffer`]）、亚像素精确命中测试 `hit(x, y)` 与视觉光标移动 `cursor_motion`；
//! - [`edit`]（`edit/`）：标准动作 [`Action`]、4 种语义选区 [`EditSelection`]、双向原子变更事务 [`Change`] / [`ChangeItem`]、统一特征 [`Edit`]、核心编辑器 [`CosmicEditor`] 与增量语法高亮包装器 [`SyntaxEditor`] / [`RustSyntaxHighlighter`]。

pub mod attrs;
pub mod buffer;
pub mod buffer_line;
pub mod cursor;
pub mod edit;
pub mod line_ending;
pub mod shape;

pub use attrs::{Attrs, AttrsList, AttrsSpan, FontStyle, FontWeight, TextDecoration};
pub use buffer::{Buffer, LayoutRunIter, TextDocBuffer};
pub use buffer_line::BufferLine;
pub use cursor::{Affinity, Cursor, LayoutCursor, Motion, Scroll};
pub use edit::{
    Action, Change, ChangeItem, CosmicEditor, Edit, EditSelection, PlainSyntaxHighlighter,
    RustSyntaxHighlighter, SyntaxEditor, SyntaxHighlighter, SyntaxTheme,
};
pub use line_ending::{split_lines_with_ending, LineEnding, LineEndingIter};
pub use shape::{Align, LayoutGlyph, LayoutLine, LayoutRun, Metrics, Wrap};
