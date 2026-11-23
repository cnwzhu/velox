//! 按行增量语法高亮装饰器（[`SyntaxEditor`]）与零依赖词法高亮器特征（[`SyntaxHighlighter`]）。
//!
//! 参照 `pop-os/cosmic-text/src/edit/syntect.rs` 架构设计，同时保持 100% `#![no_std]` 与零外部重依赖：
//! - 维护每行行尾词法状态 `line_states`（支持 `/* ... */` 跨行多级嵌套块注释）；
//! - 编辑某行后仅从受影响的起始行开始增量重染，一旦某行行尾状态与缓存一致即刻提前终止（Early Cutoff）；
//! - 语法着色直接写入 `BufferLine::attrs_list`，彻底消除每帧全局重扫整个文档的 $O(N)$ 性能开销。

use velox_engine::arena::{SmallString, SmallVec};
use velox_engine::pod::{FrameScope, KeyCode, KeyModifiers};
use velox_engine::style::Color;
use velox_engine::theme::CoreTheme;

use super::super::attrs::{Attrs, AttrsList};
use super::super::buffer::TextDocBuffer;
use super::super::cursor::Cursor;
use super::{Action, Change, CosmicEditor, Edit, EditSelection};

/// 代码语法高亮配色板（同时内置高对比度暗色/亮色预设，可从 `CoreTheme` 一键派生）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SyntaxTheme {
    pub plain: Color,
    pub keyword: Color,
    pub ty: Color,
    pub function: Color,
    pub macro_call: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    pub attribute: Color,
    pub operator: Color,
}

impl SyntaxTheme {
    /// 深色模式高辨识度代码高亮配色（对标 VS Code Dark+ / Zed One Dark）。
    pub const fn dark() -> Self {
        Self {
            plain: Color::rgb(228, 228, 231),
            keyword: Color::rgb(192, 132, 252),
            ty: Color::rgb(56, 189, 248),
            function: Color::rgb(96, 165, 250),
            macro_call: Color::rgb(244, 114, 182),
            string: Color::rgb(134, 239, 172),
            comment: Color::rgb(113, 113, 122),
            number: Color::rgb(251, 146, 60),
            attribute: Color::rgb(250, 204, 21),
            operator: Color::rgb(148, 163, 184),
        }
    }

    /// 浅色模式高对比度代码高亮配色（对标 VS Code Light+ / Zed One Light）。
    pub const fn light() -> Self {
        Self {
            plain: Color::rgb(24, 24, 27),
            keyword: Color::rgb(147, 51, 234),
            ty: Color::rgb(2, 132, 199),
            function: Color::rgb(37, 99, 235),
            macro_call: Color::rgb(219, 39, 119),
            string: Color::rgb(22, 163, 74),
            comment: Color::rgb(113, 113, 122),
            number: Color::rgb(234, 88, 12),
            attribute: Color::rgb(161, 98, 7),
            operator: Color::rgb(71, 85, 105),
        }
    }

    /// 依据当前 [`CoreTheme`] 明暗背景亮度自动挑选对应的暗/亮语法配色板。
    pub fn from_core_theme(theme: &CoreTheme) -> Self {
        let bg = theme.background;
        let luma = (bg.r() as u16) * 3 + (bg.g() as u16) * 6 + (bg.b() as u16);
        let mut st = if luma < 1280 {
            Self::dark()
        } else {
            Self::light()
        };
        st.plain = theme.foreground;
        st
    }
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        Self::dark()
    }
}

/// 增量行级词法高亮器特征（对应 `cosmic-text` `SyntaxSystem` 抽象）。
///
/// 接收单行文本切片 `line` 与上一行末尾传出的词法状态 `state_in`（如块注释嵌套深度），
/// 将本行所有着色区间写入 `attrs`，并返回传给下一行的 `state_out`。
pub trait SyntaxHighlighter {
    fn highlight_line(
        &self,
        line: &str,
        state_in: u16,
        theme: &SyntaxTheme,
        attrs: &mut AttrsList,
    ) -> u16;
}

/// 纯文本无高亮占位实现。
#[derive(Copy, Clone, Debug, Default)]
pub struct PlainSyntaxHighlighter;

impl SyntaxHighlighter for PlainSyntaxHighlighter {
    #[inline]
    fn highlight_line(
        &self,
        _line: &str,
        _state_in: u16,
        _theme: &SyntaxTheme,
        attrs: &mut AttrsList,
    ) -> u16 {
        attrs.clear_spans();
        0
    }
}

