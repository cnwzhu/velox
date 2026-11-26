//! 组件动效预设与 `std` 物理弹簧动画引擎（`motion`）。
//!
//! - 在所有目标下提供零开销的 [`MotionPreset`] 语义化动效档位与 [`MotionStyled`] 链式修饰；
//! - 在 `std` 特性下额外提供基于二阶阻尼谐振子（[`Spring`]）的多通道组件物理动画池
//!   [`SpringAnimator`] 以及开箱即用的动效组件 [`SpringProgress`]、[`SpringCard`]、[`SpringReveal`]，
//!   支持运动中途无缝打断并继承瞬时速度，收敛后自动停止保持 0% CPU 休眠。

use std::collections::HashMap;
use velox_engine::prelude::*;

/// 组件动效预设档位（统一驱动时长、缓动曲线与二阶弹簧物理参数）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum MotionPreset {
    /// 干脆利落（180ms + `Easing::Spring` / `SpringConfig::SNAPPY`），适合开关、按钮、页签与弹层。
    #[default]
    Snappy,
    /// 活泼回弹（260ms + `Easing::EaseOutBack` / `SpringConfig::BOUNCY`），适合徽章弹出、评分、提示条与卡片。
    Bouncy,
    /// 柔和顺滑（280ms + `Easing::EaseOut` / `SpringConfig::GENTLE`），适合抽屉、进度条、手风琴展开。
    Gentle,
    /// 克制微动（130ms + `Easing::EaseOut` / `SpringConfig::STIFF`），适合高频悬停配色与微调反馈。
    Subtle,
    /// 立即切换（0ms，禁用动画，适合减少动态效果偏好或极速切换）。
    Instant,
}

impl MotionPreset {
    /// 返回几何位移/尺寸过渡时长（毫秒）。
    #[inline(always)]
    pub const fn duration_ms(self) -> u16 {
        match self {
            Self::Snappy => 200,
            Self::Bouncy => 280,
            Self::Gentle => 300,
            Self::Subtle => 140,
            Self::Instant => 0,
        }
    }

    /// 返回颜色与边框过渡时长（毫秒）。
    #[inline(always)]
    pub const fn color_duration_ms(self) -> u16 {
        match self {
            Self::Snappy => 150,
            Self::Bouncy => 180,
            Self::Gentle => 220,
            Self::Subtle => 120,
            Self::Instant => 0,
        }
    }

    /// 返回对应的跨帧 `MotionCache` 缓动曲线。
    #[inline(always)]
    pub const fn easing(self) -> Easing {
        match self {
            Self::Snappy => Easing::Spring,
            Self::Bouncy => Easing::EaseOutBack,
            Self::Gentle => Easing::EaseOut,
            Self::Subtle => Easing::EaseOut,
            Self::Instant => Easing::Linear,
        }
    }

    /// 返回对应的物理二阶弹簧参数 [`SpringConfig`]。
    #[inline(always)]
    pub const fn spring_config(self) -> SpringConfig {
        match self {
            Self::Snappy => SpringConfig::SNAPPY,
            Self::Bouncy => SpringConfig::BOUNCY,
            Self::Gentle => SpringConfig::GENTLE,
            Self::Subtle => SpringConfig::STIFF,
            Self::Instant => SpringConfig::new(1200.0, 80.0, 1.0),
        }
    }

    /// 返回人类可读预设名称。
    #[inline(always)]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Snappy => "Snappy",
            Self::Bouncy => "Bouncy",
            Self::Gentle => "Gentle",
            Self::Subtle => "Subtle",
            Self::Instant => "Instant",
        }
    }
}

/// 为任意 [`Div`] 提供声明式组件动效预设的便捷扩展 Trait。
pub trait MotionDivExt<'a>: Sized {
    /// 按预设启用颜色与边框过渡动画（首帧零延迟，仅在状态变化时平滑插值）。
    fn motion_color(self, preset: MotionPreset) -> Self;

    /// 按预设启用几何边界（位置/尺寸）弹簧过渡动画。
    fn motion_bounds(self, preset: MotionPreset) -> Self;

    /// 按预设启用全属性（边界 + 颜色 + 不透明度）弹性过渡动画。
    fn motion_all(self, preset: MotionPreset) -> Self;

    /// 按预设启用入场滑入/弹入动画（从 `(dx, dy)` 与初始不透明度 `from_opacity` 弹性过渡至就位）。
    fn motion_enter(self, preset: MotionPreset, dx: i16, dy: i16, from_opacity: u8) -> Self;
}

