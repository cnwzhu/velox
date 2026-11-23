//! 事务分组撤销/重做历史栈（[`UndoHistory`]，零堆分配 `#![no_std]` 实现）。

use velox_engine::arena::SmallVec;

/// 支持显式分组（`start_grouping` / `end_grouping`）、容量上限淘汰（`max_undos`）与忽略录制模式（`ignoring`）的撤销/重做历史管理器。
#[derive(Clone, Debug)]
pub struct UndoHistory<T: Copy, const MAX_TX: usize = 32, const GROUP_CAP: usize = 4> {
    undos: SmallVec<SmallVec<T, GROUP_CAP>, MAX_TX>,
    redos: SmallVec<SmallVec<T, GROUP_CAP>, MAX_TX>,
    max_undos: usize,
    grouping: bool,
    append_to_last: bool,
    ignoring: bool,
}

impl<T: Copy, const MAX_TX: usize, const GROUP_CAP: usize> Default
    for UndoHistory<T, MAX_TX, GROUP_CAP>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const MAX_TX: usize, const GROUP_CAP: usize> UndoHistory<T, MAX_TX, GROUP_CAP> {
    /// 创建新的撤销历史栈，默认保留最多 `MAX_TX` 笔事务。
    pub const fn new() -> Self {
        Self {
            undos: SmallVec::new(),
            redos: SmallVec::new(),
            max_undos: MAX_TX,
            grouping: false,
            append_to_last: false,
            ignoring: false,
        }
    }

    /// 设置最多保留的撤销事务数量（超出时立即淘汰最旧事务）。
    pub fn max_undos(mut self, max_undos: usize) -> Self {
        self.max_undos = max_undos.min(MAX_TX);
        self.enforce_max_undos();
        self
    }

    /// 开始显式事务分组（后续连续 `push` 的变更将合并到同一笔撤销事务中）。
    #[inline]
    pub fn start_grouping(&mut self) {
        self.grouping = true;
        self.append_to_last = false;
    }

    /// 结束显式事务分组。
    #[inline]
    pub fn end_grouping(&mut self) {
        self.grouping = false;
        self.append_to_last = false;
    }

    /// 打断当前连续编辑合并，使下一次 `push` 开启独立的新事务。
    #[inline]
    pub fn break_coalescing(&mut self) {
        if !self.grouping {
            self.append_to_last = false;
        }
    }

    /// 是否正在忽略历史记录写入。
    #[inline(always)]
    pub const fn is_ignoring(&self) -> bool {
        self.ignoring
    }

    /// 设置是否忽略历史记录写入（例如在执行 `undo` / `redo` 回放期间）。
    #[inline(always)]
    pub fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
    }

    /// 压入一条变更记录。
    pub fn push(&mut self, item: T) {
        if self.ignoring || self.max_undos == 0 {
            return;
        }

        let mut appended = false;
        if self.append_to_last && !self.undos.is_empty() {
            if let Some(last_tx) = self.undos.as_mut_slice().last_mut() {
                if last_tx.push(item) {
                    appended = true;
                }
            }
        }

        if !appended {
            if self.undos.len() >= self.max_undos || self.undos.len() >= MAX_TX {
                self.evict_oldest_undo();
            }
            let mut tx = SmallVec::new();
            let _ = tx.push(item);
            let _ = self.undos.push(tx);
            if self.grouping {
                self.append_to_last = true;
            }
        }

        self.redos.clear();
    }

    /// 撤销最近一笔事务，按**最新优先（Newest-First）**顺序返回该事务中的所有变更项。
    pub fn undo(&mut self) -> Option<SmallVec<T, GROUP_CAP>> {
        let tx = self.undos.pop()?;
        let mut reversed = SmallVec::new();
        for item in tx.as_slice().iter().rev() {
            let _ = reversed.push(*item);
        }
        if self.redos.len() >= MAX_TX {
            self.evict_oldest_redo();
        }
        let _ = self.redos.push(tx);
        self.append_to_last = false;
        Some(reversed)
    }

    /// 重做最近一次撤销的事务，按**最旧优先（Oldest-First）**顺序返回该事务中的所有变更项。
    pub fn redo(&mut self) -> Option<SmallVec<T, GROUP_CAP>> {
        if self.max_undos == 0 {
            return None;
        }
        let tx = self.redos.pop()?;
        let changes = tx.clone();
        if self.undos.len() >= self.max_undos || self.undos.len() >= MAX_TX {
            self.evict_oldest_undo();
        }
        let _ = self.undos.push(tx);
        self.append_to_last = false;
        Some(changes)
    }

    #[inline(always)]
    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }

    #[inline(always)]
    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.undos.clear();
        self.redos.clear();
        self.append_to_last = false;
    }

    #[inline]
    pub(crate) fn last_undo_mut(&mut self) -> Option<&mut SmallVec<T, GROUP_CAP>> {
        self.undos.as_mut_slice().last_mut()
    }

    fn evict_oldest_undo(&mut self) {
        let len = self.undos.len();
        if len > 0 {
            let slice = self.undos.as_mut_slice();
            for i in 1..len {
                slice[i - 1] = slice[i].clone();
            }
            let _ = self.undos.pop();
        }
    }

    fn evict_oldest_redo(&mut self) {
        let len = self.redos.len();
        if len > 0 {
            let slice = self.redos.as_mut_slice();
            for i in 1..len {
                slice[i - 1] = slice[i].clone();
            }
            let _ = self.redos.pop();
        }
    }

    fn enforce_max_undos(&mut self) {
        while self.undos.len() > self.max_undos {
            self.evict_oldest_undo();
        }
    }
}