/// 内置零依赖 `#![no_std]` Rust 增量词法高亮器（支持跨行嵌套块注释 `/* ... */`、宏、属性、函数与类型识别）。
#[derive(Copy, Clone, Debug, Default)]
pub struct RustSyntaxHighlighter;

impl RustSyntaxHighlighter {
    fn is_keyword(word: &str) -> bool {
        matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        )
    }

    fn is_primitive_or_builtin_type(word: &str) -> bool {
        matches!(
            word,
            "bool"
                | "char"
                | "str"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "f32"
                | "f64"
                | "Option"
                | "Result"
                | "Some"
                | "None"
                | "Ok"
                | "Err"
                | "String"
                | "Vec"
                | "Box"
                | "Arc"
                | "Rc"
        )
    }
}

impl SyntaxHighlighter for RustSyntaxHighlighter {
    fn highlight_line(
        &self,
        line: &str,
        mut block_comment_depth: u16,
        theme: &SyntaxTheme,
        attrs: &mut AttrsList,
    ) -> u16 {
        attrs.reset(Attrs::new().color(theme.plain));
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut i = 0usize;

        while i < len {
            // 1. 正在多行块注释内部（支持 Rust 嵌套块注释 `/* /* ... */ */`）
            if block_comment_depth > 0 {
                let start = i;
                while i < len && block_comment_depth > 0 {
                    if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        block_comment_depth = block_comment_depth.saturating_add(1);
                        i += 2;
                    } else if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        block_comment_depth = block_comment_depth.saturating_sub(1);
                        i += 2;
                    } else {
                        let step = line[i..].chars().next().map_or(1, |c| c.len_utf8());
                        i += step;
                    }
                }
                attrs.add_span(start..i, Attrs::new().color(theme.comment));
                continue;
            }

            // 2. 单行注释 `// ...`
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                attrs.add_span(i..len, Attrs::new().color(theme.comment));
                break;
            }

            // 3. 块注释开始 `/*`
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                let start = i;
                block_comment_depth = 1;
                i += 2;
                while i < len && block_comment_depth > 0 {
                    if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        block_comment_depth = block_comment_depth.saturating_add(1);
                        i += 2;
                    } else if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        block_comment_depth = block_comment_depth.saturating_sub(1);
                        i += 2;
                    } else {
                        let step = line[i..].chars().next().map_or(1, |c| c.len_utf8());
                        i += step;
                    }
                }
                attrs.add_span(start..i, Attrs::new().color(theme.comment));
                continue;
            }

            // 4. 属性宏 `#[...]` / `#![...]`
            if bytes[i] == b'#'
                && ((i + 1 < len && bytes[i + 1] == b'[')
                    || (i + 2 < len && bytes[i + 1] == b'!' && bytes[i + 2] == b'['))
            {
                let start = i;
                let mut depth = 0u32;
                while i < len {
                    if bytes[i] == b'[' {
                        depth += 1;
                    } else if bytes[i] == b']' {
                        depth = depth.saturating_sub(1);
                        i += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    let step = line[i..].chars().next().map_or(1, |c| c.len_utf8());
                    i += step;
                }
                attrs.add_span(start..i, Attrs::new().color(theme.attribute));
                continue;
            }

            // 5. 字符串字面量 `"..."`
            if bytes[i] == b'"' {
                let start = i;
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        let step = line[i + 1..].chars().next().map_or(1, |c| c.len_utf8());
                        i += 1 + step;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    let step = line[i..].chars().next().map_or(1, |c| c.len_utf8());
                    i += step;
                }
                attrs.add_span(start..i, Attrs::new().color(theme.string));
                continue;
            }

            // 6. 字符字面量 `'x'` 或生命周期 `'a`
            if bytes[i] == b'\'' {
                let start = i;
                if i + 2 < len && bytes[i + 1] == b'\\' {
                    i += 2;
                    if i < len {
                        let step = line[i..].chars().next().map_or(1, |c| c.len_utf8());
                        i += step;
                    }
                    if i < len && bytes[i] == b'\'' {
                        i += 1;
                    }
                    attrs.add_span(start..i, Attrs::new().color(theme.string));
                    continue;
                }
                if let Some(ch) = line[i + 1..].chars().next() {
                    let after_ch = i + 1 + ch.len_utf8();
                    if after_ch < len && bytes[after_ch] == b'\'' {
                        i = after_ch + 1;
                        attrs.add_span(start..i, Attrs::new().color(theme.string));
                        continue;
                    }
                    // 生命周期 `'a` / `'static`
                    if ch.is_ascii_alphabetic() || ch == '_' {
                        i = after_ch;
                        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                            i += 1;
                        }
                        attrs.add_span(start..i, Attrs::new().color(theme.attribute));
                        continue;
                    }
                }
                i += 1;
                continue;
            }

            // 7. 数字字面量（含十六进制/下划线/后缀 `42u32`, `0xFF`, `3.14`）
            if bytes[i].is_ascii_digit() {
                let start = i;
                i += 1;
                while i < len
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
                {
                    if bytes[i] == b'.' && i + 1 < len && bytes[i + 1] == b'.' {
                        break;
                    }
                    i += 1;
                }
                attrs.add_span(start..i, Attrs::new().color(theme.number));
                continue;
            }

            // 8. 标识符：关键字 / 类型 / 宏调用 / 函数调用
            if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                let start = i;
                i += 1;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &line[start..i];
                if Self::is_keyword(word) {
                    attrs.add_span(start..i, Attrs::new().color(theme.keyword));
                } else if i < len && bytes[i] == b'!' {
                    attrs.add_span(start..(i + 1), Attrs::new().color(theme.macro_call));
                    i += 1;
                } else if Self::is_primitive_or_builtin_type(word)
                    || word.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
                {
                    attrs.add_span(start..i, Attrs::new().color(theme.ty));
                } else if i < len && bytes[i] == b'(' {
                    attrs.add_span(start..i, Attrs::new().color(theme.function));
                }
                continue;
            }

            // 9. 常见操作符与标点
            if matches!(
                bytes[i],
                b'=' | b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'&' | b'|' | b'^' | b'!'
            ) {
                let start = i;
                i += 1;
                attrs.add_span(start..i, Attrs::new().color(theme.operator));
                continue;
            }

            let step = line[i..].chars().next().map_or(1, |c| c.len_utf8());
            i += step;
        }

        block_comment_depth
    }
}

