//! 编辑器文本查找与替换匹配引擎（[`SearchMatcher`] / [`SearchSession`]，兼容 `#![no_std]`）。

use core::ops::Range;
use velox_core::arena::SmallVec;

use super::cursor::Selection;

/// 零堆分配文本查找匹配器，支持大小写敏感与忽略大小写匹配、循环跳转（`next_match` / `prev_match`）以及替换位置保持。
#[derive(Clone, Debug)]
pub struct SearchMatcher<const MAX_MATCHES: usize = 64> {
    query_bytes: [u8; 64],
    query_len: usize,
    pub case_sensitive: bool,
    matched_ranges: SmallVec<Selection, MAX_MATCHES>,
    current_match_ix: usize,
    replacing: bool,
}

impl<const MAX_MATCHES: usize> Default for SearchMatcher<MAX_MATCHES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_MATCHES: usize> SearchMatcher<MAX_MATCHES> {
    pub const fn new() -> Self {
        Self {
            query_bytes: [0u8; 64],
            query_len: 0,
            case_sensitive: true,
            matched_ranges: SmallVec::new(),
            current_match_ix: 0,
            replacing: false,
        }
    }

    #[inline]
    pub fn query(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.query_bytes[..self.query_len]) }
    }

    #[inline]
    pub fn matched_ranges(&self) -> &[Selection] {
        self.matched_ranges.as_slice()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.matched_ranges.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.matched_ranges.is_empty()
    }

    #[inline]
    pub const fn current_match_index(&self) -> usize {
        self.current_match_ix
    }

    #[inline]
    pub fn set_current_match_index(&mut self, index: usize) {
        if !self.matched_ranges.is_empty() {
            self.current_match_ix = index.min(self.matched_ranges.len() - 1);
        } else {
            self.current_match_ix = 0;
        }
    }

    #[inline]
    pub fn current_match(&self) -> Option<Selection> {
        self.matched_ranges
            .as_slice()
            .get(self.current_match_ix)
            .copied()
    }

    #[inline]
    pub fn begin_replacement(&mut self) {
        self.replacing = true;
    }

    #[inline]
    pub fn has_next_without_wrap(&self) -> bool {
        !self.matched_ranges.is_empty() && self.current_match_ix + 1 < self.matched_ranges.len()
    }

    /// 设置查找关键字与大小写敏感选项（若关键字与选项未变，则保留当前匹配项索引）。
    pub fn set_query(&mut self, query: &str, case_sensitive: bool) -> bool {
        let mut valid_len = 0usize;
        for ch in query.chars() {
            if valid_len + ch.len_utf8() <= self.query_bytes.len() {
                valid_len += ch.len_utf8();
            } else {
                break;
            }
        }
        let unchanged = self.case_sensitive == case_sensitive
            && self.query_len == valid_len
            && &self.query_bytes[..valid_len] == &query.as_bytes()[..valid_len];
        if unchanged {
            return false;
        }
        self.query_bytes[..valid_len].copy_from_slice(&query.as_bytes()[..valid_len]);
        self.query_len = valid_len;
        self.case_sensitive = case_sensitive;
        self.current_match_ix = 0;
        true
    }

    /// 在文本 `text` 中重新计算所有匹配范围。
    pub fn update_matches(&mut self, text: &str) {
        self.matched_ranges.clear();
        let qlen = self.query_len;
        if qlen == 0 || text.len() < qlen {
            self.current_match_ix = 0;
            self.replacing = false;
            return;
        }
        let mut q_buf = [0u8; 64];
        q_buf[..qlen].copy_from_slice(&self.query_bytes[..qlen]);
        let q = unsafe { core::str::from_utf8_unchecked(&q_buf[..qlen]) };

        if self.case_sensitive {
            let mut offset = 0usize;
            while offset + q.len() <= text.len() {
                if let Some(rel) = text[offset..].find(q) {
                    let start = offset + rel;
                    let end = start + q.len();
                    if !self.matched_ranges.push(Selection::new(start, end)) {
                        break;
                    }
                    offset = end.max(start + 1);
                } else {
                    break;
                }
            }
        } else {
            let q_bytes = &q_buf[..qlen];
            let t_bytes = text.as_bytes();
            let mut i = 0usize;
            while i + q_bytes.len() <= t_bytes.len() {
                if text.is_char_boundary(i) && text.is_char_boundary(i + q_bytes.len()) {
                    let slice = &t_bytes[i..i + q_bytes.len()];
                    if slice.eq_ignore_ascii_case(q_bytes) {
                        if !self
                            .matched_ranges
                            .push(Selection::new(i, i + q_bytes.len()))
                        {
                            break;
                        }
                        i += q_bytes.len().max(1);
                        continue;
                    }
                }
                i += 1;
            }
        }

        if self.matched_ranges.is_empty() {
            self.current_match_ix = 0;
        } else if self.replacing {
            self.current_match_ix = self.current_match_ix.min(self.matched_ranges.len() - 1);
        } else if self.current_match_ix >= self.matched_ranges.len() {
            self.current_match_ix = 0;
        }
        self.replacing = false;
    }

    /// 根据光标字节偏移定位最近的匹配项索引。
    pub fn update_cursor_by_offset(&mut self, offset: usize) {
        for (ix, m) in self.matched_ranges.as_slice().iter().enumerate() {
            if m.start >= offset || m.contains(offset) {
                self.current_match_ix = ix;
                return;
            }
        }
        if !self.matched_ranges.is_empty() {
            self.current_match_ix = self.matched_ranges.len() - 1;
        }
    }

    /// 跳转到下一个匹配项（到达末尾时回绕到第 0 项）。
    pub fn next_match(&mut self) -> Option<Range<usize>> {
        if self.matched_ranges.is_empty() {
            return None;
        }
        self.current_match_ix = (self.current_match_ix + 1) % self.matched_ranges.len();
        let sel = self.matched_ranges.as_slice()[self.current_match_ix];
        Some(sel.start..sel.end)
    }

    /// 跳转到上一个匹配项（到达开头时回绕到末尾项）。
    pub fn prev_match(&mut self) -> Option<Range<usize>> {
        if self.matched_ranges.is_empty() {
            return None;
        }
        if self.current_match_ix == 0 {
            self.current_match_ix = self.matched_ranges.len() - 1;
        } else {
            self.current_match_ix -= 1;
        }
        let sel = self.matched_ranges.as_slice()[self.current_match_ix];
        Some(sel.start..sel.end)
    }
}

/// 编辑器查找/替换会话状态（`SearchSession`）。
#[derive(Clone, Debug, Default)]
pub struct SearchSession<const MAX_MATCHES: usize = 64> {
    pub matcher: SearchMatcher<MAX_MATCHES>,
    pub open: bool,
    pub replace_mode: bool,
    active: bool,
}

impl<const MAX_MATCHES: usize> SearchSession<MAX_MATCHES> {
    pub const fn new() -> Self {
        Self {
            matcher: SearchMatcher::new(),
            open: false,
            replace_mode: false,
            active: false,
        }
    }

    #[inline]
    pub fn open(&mut self, replace_mode: bool, case_sensitive: bool) {
        self.open = true;
        self.replace_mode = replace_mode;
        self.active = true;
        self.matcher.case_sensitive = case_sensitive;
    }

    #[inline]
    pub fn activate(&mut self) {
        self.active = true;
    }

    #[inline]
    pub fn close(&mut self) {
        self.open = false;
        self.replace_mode = false;
        self.active = false;
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    pub fn update_query(&mut self, text: &str, query: &str, case_sensitive: bool) {
        self.active = true;
        if self.matcher.set_query(query, case_sensitive) {
            self.matcher.update_matches(text);
        }
    }
}