impl<'a> MotionDivExt<'a> for Div<'a> {
    #[inline]
    fn motion_color(self, preset: MotionPreset) -> Self {
        let ms = preset.color_duration_ms();
        if ms == 0 {
            self
        } else {
            self.animate_color(Duration::from_millis(ms as u64))
                .ease(Easing::EaseOut)
        }
    }

    #[inline]
    fn motion_bounds(self, preset: MotionPreset) -> Self {
        let ms = preset.duration_ms();
        if ms == 0 {
            self
        } else {
            self.animate_bounds(Duration::from_millis(ms as u64))
                .ease(preset.easing())
        }
    }

    #[inline]
    fn motion_all(self, preset: MotionPreset) -> Self {
        let ms = preset.duration_ms();
        if ms == 0 {
            self
        } else {
            self.animate_all(Duration::from_millis(ms as u64))
                .ease(preset.easing())
        }
    }

    #[inline]
    fn motion_enter(self, preset: MotionPreset, dx: i16, dy: i16, from_opacity: u8) -> Self {
        let ms = preset.duration_ms();
        if ms == 0 {
            self
        } else {
            self.enter_from(from_opacity, dx, dy, Duration::from_millis(ms as u64))
                .ease(preset.easing())
        }
    }
}

/// `std` 环境下面向桌面/移动组件的多通道物理弹簧动画池（`SpringAnimator`）。
///
/// 按 `(component_id, channel)` 键值自动管理任意数量的二阶弹簧（[`Spring`]），
/// 适合在 `View` 状态中统一驱动卡片弹跳、滑块阻尼跟随、仪表指针、抽屉展开与悬浮微动，
/// 支持运动中途无缝切换目标并继承当前瞬时速度，全部通道收敛后自动进入静止零耗电状态。
#[derive(Clone, Debug, Default)]
pub struct SpringAnimator {
    springs: HashMap<u64, Spring>,
    preset: MotionPreset,
}

impl SpringAnimator {
    /// 使用默认 `MotionPreset::Snappy` 创建空弹簧动画池。
    pub fn new() -> Self {
        Self {
            springs: HashMap::new(),
            preset: MotionPreset::Snappy,
        }
    }

    /// 指定全局默认预设档位创建弹簧动画池。
    pub fn with_preset(preset: MotionPreset) -> Self {
        Self {
            springs: HashMap::new(),
            preset,
        }
    }

    /// 构造 `(component_id, channel)` 复合通道键。
    #[inline(always)]
    pub const fn key(component_id: u32, channel: u32) -> u64 {
        ((component_id as u64) << 32) | (channel as u64)
    }

    /// 读取当前全局默认预设。
    #[inline(always)]
    pub const fn preset(&self) -> MotionPreset {
        self.preset
    }

    /// 切换全局默认预设并同步更新池内所有弹簧的物理刚度与阻尼参数。
    pub fn set_preset(&mut self, preset: MotionPreset) {
        self.preset = preset;
        let cfg = preset.spring_config();
        for sp in self.springs.values_mut() {
            sp.set_config(cfg);
        }
    }

    /// 为指定通道设定目标值（首次注册时从 `initial` 启动，后续打断自动保留瞬时速度）。
    pub fn animate_to(&mut self, key: u64, initial: f32, target: f32) -> f32 {
        self.animate_to_with(key, initial, target, self.preset.spring_config())
    }

    /// 使用显式 [`SpringConfig`] 为指定通道设定目标值。
    pub fn animate_to_with(
        &mut self,
        key: u64,
        initial: f32,
        target: f32,
        config: SpringConfig,
    ) -> f32 {
        let sp = self
            .springs
            .entry(key)
            .or_insert_with(|| Spring::new(initial, config));
        sp.set_config(config);
        if self.preset == MotionPreset::Instant {
            sp.jump_to(target);
        } else {
            sp.animate_to(target);
        }
        sp.value()
    }