/// 带行级增量语法高亮的编辑器包装器（对应 `cosmic-text::SyntaxEditor`）。
///
/// 实现与 [`CosmicEditor`] 相同的 [`Edit`] 特征，并在每次内容变化或主题切换时仅重染脏行。
#[derive(Clone, Debug)]
pub struct SyntaxEditor<H: SyntaxHighlighter = RustSyntaxHighlighter> {
    editor: CosmicEditor,
    highlighter: H,
    theme: SyntaxTheme,
    line_states: SmallVec<u16, 16>,
    dirty_from: Option<usize>,
}

impl<H: SyntaxHighlighter> SyntaxEditor<H> {
    /// 使用指定 [`CosmicEditor`] 与语法高亮器创建增量高亮编辑器。
    pub fn new(editor: CosmicEditor, highlighter: H, theme: SyntaxTheme) -> Self {
        let mut s = Self {
            editor,
            highlighter,
            theme,
            line_states: SmallVec::new(),
            dirty_from: Some(0),
        };
        s.update_highlighting();
        s
    }

    /// 便捷构造函数：以初始代码与度量创建带增量语法高亮的编辑器。
    pub fn from_str(
        metrics: super::super::shape::Metrics,
        initial: &str,
        highlighter: H,
        theme: SyntaxTheme,
    ) -> Self {
        Self::new(CosmicEditor::from_str(metrics, initial), highlighter, theme)
    }

    #[inline(always)]
    pub fn inner(&self) -> &CosmicEditor {
        &self.editor
    }

    #[inline(always)]
    pub fn inner_mut(&mut self) -> &mut CosmicEditor {
        self.mark_dirty_from(0);
        &mut self.editor
    }

    /// 同步明暗主题（仅在配色板确实变化时重染）。
    pub fn sync_theme(&mut self, core_theme: &CoreTheme) {
        let next = SyntaxTheme::from_core_theme(core_theme);
        if self.theme != next {
            self.theme = next;
            self.dirty_from = Some(0);
            self.line_states.clear();
            self.update_highlighting();
        }
    }

    #[inline]
    pub fn mark_dirty_from(&mut self, line: usize) {
        self.dirty_from = Some(match self.dirty_from {
            Some(prev) => prev.min(line),
            None => line,
        });
    }

