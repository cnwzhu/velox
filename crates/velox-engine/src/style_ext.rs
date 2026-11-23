//! 组件通用外层样式扩展包（[`StyleExt`]）与 `gpui-kit` 风格统一链式修饰 Trait（[`StyledExt`]）。
//!
//! 搭配 24 字节紧凑核心 [`Style`] / [`Styled`] 使用：
//! - 任何实现了 [`Styled`] + [`StyledExt`] 的组件（涵盖全部 `velox-base` 无样式原语与 `velox-component` 主题组件）
//!   均可直接使用与 `Div` 100% 一致的 `.hover(|s| ...)`、`.active(|s| ...)`、`.focus(|s| ...)`、
//!   `.overflow_hidden()`、`.transition_colors()`、`.duration_150()`、`.ease_out()`、`.text_xs()`、
//!   `.text_color(...)`、`.shadow_md()`、`.cursor_pointer()`、`.opacity_f(...)`、`.mx_2()` 等语法；
//! - 默认未调用任何扩展修饰符时 `StyleExt::is_empty() == true`，渲染热路径零额外 `DivVisual` / `DivStateStyles` 分配。

use core::time::Duration;

use crate::element::{div, Div, Element, EnterFrom, IntoElement, TransitionSpec};
use crate::pod::{CursorShape, FrameScope};
use crate::style::{
    AlignItems, Color, Easing, Edges, FlexDir, Length, LoopMode, Style, StyleRefinement, Styled,
};

/// 除 24B 核心 [`Style`] 以外的组件外层扩展修饰符集合（零堆分配、`const` 可构造）。
#[derive(Copy, Clone, Debug)]
pub struct StyleExt {
    pub margin: Edges<i16>,
    pub min_w: i16,
    pub max_w: i16,
    pub min_h: i16,
    pub max_h: i16,
    pub opacity: Option<u8>,
    pub clip_children: Option<bool>,
    pub scrollable_max: Option<i16>,
    pub scroll_dir: Option<FlexDir>,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub shadow_cleared: bool,
    pub cursor: Option<CursorShape>,
    pub transition: Option<TransitionSpec>,
    pub transition_cleared: bool,
    pub text_color: Option<Color>,
    pub text_size: Option<u8>,
    pub hover_style: Option<StyleRefinement>,
    pub active_style: Option<StyleRefinement>,
    pub focus_style: Option<StyleRefinement>,
    pub z_index: Option<i8>,
    pub align_self: Option<AlignItems>,
    pub items_baseline: bool,
    dirty: bool,
}

impl Default for StyleExt {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl StyleExt {
    /// 空扩展样式常量（供所有组件 `const fn new` 零成本初始化）。
    pub const DEFAULT: Self = Self::new();

