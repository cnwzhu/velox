//! 跨帧状态暂存、过渡动画插值池（`MotionCache`）与滚动偏移记忆。

use crate::arena::SmallVec;
use crate::element::TransitionSpec;
use crate::solver::Region;
use crate::style::{Color, Easing, LoopMode};

/// 跨帧过渡动画可插值的视觉状态（几何边界、背景色、边框色与整体不透明度）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct MotionState {
    /// 屏幕绝对坐标矩形。
    pub bounds: Region,
    /// 背景色。
    pub bg: Color,
    /// 边框色（文本节点复用为文本颜色之外的占位）。
    pub border: Color,
    /// 容器及其子树整体不透明度（`0..=255`）。
    pub opacity: u8,
}

impl MotionState {
    /// 构造一个完全不透明的视觉状态。
    #[inline(always)]
    pub const fn new(bounds: Region, bg: Color, border: Color) -> Self {
        Self {
            bounds,
            bg,
            border,
            opacity: 255,
        }
    }

    /// 设置整体不透明度。
    #[inline(always)]
    pub const fn with_opacity(mut self, opacity: u8) -> Self {
        self.opacity = opacity;
        self
    }

    #[inline]
    fn lerp(self, to: Self, t_256: i32) -> Self {
        let t_clamped = t_256.clamp(0, 256) as u16;
        let op = self.opacity as i32 + (((to.opacity as i32 - self.opacity as i32) * t_256) >> 8);
        Self {
            bounds: lerp_region_signed(self.bounds, to.bounds, t_256),
            bg: self.bg.lerp(to.bg, t_clamped),
            border: self.border.lerp(to.border, t_clamped),
            opacity: op.clamp(0, 255) as u8,
        }
    }
}

/// 单个元素的活跃过渡动画槽位（扁平紧凑布局，严格 52 字节以满足 MCU 静态 RAM 预算；
/// 记录 `path_hash` 与父节点布局原点 `(parent_x, parent_y)`，既能免疫跨容器重复 `id` 抢占，
/// 也能在父容器因滚动、侧边栏折叠、窗口缩放或兄弟重排发生刚性平移时自动同步跟随，避免子元素动画脱离父容器；
/// 跨容器共享元素过渡时 `path_hash` 变化则保留屏幕绝对坐标平滑飞跃）。
#[derive(Copy, Clone, Debug)]
struct ActiveTransition {
    id: u32,
    path_hash: u32,
    start_bounds: Region,
    target_bounds: Region,
    start_bg: Color,
    target_bg: Color,
    start_border: Color,
    target_border: Color,
    /// 同父容器下的上一帧父级布局原点（用于区分父容器刚性平移与元素自身相对位移）。
    parent_x: i16,
    parent_y: i16,
    /// 本轮已流逝时长（毫秒）；负值表示仍处于 `delay` 延迟段（循环动画仅首轮延迟）。
    elapsed_ms: i16,
    /// 单轮时长（毫秒，上限 `i16::MAX`）。
    duration_ms: u16,
    /// 低 3 位 `Easing`，第 3~4 位 `LoopMode`，第 5 位往复方向位，第 6 位本帧访问标记。
    mode_flags: u8,
    start_opacity: u8,
    target_opacity: u8,
}

const FLAG_REVERSE: u8 = 0x20;
const FLAG_SEEN: u8 = 0x40;

impl ActiveTransition {
    #[inline(always)]
    const fn easing(&self) -> Easing {
        Easing::from_bits(self.mode_flags)
    }

    #[inline(always)]
    const fn loop_mode(&self) -> LoopMode {
        match (self.mode_flags >> 3) & 0x03 {
            1 => LoopMode::Infinite,
            2 => LoopMode::PingPong,
            _ => LoopMode::Once,
        }
    }

    #[inline(always)]
    const fn is_reverse(&self) -> bool {
        (self.mode_flags & FLAG_REVERSE) != 0
    }

