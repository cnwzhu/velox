//! 基于绝对时间戳的声明式补间器（`Tween`）与显隐挂载生命周期状态机（`Presence`）。

use crate::style::Easing;

/// 基于绝对时间戳（`cx.now_ms()`）的单值补间器（`no_std`、零分配、`Copy`）。
///
/// 存放在 `View` 状态中驱动任意自定义数值动画（角度、进度、偏移、颜色混合比等）。
/// 渲染期以 `value(cx.now_ms())` 采样，若 `is_running(...)` 则调用 `cx.request_frame()` 请求下一帧。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Tween {
    from: i32,
    to: i32,
    start_ms: u32,
    duration_ms: u16,
    delay_ms: u16,
    easing: Easing,
}

impl Tween {
    /// 创建一个静止于 `value` 的补间器（默认 `200ms`、`EaseOut`）。
    #[inline(always)]
    pub const fn new(value: i32) -> Self {
        Self {
            from: value,
            to: value,
            start_ms: 0,
            duration_ms: 200,
            delay_ms: 0,
            easing: Easing::EaseOut,
        }
    }

    /// 设置缓动曲线。
    #[inline(always)]
    pub const fn with_easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// 设置补间时长（毫秒）。
    #[inline(always)]
    pub const fn with_duration(mut self, duration_ms: u16) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    /// 设置启动延迟（毫秒）。
    #[inline(always)]
    pub const fn with_delay(mut self, delay_ms: u16) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    /// 最终目标值。
    #[inline(always)]
    pub const fn target(&self) -> i32 {
        self.to
    }

    /// 从当前采样值平滑过渡到新目标 `to`（打断进行中的补间时保持连续）。
    #[inline]
    pub fn animate_to(&mut self, now_ms: u32, to: i32) {
        if to == self.to {
            return;
        }
        self.from = self.value(now_ms);
        self.to = to;
        self.start_ms = now_ms;
    }

    /// 以显式起止值重新开始补间。
    #[inline]
    pub fn start(&mut self, now_ms: u32, from: i32, to: i32) {
        self.from = from;
        self.to = to;
        self.start_ms = now_ms;
    }

    /// 立即跳转到 `value` 并停止补间。
    #[inline]
    pub fn jump_to(&mut self, value: i32) {
        self.from = value;
        self.to = value;
    }

    /// 已缓动的归一化进度（`0..=256`，过冲曲线可短暂超出）。
    #[inline]
    pub fn progress_256(&self, now_ms: u32) -> i32 {
        let elapsed = now_ms.wrapping_sub(self.start_ms);
        if elapsed < self.delay_ms as u32 {
            return 0;
        }
        self.easing
            .sample_256(elapsed - self.delay_ms as u32, self.duration_ms as u32)
    }

    /// 在时间戳 `now_ms` 处采样当前值。
    #[inline]
    pub fn value(&self, now_ms: u32) -> i32 {
        if self.from == self.to {
            return self.to;
        }
        let t = self.progress_256(now_ms) as i64;
        (self.from as i64 + (((self.to as i64 - self.from as i64) * t) >> 8)) as i32
    }

    /// 补间是否仍在进行中（含延迟段）。
    #[inline]
    pub fn is_running(&self, now_ms: u32) -> bool {
        self.from != self.to
            && now_ms.wrapping_sub(self.start_ms) < self.delay_ms as u32 + self.duration_ms as u32
    }
}

/// 显隐过渡状态机（实现元素的“退场动画”：隐藏后仍保持挂载直至淡出/滑出完成）。
///
/// ```ignore
/// // 渲染期：
/// let now = cx.now_ms();
/// if self.popup.is_mounted(now) {
///     root = root.child(cx, div().opacity(self.popup.opacity(now)).mt(self.popup.offset(now, 16)));
/// }
/// if self.popup.is_animating(now) { cx.request_frame(); }
/// // 事件中：self.popup.hide(acx.now_ms());
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Presence {
    tween: Tween,
}

impl Presence {
    /// 创建初始显隐状态为 `visible`、过渡时长为 `duration_ms` 的状态机。
    #[inline(always)]
    pub const fn new(visible: bool, duration_ms: u16) -> Self {
        Self {
            tween: Tween::new(if visible { 256 } else { 0 }).with_duration(duration_ms),
        }
    }

    /// 设置显隐过渡缓动曲线。
    #[inline(always)]
    pub const fn with_easing(mut self, easing: Easing) -> Self {
        self.tween = self.tween.with_easing(easing);
        self
    }

    /// 目标状态是否为可见。
    #[inline(always)]
    pub const fn is_visible(&self) -> bool {
        self.tween.target() == 256
    }

    /// 开始入场（显示）过渡。
    #[inline]
    pub fn show(&mut self, now_ms: u32) {
        self.tween.animate_to(now_ms, 256);
    }

    /// 开始退场（隐藏）过渡；过渡完成前 `is_mounted` 仍返回 `true`。
    #[inline]
    pub fn hide(&mut self, now_ms: u32) {
        self.tween.animate_to(now_ms, 0);
    }

    /// 切换显隐状态。
    #[inline]
    pub fn toggle(&mut self, now_ms: u32) {
        if self.is_visible() {
            self.hide(now_ms);
        } else {
            self.show(now_ms);
        }
    }

    /// 显式设置显隐状态。
    #[inline]
    pub fn set_visible(&mut self, now_ms: u32, visible: bool) {
        if visible {
            self.show(now_ms);
        } else {
            self.hide(now_ms);
        }
    }

    /// 元素是否仍需挂载渲染（可见，或正在执行退场过渡）。
    #[inline]
    pub fn is_mounted(&self, now_ms: u32) -> bool {
        self.is_visible() || self.tween.value(now_ms) > 0
    }

    /// 显隐过渡是否正在进行（渲染期据此调用 `cx.request_frame()`）。
    #[inline]
    pub fn is_animating(&self, now_ms: u32) -> bool {
        self.tween.is_running(now_ms)
    }

    /// 当前显现进度（`0` 完全隐藏 ~ `256` 完全显示）。
    #[inline]
    pub fn progress_256(&self, now_ms: u32) -> i32 {
        self.tween.value(now_ms)
    }

    /// 当前显现进度映射的不透明度（`0..=255`）。
    #[inline]
    pub fn opacity(&self, now_ms: u32) -> u8 {
        ((self.progress_256(now_ms).clamp(0, 256) * 255) >> 8) as u8
    }

    /// 当前显现进度映射的位移：完全隐藏时为 `distance`，完全显示时为 `0`（用于滑入/滑出）。
    #[inline]
    pub fn offset(&self, now_ms: u32, distance: i16) -> i16 {
        let rem = 256 - self.progress_256(now_ms);
        ((distance as i32 * rem) >> 8) as i16
    }
}
