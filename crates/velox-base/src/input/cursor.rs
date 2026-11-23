//! 文本选区（[`Selection`]）、带方向与列锚点的光标选区（[`CursorSelection`]）、多光标选区集合（[`Selections`]）以及二维行列坐标（[`BufferPoint`]）。
//!
//! 完全兼容 `#![no_std]` 的零堆分配多光标选区模型。

use core::ops::{Bound, Range, RangeBounds};
use velox_engine::arena::SmallVec;

/// 二维文档行列坐标（`row` 为 0 起始行号，`column` 为行内 UTF-8 字节偏移）。
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct BufferPoint {
    pub row: usize,
    pub column: usize,
}

impl BufferPoint {
    #[inline(always)]
    pub const fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

/// 以起止 UTF-8 字节偏移表示的规范化文本选区（满足 `start <= end`）。
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
}

impl Selection {
    #[inline(always)]
    pub const fn new(start: usize, end: usize) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.start = 0;
        self.end = 0;
    }

    #[inline(always)]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

impl From<Range<usize>> for Selection {
    #[inline]
    fn from(value: Range<usize>) -> Self {
        Self::new(value.start, value.end)
    }
}

impl From<Selection> for Range<usize> {
    #[inline]
    fn from(value: Selection) -> Self {
        value.start..value.end
    }
}

impl RangeBounds<usize> for Selection {
    fn start_bound(&self) -> Bound<&usize> {
        Bound::Included(&self.start)
    }

    fn end_bound(&self) -> Bound<&usize> {
        Bound::Excluded(&self.end)
    }
}

/// 单个带方向与垂直移动列锚点（`column_anchor`）的光标选区。
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CursorSelection {
    pub id: u16,
    pub start: usize,
    pub end: usize,
    /// 若为 `true`，活动光标位于 `start`（向左选择）；否则位于 `end`（向右选择或折叠光标）。
    pub reversed: bool,
    /// 上下移动行时记忆的目标列字节偏移（防止经过短行时丢失原有列位置）。
    pub column_anchor: Option<usize>,
}

impl CursorSelection {
    #[inline(always)]
    pub const fn new(id: u16, start: usize, end: usize) -> Self {
        if start <= end {
            Self {
                id,
                start,
                end,
                reversed: false,
                column_anchor: None,
            }
        } else {
            Self {
                id,
                start: end,
                end: start,
                reversed: true,
                column_anchor: None,
            }
        }
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    #[inline(always)]
    pub const fn is_collapsed(&self) -> bool {
        self.is_empty()
    }

    #[inline(always)]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// 获取当前活动光标所在的字节偏移。
    #[inline(always)]
    pub const fn cursor_offset(&self) -> usize {
        if self.reversed {
            self.start
        } else {
            self.end
        }
    }

    /// 获取当前选区的固定锚点字节偏移。
    #[inline(always)]
    pub const fn anchor_offset(&self) -> usize {
        if self.reversed {
            self.end
        } else {
            self.start
        }
    }

    /// 将光标折叠放置到指定偏移 `offset`。
    #[inline]
    pub fn place_at(&mut self, offset: usize, column_anchor: Option<usize>) {
        self.start = offset;
        self.end = offset;
        self.reversed = false;
        self.column_anchor = column_anchor;
    }

    /// 保持锚点不变，将活动光标延伸到 `new_cursor`。
    #[inline]
    pub fn extend_to(&mut self, new_cursor: usize) {
        let anchor = self.anchor_offset();
        if new_cursor < anchor {
            self.start = new_cursor;
            self.end = anchor;
            self.reversed = true;
        } else {
            self.start = anchor;
            self.end = new_cursor;
            self.reversed = false;
        }
    }

    #[inline(always)]
    pub const fn as_selection(&self) -> Selection {
        Selection {
            start: self.start,
            end: self.end,
        }
    }
}

/// 多光标选区管理器（保证至少存在 1 个主活动选区，支持重叠选区自动合并）。
#[derive(Clone, Debug)]
pub struct Selections<const MAX_CURSORS: usize = 8> {
    selections: SmallVec<CursorSelection, MAX_CURSORS>,
    next_id: u16,
}

impl<const MAX_CURSORS: usize> Default for Selections<MAX_CURSORS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CURSORS: usize> Selections<MAX_CURSORS> {
    pub fn new() -> Self {
        let mut selections = SmallVec::new();
        let _ = selections.push(CursorSelection::new(0, 0, 0));
        Self {
            selections,
            next_id: 1,
        }
    }

    #[inline]
    pub fn active(&self) -> &CursorSelection {
        &self.selections.as_slice()[0]
    }

    #[inline]
    pub fn active_mut(&mut self) -> &mut CursorSelection {
        &mut self.selections.as_mut_slice()[0]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.selections.len()
    }

    #[inline]
    pub fn is_single(&self) -> bool {
        self.selections.len() == 1
    }

    #[inline]
    pub fn as_slice(&self) -> &[CursorSelection] {
        self.selections.as_slice()
    }

    pub fn generate_id(&mut self) -> u16 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    pub fn add(&mut self, selection: CursorSelection) -> bool {
        let ok = self.selections.push(selection);
        if ok {
            self.merge_overlapping();
        }
        ok
    }

    pub fn remove_all_but_active(&mut self) {
        while self.selections.len() > 1 {
            let _ = self.selections.pop();
        }
    }

    /// 合并所有重叠或相邻的多光标选区，并保持主活动选区位于索引 `0`。
    pub fn merge_overlapping(&mut self) {
        if self.selections.len() <= 1 {
            return;
        }
        let active_id = self.active().id;
        let slice = self.selections.as_mut_slice();
        // 简单插入排序（MAX_CURSORS 通常 <= 8）
        for i in 1..slice.len() {
            let mut j = i;
            while j > 0 && slice[j - 1].start > slice[j].start {
                slice.swap(j - 1, j);
                j -= 1;
            }
        }

        let mut merged: SmallVec<CursorSelection, MAX_CURSORS> = SmallVec::new();
        for sel in self.selections.as_slice() {
            if let Some(last) = merged.as_mut_slice().last_mut() {
                if sel.start <= last.end {
                    let did_merge = sel.start != last.start || sel.end != last.end;
                    if sel.end > last.end {
                        last.end = sel.end;
                    }
                    if sel.id == active_id {
                        last.id = active_id;
                        last.reversed = sel.reversed;
                    }
                    if did_merge {
                        last.column_anchor = None;
                    }
                    continue;
                }
            }
            let _ = merged.push(*sel);
        }

        if let Some(pos) = merged.as_slice().iter().position(|s| s.id == active_id) {
            merged.as_mut_slice().swap(0, pos);
        }
        self.selections = merged;
    }
}
