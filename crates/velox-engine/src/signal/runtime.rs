//! 三色 Push-Pull 运行时调度：`batch` 事务、`PodPool` 订阅桥接、按需 Pull 验证与 `Effect` 刷新。

use crate::pod::PodId;

use super::storage::{
    dec_batch_depth, get_active_dep_tracker, get_active_render_context, get_batch_depth,
    get_has_pending_dirty, inc_batch_depth, push_batch_effects, set_has_pending_dirty,
    swap_active_dep_tracker, swap_active_render_context, take_batch_effects, EdgeIdx,
    InlineFnStorage, InlineValueStorage, NO_ACTIVE_POD, NO_ACTIVE_TRACKER, NO_EDGE,
    TRACKER_INDEX_MASK, TRACKER_KIND_COMPUTED, TRACKER_KIND_EFFECT,
};
use super::table::{with_table, NodeKind, NodeState};

/// 批量合并多次信号变更：事务内所有 `Signal::set` / `Signal::update` 仅推进三色图状态，
/// 待最外层 `batch` 退出后统一验证并执行受影响的 `Effect`，彻底消除多信号联动的 Glitch。
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    inc_batch_depth();
    let res = f();
    if dec_batch_depth() == 0 {
        let pending = take_batch_effects();
        if pending != 0 {
            flush_pending_effects(pending);
        }
    }
    res
}


#[inline]
pub(crate) fn begin_pod_tracking(pool_id: u16, pod_id: PodId) -> (u16, u16) {
    let idx = pod_id.0;
    if idx != NO_ACTIVE_POD {
        with_table(|tbl| {
            for s in tbl.signals.iter_mut() {
                if s.occupied {
                    s.clear_subscriber_on_render(pool_id, idx);
                }
            }
        });
    }
    swap_active_render_context(pool_id, idx)
}

/// 在 `PodPool` 结束某个 `PodId` 的 `View::render` 时恢复上一层追踪上下文。
#[inline(always)]
pub(crate) fn end_pod_tracking(prev: (u16, u16)) {
    let _ = swap_active_render_context(prev.0, prev.1);
}

/// 当某个 `PodId` 从 `PodPool` 销毁移除时，清理其在所属 `pool_id` 信号上的订阅位。
#[inline]
pub(crate) fn remove_pod_subscriptions(pool_id: u16, pod_id: PodId) {
    let idx = pod_id.0;
    if idx != NO_ACTIVE_POD && pool_id != 0 {
        with_table(|tbl| {
            for s in tbl.signals.iter_mut() {
                if s.occupied {
                    s.remove_subscriber(pool_id, idx);
                }
            }
        });
    }
}

/// 当整个 `PodPool` 实例被销毁时，一次性回收其在全部信号槽位上的订阅与待处理脏位。
#[inline]
pub(crate) fn remove_pool_subscriptions(pool_id: u16) {
    if pool_id != 0 {
        with_table(|tbl| {
            for s in tbl.signals.iter_mut() {
                if s.occupied {
                    s.remove_pool(pool_id);
                }
            }
        });
    }
}

/// 是否存在由 `Signal::set` / `Signal::update` 产生但尚未合入 `PodPool` 的待处理脏视图。
#[inline(always)]
pub fn has_pending_dirty_pods() -> bool {
    get_has_pending_dirty()
}

/// 取出并清零所属 `pool_id` 下所有由信号变更触发的脏 `PodId`（支持多实例引擎并发共享同一信号，互不覆盖）。
#[inline]
pub(crate) fn drain_pending_dirty_pods(pool_id: u16, mut mark_fn: impl FnMut(PodId)) {
    if !get_has_pending_dirty() {
        return;
    }
    let mut out_mask = 0u64;
    let mut ext_out: std::vec::Vec<u16> = std::vec::Vec::new();
    let mut any_remaining = false;

    with_table(|tbl| {
        for s in tbl.signals.iter_mut() {
            if s.occupied && s.has_any_pending_dirty() {
                let (mask, other_rem) = s.drain_pending_dirty_for_pool(
                    pool_id,
                    &mut ext_out,
                );
                out_mask |= mask;
                if other_rem {
                    any_remaining = true;
                }
            }
        }
    });

    if !any_remaining {
        set_has_pending_dirty(false);
    }

    while out_mask != 0 {
        let bit = out_mask.trailing_zeros() as u16;
        out_mask &= out_mask - 1;
        mark_fn(PodId(bit));
    }
    for pod_idx in ext_out {
        mark_fn(PodId(pod_idx));
    }
}

