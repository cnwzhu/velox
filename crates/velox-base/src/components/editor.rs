//! 源代码编辑器组件 `Editor` / `CodeEditor` 与参照 `cosmic-text` 构建的核心文本编辑原语。

pub use crate::input::{
    Action, Affinity, Align, Attrs, AttrsList, AttrsSpan, Buffer, BufferLine, BufferPoint, Change,
    ChangeItem, CodeEditor, CosmicEditor, Cursor, CursorSelection, DiagnosticSeverity,
    DiagnosticSpan, Edit, EditSelection, Editor, FontStyle, FontWeight, HighlightKind,
    HighlightSpan, LayoutCursor, LayoutGlyph, LayoutLine, LayoutRun, LineEnding, Metrics, Motion,
    PlainSyntaxHighlighter, RustSyntaxHighlighter, Scroll, Selection, SyntaxEditor,
    SyntaxHighlighter, SyntaxTheme, TabSize, TextDecoration, TextDocBuffer, Wrap,
};
