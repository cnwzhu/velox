//! 侵入式双向链表依赖边池、节点槽位定义与全局/`thread_local!` 响应式图存储表。

use core::cell::UnsafeCell;

use super::storage::{InlineFnStorage, InlineValueStorage, NO_EDGE};
use super::MAX_EFFECTS;

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum NodeKind {
    Signal = 0,
    Computed = 1,
    Effect = 2,
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum NodeState {
    /// 缓存值保证最新，Pull 阶段 $O(1)$ 直接返回。
    Clean = 0,
    /// 存在间接上游变更，Pull 前需递归核验直接上游版本（支持 Early Cutoff 剪枝）。
    Check = 1,
    /// 首次求值或直接上游已变更，Pull 时必须重新计算。
    Dirty = 2,
}

/// 侵入式双向链表依赖边：两端（上游 `first_sub` 与下游 `first_dep`）均为 $O(1)$ 插删。
#[derive(Copy, Clone)]
pub(super) struct DepEdge {
    pub(super) src_kind: NodeKind,
    dst_kind: NodeKind,
    pub(super) src_idx: u16,
    pub(super) src_gen: u16,
    dst_idx: u16,
    dst_gen: u16,
    pub(super) seen_version: u32,
    /// 目标节点（`Computed` / `Effect`）的上游依赖双向链表
    prev_dep: u32,
    pub(super) next_dep: u32,
    /// 源节点（`Signal` / `Computed`）的下游订阅者双向链表
    prev_sub: u32,
    next_sub: u32,
    pub(super) active: bool,
}

impl DepEdge {
    pub(super) const EMPTY: Self = Self {
        src_kind: NodeKind::Signal,
        dst_kind: NodeKind::Computed,
        src_idx: 0,
        src_gen: 0,
        dst_idx: 0,
        dst_gen: 0,
        seen_version: 0,
        prev_dep: NO_EDGE,
        next_dep: NO_EDGE,
        prev_sub: NO_EDGE,
        next_sub: NO_EDGE,
        active: false,
    };
}

#[derive(Clone)]
pub(super) struct SignalSlot {
    pub(super) storage: InlineValueStorage,
    pub(super) version: u32,
    pub(super) subscribers_mask: u64,
    pub(super) pending_dirty_mask: u64,
    ext_subscribers: std::vec::Vec<u16>,
    pub(super) ext_pending_dirty: std::vec::Vec<u16>,
    /// 侵入式下游订阅者链表头（`Computed` 与 `Effect`）
    pub(super) first_sub: u32,
    pub(super) owner_pool: u16,
    pub(super) generation: u16,
    pub(super) occupied: bool,
}

impl SignalSlot {

    pub(super) fn empty() -> Self {
        Self {
            storage: InlineValueStorage::ZERO,
            version: 0,
            subscribers_mask: 0,
            pending_dirty_mask: 0,
            ext_subscribers: std::vec::Vec::new(),
            ext_pending_dirty: std::vec::Vec::new(),
            first_sub: NO_EDGE,
            owner_pool: 0,
            generation: 0,
            occupied: false,
        }
    }

    #[inline(always)]
    pub(super) fn add_subscriber(&mut self, pod_idx: u16) {
        if pod_idx < 64 {
            self.subscribers_mask |= 1u64 << pod_idx;
        } else {
            if !self.ext_subscribers.contains(&pod_idx) {
                self.ext_subscribers.push(pod_idx);
            }
        }
    }

    #[inline(always)]
    pub(super) fn remove_subscriber(&mut self, pod_idx: u16) {
        if pod_idx < 64 {
            let bit = 1u64 << pod_idx;
            self.subscribers_mask &= !bit;
            self.pending_dirty_mask &= !bit;
        } else {
            {
                self.ext_subscribers.retain(|&x| x != pod_idx);
                self.ext_pending_dirty.retain(|&x| x != pod_idx);
            }
        }
    }

    #[inline(always)]
    pub(super) fn clear_subscriber_on_render(&mut self, pod_idx: u16) {
        if pod_idx < 64 {
            self.subscribers_mask &= !(1u64 << pod_idx);
        } else {
            self.ext_subscribers.retain(|&x| x != pod_idx);
        }
    }

    #[inline(always)]
    pub(super) fn clear_all_subscribers(&mut self) {
        self.subscribers_mask = 0;
        self.pending_dirty_mask = 0;
        {
            self.ext_subscribers.clear();
            self.ext_pending_dirty.clear();
        }
    }

    #[inline(always)]
    pub(super) fn has_any_subscribers(&self) -> bool {
        if self.first_sub != NO_EDGE {
            return true;
        }
        if !self.ext_subscribers.is_empty() {
            return true;
        }
        self.subscribers_mask != 0
    }

    #[inline(always)]
    pub(super) fn has_any_pending_dirty(&self) -> bool {
        if !self.ext_pending_dirty.is_empty() {
            return true;
        }
        self.pending_dirty_mask != 0
    }

    #[inline(always)]
    fn queue_subscribers_dirty(&mut self) -> bool {
        self.pending_dirty_mask |= self.subscribers_mask;
        {
            for &pod_idx in &self.ext_subscribers {
                if !self.ext_pending_dirty.contains(&pod_idx) {
                    self.ext_pending_dirty.push(pod_idx);
                }
            }
            if !self.ext_subscribers.is_empty() {
                return true;
            }
        }
        self.subscribers_mask != 0
    }
}

#[derive(Copy, Clone)]
pub(super) struct ComputedSlot {
    pub(super) cached: InlineValueStorage,
    pub(super) fn_storage: InlineFnStorage,
    pub(super) compute_fn: Option<unsafe fn(*const InlineFnStorage, *mut InlineValueStorage)>,
    pub(super) eq_fn: Option<unsafe fn(*const InlineValueStorage, *const InlineValueStorage) -> bool>,
    /// 上游依赖边链表头（`Signal` / `Computed`）
    pub(super) first_dep: u32,
    /// 下游订阅者边链表头（`Computed` / `Effect`）
    pub(super) first_sub: u32,
    pub(super) version: u32,
    pub(super) eval_count: u32,
    pub(super) generation: u16,
    pub(super) state: NodeState,
    pub(super) initialized: bool,
    pub(super) occupied: bool,
    pub(super) computing: bool,
}

impl ComputedSlot {
    pub(super) const EMPTY: Self = Self {
        cached: InlineValueStorage::ZERO,
        fn_storage: InlineFnStorage::ZERO,
        compute_fn: None,
        eq_fn: None,
        first_dep: NO_EDGE,
        first_sub: NO_EDGE,
        version: 0,
        eval_count: 0,
        generation: 0,
        state: NodeState::Dirty,
        initialized: false,
        occupied: false,
        computing: false,
    };
}

#[derive(Copy, Clone)]
pub(super) struct EffectSlot {
    pub(super) fn_storage: InlineFnStorage,
    pub(super) run_fn: Option<unsafe fn(*const InlineFnStorage)>,
    /// 上游依赖边链表头（`Signal` / `Computed`）
    pub(super) first_dep: u32,
    pub(super) run_count: u32,
    pub(super) generation: u16,
    pub(super) occupied: bool,
    pub(super) running: bool,
}

impl EffectSlot {
    pub(super) const EMPTY: Self = Self {
        fn_storage: InlineFnStorage::ZERO,
        run_fn: None,
        first_dep: NO_EDGE,
        run_count: 0,
        generation: 0,
        occupied: false,
        running: false,
    };
}


pub(super) struct ReactiveGraphTable {
    pub(super) signals: std::vec::Vec<SignalSlot>,
    pub(super) computeds: std::vec::Vec<ComputedSlot>,
    pub(super) effects: [EffectSlot; MAX_EFFECTS],
    pub(super) edges: std::vec::Vec<DepEdge>,
    free_edge_head: u32,
    active_edge_count: usize,
    pub(super) recycle_cursor: usize,
    pub(super) computed_cursor: usize,
    pub(super) effect_cursor: usize,
}

impl ReactiveGraphTable {
    #[inline]
    fn src_first_sub(&self, kind: NodeKind, idx: usize) -> Option<u32> {
        match kind {
            NodeKind::Signal => self.signals.get(idx).map(|s| s.first_sub),
            NodeKind::Computed => self.computeds.get(idx).map(|c| c.first_sub),
            NodeKind::Effect => None,
        }
    }

    #[inline]
    fn set_src_first_sub(&mut self, kind: NodeKind, idx: usize, head: u32) {
        match kind {
            NodeKind::Signal => {
                if let Some(s) = self.signals.get_mut(idx) {
                    s.first_sub = head;
                }
            }
            NodeKind::Computed => {
                if let Some(c) = self.computeds.get_mut(idx) {
                    c.first_sub = head;
                }
            }
            NodeKind::Effect => {}
        }
    }

    #[inline]
    fn dst_first_dep(&self, kind: NodeKind, idx: usize) -> Option<u32> {
        match kind {
            NodeKind::Computed => self.computeds.get(idx).map(|c| c.first_dep),
            NodeKind::Effect => self.effects.get(idx).map(|e| e.first_dep),
            NodeKind::Signal => None,
        }
    }

    #[inline]
    fn set_dst_first_dep(&mut self, kind: NodeKind, idx: usize, head: u32) {
        match kind {
            NodeKind::Computed => {
                if let Some(c) = self.computeds.get_mut(idx) {
                    c.first_dep = head;
                }
            }
            NodeKind::Effect => {
                if let Some(e) = self.effects.get_mut(idx) {
                    e.first_dep = head;
                }
            }
            NodeKind::Signal => {}
        }
    }

    fn alloc_edge(&mut self) -> Option<u32> {
        if self.free_edge_head != NO_EDGE {
            let idx = self.free_edge_head;
            self.free_edge_head = self.edges[idx as usize].next_dep;
            self.edges[idx as usize] = DepEdge::EMPTY;
            return Some(idx);
        }
        {
            if self.edges.len() >= (u32::MAX as usize - 1) {
                return None;
            }
            let idx = self.edges.len() as u32;
            self.edges.push(DepEdge::EMPTY);
            Some(idx)
        }
    }

    #[inline]
    fn unlink_edge(&mut self, edge_idx: u32) {
        let Some(e) = self.edges.get(edge_idx as usize).copied() else {
            return;
        };
        if !e.active {
            return;
        }

        // 1. 从下游目标节点的 first_dep 双向链表摘除
        if e.prev_dep != NO_EDGE {
            self.edges[e.prev_dep as usize].next_dep = e.next_dep;
        } else {
            self.set_dst_first_dep(e.dst_kind, e.dst_idx as usize, e.next_dep);
        }
        if e.next_dep != NO_EDGE {
            self.edges[e.next_dep as usize].prev_dep = e.prev_dep;
        }

        // 2. 从上游源节点的 first_sub 双向链表摘除
        if e.prev_sub != NO_EDGE {
            self.edges[e.prev_sub as usize].next_sub = e.next_sub;
        } else {
            self.set_src_first_sub(e.src_kind, e.src_idx as usize, e.next_sub);
        }
        if e.next_sub != NO_EDGE {
            self.edges[e.next_sub as usize].prev_sub = e.prev_sub;
        }

        // 3. 回收至空闲链表
        let slot = &mut self.edges[edge_idx as usize];
        slot.active = false;
        slot.prev_dep = NO_EDGE;
        slot.prev_sub = NO_EDGE;
        slot.next_sub = NO_EDGE;
        slot.next_dep = self.free_edge_head;
        self.free_edge_head = edge_idx;
        self.active_edge_count = self.active_edge_count.saturating_sub(1);
    }

    pub(super) fn unlink_all_deps(&mut self, dst_kind: NodeKind, dst_idx: u16) {
        while let Some(head) = self.dst_first_dep(dst_kind, dst_idx as usize) {
            if head == NO_EDGE {
                break;
            }
            self.unlink_edge(head);
        }
    }

    pub(super) fn unlink_all_subs(&mut self, src_kind: NodeKind, src_idx: u16) {
        while let Some(head) = self.src_first_sub(src_kind, src_idx as usize) {
            if head == NO_EDGE {
                break;
            }
            self.unlink_edge(head);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn link_dep(
        &mut self,
        src_kind: NodeKind,
        src_idx: u16,
        src_gen: u16,
        seen_version: u32,
        dst_kind: NodeKind,
        dst_idx: u16,
        dst_gen: u16,
    ) {
        // 去重：若目标节点已记录对同一源节点的依赖，直接刷新 seen_version
        let mut cur = self.dst_first_dep(dst_kind, dst_idx as usize).unwrap_or(NO_EDGE);
        while cur != NO_EDGE {
            let e = &mut self.edges[cur as usize];
            if e.active && e.src_kind == src_kind && e.src_idx == src_idx {
                e.src_gen = src_gen;
                e.seen_version = seen_version;
                return;
            }
            cur = e.next_dep;
        }

        let Some(edge_idx) = self.alloc_edge() else {
            return;
        };

        let old_dep_head = self.dst_first_dep(dst_kind, dst_idx as usize).unwrap_or(NO_EDGE);
        let old_sub_head = self.src_first_sub(src_kind, src_idx as usize).unwrap_or(NO_EDGE);

        self.edges[edge_idx as usize] = DepEdge {
            src_kind,
            dst_kind,
            src_idx,
            src_gen,
            dst_idx,
            dst_gen,
            seen_version,
            prev_dep: NO_EDGE,
            next_dep: old_dep_head,
            prev_sub: NO_EDGE,
            next_sub: old_sub_head,
            active: true,
        };

        if old_dep_head != NO_EDGE {
            self.edges[old_dep_head as usize].prev_dep = edge_idx;
        }
        self.set_dst_first_dep(dst_kind, dst_idx as usize, edge_idx);

        if old_sub_head != NO_EDGE {
            self.edges[old_sub_head as usize].prev_sub = edge_idx;
        }
        self.set_src_first_sub(src_kind, src_idx as usize, edge_idx);
        self.active_edge_count += 1;
    }

    /// Push 阶段：从变更的 `Signal` 出发，将直接下游 `Computed` 标记为 `Dirty`，
    /// 间接下游 `Computed` 传播为 `Check`，并聚合受影响的 `Effect` 位掩码。
    pub(super) fn push_dirty_from_signal(&mut self, sig_idx: usize) -> (bool, u32) {
        let Some(slot) = self.signals.get_mut(sig_idx) else {
            return (false, 0);
        };
        let has_pod_subs = slot.queue_subscribers_dirty();
        let first_sub = slot.first_sub;
        if first_sub == NO_EDGE {
            return (has_pod_subs, 0);
        }

        let mut pending_effects_mask = 0u32;
        let mut visited_low = 0u64;
        let mut stack: std::vec::Vec<u16> = std::vec::Vec::new();
        let mut visited_ext: std::vec::Vec<u16> = std::vec::Vec::new();

        // 1. 直接订阅者：Computed 置 Dirty，Effect 入队
        let mut edge = first_sub;
        while edge != NO_EDGE {
            let e = self.edges[edge as usize];
            edge = e.next_sub;
            if !e.active {
                continue;
            }
            match e.dst_kind {
                NodeKind::Computed => {
                    let c_idx = e.dst_idx as usize;
                    if let Some(c) = self.computeds.get_mut(c_idx) {
                        if c.occupied && c.generation == e.dst_gen {
                            c.state = NodeState::Dirty;
                            let already = if c_idx < 64 {
                                let bit = 1u64 << c_idx;
                                let hit = (visited_low & bit) != 0;
                                visited_low |= bit;
                                hit
                            } else {
                                {
                                    if visited_ext.contains(&e.dst_idx) {
                                        true
                                    } else {
                                        visited_ext.push(e.dst_idx);
                                        false
                                    }
                                }
                            };
                            if !already {
                                stack.push(e.dst_idx);
                            }
                        }
                    }
                }
                NodeKind::Effect => {
                    let e_idx = e.dst_idx as usize;
                    if e_idx < 32 {
                        if let Some(eff) = self.effects.get(e_idx) {
                            if eff.occupied && eff.generation == e.dst_gen {
                                pending_effects_mask |= 1u32 << e_idx;
                            }
                        }
                    }
                }
                NodeKind::Signal => {}
            }
        }

        // 2. 间接下游：Computed 升级为 Check（保持已有的 Dirty），收集所有下游 Effect
        loop {
            let Some(cur_c) = stack.pop() else {
                break;
            };

            let mut sub_edge = self
                .computeds
                .get(cur_c as usize)
                .map(|c| c.first_sub)
                .unwrap_or(NO_EDGE);
            while sub_edge != NO_EDGE {
                let e = self.edges[sub_edge as usize];
                sub_edge = e.next_sub;
                if !e.active {
                    continue;
                }
                match e.dst_kind {
                    NodeKind::Computed => {
                        let next_idx = e.dst_idx as usize;
                        if let Some(nc) = self.computeds.get_mut(next_idx) {
                            if nc.occupied && nc.generation == e.dst_gen {
                                if nc.state == NodeState::Clean {
                                    nc.state = NodeState::Check;
                                }
                                let already = if next_idx < 64 {
                                    let bit = 1u64 << next_idx;
                                    let hit = (visited_low & bit) != 0;
                                    visited_low |= bit;
                                    hit
                                } else {
                                    {
                                        if visited_ext.contains(&e.dst_idx) {
                                            true
                                        } else {
                                            visited_ext.push(e.dst_idx);
                                            false
                                        }
                                    }
                                };
                                if !already {
                                    stack.push(e.dst_idx);
                                }
                            }
                        }
                    }
                    NodeKind::Effect => {
                        let e_idx = e.dst_idx as usize;
                        if e_idx < 32 {
                            if let Some(eff) = self.effects.get(e_idx) {
                                if eff.occupied && eff.generation == e.dst_gen {
                                    pending_effects_mask |= 1u32 << e_idx;
                                }
                            }
                        }
                    }
                    NodeKind::Signal => {}
                }
            }
        }

        (has_pod_subs, pending_effects_mask)
    }

    /// 将一个 `Computed` DAG 底层所依赖的所有源 `Signal` 注册给正在渲染的 `PodId`。
    pub(super) fn bind_computed_signals_to_pod(&mut self, comp_idx: u16, active_pool: u16, active_pod: u16) {
        let mut visited_low = 0u64;
        let mut stack: std::vec::Vec<u16> = std::vec::Vec::new();
        let mut visited_ext: std::vec::Vec<u16> = std::vec::Vec::new();

        stack.push(comp_idx);
        if (comp_idx as usize) < 64 {
            visited_low |= 1u64 << comp_idx;
        } else {
            visited_ext.push(comp_idx);
        }

        loop {
            let Some(cur_c) = stack.pop() else {
                break;
            };

            let mut edge = self
                .computeds
                .get(cur_c as usize)
                .map(|c| c.first_dep)
                .unwrap_or(NO_EDGE);
            while edge != NO_EDGE {
                let e = self.edges[edge as usize];
                edge = e.next_dep;
                if !e.active {
                    continue;
                }
                match e.src_kind {
                    NodeKind::Signal => {
                        if let Some(sig) = self.signals.get_mut(e.src_idx as usize) {
                            if sig.occupied && sig.generation == e.src_gen {
                                if sig.owner_pool != active_pool && sig.owner_pool != 0 {
                                    sig.clear_all_subscribers();
                                }
                                sig.owner_pool = active_pool;
                                sig.add_subscriber(active_pod);
                            }
                        }
                    }
                    NodeKind::Computed => {
                        let up_idx = e.src_idx as usize;
                        let already = if up_idx < 64 {
                            let bit = 1u64 << up_idx;
                            let hit = (visited_low & bit) != 0;
                            visited_low |= bit;
                            hit
                        } else {
                            {
                                if visited_ext.contains(&e.src_idx) {
                                    true
                                } else {
                                    visited_ext.push(e.src_idx);
                                    false
                                }
                            }
                        };
                        if !already {
                            stack.push(e.src_idx);
                        }
                    }
                    NodeKind::Effect => {}
                }
            }
        }
    }
}


std::thread_local! {
    static TLS_REACTIVE_TABLE: UnsafeCell<ReactiveGraphTable> = UnsafeCell::new(ReactiveGraphTable {
        signals: std::vec::Vec::new(),
        computeds: std::vec::Vec::new(),
        effects: [EffectSlot::EMPTY; MAX_EFFECTS],
        edges: std::vec::Vec::new(),
        free_edge_head: NO_EDGE,
        active_edge_count: 0,
        recycle_cursor: 0,
        computed_cursor: 0,
        effect_cursor: 0,
    });
    static TLS_TABLE_BORROWED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

#[inline(always)]
pub(super) fn with_table<R>(f: impl FnOnce(&mut ReactiveGraphTable) -> R) -> R {
    {
        TLS_REACTIVE_TABLE.with(|cell| {
            let was_borrowed = TLS_TABLE_BORROWED.with(|b| b.replace(true));
            debug_assert!(
                !was_borrowed,
                "Reentrant ReactiveGraphTable borrow detected"
            );
            let res = f(unsafe { &mut *cell.get() });
            TLS_TABLE_BORROWED.with(|b| b.set(false));
            res
        })
    }
}

/// 返回当前响应式图中活跃的侵入式依赖边总数（用于诊断与测试验证零边泄漏）。
#[inline]
pub fn active_dep_edge_count() -> usize {
    with_table(|tbl| tbl.active_edge_count)
}