    /// 构造一个未设置任何扩展修饰符的空 `StyleExt`。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            margin: Edges::all(0),
            min_w: 0,
            max_w: 0,
            min_h: 0,
            max_h: 0,
            opacity: None,
            clip_children: None,
            scrollable_max: None,
            scroll_dir: None,
            shadow: None,
            shadow_cleared: false,
            cursor: None,
            transition: None,
            transition_cleared: false,
            text_color: None,
            text_size: None,
            hover_style: None,
            active_style: None,
            focus_style: None,
            z_index: None,
            align_self: None,
            items_baseline: false,
            dirty: false,
        }
    }

    /// 是否未调用过任何外层扩展修饰符（为 `true` 时渲染热路径零额外开销）。
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        !self.dirty
    }

    /// 便捷解析外部 `Styled` 宽度（若用户通过 `.w(...)` / `.w_64()` 设置了像素宽则优先使用，否则回退组件默认宽度）。
    #[inline(always)]
    pub const fn resolve_width(custom: Style, fallback: i16) -> i16 {
        match custom.width {
            Length::Px(w) => w,
            _ => fallback,
        }
    }

    /// 便捷解析外部 `Styled` 高度（若用户通过 `.h(...)` / `.h_10()` 设置了像素高则优先使用，否则回退组件默认高度）。
    #[inline(always)]
    pub const fn resolve_height(custom: Style, fallback: i16) -> i16 {
        match custom.height {
            Length::Px(h) => h,
            _ => fallback,
        }
    }

    /// 将用户通过 [`Styled`] 在外层显式设置的非默认 [`Style`] 属性合并到组件内部根 [`Style`] 上。
    #[inline]
    pub fn merge_style(mut base: Style, custom: Style) -> Style {
        if custom == Style::default() {
            return base;
        }
        if !matches!(custom.width, Length::Auto) {
            base.width = custom.width;
        }
        if !matches!(custom.height, Length::Auto) {
            base.height = custom.height;
        }
        if custom.padding != Edges::all(0) {
            base.padding = custom.padding;
        }
        if custom.bg != Color::TRANSPARENT {
            base.bg = custom.bg;
        }
        if custom.border_px() > 0 {
            base.set_border_px(custom.border_px());
        }
        if custom.border_color != Color::TRANSPARENT {
            base.border_color = custom.border_color;
        }
        if custom.rounded_radius() > 0 {
            base.set_rounded(custom.rounded_radius());
        }
        if custom.gap != 0 {
            base.gap = custom.gap;
        }
        if custom.margin_px() != 0 {
            base.set_margin_px(custom.margin_px());
        }
        if custom.flex_wrap() {
            base.set_flex_wrap(true);
        }
        if custom.flex_flags != 0 {
            if (custom.flex_flags & 0b0000_0001) != 0 {
                base.set_col();
            }
            if (custom.flex_flags & 0b0000_1110) != 0 {
                base.set_justify(custom.justify());
            }
            if (custom.flex_flags & 0b0011_0000) != 0 {
                base.set_items_align(custom.items_align());
            }
            if (custom.flex_flags & 0b1100_0000) != 0 {
                base.set_grow(custom.flex_grow());
            }
        }
        base
    }

    /// 仅将 `StyleExt` 扩展属性叠加到已构造的根 [`Div`] 上（适用于组件已自行合并 `self.style` 的场景）。
    #[inline]
    pub fn apply_ext_only<'a>(self, mut target: Div<'a>) -> Div<'a> {
        if !self.dirty {
            return target;
        }
        if self.margin != Edges::all(0) {
            target.margin = self.margin;
        }
        if self.min_w != 0 {
            target.min_w = self.min_w;
        }
        if self.max_w != 0 {
            target.max_w = self.max_w;
        }
        if self.min_h != 0 {
            target.min_h = self.min_h;
        }
        if self.max_h != 0 {
            target.max_h = self.max_h;
        }
        if let Some(op) = self.opacity {
            target.opacity = op;
        }
        if let Some(clip) = self.clip_children {
            target.clip_children = clip;
        }
        if let Some(scroll_max) = self.scrollable_max {
            target.scrollable_max = Some(scroll_max);
            target.clip_children = true;
            if let Some(dir) = self.scroll_dir {
                match dir {
                    FlexDir::Row => target.style.set_row(),
                    FlexDir::Col => target.style.set_col(),
                }
            }
        }
        if self.shadow_cleared {
            target.shadow = None;
        } else if let Some(sh) = self.shadow {
            target.shadow = Some(sh);
        }
        if let Some(c) = self.cursor {
            target.cursor = Some(c);
        }
        if self.transition_cleared {
            target.transition = None;
        } else if let Some(tr) = self.transition {
            target.transition = Some(tr);
        }
        if let Some(tc) = self.text_color {
            target.text_color = Some(tc);
        }
        if let Some(ts) = self.text_size {
            target.text_size = Some(ts);
        }
        if let Some(hr) = self.hover_style {
            target = target.hover(|base| merge_refinement(base, hr));
        }
        if let Some(ar) = self.active_style {
            target = target.active(|base| merge_refinement(base, ar));
        }
        if let Some(fr) = self.focus_style {
            target = target.focus(|base| merge_refinement(base, fr));
        }
        if let Some(z) = self.z_index {
            target.z_index = z;
        }
        if let Some(a) = self.align_self {
            target.align_self = Some(a);
        }
        if self.items_baseline {
            target.items_baseline = true;
        }
        target
    }

    /// 将外层 [`Style`] 与 [`StyleExt`] 一次性精准叠加到组件根 [`Div`] 上（无自定义修饰时零额外分配）。
    #[inline]
    pub fn apply_to_div<'a>(self, mut target: Div<'a>, custom_style: Style) -> Div<'a> {
        if custom_style != Style::default() {
            target.style = Self::merge_style(target.style, custom_style);
        }
        self.apply_ext_only(target)
    }

    /// 供输出非 `Div` 元素（如 `Element::Img` / `Element::Text`）的原子组件使用：
    /// 未设置外层样式时零包装直接透传，设置任意外层样式时自动用 `Div` 包裹并完整应用。
    #[inline]
    pub fn wrap_element<'a>(
        self,
        cx: &'a FrameScope,
        elem: Element<'a>,
        custom_style: Style,
    ) -> Element<'a> {
        if custom_style == Style::default() && !self.dirty {
            return elem;
        }
        self.apply_to_div(div(), custom_style)
            .child(cx, elem)
            .into_element(cx)
    }
}

