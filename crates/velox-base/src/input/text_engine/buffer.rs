//! 多行文档排版缓冲区（[`TextDocBuffer`]，别名 [`Buffer`]）。
//!
//! 参照 `pop-os/cosmic-text/src/buffer.rs` 设计：
//! - 以 `SmallVec<BufferLine, 64>` 按逻辑行组织文档（`std` 下无上限动态扩容，`#![no_std]` 下零堆分配）；
//! - 提供按需惰性行排版、视口滚动投影 [`LayoutRun`] 迭代、字形级亚像素点击命中测试 [`Self::hit`]、
//!   视觉软折行光标导航 [`Self::cursor_motion`] 与光标可见性自动滚动 [`Self::ensure_cursor_visible`]。

use velox_core::arena::{SmallString, SmallVec};
use velox_core::pod::FrameScope;

use super::attrs::{Attrs, AttrsList};
use super::buffer_line::BufferLine;
use super::cursor::{Affinity, Cursor, LayoutCursor, Motion, Scroll};
use super::line_ending::{split_lines_with_ending, LineEnding};
use super::shape::{LayoutLine, LayoutRun, Metrics, Wrap};
use crate::input::text_boundary::{clip_offset_left, next_word_boundary, prev_word_boundary};

/// 多行文本排版缓冲区（对应 `cosmic-text` `Buffer`）。
#[derive(Clone, Debug)]
pub struct TextDocBuffer {
    /// 逻辑行列表（至少包含 1 行；`std` 下无上限动态扩容，`#![no_std]` 下零堆上限 12 行）。
    pub lines: SmallVec<BufferLine, 12>,
    metrics: Metrics,
    width_opt: Option<f32>,
    height_opt: Option<f32>,
    scroll: Scroll,
    wrap: Wrap,
    tab_width: u16,
    redraw: bool,
}

/// 与 `cosmic-text` 对齐的缓冲区类型别名。
pub type Buffer = TextDocBuffer;

impl Default for TextDocBuffer {
    fn default() -> Self {
        Self::new(Metrics::default())
    }
}

impl TextDocBuffer {
    /// 创建带指定字体度量的多行文本缓冲区（默认包含 1 个空行）。
    pub fn new(metrics: Metrics) -> Self {
        let mut lines = SmallVec::new();
        let _ = lines.push(BufferLine::new("", LineEnding::None, AttrsList::default()));
        Self {
            lines,
            metrics,
            width_opt: None,
            height_opt: None,
            scroll: Scroll::default(),
            wrap: Wrap::None,
            tab_width: 4,
            redraw: true,
        }
    }

    /// 创建包含指定初始文本与默认样式的多行缓冲区。
    pub fn from_str(metrics: Metrics, text: &str, attrs: Attrs) -> Self {
        let mut buf = Self::new(metrics);
        buf.set_text(text, attrs);
        buf
    }

    #[inline(always)]
    pub const fn metrics(&self) -> Metrics {
        self.metrics
    }

    pub fn set_metrics(&mut self, metrics: Metrics) {
        if self.metrics != metrics {
            self.metrics = metrics;
            self.reset_all_layouts();
            self.redraw = true;
        }
    }

    #[inline(always)]
    pub const fn size(&self) -> (Option<f32>, Option<f32>) {
        (self.width_opt, self.height_opt)
    }

    pub fn set_size(&mut self, width_opt: Option<f32>, height_opt: Option<f32>) {
        if self.width_opt != width_opt || self.height_opt != height_opt {
            if self.width_opt != width_opt {
                self.reset_all_layouts();
            }
            self.width_opt = width_opt;
            self.height_opt = height_opt;
            self.redraw = true;
        }
    }

    #[inline(always)]
    pub const fn wrap(&self) -> Wrap {
        self.wrap
    }

    pub fn set_wrap(&mut self, wrap: Wrap) {
        if self.wrap != wrap {
            self.wrap = wrap;
            self.reset_all_layouts();
            self.redraw = true;
        }
    }

    #[inline(always)]
    pub const fn tab_width(&self) -> u16 {
        self.tab_width
    }

    pub fn set_tab_width(&mut self, tab_width: u16) {
        let tw = tab_width.max(1);
        if self.tab_width != tw {
            self.tab_width = tw;
            self.reset_all_layouts();
            self.redraw = true;
        }
    }

