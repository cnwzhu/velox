//! 代码与富文本编辑器组件（[`CodeEditor`] / [`Editor`]，兼容 `#![no_std]`）。
//!
//! 参照 `pop-os/cosmic-text` 渲染管线重构，提供两套零破坏模式：
//! - **一等 `cosmic-text` 模式**（[`CodeEditor::from_edit`] / [`CodeEditor::from_buffer`]）：
//!   直接消费 [`TextDocBuffer`] / [`LayoutRun`] / [`LayoutGlyph`] / [`AttrsList`] 产出：
//!   - 左侧行号栏（含软折行续行指示与诊断圆点 [`DiagnosticSeverity`]）；
//!   - 精准对齐空格/制表位的底层绝对定位缩进参考线；
//!   - 字形级精确选区背景矩形（`LayoutRun::highlight_x_range`，含跨行换行符选中提示）；
//!   - 合并同色连续字形的零拷贝语法高亮文本段（完整保留 TTF 字偶距与 Tab 制表位）；
//!   - 零文字抖动绝对定位叠层光标与亚像素点击/双击选词/三击选行/拖拽选区回调；
//!   - 底部状态栏（`Ln X, Col Y`、选区统计、软折行模式、缩进规格、行数与查找匹配数）。
//! - **经典兼容模式**（[`CodeEditor::new`]）：
//!   100% 保持旧版 `value: &str` + `CursorSelection` + `HighlightSpan` API 兼容，
//!   同时同步升级为零抖动绝对定位光标与精准字符级选区高亮。

use velox_engine::display::{measure_char_advance_f32, measure_line_width_f32};
use velox_engine::prelude::*;

use super::cursor::{BufferPoint, CursorSelection, Selection};
use super::editor_state::{
    DiagnosticSeverity, DiagnosticSpan, HighlightKind, HighlightSpan, TabSize,
};
use super::text_engine::{Cursor, Edit, TextDocBuffer, Wrap};

const AUTO_COLOR: Color = Color(0x0000_0001);
const GUTTER_WIDTH: i16 = 46;
const CONTENT_PAD_X: i8 = 8;

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

#[inline(always)]
fn round_f32_to_i16(v: f32) -> i16 {
    if v <= 0.0 {
        0
    } else if v >= (i16::MAX as f32) {
        i16::MAX
    } else {
        (v + 0.5) as i16
    }
}

/// 烘焙至帧瞬态 `ScratchArena` 的单条合并同色文本段（已剥离 `\t` 并保留精确像素起点 `x`）。
#[derive(Copy, Clone)]
pub(crate) struct BakedSpan<'a> {
    pub x: i16,
    pub text: &'a str,
    pub color_opt: Option<Color>,
    /// 该段起点与上一段右边缘之间是否存在制表位跳跃（存在时采用绝对定位 `.pos(x, 0)` 对齐制表位）。
    pub tab_aligned: bool,
}

/// 烘焙至帧瞬态 `ScratchArena` 的单条视觉排版行快照（零冗余 `LayoutGlyph` 拷贝，极度节省 `#![no_std]` Arena）。
#[derive(Copy, Clone)]
pub(crate) struct BakedRun<'a> {
    pub line_i: usize,
    pub layout_i: usize,
    pub line_text: &'a str,
    pub spans: &'a [BakedSpan<'a>],
    pub doc_offset: usize,
    pub sel_rect: Option<(i16, i16)>,
    pub cursor_x: Option<i16>,
}

/// 烘焙至帧瞬态 `ScratchArena` 的完整只读文档排版快照。
#[derive(Copy, Clone)]
pub(crate) struct BakedBufferSnapshot<'a> {
    pub runs: &'a [BakedRun<'a>],
    pub cursor: Cursor,
    pub selected_bytes: usize,
    pub font_size: u8,
    pub line_height: i16,
    pub tab_width: u16,
    pub wrap: Wrap,
    pub line_count: usize,
    pub total_bytes: usize,
}