    /// 直接对指定通道注入瞬时速度冲量（例如点击微弹、边界回撞或轻推手势）。
    pub fn impulse(&mut self, key: u64, base_value: f32, velocity_delta: f32) {
        let cfg = self.preset.spring_config();
        let sp = self
            .springs
            .entry(key)
            .or_insert_with(|| Spring::new(base_value, cfg));
        sp.impulse(velocity_delta);
    }

    /// 立即将指定通道跳转到 `value` 并清零速度。
    pub fn jump_to(&mut self, key: u64, value: f32) {
        let cfg = self.preset.spring_config();
        let sp = self
            .springs
            .entry(key)
            .or_insert_with(|| Spring::new(value, cfg));
        sp.jump_to(value);
    }

    /// 以 `dt_ms` 毫秒步进全部活跃弹簧；若仍有通道未收敛则返回 `true`（驱动 `cx.request_frame()`）。
    pub fn step_all(&mut self, dt_ms: u16) -> bool {
        let mut any_running = false;
        for sp in self.springs.values_mut() {
            if sp.is_running() {
                sp.step(dt_ms);
                if sp.is_running() {
                    any_running = true;
                }
            }
        }
        any_running
    }

    /// 读取指定通道当前采样值（若未注册则返回 `fallback`）。
    #[inline]
    pub fn value_or(&self, key: u64, fallback: f32) -> f32 {
        self.springs.get(&key).map(|s| s.value()).unwrap_or(fallback)
    }

    /// 读取指定通道当前瞬时速度（单位/秒）。
    #[inline]
    pub fn velocity_or(&self, key: u64, fallback: f32) -> f32 {
        self.springs
            .get(&key)
            .map(|s| s.velocity())
            .unwrap_or(fallback)
    }

    /// 是否仍有任意弹簧通道在运行中。
    #[inline]
    pub fn is_active(&self) -> bool {
        self.springs.values().any(|s| s.is_running())
    }

    /// 当前运行中的弹簧通道数量。
    #[inline]
    pub fn active_count(&self) -> usize {
        self.springs.values().filter(|s| s.is_running()).count()
    }

    /// 池中已注册通道总数。
    #[inline]
    pub fn len(&self) -> usize {
        self.springs.len()
    }

    /// 是否为空池。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.springs.is_empty()
    }

    /// 清空全部通道状态。
    pub fn clear(&mut self) {
        self.springs.clear();
    }
}

/// 弹簧驱动的平滑主题进度条组件（`SpringProgress`）。
///
/// 接收连续浮点百分比 `0.0..=100.0`（可由 [`SpringAnimator`] 或 [`Spring`] 实时提供），
/// 支持圆角渐变高亮条与右侧实时百分比标签。
pub struct SpringProgress<'a> {
    pub id: u32,
    pub value_pct: f32,
    pub width: i16,
    pub height: i16,
    pub label: Option<StrRef<'a>>,
    pub accent_color: Option<Color>,
    pub preset: MotionPreset,
}

impl<'a> SpringProgress<'a> {
    /// 创建一个弹簧进度条（`value_pct` 范围 `0.0..=100.0`）。
    #[inline(always)]
    pub const fn new(id: u32, value_pct: f32) -> Self {
        Self {
            id,
            value_pct,
            width: 240,
            height: 10,
            label: None,
            accent_color: None,
            preset: MotionPreset::Gentle,
        }
    }

    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    #[inline(always)]
    pub fn label(mut self, label: impl Into<StrRef<'a>>) -> Self {
        self.label = Some(label.into());
        self
    }

    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.accent_color = Some(color);
        self
    }

    #[inline(always)]
    pub const fn preset(mut self, preset: MotionPreset) -> Self {
        self.preset = preset;
        self
    }
}

