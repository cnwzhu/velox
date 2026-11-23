//! 多行文档与代码编辑器核心状态机（[`TextBuffer`]、[`TabSize`]、[`AutoClosedPairs`]、[`DiagnosticSpan`]、[`HighlightSpan`]）。
//!
//! 涵盖编辑状态、光标移动、括号自动闭合、缩进、代码诊断与语法高亮能力，完全兼容 `#![no_std]`。

use core::ops::Range;
use velox_engine::prelude::*;

use super::cursor::{BufferPoint, CursorSelection, Selection};
use super::search::SearchMatcher;
use super::text_boundary::{
    clip_offset_left, line_range_at, next_word_boundary, prev_word_boundary, word_range_at,
};
use super::undo_history::UndoHistory;

/// 缩进与制表符配置（`TabSize`）。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TabSize {
    /// 每个缩进层级的空格数（默认 `2`）。
    pub tab_size: u8,
    /// 是否插入硬 `\t` 字符而非空格（默认 `false`）。
    pub hard_tabs: bool,
}

impl Default for TabSize {
    fn default() -> Self {
        Self {
            tab_size: 2,
            hard_tabs: false,
        }
    }
}

impl TabSize {
    #[inline(always)]
    pub const fn new(tab_size: u8, hard_tabs: bool) -> Self {
        Self {
            tab_size,
            hard_tabs,
        }
    }

    /// 统计单行文本前导缩进的等效空格列数。
    pub fn indent_count(&self, line: &str) -> usize {
        let ts = (self.tab_size.max(1)) as usize;
        let mut count = 0usize;
        for ch in line.chars() {
            match ch {
                '\t' => count += ts - (count % ts),
                ' ' => count += 1,
                _ => break,
            }
        }
        count
    }
}

/// 自动配对闭合括号与引号跟踪器。
#[derive(Debug, Clone, Default)]
pub struct AutoClosedPairs<const CAP: usize = 16> {
    pairs: SmallVec<(Selection, Selection), CAP>,
}

impl<const CAP: usize> AutoClosedPairs<CAP> {
    pub const fn new() -> Self {
        Self {
            pairs: SmallVec::new(),
        }
    }

    /// 若输入字符 `ch` 为可自动闭合左定界符（`(`、`[`、`{`、`"`、`'`），返回对应的右定界符。
    #[inline]
    pub const fn closing_char_for(ch: char) -> Option<char> {
        match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '"' => Some('"'),
            '\'' => Some('\''),
            _ => None,
        }
    }

    pub fn record(&mut self, open: Range<usize>, close: Range<usize>) {
        if self.pairs.len() >= CAP {
            let slice = self.pairs.as_mut_slice();
            for i in 1..slice.len() {
                slice[i - 1] = slice[i];
            }
            let _ = self.pairs.pop();
        }
        let _ = self
            .pairs
            .push((Selection::from(open), Selection::from(close)));
    }

    pub fn contains_closer(&self, cursor: usize, len: usize) -> bool {
        self.pairs
            .as_slice()
            .iter()
            .any(|(_, close)| close.start == cursor && close.len() == len)
    }

    pub fn has_adjacent_pair_at(&self, cursor: usize) -> Option<(Selection, Selection)> {
        self.pairs
            .as_slice()
            .iter()
            .find(|(open, close)| open.end == cursor && close.start == cursor)
            .copied()
    }

    pub fn adjust(&mut self, edit: Range<usize>, new_len: usize) {
        let delta = new_len as isize - (edit.end.saturating_sub(edit.start)) as isize;
        let shift = |offset: usize| (offset as isize + delta).max(0) as usize;
        let mut kept: SmallVec<(Selection, Selection), CAP> = SmallVec::new();
        for &(mut open, mut close) in self.pairs.as_slice() {
            if edit.end <= open.start {
                open = Selection::new(shift(open.start), shift(open.end));
                close = Selection::new(shift(close.start), shift(close.end));
                let _ = kept.push((open, close));
            } else if edit.start >= open.end && edit.end <= close.start {
                close = Selection::new(shift(close.start), shift(close.end));
                let _ = kept.push((open, close));
            }
        }
        self.pairs = kept;
    }

    pub fn clear(&mut self) {
        self.pairs.clear();
    }
}

