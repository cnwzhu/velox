//! 核心多行文本编辑器状态机（[`CosmicEditor`]）。
//!
//! 参照 `pop-os/cosmic-text/src/edit/editor.rs` 设计并针对 Velox 工程需求增强：
//! - 完整实现 [`Edit`] 特征与全部 [`Action`] / [`Motion`] / [`EditSelection`] 语义；
//! - 原生内置基于 [`Change::reverse`] 的原子事务撤销/重做栈（支持多行删除、整块缩进、自动括号配对一键无损撤销）；
//! - 内置鼠标单/双/三击连击检测器（单击落点、双击选词、三击选行、按对应粒度流畅拖拽）；
//! - 内置 `Shift` 选区扩展、`Ctrl+Left/Right` 按词、`Ctrl+Home/End`、`Ctrl+A/Z/Y`、`Tab`/`Shift+Tab` 块缩进快捷键分发。

use velox_core::arena::{SmallString, SmallVec};
use velox_core::platform::{clipboard_read_into, clipboard_write_text};
use velox_core::pod::{FrameScope, KeyCode, KeyModifiers};

use super::super::attrs::{Attrs, AttrsList};
use super::super::buffer::TextDocBuffer;
use super::super::buffer_line::BufferLine;
use super::super::cursor::{Affinity, Cursor, Motion};
use super::super::line_ending::{split_lines_with_ending, LineEnding};
use super::super::shape::{Metrics, Wrap};
use super::{Action, Change, ChangeItem, Edit, EditSelection};
use crate::input::editor_state::AutoClosedPairs;
use crate::input::text_boundary::{clip_offset_left, word_range_at};

const DOUBLE_CLICK_MS: u32 = 380;
const DOUBLE_CLICK_SLOP_PX: i32 = 6;
const MAX_UNDO_HISTORY: usize = 64;

/// 参照 `cosmic-text::Editor` 构建的多行文本编辑器状态机。
#[derive(Clone, Debug)]
pub struct CosmicEditor {
    buffer: TextDocBuffer,
    cursor: Cursor,
    cursor_x_opt: Option<i32>,
    selection: EditSelection,
    cursor_moved: bool,
    auto_indent: bool,
    auto_close: bool,
    hard_tabs: bool,
    change: Option<Change>,
    undo_stack: SmallVec<Change, 8>,
    redo_stack: SmallVec<Change, 8>,
    last_click_ms: u32,
    last_click_pos: (i32, i32),
    click_streak: u8,
    blink_epoch_ms: u32,
}

impl Default for CosmicEditor {
    fn default() -> Self {
        Self::new(TextDocBuffer::default())
    }
}

impl CosmicEditor {
    /// 基于已有 [`TextDocBuffer`] 创建编辑器状态机。
    pub fn new(buffer: TextDocBuffer) -> Self {
        Self {
            buffer,
            cursor: Cursor::default(),
            cursor_x_opt: None,
            selection: EditSelection::None,
            cursor_moved: false,
            auto_indent: true,
            auto_close: true,
            hard_tabs: false,
            change: None,
            undo_stack: SmallVec::new(),
            redo_stack: SmallVec::new(),
            last_click_ms: 0,
            last_click_pos: (0, 0),
            click_streak: 0,
            blink_epoch_ms: 0,
        }
    }

    /// 便捷构造函数：以初始文本和字体度量创建多行编辑器（光标默认停留在文档开头或指定位置）。
    pub fn from_str(metrics: Metrics, initial: &str) -> Self {
        let mut buf = TextDocBuffer::from_str(metrics, initial, Attrs::new());
        buf.set_tab_width(4);
        buf.set_wrap(Wrap::None);
        let mut ed = Self::new(buf);
        // 与旧版 TextBuffer::from_str 行为保持一致：初始光标置于文档末尾以便即刻续写
        let last_line = ed.buffer.lines.len().saturating_sub(1);
        let last_col = ed.buffer.lines[last_line].text().len();
        ed.cursor = Cursor::new(last_line, last_col);
        ed
    }

    /// 启用或禁用自动括号/引号配对闭合。
    #[inline]
    pub fn set_auto_close(&mut self, enabled: bool) {
        self.auto_close = enabled;
    }

    #[inline(always)]
    pub const fn auto_close(&self) -> bool {
        self.auto_close
    }

    /// 设置缩进时是否插入硬 `\t`（默认 `false` 插入空格）。
    #[inline]
    pub fn set_hard_tabs(&mut self, hard_tabs: bool) {
        self.hard_tabs = hard_tabs;
    }

    #[inline(always)]
    pub const fn hard_tabs(&self) -> bool {
        self.hard_tabs
    }

    /// 重置整个文档内容并清空撤销/重做历史。
    pub fn set_text(&mut self, text: &str) {
        self.buffer.set_text(text, Attrs::new());
        let last_line = self.buffer.lines.len().saturating_sub(1);
        let last_col = self.buffer.lines[last_line].text().len();
        self.cursor = Cursor::new(last_line, last_col);
        self.cursor_x_opt = None;
        self.selection = EditSelection::None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.change = None;
    }