#[inline(always)]
fn merge_refinement(mut base: StyleRefinement, overlay: StyleRefinement) -> StyleRefinement {
    if overlay.bg.is_some() {
        base.bg = overlay.bg;
    }
    if overlay.border_width.is_some() {
        base.border_width = overlay.border_width;
    }
    if overlay.border_color.is_some() {
        base.border_color = overlay.border_color;
    }
    if overlay.text_color.is_some() {
        base.text_color = overlay.text_color;
    }
    if overlay.opacity.is_some() {
        base.opacity = overlay.opacity;
    }
    if overlay.rounded.is_some() {
        base.rounded = overlay.rounded;
    }
    base
}

#[inline(always)]
fn duration_ms_u16(d: Duration) -> u16 {
    d.as_millis().min(u16::MAX as u128) as u16
}

/// 统一 `gpui-kit` / Tailwind 风格扩展修饰符 Trait（覆盖交互态闭包、过渡动画、溢出裁剪、容器文本继承、阴影、光标与外边距）。
///
/// 所有 `velox-base` 无样式原语与 `velox-component` 主题化组件均实现此 Trait，
/// 使任意组件均可像 `div()` 一样自由编写 `.hover(|s| ...)`、`.transition_colors()`、`.overflow_hidden()`、`.text_xs()` 等链式语法。
pub trait StyledExt: Styled + Sized {
    /// 获取内部 [`StyleExt`] 可变引用。
    fn style_ext_mut(&mut self) -> &mut StyleExt;

    // --- 交互态闭包（Hover / Active / Focus） ---