/// 代码诊断严重级别（`DiagnosticSeverity`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DiagnosticSeverity {
    Hint,
    Info,
    Warning,
    Error,
}

/// 编辑器代码诊断区间标记。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct DiagnosticSpan<'a> {
    pub range: Selection,
    pub severity: DiagnosticSeverity,
    pub message: &'a str,
}

/// 语法高亮词法分类。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HighlightKind {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
    Operator,
}

/// 编辑器语法高亮字节区间。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct HighlightSpan {
    pub range: Selection,
    pub kind: HighlightKind,
}

const UNDO_POOL_CAP: usize = 4096;

/// 用于 [`UndoHistory`] 的轻量级双向增量编辑记录（20 字节定长，无截断无 UTF-8 撕裂）。
#[derive(Copy, Clone, Debug)]
pub struct TextSnapshot<const SNAP_CAP: usize = 256> {
    del_seq: u32,
    ins_seq: u32,
    offset: u16,
    del_len: u16,
    ins_len: u16,
    cursor_before: u16,
    cursor_after: u16,
}

/// 支持多行编辑、行列坐标转换、`column_anchor` 垂直导航、按词移动、自动括号配对、块缩进与撤销/重做历史的 `#![no_std]` 文本编辑器核心状态机。
#[derive(Clone, Debug)]
pub struct TextBuffer<const CAP: usize = 1024> {
    bytes: [u8; CAP],
    len: usize,
    selection: CursorSelection,
    pub tab_size: TabSize,
    pub auto_close_enabled: bool,
    pub auto_indent_enabled: bool,
    pub auto_closed_pairs: AutoClosedPairs<16>,
    pub history: UndoHistory<TextSnapshot<256>, 16, 2>,
    undo_pool: [u8; UNDO_POOL_CAP],
    undo_pool_seq: u32,
    pub search: SearchMatcher<32>,
    pub scroll_row: usize,
}

impl<const CAP: usize> Default for TextBuffer<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> TextBuffer<CAP> {
    pub const fn new() -> Self {
        Self {
            bytes: [0u8; CAP],
            len: 0,
            selection: CursorSelection::new(0, 0, 0),
            tab_size: TabSize {
                tab_size: 2,
                hard_tabs: false,
            },
            auto_close_enabled: true,
            auto_indent_enabled: true,
            auto_closed_pairs: AutoClosedPairs::new(),
            history: UndoHistory::new(),
            undo_pool: [0u8; UNDO_POOL_CAP],
            undo_pool_seq: 0,
            search: SearchMatcher::new(),
            scroll_row: 0,
        }
    }

    pub fn from_str(initial: &str) -> Self {
        let mut buf = Self::new();
        buf.history.set_ignoring(true);
        buf.set_text(initial);
        buf.history.set_ignoring(false);
        buf.history.clear();
        buf.undo_pool_seq = 0;
        buf
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }

