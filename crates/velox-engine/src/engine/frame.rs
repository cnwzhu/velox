//! 引擎帧推进管道：局部重排、Pass-4-Only 极速渲染路径、分数 DPI 条带光栅化与无障碍树生成。

use crate::arena::SmallVec;
use crate::display::PixelFormat;
use crate::motion::DrawOp;
use crate::pod::PodId;
use crate::renderers::Renderer;
use crate::solver::{
    reflatten_pass4_only, solve_and_flatten_retained_with_pool,
    try_relayout_dirty_subviews_in_place, HitRegion, LayoutNode, Region,
};

use super::{VeloxEngine, MAX_DRAW_OPS, MAX_HIT_REGIONS};

impl VeloxEngine {
    /// 推进一帧动画时钟（默认 `16ms`），执行布局求解、过渡插值与前后帧对比，返回本帧脏矩形切片。
    pub fn step_frame(&mut self) -> &[Region] {
        self.step_frame_dt(16)
    }

    /// 按给定帧间隔 `dt_ms`（毫秒）执行一帧更新并返回合并后的脏矩形列表。
    #[inline(never)]
    pub fn step_frame_dt(&mut self, dt_ms: u16) -> &[Region] {
        self.damage.clear();
        let now = self.frame_scope.now_ms.wrapping_add(dt_ms as u32);
        self.frame_scope.set_now_ms(now);
        let anim_running = self.motion.tick(dt_ms);

        // 处理按住状态下的长按与自动连发（使用可配置 GestureConfig 阈值）
        if self.interaction.is_pressed {
            if let Some((px, py)) = self.interaction.pointer_pos {
                let cfg = self.gesture_config;
                let prev_hold = self.press_hold_ms;
                self.press_hold_ms = self.press_hold_ms.saturating_add(dt_ms);
                if self.press_hold_ms >= cfg.long_press_ms && !self.long_press_fired {
                    self.long_press_fired = true;
                    if self.trigger_hold_action(px, py, true) {
                        self.long_press_handled = true;
                    }
                }
                if self.press_hold_ms >= cfg.repeat_delay_ms {
                    let eff_dt = if prev_hold < cfg.repeat_delay_ms {
                        self.press_hold_ms - cfg.repeat_delay_ms
                    } else {
                        dt_ms
                    };
                    let interval = cfg.repeat_interval_ms.max(1);
                    self.repeat_accum_ms = self.repeat_accum_ms.saturating_add(eff_dt);
                    while self.repeat_accum_ms >= interval {
                        self.repeat_accum_ms -= interval;
                        self.trigger_hold_action(px, py, false);
                    }
                }
            }
        }

        {
            let _ = self.poll_async_tasks();
        }

        self.pool.flush_signal_dirty_pods();

        if !self.first_frame
            && !self.interaction_dirty
            && !self.frame_scope.frame_requested.get()
            && !self.pool.has_dirty()
            && !anim_running
            && !self.motion.has_active_animations()
        {
            return self.damage.regions();
        }

        let Some(root_id) = self.root_pod else {
            return self.damage.regions();
        };

        let record_diff = !self.first_frame;
        self.interaction.focus_ring_color = self.frame_scope.theme().ring;
        let interaction = self.interaction;

        // Pass-4-Only 快径：拆分为 `#[inline(never)]` 子函数隔离局部 `SmallVec<DrawOp>`/`SmallVec<HitRegion>` 栈帧，
        // 避免编译器将其与后续全量求解路径的双份大数组叠加在同一栈帧中（降低 `#![no_std]` MCU 峰值栈深约 8 KB）。
        if !self.first_frame
            && !self.pool.has_dirty()
            && !self.frame_scope.frame_requested.get()
            && !self.cached_layout_ptr.is_null()
        {
            self.run_pass4_only_frame(interaction, record_diff);
            return self.damage.regions();
        }

        // 判断是否可在 ScratchArena 中增量保留未脏的 SubView Element 树与 LayoutNode 树：
        // `PodPool` 已按槽位精确追踪 `cx.request_frame()`（`STATE_FLAG_NEEDS_NEXT_FRAME`），
        // 因此只要脏视图已入队（`pool.has_dirty()`），即便子视图包含连续动画（如闪烁光标 / Spinner），
        // 依然可以保留未脏的根视图与兄弟子视图缓存，仅原地局部重排该动画子视图！
        const SUBVIEW_RETAIN_WATERMARK: usize = 512 * 1024;

        let frame_requested = self.frame_scope.frame_requested.get();
        let can_retain_subviews = if !self.first_frame
            && (!frame_requested || self.pool.has_dirty())
            && self.tree_temp_mark > 0
            && self.pool.has_any_clean_occupied()
        {
            self.frame_scope
                .arena_mut()
                .restore_mark(self.tree_cursor_mark as usize);
            self.frame_scope
                .arena_mut()
                .restore_temp_mark(self.tree_temp_mark as usize);
            (self.tree_cursor_mark as usize) < SUBVIEW_RETAIN_WATERMARK
                && self.frame_scope.arena().real_allocated_bytes() < SUBVIEW_RETAIN_WATERMARK
                && self.frame_scope.arena().can_alloc_slice::<u8>(1536)
                && self.frame_scope.arena().can_alloc_temp_slice::<u8>(1536)
        } else {
            false
        };

        let prev_layout_ptr = if can_retain_subviews && !self.pool.is_dirty(root_id) {
            self.cached_layout_ptr
        } else {
            core::ptr::null_mut()
        };
        let mut dirty_subview_ids: SmallVec<PodId, 8> = SmallVec::new();
        let can_try_in_place = if !prev_layout_ptr.is_null() {
            let slice = self.pool.dirty_pods();
            if !slice.is_empty() && slice.len() <= 8 {
                for &id in slice {
                    let _ = dirty_subview_ids.push(id);
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        if !can_retain_subviews {
            self.frame_scope.arena_mut().rewind();
            self.pool.invalidate_all_cached_elements();
        }
        self.invalidate_frame_cache();
        self.cached_layout_ptr = core::ptr::null_mut();
        // 清除上一帧的连续动画请求（本帧渲染期间视图可通过 `cx.request_frame()` 重新请求）
        self.frame_scope.frame_requested.set(false);

        if let Some((root_ref, arena)) =
            self.pool.render_pod_in_arena(root_id, &mut self.frame_scope)
        {
            let mut ops: SmallVec<DrawOp<'_>, MAX_DRAW_OPS> = SmallVec::new();
            let mut hits: SmallVec<HitRegion<'_>, MAX_HIT_REGIONS> = SmallVec::new();
            let in_place_ok = can_try_in_place
                && try_relayout_dirty_subviews_in_place(
                    arena,
                    &self.pool,
                    dirty_subview_ids.as_slice(),
                );
            let (layout_ptr, dropped) = if in_place_ok {
                let d = unsafe {
                    let root_node = &mut *(prev_layout_ptr as *mut LayoutNode<'_, '_>);
                    reflatten_pass4_only(
                        arena,
                        root_node,
                        self.viewport,
                        interaction,
                        &mut self.motion,
                        &mut ops,
                        &mut hits,
                    )
                };
                self.partial_relayout_count = self.partial_relayout_count.wrapping_add(1);
                (prev_layout_ptr, d)
            } else {
                let (lp, d) = solve_and_flatten_retained_with_pool(
                    arena,
                    root_ref,
                    Some(&self.pool),
                    self.viewport,
                    interaction,
                    &mut self.motion,
                    &mut ops,
                    &mut hits,
                );
                self.solve_count = self.solve_count.wrapping_add(1);
                (lp, d)
            };
            self.dropped_draw_ops = self.dropped_draw_ops.saturating_add(dropped);
            self.tree_cursor_mark = arena.save_mark() as u32;
            self.tree_temp_mark = arena.save_temp_mark() as u32;

            let needed_tail = ops
                .len()
                .saturating_mul(core::mem::size_of::<DrawOp<'static>>())
                .saturating_add(
                    hits.len()
                        .saturating_mul(core::mem::size_of::<HitRegion<'static>>()),
                )
                .saturating_add(32);
            if arena.can_alloc_slice::<u8>(needed_tail) {
                self.cached_layout_ptr = layout_ptr;
            } else {
                unsafe {
                    arena.reset_temp();
                }
                self.pool.invalidate_all_cached_elements();
                self.cached_layout_ptr = core::ptr::null_mut();
                self.tree_temp_mark = 0;
            }
            self.ops_cursor_mark = arena.save_mark() as u32;

            // 原地流式对比并覆写 prev_snapshots（省去 curr_snapshots 第二份缓冲）
            self.damage.diff_and_update_in_place(
                &mut self.prev_snapshots,
                ops.as_slice(),
                record_diff,
            );
            // 将本帧求解并预计算遮挡裁剪后的 DrawOp 与 HitRegion 切片缓存，供后续条带光栅化与 O(1) 命中测试零拷贝复用
            Self::cache_ops_and_hits_into(
                arena,
                ops.as_mut_slice(),
                hits.as_slice(),
                &mut self.cached_ops_buf,
                &mut self.cached_hits_buf,
                &mut self.cached_ops_ptr,
                &mut self.cached_ops_len,
                &mut self.cached_hits_ptr,
                &mut self.cached_hits_len,
                &mut self.dropped_draw_ops,
                &mut self.frame_cached,
                &mut self.last_arena_used_bytes,
            );
        }

        if self.first_frame {
            self.damage.add_region(self.viewport);
            self.first_frame = false;
        }
        self.interaction_dirty = false;
        self.pool.clear_dirty();

        self.damage.regions()
    }

    /// 独立隔离栈帧的 Pass-4-Only 快路径，避免与全量求解分支的 `SmallVec` 缓冲叠加导致 MCU 栈峰翻倍。
    #[inline(never)]
    fn run_pass4_only_frame(
        &mut self,
        interaction: crate::solver::InteractionState,
        record_diff: bool,
    ) {
        self.frame_scope
            .arena_mut()
            .restore_mark(self.ops_cursor_mark as usize);
        let arena = self.frame_scope.arena();
        let mut ops: SmallVec<DrawOp<'_>, MAX_DRAW_OPS> = SmallVec::new();
        let mut hits: SmallVec<HitRegion<'_>, MAX_HIT_REGIONS> = SmallVec::new();
        let dropped = unsafe {
            reflatten_pass4_only(
                arena,
                &mut *(self.cached_layout_ptr as *mut LayoutNode<'_, '_>),
                self.viewport,
                interaction,
                &mut self.motion,
                &mut ops,
                &mut hits,
            )
        };
        self.dropped_draw_ops = self.dropped_draw_ops.saturating_add(dropped);
        self.fast_paint_count = self.fast_paint_count.wrapping_add(1);

        self.damage
            .diff_and_update_in_place(&mut self.prev_snapshots, ops.as_slice(), record_diff);
        Self::cache_ops_and_hits_into(
            arena,
            ops.as_mut_slice(),
            hits.as_slice(),
            &mut self.cached_ops_buf,
            &mut self.cached_hits_buf,
            &mut self.cached_ops_ptr,
            &mut self.cached_ops_len,
            &mut self.cached_hits_ptr,
            &mut self.cached_hits_len,
            &mut self.dropped_draw_ops,
            &mut self.frame_cached,
            &mut self.last_arena_used_bytes,
        );
        self.interaction_dirty = false;
    }

    #[inline(never)]
    pub(super) fn ensure_frame_cached(&mut self) {
        let Some(root_id) = self.root_pod else {
            return;
        };
        self.pool.flush_signal_dirty_pods();
        self.interaction.focus_ring_color = self.frame_scope.theme().ring;
        let interaction = self.interaction;

        if !self.pool.has_dirty() && !self.cached_layout_ptr.is_null() {
            self.run_pass4_only_frame(interaction, false);
            return;
        }

        const SUBVIEW_RETAIN_WATERMARK: usize = 512 * 1024;

        let can_retain_subviews = if !self.first_frame
            && self.tree_temp_mark > 0
            && self.pool.has_any_clean_occupied()
        {
            self.frame_scope
                .arena_mut()
                .restore_mark(self.tree_cursor_mark as usize);
            self.frame_scope
                .arena_mut()
                .restore_temp_mark(self.tree_temp_mark as usize);
            (self.tree_cursor_mark as usize) < SUBVIEW_RETAIN_WATERMARK
                && self.frame_scope.arena().real_allocated_bytes() < SUBVIEW_RETAIN_WATERMARK
                && self.frame_scope.arena().can_alloc_slice::<u8>(1536)
                && self.frame_scope.arena().can_alloc_temp_slice::<u8>(1536)
        } else {
            false
        };

        let prev_layout_ptr = if can_retain_subviews && !self.pool.is_dirty(root_id) {
            self.cached_layout_ptr
        } else {
            core::ptr::null_mut()
        };
        let mut dirty_subview_ids: SmallVec<PodId, 8> = SmallVec::new();
        let can_try_in_place = if !prev_layout_ptr.is_null() {
            let slice = self.pool.dirty_pods();
            if !slice.is_empty() && slice.len() <= 8 {
                for &id in slice {
                    let _ = dirty_subview_ids.push(id);
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        if !can_retain_subviews {
            self.invalidate_layout_cache();
            self.frame_scope.arena_mut().rewind();
            self.pool.invalidate_all_cached_elements();
        } else {
            self.invalidate_frame_cache();
            self.cached_layout_ptr = core::ptr::null_mut();
        }
        if let Some((root_ref, arena)) =
            self.pool.render_pod_in_arena(root_id, &mut self.frame_scope)
        {
            let mut ops: SmallVec<DrawOp<'_>, MAX_DRAW_OPS> = SmallVec::new();
            let mut hits: SmallVec<HitRegion<'_>, MAX_HIT_REGIONS> = SmallVec::new();
            let in_place_ok = can_try_in_place
                && try_relayout_dirty_subviews_in_place(
                    arena,
                    &self.pool,
                    dirty_subview_ids.as_slice(),
                );
            let (layout_ptr, dropped) = if in_place_ok {
                let d = unsafe {
                    let root_node = &mut *(prev_layout_ptr as *mut LayoutNode<'_, '_>);
                    reflatten_pass4_only(
                        arena,
                        root_node,
                        self.viewport,
                        interaction,
                        &mut self.motion,
                        &mut ops,
                        &mut hits,
                    )
                };
                self.partial_relayout_count = self.partial_relayout_count.wrapping_add(1);
                (prev_layout_ptr, d)
            } else {
                let (lp, d) = solve_and_flatten_retained_with_pool(
                    arena,
                    root_ref,
                    Some(&self.pool),
                    self.viewport,
                    interaction,
                    &mut self.motion,
                    &mut ops,
                    &mut hits,
                );
                self.solve_count = self.solve_count.wrapping_add(1);
                (lp, d)
            };
            self.dropped_draw_ops = self.dropped_draw_ops.saturating_add(dropped);
            self.tree_cursor_mark = arena.save_mark() as u32;
            self.tree_temp_mark = arena.save_temp_mark() as u32;
            let needed_tail = ops
                .len()
                .saturating_mul(core::mem::size_of::<DrawOp<'static>>())
                .saturating_add(
                    hits.len()
                        .saturating_mul(core::mem::size_of::<HitRegion<'static>>()),
                )
                .saturating_add(32);
            if arena.can_alloc_slice::<u8>(needed_tail) {
                self.cached_layout_ptr = layout_ptr;
            } else {
                unsafe {
                    arena.reset_temp();
                }
                self.pool.invalidate_all_cached_elements();
                self.cached_layout_ptr = core::ptr::null_mut();
                self.tree_temp_mark = 0;
            }
            self.ops_cursor_mark = arena.save_mark() as u32;
            Self::cache_ops_and_hits_into(
                arena,
                ops.as_mut_slice(),
                hits.as_slice(),
                &mut self.cached_ops_buf,
                &mut self.cached_hits_buf,
                &mut self.cached_ops_ptr,
                &mut self.cached_ops_len,
                &mut self.cached_hits_ptr,
                &mut self.cached_hits_len,
                &mut self.dropped_draw_ops,
                &mut self.frame_cached,
                &mut self.last_arena_used_bytes,
            );
            self.interaction_dirty = false;
            self.pool.clear_dirty();
        }
    }

    #[inline]
    pub(super) fn ensure_hits_available(&mut self) {
        if !self.frame_cached {
            self.ensure_frame_cached();
        }
    }

    #[inline]
    pub(super) fn cached_hit_regions<'a>(&self) -> &'a [HitRegion<'a>] {
        if !self.frame_cached || self.cached_hits_len == 0 || self.cached_hits_ptr.is_null() {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(
                    self.cached_hits_ptr as *const HitRegion<'a>,
                    self.cached_hits_len as usize,
                )
            }
        }
    }

    /// 获取当前帧已求解并缓存在 `ScratchArena` 中的交互命中区域切片（生命周期绑定 `&self`）。
    #[inline]
    pub fn hit_regions<'a>(&'a self) -> &'a [HitRegion<'a>] {
        self.cached_hit_regions()
    }

    /// 构建当前帧的无障碍语义树（Accessibility Semantic Tree，兼容 AccessKit / AT-SPI 角色模型）。
    pub fn build_a11y_tree<'a, const CAP: usize>(
        &'a mut self,
    ) -> SmallVec<crate::a11y::A11yNode<'a>, CAP> {
        self.ensure_hits_available();
        let vp = self.viewport;
        let focused = self.interaction.focused_index;
        let hits = self.hit_regions();
        let ops = self.cached_draw_ops();
        crate::a11y::collect_a11y_nodes(vp, hits, ops, focused)
    }

    /// 确保当前帧的扁平化绘制指令缓存有效（若尚未布局或存在待处理脏状态则立即就地刷新，不推进动画时钟）。
    #[allow(dead_code)]
    pub(crate) fn ensure_frame_ops_ready(&mut self) {
        if self.root_pod.is_none() {
            return;
        }
        if !self.frame_cached
            || self.cached_ops_len == 0
            || self.pool.has_external_dirty()
            || self.interaction_dirty
            || crate::signal::has_pending_dirty_pods()
        {
            self.ensure_frame_cached();
            if self.first_frame {
                self.first_frame = false;
            }
            self.pool.clear_dirty();
        }
    }

    #[inline(always)]
    fn round_scaled_i16(val: i16, sf: f32) -> i16 {
        let v = (val as f32) * sf;
        let r = if v >= 0.0 { (v + 0.5) as i32 } else { (v - 0.5) as i32 };
        r.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }


    /// 获取当前帧已求解并缓存在 `ScratchArena` 中的扁平化 [`DrawOp`] 切片（供各类 [`Renderer`] 后端读取）。
    #[inline]
    pub fn cached_draw_ops<'a>(&'a self) -> &'a [DrawOp<'a>] {
        if !self.frame_cached || self.cached_ops_len == 0 || self.cached_ops_ptr.is_null() {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(
                    self.cached_ops_ptr as *const DrawOp<'a>,
                    self.cached_ops_len as usize,
                )
            }
        }
    }

    /// 推进一帧动画与布局求解（`dt_ms`），并将生成的 `DrawOp` 流与脏矩形列表提交给任意 [`Renderer`] 后端执行绘制。
    pub fn render_with_renderer<R: Renderer + ?Sized>(
        &mut self,
        renderer: &mut R,
        dt_ms: u16,
    ) -> (usize, usize) {
        let mut dirty: SmallVec<Region, 8> = SmallVec::new();
        for &r in self.step_frame_dt(dt_ms) {
            dirty.push(r);
        }
        if dirty.is_empty() {
            return (0, 0);
        }
        let vp = self.viewport;
        let scale = self.dpi_scale;
        let ops = self.cached_draw_ops();
        renderer.render_draw_ops(ops, vp, scale, dirty.as_slice())
    }

    /// 将指定区域 `reg` 内的绘制指令光栅化到全屏像素缓冲区 `pixels` 中。
    pub fn rasterize_region<P: PixelFormat>(&mut self, reg: Region, pixels: &mut [P]) {
        let stride = Self::round_scaled_i16(self.viewport.w, self.scale_factor).max(1) as usize;
        self.rasterize_strip(reg, 0, 0, stride, pixels);
    }

    /// 将指定区域 `reg` 光栅化到具有自定义起点 `(origin_x, origin_y)` 和行跨度 `stride` 的局部/条带缓冲区中。
    /// 若当前帧已通过 `step_frame_dt` 求解缓存，则直接零拷贝复用 `ScratchArena` 中的 `DrawOp` 切片，避免重复执行 `View::render()` 与布局求解。
    pub fn rasterize_strip<P: PixelFormat>(
        &mut self,
        reg: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) {
        if self.root_pod.is_none() {
            return;
        }

        if !self.frame_cached
            || self.pool.has_external_dirty()
            || self.interaction_dirty
            || crate::signal::has_pending_dirty_pods()
        {
            self.ensure_frame_cached();
        }

        if !self.frame_cached {
            return;
        }

        let sf = self.scale_factor;
        let vp = self.viewport;
        let cached_ops = self.cached_draw_ops();

        {
            crate::renderers::software::rasterize_region_scaled_parallel(
                cached_ops, vp, sf, reg, origin_x, origin_y, stride, target,
            );
        }
    }

}