    /// 执行按行增量语法高亮（状态收敛时提前中止）。
    pub fn update_highlighting(&mut self) {
        let Some(start_line) = self.dirty_from.take() else {
            return;
        };
        let line_count = self.editor.buffer_ref().lines.len();
        let line_count_changed = self.line_states.len() != line_count;
        if line_count == 0 {
            self.line_states.clear();
            return;
        }
        let start = start_line.min(line_count - 1);
        let mut state = if start == 0 {
            0
        } else {
            self.line_states.as_slice().get(start - 1).copied().unwrap_or(0)
        };

        while self.line_states.len() < line_count {
            let _ = self.line_states.push(0);
        }
        self.line_states.truncate(line_count);

        for li in start..line_count {
            let mut new_attrs = AttrsList::new(Attrs::new().color(self.theme.plain));
            let next_state = {
                let text = self.editor.buffer_ref().lines[li].text();
                self.highlighter
                    .highlight_line(text, state, &self.theme, &mut new_attrs)
            };
            let prev_state = self.line_states[li];
            let attrs_changed = self.editor.buffer_mut().lines.as_mut_slice()[li]
                .set_attrs_list(new_attrs);
            self.line_states[li] = next_state;
            state = next_state;

            // 提前截断：超过修改行后若词法状态收敛且属性未变，无需再扫描后续数千行
            if li > start && !line_count_changed && next_state == prev_state && !attrs_changed {
                break;
            }
        }
    }

    /// 便捷转发：处理带修饰键的键盘事件并立即增量更新语法高亮。
    pub fn handle_key_with_mods(&mut self, key: KeyCode, mods: KeyModifiers) -> bool {
        let before_line = self.editor.cursor().line;
        let before_sel_line = self
            .editor
            .selection_bounds()
            .map_or(before_line, |(s, _)| s.line);
        let before_lines = self.editor.line_count();
        let before_bytes = self.editor.len();
        let is_undo_redo = (mods.ctrl() || mods.logo())
            && matches!(
                key,
                KeyCode::Char('z') | KeyCode::Char('Z') | KeyCode::Char('y') | KeyCode::Char('Y')
            );
        let handled = self.editor.handle_key_with_mods(key, mods);
        if handled
            && (is_undo_redo
                || self.editor.line_count() != before_lines
                || self.editor.len() != before_bytes
                || matches!(
                    key,
                    KeyCode::Char(_)
                        | KeyCode::Space
                        | KeyCode::Enter
                        | KeyCode::Backspace
                        | KeyCode::Delete
                        | KeyCode::Tab
                        | KeyCode::BackTab
                ))
        {
            if is_undo_redo {
                self.mark_dirty_from(0);
            } else {
                let dirty_start = before_sel_line
                    .min(before_line)
                    .min(self.editor.cursor().line);
                self.mark_dirty_from(dirty_start);
            }
            self.update_highlighting();
        }
        handled
    }

    #[inline]
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        self.handle_key_with_mods(key, KeyModifiers::NONE)
    }

    #[inline]
    pub fn handle_pointer_press(&mut self, x: i32, y: i32, now_ms: u32, shift: bool) {
        self.editor.handle_pointer_press(x, y, now_ms, shift);
    }

    #[inline]
    pub fn handle_pointer_drag(&mut self, x: i32, y: i32) {
        self.editor.handle_pointer_drag(x, y);
    }

    #[inline]
    pub const fn blink_epoch_ms(&self) -> u32 {
        self.editor.blink_epoch_ms()
    }

    #[inline]
    pub fn reset_blink(&mut self, now_ms: u32) {
        self.editor.reset_blink(now_ms);
    }

    #[inline]
    pub fn copy_to_clipboard(&self) -> bool {
        self.editor.copy_to_clipboard()
    }

    pub fn cut_to_clipboard(&mut self) -> bool {
        let start_line = self
            .editor
            .selection_bounds()
            .map_or(self.editor.cursor().line, |(s, _)| s.line);
        let ok = self.editor.cut_to_clipboard();
        if ok {
            self.mark_dirty_from(start_line);
            self.update_highlighting();
        }
        ok
    }

    pub fn paste_from_clipboard(&mut self) -> bool {
        let start_line = self
            .editor
            .selection_bounds()
            .map_or(self.editor.cursor().line, |(s, _)| s.line);
        let ok = self.editor.paste_from_clipboard();
        if ok {
            self.mark_dirty_from(start_line);
            self.update_highlighting();
        }
        ok
    }

    pub fn undo(&mut self) -> bool {
        let ok = self.editor.undo();
        if ok {
            self.mark_dirty_from(0);
            self.update_highlighting();
        }
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self.editor.redo();
        if ok {
            self.mark_dirty_from(0);
            self.update_highlighting();
        }
        ok
    }

    #[inline]
    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    #[inline]
    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.editor.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.editor.is_empty()
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        self.editor.line_count()
    }

    #[inline]
    pub fn selected_byte_len(&self) -> usize {
        self.editor.selected_byte_len()
    }

    #[inline]
    pub fn to_flat_string<const CAP: usize>(&self) -> SmallString<CAP> {
        self.editor.to_flat_string()
    }

    #[inline]
    pub fn to_arena_str<'a>(&self, cx: &'a FrameScope) -> &'a str {
        self.editor.to_arena_str(cx)
    }
}