    #[inline]
    pub fn as_arena_str<'a>(&self, cx: &'a FrameScope) -> &'a str {
        let out = cx.arena().alloc_bytes(self.len);
        out.copy_from_slice(&self.bytes[..self.len]);
        unsafe { core::str::from_utf8_unchecked(out) }
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub const fn cursor(&self) -> usize {
        self.selection.cursor_offset()
    }

    #[inline(always)]
    pub const fn selection(&self) -> CursorSelection {
        self.selection
    }

    /// 获取当前选中的文本切片。
    #[inline]
    pub fn selected_text(&self) -> &str {
        if self.selection.is_empty() {
            ""
        } else {
            let s = self.as_str();
            let start = clip_offset_left(s, self.selection.start);
            let end = clip_offset_left(s, self.selection.end);
            &s[start..end]
        }
    }

    /// 统计总行数（至少为 1）。
    pub fn line_count(&self) -> usize {
        1 + self.bytes[..self.len]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
    }

    /// 获取第 `row` 行的字节范围 `Range<usize>`（不含 `\n`）。
    pub fn line_range(&self, row: usize) -> Option<Range<usize>> {
        let s = self.as_str();
        let mut cur_row = 0usize;
        let mut line_start = 0usize;
        for (idx, b) in s.bytes().enumerate() {
            if b == b'\n' {
                if cur_row == row {
                    let mut end = idx;
                    if end > line_start && s.as_bytes()[end - 1] == b'\r' {
                        end -= 1;
                    }
                    return Some(line_start..end);
                }
                cur_row += 1;
                line_start = idx + 1;
            }
        }
        if cur_row == row {
            Some(line_start..self.len)
        } else {
            None
        }
    }

    /// 获取第 `row` 行的字符串切片 `&str`。
    pub fn line_str(&self, row: usize) -> Option<&str> {
        let range = self.line_range(row)?;
        Some(&self.as_str()[range])
    }

    /// 将 UTF-8 字节偏移量转换为二维行列坐标 [`BufferPoint`]。
    pub fn offset_to_point(&self, offset: usize) -> BufferPoint {
        let off = clip_offset_left(self.as_str(), offset);
        let mut row = 0usize;
        let mut line_start = 0usize;
        for (idx, b) in self.as_str()[..off].bytes().enumerate() {
            if b == b'\n' {
                row += 1;
                line_start = idx + 1;
            }
        }
        BufferPoint::new(row, off.saturating_sub(line_start))
    }

    /// 将二维行列坐标 [`BufferPoint`] 转换为 UTF-8 字节偏移量（超出该行长度时自动钳制到行尾）。
    pub fn point_to_offset(&self, point: BufferPoint) -> usize {
        if let Some(range) = self.line_range(point.row) {
            let target = (range.start + point.column).min(range.end);
            clip_offset_left(self.as_str(), target)
        } else {
            self.len
        }
    }

    /// 获取当前光标所在的 [`BufferPoint`] `(row, column)`。
    #[inline]
    pub fn cursor_point(&self) -> BufferPoint {
        self.offset_to_point(self.cursor())
    }

    /// 将光标直接移动到指定的二维行列坐标（折叠选区）。
    pub fn set_cursor_point(&mut self, point: BufferPoint) {
        let offset = self.point_to_offset(point);
        self.selection = CursorSelection::new(0, offset, offset);
    }

    /// 按锚点与活动点设置文本选区（支持正向与反向鼠标拖拽选中）。
    pub fn select_to_point(&mut self, anchor: BufferPoint, active: BufferPoint) {
        let a_off = self.point_to_offset(anchor);
        let b_off = self.point_to_offset(active);
        let (start, end, reversed) = if b_off < a_off {
            (b_off, a_off, true)
        } else {
            (a_off, b_off, false)
        };
        let mut sel = CursorSelection::new(0, start, end);
        sel.reversed = reversed;
        self.selection = sel;
    }

    fn alloc_undo_bytes(&mut self, data: &[u8]) -> Option<u32> {
        let len = data.len();
        if len == 0 {
            return Some(self.undo_pool_seq);
        }
        if len > UNDO_POOL_CAP {
            return None;
        }
        let pos = (self.undo_pool_seq as usize) % UNDO_POOL_CAP;
        if pos + len > UNDO_POOL_CAP {
            let skip = (UNDO_POOL_CAP - pos) as u32;
            self.undo_pool_seq = self.undo_pool_seq.wrapping_add(skip);
        }
        let start_seq = self.undo_pool_seq;
        let write_pos = (start_seq as usize) % UNDO_POOL_CAP;
        self.undo_pool[write_pos..write_pos + len].copy_from_slice(data);
        self.undo_pool_seq = self.undo_pool_seq.wrapping_add(len as u32);
        Some(start_seq)
    }

    fn undo_slice_valid(&self, seq: u32, len: u16) -> bool {
        if len == 0 {
            return true;
        }
        let dist = self.undo_pool_seq.wrapping_sub(seq) as usize;
        dist <= UNDO_POOL_CAP && ((seq as usize) % UNDO_POOL_CAP) + (len as usize) <= UNDO_POOL_CAP
    }

    fn record_edit_delta(
        &mut self,
        offset: usize,
        deleted_range: Range<usize>,
        inserted: &[u8],
        cursor_before: usize,
        cursor_after: usize,
    ) {
        if self.history.is_ignoring() {
            return;
        }
        let del_len = deleted_range.len();
        let ins_len = inserted.len();
        if del_len + ins_len > UNDO_POOL_CAP || offset > u16::MAX as usize {
            return;
        }
        // 先将待删除区间复制到池（若 del_len > 0）
        let del_seq = if del_len == 0 {
            self.undo_pool_seq
        } else {
            let pos = (self.undo_pool_seq as usize) % UNDO_POOL_CAP;
            if pos + del_len > UNDO_POOL_CAP {
                let skip = (UNDO_POOL_CAP - pos) as u32;
                self.undo_pool_seq = self.undo_pool_seq.wrapping_add(skip);
            }
            let s = self.undo_pool_seq;
            let wp = (s as usize) % UNDO_POOL_CAP;
            self.undo_pool[wp..wp + del_len].copy_from_slice(&self.bytes[deleted_range]);
            self.undo_pool_seq = self.undo_pool_seq.wrapping_add(del_len as u32);
            s
        };
        let Some(ins_seq) = self.alloc_undo_bytes(inserted) else {
            return;
        };
        self.history.push(TextSnapshot {
            del_seq,
            ins_seq,
            offset: offset as u16,
            del_len: del_len as u16,
            ins_len: ins_len as u16,
            cursor_before: cursor_before.min(u16::MAX as usize) as u16,
            cursor_after: cursor_after.min(u16::MAX as usize) as u16,
        });
    }

    /// 整体设置文档内容并将光标置于末尾。
    pub fn set_text(&mut self, text: &str) {
        let mut valid_len = 0usize;
        for ch in text.chars() {
            if valid_len + ch.len_utf8() <= CAP {
                valid_len += ch.len_utf8();
            } else {
                break;
            }
        }
        let cur_before = self.cursor();
        self.record_edit_delta(
            0,
            0..self.len,
            &text.as_bytes()[..valid_len],
            cur_before,
            valid_len,
        );
        self.bytes[..valid_len].copy_from_slice(&text.as_bytes()[..valid_len]);
        self.len = valid_len;
        self.selection.place_at(valid_len, None);
        self.auto_closed_pairs.clear();
        let s = unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) };
        self.search.update_matches(s);
    }

    /// 替换字节区间 `range` 内的文本为 `replacement`。
    pub fn replace_range(&mut self, range: Range<usize>, replacement: &str) -> bool {
        let start = clip_offset_left(self.as_str(), range.start);
        let end = clip_offset_left(self.as_str(), range.end).max(start);
        let removed_len = end - start;

        let mut valid_ins = 0usize;
        for ch in replacement.chars() {
            if self.len - removed_len + valid_ins + ch.len_utf8() <= CAP {
                valid_ins += ch.len_utf8();
            } else {
                break;
            }
        }

        let cur_before = self.cursor();
        let new_cursor = start + valid_ins;
        self.record_edit_delta(
            start,
            start..end,
            &replacement.as_bytes()[..valid_ins],
            cur_before,
            new_cursor,
        );
        self.bytes.copy_within(end..self.len, start + valid_ins);
        self.bytes[start..start + valid_ins].copy_from_slice(&replacement.as_bytes()[..valid_ins]);
        self.len = self.len - removed_len + valid_ins;
        self.selection.place_at(new_cursor, None);
        self.auto_closed_pairs.adjust(start..end, valid_ins);
        let s = unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) };
        self.search.update_matches(s);
        true
    }

    /// 在当前选区或光标位置插入字符串（若存在选区则先替换选区）。
    pub fn insert_str(&mut self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let range = if !self.selection.is_empty() {
            self.selection.start..self.selection.end
        } else {
            let cur = self.cursor();
            cur..cur
        };
        self.replace_range(range, s)
    }

    /// 删除当前选区内的文本（若无选区则返回 `false`）。
    pub fn delete_selection(&mut self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        self.replace_range(self.selection.start..self.selection.end, "")
    }

    /// 撤销上一步编辑操作（`Ctrl+Z`）。
    pub fn undo(&mut self) -> bool {
        let Some(tx) = self.history.undo() else {
            return false;
        };
        let mut applied = false;
        let mut final_cursor = self.cursor();
        for rec in tx.as_slice() {
            if !self.undo_slice_valid(rec.del_seq, rec.del_len)
                || !self.undo_slice_valid(rec.ins_seq, rec.ins_len)
            {
                break;
            }
            let off = (rec.offset as usize).min(self.len);
            let ins_len = (rec.ins_len as usize).min(self.len.saturating_sub(off));
            let del_len = rec.del_len as usize;
            if !self.as_str().is_char_boundary(off)
                || !self.as_str().is_char_boundary(off + ins_len)
                || self.len - ins_len + del_len > CAP
            {
                break;
            }
            self.bytes.copy_within(off + ins_len..self.len, off + del_len);
            if del_len > 0 {
                let p = (rec.del_seq as usize) % UNDO_POOL_CAP;
                self.bytes[off..off + del_len]
                    .copy_from_slice(&self.undo_pool[p..p + del_len]);
            }
            self.len = self.len - ins_len + del_len;
            final_cursor = rec.cursor_before as usize;
            applied = true;
        }
        if applied {
            let cur = clip_offset_left(self.as_str(), final_cursor);
            self.selection.place_at(cur, None);
            let s = unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) };
            self.search.update_matches(s);
        }
        applied
    }

    /// 重做上一步撤销的编辑操作（`Ctrl+Y` / `Ctrl+Shift+Z`）。
    pub fn redo(&mut self) -> bool {
        let Some(tx) = self.history.redo() else {
            return false;
        };
        let mut applied = false;
        let mut final_cursor = self.cursor();
        for rec in tx.as_slice() {
            if !self.undo_slice_valid(rec.del_seq, rec.del_len)
                || !self.undo_slice_valid(rec.ins_seq, rec.ins_len)
            {
                break;
            }
            let off = (rec.offset as usize).min(self.len);
            let del_len = (rec.del_len as usize).min(self.len.saturating_sub(off));
            let ins_len = rec.ins_len as usize;
            if !self.as_str().is_char_boundary(off)
                || !self.as_str().is_char_boundary(off + del_len)
                || self.len - del_len + ins_len > CAP
            {
                break;
            }
            self.bytes.copy_within(off + del_len..self.len, off + ins_len);
            if ins_len > 0 {
                let p = (rec.ins_seq as usize) % UNDO_POOL_CAP;
                self.bytes[off..off + ins_len]
                    .copy_from_slice(&self.undo_pool[p..p + ins_len]);
            }
            self.len = self.len - del_len + ins_len;
            final_cursor = rec.cursor_after as usize;
            applied = true;
        }
        if applied {
            let cur = clip_offset_left(self.as_str(), final_cursor);
            self.selection.place_at(cur, None);
            let s = unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) };
            self.search.update_matches(s);
        }
        applied
    }

    /// 全选文档。
    #[inline]
    pub fn select_all(&mut self) {
        self.selection = CursorSelection::new(0, 0, self.len);
    }

    /// 选中指定偏移处的整个单词（双击选词）。
    pub fn select_word_at(&mut self, offset: usize) -> bool {
        if let Some(range) = word_range_at(self.as_str(), offset) {
            self.selection = CursorSelection::new(0, range.start, range.end);
            true
        } else {
            false
        }
    }

    /// 选中指定偏移处的整行（三击选行）。
    pub fn select_line_at(&mut self, offset: usize) {
        let range = line_range_at(self.as_str(), offset);
        self.selection = CursorSelection::new(0, range.start, range.end);
    }

    /// 插入单个字符（支持自动闭合括号/引号以及跳过已自动闭合的右括号）。
    pub fn insert_char(&mut self, ch: char) -> bool {
        if ch == '\n' {
            return self.insert_newline();
        }
        if ch.is_control() && ch != '\t' {
            return false;
        }

        let cur = self.cursor();
        // 1. 若位于自动闭合的右定界符之前且用户输入了相同的右定界符，直接跳过而不重复插入
        if self.selection.is_empty() && self.auto_close_enabled {
            if let Some(next_ch) = self.as_str()[cur..].chars().next() {
                if next_ch == ch && self.auto_closed_pairs.contains_closer(cur, ch.len_utf8()) {
                    self.selection.place_at(cur + ch.len_utf8(), None);
                    return true;
                }
            }
        }

        // 2. 若输入左括号/引号且开启了自动配对，同时插入左、右定界符并将光标置于中间
        if self.selection.is_empty() && self.auto_close_enabled {
            if let Some(close_ch) = AutoClosedPairs::<16>::closing_char_for(ch) {
                let mut pair_buf = [0u8; 8];
                let open_len = ch.encode_utf8(&mut pair_buf).len();
                let close_len = close_ch.encode_utf8(&mut pair_buf[open_len..]).len();
                let total_len = open_len + close_len;
                    if self.len + total_len <= CAP {
                        let pair_str =
                            unsafe { core::str::from_utf8_unchecked(&pair_buf[..total_len]) };
                        self.replace_range(cur..cur, pair_str);
                        let mid = cur + open_len;
                        self.selection.place_at(mid, None);
                        if let Some(last_tx) = self.history.last_undo_mut() {
                            if let Some(last_rec) = last_tx.as_mut_slice().last_mut() {
                                last_rec.cursor_after = mid.min(u16::MAX as usize) as u16;
                            }
                        }
                        self.auto_closed_pairs
                            .record(cur..mid, mid..(mid + close_len));
                        return true;
                    }
            }
        }

        let mut b = [0u8; 4];
        let s = ch.encode_utf8(&mut b);
        let range = self.selection.start..self.selection.end;
        self.replace_range(range, s)
    }

    /// 插入换行符，并自动继承当前行的前导缩进（若行尾为 `{`、`(`、`[` 则额外增加一级缩进）。
    pub fn insert_newline(&mut self) -> bool {
        let cur = self.cursor();
        let line_r = line_range_at(self.as_str(), cur);
        let current_line = &self.as_str()[line_r.start..cur.min(line_r.end)];
        let base_indent = if self.auto_indent_enabled {
            self.tab_size.indent_count(current_line)
        } else {
            0
        };
        let extra_indent = if self.auto_indent_enabled
            && current_line
                .trim_end()
                .chars()
                .next_back()
                .is_some_and(|c| matches!(c, '{' | '(' | '[' | ':'))
        {
            self.tab_size.tab_size as usize
        } else {
            0
        };
        let total_spaces = (base_indent + extra_indent).min(32);
        let mut nl_buf = [b' '; 33];
        nl_buf[0] = b'\n';
        let ins_str = unsafe { core::str::from_utf8_unchecked(&nl_buf[..1 + total_spaces]) };
        let range = self.selection.start..self.selection.end;
        self.replace_range(range, ins_str)
    }

    /// 退格删除（若光标处于刚自动配对的空括号 `()` 中间，则一并删除成对括号）。
    pub fn backspace(&mut self) -> bool {
        if !self.selection.is_empty() {
            let r = self.selection.start..self.selection.end;
            return self.replace_range(r, "");
        }
        let cur = self.cursor();
        if cur == 0 {
            return false;
        }
        if self.auto_close_enabled {
            if let Some((open, close)) = self.auto_closed_pairs.has_adjacent_pair_at(cur) {
                return self.replace_range(open.start..close.end, "");
            }
        }
        let mut prev = self.as_str()[..cur]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        if self.bytes[prev] == b'\n' && prev > 0 && self.bytes[prev - 1] == b'\r' {
            prev -= 1;
        }
        self.replace_range(prev..cur, "")
    }

    /// 向后删除一个字符（`Delete`）。
    pub fn delete_forward(&mut self) -> bool {
        if !self.selection.is_empty() {
            let r = self.selection.start..self.selection.end;
            return self.replace_range(r, "");
        }
        let cur = self.cursor();
        if cur >= self.len {
            return false;
        }
        let mut next = self.as_str()[cur..]
            .chars()
            .next()
            .map(|c| cur + c.len_utf8())
            .unwrap_or(self.len);
        if self.bytes[cur] == b'\r' && next < self.len && self.bytes[next] == b'\n' {
            next += 1;
        }
        self.replace_range(cur..next, "")
    }

    /// 向左移动光标（`extend_selection == true` 时扩展选区）。
    pub fn move_left(&mut self, extend_selection: bool) -> bool {
        if !extend_selection && !self.selection.is_empty() {
            self.selection.place_at(self.selection.start, None);
            return true;
        }
        let cur = self.cursor();
        if cur == 0 {
            return false;
        }
        let mut prev = self.as_str()[..cur]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        if self.bytes[prev] == b'\n' && prev > 0 && self.bytes[prev - 1] == b'\r' {
            prev -= 1;
        }
        if extend_selection {
            self.selection.extend_to(prev);
        } else {
            self.selection.place_at(prev, None);
        }
        true
    }

    /// 向右移动光标（`extend_selection == true` 时扩展选区）。
    pub fn move_right(&mut self, extend_selection: bool) -> bool {
        if !extend_selection && !self.selection.is_empty() {
            self.selection.place_at(self.selection.end, None);
            return true;
        }
        let cur = self.cursor();
        if cur >= self.len {
            return false;
        }
        let mut next = self.as_str()[cur..]
            .chars()
            .next()
            .map(|c| cur + c.len_utf8())
            .unwrap_or(self.len);
        if self.bytes[cur] == b'\r' && next < self.len && self.bytes[next] == b'\n' {
            next += 1;
        }
        if extend_selection {
            self.selection.extend_to(next);
        } else {
            self.selection.place_at(next, None);
        }
        true
    }

    /// 按词向左跳转（`Ctrl+Left`）。
    pub fn move_word_left(&mut self, extend_selection: bool) -> bool {
        let cur = self.cursor();
        let target = prev_word_boundary(self.as_str(), cur);
        if target == cur && (extend_selection || self.selection.is_empty()) {
            return false;
        }
        if extend_selection {
            self.selection.extend_to(target);
        } else {
            self.selection.place_at(target, None);
        }
        true
    }

    /// 按词向右跳转（`Ctrl+Right`）。
    pub fn move_word_right(&mut self, extend_selection: bool) -> bool {
        let cur = self.cursor();
        let target = next_word_boundary(self.as_str(), cur);
        if target == cur && (extend_selection || self.selection.is_empty()) {
            return false;
        }
        if extend_selection {
            self.selection.extend_to(target);
        } else {
            self.selection.place_at(target, None);
        }
        true
    }

    /// 向上移动一行并保持记忆列锚点 `column_anchor`。
    pub fn move_up(&mut self, extend_selection: bool) -> bool {
        let pt = self.cursor_point();
        if pt.row == 0 {
            if extend_selection {
                self.selection.extend_to(0);
            } else {
                self.selection.place_at(0, Some(0));
            }
            return pt.column > 0;
        }
        let preferred_col = self.selection.column_anchor.unwrap_or(pt.column);
        let target_offset = self.point_to_offset(BufferPoint::new(pt.row - 1, preferred_col));
        if extend_selection {
            self.selection.extend_to(target_offset);
            self.selection.column_anchor = Some(preferred_col);
        } else {
            self.selection.place_at(target_offset, Some(preferred_col));
        }
        true
    }

    /// 向下移动一行并保持记忆列锚点 `column_anchor`。
    pub fn move_down(&mut self, extend_selection: bool) -> bool {
        let pt = self.cursor_point();
        let total_lines = self.line_count();
        if pt.row + 1 >= total_lines {
            if extend_selection {
                self.selection.extend_to(self.len);
            } else {
                self.selection.place_at(self.len, None);
            }
            return false;
        }
        let preferred_col = self.selection.column_anchor.unwrap_or(pt.column);
        let target_offset = self.point_to_offset(BufferPoint::new(pt.row + 1, preferred_col));
        if extend_selection {
            self.selection.extend_to(target_offset);
            self.selection.column_anchor = Some(preferred_col);
        } else {
            self.selection.place_at(target_offset, Some(preferred_col));
        }
        true
    }

    /// 移动到当前行首（若已在非空白首字符则跳到行首第 0 列，否则跳到首个非空白字符）。
    pub fn move_line_home(&mut self, extend_selection: bool) -> bool {
        let cur = self.cursor();
        let line_r = line_range_at(self.as_str(), cur);
        let indent_bytes = self.as_str()[line_r.clone()]
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let first_non_ws = line_r.start + indent_bytes;
        let target = if cur == first_non_ws {
            line_r.start
        } else {
            first_non_ws
        };
        if extend_selection {
            self.selection.extend_to(target);
        } else {
            self.selection.place_at(target, None);
        }
        true
    }

    /// 移动到当前行尾。
    pub fn move_line_end(&mut self, extend_selection: bool) -> bool {
        let cur = self.cursor();
        let line_r = line_range_at(self.as_str(), cur);
        if extend_selection {
            self.selection.extend_to(line_r.end);
        } else {
            self.selection.place_at(line_r.end, None);
        }
        true
    }

    /// 对当前行增加一级缩进（`Tab`）。
    pub fn indent_line(&mut self) -> bool {
        let cur = self.cursor();
        let line_r = line_range_at(self.as_str(), cur);
        if self.tab_size.hard_tabs {
            self.replace_range(line_r.start..line_r.start, "\t")
        } else {
            let spaces = (self.tab_size.tab_size.max(1) as usize).min(8);
            let buf = [b' '; 8];
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..spaces]) };
            self.replace_range(line_r.start..line_r.start, s)
        }
    }

    /// 对当前行减少一级缩进（`Shift+Tab`）。
    pub fn outdent_line(&mut self) -> bool {
        let cur = self.cursor();
        let line_r = line_range_at(self.as_str(), cur);
        let line = &self.as_str()[line_r.clone()];
        if line.starts_with('\t') {
            return self.replace_range(line_r.start..(line_r.start + 1), "");
        }
        let max_remove = self.tab_size.tab_size.max(1) as usize;
        let remove_n = line
            .bytes()
            .take(max_remove)
            .take_while(|&b| b == b' ')
            .count();
        if remove_n > 0 {
            self.replace_range(line_r.start..(line_r.start + remove_n), "")
        } else {
            false
        }
    }

    /// 处理引擎派发的键盘输入事件。
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char(ch) => self.insert_char(ch),
            KeyCode::Space => self.insert_char(' '),
            KeyCode::Enter => self.insert_newline(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.move_left(false),
            KeyCode::Right => self.move_right(false),
            KeyCode::Up => self.move_up(false),
            KeyCode::Down => self.move_down(false),
            KeyCode::Home => self.move_line_home(false),
            KeyCode::End => self.move_line_end(false),
            KeyCode::Tab => self.indent_line(),
            _ => false,
        }
    }
}