    /// 文档总字节长度（含换行符）。
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.total_bytes()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.total_bytes() == 0
    }

    /// 文档逻辑行总数。
    #[inline]
    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// 返回当前选区包含的字节长度（无选区返回 `0`）。
    pub fn selected_byte_len(&self) -> usize {
        let Some((s, e)) = self.selection_bounds() else {
            return 0;
        };
        let a = self.buffer.cursor_to_byte_offset(s);
        let b = self.buffer.cursor_to_byte_offset(e);
        b.saturating_sub(a)
    }

    /// 将当前文档序列化为扁平 [`SmallString`]。
    #[inline]
    pub fn to_flat_string<const CAP: usize>(&self) -> SmallString<CAP> {
        self.buffer.to_flat_string()
    }

    /// 将当前文档零堆拷贝至帧瞬态 Arena 并返回切片（便于测试或兼容旧视图）。
    #[inline]
    pub fn to_arena_str<'a>(&self, cx: &'a FrameScope) -> &'a str {
        self.buffer.to_arena_str(cx)
    }

    #[inline]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[inline]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// 撤销上一条原子编辑事务（`Ctrl+Z`）。
    pub fn undo(&mut self) -> bool {
        let Some(chg) = self.undo_stack.pop() else {
            return false;
        };
        let mut rev = chg.reversed();
        let saved_active = self.change.take();
        let ok = self.apply_change(&rev);
        self.change = saved_active;
        if ok {
            self.selection = EditSelection::None;
            self.cursor_x_opt = None;
            self.cursor_moved = true;
            // 推入重做栈（保持正向事务）
            rev.reverse();
            let _ = chg;
            Self::push_bounded(&mut self.redo_stack, rev);
            self.buffer.ensure_cursor_visible(self.cursor);
            self.buffer.set_redraw(true);
            true
        } else {
            false
        }
    }

    /// 重做上一条被撤销的事务（`Ctrl+Y` / `Ctrl+Shift+Z`）。
    pub fn redo(&mut self) -> bool {
        let Some(chg) = self.redo_stack.pop() else {
            return false;
        };
        let saved_active = self.change.take();
        let ok = self.apply_change(&chg);
        self.change = saved_active;
        if ok {
            self.selection = EditSelection::None;
            self.cursor_x_opt = None;
            self.cursor_moved = true;
            Self::push_bounded(&mut self.undo_stack, chg);
            self.buffer.ensure_cursor_visible(self.cursor);
            self.buffer.set_redraw(true);
            true
        } else {
            false
        }
    }

    /// 获取最近一次交互的光标闪烁复位时间戳（毫秒）。
    #[inline(always)]
    pub const fn blink_epoch_ms(&self) -> u32 {
        self.blink_epoch_ms
    }

    /// 显式复位光标闪烁相位，使光标立即常亮。
    #[inline(always)]
    pub fn reset_blink(&mut self, now_ms: u32) {
        self.blink_epoch_ms = now_ms;
    }

    /// 将当前选区复制到操作系统剪贴板。
    pub fn copy_to_clipboard(&self) -> bool {
        let Some(sel) = self.copy_selection() else {
            return false;
        };
        if sel.is_empty() {
            return false;
        }
        clipboard_write_text(sel.as_str())
    }

    /// 将当前选区剪切到操作系统剪贴板（完整记入撤销历史栈）。
    pub fn cut_to_clipboard(&mut self) -> bool {
        let Some(sel) = self.copy_selection() else {
            return false;
        };
        if sel.is_empty() {
            return false;
        }
        if clipboard_write_text(sel.as_str()) {
            self.action(Action::Backspace);
            true
        } else {
            false
        }
    }

    /// 从操作系统剪贴板读取文本并粘贴至当前光标/选区（完整记入撤销历史栈）。
    pub fn paste_from_clipboard(&mut self) -> bool {
        let clip = clipboard_read_into::<1024>();
        if clip.is_empty() {
            return false;
        }
        let owns_tx = self.change.is_none();
        if owns_tx {
            self.start_change();
        }
        self.insert_string(clip.as_str(), None);
        if owns_tx {
            let chg = self.finish_change();
            self.commit_undo_change(chg);
        }
        true
    }

    /// 处理鼠标左键按下事件：自动识别单击、双击（选词）、三击（选行）与 `Shift+Click` 扩展选区。
    pub fn handle_pointer_press(&mut self, x: i32, y: i32, now_ms: u32, shift: bool) {
        self.blink_epoch_ms = now_ms;
        if shift {
            if matches!(self.selection, EditSelection::None) {
                self.selection = EditSelection::Normal(self.cursor);
            }
            self.action(Action::Drag { x, y });
            self.click_streak = 0;
            return;
        }
        let dt = now_ms.wrapping_sub(self.last_click_ms);
        let dx = (x - self.last_click_pos.0).abs();
        let dy = (y - self.last_click_pos.1).abs();
        if self.click_streak > 0
            && dt <= DOUBLE_CLICK_MS
            && dx <= DOUBLE_CLICK_SLOP_PX
            && dy <= DOUBLE_CLICK_SLOP_PX
        {
            self.click_streak = (self.click_streak % 3) + 1;
        } else {
            self.click_streak = 1;
        }
        self.last_click_ms = now_ms;
        self.last_click_pos = (x, y);

        match self.click_streak {
            2 => self.action(Action::DoubleClick { x, y }),
            3 => self.action(Action::TripleClick { x, y }),
            _ => self.action(Action::Click { x, y }),
        }
    }

    /// 处理鼠标拖拽选区事件（自动延续单击普通选区、双击按词选区或三击按行选区）。
    #[inline]
    pub fn handle_pointer_drag(&mut self, x: i32, y: i32) {
        self.action(Action::Drag { x, y });
    }

    /// 处理引擎派发的键盘按键与修饰键组合（支持完整现代编辑器快捷键）。
    pub fn handle_key_with_mods(&mut self, key: KeyCode, mods: KeyModifiers) -> bool {
        let ctrl_or_cmd = mods.ctrl() || mods.logo();
        let shift = mods.shift();

        // 1. 组合快捷键（Ctrl / Cmd + 键）
        if ctrl_or_cmd {
            match key {
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.select_all();
                    return true;
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    return self.copy_to_clipboard();
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    return self.cut_to_clipboard();
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    return self.paste_from_clipboard();
                }
                KeyCode::Char('z') | KeyCode::Char('Z') => {
                    return if shift { self.redo() } else { self.undo() };
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    return self.redo();
                }
                KeyCode::Left => {
                    return self.apply_motion_with_shift(Motion::PreviousWord, shift);
                }
                KeyCode::Right => {
                    return self.apply_motion_with_shift(Motion::NextWord, shift);
                }
                KeyCode::Home => {
                    return self.apply_motion_with_shift(Motion::BufferStart, shift);
                }
                KeyCode::End => {
                    return self.apply_motion_with_shift(Motion::BufferEnd, shift);
                }
                KeyCode::Backspace => {
                    if self.selection_bounds().is_none() {
                        let prev_sel = self.cursor;
                        if let Some((target, _)) =
                            self.buffer.cursor_motion(self.cursor, None, Motion::PreviousWord)
                        {
                            if target < prev_sel {
                                self.selection = EditSelection::Normal(prev_sel);
                                self.cursor = target;
                            }
                        }
                    }
                    self.action(Action::Backspace);
                    return true;
                }
                KeyCode::Delete => {
                    if self.selection_bounds().is_none() {
                        let start = self.cursor;
                        if let Some((target, _)) =
                            self.buffer.cursor_motion(self.cursor, None, Motion::NextWord)
                        {
                            if target > start {
                                self.selection = EditSelection::Normal(start);
                                self.cursor = target;
                            }
                        }
                    }
                    self.action(Action::Delete);
                    return true;
                }
                _ => return false,
            }
        }

        // 2. 常规按键与 Shift 选区扩展
        match key {
            KeyCode::Char(ch) => {
                if ch == '\n' || ch == '\r' {
                    self.action(Action::Enter);
                } else if !ch.is_control() || ch == '\t' {
                    self.action(Action::Insert(ch));
                } else {
                    return false;
                }
                true
            }
            KeyCode::Space => {
                self.action(Action::Insert(' '));
                true
            }
            KeyCode::Enter => {
                self.action(Action::Enter);
                true
            }
            KeyCode::Backspace => {
                self.action(Action::Backspace);
                true
            }
            KeyCode::Delete => {
                self.action(Action::Delete);
                true
            }
            KeyCode::Tab => {
                if shift {
                    self.action(Action::Unindent);
                } else {
                    self.action(Action::Indent);
                }
                true
            }
            KeyCode::BackTab => {
                self.action(Action::Unindent);
                true
            }
            KeyCode::Escape => {
                if !matches!(self.selection, EditSelection::None) {
                    self.action(Action::Escape);
                    true
                } else {
                    false
                }
            }
            KeyCode::Left => self.apply_motion_with_shift(Motion::Left, shift),
            KeyCode::Right => self.apply_motion_with_shift(Motion::Right, shift),
            KeyCode::Up => self.apply_motion_with_shift(Motion::Up, shift),
            KeyCode::Down => self.apply_motion_with_shift(Motion::Down, shift),
            KeyCode::Home => self.apply_motion_with_shift(Motion::SoftHome, shift),
            KeyCode::End => self.apply_motion_with_shift(Motion::End, shift),
            KeyCode::PageUp => self.apply_motion_with_shift(Motion::PageUp, shift),
            KeyCode::PageDown => self.apply_motion_with_shift(Motion::PageDown, shift),
        }
    }

    /// 无修饰键快捷入口。
    #[inline]
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        self.handle_key_with_mods(key, KeyModifiers::NONE)
    }

    fn apply_motion_with_shift(&mut self, motion: Motion, extend_selection: bool) -> bool {
        let prev_cursor = self.cursor;
        let prev_sel = self.selection;
        if extend_selection {
            if matches!(self.selection, EditSelection::None) {
                self.selection = EditSelection::Normal(self.cursor);
            }
            if let Some((next_c, next_x)) =
                self.buffer.cursor_motion(self.cursor, self.cursor_x_opt, motion)
            {
                self.cursor = next_c;
                self.cursor_x_opt = next_x;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                self.buffer.set_redraw(true);
            }
        } else {
            // 未按 Shift 且存在选区时，Left/Right 直接折叠到选区首/尾（标准编辑器手感）
            if let Some((s, e)) = self.selection_bounds() {
                match motion {
                    Motion::Left | Motion::Previous => {
                        self.cursor = s;
                        self.selection = EditSelection::None;
                        self.cursor_x_opt = None;
                        self.cursor_moved = true;
                        self.buffer.ensure_cursor_visible(self.cursor);
                        self.buffer.set_redraw(true);
                        return true;
                    }
                    Motion::Right | Motion::Next => {
                        self.cursor = e;
                        self.selection = EditSelection::None;
                        self.cursor_x_opt = None;
                        self.cursor_moved = true;
                        self.buffer.ensure_cursor_visible(self.cursor);
                        self.buffer.set_redraw(true);
                        return true;
                    }
                    _ => {}
                }
            }
            self.selection = EditSelection::None;
            self.action(Action::Motion(motion));
        }
        self.cursor != prev_cursor || self.selection != prev_sel
    }

    fn push_bounded(stack: &mut SmallVec<Change, 8>, chg: Change) {
        if stack.len() >= MAX_UNDO_HISTORY {
            let _ = stack.remove(0);
        }
        if !stack.push(chg.clone()) && !stack.is_empty() {
            // `#![no_std]` 固定 32 槽满时淘汰最旧记录
            let _ = stack.remove(0);
            let _ = stack.push(chg);
        }
    }

    fn commit_undo_change(&mut self, chg: Option<Change>) {
        if let Some(c) = chg {
            if !c.is_empty() {
                Self::push_bounded(&mut self.undo_stack, c);
                self.redo_stack.clear();
            }
        }
    }

    /// 钳制光标至当前缓冲区合法逻辑行与 UTF-8 字符边界。
    fn clamp_cursor(&self, mut c: Cursor) -> Cursor {
        if self.buffer.lines.is_empty() {
            return Cursor::default();
        }
        c.line = c.line.min(self.buffer.lines.len() - 1);
        c.index = clip_offset_left(self.buffer.lines[c.line].text(), c.index);
        c
    }

    /// 计算逻辑光标所在单词的起止边界 `(start_cursor, end_cursor)`。
    fn word_bounds_at(&self, c: Cursor) -> (Cursor, Cursor) {
        let c = self.clamp_cursor(c);
        let line = self.buffer.lines[c.line].text();
        if line.is_empty() {
            return (Cursor::new(c.line, 0), Cursor::new(c.line, 0));
        }
        let probe = if c.index >= line.len() {
            line.char_indices().next_back().map_or(0, |(i, _)| i)
        } else if c.affinity.before() && c.index > 0 {
            let prev = line[..c.index]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            let cur_ch = line[c.index..].chars().next();
            let prev_ch = line[prev..].chars().next();
            if matches!(cur_ch, Some(ch) if ch.is_whitespace())
                && matches!(prev_ch, Some(ch) if !ch.is_whitespace())
            {
                prev
            } else {
                c.index
            }
        } else {
            c.index
        };
        if let Some(r) = word_range_at(line, probe) {
            (Cursor::new(c.line, r.start), Cursor::new(c.line, r.end))
        } else {
            (c, c)
        }
    }

    /// 提取指定区间 `[start, end)` 的完整文本（含跨行行尾结束符）。
    fn extract_range(&self, start: Cursor, end: Cursor) -> SmallString<128> {
        let s = self.clamp_cursor(start);
        let e = self.clamp_cursor(end);
        let (start, end) = if s <= e { (s, e) } else { (e, s) };
        let mut out = SmallString::new();
        if start == end {
            return out;
        }
        if start.line == end.line {
            let line = self.buffer.lines[start.line].text();
            out.push_str_trunc(&line[start.index..end.index]);
            return out;
        }
        let first = &self.buffer.lines[start.line];
        out.push_str_trunc(&first.text()[start.index..]);
        out.push_str_trunc(first.ending().as_str());
        for li in (start.line + 1)..end.line {
            let mid = &self.buffer.lines[li];
            out.push_str_trunc(mid.text());
            out.push_str_trunc(mid.ending().as_str());
        }
        let last = &self.buffer.lines[end.line];
        out.push_str_trunc(&last.text()[..end.index]);
        out
    }

    /// 内部底层删除（不自动提交历史，仅在存在 `self.change` 时追加 `ChangeItem`）。
    fn delete_range_raw(&mut self, start: Cursor, end: Cursor) {
        let s = self.clamp_cursor(start);
        let e = self.clamp_cursor(end);
        let (start, end) = if s <= e { (s, e) } else { (e, s) };
        if start == end {
            return;
        }

        if self.change.is_some() {
            let deleted = self.extract_range(start, end);
            if let Some(chg) = &mut self.change {
                let _ = chg.now.push(ChangeItem {
                    start,
                    end,
                    text: deleted,
                    insert: false,
                });
            }
        }

        if start.line == end.line {
            self.buffer.lines.as_mut_slice()[start.line].replace_range(start.index, end.index, "");
        } else {
            // 截取尾行后缀并拼接到首行前缀，然后移除中间所有行
            let tail = self.buffer.lines.as_mut_slice()[end.line].split_off(end.index);
            let first = &mut self.buffer.lines.as_mut_slice()[start.line];
            let first_len = first.text().len();
            first.replace_range(start.index, first_len, "");
            first.append(&tail);
            for _ in (start.line + 1)..=end.line {
                let _ = self.buffer.lines.remove(start.line + 1);
            }
        }
        self.cursor = start;
        self.buffer.set_redraw(true);
    }

    /// 内部底层插入（支持单行与多行字符串，不自动提交历史，仅在存在 `self.change` 时追加 `ChangeItem`）。
    fn insert_at_raw(
        &mut self,
        cursor: Cursor,
        data: &str,
        attrs_list: Option<AttrsList>,
    ) -> Cursor {
        let start = self.clamp_cursor(cursor);
        if data.is_empty() {
            return start;
        }

        let default_attrs = attrs_list
            .as_ref()
            .map(|a| a.defaults())
            .unwrap_or_else(|| self.buffer.lines[start.line].attrs_list().defaults());

        let mut parts: SmallVec<(&str, LineEnding), 16> = SmallVec::new();
        for pair in split_lines_with_ending(data) {
            let _ = parts.push(pair);
        }

        let end = if parts.len() <= 1 && !parts.first().is_some_and(|p| p.1.has_ending()) {
            let piece = parts.first().map_or(data, |p| p.0);
            let inserted = self.buffer.lines.as_mut_slice()[start.line].replace_range(
                start.index,
                start.index,
                piece,
            );
            Cursor::new(start.line, start.index + inserted)
        } else {
            // 多行插入：先在 start.index 处把当前行拆分为前段与尾段
            let mut tail_line = self.buffer.lines.as_mut_slice()[start.line].split_off(start.index);
            let last_idx = parts.len().saturating_sub(1);

            // 首段拼到 start.line 行尾，并设置首段自带的 LineEnding
            let (first_text, first_end) = parts[0];
            let first_line = &mut self.buffer.lines.as_mut_slice()[start.line];
            let flen = first_line.text().len();
            first_line.replace_range(flen, flen, first_text);
            first_line.set_ending(if first_end.has_ending() {
                first_end
            } else {
                LineEnding::Lf
            });

            // 中间行依次插入
            let mut insert_line_i = start.line + 1;
            for &(mid_text, mid_end) in &parts.as_slice()[1..last_idx] {
                let eff_end = if mid_end.has_ending() {
                    mid_end
                } else {
                    LineEnding::Lf
                };
                let new_line = BufferLine::new(mid_text, eff_end, AttrsList::new(default_attrs));
                let _ = self.buffer.lines.insert(insert_line_i, new_line);
                insert_line_i += 1;
            }

            // 末段与原尾段合并为最后一行
            let (last_text, _last_end) = parts[last_idx];
            let mut last_line = BufferLine::new(last_text, LineEnding::None, AttrsList::new(default_attrs));
            let end_col = last_line.text().len();
            last_line.append(&tail_line);
            tail_line.reset_layout();
            let _ = self.buffer.lines.insert(insert_line_i, last_line);

            Cursor::new(insert_line_i, end_col)
        };

        if let Some(chg) = &mut self.change {
            let _ = chg.now.push(ChangeItem {
                start,
                end,
                text: SmallString::from_str_trunc(data),
                insert: true,
            });
        }
        self.cursor = end;
        self.buffer.set_redraw(true);
        end
    }
}

