//! 共享文本编辑引擎与三大文本编辑组件体系：
//! - [`text_engine`]：参照 `pop-os/cosmic-text/tree/main/src` 重构的多行排版与代码编辑引擎（[`LineEnding`]、[`Attrs`] / [`AttrsList`]、[`Cursor`] / [`Affinity`] / [`LayoutCursor`] / [`Motion`] / [`Scroll`]、[`Metrics`] / [`Wrap`] / [`Align`] / [`LayoutGlyph`] / [`LayoutLine`] / [`LayoutRun`]、[`BufferLine`]、[`TextDocBuffer`] / [`Buffer`]、[`Action`] / [`EditSelection`] / [`Change`] / [`Edit`] / [`CosmicEditor`] / [`SyntaxEditor`] / [`RustSyntaxHighlighter`]）；
//! - [`text_boundary`]：词法分类（[`CharacterKind`]）、词边界（[`word_range_at`] / [`prev_word_boundary`] / [`next_word_boundary`]）与行边界（[`line_range_at`]）；
//! - [`cursor`]：规范化选区（[`Selection`]）、带方向与列锚点的光标选区（[`CursorSelection`]）、多光标管理（[`Selections`]）与行列坐标（[`BufferPoint`]）；
//! - [`undo_history`]：支持事务分组与容量淘汰的撤销/重做历史栈（[`UndoHistory`]）；
//! - [`mask_pattern`]：格式化输入掩码（[`MaskPattern`] / [`MaskToken`] / [`MASK_CHAR`]）与全角数字规范化（[`normalize_number_char`]）；
//! - [`search`]：文本查找与替换匹配器（[`SearchMatcher`] / [`SearchSession`]）；
//! - [`editor_state`]：多行文档缓冲区状态机（[`TextBuffer`]）、缩进配置（[`TabSize`]）、括号自动配对（[`AutoClosedPairs`]）、诊断标记（[`DiagnosticSpan`] / [`DiagnosticSeverity`]）与语法高亮区间（[`HighlightSpan`] / [`HighlightKind`]）；
//! - [`textarea::Textarea`]：多行文本编辑框组件；
//! - [`editor::CodeEditor`]（别名 [`editor::Editor`]）：带行号栏、缩进参考线、查找高亮、语法染色与状态栏的代码编辑器组件。

pub mod cursor;
pub mod editor;
pub mod editor_state;
pub mod mask_pattern;
pub mod search;
pub mod text_boundary;
pub mod text_engine;
pub mod textarea;
pub mod undo_history;

pub use cursor::{BufferPoint, CursorSelection, Selection, Selections};
pub use editor::{CodeEditor, Editor};
pub use editor_state::{
    AutoClosedPairs, DiagnosticSeverity, DiagnosticSpan, HighlightKind, HighlightSpan, TabSize,
    TextBuffer, TextSnapshot,
};
pub use mask_pattern::{normalize_number_char, MaskPattern, MaskToken, MASK_CHAR};
pub use search::{SearchMatcher, SearchSession};
pub use text_boundary::{
    clip_offset_left, clip_offset_right, line_range_at, next_word_boundary, prev_word_boundary,
    word_range_at, CharacterKind,
};
pub use text_engine::{
    split_lines_with_ending, Action, Affinity, Align, Attrs, AttrsList, AttrsSpan, Buffer,
    BufferLine, Change, ChangeItem, CosmicEditor, Cursor, Edit, EditSelection, FontStyle,
    FontWeight, LayoutCursor, LayoutGlyph, LayoutLine, LayoutRun, LayoutRunIter, LineEnding,
    LineEndingIter, Metrics, Motion, PlainSyntaxHighlighter, RustSyntaxHighlighter, Scroll,
    SyntaxEditor, SyntaxHighlighter, SyntaxTheme, TextDecoration, TextDocBuffer, Wrap,
};
pub use textarea::Textarea;
pub use undo_history::UndoHistory;