impl<H: SyntaxHighlighter> Edit for SyntaxEditor<H> {
    #[inline]
    fn buffer_ref(&self) -> &TextDocBuffer {
        self.editor.buffer_ref()
    }

    #[inline]
    fn buffer_mut(&mut self) -> &mut TextDocBuffer {
        self.editor.buffer_mut()
    }

    #[inline]
    fn cursor(&self) -> Cursor {
        self.editor.cursor()
    }

    #[inline]
    fn set_cursor(&mut self, cursor: Cursor) {
        self.editor.set_cursor(cursor);
    }

    #[inline]
    fn selection(&self) -> EditSelection {
        self.editor.selection()
    }

    #[inline]
    fn set_selection(&mut self, selection: EditSelection) {
        self.editor.set_selection(selection);
    }

    #[inline]
    fn selection_bounds(&self) -> Option<(Cursor, Cursor)> {
        self.editor.selection_bounds()
    }

    #[inline]
    fn auto_indent(&self) -> bool {
        self.editor.auto_indent()
    }

    #[inline]
    fn set_auto_indent(&mut self, auto_indent: bool) {
        self.editor.set_auto_indent(auto_indent);
    }

    #[inline]
    fn tab_width(&self) -> u16 {
        self.editor.tab_width()
    }

    #[inline]
    fn set_tab_width(&mut self, tab_width: u16) {
        self.editor.set_tab_width(tab_width);
    }

    fn shape_as_needed(&mut self, prune: bool) {
        self.update_highlighting();
        self.editor.shape_as_needed(prune);
    }

    fn delete_range(&mut self, start: Cursor, end: Cursor) {
        let min_line = start.line.min(end.line);
        self.editor.delete_range(start, end);
        self.mark_dirty_from(min_line);
        self.update_highlighting();
    }

    fn insert_at(
        &mut self,
        cursor: Cursor,
        data: &str,
        attrs_list: Option<AttrsList>,
    ) -> Cursor {
        let res = self.editor.insert_at(cursor, data, attrs_list);
        self.mark_dirty_from(cursor.line);
        self.update_highlighting();
        res
    }

    #[inline]
    fn copy_selection(&self) -> Option<SmallString<1024>> {
        self.editor.copy_selection()
    }

    fn delete_selection(&mut self) -> bool {
        let line = self
            .editor
            .selection_bounds()
            .map_or(self.editor.cursor().line, |(s, _)| s.line);
        let ok = self.editor.delete_selection();
        if ok {
            self.mark_dirty_from(line);
            self.update_highlighting();
        }
        ok
    }

    fn apply_change(&mut self, change: &Change) -> bool {
        let ok = self.editor.apply_change(change);
        if ok {
            self.mark_dirty_from(0);
            self.update_highlighting();
        }
        ok
    }

    #[inline]
    fn start_change(&mut self) {
        self.editor.start_change();
    }

    #[inline]
    fn finish_change(&mut self) -> Option<Change> {
        self.editor.finish_change()
    }

    fn action(&mut self, action: Action) {
        let before_line = self.editor.cursor().line;
        let is_mutating = matches!(
            action,
            Action::Insert(_)
                | Action::Enter
                | Action::Backspace
                | Action::Delete
                | Action::Indent
                | Action::Unindent
        );
        self.editor.action(action);
        if is_mutating {
            self.mark_dirty_from(before_line.min(self.editor.cursor().line));
            self.update_highlighting();
        }
    }

    #[inline]
    fn cursor_position(&mut self) -> Option<(i32, i32)> {
        self.update_highlighting();
        self.editor.cursor_position()
    }

    fn insert_string(&mut self, data: &str, attrs_list: Option<AttrsList>) {
        let start_line = self
            .editor
            .selection_bounds()
            .map_or(self.editor.cursor().line, |(s, _)| s.line);
        self.editor.insert_string(data, attrs_list);
        self.mark_dirty_from(start_line);
        self.update_highlighting();
    }

    #[inline]
    fn select_all(&mut self) {
        self.editor.select_all();
    }
}