    #[inline(always)]
    const fn seen(&self) -> bool {
        (self.mode_flags & FLAG_SEEN) != 0
    }

    #[inline(always)]
    fn set_seen(&mut self, seen: bool) {
        if seen {
            self.mode_flags |= FLAG_SEEN;
        } else {
            self.mode_flags &= !FLAG_SEEN;
        }
    }

    /// 更新缓动与循环模式（保留方向位与访问标记）。
    #[inline(always)]
    fn set_mode(&mut self, easing: Easing, loop_mode: LoopMode) {
        self.mode_flags = (easing as u8 & 0x07)
            | ((loop_mode as u8 & 0x03) << 3)
            | (self.mode_flags & (FLAG_REVERSE | FLAG_SEEN));
    }

    #[inline(always)]
    const fn clamp_duration(ms: u16) -> u16 {
        if ms > i16::MAX as u16 { i16::MAX as u16 } else { ms }
    }

    #[inline(always)]
    const fn delay_to_elapsed(delay_ms: u16) -> i16 {
        if delay_ms > i16::MAX as u16 {
            i16::MIN + 1
        } else {
            -(delay_ms as i16)
        }
    }

    #[inline(always)]
    const fn is_active(&self) -> bool {
        if self.duration_ms == 0 {
            return false;
        }
        !matches!(self.loop_mode(), LoopMode::Once) || (self.elapsed_ms as i32) < self.duration_ms as i32
    }

    #[inline(always)]
    fn start_state(&self) -> MotionState {
        MotionState {
            bounds: self.start_bounds,
            bg: self.start_bg,
            border: self.start_border,
            opacity: self.start_opacity,
        }
    }

    #[inline(always)]
    fn target_state(&self) -> MotionState {
        MotionState {
            bounds: self.target_bounds,
            bg: self.target_bg,
            border: self.target_border,
            opacity: self.target_opacity,
        }
    }

    #[inline(always)]
    fn set_start(&mut self, s: MotionState) {
        self.start_bounds = s.bounds;
        self.start_bg = s.bg;
        self.start_border = s.border;
        self.start_opacity = s.opacity;
    }

    #[inline(always)]
    fn set_target(&mut self, s: MotionState) {
        self.target_bounds = s.bounds;
        self.target_bg = s.bg;
        self.target_border = s.border;
        self.target_opacity = s.opacity;
    }

    /// 立即结束动画并停留在目标状态。
    #[inline]
    fn settle(&mut self, target: MotionState) {
        self.set_start(target);
        self.set_target(target);
        self.elapsed_ms = self.duration_ms as i16;
        self.mode_flags &= 0x07 | FLAG_SEEN;
    }

    #[inline]
    fn current_state(&self) -> MotionState {
        if self.duration_ms == 0
            || (matches!(self.loop_mode(), LoopMode::Once)
                && (self.elapsed_ms as i32) >= self.duration_ms as i32)
        {
            return self.target_state();
        }
        let phase = self.elapsed_ms.max(0) as u32;
        let dur = self.duration_ms as u32;
        let eff = if self.is_reverse() {
            dur.saturating_sub(phase)
        } else {
            phase
        };
        let t_256 = self.easing().sample_256(eff, dur);
        self.start_state().lerp(self.target_state(), t_256)
    }
}

/// 滚动容器跨帧状态条目（12 字节紧凑布局，支持触摸惯性滑动衰减；`max_scroll < 0` 表示尚未经布局解析真实上限）。
#[derive(Copy, Clone, Debug)]
struct ScrollEntry {
    id: u32,
    offset_y: i16,
    velocity_y: i16,
    max_scroll: i16,
}

/// 跨帧状态与过渡动画插值池（`MotionCache`）。
///
/// 维护活跃属性过渡动画（`no_std` 下最多 16 个，`std` 下按需扩容；每帧求解后自动回收
/// 已从元素树中消失的槽位）以及虚拟滚动列表的滚动偏移量与惯性速度。
#[derive(Debug)]
pub struct MotionCache {
    transitions: SmallVec<ActiveTransition, 16>,
    scroll_offsets: SmallVec<ScrollEntry, 16>,
    /// 前 16 个滚动槽位的陈旧帧计数（每槽位 2 bit，上限 3），零 8B 对齐膨胀。
    scroll_stale_bits: u32,
}