impl Edit for CosmicEditor {
    #[inline(always)]
    fn buffer_ref(&self) -> &TextDocBuffer {
        &self.buffer
    }

    #[inline(always)]
    fn buffer_mut(&mut self) -> &mut TextDocBuffer {
        &mut self.buffer
    }

    #[inline(always)]
    fn cursor(&self) -> Cursor {
        self.cursor
    }

    #[inline]
    fn set_cursor(&mut self, cursor: Cursor) {
        let clamped = self.clamp_cursor(cursor);
        if self.cursor != clamped {
            self.cursor = clamped;
            self.cursor_x_opt = None;
            self.cursor_moved = true;
            self.buffer.set_redraw(true);
        }
    }

    #[inline(always)]
    fn selection(&self) -> EditSelection {
        self.selection
    }

    #[inline]
    fn set_selection(&mut self, selection: EditSelection) {
        if self.selection != selection {
            self.selection = selection;
            self.buffer.set_redraw(true);
        }
    }

    fn selection_bounds(&self) -> Option<(Cursor, Cursor)> {
        let cur = self.clamp_cursor(self.cursor);
        match self.selection {
            EditSelection::None => None,
            EditSelection::Normal(anchor) => {
                let a = self.clamp_cursor(anchor);
                if a == cur {
                    None
                } else if a < cur {
                    Some((a, cur))
                } else {
                    Some((cur, a))
                }
            }
            EditSelection::Word(anchor) => {
                let (as_, ae) = self.word_bounds_at(anchor);
                let (cs, ce) = self.word_bounds_at(cur);
                let start = if as_ <= cs { as_ } else { cs };
                let end = if ae >= ce { ae } else { ce };
                if start < end {
                    Some((start, end))
                } else {
                    None
                }
            }
            EditSelection::Line(anchor) => {
                let a = self.clamp_cursor(anchor);
                let start_line = a.line.min(cur.line);
                let end_line = a.line.max(cur.line);
                let end_col = self.buffer.lines[end_line].text().len();
                Some((Cursor::new(start_line, 0), Cursor::new(end_line, end_col)))
            }
        }
    }

