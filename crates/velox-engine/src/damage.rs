//! 前后帧绘制指令对比器与 `DamageTracker` 定长脏矩形合并器。
//!
//! 最多维护 8 个独立脏矩形（`SmallVec<Region, 8>`）：
//! - 当新矩形与现有矩形重叠时，直接合并为并集（Union）；
//! - 当队列已满 8 个时，计算两两合并后的面积膨胀代价 $\text{Area}(A \cup B) - \text{Area}(A) - \text{Area}(B)$，
//!   将代价最小的两个矩形合并，确保零动态内存分配且绝不溢出。

use crate::arena::SmallVec;
use crate::motion::{DrawOp, DrawOpSnapshot};
use crate::solver::Region;

/// 定长包围盒自动聚合脏矩形追踪器（最大容量 8）。
#[derive(Clone, Debug, Default)]
pub struct DamageTracker {
    regions: SmallVec<Region, 8>,
}

impl DamageTracker {
    /// 创建空的脏矩形追踪器。
    #[inline]
    pub const fn new() -> Self {
        Self {
            regions: SmallVec::new(),
        }
    }

    /// 清空当前帧记录的所有脏矩形。
    #[inline]
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    /// 是否没有任何脏区域（画面完全静止）。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// 获取当前合并后的脏矩形切片（最多 8 个）。
    #[inline]
    pub fn regions(&self) -> &[Region] {
        self.regions.as_slice()
    }

    /// 向追踪器提交一个脏矩形区域，执行重叠合并或最小面积膨胀代价聚合。
    pub fn add_region(&mut self, reg: Region) {
        if reg.is_empty() {
            return;
        }

        // 1. 若与现有某个矩形重叠，合并为并集并级联归并
        let mut merged_idx = None;
        for (i, existing) in self.regions.iter_mut().enumerate() {
            if existing.overlaps_or_touches(reg) {
                *existing = existing.union(reg);
                merged_idx = Some(i);
                break;
            }
        }

        if merged_idx.is_some() {
            self.cascade_merge_overlaps();
            return;
        }

        // 2. 若队列未满 8 个，直接压入
        if self.regions.len() < 8 {
            self.regions.push(reg);
            return;
        }

        // 3. 队列已满 8 个：在 8 个现有矩形 + 1 个新矩形（共 9 个候选）中，
        // 寻找合并面积膨胀代价 Cost(A, B) = Area(A ∪ B) - Area(A) - Area(B) 最小的一对进行合并
        let mut candidates = [Region::EMPTY; 9];
        for (i, &r) in self.regions.iter().enumerate() {
            candidates[i] = r;
        }
        candidates[8] = reg;

        let mut best_i = 0usize;
        let mut best_j = 1usize;
        let mut min_cost = i32::MAX;

        for i in 0..9 {
            for j in (i + 1)..9 {
                let cost = merge_expansion_cost(candidates[i], candidates[j]);
                if cost < min_cost {
                    min_cost = cost;
                    best_i = i;
                    best_j = j;
                }
            }
        }

        let merged = candidates[best_i].union(candidates[best_j]);
        self.regions.clear();
        for (idx, &cand) in candidates.iter().enumerate() {
            if idx == best_i {
                self.regions.push(merged);
            } else if idx != best_j {
                self.regions.push(cand);
            }
        }
        self.cascade_merge_overlaps();
    }

