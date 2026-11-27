//! 跨帧状态暂存、二阶弹簧速度继承过渡池（`MotionCache`）、幽灵快照退场与橡皮筋弹性滚动物理。

use crate::arena::{ScratchArena, SmallVec};
use crate::element::{ExitTo, TransitionSpec};
use crate::motion::draw_op::{DrawOp, DrawPayload};
use crate::motion::spring::rubberband_i16;
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

/// 单个元素的活跃过渡动画槽位：
/// - 记录 `path_hash` 与父节点布局原点 `(parent_x, parent_y)`，防止滚动/侧边栏折叠引发误漂移；
/// - 当 `easing == Easing::Spring && loop_mode == LoopMode::Once` 时自动激活无 FPU 友好的纯整数定点数二阶弹簧动力学
///   （`pos_q4` / `vel_q4`），中途改变目标或快速来回打断时严格保留瞬时速度，彻底消除折角感。
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
    /// 低 3 位 `Easing`，第 3~4 位 `LoopMode`，第 5 位往复方向位，第 6 位本帧访问标记，第 7 位二阶物理弹簧标志。
    mode_flags: u8,
    start_opacity: u8,
    target_opacity: u8,
    /// 弹簧几何四通道 Q4 位置（`[x, y, w, h] << 4`）。
    pos_q4: [i32; 4],
    /// 弹簧几何四通道 Q4 瞬时速度（`px/s << 4`）。
    vel_q4: [i32; 4],
    /// 弹簧标量进度（Q8：`0..=256`，驱动颜色与不透明度与几何弹簧同步收敛）。
    spring_t_q8: i32,
    /// 弹簧标量速度（Q8/s）。
    spring_vt_q8: i32,
}

const FLAG_REVERSE: u8 = 0x20;
const FLAG_SEEN: u8 = 0x40;
const FLAG_SPRING_PHYSICS: u8 = 0x80;

#[inline(always)]
const fn region_to_q4(r: Region) -> [i32; 4] {
    [
        (r.x as i32) << 4,
        (r.y as i32) << 4,
        (r.w as i32) << 4,
        (r.h as i32) << 4,
    ]
}

#[inline(always)]
fn q4_to_region(p: [i32; 4]) -> Region {
    let round = |v: i32| -> i16 {
        let r = if v >= 0 { (v + 8) >> 4 } else { -((-v + 8) >> 4) };
        r.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    };
    Region {
        x: round(p[0]),
        y: round(p[1]),
        w: round(p[2]).max(0),
        h: round(p[3]).max(0),
    }
}

#[inline(always)]
fn div_round_i64(num: i64, den: i64) -> i64 {
    if num >= 0 {
        (num + (den >> 1)) / den
    } else {
        (num - (den >> 1)) / den
    }
}