    #[inline(always)]
    pub const fn scroll(&self) -> Scroll {
        self.scroll
    }

    pub fn set_scroll(&mut self, mut scroll: Scroll) {
        if self.lines.is_empty() {
            scroll.line = 0;
            scroll.vertical = 0.0;
        } else {
            let max_line = self.lines.len().saturating_sub(1);
            if scroll.line > max_line {
                scroll.line = max_line;
                scroll.vertical = 0.0;
            }
            if scroll.vertical < 0.0 {
                scroll.vertical = 0.0;
            }
            if scroll.horizontal < 0.0 {
                scroll.horizontal = 0.0;
            }
        }
        if self.scroll != scroll {
            self.scroll = scroll;
            self.redraw = true;
        }
    }

    #[inline(always)]
    pub const fn redraw(&self) -> bool {
        self.redraw
    }

    #[inline(always)]
    pub fn set_redraw(&mut self, redraw: bool) {
        self.redraw = redraw;
    }

    /// 清除所有行的视觉排版缓存。
    pub fn reset_all_layouts(&mut self) {
        for line in self.lines.as_mut_slice() {
            line.reset_layout();
        }
    }

    /// 整体替换文档文本，自动保留原始行尾结束符（`LF` / `CRLF` / `CR`）。
    pub fn set_text(&mut self, text: &str, attrs: Attrs) {
        self.lines.clear();
        for (line_str, ending) in split_lines_with_ending(text) {
            let _ = self
                .lines
                .push(BufferLine::new(line_str, ending, AttrsList::new(attrs)));
        }
        if self.lines.is_empty() {
            let _ = self
                .lines
                .push(BufferLine::new("", LineEnding::None, AttrsList::new(attrs)));
        }
        self.scroll = Scroll::default();
        self.redraw = true;
    }

    /// 返回当前逻辑行总数（至少为 1）。
    #[inline]
    pub fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    /// 统计文档总字节数（包含各行行尾结束符）。
    pub fn total_bytes(&self) -> usize {
        let mut total = 0usize;
        for line in self.lines.as_slice() {
            total = total
                .saturating_add(line.text().len())
                .saturating_add(line.ending().byte_len());
        }
        total
    }

    /// 将多行缓冲区序列化为扁平 [`SmallString`]（用于剪贴板导出、搜索或测试断言）。
    pub fn to_flat_string<const CAP: usize>(&self) -> SmallString<CAP> {
        let mut out = SmallString::new();
        for line in self.lines.as_slice() {
            out.push_str_trunc(line.text());
            out.push_str_trunc(line.ending().as_str());
        }
        out
    }