#[inline]
pub(super) fn record_signal_read(sig_idx: u16, sig_gen: u16, sig_version: u32) {
    let (active_pool, active_pod) = get_active_render_context();
    let tracker = get_active_dep_tracker();
    if active_pod == NO_ACTIVE_POD && tracker == NO_ACTIVE_TRACKER {
        return;
    }

    with_table(|tbl| {
        if active_pod != NO_ACTIVE_POD {
            if let Some(slot) = tbl.signals.get_mut(sig_idx as usize) {
                if slot.occupied
                    && slot.generation == sig_gen
                    && !slot.add_subscriber(active_pool, active_pod)
                {
                    tbl.dropped_pool_subscriptions =
                        tbl.dropped_pool_subscriptions.saturating_add(1);
                }
            }
        }

        if tracker != NO_ACTIVE_TRACKER {
            let kind = tracker & 0xC000;
            let dst_idx = tracker & TRACKER_INDEX_MASK;
            if kind == TRACKER_KIND_COMPUTED {
                if let Some(dst_gen) = tbl
                    .computeds
                    .get(dst_idx as usize)
                    .filter(|c| c.occupied)
                    .map(|c| c.generation)
                {
                    tbl.link_dep(
                        NodeKind::Signal,
                        sig_idx,
                        sig_gen,
                        sig_version,
                        NodeKind::Computed,
                        dst_idx,
                        dst_gen,
                    );
                }
            } else if kind == TRACKER_KIND_EFFECT {
                if let Some(dst_gen) = tbl
                    .effects
                    .get(dst_idx as usize)
                    .filter(|e| e.occupied)
                    .map(|e| e.generation)
                {
                    tbl.link_dep(
                        NodeKind::Signal,
                        sig_idx,
                        sig_gen,
                        sig_version,
                        NodeKind::Effect,
                        dst_idx,
                        dst_gen,
                    );
                }
            }
        }
    });
}

#[inline]
pub(super) fn record_computed_read(comp_idx: u16, comp_gen: u16, comp_version: u32) {
    let (active_pool, active_pod) = get_active_render_context();
    let tracker = get_active_dep_tracker();
    if active_pod == NO_ACTIVE_POD && tracker == NO_ACTIVE_TRACKER {
        return;
    }

    with_table(|tbl| {
        if active_pod != NO_ACTIVE_POD {
            tbl.bind_computed_signals_to_pod(comp_idx, active_pool, active_pod);
        }

        if tracker != NO_ACTIVE_TRACKER {
            let kind = tracker & 0xC000;
            let dst_idx = tracker & TRACKER_INDEX_MASK;
            if kind == TRACKER_KIND_COMPUTED {
                if dst_idx != comp_idx {
                    if let Some(dst_gen) = tbl
                        .computeds
                        .get(dst_idx as usize)
                        .filter(|c| c.occupied)
                        .map(|c| c.generation)
                    {
                        tbl.link_dep(
                            NodeKind::Computed,
                            comp_idx,
                            comp_gen,
                            comp_version,
                            NodeKind::Computed,
                            dst_idx,
                            dst_gen,
                        );
                    }
                }
            } else if kind == TRACKER_KIND_EFFECT {
                if let Some(dst_gen) = tbl
                    .effects
                    .get(dst_idx as usize)
                    .filter(|e| e.occupied)
                    .map(|e| e.generation)
                {
                    tbl.link_dep(
                        NodeKind::Computed,
                        comp_idx,
                        comp_gen,
                        comp_version,
                        NodeKind::Effect,
                        dst_idx,
                        dst_gen,
                    );
                }
            }
        }
    });
}