pub(crate) fn bake_buffer_snapshot<'a>(
    cx: &'a FrameScope,
    buffer: &TextDocBuffer,
    cursor: Cursor,
    selection_bounds: Option<(Cursor, Cursor)>,
) -> BakedBufferSnapshot<'a> {
    let metrics = buffer.metrics();
    let use_buf_scroll = buffer.scroll().line > 0 || buffer.scroll().vertical > 0.0;
    // 双击选词静止态光标位于选区内部时，将渲染光标对齐至选区末端（符合 VS Code / Zed 视觉直觉）
    let display_cursor = match selection_bounds {
        Some((s, e)) if cursor > s && cursor < e => e,
        _ => cursor,
    };
    let cursor_layout_opt = buffer.layout_cursor_cached(display_cursor);
    let iter = if use_buf_scroll {
        buffer.layout_runs()
    } else {
        buffer.layout_runs_all()
    };

    let mut runs_vec: SmallVec<BakedRun<'a>, 64> = SmallVec::new();
    let mut cached_line_i: Option<usize> = None;
    let mut cached_line_str: &'a str = "";
    let mut cached_doc_offset: usize = 0;

    for run in iter {
        if cached_line_i != Some(run.line_i) {
            cached_line_str = cx.arena().alloc_str(run.text).as_str();
            cached_doc_offset = buffer.cursor_to_byte_offset(Cursor::new(run.line_i, 0));
            cached_line_i = Some(run.line_i);
        }

        // 1. 当场合并同色连续字形段（消除每字形 40B 拷贝与多余布局节点）
        let mut spans_vec: SmallVec<BakedSpan<'a>, 24> = SmallVec::new();
        let glyphs = run.glyphs;
        let text_len = cached_line_str.len();
        let mut gi = 0usize;
        let mut saw_tab = false;

        while gi < glyphs.len() {
            let g0 = glyphs[gi];
            if g0.start >= g0.end
                || g0.end > text_len
                || !cached_line_str.is_char_boundary(g0.start)
                || !cached_line_str.is_char_boundary(g0.end)
            {
                gi += 1;
                continue;
            }
            if &cached_line_str[g0.start..g0.end] == "\t" {
                saw_tab = true;
                gi += 1;
                continue;
            }
            let seg_x = g0.x;
            let seg_start = g0.start;
            let mut seg_end = g0.end;
            let seg_col = g0.color_opt;
            gi += 1;

            while gi < glyphs.len() {
                let gn = glyphs[gi];
                if gn.start != seg_end
                    || gn.end > text_len
                    || gn.start >= gn.end
                    || !cached_line_str.is_char_boundary(gn.end)
                {
                    break;
                }
                let piece = &cached_line_str[gn.start..gn.end];
                if piece == "\t" {
                    break;
                }
                // 空格本身无墨迹，并入前一个同色段可减少约 45% 的 TextRun 与 Arena 节点
                if gn.color_opt != seg_col && piece != " " {
                    break;
                }
                seg_end = gn.end;
                gi += 1;
            }

            let _ = spans_vec.push(BakedSpan {
                x: round_f32_to_i16(seg_x),
                text: &cached_line_str[seg_start..seg_end],
                color_opt: seg_col,
                tab_aligned: saw_tab,
            });
        }

        let spans: &'a [BakedSpan<'a>] = if spans_vec.is_empty() {
            &[]
        } else {
            cx.arena().alloc_iter(spans_vec.as_slice().iter().copied())
        };

        // 2. 预计算字符级精确选区像素区间 (x0, x1)
        let is_last_visual = run.layout_i + 1 >= buffer.line_visual_count(run.line_i);
        let sel_rect = if let Some((sel_s, sel_e)) = selection_bounds {
            if sel_s.line <= run.line_i && run.line_i <= sel_e.line {
                let line_sel_s = if run.line_i == sel_s.line {
                    sel_s.index
                } else {
                    0
                };
                let line_sel_e = if run.line_i == sel_e.line {
                    sel_e.index
                } else {
                    run.text.len()
                };
                let includes_newline = run.line_i < sel_e.line && is_last_visual;
                match run.highlight_x_range(line_sel_s, line_sel_e) {
                    Some((x0, x1)) => {
                        let x1_ext = if includes_newline { x1 + 7.0 } else { x1 };
                        Some((round_f32_to_i16(x0), round_f32_to_i16(x1_ext)))
                    }
                    None if includes_newline => {
                        let bx = run.x_for_index(line_sel_s);
                        Some((round_f32_to_i16(bx), round_f32_to_i16(bx + 7.0)))
                    }
                    None => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        // 3. 预计算 0 抖动叠层光标精确像素 X
        let cursor_here = match cursor_layout_opt {
            Some(lc) => lc.line == run.line_i && lc.layout == run.layout_i,
            None => display_cursor.line == run.line_i && run.layout_i == 0,
        };
        let cursor_x = if cursor_here {
            let raw_x = run.x_for_index(display_cursor.index);
            let adjusted = if display_cursor.index == 0 {
                0.0
            } else if display_cursor.index < run.text.len() {
                (raw_x - 0.85).max(0.0)
            } else {
                raw_x
            };
            Some(round_f32_to_i16(adjusted))
        } else {
            None
        };

        let _ = runs_vec.push(BakedRun {
            line_i: run.line_i,
            layout_i: run.layout_i,
            line_text: cached_line_str,
            spans,
            doc_offset: cached_doc_offset,
            sel_rect,
            cursor_x,
        });
    }

    let runs: &'a [BakedRun<'a>] = if runs_vec.is_empty() {
        &[]
    } else {
        cx.arena().alloc_iter(runs_vec.as_slice().iter().copied())
    };

    let selected_bytes = if let Some((s, e)) = selection_bounds {
        let a = buffer.cursor_to_byte_offset(s);
        let b = buffer.cursor_to_byte_offset(e);
        b.saturating_sub(a)
    } else {
        0
    };

    BakedBufferSnapshot {
        runs,
        cursor,
        selected_bytes,
        font_size: metrics.font_size_u8(),
        line_height: metrics.line_height_i16().max(12),
        tab_width: buffer.tab_width().max(1),
        wrap: buffer.wrap(),
        line_count: buffer.line_count(),
        total_bytes: buffer.total_bytes(),
    }
}

/// 代码与富文本编辑器组件（`CodeEditor`，别名 `Editor`）。
pub struct CodeEditor<'a> {
    pub id: u32,
    pub value: &'a str,
    pub focused: bool,
    pub selection: CursorSelection,
    pub cursor_point: BufferPoint,
    pub tab_size: TabSize,
    pub show_line_numbers: bool,
    pub show_indent_guides: bool,
    pub show_status_bar: bool,
    /// 聚焦时是否启用标准 530ms 周期光标闪烁（默认 `true`）。
    pub cursor_blink: bool,
    /// 最近一次键入或点击时间戳（毫秒），用于立即将光标复位为常亮状态。
    pub blink_epoch_ms: u32,
    pub width: i16,
    pub height: i16,
    pub line_height: i16,
    pub font_size: u8,
    pub highlights: &'a [HighlightSpan],
    pub search_matches: &'a [Selection],
    pub diagnostics: &'a [DiagnosticSpan<'a>],
    pub bg: Color,
    pub gutter_bg: Color,
    pub active_line_bg: Color,
    pub selection_bg: Color,
    pub match_bg: Color,
    pub border_color: Color,
    pub focus_border_color: Color,
    pub divider_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub active_line_num_color: Color,
    pub cursor_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_focus: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    pub on_pointer_select: Option<&'a dyn Fn((BufferPoint, bool), &mut ActionScope)>,
    pub on_secondary_click: Option<&'a dyn Fn((BufferPoint, i16, i16), &mut ActionScope)>,
    /// 亚像素内容区指针交互回调：`(content_x, content_y, is_drag, now_ms, shift)`。
    pub on_pointer_action: Option<&'a dyn Fn((i32, i32, bool, u32, bool), &mut ActionScope)>,
    baked: Option<BakedBufferSnapshot<'a>>,
}

/// 通用代码编辑器类型别名。
pub type Editor<'a> = CodeEditor<'a>;