    #[inline(always)]
    fn auto_indent(&self) -> bool {
        self.auto_indent
    }

    #[inline]
    fn set_auto_indent(&mut self, auto_indent: bool) {
        self.auto_indent = auto_indent;
    }

    #[inline(always)]
    fn tab_width(&self) -> u16 {
        self.buffer.tab_width()
    }

    #[inline]
    fn set_tab_width(&mut self, tab_width: u16) {
        self.buffer.set_tab_width(tab_width);
    }

    #[inline]
    fn shape_as_needed(&mut self, prune: bool) {
        if self.cursor_moved {
            self.buffer.ensure_cursor_visible(self.cursor);
            self.cursor_moved = false;
        }
        self.buffer.shape_until_scroll(prune);
    }

    #[inline]
    fn delete_range(&mut self, start: Cursor, end: Cursor) {
        self.delete_range_raw(start, end);
    }

    #[inline]
    fn insert_at(
        &mut self,
        cursor: Cursor,
        data: &str,
        attrs_list: Option<AttrsList>,
    ) -> Cursor {
        self.insert_at_raw(cursor, data, attrs_list)
    }

    fn copy_selection(&self) -> Option<SmallString<1024>> {
        let (start, end) = self.selection_bounds()?;
        let extracted = self.extract_range(start, end);
        Some(SmallString::from_str_trunc(extracted.as_str()))
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_bounds() else {
            return false;
        };
        self.selection = EditSelection::None;
        self.delete_range_raw(start, end);
        self.cursor = start;
        self.cursor_x_opt = None;
        self.cursor_moved = true;
        true
    }

