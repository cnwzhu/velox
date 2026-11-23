//! 无样式连续数值滑块行为原语 `Slider`（步进约束、点击轨道直跳/连续拖拽与方向键/`Home`/`End` 键盘微调；
//! 默认直角中性轨道与滑块、零主题蓝填充、零阴影与零悬停渐变）。

use crate::traits::ComponentSize;
use velox_core::prelude::*;

const FLAG_CUSTOM_COLORS: u8 = 1 << 0;
const FLAG_CUSTOM_HEIGHT: u8 = 1 << 1;

#[inline(always)]
fn snap_to_step(val: i32, min: i32, max: i32, step: i32) -> i32 {
    let clamped = val.clamp(min, max);
    let s = step.max(1);
    if s <= 1 {
        return clamped;
    }
    let offset = clamped - min;
    let snapped = min + ((offset + (s >> 1)) / s) * s;
    snapped.clamp(min, max)
}

/// 传递给 `Slider::track(...)` / `Slider::indicator(...)` / `Slider::thumb(...)` 插槽回调的状态。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SliderState {
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub disabled: bool,
}

impl SliderState {
    #[inline(always)] pub const fn value(&self) -> i32 { self.value }
    #[inline(always)] pub const fn min(&self) -> i32 { self.min }
    #[inline(always)] pub const fn max(&self) -> i32 { self.max }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
    #[inline(always)]
    pub fn ratio(&self) -> f32 {
        let span = (self.max - self.min).max(1) as f32;
        ((self.value.clamp(self.min, self.max) - self.min) as f32) / span
    }
}

type SliderPartRenderer<'a> = &'a dyn Fn(Div<'a>, SliderState, &'a FrameScope) -> Element<'a>;

/// 无样式可拖拽/可点击/可键盘微调数值滑块原语（`Slider`）。
pub struct Slider<'a> {
    pub id: u32,
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub width: i16,
    pub height: i16,
    pub size: ComponentSize,
    pub disabled: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub track_bg: Color,
    pub track_border_color: Color,
    pub fill_bg: Color,
    pub thumb_bg: Color,
    pub thumb_border_color: Color,
    pub thumb_border_width: u8,
    pub track_rounded: Option<u8>,
    pub thumb_rounded: Option<u8>,
    pub thumb_shadow: Option<(u8, i8, i8, Color)>,
    pub thumb_hover_bg: Option<Color>,
    pub thumb_active_bg: Option<Color>,
    pub on_change: Option<&'a dyn Fn(i32, &mut ActionScope)>,
    track_renderer: Option<SliderPartRenderer<'a>>,
    indicator_renderer: Option<SliderPartRenderer<'a>>,
    thumb_renderer: Option<SliderPartRenderer<'a>>,
    custom_flags: u8,
}