impl<'a> CodeEditor<'a> {
    /// 创建一个无样式代码编辑器原语（默认零背景、零边框、零圆角与中性选区/光标）。
    #[inline(always)]
    pub const fn new(id: u32, value: &'a str, focused: bool) -> Self {
        let len = value.len();
        Self {
            id,
            value,
            focused,
            selection: CursorSelection::new(0, len, len),
            cursor_point: BufferPoint::new(0, 0),
            tab_size: TabSize::new(2, false),
            show_line_numbers: true,
            show_indent_guides: true,
            show_status_bar: true,
            cursor_blink: true,
            blink_epoch_ms: 0,
            width: 420,
            height: 220,
            line_height: 20,
            font_size: 13,
            highlights: &[],
            search_matches: &[],
            diagnostics: &[],
            bg: Color::TRANSPARENT,
            gutter_bg: Color::TRANSPARENT,
            active_line_bg: Color::TRANSPARENT,
            selection_bg: AUTO_COLOR,
            match_bg: AUTO_COLOR,
            border_color: Color::TRANSPARENT,
            focus_border_color: Color::TRANSPARENT,
            divider_color: Color::TRANSPARENT,
            text_color: AUTO_COLOR,
            muted_color: AUTO_COLOR,
            active_line_num_color: AUTO_COLOR,
            cursor_color: AUTO_COLOR,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_focus: None,
            on_key_input: None,
            on_pointer_select: None,
            on_secondary_click: None,
            on_pointer_action: None,
            baked: None,
        }
    }

    /// 基于参照 `cosmic-text` 重构的 [`TextDocBuffer`] 烘焙帧级零堆渲染快照，创建一等代码编辑器组件。
    ///
    /// 调用前请确保已调用 `buffer.shape_until_scroll(...)` 或 `buffer.layout_all()`。
    pub fn from_buffer(
        cx: &'a FrameScope,
        id: u32,
        buffer: &TextDocBuffer,
        cursor: Cursor,
        selection_bounds: Option<(Cursor, Cursor)>,
        focused: bool,
    ) -> Self {
        let snap = bake_buffer_snapshot(cx, buffer, cursor, selection_bounds);
        let line_idx = cursor.line.min(buffer.lines.len().saturating_sub(1));
        let col_chars = buffer
            .lines
            .as_slice()
            .get(line_idx)
            .map(|l| {
                let mut byte_idx = cursor.index.min(l.text().len());
                while byte_idx > 0 && !l.text().is_char_boundary(byte_idx) {
                    byte_idx -= 1;
                }
                l.text()[..byte_idx].chars().count()
            })
            .unwrap_or(0);
        let mut editor = Self::new(id, "", focused);
        editor.font_size = snap.font_size;
        editor.line_height = snap.line_height;
        editor.tab_size = TabSize::new((snap.tab_width.min(16)) as u8, false);
        editor.cursor_point = BufferPoint::new(line_idx, col_chars);
        editor.baked = Some(snap);
        editor
    }

    /// 直接从任意实现 [`Edit`] 的状态机（如 [`super::text_engine::CosmicEditor`]、[`super::text_engine::SyntaxEditor`]）
    /// 创建一等代码编辑器组件。
    #[inline]
    pub fn from_edit<E: Edit>(cx: &'a FrameScope, id: u32, edit: &E, focused: bool) -> Self {
        Self::from_buffer(
            cx,
            id,
            edit.buffer_ref(),
            edit.cursor(),
            edit.selection_bounds(),
            focused,
        )
    }

    #[inline(always)]
    pub const fn selection(mut self, selection: CursorSelection, cursor_point: BufferPoint) -> Self {
        self.selection = selection;
        self.cursor_point = cursor_point;
        self
    }

    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    #[inline(always)]
    pub const fn font_size(mut self, font_size: u8) -> Self {
        self.font_size = font_size;
        self
    }

    #[inline(always)]
    pub const fn line_height(mut self, line_height: i16) -> Self {
        self.line_height = line_height;
        self
    }

    #[inline(always)]
    pub const fn tab_size(mut self, tab_size: TabSize) -> Self {
        self.tab_size = tab_size;
        self
    }

    #[inline(always)]
    pub const fn line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    #[inline(always)]
    pub const fn indent_guides(mut self, show: bool) -> Self {
        self.show_indent_guides = show;
        self
    }

    #[inline(always)]
    pub const fn status_bar(mut self, show: bool) -> Self {
        self.show_status_bar = show;
        self
    }

    #[inline(always)]
    pub const fn cursor_blink(mut self, enabled: bool) -> Self {
        self.cursor_blink = enabled;
        self
    }

    #[inline(always)]
    pub const fn blink_epoch(mut self, epoch_ms: u32) -> Self {
        self.blink_epoch_ms = epoch_ms;
        self
    }

    #[inline(always)]
    pub const fn highlights(mut self, spans: &'a [HighlightSpan]) -> Self {
        self.highlights = spans;
        self
    }

    #[inline(always)]
    pub const fn search_matches(mut self, matches: &'a [Selection]) -> Self {
        self.search_matches = matches;
        self
    }