/// 纯整数定点数半隐式欧拉单步积分（`k` 为整数刚度，`c_q4` 为 Q4 阻尼系数，`sub_ms` <= 4ms），
/// 带对称四舍五入与防截断死区推进，确保 100% 无 FPU 兼容且精确收敛至 `target`（0px 稳态误差）。
#[inline(always)]
fn step_spring_axis_i32(pos: &mut i32, vel: &mut i32, target: i32, k: i64, c_q4: i64, sub_ms: i64) {
    let disp = (*pos as i64) - (target as i64);
    let v = *vel as i64;
    let accel = -k * disp - ((c_q4 * v) >> 4);
    let mut dv = div_round_i64(accel * sub_ms, 1000);
    if dv == 0 && accel != 0 {
        dv = accel.signum();
    }
    let next_v = (v + dv).clamp(-200_000_000, 200_000_000);
    let mut dp = div_round_i64(next_v * sub_ms, 1000);
    if dp == 0 && next_v.abs() >= 32 {
        dp = next_v.signum();
    }
    let next_p = (*pos as i64 + dp).clamp(i32::MIN as i64 / 2, i32::MAX as i64 / 2);
    *vel = next_v as i32;
    *pos = next_p as i32;
}

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
    const fn is_spring_physics(&self) -> bool {
        (self.mode_flags & FLAG_SPRING_PHYSICS) != 0
    }

    #[inline(always)]
    fn set_seen(&mut self, seen: bool) {
        if seen {
            self.mode_flags |= FLAG_SEEN;
        } else {
            self.mode_flags &= !FLAG_SEEN;
        }
    }

    /// 更新缓动与循环模式（保留方向位与访问标记；`Easing::Spring + LoopMode::Once` 自动激活二阶速度继承弹簧）。
    #[inline(always)]
    fn set_mode(&mut self, easing: Easing, loop_mode: LoopMode) {
        let spring_flag = if matches!(easing, Easing::Spring) && matches!(loop_mode, LoopMode::Once) {
            FLAG_SPRING_PHYSICS
        } else {
            0
        };
        self.mode_flags = (easing as u8 & 0x07)
            | ((loop_mode as u8 & 0x03) << 3)
            | (self.mode_flags & (FLAG_REVERSE | FLAG_SEEN))
            | spring_flag;
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
    fn spring_settled(&self) -> bool {
        let tgt = region_to_q4(self.target_bounds);
        for i in 0..4 {
            if (self.pos_q4[i] - tgt[i]).abs() > 4 || self.vel_q4[i].abs() > 32 {
                return false;
            }
        }
        (self.spring_t_q8 - 256).abs() <= 4 && self.spring_vt_q8.abs() <= 32
    }

    #[inline(always)]
    fn is_active(&self) -> bool {
        if self.duration_ms == 0 {
            return false;
        }
        if self.is_spring_physics() {
            return self.elapsed_ms < 0 || !self.spring_settled();
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

    /// 立即结束动画并停留在目标状态（同时归零二阶弹簧速度）。
    #[inline]
    fn settle(&mut self, target: MotionState) {
        self.set_start(target);
        self.set_target(target);
        self.pos_q4 = region_to_q4(target.bounds);
        self.vel_q4 = [0; 4];
        self.spring_t_q8 = 256;
        self.spring_vt_q8 = 0;
        self.elapsed_ms = self.duration_ms as i16;
        self.mode_flags &= 0x07 | FLAG_SEEN | FLAG_SPRING_PHYSICS;
    }

    /// 按 `duration_ms` 映射刚度 `k` 与欠阻尼系数 `c_q4`（默认欠阻尼比 `ζ ≈ 0.80`，250ms 对应 `k ≈ 256, c ≈ 25.6`），
    /// 推进定点数二阶弹簧。
    fn step_spring(&mut self, dt_ms: u16) {
        let dt = dt_ms.min(100) as i32;
        if self.elapsed_ms < 0 {
            let rem = self.elapsed_ms as i32 + dt;
            if rem <= 0 {
                self.elapsed_ms = rem as i16;
                return;
            }
            self.elapsed_ms = 0;
            // 用超出延迟段的剩余时间推进首帧弹簧
            self.step_spring_substeps(rem as i64);
            return;
        }
        self.step_spring_substeps(dt as i64);
    }

    fn step_spring_substeps(&mut self, total_ms: i64) {
        if total_ms <= 0 {
            return;
        }
        let dur = (self.duration_ms as i64).clamp(60, 1000);
        // omega_q4 = (64_000 / dur) => dur=250ms -> omega_q4=256 (omega=16 rad/s, k=256, c=2*0.80*16=25.6)
        let omega_q4 = (64_000 / dur).clamp(64, 640);
        let k = (omega_q4 * omega_q4) >> 8;
        // zeta = 0.80 => 2 * zeta = 1.60 => c_q4 = omega_q4 * 16 / 10
        let c_q4 = (omega_q4 * 16) / 10;

        let sub_steps = ((total_ms + 3) / 4).max(1);
        let sub_ms = (total_ms / sub_steps).max(1);
        let rem_ms = total_ms - sub_ms * (sub_steps - 1);
        let tgt = region_to_q4(self.target_bounds);

        for step_idx in 0..sub_steps {
            let h = if step_idx + 1 == sub_steps { rem_ms } else { sub_ms };
            for axis in 0..4 {
                step_spring_axis_i32(&mut self.pos_q4[axis], &mut self.vel_q4[axis], tgt[axis], k, c_q4, h);
            }
            step_spring_axis_i32(&mut self.spring_t_q8, &mut self.spring_vt_q8, 256, k, c_q4, h);
        }

        if self.spring_settled() {
            self.pos_q4 = tgt;
            self.vel_q4 = [0; 4];
            self.spring_t_q8 = 256;
            self.spring_vt_q8 = 0;
            self.elapsed_ms = self.duration_ms as i16;
        } else {
            self.elapsed_ms = (self.elapsed_ms as i32 + total_ms as i32).min(i16::MAX as i32) as i16;
        }
    }

    #[inline]
    fn current_state(&self) -> MotionState {
        if self.duration_ms == 0 {
            return self.target_state();
        }
        if self.is_spring_physics() {
            if self.elapsed_ms < 0 {
                return self.start_state();
            }
            if self.spring_settled() {
                return self.target_state();
            }
            let t_clamped = self.spring_t_q8.clamp(0, 256) as u16;
            let op = self.start_opacity as i32
                + (((self.target_opacity as i32 - self.start_opacity as i32) * self.spring_t_q8) >> 8);
            return MotionState {
                bounds: q4_to_region(self.pos_q4),
                bg: self.start_bg.lerp(self.target_bg, t_clamped),
                border: self.start_border.lerp(self.target_border, t_clamped),
                opacity: op.clamp(0, 255) as u8,
            };
        }
        if matches!(self.loop_mode(), LoopMode::Once)
            && (self.elapsed_ms as i32) >= self.duration_ms as i32
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

const GHOST_LABEL_MAX: usize = 48;

/// 节点退场幽灵快照（纯 `Copy` 自包含，不借用帧内 `ScratchArena`；支持自动淡出/滑出及附带短标签文字）。
#[derive(Copy, Clone, Debug)]
struct GhostEntry {
    id: u32,
    path_hash: u32,
    bounds: Region,
    clip: Region,
    clip_radius: u8,
    bg: Color,
    bg_grad: Color,
    border_color: Color,
    rounded: u8,
    encoded_bw: u8,
    start_opacity: u8,
    exit_to: ExitTo,
    elapsed_ms: u16,
    duration_ms: u16,
    easing: Easing,
    label_len: u8,
    label_bytes: [u8; GHOST_LABEL_MAX],
    label_color: Color,
    label_font_size: u8,
    /// 标签文本相对 `bounds` 左上角的真实偏移（`i16::MIN` 表示回退垂直居中）。
    label_dx: i16,
    label_dy: i16,
    active_exiting: bool,
    seen_this_frame: bool,
}

/// 滚动容器跨帧状态条目（支持触摸惯性滑动衰减、越界双曲橡皮筋阻尼与撞边临界阻尼弹簧回弹）。
#[derive(Copy, Clone, Debug)]
struct ScrollEntry {
    id: u32,
    offset_y: i16,
    velocity_y: i16,
    max_scroll: i16,
    /// 越界橡皮筋弹性位移（Q4 像素，正值表示过底拉出、负值表示过顶拉出）。
    overscroll_q4: i32,
    /// 越界临界阻尼回弹弹簧瞬时速度（Q4 px/s）。
    overscroll_vel_q4: i32,
    /// 拖拽期间累计的原始越界位移（用于双曲橡皮筋公式非线性映射）。
    drag_overscroll_raw: i16,
    /// 滚动容器视口特征尺寸（px，用于双曲橡皮筋阻尼计算）。
    viewport_dim: i16,
    /// 当前是否正处于手指/指针按住拖拽状态。
    is_dragging: bool,
}

/// 跨帧状态、物理弹簧过渡、幽灵退场快照与弹性滚动缓存池（`MotionCache`）。
#[derive(Debug)]
pub struct MotionCache {
    transitions: SmallVec<ActiveTransition, 16>,
    scroll_offsets: SmallVec<ScrollEntry, 16>,
    ghosts: SmallVec<GhostEntry, 4>,
    /// 前 16 个滚动槽位的陈旧帧计数（每槽位 2 bit，上限 3）。
    scroll_stale_bits: u32,
    /// 无障碍「减弱动态效果」（Reduced Motion）全局开关。
    reduced_motion: bool,
    /// 本帧 Pass 4 捕获到的活跃文本输入光标精确屏幕逻辑矩形（供系统 IME 候选框实时跟随光标）。
    ime_caret_bounds: Option<Region>,
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
            ghosts: SmallVec::new(),
            scroll_stale_bits: 0,
            reduced_motion: false,
            ime_caret_bounds: None,
        }
    }

    /// 记录本帧 Pass 4 布局求解得到的精确文本输入光标逻辑屏幕矩形。
    #[inline(always)]
    pub(crate) fn set_ime_caret_bounds(&mut self, bounds: Option<Region>) {
        self.ime_caret_bounds = bounds;
    }

    /// 读取本帧捕获到的精确文本输入光标逻辑屏幕矩形（无活跃文本光标时返回 `None`）。
    #[inline(always)]
    pub const fn ime_caret_bounds(&self) -> Option<Region> {
        self.ime_caret_bounds
    }

    /// 配置无障碍「减弱动态效果」（开启后自动关闭几何位移动画、橡皮筋越界回弹与幽灵滑出）。
    #[inline]
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.finish_all();
            self.ghosts.clear();
            for sc in self.scroll_offsets.iter_mut() {
                sc.overscroll_q4 = 0;
                sc.overscroll_vel_q4 = 0;
                sc.drag_overscroll_raw = 0;
            }
        }
    }

    /// 查询当前是否开启了「减弱动态效果」。
    #[inline(always)]
    pub const fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// 查询当前正在播放退场动画的幽灵快照数量。
    #[inline]
    pub fn active_ghost_count(&self) -> usize {
        self.ghosts.iter().filter(|g| g.active_exiting).count()
    }

    /// 只读查询指定元素 ID 的当前二阶弹簧瞬时水平/垂直速度 `(vx_px_per_s, vy_px_per_s)`。
    pub fn current_velocity(&self, id: u32) -> Option<(i32, i32)> {
        self.transitions
            .iter()
            .find(|t| t.id == id)
            .map(|t| (t.vel_q4[0] >> 4, t.vel_q4[1] >> 4))
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

    /// 推进所有活跃过渡动画（含二阶定点数弹簧）、幽灵退场快照与弹性滚动物理（`dt_ms` 毫秒）。
    /// 返回本帧是否有动画、幽灵退场或滚动物理发生步进。
    pub fn tick(&mut self, dt_ms: u16) -> bool {
        let mut any_stepped = false;
        for tr in self.transitions.iter_mut() {
            if tr.is_active() {
                any_stepped = true;
                if tr.is_spring_physics() {
                    tr.step_spring(dt_ms);
                } else {
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
        }

        // 推进活跃退场幽灵快照时钟
        for g in self.ghosts.iter_mut() {
            if g.active_exiting {
                any_stepped = true;
                g.elapsed_ms = g.elapsed_ms.saturating_add(dt_ms);
            }
        }

        // 推进触摸惯性滚动速度衰减与越界临界阻尼弹簧回弹
        let reduced = self.reduced_motion;
        let dt_clamped = dt_ms.clamp(1, 100) as i64;
        for sc in self.scroll_offsets.iter_mut() {
            if sc.velocity_y != 0 && !sc.is_dragging {
                any_stepped = true;
                let max_s = if sc.max_scroll >= 0 {
                    sc.max_scroll
                } else {
                    i16::MAX
                };
                let step = (((sc.velocity_y as i32) * (dt_clamped as i32)) / 16)
                    .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                let delta = if step != 0 {
                    step
                } else if sc.velocity_y > 0 {
                    1
                } else {
                    -1
                };
                let raw_next = (sc.offset_y as i32) + (delta as i32);
                if raw_next < 0 || raw_next > (max_s as i32) {
                    // 惯性撞击边界：非减弱动态模式下将剩余动量无缝移交临界阻尼回弹弹簧，取代硬截断清零！
                    let hit_edge = if raw_next < 0 { 0 } else { max_s };
                    let overshoot_px = raw_next - (hit_edge as i32);
                    sc.offset_y = hit_edge;
                    if !reduced {
                        // velocity_y 单位是 px/16ms，转换为 Q4 px/s：vel * 1000 / 16 * 16 = vel * 1000
                        let impulse_q4 = (sc.velocity_y as i32).saturating_mul(640);
                        sc.overscroll_q4 = sc
                            .overscroll_q4
                            .saturating_add(overshoot_px.clamp(-64, 64) << 4);
                        sc.overscroll_vel_q4 = sc.overscroll_vel_q4.saturating_add(impulse_q4);
                    }
                    sc.velocity_y = 0;
                } else {
                    sc.offset_y = raw_next as i16;
                    sc.velocity_y = (((sc.velocity_y as i32) * 7) / 8) as i16;
                    if sc.velocity_y.abs() <= 1 {
                        sc.velocity_y = 0;
                    }
                }
            }

            // 越界弹性位移临界阻尼弹簧回弹（松手或惯性撞边后平滑归零）
            if !sc.is_dragging && (sc.overscroll_q4 != 0 || sc.overscroll_vel_q4 != 0) {
                if reduced {
                    sc.overscroll_q4 = 0;
                    sc.overscroll_vel_q4 = 0;
                } else {
                    any_stepped = true;
                    // 临界阻尼弹簧：k = 240, c = 2 * sqrt(240) ≈ 31.0 => c_q4 = 496（平滑无多余回荡）
                    let sub_steps = ((dt_clamped + 3) / 4).max(1);
                    let sub_ms = (dt_clamped / sub_steps).max(1);
                    let rem_ms = dt_clamped - sub_ms * (sub_steps - 1);
                    for step_idx in 0..sub_steps {
                        let h = if step_idx + 1 == sub_steps { rem_ms } else { sub_ms };
                        step_spring_axis_i32(
                            &mut sc.overscroll_q4,
                            &mut sc.overscroll_vel_q4,
                            0,
                            240,
                            496,
                            h,
                        );
                    }
                    if sc.overscroll_q4.abs() <= 4 && sc.overscroll_vel_q4.abs() <= 48 {
                        sc.overscroll_q4 = 0;
                        sc.overscroll_vel_q4 = 0;
                    }
                }
            }
        }
        any_stepped
    }

    /// 是否存在正在进行的过渡动画、幽灵退场或弹性惯性滚动。
    #[inline]
    pub fn has_active_animations(&self) -> bool {
        self.transitions.iter().any(|t| t.is_active())
            || self.ghosts.iter().any(|g| g.active_exiting)
            || self.scroll_offsets.iter().any(|s| {
                s.velocity_y != 0
                    || (!s.is_dragging && (s.overscroll_q4 != 0 || s.overscroll_vel_q4 != 0))
            })
    }

    /// 当前占用的过渡动画槽位数量（用于容量监控与测试）。
    #[inline]
    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// 帧求解开始：清除所有槽位与幽灵快照的访问标记（由布局求解器自动调用）。
    pub fn begin_frame(&mut self) {
        self.ime_caret_bounds = None;
        for tr in self.transitions.iter_mut() {
            tr.set_seen(false);
        }
        for g in self.ghosts.iter_mut() {
            g.seen_this_frame = false;
        }
        let n = self.scroll_offsets.len().min(16);
        for idx in 0..n {
            let next = self.stale_at(idx).saturating_add(1);
            self.set_stale_at(idx, next);
        }
    }

    /// 在 Pass 4 发射带 `exit` 退场配置的节点时记录其最新视觉外观快照（供卸载后自动播放零配置退场）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_ghost_snapshot(
        &mut self,
        id: u32,
        path_hash: u32,
        bounds: Region,
        clip: Region,
        clip_radius: u8,
        bg: Color,
        bg_grad: Color,
        border_color: Color,
        rounded: u8,
        encoded_bw: u8,
        opacity: u8,
        exit_to: ExitTo,
        duration_ms: u16,
        easing: Easing,
        label: Option<(&str, Color, u8, i16, i16)>,
    ) {
        if self.reduced_motion || duration_ms == 0 {
            return;
        }
        let mut label_bytes = [0u8; GHOST_LABEL_MAX];
        let mut label_len = 0u8;
        let mut label_color = Color::TRANSPARENT;
        let mut label_font_size = 14u8;
        let mut label_dx = i16::MIN;
        let mut label_dy = i16::MIN;
        if let Some((txt, col, fs, dx, dy)) = label {
            // 按 UTF-8 字符边界安全截断至最多 `GHOST_LABEL_MAX` 字节
            let mut end = txt.len().min(GHOST_LABEL_MAX);
            while end > 0 && !txt.is_char_boundary(end) {
                end -= 1;
            }
            label_bytes[..end].copy_from_slice(&txt.as_bytes()[..end]);
            label_len = end as u8;
            label_color = col;
            label_font_size = fs;
            label_dx = dx;
            label_dy = dy;
        }
        // 入场常用的回弹曲线（EaseOutBack / Spring）用于退场会产生反向回扯感，自动平滑为 EaseOut
        let exit_easing = match easing {
            Easing::EaseOutBack | Easing::Spring => Easing::EaseOut,
            other => other,
        };

        let entry = GhostEntry {
            id,
            path_hash,
            bounds,
            clip,
            clip_radius,
            bg,
            bg_grad,
            border_color,
            rounded,
            encoded_bw,
            start_opacity: opacity,
            exit_to,
            elapsed_ms: 0,
            duration_ms,
            easing: exit_easing,
            label_len,
            label_bytes,
            label_color,
            label_font_size,
            label_dx,
            label_dy,
            active_exiting: false,
            seen_this_frame: true,
        };

        if let Some(slot) = self
            .ghosts
            .iter_mut()
            .find(|g| g.id == id && g.path_hash == path_hash)
        {
            *slot = entry;
            return;
        }
        if !self.ghosts.push(entry) {
            if let Some(slot) = self.ghosts.iter_mut().find(|g| !g.active_exiting && !g.seen_this_frame) {
                *slot = entry;
            }
        }
    }

    /// 帧求解结束：回收本帧未被访问（已从元素树中移除）的过渡动画、推进幽灵退场槽位与回收陈旧滚动槽位。
    pub fn end_frame(&mut self) {
        self.cleanup_transitions_and_scrolls();
        let mut g_idx = 0;
        while g_idx < self.ghosts.len() {
            let g = &mut self.ghosts[g_idx];
            if g.seen_this_frame {
                g_idx += 1;
            } else if !g.active_exiting && !self.reduced_motion && g.duration_ms > 0 {
                g.active_exiting = true;
                g.elapsed_ms = 0;
                g_idx += 1;
            } else if g.active_exiting && g.elapsed_ms < g.duration_ms {
                g_idx += 1;
            } else {
                self.ghosts.swap_remove(g_idx);
            }
        }
    }

    /// 帧求解结束并发射所有正在执行零配置退场动画的幽灵 `DrawOp` 快照。
    pub(crate) fn end_frame_with_ghosts<'a, const MAX_OPS: usize>(
        &mut self,
        arena: &'a ScratchArena,
        out_ops: &mut SmallVec<DrawOp<'a>, MAX_OPS>,
        dropped_ops: &mut u32,
    ) {
        self.cleanup_transitions_and_scrolls();

        let mut g_idx = 0;
        while g_idx < self.ghosts.len() {
            let g = &mut self.ghosts[g_idx];
            if g.seen_this_frame {
                g_idx += 1;
                continue;
            }
            if !g.active_exiting {
                if !self.reduced_motion && g.duration_ms > 0 {
                    g.active_exiting = true;
                    g.elapsed_ms = 0;
                } else {
                    self.ghosts.swap_remove(g_idx);
                    continue;
                }
            }
            if g.elapsed_ms >= g.duration_ms {
                self.ghosts.swap_remove(g_idx);
                continue;
            }

            let t_256 = g
                .easing
                .sample_256(g.elapsed_ms as u32, g.duration_ms.max(1) as u32);
            let cur_op = (g.start_opacity as i32
                + (((g.exit_to.opacity as i32 - g.start_opacity as i32) * t_256) >> 8))
                .clamp(0, 255) as u8;
            if cur_op > 0 {
                let dx = (((g.exit_to.dx as i32) * t_256) >> 8) as i16;
                let dy = (((g.exit_to.dy as i32) * t_256) >> 8) as i16;
                let ghost_bounds = Region::new(
                    g.bounds.x.saturating_add(dx),
                    g.bounds.y.saturating_add(dy),
                    g.bounds.w,
                    g.bounds.h,
                );
                let ghost_bg = g.bg.scale_alpha(cur_op);
                let ghost_grad = g.bg_grad.scale_alpha(cur_op);
                let ghost_border = g.border_color.scale_alpha(cur_op);
                if ghost_bg.a() > 0 || ghost_grad.a() > 0 || ghost_border.a() > 0 {
                    if !out_ops.push(DrawOp {
                        id: Some(g.id ^ 0x4748_0000),
                        bounds: ghost_bounds,
                        clip: g.clip,
                        clip_radius: g.clip_radius,
                        payload: DrawPayload::Quad {
                            bg: ghost_bg,
                            bg_grad: ghost_grad,
                            border_color: ghost_border,
                            rounded: g.rounded,
                            border_width: g.encoded_bw,
                        },
                    }) {
                        *dropped_ops = dropped_ops.saturating_add(1);
                    }
                }
                if g.label_len > 0 {
                    let s_bytes = &g.label_bytes[..(g.label_len as usize)];
                    if let Ok(txt) = core::str::from_utf8(s_bytes) {
                        if arena.can_alloc_slice::<u8>(txt.len()) {
                            let copied_str = arena.alloc_str(txt);
                            let tc = g.label_color.scale_alpha(cur_op);
                            if tc.a() > 0 {
                                let tx = if g.label_dx != i16::MIN {
                                    ghost_bounds.x.saturating_add(g.label_dx)
                                } else {
                                    ghost_bounds.x.saturating_add(8)
                                };
                                let ty = if g.label_dy != i16::MIN {
                                    ghost_bounds.y.saturating_add(g.label_dy)
                                } else {
                                    ghost_bounds.y.saturating_add(
                                        ghost_bounds
                                            .h
                                            .saturating_sub(g.label_font_size as i16)
                                            / 2,
                                    )
                                };
                                let tw = ghost_bounds
                                    .right()
                                    .saturating_sub(tx)
                                    .saturating_sub(8)
                                    .max(1);
                                let text_bounds = Region::new(
                                    tx,
                                    ty,
                                    tw,
                                    (g.label_font_size as i16).saturating_add(4).max(8),
                                );
                                if !out_ops.push(DrawOp {
                                    id: Some(g.id ^ 0x4749_0000),
                                    bounds: text_bounds,
                                    clip: g.clip.intersect(ghost_bounds),
                                    clip_radius: 0,
                                    payload: DrawPayload::TextRun {
                                        text: copied_str,
                                        color: tc,
                                        font_size: g.label_font_size,
                                        text_flags: 0,
                                    },
                                }) {
                                    *dropped_ops = dropped_ops.saturating_add(1);
                                }
                            }
                        }
                    }
                }
            }
            g_idx += 1;
        }
    }

    fn cleanup_transitions_and_scrolls(&mut self) {
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
            let idle = sc.velocity_y == 0
                && !sc.is_dragging
                && sc.overscroll_q4 == 0
                && sc.overscroll_vel_q4 == 0;
            let evict = idle
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

    /// 解析并更新带 ID 节点的跨帧过渡属性（颜色、几何、不透明度、入场、延迟、循环与二阶弹簧速度继承）。
    pub fn resolve(
        &mut self,
        id: u32,
        target: MotionState,
        spec: Option<TransitionSpec>,
    ) -> MotionState {
        self.resolve_inner(id, id, (0, 0), (0, 0), target, spec, false)
    }

    /// 带祖先插值偏移扣除的跨帧过渡解析（借鉴 GSAP Flip `nested: true`：跨容器共享元素迁入正在移动的父容器时，
    /// 扣除 `ancestor_offset` 避免叠加双倍偏移）。
    pub(crate) fn resolve_scoped_with_ancestor_offset(
        &mut self,
        id: u32,
        path_hash: u32,
        parent_origin: (i16, i16),
        ancestor_offset: (i16, i16),
        target: MotionState,
        spec: Option<TransitionSpec>,
    ) -> MotionState {
        self.resolve_inner(id, path_hash, parent_origin, ancestor_offset, target, spec, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_inner(
        &mut self,
        id: u32,
        path_hash: u32,
        parent_origin: (i16, i16),
        ancestor_offset: (i16, i16),
        target: MotionState,
        spec: Option<TransitionSpec>,
        guard_same_frame_dup: bool,
    ) -> MotionState {
        // 无障碍「减弱动态效果」：直接收敛至目标（保留非几何循环指示灯外的所有单次位移瞬时到位）
        if self.reduced_motion {
            if let Some(s) = spec {
                if matches!(s.loop_mode, LoopMode::Once) {
                    if let Some(tr) = self
                        .transitions
                        .iter_mut()
                        .find(|t| t.id == id && t.path_hash == path_hash)
                    {
                        tr.set_seen(true);
                        tr.settle(target);
                    }
                    return target;
                }
            }
        }

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
                    tr.pos_q4[0] = tr.pos_q4[0].saturating_add((pdx as i32) << 4);
                    tr.pos_q4[1] = tr.pos_q4[1].saturating_add((pdy as i32) << 4);
                }
            } else if ancestor_offset.0 != 0 || ancestor_offset.1 != 0 {
                // 跨容器共享元素迁入正在做 FLIP 动画的新祖先容器：扣除祖先当前偏移防止首帧双倍跳变
                let adx = ancestor_offset.0;
                let ady = ancestor_offset.1;
                tr.start_bounds.x = tr.start_bounds.x.saturating_sub(adx);
                tr.start_bounds.y = tr.start_bounds.y.saturating_sub(ady);
                tr.target_bounds.x = tr.target_bounds.x.saturating_sub(adx);
                tr.target_bounds.y = tr.target_bounds.y.saturating_sub(ady);
                tr.pos_q4[0] = tr.pos_q4[0].saturating_sub((adx as i32) << 4);
                tr.pos_q4[1] = tr.pos_q4[1].saturating_sub((ady as i32) << 4);
            }
            tr.parent_x = parent_origin.0;
            tr.parent_y = parent_origin.1;
            tr.path_hash = path_hash;
            tr.set_seen(true);
            let Some(s) = spec else {
                tr.settle(target);
                return target;
            };
            let was_spring = tr.is_spring_physics();
            tr.set_mode(s.easing, s.loop_mode);
            let now_spring = tr.is_spring_physics();
            if now_spring && !was_spring {
                // 从普通缓动切入弹簧模式时以当前位置对齐 Q4 物理坐标
                let cur = tr.current_state();
                tr.pos_q4 = region_to_q4(cur.bounds);
                tr.vel_q4 = [0; 4];
                tr.spring_t_q8 = 256;
                tr.spring_vt_q8 = 0;
            }

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

                if now_spring {
                    // 二阶弹簧速度继承：打断进行中的动画时严格保留当前 `pos_q4` 与瞬时速度 `vel_q4`，
                    // 仅对未启用几何插值的分量同步至目标；颜色/不透明度若变化则重设标量弹簧起点。
                    if !s.animate_bounds && !in_flight {
                        tr.pos_q4 = region_to_q4(target.bounds);
                        tr.vel_q4 = [0; 4];
                    }
                    if color_changed || opacity_changed {
                        tr.spring_t_q8 = 0;
                        // 保留部分标量初速以柔化打断
                        tr.spring_vt_q8 = 0;
                    }
                    tr.elapsed_ms = if in_flight {
                        0
                    } else {
                        ActiveTransition::delay_to_elapsed(s.delay_ms)
                    };
                } else {
                    tr.elapsed_ms = ActiveTransition::delay_to_elapsed(s.delay_ms);
                }
            } else if !tr.is_active() {
                tr.set_start(target);
                tr.set_target(target);
                tr.pos_q4 = region_to_q4(target.bounds);
                tr.vel_q4 = [0; 4];
                tr.spring_t_q8 = 256;
                tr.spring_vt_q8 = 0;
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
                    tr.pos_q4[0] = tr.pos_q4[0].saturating_add((dx as i32) << 4);
                    tr.pos_q4[1] = tr.pos_q4[1].saturating_add((dy as i32) << 4);
                    tr.pos_q4[2] = (target.bounds.w as i32) << 4;
                    tr.pos_q4[3] = (target.bounds.h as i32) << 4;
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
            pos_q4: region_to_q4(if animating { start.bounds } else { target.bounds }),
            vel_q4: [0; 4],
            spring_t_q8: if animating { 0 } else { 256 },
            spring_vt_q8: 0,
        };
        entry.set_mode(s.easing, s.loop_mode);
        let state = entry.current_state();
        if !self.transitions.push(entry) {
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

    /// 获取指定滚动容器 ID 的当前有效滚动偏移量（含橡皮筋越界弹性位移，单位像素）。
    pub fn scroll_offset(&self, id: u32) -> i16 {
        self.scroll_offsets
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.offset_y.saturating_add((e.overscroll_q4 >> 4) as i16))
            .unwrap_or(0)
    }

    /// 获取指定滚动容器当前的橡皮筋越界弹性位移（像素；`0` 表示处于正常区间内）。
    pub fn scroll_overscroll(&self, id: u32) -> i16 {
        self.scroll_offsets
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.overscroll_q4 >> 4) as i16)
            .unwrap_or(0)
    }

    /// 应用触摸/手势拖拽滚动增量（带 `@use-gesture` 双曲橡皮筋越界阻尼）。
    pub fn apply_scroll_drag(
        &mut self,
        id: u32,
        delta_scroll: i16,
        max_scroll: i16,
        viewport_dim: i16,
    ) {
        let clamped_max = max_scroll.max(0);
        let dim = viewport_dim.max(64);
        let reduced = self.reduced_motion;

        let idx = if let Some(i) = self.scroll_offsets.iter().position(|e| e.id == id) {
            i
        } else {
            self.insert_scroll_entry(ScrollEntry {
                id,
                offset_y: 0,
                velocity_y: 0,
                max_scroll: clamped_max,
                overscroll_q4: 0,
                overscroll_vel_q4: 0,
                drag_overscroll_raw: 0,
                viewport_dim: dim,
                is_dragging: true,
            });
            match self.scroll_offsets.iter().position(|e| e.id == id) {
                Some(i) => i,
                None => return,
            }
        };

        let entry = &mut self.scroll_offsets[idx];
        entry.max_scroll = clamped_max;
        entry.viewport_dim = dim;
        entry.is_dragging = true;
        entry.velocity_y = 0;
        entry.overscroll_vel_q4 = 0;

        if entry.drag_overscroll_raw != 0 {
            let next_raw = (entry.drag_overscroll_raw as i32) + (delta_scroll as i32);
            if (entry.drag_overscroll_raw > 0 && next_raw <= 0)
                || (entry.drag_overscroll_raw < 0 && next_raw >= 0)
            {
                entry.drag_overscroll_raw = 0;
                entry.overscroll_q4 = 0;
                entry.offset_y = ((entry.offset_y as i32) + next_raw).clamp(0, clamped_max as i32) as i16;
            } else {
                entry.drag_overscroll_raw = next_raw.clamp(-4096, 4096) as i16;
                let damped = if reduced {
                    0
                } else {
                    rubberband_i16(entry.drag_overscroll_raw, dim, 140)
                };
                entry.overscroll_q4 = (damped as i32) << 4;
            }
        } else {
            let raw_next = (entry.offset_y as i32) + (delta_scroll as i32);
            if raw_next < 0 {
                entry.offset_y = 0;
                if !reduced {
                    entry.drag_overscroll_raw = raw_next.clamp(-4096, 4096) as i16;
                    entry.overscroll_q4 = (rubberband_i16(entry.drag_overscroll_raw, dim, 140) as i32) << 4;
                }
            } else if raw_next > clamped_max as i32 {
                entry.offset_y = clamped_max;
                if !reduced {
                    let over = raw_next - clamped_max as i32;
                    entry.drag_overscroll_raw = over.clamp(-4096, 4096) as i16;
                    entry.overscroll_q4 = (rubberband_i16(entry.drag_overscroll_raw, dim, 140) as i32) << 4;
                }
            } else {
                entry.offset_y = raw_next as i16;
                entry.overscroll_q4 = 0;
            }
        }
        self.set_stale_at(idx, 0);
    }

    /// 结束触摸/手势拖拽并移交瞬时速度：若当前处于越界橡皮筋态则将速度注入临界阻尼回弹弹簧，否则进入惯性衰减。
    pub fn release_scroll_drag(&mut self, id: u32, release_velocity_y: i16, max_scroll: i16) {
        if let Some((idx, entry)) = self
            .scroll_offsets
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.id == id)
        {
            entry.is_dragging = false;
            entry.drag_overscroll_raw = 0;
            if max_scroll >= 0 {
                entry.max_scroll = max_scroll;
                entry.offset_y = entry.offset_y.clamp(0, max_scroll);
            }
            if entry.overscroll_q4 != 0 {
                entry.velocity_y = 0;
                if self.reduced_motion {
                    entry.overscroll_q4 = 0;
                    entry.overscroll_vel_q4 = 0;
                } else {
                    entry.overscroll_vel_q4 = (release_velocity_y as i32).saturating_mul(640);
                }
            } else {
                entry.velocity_y = release_velocity_y;
            }
            self.set_stale_at(idx, 0);
        } else if release_velocity_y != 0 {
            self.set_scroll_inertia(id, release_velocity_y, max_scroll);
        }
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
            entry.overscroll_q4 = 0;
            entry.overscroll_vel_q4 = 0;
            entry.drag_overscroll_raw = 0;
            entry.is_dragging = false;
            self.set_stale_at(idx, 0);
        } else {
            self.insert_scroll_entry(ScrollEntry {
                id,
                offset_y: offset_y.max(0),
                velocity_y: 0,
                max_scroll: -1,
                overscroll_q4: 0,
                overscroll_vel_q4: 0,
                drag_overscroll_raw: 0,
                viewport_dim: 240,
                is_dragging: false,
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
            entry.is_dragging = false;
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
                overscroll_q4: 0,
                overscroll_vel_q4: 0,
                drag_overscroll_raw: 0,
                viewport_dim: 240,
                is_dragging: false,
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