    fn apply_change(&mut self, change: &Change) -> bool {
        if change.is_empty() {
            return false;
        }
        for item in change.now.as_slice() {
            if item.insert {
                self.cursor = self.insert_at_raw(item.start, item.text.as_str(), None);
            } else {
                self.delete_range_raw(item.start, item.end);
                self.cursor = item.start;
            }
        }
        self.selection = EditSelection::None;
        self.cursor_x_opt = None;
        self.cursor_moved = true;
        self.buffer.set_redraw(true);
        true
    }

    #[inline]
    fn start_change(&mut self) {
        if self.change.is_none() {
            self.change = Some(Change::new());
        }
    }

    #[inline]
    fn finish_change(&mut self) -> Option<Change> {
        self.change.take().filter(|c| !c.is_empty())
    }

    fn insert_string(&mut self, data: &str, attrs_list: Option<AttrsList>) {
        let owns_tx = self.change.is_none();
        if owns_tx {
            self.start_change();
        }
        self.delete_selection();
        let cur = self.cursor;
        self.cursor = self.insert_at_raw(cur, data, attrs_list);
        self.cursor_x_opt = None;
        self.cursor_moved = true;
        self.buffer.ensure_cursor_visible(self.cursor);
        if owns_tx {
            let chg = self.finish_change();
            self.commit_undo_change(chg);
        }
    }