/// Pull 阶段核心：确保指定 `Computed` 槽位处于最新 `Clean` 状态，返回其最新 `version`。
/// - `Clean`：$O(1)$ 直接返回；
/// - `Check`：验证所有上游 `Signal` / `Computed` 版本，若均未变则 **Early Cutoff** 剪枝直接置回 `Clean`；
/// - `Dirty` 或上游版本变化：摘除旧依赖边、重算并仅在结果真实变化时递增 `version`。
pub(super) fn ensure_computed_fresh(idx: usize, expected_gen: u16) -> Option<u32> {
    enum PullAction {
        Ready(u32),
        Check(EdgeIdx, u32), // (first_dep, cur_version)
        Recompute {
            compute_fn: unsafe fn(*const InlineFnStorage, *mut InlineValueStorage),
            fn_storage: InlineFnStorage,
            eq_fn: Option<unsafe fn(*const InlineValueStorage, *const InlineValueStorage) -> bool>,
            prev_initialized: bool,
            prev_cached: InlineValueStorage,
        },
    }

    let action = with_table(|tbl| {
        let slot = tbl.computeds.get_mut(idx)?;
        if !slot.occupied || slot.generation != expected_gen {
            return None;
        }
        assert!(
            !slot.computing,
            "Cyclic Computed dependency detected during evaluation"
        );
        if slot.initialized && slot.state == NodeState::Clean {
            return Some(PullAction::Ready(slot.version));
        }
        if slot.initialized && slot.state == NodeState::Check {
            return Some(PullAction::Check(slot.first_dep, slot.version));
        }
        // Dirty 或尚未初始化：准备重算
        let compute_fn = slot.compute_fn?;
        let fn_storage = slot.fn_storage;
        let eq_fn = slot.eq_fn;
        let prev_initialized = slot.initialized;
        let prev_cached = slot.cached;
        slot.computing = true;
        tbl.unlink_all_deps(NodeKind::Computed, idx as u16);
        Some(PullAction::Recompute {
            compute_fn,
            fn_storage,
            eq_fn,
            prev_initialized,
            prev_cached,
        })
    })?;

    let recompute_info = match action {
        PullAction::Ready(ver) => return Some(ver),
        PullAction::Recompute {
            compute_fn,
            fn_storage,
            eq_fn,
            prev_initialized,
            prev_cached,
        } => (compute_fn, fn_storage, eq_fn, prev_initialized, prev_cached),
        PullAction::Check(first_dep, cur_ver) => {
            let mut needs_recompute = false;
            let mut cur_edge = first_dep;
            while cur_edge != NO_EDGE {
                let step = with_table(|tbl| {
                    let e = tbl.edges.get(cur_edge as usize)?;
                    if !e.active {
                        return None;
                    }
                    let next = e.next_dep;
                    match e.src_kind {
                        NodeKind::Signal => {
                            let changed = tbl
                                .signals
                                .get(e.src_idx as usize)
                                .map(|s| {
                                    !s.occupied
                                        || s.generation != e.src_gen
                                        || s.version != e.seen_version
                                })
                                .unwrap_or(true);
                            Some((next, None, changed))
                        }
                        NodeKind::Computed => {
                            Some((next, Some((e.src_idx, e.src_gen, e.seen_version)), false))
                        }
                        NodeKind::Effect => Some((next, None, false)),
                    }
                });
                let Some((next, upstream_comp, sig_changed)) = step else {
                    needs_recompute = true;
                    break;
                };
                if sig_changed {
                    needs_recompute = true;
                    break;
                }
                if let Some((up_idx, up_gen, seen_ver)) = upstream_comp {
                    match ensure_computed_fresh(up_idx as usize, up_gen) {
                        Some(actual_ver) if actual_ver == seen_ver => {}
                        _ => {
                            needs_recompute = true;
                            break;
                        }
                    }
                }
                cur_edge = next;
            }

            if !needs_recompute {
                // Early Cutoff：所有上游最终版本均未改变，直接置回 Clean，零重算！
                with_table(|tbl| {
                    if let Some(slot) = tbl.computeds.get_mut(idx) {
                        if slot.occupied && slot.generation == expected_gen {
                            slot.state = NodeState::Clean;
                        }
                    }
                });
                return Some(cur_ver);
            }

            // 上游确有实质变化：正式摘除旧依赖边进入重算
            let extracted = with_table(|tbl| {
                let slot = tbl.computeds.get_mut(idx)?;
                if !slot.occupied || slot.generation != expected_gen {
                    return None;
                }
                let compute_fn = slot.compute_fn?;
                let fn_storage = slot.fn_storage;
                let eq_fn = slot.eq_fn;
                let prev_initialized = slot.initialized;
                let prev_cached = slot.cached;
                slot.computing = true;
                tbl.unlink_all_deps(NodeKind::Computed, idx as u16);
                Some((compute_fn, fn_storage, eq_fn, prev_initialized, prev_cached))
            })?;
            extracted
        }
    };

    let (compute_fn, fn_storage, eq_fn, prev_initialized, prev_cached) = recompute_info;
    // 执行计算闭包期间不持有 ReactiveGraphTable 借用；同时暂存外层 Pod 上下文以防重复混合绑定
    let prev_render = swap_active_render_context(0, NO_ACTIVE_POD);
    let prev_tracker =
        swap_active_dep_tracker(TRACKER_KIND_COMPUTED | ((idx as u16) & TRACKER_INDEX_MASK));
    let mut new_val = InlineValueStorage::ZERO;
    unsafe {
        compute_fn(&fn_storage, &mut new_val);
    }
    let _ = swap_active_dep_tracker(prev_tracker);
    let _ = swap_active_render_context(prev_render.0, prev_render.1);

    with_table(|tbl| {
        let slot = tbl.computeds.get_mut(idx)?;
        if !slot.occupied || slot.generation != expected_gen {
            return None;
        }
        let changed = if !prev_initialized {
            true
        } else if let Some(eq_f) = eq_fn {
            unsafe { !eq_f(&prev_cached, &new_val) }
        } else {
            true
        };
        slot.cached = new_val;
        slot.eval_count = slot.eval_count.wrapping_add(1);
        slot.initialized = true;
        slot.computing = false;
        slot.state = NodeState::Clean;
        if changed {
            slot.version = slot.version.wrapping_add(1).max(1);
        }
        Some(slot.version)
    })
}