    /// 对比前后两帧的 `DrawOp` 数组（带滑动窗口重同步，避免中间插入或删除单个节点导致后续整段全部标脏）。
    pub fn diff_ops(&mut self, prev_ops: &[DrawOp<'_>], curr_ops: &[DrawOp<'_>]) {
        const WINDOW: usize = 8;
        let mut pi = 0usize;
        let mut ci = 0usize;
        while pi < prev_ops.len() && ci < curr_ops.len() {
            if prev_ops[pi] == curr_ops[ci] {
                pi += 1;
                ci += 1;
                continue;
            }
            // 检查是否为 curr_ops 插入了新指令（prev_ops[pi] 出现在 curr_ops[ci + 1..ci + WINDOW]）
            let ins_offset = (1..=WINDOW.min(curr_ops.len() - ci - 1))
                .find(|&k| prev_ops[pi] == curr_ops[ci + k]);
            // 检查是否为 prev_ops 删除了旧指令（curr_ops[ci] 出现在 prev_ops[pi + 1..pi + WINDOW]）
            let del_offset = (1..=WINDOW.min(prev_ops.len() - pi - 1))
                .find(|&k| curr_ops[ci] == prev_ops[pi + k]);

            match (ins_offset, del_offset) {
                (Some(k_ins), Some(k_del)) if k_ins <= k_del => {
                    for k in 0..k_ins {
                        let c = &curr_ops[ci + k];
                        self.add_region(c.bounds.intersect(c.clip));
                    }
                    ci += k_ins;
                }
                (Some(k_ins), None) => {
                    for k in 0..k_ins {
                        let c = &curr_ops[ci + k];
                        self.add_region(c.bounds.intersect(c.clip));
                    }
                    ci += k_ins;
                }
                (_, Some(k_del)) => {
                    for k in 0..k_del {
                        let p = &prev_ops[pi + k];
                        self.add_region(p.bounds.intersect(p.clip));
                    }
                    pi += k_del;
                }
                (None, None) => {
                    let p = &prev_ops[pi];
                    let c = &curr_ops[ci];
                    self.add_region(p.bounds.intersect(p.clip));
                    self.add_region(c.bounds.intersect(c.clip));
                    pi += 1;
                    ci += 1;
                }
            }
        }
        while pi < prev_ops.len() {
            let p = &prev_ops[pi];
            self.add_region(p.bounds.intersect(p.clip));
            pi += 1;
        }
        while ci < curr_ops.len() {
            let c = &curr_ops[ci];
            self.add_region(c.bounds.intersect(c.clip));
            ci += 1;
        }
    }

    /// 对比上一帧的 `DrawOpSnapshot` 与当前帧的 `DrawOpSnapshot`（带滑动窗口重同步）。
    pub fn diff_snapshots(&mut self, prev_ops: &[DrawOpSnapshot], curr_ops: &[DrawOpSnapshot]) {
        const WINDOW: usize = 8;
        let mut pi = 0usize;
        let mut ci = 0usize;
        while pi < prev_ops.len() && ci < curr_ops.len() {
            if prev_ops[pi] == curr_ops[ci] {
                pi += 1;
                ci += 1;
                continue;
            }
            let ins_offset = (1..=WINDOW.min(curr_ops.len() - ci - 1))
                .find(|&k| prev_ops[pi] == curr_ops[ci + k]);
            let del_offset = (1..=WINDOW.min(prev_ops.len() - pi - 1))
                .find(|&k| curr_ops[ci] == prev_ops[pi + k]);

            match (ins_offset, del_offset) {
                (Some(k_ins), Some(k_del)) if k_ins <= k_del => {
                    for k in 0..k_ins {
                        self.add_region(curr_ops[ci + k].effective_bounds);
                    }
                    ci += k_ins;
                }
                (Some(k_ins), None) => {
                    for k in 0..k_ins {
                        self.add_region(curr_ops[ci + k].effective_bounds);
                    }
                    ci += k_ins;
                }
                (_, Some(k_del)) => {
                    for k in 0..k_del {
                        self.add_region(prev_ops[pi + k].effective_bounds);
                    }
                    pi += k_del;
                }
                (None, None) => {
                    self.add_region(prev_ops[pi].effective_bounds);
                    self.add_region(curr_ops[ci].effective_bounds);
                    pi += 1;
                    ci += 1;
                }
            }
        }
        while pi < prev_ops.len() {
            self.add_region(prev_ops[pi].effective_bounds);
            pi += 1;
        }
        while ci < curr_ops.len() {
            self.add_region(curr_ops[ci].effective_bounds);
            ci += 1;
        }
    }