    #[inline(always)]
    pub const fn diagnostics(mut self, diagnostics: &'a [DiagnosticSpan<'a>]) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    #[inline(always)]
    pub fn on_focus(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_focus = Some(handler);
        self
    }

    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// 绑定兼容模式鼠标点击/拖拽文本选区回调：`handler((point, is_drag), acx)`。
    #[inline(always)]
    pub fn on_pointer_select(
        mut self,
        handler: &'a dyn Fn((BufferPoint, bool), &mut ActionScope),
    ) -> Self {
        self.on_pointer_select = Some(handler);
        self
    }

    /// 绑定一等 `cosmic-text` 内容区亚像素指针交互回调：
    /// `handler((content_x, content_y, is_drag, now_ms, shift), acx)`。
    ///
    /// 可直接转发给 `CosmicEditor::handle_pointer_press(x, y, now_ms, shift)` 与 `handle_pointer_drag(x, y)`，
    /// 原生支持亚像素落点、双击选词、三击选行、`Shift+Click` 与拖拽选区。
    #[inline(always)]
    pub fn on_pointer_action(
        mut self,
        handler: &'a dyn Fn((i32, i32, bool, u32, bool), &mut ActionScope),
    ) -> Self {
        self.on_pointer_action = Some(handler);
        self
    }

    /// 绑定鼠标右键点击回调：`handler((point, window_x, window_y), acx)`。
    #[inline(always)]
    pub fn on_secondary_click(
        mut self,
        handler: &'a dyn Fn((BufferPoint, i16, i16), &mut ActionScope),
    ) -> Self {
        self.on_secondary_click = Some(handler);
        self
    }

    fn highlight_color(kind: HighlightKind, default_col: Color) -> Color {
        match kind {
            HighlightKind::Plain => default_col,
            HighlightKind::Keyword => Color::rgb(192, 132, 252),
            HighlightKind::String => Color::rgb(134, 239, 172),
            HighlightKind::Comment => Color::rgb(100, 116, 139),
            HighlightKind::Number => Color::rgb(251, 146, 60),
            HighlightKind::Type => Color::rgb(56, 189, 248),
            HighlightKind::Function => Color::rgb(96, 165, 250),
            HighlightKind::Operator => Color::rgb(244, 114, 182),
        }
    }

    /// 统计行首缩进宽度（按列数计，`\t` 对齐至下一个制表位）。
    fn leading_indent_columns(line: &str, tab_width: usize) -> usize {
        let tw = tab_width.max(1);
        let mut cols = 0usize;
        for b in line.bytes() {
            match b {
                b' ' => cols += 1,
                b'\t' => {
                    let rem = cols % tw;
                    cols += tw - rem;
                }
                _ => break,
            }
        }
        cols
    }

    /// 经典模式：渲染按语法切片分色的单行文本（不切分光标，确保文字 0 抖动）。
    #[allow(clippy::too_many_arguments)]
    fn append_legacy_highlighted_line(
        mut content: Div<'a>,
        cx: &'a FrameScope,
        doc: &'a str,
        line_start: usize,
        line_end: usize,
        highlights: &'a [HighlightSpan],
        font_size: u8,
        default_col: Color,
    ) -> Div<'a> {
        let mut pos = line_start.min(doc.len());
        let end = line_end.min(doc.len());
        while pos < end && !doc.is_char_boundary(pos) {
            pos += 1;
        }
        while pos < end {
            let active = highlights
                .iter()
                .find(|h| h.range.start <= pos && h.range.end > pos);
            let (raw_next, span_col) = if let Some(h) = active {
                (
                    h.range.end.min(end),
                    Self::highlight_color(h.kind, default_col),
                )
            } else {
                let next_start = highlights
                    .iter()
                    .filter(|h| h.range.start > pos && h.range.start < end)
                    .map(|h| h.range.start)
                    .min()
                    .unwrap_or(end);
                (next_start, default_col)
            };

            let mut chunk_end = raw_next.min(end);
            while chunk_end < end && !doc.is_char_boundary(chunk_end) {
                chunk_end += 1;
            }
            if chunk_end > end || !doc.is_char_boundary(chunk_end) {
                chunk_end = end;
                while chunk_end > pos && !doc.is_char_boundary(chunk_end) {
                    chunk_end -= 1;
                }
            }
            if chunk_end <= pos {
                let step = doc[pos..].chars().next().map_or(1, |c| c.len_utf8());
                chunk_end = (pos + step).min(end);
            }
            let slice = &doc[pos..chunk_end];
            if !slice.is_empty() {
                content = content.child(cx, text(slice).size(font_size).color(span_col).nowrap());
            }
            pos = chunk_end;
        }
        content
    }

    /// 一等 `cosmic-text` 模式：按精确浮点字形起点 `span.x` 渲染合并后的同色 [`BakedSpan`] 列表，
    /// 彻底杜绝多段彩色 `Text` 在 `flex_row` 中累加 `+2px` 测量余量，确保文字与选区/光标/鼠标点击 0.0px 严格同源对齐。
    #[allow(clippy::too_many_arguments)]
    fn append_baked_spans(
        mut content: Div<'a>,
        cx: &'a FrameScope,
        run: BakedRun<'a>,
        lh: i16,
        font_size: u8,
        default_col: Color,
    ) -> Div<'a> {
        if run.spans.len() == 1 && !run.spans[0].tab_aligned && run.spans[0].x == 0 {
            let span = run.spans[0];
            let col = span.color_opt.unwrap_or(default_col);
            return content.child(cx, text(span.text).size(font_size).color(col).nowrap());
        }
        for &span in run.spans {
            let col = span.color_opt.unwrap_or(default_col);
            content = content.child(
                cx,
                div()
                    .pos(span.x, 0)
                    .h(lh)
                    .flex_row()
                    .items_center()
                    .child(cx, text(span.text).size(font_size).color(col).nowrap()),
            );
        }
        content
    }
}

