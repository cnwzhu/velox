//! 基础单行文本输入框组件 `TextInput`（支持文本状态、闪烁叠层光标、亚像素鼠标任意选区、
//! 双击选词/三击全选、修饰键快捷键、系统剪贴板复制粘贴、右键菜单回调与密码掩码模式）。

use velox_engine::display::measure_line_width_f32;
use velox_engine::prelude::*;

use core::cell::RefCell;
use std::collections::HashSet;

std::thread_local! {
    /// 非受控密码框（开启 `.password(true)` 或 `.password_toggle(true)` 且未提供 `on_toggle_mask` 时）的明文显示开关缓存。
    static PASSWORD_REVEALED_IDS: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
}

#[inline]
fn is_password_revealed(id: u32) -> bool {
    PASSWORD_REVEALED_IDS.with(|s| s.borrow().contains(&id))
}

#[inline]
fn toggle_password_revealed(id: u32) {
    PASSWORD_REVEALED_IDS.with(|s| {
        let mut set = s.borrow_mut();
        if !set.remove(&id) {
            set.insert(id);
        }
    });
}

const EYE_OPEN_SVG: &str =
    "M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7ZM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z";
const EYE_OFF_SVG: &str =
    "M9.88 9.88a3 3 0 1 0 4.24 4.24M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61M2 2l20 20";

const AUTO_COLOR: Color = Color(0x0000_0001);
const DOUBLE_CLICK_MS: u32 = 380;
const DOUBLE_CLICK_SLOP_PX: i16 = 6;
/// 标准光标闪烁半周期（毫秒，530ms 亮 / 530ms 灭）。
pub const CURSOR_BLINK_HALF_PERIOD_MS: u32 = 530;

#[inline(always)]
fn round_nonneg_i16(v: f32) -> i16 {
    if v <= 0.0 {
        0
    } else {
        let n = (v + 0.5) as i32;
        n.min(i16::MAX as i32) as i16
    }
}

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

/// 面向 [`TextInput`] 的内联无堆分配可编辑文本缓冲区（同时支持 `#![no_std]` 与 `std`，
/// 支持亚像素点击/拖拽任意选区、双击选词、三击全选、IME 预编辑、系统剪贴板读写与光标闪烁复位）。
#[derive(Clone, Debug)]
pub struct EditableText<const CAP: usize = 64> {
    bytes: [u8; CAP],
    len: usize,
    /// 光标所在的 UTF-8 字节偏移量（`0..=len`）。
    cursor: usize,
    /// 选区锚点字节偏移量（`Some(anchor)` 且 `anchor != cursor` 时表示存在活动选区）。
    selection_anchor: Option<usize>,
    /// 输入法（IME）预编辑组合文本内联缓冲。
    preedit_bytes: [u8; 32],
    preedit_len: u8,
    /// 最近一次交互时间戳（毫秒），用于在键入/点击后立即复位光标为常亮状态。
    blink_epoch_ms: u32,
    /// 鼠标连击检测状态（单击 / 双击选词 / 三击全选）。
    last_click_ms: u32,
    last_click_x: i16,
    click_streak: u8,
    /// 双击后延续按词拖拽选区时的初始单词范围。
    drag_word_anchor: Option<(usize, usize)>,
}

impl<const CAP: usize> Default for EditableText<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> EditableText<CAP> {
    /// 创建空的可编辑文本缓冲区。
    pub const fn new() -> Self {
        Self {
            bytes: [0u8; CAP],
            len: 0,
            cursor: 0,
            selection_anchor: None,
            preedit_bytes: [0u8; 32],
            preedit_len: 0,
            blink_epoch_ms: 0,
            last_click_ms: 0,
            last_click_x: 0,
            click_streak: 0,
            drag_word_anchor: None,
        }
    }

    /// 使用初始字符串切片创建可编辑文本缓冲区，光标默认置于末尾。
    pub fn from_str(initial: &str) -> Self {
        let mut s = Self::new();
        s.set(initial);
        s
    }

    /// 获取最近一次交互的光标闪烁复位时间戳（毫秒）。
    #[inline(always)]
    pub const fn blink_epoch_ms(&self) -> u32 {
        self.blink_epoch_ms
    }

    /// 显式复位光标闪烁相位，使光标立即以常亮状态显示。
    #[inline(always)]
    pub fn reset_blink(&mut self, now_ms: u32) {
        self.blink_epoch_ms = now_ms;
    }