    fn select_all(&mut self) {
        if self.buffer.lines.is_empty() {
            return;
        }
        let last_line = self.buffer.lines.len() - 1;
        let last_col = self.buffer.lines[last_line].text().len();
        self.selection = EditSelection::Normal(Cursor::new(0, 0));
        self.cursor = Cursor::new_with_affinity(last_line, last_col, Affinity::Before);
        self.cursor_x_opt = None;
        self.buffer.set_redraw(true);
    }

    fn cursor_position(&mut self) -> Option<(i32, i32)> {
        self.buffer
            .cursor_position(self.cursor)
            .map(|(x, y)| ((x + 0.5) as i32, (y + 0.5) as i32))
    }

    fn action(&mut self, action: Action) {
        match action {
            Action::Motion(motion) => {
                if let Some((next_c, next_x)) =
                    self.buffer.cursor_motion(self.cursor, self.cursor_x_opt, motion)
                {
                    self.cursor = next_c;
                    self.cursor_x_opt = next_x;
                    self.cursor_moved = true;
                    self.buffer.ensure_cursor_visible(self.cursor);
                    self.buffer.set_redraw(true);
                }
            }
            Action::Escape => {
                if !matches!(self.selection, EditSelection::None) {
                    self.selection = EditSelection::None;
                    self.buffer.set_redraw(true);
                }
            }
            Action::Insert(ch) => {
                if ch == '\n' || ch == '\r' {
                    self.action(Action::Enter);
                    return;
                }
                let owns_tx = self.change.is_none();
                if owns_tx {
                    self.start_change();
                }

                // 1. 若无选区且开启自动闭合：输入紧邻的右定界符时直接越过而不重复插入
                let had_sel = self.selection_bounds().is_some();
                if !had_sel && self.auto_close {
                    let cur = self.clamp_cursor(self.cursor);
                    let rest = &self.buffer.lines[cur.line].text()[cur.index..];
                    if matches!(ch, ')' | ']' | '}' | '"' | '\'') && rest.starts_with(ch) {
                        self.cursor = Cursor::new(cur.line, cur.index + ch.len_utf8());
                        self.cursor_x_opt = None;
                        self.cursor_moved = true;
                        self.buffer.ensure_cursor_visible(self.cursor);
                        self.buffer.set_redraw(true);
                        if owns_tx {
                            let _ = self.finish_change();
                        }
                        return;
                    }
                }

                self.delete_selection();

                // 2. 若输入左定界符且开启自动闭合，同时成对插入并将光标落在中间
                if !had_sel && self.auto_close {
                    if let Some(close_ch) = AutoClosedPairs::<16>::closing_char_for(ch) {
                        let mut buf = [0u8; 8];
                        let o_len = ch.encode_utf8(&mut buf).len();
                        let c_len = close_ch.encode_utf8(&mut buf[o_len..]).len();
                        let pair_s =
                            unsafe { core::str::from_utf8_unchecked(&buf[..o_len + c_len]) };
                        let cur = self.cursor;
                        let _ = self.insert_at_raw(cur, pair_s, None);
                        self.cursor = Cursor::new(cur.line, cur.index + o_len);
                        self.cursor_x_opt = None;
                        self.cursor_moved = true;
                        self.buffer.ensure_cursor_visible(self.cursor);
                        if owns_tx {
                            let chg = self.finish_change();
                            self.commit_undo_change(chg);
                        }
                        return;
                    }
                }

                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                let cur = self.cursor;
                self.cursor = self.insert_at_raw(cur, s, None);
                self.cursor_x_opt = None;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                if owns_tx {
                    let chg = self.finish_change();
                    self.commit_undo_change(chg);
                }
            }
            Action::Enter => {
                let owns_tx = self.change.is_none();
                if owns_tx {
                    self.start_change();
                }
                self.delete_selection();
                let cur = self.clamp_cursor(self.cursor);
                let mut ins = SmallString::<64>::from_str_trunc("\n");

                if self.auto_indent {
                    let line_prefix = &self.buffer.lines[cur.line].text()[..cur.index];
                    // 继承当前行前导空白
                    for b in line_prefix.bytes() {
                        if b == b' ' || b == b'\t' {
                            let _ = ins.push(b as char);
                        } else {
                            break;
                        }
                    }
                    // 若行尾为 `{`、`(`、`[`、`:` 则额外增加一级缩进
                    let open_block = line_prefix
                        .trim_end()
                        .chars()
                        .next_back()
                        .is_some_and(|c| matches!(c, '{' | '(' | '[' | ':'));
                    if open_block {
                        if self.hard_tabs {
                            let _ = ins.push('\t');
                        } else {
                            let tw = (self.buffer.tab_width() as usize).clamp(1, 8);
                            for _ in 0..tw {
                                let _ = ins.push(' ');
                            }
                        }
                    }
                }

                self.cursor = self.insert_at_raw(cur, ins.as_str(), None);
                self.cursor_x_opt = None;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                if owns_tx {
                    let chg = self.finish_change();
                    self.commit_undo_change(chg);
                }
            }
            Action::Backspace => {
                let owns_tx = self.change.is_none();
                if owns_tx {
                    self.start_change();
                }
                if !self.delete_selection() {
                    let cur = self.clamp_cursor(self.cursor);
                    // 若位于成对空括号/引号中间，退格同时移除左右定界符
                    let mut paired_deleted = false;
                    if self.auto_close && cur.index > 0 {
                        let text = self.buffer.lines[cur.line].text();
                        if let (Some((prev_i, prev_ch)), Some(next_ch)) = (
                            text[..cur.index].char_indices().next_back(),
                            text[cur.index..].chars().next(),
                        ) {
                            if AutoClosedPairs::<16>::closing_char_for(prev_ch) == Some(next_ch) {
                                let start = Cursor::new(cur.line, prev_i);
                                let end = Cursor::new(cur.line, cur.index + next_ch.len_utf8());
                                self.delete_range_raw(start, end);
                                paired_deleted = true;
                            }
                        }
                    }
                    if !paired_deleted {
                        if cur.index > 0 {
                            let prev_i = self.buffer.lines[cur.line].text()[..cur.index]
                                .char_indices()
                                .next_back()
                                .map_or(0, |(i, _)| i);
                            self.delete_range_raw(Cursor::new(cur.line, prev_i), cur);
                        } else if cur.line > 0 {
                            let prev_line = cur.line - 1;
                            let prev_end = self.buffer.lines[prev_line].text().len();
                            self.delete_range_raw(Cursor::new(prev_line, prev_end), cur);
                        }
                    }
                }
                self.cursor_x_opt = None;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                if owns_tx {
                    let chg = self.finish_change();
                    self.commit_undo_change(chg);
                }
            }
            Action::Delete => {
                let owns_tx = self.change.is_none();
                if owns_tx {
                    self.start_change();
                }
                if !self.delete_selection() {
                    let cur = self.clamp_cursor(self.cursor);
                    let line_len = self.buffer.lines[cur.line].text().len();
                    if cur.index < line_len {
                        let next_i = self.buffer.lines[cur.line].text()[cur.index..]
                            .chars()
                            .next()
                            .map_or(line_len, |c| cur.index + c.len_utf8());
                        self.delete_range_raw(cur, Cursor::new(cur.line, next_i));
                    } else if cur.line + 1 < self.buffer.lines.len() {
                        self.delete_range_raw(cur, Cursor::new(cur.line + 1, 0));
                    }
                }
                self.cursor_x_opt = None;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                if owns_tx {
                    let chg = self.finish_change();
                    self.commit_undo_change(chg);
                }
            }
            Action::Indent => {
                let owns_tx = self.change.is_none();
                if owns_tx {
                    self.start_change();
                }
                let mut indent_str = SmallString::<8>::new();
                if self.hard_tabs {
                    let _ = indent_str.push('\t');
                } else {
                    let tw = (self.buffer.tab_width() as usize).clamp(1, 8);
                    for _ in 0..tw {
                        let _ = indent_str.push(' ');
                    }
                }
                let delta = indent_str.len();

                if let Some((sel_s, sel_e)) = self.selection_bounds() {
                    let first_line = sel_s.line;
                    let last_line = if sel_e.line > sel_s.line && sel_e.index == 0 {
                        sel_e.line - 1
                    } else {
                        sel_e.line
                    };
                    for li in first_line..=last_line {
                        let _ = self.insert_at_raw(Cursor::new(li, 0), indent_str.as_str(), None);
                    }
                    self.selection = EditSelection::Normal(Cursor::new(sel_s.line, sel_s.index + delta));
                    self.cursor = Cursor::new(sel_e.line, sel_e.index + if sel_e.line <= last_line { delta } else { 0 });
                } else {
                    let cur = self.clamp_cursor(self.cursor);
                    let _ = self.insert_at_raw(Cursor::new(cur.line, 0), indent_str.as_str(), None);
                    self.cursor = Cursor::new(cur.line, cur.index + delta);
                }
                self.cursor_x_opt = None;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                if owns_tx {
                    let chg = self.finish_change();
                    self.commit_undo_change(chg);
                }
            }
            Action::Unindent => {
                let owns_tx = self.change.is_none();
                if owns_tx {
                    self.start_change();
                }
                let tw = (self.buffer.tab_width() as usize).clamp(1, 8);
                let (first_line, last_line, old_sel) = if let Some((s, e)) = self.selection_bounds()
                {
                    let ll = if e.line > s.line && e.index == 0 {
                        e.line - 1
                    } else {
                        e.line
                    };
                    (s.line, ll, Some((s, e)))
                } else {
                    let l = self.clamp_cursor(self.cursor).line;
                    (l, l, None)
                };

                let mut first_removed = 0usize;
                let mut last_removed = 0usize;
                let orig_cur = self.clamp_cursor(self.cursor);

                for li in first_line..=last_line {
                    let text = self.buffer.lines[li].text();
                    let rem = if text.starts_with('\t') {
                        1
                    } else {
                        text.bytes().take(tw).take_while(|&b| b == b' ').count()
                    };
                    if rem > 0 {
                        self.delete_range_raw(Cursor::new(li, 0), Cursor::new(li, rem));
                    }
                    if li == first_line {
                        first_removed = rem;
                    }
                    if li == last_line {
                        last_removed = rem;
                    }
                }

                if let Some((s, e)) = old_sel {
                    self.selection = EditSelection::Normal(Cursor::new(
                        s.line,
                        s.index.saturating_sub(first_removed),
                    ));
                    self.cursor = Cursor::new(
                        e.line,
                        if e.line <= last_line {
                            e.index.saturating_sub(last_removed)
                        } else {
                            e.index
                        },
                    );
                } else {
                    self.cursor = Cursor::new(
                        orig_cur.line,
                        orig_cur.index.saturating_sub(first_removed),
                    );
                }
                self.cursor_x_opt = None;
                self.cursor_moved = true;
                self.buffer.ensure_cursor_visible(self.cursor);
                if owns_tx {
                    let chg = self.finish_change();
                    self.commit_undo_change(chg);
                }
            }
            Action::Click { x, y } => {
                self.selection = EditSelection::None;
                if let Some(c) = self.buffer.hit(x as f32, y as f32) {
                    self.cursor = c;
                    self.cursor_x_opt = None;
                    self.cursor_moved = true;
                    self.buffer.set_redraw(true);
                }
            }
            Action::DoubleClick { x, y } => {
                if let Some(c) = self.buffer.hit(x as f32, y as f32) {
                    self.selection = EditSelection::Word(c);
                    self.cursor = c;
                    self.cursor_x_opt = None;
                    self.cursor_moved = true;
                    self.buffer.set_redraw(true);
                }
            }
            Action::TripleClick { x, y } => {
                if let Some(c) = self.buffer.hit(x as f32, y as f32) {
                    let end_col = self.buffer.lines[c.line].text().len();
                    self.selection = EditSelection::Line(Cursor::new(c.line, 0));
                    self.cursor = Cursor::new(c.line, end_col);
                    self.cursor_x_opt = None;
                    self.cursor_moved = true;
                    self.buffer.set_redraw(true);
                }
            }
            Action::Drag { x, y } => {
                if matches!(self.selection, EditSelection::None) {
                    self.selection = EditSelection::Normal(self.cursor);
                }
                if let Some(c) = self.buffer.hit(x as f32, y as f32) {
                    self.cursor = c;
                    self.cursor_x_opt = None;
                    self.cursor_moved = true;
                    self.buffer.set_redraw(true);
                }
            }
            Action::Scroll { lines } => {
                let lh = self.buffer.metrics().line_height.max(1.0);
                self.buffer.scroll_vertical_pixels((lines as f32) * lh);
            }
            Action::ScrollPixels { pixels } => {
                self.buffer.scroll_vertical_pixels(pixels);
            }
        }
    }
}