impl Default for MotionCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MotionCache {
    /// 创建空的跨帧状态缓存池。
    pub const fn new() -> Self {
        Self {
            transitions: SmallVec::new(),
            scroll_offsets: SmallVec::new(),
            scroll_stale_bits: 0,
        }
    }

    #[inline(always)]
    fn stale_at(&self, idx: usize) -> u8 {
        if idx < 16 {
            ((self.scroll_stale_bits >> (idx * 2)) & 0x03) as u8
        } else {
            0
        }
    }

    #[inline(always)]
    fn set_stale_at(&mut self, idx: usize, val: u8) {
        if idx < 16 {
            let shift = idx * 2;
            self.scroll_stale_bits = (self.scroll_stale_bits & !(0x03u32 << shift))
                | (((val.min(3)) as u32) << shift);
        }
    }

    /// 推进所有活跃过渡动画与惯性滚动的时间戳（`dt_ms` 毫秒）。
    /// 返回本帧是否有动画或惯性滚动发生步进。
    pub fn tick(&mut self, dt_ms: u16) -> bool {
        let mut any_stepped = false;
        for tr in self.transitions.iter_mut() {
            if tr.is_active() {
                any_stepped = true;
                let dur = tr.duration_ms as i32;
                let next = tr.elapsed_ms as i32 + dt_ms as i32;
                if next >= dur {
                    match tr.loop_mode() {
                        LoopMode::Once => {
                            tr.elapsed_ms = dur as i16;
                        }
                        LoopMode::Infinite => {
                            tr.elapsed_ms = (next % dur.max(1)) as i16;
                        }
                        LoopMode::PingPong => {
                            tr.elapsed_ms = (next % dur.max(1)) as i16;
                            tr.mode_flags ^= FLAG_REVERSE;
                        }
                    }
                } else {
                    tr.elapsed_ms = next as i16;
                }
            }
        }

        // 推进触摸惯性滚动速度衰减（指数阻尼，按 dt_ms 相对 16ms 基准帧缩放）
        for sc in self.scroll_offsets.iter_mut() {
            if sc.velocity_y != 0 {
                any_stepped = true;
                let max_s = if sc.max_scroll >= 0 {
                    sc.max_scroll
                } else {
                    i16::MAX
                };
                let step = (((sc.velocity_y as i32) * (dt_ms.max(1) as i32)) / 16)
                    .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                let delta = if step != 0 {
                    step
                } else if sc.velocity_y > 0 {
                    1
                } else {
                    -1
                };
                let next_off = sc.offset_y.saturating_add(delta).clamp(0, max_s);
                if next_off == sc.offset_y {
                    sc.velocity_y = 0;
                } else {
                    sc.offset_y = next_off;
                    sc.velocity_y = (((sc.velocity_y as i32) * 7) / 8) as i16;
                    if sc.velocity_y.abs() <= 1 {
                        sc.velocity_y = 0;
                    }
                }
            }
        }
        any_stepped
    }

    /// 是否存在正在进行的过渡动画或惯性滚动。
    #[inline]
    pub fn has_active_animations(&self) -> bool {
        self.transitions.iter().any(|t| t.is_active())
            || self.scroll_offsets.iter().any(|s| s.velocity_y != 0)
    }

    /// 当前占用的过渡动画槽位数量（用于容量监控与测试）。
    #[inline]
    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// 帧求解开始：清除所有槽位的访问标记（由布局求解器自动调用）。
    pub fn begin_frame(&mut self) {
        for tr in self.transitions.iter_mut() {
            tr.set_seen(false);
        }
        let n = self.scroll_offsets.len().min(16);
        for idx in 0..n {
            let next = self.stale_at(idx).saturating_add(1);
            self.set_stale_at(idx, next);
        }
    }