crate::impl_base_styled_box!(CodeEditor<'a>);

impl<'a> Widget<'a> for CodeEditor<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let gutter_bg = self.gutter_bg;
        let active_line_bg = self.active_line_bg;
        let selection_bg = resolve_color(self.selection_bg, def_fg.scale_alpha(48));
        let match_bg = resolve_color(self.match_bg, def_fg.scale_alpha(64));
        let border_color = self.border_color;
        let focus_border_color = if self.focus_border_color != Color::TRANSPARENT {
            self.focus_border_color
        } else {
            border_color
        };
        let divider_color = if self.divider_color != Color::TRANSPARENT {
            self.divider_color
        } else if border_color != Color::TRANSPARENT {
            border_color.scale_alpha(95)
        } else {
            Color::TRANSPARENT
        };
        let text_color = resolve_color(self.text_color, def_fg);
        let muted_color = resolve_color(self.muted_color, def_fg.scale_alpha(140));
        let active_line_num_color = resolve_color(self.active_line_num_color, def_fg);
        let cursor_color = resolve_color(self.cursor_color, def_fg);

        let border_col = if self.focused {
            focus_border_color
        } else {
            border_color
        };
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let status_h = if self.show_status_bar { 24 } else { 0 };
        let body_h = if height > 0 {
            (height - status_h - 2).max(40)
        } else {
            360
        };
        let lh = self.line_height.max(12);
        let gutter_w = if self.show_line_numbers {
            GUTTER_WIDTH
        } else {
            0
        };
        let content_origin_x = (gutter_w as i32) + (CONTENT_PAD_X as i32);

        let mut root = div()
            .id(self.id)
            .flex_col()
            .p(1)
            .rounded(self.rounded)
            .bg(bg)
            .overflow_hidden()
            .focusable();
        if border_col != Color::TRANSPARENT {
            root = root.border(1, border_col);
        }
        if self.focus_border_color != Color::TRANSPARENT {
            root = root.focus_border(1, self.focus_border_color);
        } else if !self.focus_ring {
            root = root.no_focus_border();
        }
        root = root.cursor(CursorShape::Text);

        if width > 0 {
            root = root.w(width);
        } else {
            root = root.w_full();
        }
        if height > 0 {
            root = root.h(height);
        } else {
            root = root.flex_grow(1);
        }

        if let Some(cb) = self.on_focus {
            root = root.on_click_raw(cb);
        }
        if let Some(kcb) = self.on_key_input {
            root = root.on_key_input(kcb);
        }
        root = self.style_ext.apply_to_div(root, self.style);

        let mut lines_viewport = div()
            .id(self.id ^ 0x4544_5650)
            .flex_col()
            .w_full()
            .scrollable_auto()
            .cursor(CursorShape::Text);
        lines_viewport = if height > 0 {
            lines_viewport.h(body_h)
        } else {
            lines_viewport.flex_grow(1)
        };

        let cursor_visible = if self.focused {
            if self.cursor_blink {
                cx.request_frame();
                let elapsed = cx.now_ms().wrapping_sub(self.blink_epoch_ms);
                (elapsed / crate::components::text_input::CURSOR_BLINK_HALF_PERIOD_MS) % 2 == 0
            } else {
                true
            }
        } else {
            false
        };

        if let Some(snap) = self.baked {
            // =========================================================================
            // 管线 A：一等 `cosmic-text` 排版引擎渲染路径（BakedRun + BakedSpan）
            // =========================================================================
            let font_size = snap.font_size;
            let row_lh = snap.line_height.max(12);
            let tab_w = snap.tab_width.max(1) as usize;
            let space_adv = measure_char_advance_f32(' ', font_size).max(4.0);
            let indent_step_px = space_adv * (tab_w as f32);
            let row_count = snap.runs.len();

            let mut lines_inner = div()
                .id(self.id ^ 0x4544_494E)
                .flex_col()
                .w_full()
                .focusable()
                .no_focus_border()
                .cursor(CursorShape::Text);
            if let Some(kcb) = self.on_key_input {
                lines_inner = lines_inner.on_key_input(kcb);
            }

            if self.on_pointer_action.is_some()
                || self.on_pointer_select.is_some()
                || self.on_focus.is_some()
            {
                let on_focus = self.on_focus;
                let on_act = self.on_pointer_action;
                let on_sel = self.on_pointer_select;
                let max_row = snap.line_count.saturating_sub(1);
                let char_w = space_adv.max(6.0) as i32;
                let pointer_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let ev = acx.drag_event();
                    let is_drag = ev.delta_x != 0 || ev.delta_y != 0;
                    if !is_drag {
                        if let Some(fcb) = on_focus {
                            fcb(acx);
                        }
                    }
                    let content_x = (ev.local_x as i32) - content_origin_x;
                    let content_y = ev.local_y as i32;
                    if let Some(acb) = on_act {
                        acb(
                            (
                                content_x,
                                content_y,
                                is_drag,
                                acx.now_ms(),
                                acx.modifiers().shift(),
                            ),
                            acx,
                        );
                    } else if let Some(scb) = on_sel {
                        let row = ((content_y.max(0) as usize) / (row_lh as usize)).min(max_row);
                        let col = (content_x.max(0) / char_w.max(1)) as usize;
                        scb((BufferPoint::new(row, col), is_drag), acx);
                    }
                });
                lines_inner = lines_inner.on_drag_raw(pointer_cb);
            }

            if let Some(sec_cb) = self.on_secondary_click {
                let max_row = snap.line_count.saturating_sub(1);
                let char_w = space_adv.max(6.0) as i32;
                let secondary_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let ev = acx.drag_event();
                    let (wx, wy) = acx.click_position();
                    let content_x = (ev.local_x as i32) - content_origin_x;
                    let content_y = ev.local_y as i32;
                    let row = ((content_y.max(0) as usize) / (row_lh as usize)).min(max_row);
                    let col = (content_x.max(0) / char_w.max(1)) as usize;
                    sec_cb((BufferPoint::new(row, col), wx, wy), acx);
                });
                lines_inner = lines_inner.on_secondary_click_raw(secondary_cb);
            }

            let content_h = ((row_count as i32) * (row_lh as i32)).min(i16::MAX as i32) as i16;
            let total_inner_h = content_h.max(body_h);
            if self.show_line_numbers {
                if gutter_bg != Color::TRANSPARENT {
                    lines_inner = lines_inner.child(
                        cx,
                        div()
                            .pos(0, 0)
                            .w(gutter_w - 1)
                            .h(total_inner_h)
                            .bg(gutter_bg),
                    );
                }
                if divider_color != Color::TRANSPARENT {
                    lines_inner = lines_inner.child(
                        cx,
                        div()
                            .pos(gutter_w - 1, 0)
                            .w(1)
                            .h(total_inner_h)
                            .bg(divider_color),
                    );
                }
            }

            for &run in snap.runs {
                let cur_line_doc_offset = run.doc_offset;
                let line_doc_end = cur_line_doc_offset + run.line_text.len();
                let is_active_line = self.focused && run.line_i == snap.cursor.line;
                let line_diag = if run.layout_i == 0 {
                    self.diagnostics.iter().find(|d| {
                        d.range.start < line_doc_end.max(cur_line_doc_offset + 1)
                            && d.range.end >= cur_line_doc_offset
                    })
                } else {
                    None
                };

                let row_seed = ((run.line_i as u32) << 8) | (run.layout_i as u32 & 0xFF);
                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x5C0D_8278, 0x2000 + row_seed))
                    .flex_row()
                    .w_full()
                    .h(row_lh)
                    .items_center();
                if is_active_line && active_line_bg != Color::TRANSPARENT {
                    row = row.bg(active_line_bg);
                }

                // 1. 左侧行号栏（含软折行续行指示；无诊断时不分配空占位 Div）
                if self.show_line_numbers {
                    let mut gutter = div()
                        .flex_row()
                        .w(gutter_w)
                        .h(row_lh)
                        .pl(6)
                        .pr(8)
                        .items_center()
                        .justify_end();

                    if run.layout_i == 0 {
                        if let Some(diag) = line_diag {
                            let diag_col = match diag.severity {
                                DiagnosticSeverity::Error => Color::rgb(239, 68, 68),
                                DiagnosticSeverity::Warning => Color::rgb(245, 158, 11),
                                DiagnosticSeverity::Info => active_line_num_color,
                                DiagnosticSeverity::Hint => muted_color,
                            };
                            gutter = gutter.child(
                                cx,
                                div()
                                    .pos(0, ((row_lh - 6) / 2).max(0))
                                    .w(6)
                                    .h(6)
                                    .rounded(3)
                                    .bg(diag_col),
                            );
                        }
                        let num_col = if is_active_line {
                            active_line_num_color
                        } else {
                            muted_color
                        };
                        gutter = gutter.child(
                            cx,
                            text(fmt!(cx, "{}", run.line_i + 1))
                                .size(11)
                                .color(num_col)
                                .nowrap(),
                        );
                    } else {
                        gutter = gutter.child(
                            cx,
                            text("·")
                                .size(11)
                                .color(muted_color.scale_alpha(120))
                                .nowrap(),
                        );
                    }

                    row = row.child(cx, gutter);
                }

                // 2. 代码内容区：分层绝对定位（参考线 -> 匹配 -> 选区 -> 文本流 -> 0 抖动光标）
                let mut content = div()
                    .flex_row()
                    .flex_grow(1)
                    .h(row_lh)
                    .px(CONTENT_PAD_X)
                    .items_center()
                    .overflow_hidden();

                // Layer 1：缩进参考线（精准对齐制表位，绝不挤占文字排版）
                if self.show_indent_guides && run.layout_i == 0 && divider_color != Color::TRANSPARENT {
                    let indent_cols = Self::leading_indent_columns(run.line_text, tab_w);
                    let levels = (indent_cols / tab_w).min(8);
                    for level in 1..=levels {
                        let gx = round_f32_to_i16((level as f32) * indent_step_px);
                        content = content.child(
                            cx,
                            div()
                                .pos(gx, 0)
                                .w(1)
                                .h(row_lh)
                                .bg(divider_color),
                        );
                    }
                }

                // Layer 2：查找匹配精准矩形
                if !self.search_matches.is_empty() && run.layout_i == 0 {
                    for m in self.search_matches {
                        if m.end <= cur_line_doc_offset || m.start >= line_doc_end {
                            continue;
                        }
                        let mut rs = m
                            .start
                            .saturating_sub(cur_line_doc_offset)
                            .min(run.line_text.len());
                        let mut re = m
                            .end
                            .saturating_sub(cur_line_doc_offset)
                            .min(run.line_text.len());
                        while rs > 0 && !run.line_text.is_char_boundary(rs) {
                            rs -= 1;
                        }
                        while re < run.line_text.len() && !run.line_text.is_char_boundary(re) {
                            re += 1;
                        }
                        if rs < re {
                            let x0 = round_f32_to_i16(measure_line_width_f32(
                                &run.line_text[..rs],
                                font_size,
                            ));
                            let x1 = round_f32_to_i16(measure_line_width_f32(
                                &run.line_text[..re],
                                font_size,
                            ));
                            content = content.child(
                                cx,
                                div()
                                    .pos(x0, 1)
                                    .w((x1 - x0).max(2))
                                    .h((row_lh - 2).max(10))
                                    .rounded(2)
                                    .bg(match_bg),
                            );
                        }
                    }
                }

                // Layer 3：字形级精准选区矩形（含跨行换行符选中提示）
                if let Some((x0, x1)) = run.sel_rect {
                    content = content.child(
                        cx,
                        div()
                            .pos(x0, 1)
                            .w((x1 - x0).max(3))
                            .h((row_lh - 2).max(10))
                            .rounded(2)
                            .bg(selection_bg),
                    );
                }

                // Layer 4：同色合并文本段渲染（零多余包装节点 + 完整字偶距）
                content = Self::append_baked_spans(
                    content, cx, run, row_lh, font_size, text_color,
                );

                // Layer 5：0 抖动绝对定位闪烁叠层光标
                if cursor_visible {
                    if let Some(cx_px) = run.cursor_x {
                        content = content.child(
                            cx,
                            div()
                                .id(self.id ^ 0x4544_4352)
                                .pos(cx_px, 2)
                                .w(1)
                                .h((row_lh - 4).max(10))
                                .bg(cursor_color),
                        );
                    }
                }

                row = row.child(cx, content);
                lines_inner = lines_inner.child(cx, row);
            }

            // 撑满可视区高度使整片编辑区（含底部空白区）均可点击聚焦与落点
            if content_h < body_h {
                lines_inner = lines_inner.h(body_h);
            }

            lines_viewport = lines_viewport.child(cx, lines_inner);
            root = root.child(cx, lines_viewport);

            if self.show_status_bar {
                let wrap_label = match snap.wrap {
                    Wrap::None => "Wrap: Off",
                    Wrap::Glyph => "Wrap: Glyph",
                    Wrap::Word => "Wrap: Word",
                    Wrap::WordOrGlyph => "Wrap: Auto",
                };
                let left_status = if snap.selected_bytes > 0 {
                    fmt!(
                        cx,
                        "Ln {}, Col {}  •  Sel {}B",
                        self.cursor_point.row + 1,
                        self.cursor_point.column + 1,
                        snap.selected_bytes
                    )
                } else {
                    fmt!(
                        cx,
                        "Ln {}, Col {}",
                        self.cursor_point.row + 1,
                        self.cursor_point.column + 1
                    )
                };
                let right_status = if !self.search_matches.is_empty() {
                    fmt!(
                        cx,
                        "{}  •  Spaces: {}  •  Lines: {}  •  Matches: {}",
                        wrap_label,
                        tab_w,
                        snap.line_count,
                        self.search_matches.len()
                    )
                } else {
                    fmt!(
                        cx,
                        "{}  •  Spaces: {}  •  Lines: {}",
                        wrap_label,
                        tab_w,
                        snap.line_count
                    )
                };
                let status = div()
                    .flex_row()
                    .w_full()
                    .h((status_h - 1).max(16))
                    .px(10)
                    .items_center()
                    .justify_between()
                    .bg(gutter_bg)
                    .child(
                        cx,
                        text(left_status)
                            .size(11)
                            .color(muted_color)
                            .nowrap(),
                    )
                    .child(
                        cx,
                        text(right_status)
                            .size(11)
                            .color(muted_color)
                            .nowrap(),
                    );
                if divider_color != Color::TRANSPARENT {
                    root = root.child(cx, div().w_full().h(1).bg(divider_color));
                }
                root = root.child(cx, status);
            }

            return root.into_element(cx);
        }

        // =========================================================================
        // 管线 B：经典 `&str` 兼容模式（同样升级为 0 抖动绝对定位光标 + 精准选区高亮）
        // =========================================================================
        let cursor_off = self.selection.cursor_offset().min(self.value.len());
        let sel_start = self.selection.start.min(self.value.len());
        let sel_end = self.selection.end.min(self.value.len());
        let has_sel = sel_start < sel_end;

        let total_lines = self.value.bytes().filter(|&b| b == b'\n').count() + 1;
        let vis_rows = ((body_h as usize) / (lh as usize)).max(1);
        let max_window = (vis_rows * 2 + 12).max(48);
        let (win_start, win_end) = if total_lines <= max_window {
            (0, total_lines)
        } else {
            let half = max_window / 2;
            let center = self.cursor_point.row.min(total_lines.saturating_sub(1));
            let s = center.saturating_sub(half);
            let e = (s + max_window).min(total_lines);
            (e.saturating_sub(max_window), e)
        };

        let space_adv = measure_char_advance_f32(' ', self.font_size).max(4.0);
        let char_w = ((self.font_size as i16 * 6) / 10).max(7);
        let max_row = total_lines.saturating_sub(1);
        let point_from_local = move |local_x: i16, local_y: i16| -> BufferPoint {
            let row = win_start
                .saturating_add((local_y.max(0) as usize) / (lh as usize))
                .min(max_row);
            let col = ((local_x as i32 - content_origin_x).max(0) as usize) / (char_w as usize);
            BufferPoint::new(row, col)
        };

        let mut lines_inner = div()
            .id(self.id ^ 0x4544_494E)
            .flex_col()
            .w_full()
            .focusable()
            .no_focus_border()
            .cursor(CursorShape::Text);
        if let Some(kcb) = self.on_key_input {
            lines_inner = lines_inner.on_key_input(kcb);
        }

        if self.on_pointer_select.is_some()
            || self.on_pointer_action.is_some()
            || self.on_focus.is_some()
        {
            let on_focus = self.on_focus;
            let on_sel = self.on_pointer_select;
            let on_act = self.on_pointer_action;
            let pointer_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                let ev = acx.drag_event();
                let is_drag = ev.delta_x != 0 || ev.delta_y != 0;
                if !is_drag {
                    if let Some(fcb) = on_focus {
                        fcb(acx);
                    }
                }
                if let Some(acb) = on_act {
                    acb(
                        (
                            (ev.local_x as i32) - content_origin_x,
                            ev.local_y as i32,
                            is_drag,
                            acx.now_ms(),
                            acx.modifiers().shift(),
                        ),
                        acx,
                    );
                }
                if let Some(scb) = on_sel {
                    let point = point_from_local(ev.local_x, ev.local_y);
                    scb((point, is_drag), acx);
                }
            });
            lines_inner = lines_inner.on_drag_raw(pointer_cb);
        }

        if let Some(sec_cb) = self.on_secondary_click {
            let secondary_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                let ev = acx.drag_event();
                let (wx, wy) = acx.click_position();
                let point = point_from_local(ev.local_x, ev.local_y);
                sec_cb((point, wx, wy), acx);
            });
            lines_inner = lines_inner.on_secondary_click_raw(secondary_cb);
        }

        if win_start > 0 {
            let top_spacer_h = ((win_start as i32) * (lh as i32)).min(i16::MAX as i32) as i16;
            lines_inner = lines_inner.child(cx, div().w_full().h(top_spacer_h));
        }

        let ts = (self.tab_size.tab_size.max(1)) as usize;
        let indent_step_px = space_adv * (ts as f32);

        let mut line_start = 0usize;
        for (line_idx, raw_line) in self.value.split('\n').enumerate() {
            if line_idx < win_start {
                line_start += raw_line.len() + 1;
                continue;
            }
            if line_idx >= win_end {
                break;
            }
            let line_str = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let line_end = line_start + line_str.len();
            let is_active_line = self.focused && line_idx == self.cursor_point.row;
            let line_has_sel =
                has_sel && sel_start < line_end.max(line_start + 1) && sel_end > line_start;
            let line_diag = self
                .diagnostics
                .iter()
                .find(|d| d.range.start < line_end.max(line_start + 1) && d.range.end >= line_start);

            let row_bg = if is_active_line {
                active_line_bg
            } else {
                Color::TRANSPARENT
            };

            let mut row = div()
                .id(crate::scoped_child_id(
                    self.id,
                    0x5C0D_8278,
                    0x2000 + line_idx as u32,
                ))
                .flex_row()
                .w_full()
                .h(lh)
                .items_center()
                .bg(row_bg);

            if self.show_line_numbers {
                let mut gutter = div()
                    .flex_row()
                    .w(gutter_w - 1)
                    .h(lh)
                    .pl(6)
                    .pr(8)
                    .items_center()
                    .justify_between()
                    .bg(gutter_bg);

                if let Some(diag) = line_diag {
                    let diag_col = match diag.severity {
                        DiagnosticSeverity::Error => Color::rgb(239, 68, 68),
                        DiagnosticSeverity::Warning => Color::rgb(245, 158, 11),
                        DiagnosticSeverity::Info => active_line_num_color,
                        DiagnosticSeverity::Hint => muted_color,
                    };
                    gutter = gutter.child(cx, div().w(6).h(6).rounded(3).bg(diag_col));
                } else {
                    gutter = gutter.child(cx, div().w(6).h(6));
                }

                let num_col = if is_active_line {
                    active_line_num_color
                } else {
                    muted_color
                };
                gutter = gutter.child(
                    cx,
                    text(fmt!(cx, "{}", line_idx + 1))
                        .size(11)
                        .color(num_col)
                        .nowrap(),
                );
                row = row.child(cx, gutter);
                if divider_color != Color::TRANSPARENT {
                    row = row.child(cx, div().w(1).h(lh).bg(divider_color));
                }
            }

            let mut content = div()
                .flex_row()
                .flex_grow(1)
                .h(lh)
                .px(CONTENT_PAD_X)
                .items_center()
                .overflow_hidden();

            if self.show_indent_guides && divider_color != Color::TRANSPARENT {
                let indent_cols = self.tab_size.indent_count(line_str);
                let guides = (indent_cols / ts).min(6);
                for level in 1..=guides {
                    let gx = round_f32_to_i16((level as f32) * indent_step_px);
                    content = content.child(
                        cx,
                        div()
                            .pos(gx, 0)
                            .w(1)
                            .h(lh)
                            .bg(divider_color),
                    );
                }
            }

            for m in self.search_matches {
                if m.end <= line_start || m.start >= line_end {
                    continue;
                }
                let mut rs = m.start.saturating_sub(line_start).min(line_str.len());
                let mut re = m.end.saturating_sub(line_start).min(line_str.len());
                while rs > 0 && !line_str.is_char_boundary(rs) {
                    rs -= 1;
                }
                while re < line_str.len() && !line_str.is_char_boundary(re) {
                    re += 1;
                }
                if rs < re {
                    let x0 =
                        round_f32_to_i16(measure_line_width_f32(&line_str[..rs], self.font_size));
                    let x1 =
                        round_f32_to_i16(measure_line_width_f32(&line_str[..re], self.font_size));
                    content = content.child(
                        cx,
                        div()
                            .pos(x0, 1)
                            .w((x1 - x0).max(2))
                            .h((lh - 2).max(10))
                            .rounded(2)
                            .bg(match_bg),
                    );
                }
            }

            if line_has_sel {
                let mut rs = sel_start.saturating_sub(line_start).min(line_str.len());
                let mut re = sel_end.saturating_sub(line_start).min(line_str.len());
                while rs > 0 && !line_str.is_char_boundary(rs) {
                    rs -= 1;
                }
                while re < line_str.len() && !line_str.is_char_boundary(re) {
                    re += 1;
                }
                let x0 = round_f32_to_i16(measure_line_width_f32(&line_str[..rs], self.font_size));
                let mut x1 =
                    round_f32_to_i16(measure_line_width_f32(&line_str[..re], self.font_size));
                if sel_end > line_end {
                    x1 = x1.saturating_add(7);
                }
                let w = (x1 - x0).max(3);
                content = content.child(
                    cx,
                    div()
                        .pos(x0, 1)
                        .w(w)
                        .h((lh - 2).max(10))
                        .rounded(2)
                        .bg(selection_bg),
                );
            }

            if line_start < line_end {
                content = Self::append_legacy_highlighted_line(
                    content,
                    cx,
                    self.value,
                    line_start,
                    line_end,
                    self.highlights,
                    self.font_size,
                    text_color,
                );
            }

            let cursor_in_line =
                cursor_visible && cursor_off >= line_start && cursor_off <= line_end;
            if cursor_in_line {
                let mut rel_cursor = (cursor_off - line_start).min(line_str.len());
                while rel_cursor > 0 && !line_str.is_char_boundary(rel_cursor) {
                    rel_cursor -= 1;
                }
                let raw_x = measure_line_width_f32(&line_str[..rel_cursor], self.font_size);
                let cx_px = if rel_cursor == 0 {
                    0
                } else if rel_cursor < line_str.len() {
                    round_f32_to_i16((raw_x - 0.85).max(0.0))
                } else {
                    round_f32_to_i16(raw_x)
                };
                content = content.child(
                    cx,
                    div()
                        .id(self.id ^ 0x4544_4352)
                        .pos(cx_px, 2)
                        .w(1)
                        .h((lh - 4).max(10))
                        .bg(cursor_color),
                );
            }

            row = row.child(cx, content);
            lines_inner = lines_inner.child(cx, row);
            line_start += raw_line.len() + 1;
        }

        if win_end < total_lines {
            let bot_spacer_h =
                (((total_lines - win_end) as i32) * (lh as i32)).min(i16::MAX as i32) as i16;
            lines_inner = lines_inner.child(cx, div().w_full().h(bot_spacer_h));
        }

        lines_viewport = lines_viewport.child(cx, lines_inner);
        root = root.child(cx, lines_viewport);

        if self.show_status_bar {
            let status = div()
                .flex_row()
                .w_full()
                .h((status_h - 1).max(16))
                .px(10)
                .items_center()
                .justify_between()
                .bg(gutter_bg)
                .child(
                    cx,
                    text(fmt!(
                        cx,
                        "Ln {}, Col {}",
                        self.cursor_point.row + 1,
                        self.cursor_point.column + 1
                    ))
                    .size(11)
                    .color(muted_color)
                    .nowrap(),
                )
                .child(
                    cx,
                    text(fmt!(
                        cx,
                        "Spaces: {}  Matches: {}",
                        self.tab_size.tab_size,
                        self.search_matches.len()
                    ))
                    .size(11)
                    .color(muted_color)
                    .nowrap(),
                );
            if divider_color != Color::TRANSPARENT {
                root = root.child(cx, div().w_full().h(1).bg(divider_color));
            }
            root = root.child(cx, status);
        }

        root.into_element(cx)
    }
}