    /// 将当前文档零堆拼装到帧瞬态 [`FrameScope`] Arena 中，返回 `&'a str`。
    pub fn to_arena_str<'a>(&self, cx: &'a FrameScope) -> &'a str {
        let total = self.total_bytes();
        let buf = cx.arena().alloc_bytes(total);
        let mut off = 0usize;
        for line in self.lines.as_slice() {
            let tb = line.text().as_bytes();
            let take_t = tb.len().min(total.saturating_sub(off));
            buf[off..off + take_t].copy_from_slice(&tb[..take_t]);
            off += take_t;
            let eb = line.ending().as_str().as_bytes();
            let take_e = eb.len().min(total.saturating_sub(off));
            buf[off..off + take_e].copy_from_slice(&eb[..take_e]);
            off += take_e;
        }
        unsafe { core::str::from_utf8_unchecked(&buf[..off]) }
    }

    /// 将逻辑光标 [`Cursor`] 转换为全文线性字节偏移量。
    pub fn cursor_to_byte_offset(&self, cursor: Cursor) -> usize {
        let mut offset = 0usize;
        for (idx, line) in self.lines.as_slice().iter().enumerate() {
            if idx == cursor.line {
                return offset.saturating_add(clip_offset_left(line.text(), cursor.index));
            }
            offset = offset
                .saturating_add(line.text().len())
                .saturating_add(line.ending().byte_len());
        }
        offset
    }

    /// 将全文线性字节偏移量转换为逻辑光标 [`Cursor`]。
    pub fn byte_offset_to_cursor(&self, byte_offset: usize) -> Cursor {
        let mut remaining = byte_offset;
        let count = self.lines.len();
        for (idx, line) in self.lines.as_slice().iter().enumerate() {
            let line_len = line.text().len();
            let end_len = line.ending().byte_len();
            if remaining <= line_len || idx + 1 >= count {
                let clamped = clip_offset_left(line.text(), remaining.min(line_len));
                return Cursor::new(idx, clamped);
            }
            remaining = remaining.saturating_sub(line_len + end_len);
        }
        Cursor::default()
    }

    /// 惰性排版指定逻辑行并返回其视觉折行切片。
    pub fn line_layout(&mut self, line_i: usize) -> Option<&[LayoutLine]> {
        let fs = self.metrics.font_size_u8();
        let w = self.width_opt;
        let wrap = self.wrap;
        let tw = self.tab_width;
        self.lines
            .as_mut_slice()
            .get_mut(line_i)
            .map(|line| line.layout(fs, w, wrap, tw))
    }

    /// 按需排版从 `scroll.line` 到视口底部的全部可见逻辑行（对应 `cosmic-text` `shape_until_scroll`）。
    pub fn shape_until_scroll(&mut self, _prune: bool) {
        let fs = self.metrics.font_size_u8();
        let w = self.width_opt;
        let wrap = self.wrap;
        let tw = self.tab_width;
        let lh = self.metrics.line_height.max(1.0);
        let max_h = self.height_opt.unwrap_or(f32::MAX);

        let start = self.scroll.line.min(self.lines.len().saturating_sub(1));
        let mut y = -self.scroll.vertical;
        for line in self.lines.as_mut_slice()[start..].iter_mut() {
            let visual_lines = line.layout(fs, w, wrap, tw);
            y += (visual_lines.len().max(1) as f32) * lh;
            if y > max_h + lh * 2.0 {
                break;
            }
        }
    }

    /// 确保文档内所有逻辑行都已完成视觉排版。
    pub fn layout_all(&mut self) {
        let fs = self.metrics.font_size_u8();
        let w = self.width_opt;
        let wrap = self.wrap;
        let tw = self.tab_width;
        for line in self.lines.as_mut_slice() {
            let _ = line.layout(fs, w, wrap, tw);
        }
    }

    /// 遍历当前视口可见的视觉排版行投影（对应 `cosmic-text` `Buffer::layout_runs`）。
    ///
    /// 调用前请确保已调用 [`Self::shape_until_scroll`] 或 [`Self::layout_all`]。
    pub fn layout_runs(&self) -> LayoutRunIter<'_> {
        let start = self.scroll.line.min(self.lines.len());
        let lh = self.metrics.line_height.max(1.0);
        let ascent = self.metrics.font_size.max(8.0) * 0.8;
        LayoutRunIter {
            buffer: self,
            line_i: start,
            layout_i: 0,
            line_top: -self.scroll.vertical,
            line_height: lh,
            ascent,
            max_h: self.height_opt.unwrap_or(f32::MAX),
        }
    }

    /// 遍历整篇文档的所有视觉排版行（不执行视口裁剪，适用于外层由滚动容器托管滚动的场景）。
    pub fn layout_runs_all(&self) -> LayoutRunIter<'_> {
        let lh = self.metrics.line_height.max(1.0);
        let ascent = self.metrics.font_size.max(8.0) * 0.8;
        LayoutRunIter {
            buffer: self,
            line_i: 0,
            layout_i: 0,
            line_top: 0.0,
            line_height: lh,
            ascent,
            max_h: f32::MAX,
        }
    }

    /// 查询指定逻辑行当前缓存的视觉折行数量（至少为 1）。
    #[inline]
    pub fn line_visual_count(&self, line_i: usize) -> usize {
        self.lines
            .as_slice()
            .get(line_i)
            .and_then(|l| l.layout_opt())
            .map_or(1, |v| v.len().max(1))
    }

    /// 统计文档总视觉行数（含软折行）。
    pub fn total_visual_lines(&mut self) -> usize {
        self.layout_all();
        let mut total = 0usize;
        for line in self.lines.as_slice() {
            total += line.layout_opt().map_or(1, |v| v.len().max(1));
        }
        total.max(1)
    }

    fn resolve_layout_cursor_in_layouts(
        line_i: usize,
        layouts: &[LayoutLine],
        cursor: Cursor,
    ) -> LayoutCursor {
        if layouts.is_empty() {
            return LayoutCursor::new(line_i, 0, 0);
        }
        for (layout_i, layout_line) in layouts.iter().enumerate() {
            let glyphs = layout_line.glyphs.as_slice();
            if glyphs.is_empty() {
                return LayoutCursor::new(line_i, layout_i, 0);
            }
            let first_s = glyphs[0].start;
            let last_e = glyphs[glyphs.len() - 1].end;
            let is_last_layout = layout_i + 1 == layouts.len();

            if cursor.index < first_s {
                return LayoutCursor::new(line_i, layout_i, 0);
            }
            if cursor.index < last_e
                || (cursor.index == last_e && (cursor.affinity.before() || is_last_layout))
            {
                for (gi, g) in glyphs.iter().enumerate() {
                    if cursor.index <= g.start || cursor.index < g.end {
                        return LayoutCursor::new(line_i, layout_i, gi);
                    }
                }
                return LayoutCursor::new(line_i, layout_i, glyphs.len());
            }
        }
        let last_li = layouts.len().saturating_sub(1);
        let last_gl = layouts[last_li].glyphs.len();
        LayoutCursor::new(line_i, last_li, last_gl)
    }

    /// 只读版本：利用已有排版缓存将逻辑光标 [`Cursor`] 映射到三维视觉坐标 [`LayoutCursor`]。
    pub fn layout_cursor_cached(&self, cursor: Cursor) -> Option<LayoutCursor> {
        if self.lines.is_empty() {
            return None;
        }
        let line_i = cursor.line.min(self.lines.len().saturating_sub(1));
        let layouts = self.lines[line_i].layout_opt()?;
        Some(Self::resolve_layout_cursor_in_layouts(line_i, layouts, cursor))
    }

    /// 将逻辑光标 [`Cursor`] 映射到三维视觉坐标 [`LayoutCursor`]（精确处理软折行边界的 [`Affinity`]）。
    pub fn layout_cursor(&mut self, cursor: Cursor) -> Option<LayoutCursor> {
        let line_i = cursor.line.min(self.lines.len().saturating_sub(1));
        let layouts = self.line_layout(line_i)?;
        Some(Self::resolve_layout_cursor_in_layouts(line_i, layouts, cursor))
    }

    /// 查询光标相对于内容视口左上角的精确 `(x, y)` 像素坐标（已扣除水平/垂直滚动量）。
    pub fn cursor_position(&mut self, cursor: Cursor) -> Option<(f32, f32)> {
        self.shape_until_scroll(false);
        let lc = self.layout_cursor(cursor)?;
        for run in self.layout_runs() {
            if run.line_i == lc.line && run.layout_i == lc.layout {
                let x = if lc.glyph < run.glyphs.len() {
                    run.glyphs[lc.glyph].x
                } else {
                    run.line_w
                };
                return Some((x - self.scroll.horizontal, run.line_top));
            }
        }
        None
    }

    /// 基于真实 [`LayoutRun`] 与 [`LayoutGlyph`] 执行亚像素级精确鼠标点击命中测试（对应 `cosmic-text` `Buffer::hit`）。
    ///
    /// 参数 `(x, y)` 为相对于文本内容区左上角的局部像素坐标（不含行号栏宽度）。
    pub fn hit(&mut self, x: f32, y: f32) -> Option<Cursor> {
        self.shape_until_scroll(false);
        let target_x = (x + self.scroll.horizontal).max(0.0);
        let mut last_run: Option<(usize, usize)> = None;

        for run in self.layout_runs() {
            let line_i = run.line_i;
            let layout_i = run.layout_i;
            last_run = Some((line_i, layout_i));

            // 若点击坐标落于首条可见行上方，贴齐首条可见行
            if y < run.line_top && line_i == self.scroll.line && layout_i == 0 {
                let (idx, aff) = self.lines[line_i]
                    .layout_opt()
                    .and_then(|v| v.get(layout_i))
                    .map(|ll| ll.hit_x(target_x))
                    .unwrap_or((0, Affinity::After));
                return Some(Cursor::new_with_affinity(line_i, idx, aff));
            }

            if y >= run.line_top && y < run.line_top + run.line_height {
                let (idx, aff) = self.lines[line_i]
                    .layout_opt()
                    .and_then(|v| v.get(layout_i))
                    .map(|ll| ll.hit_x(target_x))
                    .unwrap_or((0, Affinity::After));
                return Some(Cursor::new_with_affinity(line_i, idx, aff));
            }
        }

        // 点击落在所有可见行下方：定位到最末可见行的对应水平位置
        if let Some((line_i, layout_i)) = last_run {
            let (idx, aff) = self.lines[line_i]
                .layout_opt()
                .and_then(|v| v.get(layout_i))
                .map(|ll| ll.hit_x(target_x))
                .unwrap_or((0, Affinity::After));
            return Some(Cursor::new_with_affinity(line_i, idx, aff));
        }

        None
    }

    /// 执行全部 20 种 [`Motion`] 光标导航（对应 `cosmic-text` `Buffer::cursor_motion`），
    /// 返回 `(new_cursor, new_cursor_x_opt)`；垂直移动时自动记忆并复原视觉 X 像素列坐标。
    pub fn cursor_motion(
        &mut self,
        mut cursor: Cursor,
        mut cursor_x_opt: Option<i32>,
        motion: Motion,
    ) -> Option<(Cursor, Option<i32>)> {
        if self.lines.is_empty() {
            return None;
        }
        cursor.line = cursor.line.min(self.lines.len().saturating_sub(1));
        cursor.index = clip_offset_left(self.lines[cursor.line].text(), cursor.index);

        match motion {
            Motion::Left | Motion::Previous => {
                cursor_x_opt = None;
                if cursor.index > 0 {
                    let prev = self.lines[cursor.line].text()[..cursor.index]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    cursor.index = prev;
                    cursor.affinity = Affinity::After;
                } else if cursor.line > 0 {
                    cursor.line -= 1;
                    cursor.index = self.lines[cursor.line].text().len();
                    cursor.affinity = Affinity::Before;
                } else {
                    return None;
                }
            }
            Motion::Right | Motion::Next => {
                cursor_x_opt = None;
                let line_len = self.lines[cursor.line].text().len();
                if cursor.index < line_len {
                    let next = self.lines[cursor.line].text()[cursor.index..]
                        .chars()
                        .next()
                        .map(|c| cursor.index + c.len_utf8())
                        .unwrap_or(line_len);
                    cursor.index = next;
                    cursor.affinity = Affinity::After;
                } else if cursor.line + 1 < self.lines.len() {
                    cursor.line += 1;
                    cursor.index = 0;
                    cursor.affinity = Affinity::After;
                } else {
                    return None;
                }
            }
            Motion::Up => {
                let (next_c, next_x) = self.move_visual_lines(cursor, cursor_x_opt, -1)?;
                cursor = next_c;
                cursor_x_opt = next_x;
            }
            Motion::Down => {
                let (next_c, next_x) = self.move_visual_lines(cursor, cursor_x_opt, 1)?;
                cursor = next_c;
                cursor_x_opt = next_x;
            }
            Motion::PageUp => {
                let lh = self.metrics.line_height.max(1.0);
                let step = self
                    .height_opt
                    .map(|h| ((h / lh) as i32).max(1))
                    .unwrap_or(10);
                let (next_c, next_x) = self.move_visual_lines(cursor, cursor_x_opt, -step)?;
                cursor = next_c;
                cursor_x_opt = next_x;
            }
            Motion::PageDown => {
                let lh = self.metrics.line_height.max(1.0);
                let step = self
                    .height_opt
                    .map(|h| ((h / lh) as i32).max(1))
                    .unwrap_or(10);
                let (next_c, next_x) = self.move_visual_lines(cursor, cursor_x_opt, step)?;
                cursor = next_c;
                cursor_x_opt = next_x;
            }
            Motion::Vertical(px) => {
                let lh = self.metrics.line_height.max(1.0);
                let raw_steps = (px as f32) / lh;
                let step = if raw_steps >= 0.0 {
                    (raw_steps + 0.5) as i32
                } else {
                    (raw_steps - 0.5) as i32
                };
                if step == 0 {
                    return Some((cursor, cursor_x_opt));
                }
                let (next_c, next_x) = self.move_visual_lines(cursor, cursor_x_opt, step)?;
                cursor = next_c;
                cursor_x_opt = next_x;
            }
            Motion::Home => {
                cursor_x_opt = None;
                let lc = self.layout_cursor(cursor)?;
                let start = self
                    .line_layout(lc.line)
                    .and_then(|v| v.get(lc.layout))
                    .map_or(0, |ll| ll.start_index());
                cursor.index = start;
                cursor.affinity = Affinity::After;
            }
            Motion::SoftHome => {
                cursor_x_opt = None;
                let line_text = self.lines[cursor.line].text();
                let first_non_ws = line_text
                    .char_indices()
                    .find(|&(_, ch)| ch != ' ' && ch != '\t')
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                if cursor.index == first_non_ws {
                    cursor.index = 0;
                } else {
                    cursor.index = first_non_ws;
                }
                cursor.affinity = Affinity::After;
            }
            Motion::End => {
                cursor_x_opt = None;
                let lc = self.layout_cursor(cursor)?;
                let fallback_end = self.lines[cursor.line].text().len();
                let end = self
                    .line_layout(lc.line)
                    .and_then(|v| v.get(lc.layout))
                    .map_or(fallback_end, |ll| ll.end_index());
                cursor.index = end;
                cursor.affinity = Affinity::Before;
            }
            Motion::ParagraphStart => {
                cursor_x_opt = None;
                cursor.index = 0;
                cursor.affinity = Affinity::After;
            }
            Motion::ParagraphEnd => {
                cursor_x_opt = None;
                cursor.index = self.lines[cursor.line].text().len();
                cursor.affinity = Affinity::Before;
            }
            Motion::PreviousWord | Motion::LeftWord => {
                cursor_x_opt = None;
                if cursor.index > 0 {
                    cursor.index = prev_word_boundary(self.lines[cursor.line].text(), cursor.index);
                    cursor.affinity = Affinity::After;
                } else if cursor.line > 0 {
                    cursor.line -= 1;
                    cursor.index = self.lines[cursor.line].text().len();
                    cursor.affinity = Affinity::Before;
                } else {
                    return None;
                }
            }
            Motion::NextWord | Motion::RightWord => {
                cursor_x_opt = None;
                let line_len = self.lines[cursor.line].text().len();
                if cursor.index < line_len {
                    cursor.index = next_word_boundary(self.lines[cursor.line].text(), cursor.index);
                    cursor.affinity = Affinity::After;
                } else if cursor.line + 1 < self.lines.len() {
                    cursor.line += 1;
                    cursor.index = 0;
                    cursor.affinity = Affinity::After;
                } else {
                    return None;
                }
            }
            Motion::BufferStart => {
                cursor_x_opt = None;
                cursor = Cursor::new(0, 0);
            }
            Motion::BufferEnd => {
                cursor_x_opt = None;
                let last_line = self.lines.len().saturating_sub(1);
                let last_len = self.lines[last_line].text().len();
                cursor = Cursor::new_with_affinity(last_line, last_len, Affinity::Before);
            }
            Motion::GotoLine(target_line) => {
                cursor_x_opt = None;
                let clamped = target_line.min(self.lines.len().saturating_sub(1));
                cursor = Cursor::new(clamped, 0);
            }
        }

        Some((cursor, cursor_x_opt))
    }

    /// 在视觉行空间内上下移动 `delta` 条视觉行（无缝跨软折行与逻辑行边界，保持精确 `cursor_x_opt`）。
    fn move_visual_lines(
        &mut self,
        cursor: Cursor,
        cursor_x_opt: Option<i32>,
        delta: i32,
    ) -> Option<(Cursor, Option<i32>)> {
        let mut lc = self.layout_cursor(cursor)?;
        let cur_x = match cursor_x_opt {
            Some(x) => x,
            None => {
                let layouts = self.line_layout(lc.line)?;
                let x_f = layouts
                    .get(lc.layout)
                    .map_or(0.0, |ll| ll.x_for_index(cursor.index));
                (x_f + 0.5) as i32
            }
        };

        let mut rem = delta;
        let line_count = self.lines.len();
        if rem < 0 {
            while rem < 0 {
                if lc.layout > 0 {
                    lc.layout -= 1;
                } else if lc.line > 0 {
                    lc.line -= 1;
                    let prev_len = self.line_layout(lc.line).map_or(1, |v| v.len().max(1));
                    lc.layout = prev_len - 1;
                } else {
                    // 到达文档最首视觉行：若尚未在行首则落至 (0, 0)
                    if cursor.index == 0 {
                        return None;
                    }
                    return Some((Cursor::new(0, 0), Some(0)));
                }
                rem += 1;
            }
        } else if rem > 0 {
            while rem > 0 {
                let cur_len = self.line_layout(lc.line).map_or(1, |v| v.len().max(1));
                if lc.layout + 1 < cur_len {
                    lc.layout += 1;
                } else if lc.line + 1 < line_count {
                    lc.line += 1;
                    lc.layout = 0;
                } else {
                    // 到达文档最末视觉行：若尚未在行尾则落至文末
                    let end_idx = self.lines[lc.line].text().len();
                    if cursor.index == end_idx {
                        return None;
                    }
                    return Some((
                        Cursor::new_with_affinity(lc.line, end_idx, Affinity::Before),
                        Some(cur_x),
                    ));
                }
                rem -= 1;
            }
        }

        let layouts = self.line_layout(lc.line)?;
        let target_ll = layouts.get(lc.layout).or_else(|| layouts.last())?;
        let (idx, aff) = target_ll.hit_x(cur_x as f32);
        Some((Cursor::new_with_affinity(lc.line, idx, aff), Some(cur_x)))
    }

    /// 滚动视口以确保 `cursor` 位于可见区域内（垂直 + 水平双向自动跟随）。
    pub fn ensure_cursor_visible(&mut self, cursor: Cursor) {
        let Some(vh) = self.height_opt else {
            return;
        };
        let lh = self.metrics.line_height.max(1.0);
        let target_line = cursor.line.min(self.lines.len().saturating_sub(1));

        // 1. 若光标行在当前滚动行上方，直接上滚对齐该行
        if target_line < self.scroll.line {
            self.scroll.line = target_line;
            self.scroll.vertical = 0.0;
            self.redraw = true;
        } else {
            // 计算光标视觉行相对于当前 scroll.line 顶部的 Y 坐标
            let Some(lc) = self.layout_cursor(cursor) else {
                return;
            };
            let mut y = -self.scroll.vertical;
            for li in self.scroll.line..=lc.line {
                let vis_cnt = self.line_layout(li).map_or(1, |v| v.len().max(1));
                if li == lc.line {
                    y += (lc.layout as f32) * lh;
                    break;
                }
                y += (vis_cnt as f32) * lh;
            }
            if y < 0.0 {
                self.scroll.line = lc.line;
                self.scroll.vertical = (lc.layout as f32) * lh;
                self.redraw = true;
            } else if y + lh > vh {
                // 向下推进 scroll.line 直到光标行底边落在 vh 内
                let mut excess = (y + lh) - vh;
                let mut cur_l = self.scroll.line;
                let mut cur_v = self.scroll.vertical;
                while excess > 0.0 && cur_l < lc.line {
                    let line_total_h =
                        (self.line_layout(cur_l).map_or(1, |v| v.len().max(1)) as f32) * lh;
                    let remain_in_line = (line_total_h - cur_v).max(0.0);
                    if excess >= remain_in_line {
                        excess -= remain_in_line;
                        cur_l += 1;
                        cur_v = 0.0;
                    } else {
                        cur_v += excess;
                        excess = 0.0;
                    }
                }
                if excess > 0.0 {
                    cur_v += excess;
                }
                self.scroll.line = cur_l;
                self.scroll.vertical = cur_v;
                self.redraw = true;
            }
        }

        // 2. 非折行模式下的水平自动跟随
        if matches!(self.wrap, Wrap::None) {
            if let Some(vw) = self.width_opt {
                if let Some(lc) = self.layout_cursor(cursor) {
                    if let Some(x) = self
                        .line_layout(lc.line)
                        .and_then(|v| v.get(lc.layout))
                        .map(|ll| ll.x_for_index(cursor.index))
                    {
                        let margin = 24.0f32.min(vw * 0.25);
                        if x < self.scroll.horizontal {
                            self.scroll.horizontal = (x - margin).max(0.0);
                            self.redraw = true;
                        } else if x > self.scroll.horizontal + vw - margin {
                            self.scroll.horizontal = (x - vw + margin).max(0.0);
                            self.redraw = true;
                        }
                    }
                }
            }
        } else if self.scroll.horizontal != 0.0 {
            self.scroll.horizontal = 0.0;
        }
    }

    /// 按垂直像素增量滚动视口（正值向下滚看后文，负值向上滚看前文）。
    pub fn scroll_vertical_pixels(&mut self, delta_px: f32) {
        if self.lines.is_empty() || (delta_px > -0.01 && delta_px < 0.01) {
            return;
        }
        let lh = self.metrics.line_height.max(1.0);
        if delta_px > 0.0 {
            let mut rem = delta_px;
            let mut cur_l = self.scroll.line;
            let mut cur_v = self.scroll.vertical;
            let last_l = self.lines.len().saturating_sub(1);
            while rem > 0.0 && cur_l < last_l {
                let line_h =
                    (self.line_layout(cur_l).map_or(1, |v| v.len().max(1)) as f32) * lh;
                let avail = (line_h - cur_v).max(0.0);
                if rem >= avail {
                    rem -= avail;
                    cur_l += 1;
                    cur_v = 0.0;
                } else {
                    cur_v += rem;
                    rem = 0.0;
                }
            }
            self.set_scroll(Scroll {
                line: cur_l,
                vertical: cur_v,
                horizontal: self.scroll.horizontal,
            });
        } else {
            let mut rem = -delta_px;
            let mut cur_l = self.scroll.line;
            let mut cur_v = self.scroll.vertical;
            while rem > 0.0 {
                if cur_v >= rem {
                    cur_v -= rem;
                    rem = 0.0;
                } else if cur_l > 0 {
                    rem -= cur_v;
                    cur_l -= 1;
                    let line_h =
                        (self.line_layout(cur_l).map_or(1, |v| v.len().max(1)) as f32) * lh;
                    cur_v = line_h;
                } else {
                    cur_v = 0.0;
                    rem = 0.0;
                }
            }
            self.set_scroll(Scroll {
                line: cur_l,
                vertical: cur_v,
                horizontal: self.scroll.horizontal,
            });
        }
    }
}