    /// 获取当前完整文本切片 `&str`。
    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }

    /// 将当前文本零堆分配复制到本帧 [`ScratchArena`] 中并返回 `&'a str`（便于在 `View::render` 中直接传给 [`TextInput`]）。
    #[inline]
    pub fn as_arena_str<'a>(&self, cx: &'a FrameScope) -> &'a str {
        let bytes = cx.arena().alloc_bytes(self.len);
        bytes.copy_from_slice(&self.bytes[..self.len]);
        unsafe { core::str::from_utf8_unchecked(bytes) }
    }

    /// 获取光标前的文本切片。
    #[inline]
    pub fn before_cursor(&self) -> &str {
        let c = self.cursor.min(self.len);
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..c]) }
    }

    /// 获取光标后的文本切片。
    #[inline]
    pub fn after_cursor(&self) -> &str {
        let c = self.cursor.min(self.len);
        unsafe { core::str::from_utf8_unchecked(&self.bytes[c..self.len]) }
    }

    /// 获取当前光标字节偏移量。
    #[inline(always)]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// 当前文本是否为空。
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 当前文本 UTF-8 字节长度。
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 整体替换文本内容并将光标移至末尾。
    pub fn set(&mut self, text: &str) {
        let mut valid_len = 0;
        for ch in text.chars() {
            if valid_len + ch.len_utf8() <= CAP {
                valid_len += ch.len_utf8();
            } else {
                break;
            }
        }
        self.bytes[..valid_len].copy_from_slice(&text.as_bytes()[..valid_len]);
        self.len = valid_len;
        self.cursor = valid_len;
        self.selection_anchor = None;
        self.preedit_len = 0;
        self.drag_word_anchor = None;
    }

    /// 清空文本与光标。
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
        self.selection_anchor = None;
        self.preedit_len = 0;
        self.drag_word_anchor = None;
    }

    /// 全选当前文本。
    #[inline]
    pub fn select_all(&mut self) {
        if self.len > 0 {
            self.selection_anchor = Some(0);
            self.cursor = self.len;
        }
    }

    /// 清除当前选区。
    #[inline]
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.drag_word_anchor = None;
    }

    /// 显式设置选区范围 `(anchor, cursor)`（自动对齐至合法 UTF-8 字符边界）。
    pub fn set_selection(&mut self, anchor: usize, cursor: usize) {
        let s = self.as_str();
        let a = anchor.min(self.len);
        let c = cursor.min(self.len);
        if s.is_char_boundary(a) && s.is_char_boundary(c) {
            self.selection_anchor = if a != c { Some(a) } else { None };
            self.cursor = c;
        }
    }

    /// 获取当前选区的起止字节范围 `Some((start, end))`（满足 `start < end`）。
    #[inline]
    pub fn selected_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?.min(self.len);
        let cur = self.cursor.min(self.len);
        if anchor == cur {
            None
        } else if anchor < cur {
            Some((anchor, cur))
        } else {
            Some((cur, anchor))
        }
    }

    /// 获取当前选中的文本切片 `&str`（无选区时返回 `""`）。
    #[inline]
    pub fn selected_text(&self) -> &str {
        if let Some((start, end)) = self.selected_range() {
            let s = self.as_str();
            if s.is_char_boundary(start) && s.is_char_boundary(end) {
                return &s[start..end];
            }
        }
        ""
    }

    /// 删除当前选中的文本范围，返回是否实际删除了选区。
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selected_range() else {
            self.selection_anchor = None;
            return false;
        };
        let removed = end - start;
        self.bytes.copy_within(end..self.len, start);
        self.len -= removed;
        self.cursor = start;
        self.selection_anchor = None;
        self.drag_word_anchor = None;
        true
    }

    /// 批量插入字符串（用于粘贴 `paste` 或输入法整词上屏），返回实际插入的字节数。
    pub fn insert_str(&mut self, text: &str) -> usize {
        self.delete_selection();
        let mut inserted = 0usize;
        for ch in text.chars() {
            if self.insert_char(ch) {
                inserted += ch.len_utf8();
            } else {
                break;
            }
        }
        inserted
    }

    /// 将单行安全规范化后的剪贴板文本粘贴至光标处（换行符转换为空格，若有选区则替换选区）。
    pub fn paste_from_clipboard(&mut self, now_ms: u32) -> bool {
        let clip = clipboard_read_into::<256>();
        let raw = clip.as_str();
        if raw.is_empty() {
            return false;
        }
        self.delete_selection();
        let mut inserted = 0usize;
        let mut prev_cr = false;
        for ch in raw.chars() {
            let mapped = match ch {
                '\r' => {
                    prev_cr = true;
                    ' '
                }
                '\n' => {
                    if prev_cr {
                        prev_cr = false;
                        continue;
                    }
                    ' '
                }
                '\t' => {
                    prev_cr = false;
                    ' '
                }
                c if c.is_control() => {
                    prev_cr = false;
                    continue;
                }
                c => {
                    prev_cr = false;
                    c
                }
            };
            if self.insert_char(mapped) {
                inserted += mapped.len_utf8();
            } else {
                break;
            }
        }
        self.blink_epoch_ms = now_ms;
        inserted > 0
    }

    /// 将当前选区复制到系统剪贴板（密码掩码模式下禁止导出明文，返回 `false`）。
    pub fn copy_to_clipboard(&self, masked: bool) -> bool {
        if masked {
            return false;
        }
        let sel = self.selected_text();
        if sel.is_empty() {
            return false;
        }
        clipboard_write_text(sel)
    }

    /// 将当前选区剪切到系统剪贴板（密码掩码模式下禁止导出明文，返回 `false`）。
    pub fn cut_to_clipboard(&mut self, masked: bool, now_ms: u32) -> bool {
        if masked {
            return false;
        }
        let sel = self.selected_text();
        if sel.is_empty() {
            return false;
        }
        let ok = clipboard_write_text(sel);
        if ok {
            let _ = self.delete_selection();
            self.blink_epoch_ms = now_ms;
        }
        ok
    }

    /// 设置输入法（IME）预编辑组合串（`preedit`）。
    pub fn set_preedit(&mut self, preedit: &str) {
        let mut valid_len = 0usize;
        for ch in preedit.chars() {
            if valid_len + ch.len_utf8() <= self.preedit_bytes.len() {
                valid_len += ch.len_utf8();
            } else {
                break;
            }
        }
        self.preedit_bytes[..valid_len].copy_from_slice(&preedit.as_bytes()[..valid_len]);
        self.preedit_len = valid_len as u8;
    }

    /// 获取当前输入法（IME）预编辑字符串切片。
    #[inline]
    pub fn preedit_str(&self) -> &str {
        unsafe {
            core::str::from_utf8_unchecked(&self.preedit_bytes[..self.preedit_len as usize])
        }
    }

    /// 清空输入法（IME）预编辑状态。
    #[inline]
    pub fn clear_preedit(&mut self) {
        self.preedit_len = 0;
    }

    /// 将当前输入法（IME）预编辑组合串正式提交插入到光标处。
    pub fn commit_preedit(&mut self) -> bool {
        let plen = self.preedit_len as usize;
        if plen == 0 {
            return false;
        }
        let mut tmp = [0u8; 32];
        tmp[..plen].copy_from_slice(&self.preedit_bytes[..plen]);
        self.preedit_len = 0;
        let s = unsafe { core::str::from_utf8_unchecked(&tmp[..plen]) };
        self.insert_str(s) > 0
    }

    /// 测量当前光标位置相对于文本起始处的水平像素偏移量（与渲染器精确浮点步进及字偶距一致）。
    pub fn cursor_pixel_offset(&self, font_size: u8) -> i16 {
        round_nonneg_i16(measure_line_width_f32(self.before_cursor(), font_size))
    }

    /// 根据相对于文本起始处的水平点击坐标 `local_x`（像素）反查最近的 UTF-8 字符边界字节偏移。
    pub fn hit_test_cursor_byte(&self, local_x: i16, font_size: u8) -> usize {
        self.hit_test_cursor_byte_masked(local_x, font_size, false)
    }

    /// 支持密码掩码模式（`masked == true` 时按 `•` 浮点字符宽度反查）的精确水平像素命中测试。
    pub fn hit_test_cursor_byte_masked(&self, local_x: i16, font_size: u8, masked: bool) -> usize {
        if local_x <= 0 {
            return 0;
        }
        let target_x = local_x as f32;
        let s = self.as_str();
        if masked {
            let mut mask_buf = [0u8; 4];
            let mask_str = crate::input::MASK_CHAR.encode_utf8(&mut mask_buf);
            let dot_w = measure_line_width_f32(mask_str, font_size).max(4.0);
            let mut char_i = 0usize;
            for (byte_idx, _ch) in s.char_indices() {
                let prev_x = (char_i as f32) * dot_w;
                let mid_x = prev_x + dot_w * 0.5;
                if target_x < mid_x {
                    return byte_idx;
                }
                char_i += 1;
            }
            return self.len;
        }

        let mut prev_x: f32 = 0.0;
        for (byte_idx, ch) in s.char_indices() {
            let next_byte = byte_idx + ch.len_utf8();
            let next_x = measure_line_width_f32(&s[..next_byte], font_size);
            let mid_x = 0.5 * (prev_x + next_x);
            if target_x < mid_x {
                return byte_idx;
            }
            prev_x = next_x;
        }
        self.len
    }

    /// 根据点击像素偏移 `local_x` 将光标定位到对应字符边界，并清除选区。
    pub fn set_cursor_from_pixel(&mut self, local_x: i16, font_size: u8) -> usize {
        let pos = self.hit_test_cursor_byte(local_x, font_size);
        self.cursor = pos;
        self.selection_anchor = None;
        self.drag_word_anchor = None;
        pos
    }

    /// 处理鼠标左键按下：自动支持单击精确落点、双击选词、三击全选与 `Shift+Click` 扩展选区。
    pub fn handle_pointer_press(
        &mut self,
        local_x: i16,
        now_ms: u32,
        shift: bool,
        masked: bool,
        font_size: u8,
    ) {
        self.blink_epoch_ms = now_ms;
        let hit = self.hit_test_cursor_byte_masked(local_x, font_size, masked);
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = hit;
            if self.selection_anchor == Some(self.cursor) {
                self.selection_anchor = None;
            }
            self.click_streak = 0;
            self.drag_word_anchor = None;
            return;
        }

        let dt = now_ms.wrapping_sub(self.last_click_ms);
        let dx = (local_x - self.last_click_x).abs();
        if self.click_streak > 0 && dt <= DOUBLE_CLICK_MS && dx <= DOUBLE_CLICK_SLOP_PX {
            self.click_streak = (self.click_streak % 3) + 1;
        } else {
            self.click_streak = 1;
        }
        self.last_click_ms = now_ms;
        self.last_click_x = local_x;

        match self.click_streak {
            2 => {
                if masked {
                    self.select_all();
                    self.drag_word_anchor = None;
                } else if self.select_word_near(hit) {
                    self.drag_word_anchor = self.selected_range();
                } else {
                    self.cursor = hit;
                    self.selection_anchor = None;
                    self.drag_word_anchor = None;
                }
            }
            3 => {
                self.select_all();
                self.drag_word_anchor = None;
            }
            _ => {
                self.cursor = hit;
                self.selection_anchor = None;
                self.drag_word_anchor = None;
            }
        }
    }

    /// 处理鼠标按住拖拽：支持任意逐字符框选，并在双击后平滑按整词扩展选区。
    pub fn handle_pointer_drag(
        &mut self,
        local_x: i16,
        now_ms: u32,
        masked: bool,
        font_size: u8,
    ) {
        self.blink_epoch_ms = now_ms;
        let hit = self.hit_test_cursor_byte_masked(local_x, font_size, masked);
        if let Some((aw_s, aw_e)) = self.drag_word_anchor {
            let probe = if hit >= self.len && self.len > 0 {
                self.as_str()
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i)
            } else {
                hit
            };
            let (cw_s, cw_e) = crate::input::word_range_at(self.as_str(), probe)
                .map_or((hit, hit), |r| (r.start, r.end));
            if cw_s < aw_s {
                self.selection_anchor = Some(aw_e);
                self.cursor = cw_s;
            } else {
                self.selection_anchor = Some(aw_s);
                self.cursor = cw_e.max(aw_e);
            }
            return;
        }

        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.cursor = hit;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
    }

    /// 便捷入口：直接消费 [`TextInput::on_pointer_action`] 传回的 `(local_x, is_drag, now_ms, shift)`。
    #[inline]
    pub fn handle_pointer_action(
        &mut self,
        local_x: i16,
        is_drag: bool,
        now_ms: u32,
        shift: bool,
        masked: bool,
        font_size: u8,
    ) {
        if is_drag {
            self.handle_pointer_drag(local_x, now_ms, masked, font_size);
        } else {
            self.handle_pointer_press(local_x, now_ms, shift, masked, font_size);
        }
    }

    /// 在当前光标位置插入单个字符 `ch`（若存在活动选区则先替换选区）。
    pub fn insert_char(&mut self, ch: char) -> bool {
        if ch.is_control() {
            return false;
        }
        self.delete_selection();
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf).as_bytes();
        let clen = encoded.len();
        if self.len + clen > CAP {
            return false;
        }
        let c = self.cursor.min(self.len);
        self.bytes.copy_within(c..self.len, c + clen);
        self.bytes[c..c + clen].copy_from_slice(encoded);
        self.len += clen;
        self.cursor = c + clen;
        true
    }

    /// 删除光标前的一个字符（`Backspace`；若存在选区则删除整个选区）。
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let c = self.cursor.min(self.len);
        if c == 0 {
            return false;
        }
        let prev = self
            .before_cursor()
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        let removed = c - prev;
        self.bytes.copy_within(c..self.len, prev);
        self.len -= removed;
        self.cursor = prev;
        true
    }

    /// 删除光标后的一个字符（`Delete`；若存在选区则删除整个选区）。
    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let c = self.cursor.min(self.len);
        if c >= self.len {
            return false;
        }
        let next_ch_len = self
            .after_cursor()
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(0);
        if next_ch_len == 0 {
            return false;
        }
        self.bytes.copy_within(c + next_ch_len..self.len, c);
        self.len -= next_ch_len;
        true
    }

    /// 将光标向左移动一个字符（清除选区）。
    pub fn move_left(&mut self) -> bool {
        if let Some((start, _)) = self.selected_range() {
            self.cursor = start;
            self.selection_anchor = None;
            return true;
        }
        self.selection_anchor = None;
        let c = self.cursor.min(self.len);
        if c == 0 {
            return false;
        }
        let prev = self
            .before_cursor()
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.cursor = prev;
        true
    }

    /// 将光标向右移动一个字符（清除选区）。
    pub fn move_right(&mut self) -> bool {
        if let Some((_, end)) = self.selected_range() {
            self.cursor = end;
            self.selection_anchor = None;
            return true;
        }
        self.selection_anchor = None;
        let c = self.cursor.min(self.len);
        if c >= self.len {
            return false;
        }
        let step = self
            .after_cursor()
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(0);
        self.cursor = c + step;
        true
    }

    /// 按住 Shift 向左扩展或收缩选区。
    pub fn move_left_select(&mut self) -> bool {
        let c = self.cursor.min(self.len);
        if c == 0 {
            return false;
        }
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(c);
        }
        let prev = self
            .before_cursor()
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.cursor = prev;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
        true
    }

    /// 按住 Shift 向右扩展或收缩选区。
    pub fn move_right_select(&mut self) -> bool {
        let c = self.cursor.min(self.len);
        if c >= self.len {
            return false;
        }
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(c);
        }
        let step = self
            .after_cursor()
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(0);
        self.cursor = c + step;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
        true
    }

    /// 将光标向左移动一个单词边界（清除选区）。
    pub fn move_word_left(&mut self) -> bool {
        self.selection_anchor = None;
        let next = crate::input::prev_word_boundary(self.as_str(), self.cursor);
        if next != self.cursor {
            self.cursor = next;
            true
        } else {
            false
        }
    }

    /// 将光标向右移动一个单词边界（清除选区）。
    pub fn move_word_right(&mut self) -> bool {
        self.selection_anchor = None;
        let next = crate::input::next_word_boundary(self.as_str(), self.cursor);
        if next != self.cursor {
            self.cursor = next;
            true
        } else {
            false
        }
    }

    /// 按住 Ctrl+Shift 向左按词扩选。
    pub fn move_word_left_select(&mut self) -> bool {
        let next = crate::input::prev_word_boundary(self.as_str(), self.cursor);
        if next == self.cursor {
            return false;
        }
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.cursor = next;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
        true
    }

    /// 按住 Ctrl+Shift 向右按词扩选。
    pub fn move_word_right_select(&mut self) -> bool {
        let next = crate::input::next_word_boundary(self.as_str(), self.cursor);
        if next == self.cursor {
            return false;
        }
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.cursor = next;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
        true
    }

    /// 双击选中当前光标或指定字节偏移处的整个单词。
    pub fn select_word(&mut self, offset: usize) -> bool {
        if let Some(range) = crate::input::word_range_at(self.as_str(), offset) {
            if range.start < range.end {
                self.selection_anchor = Some(range.start);
                self.cursor = range.end;
                return true;
            }
        }
        false
    }

    fn select_word_near(&mut self, offset: usize) -> bool {
        if self.len == 0 {
            return false;
        }
        let s = self.as_str();
        let probe = if offset >= self.len {
            s.char_indices().next_back().map_or(0, |(i, _)| i)
        } else {
            offset
        };
        self.select_word(probe)
    }

    /// 删除光标前的整个单词（`Ctrl+Backspace`）。
    pub fn delete_word_backward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let c = self.cursor.min(self.len);
        if c == 0 {
            return false;
        }
        let prev = crate::input::prev_word_boundary(self.as_str(), c);
        if prev < c {
            self.bytes.copy_within(c..self.len, prev);
            self.len -= c - prev;
            self.cursor = prev;
            true
        } else {
            false
        }
    }

    /// 处理带修饰键的完整键盘事件（支持 `Ctrl+A/C/X/V`、`Shift+方向键/Home/End`、`Ctrl+Left/Right/Backspace` 与 `Escape`）。
    pub fn handle_key_with_mods(
        &mut self,
        key: KeyCode,
        mods: KeyModifiers,
        masked: bool,
        now_ms: u32,
    ) -> bool {
        let ctrl_or_cmd = mods.ctrl() || mods.logo();
        let shift = mods.shift();

        if ctrl_or_cmd {
            let handled = match key {
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.select_all();
                    true
                }
                KeyCode::Char('c') | KeyCode::Char('C') => self.copy_to_clipboard(masked),
                KeyCode::Char('x') | KeyCode::Char('X') => self.cut_to_clipboard(masked, now_ms),
                KeyCode::Char('v') | KeyCode::Char('V') => self.paste_from_clipboard(now_ms),
                KeyCode::Left => {
                    if shift {
                        self.move_word_left_select()
                    } else {
                        self.move_word_left()
                    }
                }
                KeyCode::Right => {
                    if shift {
                        self.move_word_right_select()
                    } else {
                        self.move_word_right()
                    }
                }
                KeyCode::Backspace => self.delete_word_backward(),
                _ => false,
            };
            if handled {
                self.blink_epoch_ms = now_ms;
            }
            return handled;
        }

        let handled = match key {
            KeyCode::Left if shift => self.move_left_select(),
            KeyCode::Right if shift => self.move_right_select(),
            KeyCode::Home if shift => {
                if self.cursor > 0 {
                    if self.selection_anchor.is_none() {
                        self.selection_anchor = Some(self.cursor);
                    }
                    self.cursor = 0;
                    if self.selection_anchor == Some(0) {
                        self.selection_anchor = None;
                    }
                    true
                } else {
                    false
                }
            }
            KeyCode::End if shift => {
                if self.cursor < self.len {
                    if self.selection_anchor.is_none() {
                        self.selection_anchor = Some(self.cursor);
                    }
                    self.cursor = self.len;
                    if self.selection_anchor == Some(self.len) {
                        self.selection_anchor = None;
                    }
                    true
                } else {
                    false
                }
            }
            KeyCode::Escape => {
                if self.selection_anchor.is_some() {
                    self.clear_selection();
                    true
                } else {
                    false
                }
            }
            other => self.handle_key(other),
        };
        if handled {
            self.blink_epoch_ms = now_ms;
        }
        handled
    }

    /// 处理引擎派发的 [`KeyCode`] 编辑按键，返回文本或光标是否发生变更。
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char(ch) => self.insert_char(ch),
            KeyCode::Space => self.insert_char(' '),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => {
                let had_sel = self.selection_anchor.take().is_some();
                if self.cursor != 0 || had_sel {
                    self.cursor = 0;
                    true
                } else {
                    false
                }
            }
            KeyCode::End => {
                let had_sel = self.selection_anchor.take().is_some();
                if self.cursor != self.len || had_sel {
                    self.cursor = self.len;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// `Input` 单行输入框组件类型别名（与 [`TextInput`] 等价）。
pub type Input<'a> = TextInput<'a>;

/// 无样式单行文本输入框行为原语（`TextInput`，别名 `Input`）。
///
/// 默认零背景、零边框、零圆角与零主题蓝焦环，专注文本显示、530ms 闪烁叠层光标、
/// 亚像素拖选、双击选词/三击全选、快捷键与右键剪贴板交互；预设主题外观由 `velox-component::TextInput` 提供。
pub struct TextInput<'a> {
    pub id: u32,
    pub value: &'a str,
    pub placeholder: &'a str,
    pub focused: bool,
    pub disabled: bool,
    /// 是否以密码掩码圆点（`•`）显示文本。
    pub masked: bool,
    /// 是否在输入框右侧显示密码可见性切换小眼睛按钮（点击在明文/掩码间切换）。
    pub password_toggle: bool,
    /// 可选的光标字节偏移位置（`None` 时默认显示在文本末尾）。
    pub cursor_byte_pos: Option<usize>,
    /// 可选的活动选区字节范围 `(start, end)`。
    pub selection_range: Option<(usize, usize)>,
    /// 聚焦时是否启用标准 530ms 周期光标闪烁动画（默认 `true`）。
    pub cursor_blink: bool,
    /// 最近一次键入或点击的时间戳（毫秒），用于立即将光标复位为常亮状态。
    pub blink_epoch_ms: u32,
    pub width: i16,
    pub height: i16,
    pub bg: Color,
    pub border_color: Color,
    pub focus_border_color: Color,
    pub text_color: Color,
    pub placeholder_color: Color,
    pub cursor_color: Color,
    pub selection_bg: Color,
    pub selection_rounded: u8,
    pub clear_icon_color: Color,
    pub clear_hover_bg: Option<Color>,
    pub clear_active_bg: Option<Color>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub clearable: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub prefix: Option<Element<'a>>,
    pub suffix: Option<Element<'a>>,
    pub on_focus: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_clear: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_toggle_mask: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    /// 亚像素点击与拖拽选区回调：`(text_local_x, is_drag, now_ms, shift)`。
    pub on_pointer_action: Option<&'a dyn Fn((i16, bool, u32, bool), &mut ActionScope)>,
    /// 鼠标右键上下文菜单触发回调：`(window_x, window_y)`。
    pub on_secondary_click: Option<&'a dyn Fn((i16, i16), &mut ActionScope)>,
}

impl<'a> TextInput<'a> {
    /// 创建一个无样式单行文本输入框原语（默认透明底、零圆角、零主题边框与中性光标/选区）。
    #[inline(always)]
    pub const fn new(id: u32, value: &'a str, focused: bool) -> Self {
        Self {
            id,
            value,
            placeholder: "",
            focused,
            disabled: false,
            masked: false,
            password_toggle: false,
            cursor_byte_pos: None,
            selection_range: None,
            cursor_blink: true,
            blink_epoch_ms: 0,
            width: 160,
            height: 32,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            focus_border_color: Color::TRANSPARENT,
            text_color: AUTO_COLOR,
            placeholder_color: AUTO_COLOR,
            cursor_color: AUTO_COLOR,
            selection_bg: AUTO_COLOR,
            selection_rounded: 0,
            clear_icon_color: AUTO_COLOR,
            clear_hover_bg: None,
            clear_active_bg: None,
            focus_ring: false,
            rounded: 0,
            clearable: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            prefix: None,
            suffix: None,
            on_focus: None,
            on_clear: None,
            on_toggle_mask: None,
            on_key_input: None,
            on_pointer_action: None,
            on_secondary_click: None,
        }
    }

    /// 显式设置输入框表面配色 `(bg, border_color, focus_border_color)`。
    #[inline(always)]
    pub const fn surface(
        mut self,
        bg: Color,
        border_color: Color,
        focus_border_color: Color,
    ) -> Self {
        self.bg = bg;
        self.border_color = border_color;
        self.focus_border_color = focus_border_color;
        self
    }

    /// 显式设置输入框圆角半径。
    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.rounded = radius;
        self
    }

    /// 启用或禁用右侧一键清空按钮（当值非空且未禁用时显示矢量 SVG `×` 按钮）。
    #[inline(always)]
    pub const fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// 绑定点击清空按钮回调。
    #[inline(always)]
    pub fn on_clear(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_clear = Some(handler);
        self.clearable = true;
        self
    }

    /// 设置当 `value` 为空时显示的占位提示文本。
    #[inline(always)]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// 开启或关闭密码掩码（`•`）模式。
    #[inline(always)]
    pub const fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// 开启或关闭右侧密码可见性切换小眼睛按钮（点击查看/隐藏密码）。
    #[inline(always)]
    pub const fn password_toggle(mut self, enabled: bool) -> Self {
        self.password_toggle = enabled;
        self
    }

    /// 一键配置为标准密码输入框（开启掩码显示并附带右侧小眼睛切换按钮）。
    #[inline(always)]
    pub const fn password(mut self, enabled: bool) -> Self {
        self.masked = enabled;
        self.password_toggle = enabled;
        self
    }

    /// 绑定点击右侧密码小眼睛切换按钮时的受控回调（自动显示小眼睛按钮）。
    #[inline(always)]
    pub fn on_toggle_mask(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_mask = Some(handler);
        self.password_toggle = true;
        self
    }

    /// 设置光标在 `value` 中的 UTF-8 字节偏移位置。
    #[inline(always)]
    pub const fn cursor(mut self, byte_offset: usize) -> Self {
        self.cursor_byte_pos = Some(byte_offset);
        self
    }

    /// 设置活动选区字节范围 `(start, end)`。
    #[inline(always)]
    pub const fn selection(mut self, range: Option<(usize, usize)>) -> Self {
        self.selection_range = range;
        self
    }

    /// 开启或关闭光标周期闪烁动画（默认开启）。
    #[inline(always)]
    pub const fn cursor_blink(mut self, enabled: bool) -> Self {
        self.cursor_blink = enabled;
        self
    }

    /// 传入最近一次键入/点击时间戳（来自 [`EditableText::blink_epoch_ms`]），键入瞬间立即保持光标常亮。
    #[inline(always)]
    pub const fn blink_epoch(mut self, epoch_ms: u32) -> Self {
        self.blink_epoch_ms = epoch_ms;
        self
    }

    /// 设置输入框宽高（像素）。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 绑定点击激活输入框回调。
    #[inline(always)]
    pub fn on_focus(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_focus = Some(handler);
        self
    }

    /// 绑定键盘字符与光标编辑按键回调（配合 `EditableText::handle_key_with_mods` 使用）。
    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// 绑定鼠标单击落点/双击选词/三击全选/按住拖拽选区回调：
    /// `handler((text_local_x, is_drag, now_ms, shift), acx)`。
    #[inline(always)]
    pub fn on_pointer_action(
        mut self,
        handler: &'a dyn Fn((i16, bool, u32, bool), &mut ActionScope),
    ) -> Self {
        self.on_pointer_action = Some(handler);
        self
    }

    /// 设置前置图标/装饰插槽（位于输入文本区域之前）。
    #[inline(always)]
    pub fn prefix(mut self, cx: &'a FrameScope, element: impl IntoElement<'a>) -> Self {
        self.prefix = Some(element.into_element(cx));
        self
    }

    /// 设置后置图标/装饰插槽（位于输入文本区域之后、清除按钮之前）。
    #[inline(always)]
    pub fn suffix(mut self, cx: &'a FrameScope, element: impl IntoElement<'a>) -> Self {
        self.suffix = Some(element.into_element(cx));
        self
    }

    /// 绑定鼠标右击回调：`handler((window_x, window_y), acx)`（用于就地弹出复制/粘贴上下文菜单）。
    #[inline(always)]
    pub fn on_secondary_click(
        mut self,
        handler: &'a dyn Fn((i16, i16), &mut ActionScope),
    ) -> Self {
        self.on_secondary_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(TextInput<'a>);
crate::impl_base_disableable!(TextInput<'a>);

impl<'a> crate::traits::Sizable for TextInput<'a> {
    #[inline(always)]
    fn with_size(mut self, size: crate::traits::ComponentSize) -> Self {
        self.height = match size {
            crate::traits::ComponentSize::Small => 26,
            crate::traits::ComponentSize::Medium => 32,
            crate::traits::ComponentSize::Large => 38,
        };
        self
    }
}

impl<'a> Widget<'a> for TextInput<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let focus_border_color = if self.focus_border_color != Color::TRANSPARENT {
            self.focus_border_color
        } else {
            border_color
        };
        let text_color = resolve_color(self.text_color, def_fg);
        let placeholder_color = resolve_color(self.placeholder_color, def_fg.scale_alpha(128));
        let cursor_color = resolve_color(self.cursor_color, def_fg);
        let selection_bg = resolve_color(self.selection_bg, def_fg.scale_alpha(48));
        let clear_icon_color = resolve_color(self.clear_icon_color, def_fg.scale_alpha(160));

        let border_col = if self.focused && !self.disabled {
            focus_border_color
        } else {
            border_color
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let show_clear = self.clearable && !self.value.is_empty() && !self.disabled;
        let show_eye = self.password_toggle && !self.disabled;
        let effective_masked = if self.on_toggle_mask.is_some() {
            self.masked
        } else if self.password_toggle && self.masked {
            {
                !is_password_revealed(self.id)
            }
        } else {
            self.masked
        };
        let inner_h = (height - 2).max(16);
        let font_size: u8 = self.style_ext.text_size.unwrap_or(13);

        // 构造完整显示串（掩码模式下由等量 `•` 构成，始终整串渲染不劈裂，彻底消除光标空隙与抖动）
        let display_str: &'a str = if effective_masked && !self.value.is_empty() {
            let char_count = self.value.chars().count();
            let mask_len = crate::input::MASK_CHAR.len_utf8();
            let bytes = cx.arena().alloc_bytes(char_count * mask_len);
            let mut enc = [0u8; 4];
            let mask_bytes = crate::input::MASK_CHAR.encode_utf8(&mut enc).as_bytes();
            for i in 0..char_count {
                bytes[i * mask_len..(i + 1) * mask_len].copy_from_slice(mask_bytes);
            }
            unsafe { core::str::from_utf8_unchecked(bytes) }
        } else {
            self.value
        };

        let map_byte_to_display = |raw_byte: usize| -> usize {
            let mut b = raw_byte.min(self.value.len());
            while b > 0 && !self.value.is_char_boundary(b) {
                b -= 1;
            }
            if effective_masked {
                let chars = self.value[..b].chars().count();
                chars.saturating_mul(crate::input::MASK_CHAR.len_utf8())
            } else {
                b
            }
        };

        let cursor_pos = self
            .cursor_byte_pos
            .unwrap_or(self.value.len())
            .min(self.value.len());
        let disp_cursor = map_byte_to_display(cursor_pos).min(display_str.len());
        let cursor_x_f = measure_line_width_f32(&display_str[..disp_cursor], font_size);
        let cursor_x = round_nonneg_i16(cursor_x_f);

        // 超长文本自动水平滚屏跟随光标
        let trail_reserve = if show_clear { 24 } else { 0 } + if show_eye { 26 } else { 0 };
        let avail_w = (width - 20 - trail_reserve - 2).max(20);
        let scroll_x = if self.focused && cursor_x + 4 > avail_w {
            cursor_x + 4 - avail_w
        } else {
            0
        };

        let mut box_div = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .h(height)
            .px(10)
            .gap(6)
            .items_center()
            .rounded(self.rounded)
            .bg(bg)
            .overflow_hidden();
        if border_col != Color::TRANSPARENT {
            box_div = box_div.border(1, border_col);
        }

        if self.disabled {
            box_div = box_div.opacity(120).cursor(CursorShape::NotAllowed);
        } else {
            box_div = box_div.focusable().cursor(CursorShape::Text);
            if self.focus_border_color != Color::TRANSPARENT {
                box_div = box_div.focus_border(1, self.focus_border_color);
            } else if !self.focus_ring {
                box_div = box_div.no_focus_border();
            }
            if let Some(on_ptr) = self.on_pointer_action {
                let on_focus = self.on_focus;
                let ptr_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let ev = acx.drag_event();
                    let is_drag = ev.delta_x != 0 || ev.delta_y != 0;
                    if !is_drag {
                        if let Some(fcb) = on_focus {
                            fcb(acx);
                        }
                    }
                    let text_x = ev
                        .local_x
                        .saturating_sub(10)
                        .saturating_add(scroll_x)
                        .max(0);
                    on_ptr(
                        (text_x, is_drag, acx.now_ms(), acx.modifiers().shift()),
                        acx,
                    );
                });
                box_div = box_div.on_drag_raw(ptr_cb);
            } else if let Some(cb) = self.on_focus {
                box_div = box_div.on_click_raw(cb);
            }
            if let Some(kcb) = self.on_key_input {
                box_div = box_div.on_key_input(kcb);
            }
            if let Some(sec_cb) = self.on_secondary_click {
                let sec_wrap = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let (wx, wy) = acx.click_position();
                    sec_cb((wx, wy), acx);
                });
                box_div = box_div.on_secondary_click_raw(sec_wrap);
            }
        }

        let mut content = div()
            .flex_row()
            .flex_grow(1)
            .h(inner_h)
            .items_center()
            .overflow_hidden();

        if self.value.is_empty() && !self.focused {
            content = content.child(
                cx,
                text(self.placeholder)
                    .size(font_size)
                    .color(placeholder_color)
                    .ellipsis(),
            );
        } else {
            // 1. 选区高亮层（精确浮点边界绝对定位，与文字行盒垂直中心严格重合）
            if let Some((raw_s, raw_e)) = self.selection_range.filter(|&(s, e)| s < e) {
                let disp_s = map_byte_to_display(raw_s).min(display_str.len());
                let disp_e = map_byte_to_display(raw_e).min(display_str.len());
                if disp_s < disp_e {
                    let sx0 =
                        round_nonneg_i16(measure_line_width_f32(&display_str[..disp_s], font_size));
                    let sx1 =
                        round_nonneg_i16(measure_line_width_f32(&display_str[..disp_e], font_size));
                    let sel_h = ((font_size as i16) + 4).clamp(14, (inner_h - 4).max(14));
                    let sel_y = ((inner_h - sel_h) / 2).max(1);
                    let x0 = sx0.saturating_sub(scroll_x);
                    let w = sx1.saturating_sub(sx0).max(3);
                    content = content.child(
                        cx,
                        div()
                            .pos(x0, sel_y)
                            .w(w)
                            .h(sel_h)
                            .rounded(self.selection_rounded)
                            .bg(selection_bg),
                    );
                }
            }

            // 2. 完整连续文本层（绝不在光标或选区处劈裂，与同行其他输入框垂直绝对齐平）
            if !display_str.is_empty() {
                if scroll_x == 0 {
                    content = content.child(
                        cx,
                        text(display_str)
                            .size(font_size)
                            .color(text_color)
                            .nowrap(),
                    );
                } else {
                    content = content.child(
                        cx,
                        div()
                            .pos(-scroll_x, 0)
                            .h(inner_h)
                            .flex_row()
                            .items_center()
                            .child(
                                cx,
                                text(display_str)
                                    .size(font_size)
                                    .color(text_color)
                                    .nowrap(),
                            ),
                    );
                }
            } else if self.focused && !self.placeholder.is_empty() {
                content = content.child(
                    cx,
                    text(self.placeholder)
                        .size(font_size)
                        .color(placeholder_color.scale_alpha(150))
                        .ellipsis(),
                );
            }

            // 3. 零抖动闪烁叠层光标（标准 530ms 周期，输入瞬间立即常亮；暗期保持透明 `.ime_caret()` 锚点防止候选窗跳动）
            if self.focused && !self.disabled {
                let caret_visible = if self.cursor_blink {
                    cx.request_frame();
                    let elapsed = cx.now_ms().wrapping_sub(self.blink_epoch_ms);
                    (elapsed / CURSOR_BLINK_HALF_PERIOD_MS) % 2 == 0
                } else {
                    true
                };
                let cur_h = ((font_size as i16) + 2).clamp(12, (inner_h - 6).max(12));
                let cur_y = ((inner_h - cur_h) / 2).max(1);
                let raw_cur_x = if disp_cursor == 0 {
                    0
                } else if disp_cursor < display_str.len() {
                    round_nonneg_i16(cursor_x_f - 0.85)
                } else {
                    cursor_x
                };
                let cur_x = raw_cur_x.saturating_sub(scroll_x).max(0);
                let caret_bg = if caret_visible {
                    cursor_color
                } else {
                    Color::TRANSPARENT
                };
                content = content.child(
                    cx,
                    div()
                        .id(self.id ^ 0x4355_5253)
                        .pos(cur_x, cur_y)
                        .w(1)
                        .h(cur_h)
                        .bg(caret_bg)
                        .ime_caret(),
                );
            }
        }

        if let Some(prefix_el) = self.prefix {
            box_div = box_div.child(cx, prefix_el);
        }

        box_div = box_div.child(cx, content);

        if let Some(suffix_el) = self.suffix {
            box_div = box_div.child(cx, suffix_el);
        }

        if show_eye {
            let eye_svg = if effective_masked {
                EYE_OPEN_SVG
            } else {
                EYE_OFF_SVG
            };
            let input_id = self.id;
            let eye_cb: &'a dyn Fn(&mut ActionScope) = if let Some(cb) = self.on_toggle_mask {
                cb
            } else {
                cx.arena().alloc(move |acx: &mut ActionScope| {
                    let _ = (input_id, &acx);
                    {
                        toggle_password_revealed(input_id);
                        acx.notify();
                    }
                })
            };
            let mut eye_btn = div()
                .id(crate::scoped_child_id(self.id, 0xE7E5_4001, 1))
                .w(22)
                .h(22)
                .rounded(if self.rounded > 0 {
                    self.rounded.saturating_sub(2).max(3)
                } else {
                    4
                })
                .items_center()
                .justify_center()
                .cursor(CursorShape::Pointer)
                .animate_color(Duration::from_millis(120))
                .child(
                    cx,
                    svg_icon(14, 14, eye_svg)
                        .tint(clear_icon_color)
                        .stroke_width(18),
                )
                .on_click_raw(eye_cb)
                .unfocusable()
                .occlude_hit();
            if let Some(h) = self.clear_hover_bg {
                eye_btn = eye_btn.hover_bg(h);
            }
            if let Some(a) = self.clear_active_bg {
                eye_btn = eye_btn.active_bg(a);
            }
            box_div = box_div.child(cx, eye_btn);
        }

        if show_clear {
            let mut clear_btn = div()
                .id(crate::scoped_child_id(self.id, 0xC1EA_4001, 1))
                .w(18)
                .h(18)
                .rounded(if self.rounded > 0 {
                    self.rounded.saturating_sub(2).max(2)
                } else {
                    0
                })
                .items_center()
                .justify_center()
                .cursor(CursorShape::Pointer)
                .child(
                    cx,
                    svg_icon(11, 11, "M18 6 6 18M6 6l12 12")
                        .tint(clear_icon_color)
                        .stroke_width(22),
                );
            if let Some(h) = self.clear_hover_bg {
                clear_btn = clear_btn.hover_bg(h);
            }
            if let Some(a) = self.clear_active_bg {
                clear_btn = clear_btn.active_bg(a);
            }
            if let Some(clear_cb) = self.on_clear {
                clear_btn = clear_btn
                    .on_click_raw(clear_cb)
                    .unfocusable()
                    .occlude_hit();
            }
            box_div = box_div.child(cx, clear_btn);
        }

        box_div = self.style_ext.apply_to_div(box_div, self.style);
        box_div.into_element(cx)
    }
}