/// 验证某个 `Effect` 的上游依赖（含经由 `Computed` 剪枝后的版本）是否发生实质变更。
pub(super) fn verify_effect_needs_run(eff_idx: usize) -> bool {
    let first_dep = with_table(|tbl| {
        tbl.effects
            .get(eff_idx)
            .filter(|e| e.occupied && !e.running)
            .map(|e| e.first_dep)
    });
    let Some(mut cur_edge) = first_dep else {
        return false;
    };

    while cur_edge != NO_EDGE {
        let step = with_table(|tbl| {
            let e = tbl.edges.get(cur_edge as usize)?;
            if !e.active {
                return None;
            }
            let next = e.next_dep;
            match e.src_kind {
                NodeKind::Signal => {
                    let changed = tbl
                        .signals
                        .get(e.src_idx as usize)
                        .map(|s| {
                            !s.occupied
                                || s.generation != e.src_gen
                                || s.version != e.seen_version
                        })
                        .unwrap_or(true);
                    Some((next, None, changed))
                }
                NodeKind::Computed => {
                    Some((next, Some((e.src_idx, e.src_gen, e.seen_version)), false))
                }
                NodeKind::Effect => Some((next, None, false)),
            }
        });
        let Some((next, upstream_comp, sig_changed)) = step else {
            return true;
        };
        if sig_changed {
            return true;
        }
        if let Some((up_idx, up_gen, seen_ver)) = upstream_comp {
            match ensure_computed_fresh(up_idx as usize, up_gen) {
                Some(actual_ver) if actual_ver == seen_ver => {}
                _ => return true,
            }
        }
        cur_edge = next;
    }

    false
}

pub(super) fn notify_signal_changed(has_subscribers: bool, effect_mask: u32) {
    if has_subscribers {
        set_has_pending_dirty(true);
    }
    if effect_mask != 0 {
        if get_batch_depth() > 0 {
            push_batch_effects(effect_mask);
        } else {
            flush_pending_effects(effect_mask);
        }
    }
}

pub(super) fn flush_pending_effects(mut rem: u32) {
    while rem != 0 {
        let eff_idx = rem.trailing_zeros() as usize;
        rem &= !(1u32 << eff_idx);
        if verify_effect_needs_run(eff_idx) {
            run_effect_slot(eff_idx);
        }
    }
}

pub(super) fn run_effect_slot(eff_idx: usize) {
    let extracted = with_table(|tbl| {
        let slot = tbl.effects.get_mut(eff_idx)?;
        if !slot.occupied || slot.running {
            return None;
        }
        let f = slot.run_fn?;
        let fn_storage = slot.fn_storage;
        slot.running = true;
        slot.run_count = slot.run_count.wrapping_add(1);
        tbl.unlink_all_deps(NodeKind::Effect, eff_idx as u16);
        Some((f, fn_storage))
    });

    let Some((run_fn, fn_storage)) = extracted else {
        return;
    };

    let prev_render = swap_active_render_context(0, NO_ACTIVE_POD);
    let prev_tracker = swap_active_dep_tracker(TRACKER_KIND_EFFECT | (eff_idx as u16));
    unsafe {
        run_fn(&fn_storage);
    }
    let _ = swap_active_dep_tracker(prev_tracker);
    let _ = swap_active_render_context(prev_render.0, prev_render.1);

    with_table(|tbl| {
        if let Some(slot) = tbl.effects.get_mut(eff_idx) {
            slot.running = false;
        }
    });
}