/// 视口可见视觉行投影迭代器（对应 `cosmic-text` `LayoutRunIter`）。
pub struct LayoutRunIter<'a> {
    buffer: &'a TextDocBuffer,
    line_i: usize,
    layout_i: usize,
    line_top: f32,
    line_height: f32,
    ascent: f32,
    max_h: f32,
}

impl<'a> Iterator for LayoutRunIter<'a> {
    type Item = LayoutRun<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let lines = self.buffer.lines.as_slice();
        while self.line_i < lines.len() {
            if self.line_top > self.max_h {
                return None;
            }
            let line = &lines[self.line_i];
            if let Some(layouts) = line.layout_opt() {
                if self.layout_i < layouts.len() {
                    let ll = &layouts[self.layout_i];
                    let top = self.line_top;
                    let cur_li = self.line_i;
                    let cur_lo = self.layout_i;
                    self.layout_i += 1;
                    self.line_top += self.line_height;
                    if top + self.line_height <= 0.0 {
                        continue;
                    }
                    return Some(LayoutRun {
                        line_i: cur_li,
                        layout_i: cur_lo,
                        text: line.text(),
                        glyphs: ll.glyphs.as_slice(),
                        line_y: top + self.ascent,
                        line_top: top,
                        line_height: self.line_height,
                        line_w: ll.w,
                    });
                }
            } else if self.layout_i == 0 {
                // 尚未排版时的兜底空视觉行投影
                let top = self.line_top;
                let cur_li = self.line_i;
                self.line_i += 1;
                self.layout_i = 0;
                self.line_top += self.line_height;
                if top + self.line_height <= 0.0 {
                    continue;
                }
                return Some(LayoutRun {
                    line_i: cur_li,
                    layout_i: 0,
                    text: line.text(),
                    glyphs: &[],
                    line_y: top + self.ascent,
                    line_top: top,
                    line_height: self.line_height,
                    line_w: 0.0,
                });
            }
            self.line_i += 1;
            self.layout_i = 0;
        }
        None
    }
}
