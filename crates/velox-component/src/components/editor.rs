//! 主题化源代码编辑器组件 `CodeEditor` / `Editor`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::{BufferPoint, CursorSelection, Selection, TabSize};
use velox_engine::prelude::*;

pub use velox_base::components::editor::{
    Action, Affinity, Align, Attrs, AttrsList, AttrsSpan, Buffer, BufferLine, Change, ChangeItem,
    CosmicEditor, Cursor, DiagnosticSeverity, DiagnosticSpan, Edit, EditSelection, FontStyle,
    FontWeight, HighlightKind, HighlightSpan, LayoutCursor, LayoutGlyph, LayoutLine, LayoutRun,
    LineEnding, Metrics, Motion, PlainSyntaxHighlighter, RustSyntaxHighlighter, Scroll,
    SyntaxEditor, SyntaxHighlighter, SyntaxTheme, TextDecoration, TextDocBuffer, Wrap,
};

crate::define_themed_component! {
    name: CodeEditor<'a>,
    base: velox_base::CodeEditor<'a>,
    new: (id: u32, value: &'a str, focused: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        let p = theme.primary;
        let w = theme.warning;
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.gutter_bg, theme.secondary);
        fill_color(&mut base.active_line_bg, theme.list_hover);
        fill_color(&mut base.selection_bg, Color::rgba(p.r(), p.g(), p.b(), 102));
        fill_color(&mut base.match_bg, Color::rgba(w.r(), w.g(), w.b(), 88));
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.focus_border_color, theme.ring);
        fill_color(&mut base.divider_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.active_line_num_color, theme.foreground);
        fill_color(&mut base.cursor_color, theme.primary);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        selection(selection: CursorSelection, cursor_point: BufferPoint),
        size(width: i16, height: i16),
        font_size(font_size: u8),
        line_height(line_height: i16),
        tab_size(tab_size: TabSize),
        line_numbers(show: bool),
        indent_guides(show: bool),
        status_bar(show: bool),
        cursor_blink(enabled: bool),
        blink_epoch(epoch_ms: u32),
        highlights(spans: &'a [HighlightSpan]),
        search_matches(matches: &'a [Selection]),
        diagnostics(diagnostics: &'a [DiagnosticSpan<'a>]),
        on_focus(handler: &'a dyn Fn(&mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
        on_pointer_select(handler: &'a dyn Fn((BufferPoint, bool), &mut ActionScope)),
        on_secondary_click(handler: &'a dyn Fn((BufferPoint, i16, i16), &mut ActionScope)),
        on_pointer_action(handler: &'a dyn Fn((i32, i32, bool, u32, bool), &mut ActionScope)),
    ],
}

impl<'a> CodeEditor<'a> {
    /// 基于 `cosmic-text` 架构的 [`velox_base::TextDocBuffer`] 构造主题化代码编辑器。
    #[inline]
    pub fn from_buffer(
        cx: &'a FrameScope,
        id: u32,
        buffer: &velox_base::TextDocBuffer,
        cursor: velox_base::Cursor,
        selection_bounds: Option<(velox_base::Cursor, velox_base::Cursor)>,
        focused: bool,
    ) -> Self {
        Self::from_base(velox_base::CodeEditor::from_buffer(
            cx,
            id,
            buffer,
            cursor,
            selection_bounds,
            focused,
        ))
    }

    /// 直接从任意 [`velox_base::Edit`] 状态机构造主题化代码编辑器。
    #[inline]
    pub fn from_edit<E: velox_base::Edit>(
        cx: &'a FrameScope,
        id: u32,
        edit: &E,
        focused: bool,
    ) -> Self {
        Self::from_base(velox_base::CodeEditor::from_edit(cx, id, edit, focused))
    }
}

/// `CodeEditor` 主题化简短别名。

pub type Editor<'a> = CodeEditor<'a>;