    /// 帧求解结束：回收本帧未被访问（已从元素树中移除）的过渡动画与长期陈旧滚动槽位，防止槽位泄漏与容量耗尽。
    pub fn end_frame(&mut self) {
        let mut i = 0;
        while i < self.transitions.len() {
            if self.transitions[i].seen() {
                i += 1;
            } else {
                self.transitions.swap_remove(i);
            }
        }
        let mut s_idx = 0;
        while s_idx < self.scroll_offsets.len() {
            let stale = self.stale_at(s_idx);
            let sc = &self.scroll_offsets[s_idx];
            let evict = sc.velocity_y == 0
                && (stale >= 3
                    || (stale >= 2 && (sc.offset_y == 0 || self.scroll_offsets.len() > 12)));
            if evict {
                let last_idx = self.scroll_offsets.len() - 1;
                let last_stale = self.stale_at(last_idx);
                self.scroll_offsets.swap_remove(s_idx);
                self.set_stale_at(s_idx, last_stale);
                self.set_stale_at(last_idx, 0);
            } else {
                s_idx += 1;
            }
        }
    }

    /// 由布局求解器在 Pass 4 同步当前可见滚动容器的真实滚动上限，并重置陈旧计数。
    #[inline]
    pub(crate) fn sync_scroll_bounds(&mut self, id: u32, max_scroll: i16) {
        let clamped_max = max_scroll.max(0);
        if let Some((idx, entry)) = self
            .scroll_offsets
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.id == id)
        {
            entry.max_scroll = clamped_max;
            entry.offset_y = entry.offset_y.clamp(0, clamped_max);
            self.set_stale_at(idx, 0);
        }
    }

    /// 立即结束所有单次过渡动画并停留在各自目标状态（用于“减少动态效果”无障碍设置或测试快进）。
    pub fn finish_all(&mut self) {
        for tr in self.transitions.iter_mut() {
            if !matches!(tr.loop_mode(), LoopMode::Once) {
                continue;
            }
            let target = tr.target_state();
            tr.settle(target);
        }
    }

    /// 只读查询指定 ID 当前插值中的视觉状态（不推进时间、不更新访问标记）。
    pub fn current_state(&self, id: u32) -> Option<MotionState> {
        self.transitions
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.current_state())
    }

    /// 查询指定 `(id, path_hash)` 当前插值位置相对布局目标位置的平移量 `(dx, dy)` 与当前不透明度。
    pub(crate) fn peek_offset_opacity_scoped(
        &self,
        id: u32,
        path_hash: u32,
    ) -> Option<(i16, i16, u8)> {
        self.transitions
            .iter()
            .find(|t| t.id == id && t.path_hash == path_hash)
            .or_else(|| self.transitions.iter().find(|t| t.id == id))
            .map(|t| {
                let cur = t.current_state();
                (
                    cur.bounds.x.saturating_sub(t.target_bounds.x),
                    cur.bounds.y.saturating_sub(t.target_bounds.y),
                    cur.opacity,
                )
            })
    }

    /// 解析并更新带 ID 节点的跨帧过渡属性（颜色、几何、不透明度、入场、延迟与循环）。
    pub fn resolve(
        &mut self,
        id: u32,
        target: MotionState,
        spec: Option<TransitionSpec>,
    ) -> MotionState {
        self.resolve_inner(id, id, (0, 0), target, spec, false)
    }

    /// 带树路径哈希作用域与父容器布局原点的跨帧过渡解析（在每帧 `begin_frame()` .. `end_frame()` 周期内调用）：
    /// 1. 优先精确匹配 `(id, path_hash)`；若命中同父容器，则先将上一帧起止矩形按父容器刚性位移 `parent_delta` 平移，
    ///    使滚动、侧边栏折叠、窗口缩放或兄弟节点重排绝不会误触发子元素脱离父容器的 `animate_bounds` 漂移；
    /// 2. 若本帧该槽位已被访问（极端同父级重复 ID 冲突），直接返回 `target` 绝不覆写同一槽位；
    /// 3. 仅当节点声明了 `spec` 时才允许跨容器按 `id` 接管未被本帧访问的旧槽位（保留共享元素屏幕空间过渡）；
    ///    未声明过渡（`spec = None`）的节点绝不跨路径误杀其他容器同 ID 节点的活跃动画。
    pub(crate) fn resolve_scoped(
        &mut self,
        id: u32,
        path_hash: u32,
        parent_origin: (i16, i16),
        target: MotionState,
        spec: Option<TransitionSpec>,
    ) -> MotionState {
        self.resolve_inner(id, path_hash, parent_origin, target, spec, true)
    }

    fn resolve_inner(
        &mut self,
        id: u32,
        path_hash: u32,
        parent_origin: (i16, i16),
        target: MotionState,
        spec: Option<TransitionSpec>,
        guard_same_frame_dup: bool,
    ) -> MotionState {
        let matched_idx = self
            .transitions
            .iter()
            .position(|t| t.id == id && t.path_hash == path_hash)
            .or_else(|| {
                if spec.is_some() {
                    self.transitions
                        .iter()
                        .position(|t| t.id == id && (!guard_same_frame_dup || !t.seen()))
                } else {
                    None
                }
            });

        if let Some(idx) = matched_idx {
            let tr = &mut self.transitions[idx];
            if guard_same_frame_dup && tr.seen() {
                // 同帧内遇到完全相同的 (id, path_hash) 冲突：保持首个节点的过渡状态，防止无限重启动画
                return target;
            }
            let same_parent = tr.path_hash == path_hash;
            if same_parent {
                let pdx = parent_origin.0.saturating_sub(tr.parent_x);
                let pdy = parent_origin.1.saturating_sub(tr.parent_y);
                if pdx != 0 || pdy != 0 {
                    tr.start_bounds.x = tr.start_bounds.x.saturating_add(pdx);
                    tr.start_bounds.y = tr.start_bounds.y.saturating_add(pdy);
                    tr.target_bounds.x = tr.target_bounds.x.saturating_add(pdx);
                    tr.target_bounds.y = tr.target_bounds.y.saturating_add(pdy);
                }
            }
            tr.parent_x = parent_origin.0;
            tr.parent_y = parent_origin.1;
            tr.path_hash = path_hash;
            tr.set_seen(true);
            let Some(s) = spec else {
                tr.settle(target);
                return target;
            };
            tr.set_mode(s.easing, s.loop_mode);

            if let Some(pulse_from) = s.pulse_from_bg {
                tr.set_start(MotionState {
                    bg: pulse_from,
                    ..target
                });
                tr.set_target(target);
                tr.duration_ms = ActiveTransition::clamp_duration(s.duration_ms.max(1));
                return tr.current_state();
            }

            let color_changed = s.animate_color
                && (tr.target_bg != target.bg || tr.target_border != target.border);
            let bounds_changed = s.animate_bounds && tr.target_bounds != target.bounds;
            let opacity_changed = s.animate_opacity && tr.target_opacity != target.opacity;

            if color_changed || bounds_changed || opacity_changed {
                // 以当前插值状态为起点重新出发（打断进行中的动画时保持视觉连续，无跳变）
                let in_flight = tr.is_active();
                let cur = tr.current_state();
                tr.set_start(MotionState {
                    bounds: if s.animate_bounds || in_flight {
                        cur.bounds
                    } else {
                        target.bounds
                    },
                    bg: if s.animate_color || in_flight {
                        cur.bg
                    } else {
                        target.bg
                    },
                    border: if (s.animate_color || in_flight)
                        && cur.border.a() > 0
                        && target.border.a() > 0
                    {
                        cur.border
                    } else {
                        target.border
                    },
                    opacity: if s.animate_opacity || in_flight {
                        cur.opacity
                    } else {
                        target.opacity
                    },
                });
                tr.set_target(target);
                tr.duration_ms = ActiveTransition::clamp_duration(s.duration_ms.max(1));
                tr.elapsed_ms = ActiveTransition::delay_to_elapsed(s.delay_ms);
            } else if !tr.is_active() {
                tr.set_start(target);
                tr.set_target(target);
            } else {
                // 动画（入场/循环）进行中，但未声明插值的属性发生跳变：同步目标并平移起点，保持动画连续
                if tr.target_bounds != target.bounds {
                    let dx = target.bounds.x.saturating_sub(tr.target_bounds.x);
                    let dy = target.bounds.y.saturating_sub(tr.target_bounds.y);
                    tr.start_bounds = Region::new(
                        tr.start_bounds.x.saturating_add(dx),
                        tr.start_bounds.y.saturating_add(dy),
                        target.bounds.w,
                        target.bounds.h,
                    );
                    tr.target_bounds = target.bounds;
                }
                if tr.target_bg != target.bg || tr.target_border != target.border {
                    tr.start_bg = target.bg;
                    tr.start_border = target.border;
                    tr.target_bg = target.bg;
                    tr.target_border = target.border;
                }
                tr.target_opacity = target.opacity;
            }
            return tr.current_state();
        }

        // 首次出现该节点：未声明过渡则无需占用槽位
        let Some(s) = spec else {
            return target;
        };
        let mut start = target;
        if let Some(pulse_from) = s.pulse_from_bg {
            start.bg = pulse_from;
        }
        if let Some(enter) = s.enter {
            start.opacity = enter.opacity;
            start.bounds = Region::new(
                target.bounds.x.saturating_add(enter.dx),
                target.bounds.y.saturating_add(enter.dy),
                target.bounds.w,
                target.bounds.h,
            );
        }
        let animating = s.enter.is_some() || !matches!(s.loop_mode, LoopMode::Once);
        let duration_ms = ActiveTransition::clamp_duration(s.duration_ms);
        let mut entry = ActiveTransition {
            id,
            path_hash,
            start_bounds: start.bounds,
            target_bounds: target.bounds,
            start_bg: start.bg,
            target_bg: target.bg,
            start_border: start.border,
            target_border: target.border,
            parent_x: parent_origin.0,
            parent_y: parent_origin.1,
            elapsed_ms: if animating {
                ActiveTransition::delay_to_elapsed(s.delay_ms)
            } else {
                duration_ms as i16
            },
            duration_ms,
            mode_flags: FLAG_SEEN,
            start_opacity: start.opacity,
            target_opacity: target.opacity,
        };
        entry.set_mode(s.easing, s.loop_mode);
        let state = entry.current_state();
        if !self.transitions.push(entry) {
            // `no_std` 定长槽位已满：复用一个已静止的槽位（静止槽位仅用于记忆上一帧目标值）
            if let Some(slot) = self.transitions.iter_mut().find(|t| !t.is_active()) {
                *slot = entry;
            } else {
                return target;
            }
        }
        state
    }

    /// 解析并更新带 ID 的 `Div` 节点的跨帧过渡属性（兼容旧接口，不含不透明度）。
    pub fn resolve_div(
        &mut self,
        id: u32,
        target_bounds: Region,
        target_bg: Color,
        target_border: Color,
        spec: Option<TransitionSpec>,
    ) -> (Region, Color, Color) {
        let st = self.resolve(
            id,
            MotionState::new(target_bounds, target_bg, target_border),
            spec,
        );
        (st.bounds, st.bg, st.border)
    }

    /// 解析并更新文本颜色跨帧平滑过渡。
    pub fn resolve_text_color(&mut self, id: u32, target_color: Color, duration_ms: u16) -> Color {
        self.resolve_text_color_with(
            id,
            target_color,
            TransitionSpec {
                duration_ms,
                ..TransitionSpec::default()
            },
        )
    }

    /// 按父容器过渡配置（时长、缓动、延迟）解析文本颜色跨帧过渡。
    pub fn resolve_text_color_with(
        &mut self,
        id: u32,
        target_color: Color,
        parent: TransitionSpec,
    ) -> Color {
        let spec = TransitionSpec {
            duration_ms: parent.duration_ms,
            delay_ms: parent.delay_ms,
            easing: parent.easing,
            animate_color: true,
            ..TransitionSpec::default()
        };
        self.resolve(
            id,
            MotionState::new(Region::EMPTY, target_color, Color::TRANSPARENT),
            Some(spec),
        )
        .bg
    }

    /// 获取指定滚动容器 ID 的当前垂直滚动偏移量（像素）。
    pub fn scroll_offset(&self, id: u32) -> i16 {
        self.scroll_offsets
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.offset_y)
            .unwrap_or(0)
    }

    fn insert_scroll_entry(&mut self, entry: ScrollEntry) {
        if self.scroll_offsets.len() >= 64 {
            if let Some((idx, _)) = (0..self.scroll_offsets.len())
                .filter(|&i| self.scroll_offsets[i].velocity_y == 0)
                .map(|i| (i, self.stale_at(i)))
                .max_by_key(|&(_, st)| st)
            {
                self.scroll_offsets[idx] = entry;
                self.set_stale_at(idx, 0);
                return;
            }
            self.scroll_offsets[0] = entry;
            self.set_stale_at(0, 0);
            return;
        }
        let new_idx = self.scroll_offsets.len();
        if self.scroll_offsets.push(entry) {
            self.set_stale_at(new_idx, 0);
        } else if let Some((idx, _)) = (0..self.scroll_offsets.len())
            .map(|i| (i, (self.scroll_offsets[i].velocity_y == 0, self.stale_at(i))))
            .max_by_key(|&(_, k)| k)
        {
            self.scroll_offsets[idx] = entry;
            self.set_stale_at(idx, 0);
        }
    }

    /// 设置指定滚动容器 ID 的垂直滚动偏移量（像素）。
    pub fn set_scroll_offset(&mut self, id: u32, offset_y: i16) {
        if let Some((idx, entry)) = self
            .scroll_offsets
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.id == id)
        {
            let max_s = if entry.max_scroll >= 0 {
                entry.max_scroll
            } else {
                i16::MAX
            };
            entry.offset_y = offset_y.clamp(0, max_s);
            self.set_stale_at(idx, 0);
        } else {
            self.insert_scroll_entry(ScrollEntry {
                id,
                offset_y: offset_y.max(0),
                velocity_y: 0,
                max_scroll: -1,
            });
        }
    }

    /// 设置滚动容器的最大可滚动范围与惯性速度。
    pub fn set_scroll_inertia(&mut self, id: u32, velocity_y: i16, max_scroll: i16) {
        if let Some((idx, entry)) = self
            .scroll_offsets
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.id == id)
        {
            entry.velocity_y = velocity_y;
            if max_scroll >= 0 {
                entry.max_scroll = max_scroll;
                entry.offset_y = entry.offset_y.clamp(0, max_scroll);
            }
            self.set_stale_at(idx, 0);
        } else {
            let eff_max = if max_scroll >= 0 { max_scroll } else { -1 };
            self.insert_scroll_entry(ScrollEntry {
                id,
                offset_y: 0,
                velocity_y,
                max_scroll: eff_max,
            });
        }
    }
}

#[inline]
fn lerp_i16_signed(a: i16, b: i16, t_256: i32) -> i16 {
    let diff = (b as i32) - (a as i32);
    let val = (a as i32) + ((diff * t_256) >> 8);
    val.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[inline]
fn lerp_region_signed(a: Region, b: Region, t_256: i32) -> Region {
    Region {
        x: lerp_i16_signed(a.x, b.x, t_256),
        y: lerp_i16_signed(a.y, b.y, t_256),
        w: lerp_i16_signed(a.w, b.w, t_256).max(0),
        h: lerp_i16_signed(a.h, b.h, t_256).max(0),
    }
}