impl<'a> Widget<'a> for SpringProgress<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let pct = self.value_pct.clamp(0.0, 100.0);
        let fill_w = ((self.width as f32) * (pct / 100.0) + 0.5) as i16;
        let radius = (self.height / 2).max(2) as u8;
        let accent = self.accent_color.unwrap_or(theme.primary);

        let fill = div()
            .id(self.id ^ 0x5350_5247)
            .w(fill_w.clamp(0, self.width))
            .h(self.height)
            .rounded(radius)
            .bg(accent)
            .motion_bounds(self.preset);

        let track = div()
            .id(self.id)
            .w(self.width)
            .h(self.height)
            .rounded(radius)
            .bg(theme.muted)
            .overflow_hidden()
            .child(cx, fill);

        if let Some(lbl) = self.label {
            div()
                .flex_col()
                .gap(4)
                .child(
                    cx,
                    div()
                        .flex_row()
                        .w(self.width)
                        .justify_between()
                        .items_center()
                        .child(cx, text(lbl).size(12).color(theme.foreground))
                        .child(
                            cx,
                            text(fmt!(cx, "{}%", (pct + 0.5) as i32))
                                .size(11)
                                .color(theme.muted_foreground),
                        ),
                )
                .child(cx, track)
                .into_element(cx)
        } else {
            track.into_element(cx)
        }
    }
}

/// 带弹簧微交互悬停与可选入场过渡的主题卡片包装组件（`SpringCard`）。
pub struct SpringCard<'a> {
    pub id: u32,
    pub preset: MotionPreset,
    pub enter_offset_y: i16,
    pub child: Option<Element<'a>>,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> SpringCard<'a> {
    /// 创建一个主题化动效卡片（默认启用配色平滑过渡，不触发首帧跳动）。
    #[inline(always)]
    pub const fn new(id: u32) -> Self {
        Self {
            id,
            preset: MotionPreset::Snappy,
            enter_offset_y: 0,
            child: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    #[inline(always)]
    pub const fn preset(mut self, preset: MotionPreset) -> Self {
        self.preset = preset;
        self
    }

    /// 启用入场垂直滑入距离（像素，默认 `0` 表示仅交互过渡）。
    #[inline(always)]
    pub const fn enter_slide_y(mut self, dy: i16) -> Self {
        self.enter_offset_y = dy;
        self
    }

    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.child = Some(child.into_element(cx));
        self
    }
}

impl<'a> Styled for SpringCard<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl<'a> StyledExt for SpringCard<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        &mut self.style_ext
    }
}

impl<'a> Widget<'a> for SpringCard<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let mut card = div()
            .id(self.id)
            .flex_col()
            .p(14)
            .gap(8)
            .rounded(8)
            .bg(theme.card)
            .border(1, theme.border)
            .hover_bg(theme.accent.lerp(theme.card, 160))
            .motion_color(self.preset);

        if self.enter_offset_y != 0 {
            card = card.motion_enter(self.preset, 0, self.enter_offset_y, 0);
        }
        if let Some(ch) = self.child {
            card = card.child(cx, ch);
        }
        self.style_ext
            .apply_to_div(card, self.style)
            .into_element(cx)
    }
}

/// 弹簧展开/折叠容器（`SpringReveal`）：由 `progress`（`0.0..=1.0`）平滑控制可见高度与不透明度。
pub struct SpringReveal<'a> {
    pub id: u32,
    pub max_height: i16,
    pub progress: f32,
    pub child: Option<Element<'a>>,
}

impl<'a> SpringReveal<'a> {
    #[inline(always)]
    pub const fn new(id: u32, max_height: i16, progress: f32) -> Self {
        Self {
            id,
            max_height,
            progress,
            child: None,
        }
    }

    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.child = Some(child.into_element(cx));
        self
    }
}

impl<'a> Widget<'a> for SpringReveal<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let p = self.progress.clamp(0.0, 1.0);
        let h = ((self.max_height.max(0) as f32) * p + 0.5) as i16;
        let alpha = (p * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        let mut box_div = div()
            .id(self.id)
            .w_full()
            .h(h)
            .overflow_hidden()
            .opacity(alpha);
        if h > 0 {
            if let Some(ch) = self.child {
                box_div = box_div.child(cx, ch);
            }
        }
        box_div.into_element(cx)
    }
}