    /// GPUI 风格悬停态样式闭包（如 `.hover(|s| s.bg(rgb(0xf5f5f5)).border_color(rgb(0x171717)))`）。
    #[inline(always)]
    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        let ext = self.style_ext_mut();
        let refined = f(ext.hover_style.unwrap_or_default());
        if !refined.is_empty() {
            ext.hover_style = Some(refined);
            ext.dirty = true;
        }
        self
    }

    /// GPUI 风格按下态样式闭包（如 `.active(|s| s.bg(rgb(0xe5e5e5)))`）。
    #[inline(always)]
    fn active(self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.active_style(f)
    }

    /// 按下态样式闭包显式别名（避免与少数自带 `.active(id)` 固有方法的导航组件冲突）。
    #[inline(always)]
    fn active_style(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        let ext = self.style_ext_mut();
        let refined = f(ext.active_style.unwrap_or_default());
        if !refined.is_empty() {
            ext.active_style = Some(refined);
            ext.dirty = true;
        }
        self
    }

    /// GPUI 风格聚焦态样式闭包（如 `.focus(|s| s.border_1().border_color(rgb(0x3b82f6)))`）。
    #[inline(always)]
    fn focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        let ext = self.style_ext_mut();
        let refined = f(ext.focus_style.unwrap_or_default());
        if !refined.is_empty() {
            ext.focus_style = Some(refined);
            ext.dirty = true;
        }
        self
    }

    // --- 溢出与滚动控制 ---

    /// 启用溢出裁剪（等价 CSS `overflow: hidden`）。
    #[inline(always)]
    fn overflow_hidden(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.clip_children = Some(true);
        ext.dirty = true;
        self
    }

    /// 关闭溢出裁剪（等价 CSS `overflow: visible`）。
    #[inline(always)]
    fn overflow_visible(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.clip_children = Some(false);
        ext.dirty = true;
        self
    }

    /// 启用自动测量边界的滚动视口（等价 CSS `overflow: auto/scroll`）。
    #[inline(always)]
    fn overflow_scroll(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.scrollable_max = Some(i16::MAX);
        ext.clip_children = Some(true);
        ext.dirty = true;
        self
    }

    /// 启用垂直方向自动滚动视口（等价 CSS `overflow-y: auto/scroll`）。
    #[inline(always)]
    fn overflow_y_scroll(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.scrollable_max = Some(i16::MAX);
        ext.scroll_dir = Some(FlexDir::Col);
        ext.clip_children = Some(true);
        ext.dirty = true;
        self
    }

    /// 启用水平方向自动滚动视口（等价 CSS `overflow-x: auto/scroll`）。
    #[inline(always)]
    fn overflow_x_scroll(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.scrollable_max = Some(i16::MAX);
        ext.scroll_dir = Some(FlexDir::Row);
        ext.clip_children = Some(true);
        ext.dirty = true;
        self
    }

    /// 启用指定最大滚动距离的可滚动视口。
    #[inline(always)]
    fn scrollable(mut self, max_scroll: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.scrollable_max = Some(max_scroll.max(0));
        ext.clip_children = Some(true);
        ext.dirty = true;
        self
    }

    // --- 过渡与缓动动画（Tailwind / GPUI 语法） ---

    /// 默认过渡动画（150ms 颜色 + 不透明度 + 几何边界）。
    #[inline(always)]
    fn transition(self) -> Self {
        self.animate_all(Duration::from_millis(150))
    }

    /// 颜色过渡简写（150ms 背景/边框/文本色平滑过渡）。
    #[inline(always)]
    fn transition_colors(self) -> Self {
        self.animate_color(Duration::from_millis(150))
    }

    /// 不透明度过渡简写（150ms 淡入淡出）。
    #[inline(always)]
    fn transition_opacity(self) -> Self {
        self.animate_opacity(Duration::from_millis(150))
    }

    /// 全属性过渡简写（150ms 颜色 + 边界 + 不透明度）。
    #[inline(always)]
    fn transition_all(self) -> Self {
        self.animate_all(Duration::from_millis(150))
    }

    /// 显式禁用过渡动画（等价 CSS `transition: none`）。
    #[inline(always)]
    fn transition_none(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.transition = None;
        ext.transition_cleared = true;
        ext.dirty = true;
        self
    }

    /// 设置颜色跨帧过渡时长。
    #[inline(always)]
    fn animate_color(mut self, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        spec.animate_color = true;
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 设置背景色跨帧过渡时长。
    #[inline(always)]
    fn animate_bg(self, duration: Duration) -> Self {
        self.animate_color(duration)
    }

    /// 设置几何边界跨帧过渡时长。
    #[inline(always)]
    fn animate_bounds(mut self, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        spec.animate_bounds = true;
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 设置不透明度跨帧过渡时长。
    #[inline(always)]
    fn animate_opacity(mut self, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        spec.animate_opacity = true;
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 设置全属性跨帧过渡时长。
    #[inline(always)]
    fn animate_all(mut self, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        spec.animate_color = true;
        spec.animate_bounds = true;
        spec.animate_opacity = true;
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 调整过渡动画时长。
    #[inline(always)]
    fn duration(mut self, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    #[inline(always)] fn duration_75(self) -> Self { self.duration(Duration::from_millis(75)) }
    #[inline(always)] fn duration_100(self) -> Self { self.duration(Duration::from_millis(100)) }
    #[inline(always)] fn duration_150(self) -> Self { self.duration(Duration::from_millis(150)) }
    #[inline(always)] fn duration_200(self) -> Self { self.duration(Duration::from_millis(200)) }
    #[inline(always)] fn duration_300(self) -> Self { self.duration(Duration::from_millis(300)) }
    #[inline(always)] fn duration_500(self) -> Self { self.duration(Duration::from_millis(500)) }

    /// 调整过渡动画缓动曲线。
    #[inline(always)]
    fn ease(mut self, easing: Easing) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.easing = easing;
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    #[inline(always)] fn ease_linear(self) -> Self { self.ease(Easing::Linear) }
    #[inline(always)] fn ease_in(self) -> Self { self.ease(Easing::EaseIn) }
    #[inline(always)] fn ease_out(self) -> Self { self.ease(Easing::EaseOut) }
    #[inline(always)] fn ease_in_out(self) -> Self { self.ease(Easing::EaseInOut) }
    #[inline(always)] fn ease_spring(self) -> Self { self.ease(Easing::Spring) }

    /// 设置动画延迟时间。
    #[inline(always)]
    fn delay(mut self, delay: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.delay_ms = duration_ms_u16(delay);
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 声明动画循环模式。
    #[inline(always)]
    fn loop_mode(mut self, mode: LoopMode) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.loop_mode = mode;
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 声明入场淡入动画。
    #[inline(always)]
    fn fade_in(mut self, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        spec.enter = Some(EnterFrom { opacity: 0, dx: 0, dy: 0 });
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    /// 声明入场滑入淡入动画。
    #[inline(always)]
    fn slide_in(mut self, dx: i16, dy: i16, duration: Duration) -> Self {
        let ext = self.style_ext_mut();
        let mut spec = ext.transition.unwrap_or_default();
        spec.duration_ms = duration_ms_u16(duration);
        spec.enter = Some(EnterFrom { opacity: 0, dx, dy });
        ext.transition = Some(spec);
        ext.transition_cleared = false;
        ext.dirty = true;
        self
    }

    // --- 容器级可继承文本样式 ---

    /// 设置容器级可继承文本颜色。
    #[inline(always)]
    fn text_color(mut self, color: Color) -> Self {
        let ext = self.style_ext_mut();
        ext.text_color = Some(color);
        ext.dirty = true;
        self
    }

    /// 设置容器级可继承字号（像素）。
    #[inline(always)]
    fn text_size(mut self, px: u8) -> Self {
        let ext = self.style_ext_mut();
        ext.text_size = Some(px);
        ext.dirty = true;
        self
    }

    #[inline(always)] fn text_xs(self) -> Self { self.text_size(11) }
    #[inline(always)] fn text_sm(self) -> Self { self.text_size(12) }
    #[inline(always)] fn text_base(self) -> Self { self.text_size(14) }
    #[inline(always)] fn text_lg(self) -> Self { self.text_size(16) }
    #[inline(always)] fn text_xl(self) -> Self { self.text_size(18) }
    #[inline(always)] fn text_2xl(self) -> Self { self.text_size(22) }
    #[inline(always)] fn text_3xl(self) -> Self { self.text_size(28) }

    // --- 不透明度、阴影与光标 ---

    /// 设置组件整体不透明度（`0..=255`）。
    #[inline(always)]
    fn opacity(mut self, alpha: u8) -> Self {
        let ext = self.style_ext_mut();
        ext.opacity = Some(alpha);
        ext.dirty = true;
        self
    }

    /// 浮点不透明度简写（`0.0..=1.0`）。
    #[inline(always)]
    fn opacity_f(self, alpha: f32) -> Self {
        let c = if alpha <= 0.0 {
            0.0
        } else if alpha >= 1.0 {
            1.0
        } else {
            alpha
        };
        self.opacity((c * 255.0 + 0.5) as u8)
    }

    /// 设置解析式 SDF 外阴影。
    #[inline(always)]
    fn shadow(mut self, blur: u8, offset_x: i8, offset_y: i8, color: Color) -> Self {
        let ext = self.style_ext_mut();
        if blur > 0 && color.a() > 0 {
            ext.shadow = Some((blur.min(64), offset_x, offset_y, color));
            ext.shadow_cleared = false;
            ext.dirty = true;
        }
        self
    }

    #[inline(always)]
    fn shadow_none(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.shadow = None;
        ext.shadow_cleared = true;
        ext.dirty = true;
        self
    }

    #[inline(always)] fn shadow_sm(self) -> Self { self.shadow(4, 0, 1, Color::rgba(0, 0, 0, 48)) }
    #[inline(always)] fn shadow_md(self) -> Self { self.shadow(10, 0, 3, Color::rgba(0, 0, 0, 72)) }
    #[inline(always)] fn shadow_lg(self) -> Self { self.shadow(18, 0, 6, Color::rgba(0, 0, 0, 96)) }
    #[inline(always)] fn shadow_xl(self) -> Self { self.shadow(28, 0, 10, Color::rgba(0, 0, 0, 120)) }

    /// 显式设置鼠标悬停光标形状。
    #[inline(always)]
    fn cursor(self, cursor: CursorShape) -> Self {
        self.cursor_shape(cursor)
    }

    /// 鼠标光标形状显式别名（避免与 `TextInput::cursor(byte_offset)` 冲突）。
    #[inline(always)]
    fn cursor_shape(mut self, cursor: CursorShape) -> Self {
        let ext = self.style_ext_mut();
        ext.cursor = Some(cursor);
        ext.dirty = true;
        self
    }

    #[inline(always)] fn cursor_pointer(self) -> Self { self.cursor_shape(CursorShape::Pointer) }
    #[inline(always)] fn cursor_default(self) -> Self { self.cursor_shape(CursorShape::Default) }
    #[inline(always)] fn cursor_text(self) -> Self { self.cursor_shape(CursorShape::Text) }
    #[inline(always)] fn cursor_not_allowed(self) -> Self { self.cursor_shape(CursorShape::NotAllowed) }
    #[inline(always)] fn cursor_grab(self) -> Self { self.cursor_shape(CursorShape::Grab) }
    #[inline(always)] fn cursor_grabbing(self) -> Self { self.cursor_shape(CursorShape::Grabbing) }

    // --- 外边距与最小/最大尺寸约束 ---

    #[inline(always)]
    fn mx(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.margin.left = px;
        ext.margin.right = px;
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn my(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.margin.top = px;
        ext.margin.bottom = px;
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn mt(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.margin.top = px;
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn mb(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.margin.bottom = px;
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn ml(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.margin.left = px;
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn mr(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.margin.right = px;
        ext.dirty = true;
        self
    }

    #[inline(always)] fn mx_0(self) -> Self { self.mx(0) }
    #[inline(always)] fn mx_1(self) -> Self { self.mx(4) }
    #[inline(always)] fn mx_2(self) -> Self { self.mx(8) }
    #[inline(always)] fn mx_3(self) -> Self { self.mx(12) }
    #[inline(always)] fn mx_4(self) -> Self { self.mx(16) }
    #[inline(always)] fn my_0(self) -> Self { self.my(0) }
    #[inline(always)] fn my_1(self) -> Self { self.my(4) }
    #[inline(always)] fn my_2(self) -> Self { self.my(8) }
    #[inline(always)] fn my_3(self) -> Self { self.my(12) }
    #[inline(always)] fn my_4(self) -> Self { self.my(16) }
    #[inline(always)] fn mt_1(self) -> Self { self.mt(4) }
    #[inline(always)] fn mt_2(self) -> Self { self.mt(8) }
    #[inline(always)] fn mt_3(self) -> Self { self.mt(12) }
    #[inline(always)] fn mt_4(self) -> Self { self.mt(16) }
    #[inline(always)] fn mb_1(self) -> Self { self.mb(4) }
    #[inline(always)] fn mb_2(self) -> Self { self.mb(8) }
    #[inline(always)] fn mb_3(self) -> Self { self.mb(12) }
    #[inline(always)] fn mb_4(self) -> Self { self.mb(16) }
    #[inline(always)] fn ml_1(self) -> Self { self.ml(4) }
    #[inline(always)] fn ml_2(self) -> Self { self.ml(8) }
    #[inline(always)] fn ml_3(self) -> Self { self.ml(12) }
    #[inline(always)] fn ml_4(self) -> Self { self.ml(16) }
    #[inline(always)] fn mr_1(self) -> Self { self.mr(4) }
    #[inline(always)] fn mr_2(self) -> Self { self.mr(8) }
    #[inline(always)] fn mr_3(self) -> Self { self.mr(12) }
    #[inline(always)] fn mr_4(self) -> Self { self.mr(16) }

    #[inline(always)]
    fn min_w(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.min_w = px.max(0);
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn max_w(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.max_w = px.max(0);
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn min_h(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.min_h = px.max(0);
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn max_h(mut self, px: i16) -> Self {
        let ext = self.style_ext_mut();
        ext.max_h = px.max(0);
        ext.dirty = true;
        self
    }

    // --- 自身对齐、层级与语义显示标记 ---

    #[inline(always)]
    fn align_self(mut self, align: AlignItems) -> Self {
        let ext = self.style_ext_mut();
        ext.align_self = Some(align);
        ext.dirty = true;
        self
    }

    #[inline(always)] fn self_start(self) -> Self { self.align_self(AlignItems::Start) }
    #[inline(always)] fn self_center(self) -> Self { self.align_self(AlignItems::Center) }
    #[inline(always)] fn self_end(self) -> Self { self.align_self(AlignItems::End) }
    #[inline(always)] fn self_stretch(self) -> Self { self.align_self(AlignItems::Stretch) }

    #[inline(always)]
    fn items_baseline(mut self) -> Self {
        let ext = self.style_ext_mut();
        ext.items_baseline = true;
        ext.dirty = true;
        self
    }

    #[inline(always)]
    fn z_index(mut self, z: i8) -> Self {
        let ext = self.style_ext_mut();
        ext.z_index = Some(z);
        ext.dirty = true;
        self
    }

    /// 语义化相对定位标记（Velox 子节点天然相对父容器定位）。
    #[inline(always)]
    fn relative(self) -> Self {
        self
    }

    /// 块级垂直容器简写（等价 `.flex_col()`）。
    #[inline(always)]
    fn block(self) -> Self {
        self.flex_col()
    }

    /// 行内弹性容器简写（等价 `.flex_row()`）。
    #[inline(always)]
    fn inline_flex(self) -> Self {
        self.flex_row()
    }
}