    /// 原地流式比对并更新 `prev_snapshots`（Streaming In-Place Diff，带滑动窗口重同步）：
    /// 无需分配第二份 `curr_snapshots` 缓冲区，在比对 `curr_ops` 后直接更新 `prev_snapshots`。
    pub fn diff_and_update_in_place<const CAP: usize>(
        &mut self,
        prev_snapshots: &mut SmallVec<DrawOpSnapshot, CAP>,
        curr_ops: &[DrawOp<'_>],
        record_damage: bool,
    ) {
        if record_damage {
            const WINDOW: usize = 8;
            let prev_slice = prev_snapshots.as_slice();
            let mut pi = 0usize;
            let mut ci = 0usize;
            while pi < prev_slice.len() && ci < curr_ops.len() {
                let c_snap = curr_ops[ci].to_snapshot();
                if prev_slice[pi] == c_snap {
                    pi += 1;
                    ci += 1;
                    continue;
                }
                let ins_offset = (1..=WINDOW.min(curr_ops.len() - ci - 1))
                    .find(|&k| prev_slice[pi] == curr_ops[ci + k].to_snapshot());
                let del_offset = (1..=WINDOW.min(prev_slice.len() - pi - 1))
                    .find(|&k| c_snap == prev_slice[pi + k]);

                match (ins_offset, del_offset) {
                    (Some(k_ins), Some(k_del)) if k_ins <= k_del => {
                        for k in 0..k_ins {
                            self.add_region(curr_ops[ci + k].to_snapshot().effective_bounds);
                        }
                        ci += k_ins;
                    }
                    (Some(k_ins), None) => {
                        for k in 0..k_ins {
                            self.add_region(curr_ops[ci + k].to_snapshot().effective_bounds);
                        }
                        ci += k_ins;
                    }
                    (_, Some(k_del)) => {
                        for k in 0..k_del {
                            self.add_region(prev_slice[pi + k].effective_bounds);
                        }
                        pi += k_del;
                    }
                    (None, None) => {
                        if prev_slice[pi].effective_bounds == c_snap.effective_bounds {
                            if let crate::motion::DrawPayload::SceneCanvas(spec) =
                                curr_ops[ci].payload
                            {
                                if !spec.dirty_screen_rects.is_empty() {
                                    for &r in spec.dirty_screen_rects {
                                        let sub = r.intersect(c_snap.effective_bounds);
                                        if !sub.is_empty() {
                                            self.add_region(sub);
                                        }
                                    }
                                    pi += 1;
                                    ci += 1;
                                    continue;
                                }
                            }
                        }
                        self.add_region(prev_slice[pi].effective_bounds);
                        self.add_region(c_snap.effective_bounds);
                        pi += 1;
                        ci += 1;
                    }
                }
            }
            while pi < prev_slice.len() {
                self.add_region(prev_slice[pi].effective_bounds);
                pi += 1;
            }
            while ci < curr_ops.len() {
                self.add_region(curr_ops[ci].to_snapshot().effective_bounds);
                ci += 1;
            }
        }

        prev_snapshots.clear();
        for op in curr_ops {
            let _ = prev_snapshots.push(op.to_snapshot());
        }
    }

    fn cascade_merge_overlaps(&mut self) {
        loop {
            let mut merged_pair = None;
            let len = self.regions.len();
            'outer: for i in 0..len {
                for j in (i + 1)..len {
                    if self.regions[i].overlaps_or_touches(self.regions[j]) {
                        merged_pair = Some((i, j));
                        break 'outer;
                    }
                }
            }
            if let Some((i, j)) = merged_pair {
                let u = self.regions[i].union(self.regions[j]);
                self.regions[i] = u;
                self.regions.remove(j);
            } else {
                break;
            }
        }
    }
}

/// 计算两个矩形合并后的面积膨胀代价：$\text{Area}(A \cup B) - \text{Area}(A) - \text{Area}(B)$。
#[inline]
pub fn merge_expansion_cost(a: Region, b: Region) -> i32 {
    let union_area = a.union(b).area();
    union_area - a.area() - b.area()
}