impl<'a> Slider<'a> {
    /// 创建一个无样式滑块原语，默认范围 `0..=100`、步进 `1`、尺寸 `160x20`（直角中性轨道与滑块，无阴影/主题蓝）。
    #[inline(always)]
    pub const fn new(id: u32, value: i32) -> Self {
        Self {
            id,
            value,
            min: 0,
            max: 100,
            step: 1,
            width: 160,
            height: 20,
            size: ComponentSize::Medium,
            disabled: false,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            track_bg: Color::TRANSPARENT,
            track_border_color: Color::TRANSPARENT,
            fill_bg: Color::TRANSPARENT,
            thumb_bg: Color::TRANSPARENT,
            thumb_border_color: Color::TRANSPARENT,
            thumb_border_width: 0,
            track_rounded: None,
            thumb_rounded: None,
            thumb_shadow: None,
            thumb_hover_bg: None,
            thumb_active_bg: None,
            on_change: None,
            track_renderer: None,
            indicator_renderer: None,
            thumb_renderer: None,
            custom_flags: 0,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<i32>`]：渲染期自动建立细粒度订阅，拖动或按键时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<i32>) -> Self {
        let cb = cx.arena().alloc(move |v: i32, _: &mut ActionScope| {
            sig.set_if_changed(v);
        });
        Self::new(id, sig.get()).on_change(cb)
    }

    /// 设置滑块取值范围 `min..=max`。
    #[inline(always)]
    pub const fn range(mut self, min: i32, max: i32) -> Self {
        self.min = min;
        self.max = if max > min { max } else { min + 1 };
        self
    }

    /// 设置拖拽、点击与键盘方向键调节的步进增量（默认 `1`）。
    #[inline(always)]
    pub const fn step(mut self, step: i32) -> Self {
        self.step = if step > 0 { step } else { 1 };
        self
    }

    /// 设置滑块总宽度（像素）。
    #[inline(always)]
    pub const fn w(mut self, width: i16) -> Self {
        self.width = if width > 32 { width } else { 32 };
        self
    }

    /// 设置滑块交互高度（像素）。
    #[inline(always)]
    pub const fn h(mut self, height: i16) -> Self {
        self.height = if height > 12 { height } else { 12 };
        self.custom_flags |= FLAG_CUSTOM_HEIGHT;
        self
    }

    /// 自定义激活填充与滑块边框颜色。
    #[inline(always)]
    pub const fn accent(mut self, color: Color) -> Self {
        self.fill_bg = color;
        self.thumb_border_color = color;
        self
    }

    /// 设置轨道、填充与滑块颜色。
    #[inline(always)]
    pub const fn colors(
        mut self,
        track_bg: Color,
        track_border_color: Color,
        fill_bg: Color,
        thumb_bg: Color,
        thumb_border_color: Color,
    ) -> Self {
        self.track_bg = track_bg;
        self.track_border_color = track_border_color;
        self.fill_bg = fill_bg;
        self.thumb_bg = thumb_bg;
        self.thumb_border_color = thumb_border_color;
        self.custom_flags |= FLAG_CUSTOM_COLORS;
        self
    }

    /// 设置禁用状态。
    #[inline(always)]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 绑定数值变更回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(i32, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// `gpui-base` 风格轨道插槽定制回调（零堆分配）。
    #[inline]
    pub fn track<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SliderState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: SliderState, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.track_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格已填充指示条插槽定制回调（零堆分配）。
    #[inline]
    pub fn indicator<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SliderState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: SliderState, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.indicator_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格拖拽滑块插槽定制回调（零堆分配）。
    #[inline]
    pub fn thumb<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SliderState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: SliderState, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.thumb_renderer = Some(cb);
        self
    }

    /// 是否已显式配置过颜色组。
    #[inline(always)]
    pub const fn has_custom_colors(&self) -> bool {
        (self.custom_flags & FLAG_CUSTOM_COLORS) != 0
    }

    /// 计算当前值在给定可用像素跨度 `usable_w` 下的偏移像素。
    #[inline(always)]
    pub fn thumb_offset(&self, usable_w: i16) -> i16 {
        let clamped = self.value.clamp(self.min, self.max);
        let span = (self.max - self.min).max(1) as i64;
        (((clamped - self.min) as i64 * usable_w as i64) / span) as i16
    }
}

crate::impl_base_styled_box!(Slider<'a>);
crate::impl_base_disableable!(Slider<'a>);
crate::impl_base_sizable!(Slider<'a>);

impl<'a> Widget<'a> for Slider<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let eff_width = StyleExt::resolve_width(self.style, self.width).max(32);
        let default_h = if (self.custom_flags & FLAG_CUSTOM_HEIGHT) != 0 || self.height != 20 {
            self.height
        } else {
            match self.size {
                ComponentSize::Small => 16,
                ComponentSize::Medium => 20,
                ComponentSize::Large => 24,
            }
        };
        let height = StyleExt::resolve_height(self.style, default_h).max(12);

        let (track_bg, track_border_color, fill_bg, thumb_bg, thumb_border_color) =
            if (self.custom_flags & FLAG_CUSTOM_COLORS) != 0 {
                (
                    self.track_bg,
                    self.track_border_color,
                    self.fill_bg,
                    self.thumb_bg,
                    self.thumb_border_color,
                )
            } else {
                (
                    if self.track_bg == Color::TRANSPARENT {
                        def_fg.scale_alpha(48)
                    } else {
                        self.track_bg
                    },
                    self.track_border_color,
                    if self.fill_bg == Color::TRANSPARENT {
                        def_fg
                    } else {
                        self.fill_bg
                    },
                    if self.thumb_bg == Color::TRANSPARENT {
                        def_fg
                    } else {
                        self.thumb_bg
                    },
                    self.thumb_border_color,
                )
            };

        let thumb_sz = (height - 4).clamp(12, 20);
        let track_h = match self.size {
            ComponentSize::Small => 4i16,
            ComponentSize::Medium => 6i16,
            ComponentSize::Large => 8i16,
        }
        .min((height - 4).max(4));
        let track_y = (height - track_h) >> 1;
        let usable_w = (eff_width - thumb_sz).max(1);
        let thumb_x = self.thumb_offset(usable_w);
        let fill_w = (thumb_x + (thumb_sz >> 1)).clamp(0, eff_width);
        let track_r = self.track_rounded.unwrap_or(0);
        let thumb_r = self.thumb_rounded.unwrap_or(0);
        let sl_state = SliderState {
            value: self.value,
            min: self.min,
            max: self.max,
            disabled: self.disabled,
        };

        let mut root = div().id(self.id).w(eff_width).h(height);
        if self.disabled {
            root = root.opacity(115).cursor(CursorShape::NotAllowed);
        } else {
            root = root
                .focusable()
                .no_focus_border()
                .cursor(CursorShape::Pointer);
            if let Some(cb) = self.on_change {
                let min_v = self.min;
                let max_v = self.max;
                let step_v = self.step.max(1);
                let cur_v = self.value.clamp(min_v, max_v);
                let span = (max_v - min_v).max(1);
                let half_thumb = thumb_sz >> 1;

                let raw_drag = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let ev = acx.drag_event();
                    let eff_w = (ev.width - thumb_sz).max(1) as i32;
                    let adj_x = (ev.local_x - half_thumb).clamp(0, eff_w as i16) as i32;
                    let raw_val = min_v + ((adj_x * span + (eff_w >> 1)) / eff_w);
                    cb(snap_to_step(raw_val, min_v, max_v, step_v), acx);
                });
                root = root.on_drag_raw(raw_drag);

                let nav_cb = cx.arena().alloc(move |key: KeyCode, acx: &mut ActionScope| {
                    let next = match key {
                        KeyCode::Left | KeyCode::Down => {
                            snap_to_step(cur_v.saturating_sub(step_v), min_v, max_v, step_v)
                        }
                        KeyCode::Right | KeyCode::Up => {
                            snap_to_step(cur_v.saturating_add(step_v), min_v, max_v, step_v)
                        }
                        KeyCode::Home => min_v,
                        KeyCode::End => max_v,
                        _ => return,
                    };
                    if next != cur_v {
                        cb(next, acx);
                    }
                });
                root = root.on_key_nav(nav_cb);
            }
        }

        let track_bg_el = if let Some(track_fn) = self.track_renderer {
            track_fn(div().pos(0, track_y).w(eff_width).h(track_h), sl_state, cx)
        } else {
            let mut t = div()
                .pos(0, track_y)
                .w(eff_width)
                .h(track_h)
                .rounded(track_r)
                .bg(track_bg);
            if track_border_color.a() > 0 {
                t = t.border(1, track_border_color);
            }
            t.into_element(cx)
        };

        let track_fill = if let Some(ind_fn) = self.indicator_renderer {
            ind_fn(div().pos(0, track_y).w(fill_w).h(track_h), sl_state, cx)
        } else {
            div()
                .pos(0, track_y)
                .w(fill_w)
                .h(track_h)
                .rounded(track_r)
                .bg(fill_bg)
                .into_element(cx)
        };

        let thumb_y = (height - thumb_sz) >> 1;
        let thumb_el = if let Some(thumb_fn) = self.thumb_renderer {
            thumb_fn(
                div()
                    .id(self.id ^ 0x2000_0000)
                    .pos(thumb_x, thumb_y)
                    .w(thumb_sz)
                    .h(thumb_sz),
                sl_state,
                cx,
            )
        } else {
            let mut thumb = div()
                .id(self.id ^ 0x2000_0000)
                .pos(thumb_x, thumb_y)
                .w(thumb_sz)
                .h(thumb_sz)
                .rounded(thumb_r)
                .bg(thumb_bg);
            if thumb_border_color.a() > 0 {
                let bw = if self.thumb_border_width > 0 {
                    self.thumb_border_width
                } else {
                    2
                };
                thumb = thumb.border(bw, thumb_border_color);
            }
            if let Some((blur, ox, oy, col)) = self.thumb_shadow {
                thumb = thumb.shadow(blur, ox, oy, col);
            }
            if !self.disabled {
                if let Some(h) = self.thumb_hover_bg {
                    thumb = thumb.hover_bg(h);
                }
                if let Some(a) = self.thumb_active_bg {
                    thumb = thumb.active_bg(a);
                }
            }
            thumb.into_element(cx)
        };

        let root = root
            .child(cx, track_bg_el)
            .child(cx, track_fill)
            .child(cx, thumb_el);
        self.style_ext
            .apply_to_div(root, self.style)
            .into_element(cx)
    }
}
