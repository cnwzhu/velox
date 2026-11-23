//! 无样式数值步进输入框行为原语 `NumberInput`（默认零背景、零边框、零圆角与零悬停渐变）。

use crate::traits::ComponentSize;
use velox_engine::prelude::*;

/// 数值调整步进动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberStep {
    /// 递增一个步长。
    Increment,
    /// 递减一个步长。
    Decrement,
}

/// 根据步进动作、步长与上下限计算新数值。
#[inline]
pub fn step_value(value: i64, step: i64, min: i64, max: i64, action: NumberStep) -> i64 {
    let delta = match action {
        NumberStep::Increment => step.max(1),
        NumberStep::Decrement => -(step.max(1)),
    };
    value.saturating_add(delta).clamp(min, max)
}

type NumberStepButtonRenderer<'a> =
    &'a dyn Fn(Div<'a>, NumberStep, bool, &'a FrameScope) -> Element<'a>;

/// 无样式数值步进输入框原语（`NumberInput`）。
pub struct NumberInput<'a> {
    pub id: u32,
    pub value: i64,
    pub min: i64,
    pub max: i64,
    pub step: i64,
    pub prefix: &'a str,
    pub suffix: &'a str,
    pub width: i16,
    pub disabled: bool,
    pub size: ComponentSize,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub button_bg: Color,
    pub button_hover_bg: Option<Color>,
    pub button_active_bg: Option<Color>,
    pub icon_color: Color,
    pub focus_ring: bool,
    pub rounded: u8,
    pub on_change: Option<&'a dyn Fn(i64, &mut ActionScope)>,
    step_button_renderer: Option<NumberStepButtonRenderer<'a>>,
}

impl<'a> NumberInput<'a> {
    /// 创建一个无样式数值步进输入框原语。
    #[inline(always)]
    pub const fn new(id: u32, value: i64) -> Self {
        Self {
            id,
            value,
            min: i64::MIN,
            max: i64::MAX,
            step: 1,
            prefix: "",
            suffix: "",
            width: 140,
            disabled: false,
            size: ComponentSize::Medium,
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
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            button_bg: Color::TRANSPARENT,
            button_hover_bg: None,
            button_active_bg: None,
            icon_color: Color::TRANSPARENT,
            focus_ring: false,
            rounded: 0,
            on_change: None,
            step_button_renderer: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<i64>`]：渲染期自动建立细粒度订阅，加减步进时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<i64>) -> Self {
        let cb = cx.arena().alloc(move |v: i64, _: &mut ActionScope| {
            sig.set_if_changed(v);
        });
        Self::new(id, sig.get()).on_change(cb)
    }

    /// 设置最小/最大边界。
    #[inline(always)]
    pub const fn range(mut self, min: i64, max: i64) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// 设置每次步进增量。
    #[inline(always)]
    pub const fn step(mut self, step: i64) -> Self {
        self.step = step;
        self
    }

    /// 设置前缀文本（如 `"$"`）。
    #[inline(always)]
    pub const fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }

    /// 设置后缀单位文本（如 `"px"`、`"%"`）。
    #[inline(always)]
    pub const fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }

    /// 设置组件宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定数值变更回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(i64, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }
    /// `gpui-base` 风格加/减步进按钮插槽定制回调（零堆分配）。
    #[inline]
    pub fn step_button<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, NumberStep, bool) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |btn: Div<'a>, step: NumberStep, disabled: bool, frame_cx: &'a FrameScope| {
                f(btn, step, disabled).into_element(frame_cx)
            },
        );
        self.step_button_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(NumberInput<'a>);
crate::impl_base_sizable!(NumberInput<'a>);
crate::impl_base_disableable!(NumberInput<'a>);

impl<'a> Widget<'a> for NumberInput<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let icon_fg = if self.icon_color == Color::TRANSPARENT {
            text_color
        } else {
            self.icon_color
        };

        let pad = 3i16;
        let (h, icon_sz, font_sz) = match self.size {
            ComponentSize::Small => (26i16, 11i16, 12u8),
            ComponentSize::Medium => (32i16, 12i16, 13u8),
            ComponentSize::Large => (38i16, 14i16, 14u8),
        };
        let btn_sz = (h - pad * 2).max(16);
        let btn_r = if self.rounded > 0 {
            self.rounded.saturating_sub(2).max(2)
        } else {
            0
        };
        let can_dec = !self.disabled && self.value > self.min;
        let can_inc = !self.disabled && self.value < self.max;

        let mut dec_btn = div()
            .id(crate::scoped_child_id(self.id, 0x9039_CF33, 1))
            .w(btn_sz)
            .h(btn_sz)
            .rounded(btn_r)
            .bg(self.button_bg)
            .items_center()
            .justify_center()
            .child(
                cx,
                svg_icon(icon_sz, icon_sz, "M5 12h14")
                    .tint(icon_fg)
                    .stroke_width(22),
            );

        let mut inc_btn = div()
            .id(crate::scoped_child_id(self.id, 0x9039_CF33, 2))
            .w(btn_sz)
            .h(btn_sz)
            .rounded(btn_r)
            .bg(self.button_bg)
            .items_center()
            .justify_center()
            .child(
                cx,
                svg_icon(icon_sz, icon_sz, "M12 5v14M5 12h14")
                    .tint(icon_fg)
                    .stroke_width(22),
            );

        if can_dec {
            dec_btn = dec_btn.cursor(CursorShape::Pointer);
            if let Some(h) = self.button_hover_bg {
                dec_btn = dec_btn.hover_bg(h);
            }
            if let Some(a) = self.button_active_bg {
                dec_btn = dec_btn.active_bg(a);
            }
        } else if !self.disabled {
            dec_btn = dec_btn.opacity(95).cursor(CursorShape::NotAllowed);
        }
        if can_inc {
            inc_btn = inc_btn.cursor(CursorShape::Pointer);
            if let Some(h) = self.button_hover_bg {
                inc_btn = inc_btn.hover_bg(h);
            }
            if let Some(a) = self.button_active_bg {
                inc_btn = inc_btn.active_bg(a);
            }
        } else if !self.disabled {
            inc_btn = inc_btn.opacity(95).cursor(CursorShape::NotAllowed);
        }

        let nav_cb: Option<&'a dyn Fn(KeyCode, &mut ActionScope)> = if !self.disabled {
            if let Some(cb) = self.on_change {
                let val = self.value;
                let step = self.step;
                let min = self.min;
                let max = self.max;
                if can_dec {
                    let dec_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        let next = step_value(val, step, min, max, NumberStep::Decrement);
                        cb(next, scope);
                    });
                    dec_btn = dec_btn.on_click_raw(dec_cb);
                }
                if can_inc {
                    let inc_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        let next = step_value(val, step, min, max, NumberStep::Increment);
                        cb(next, scope);
                    });
                    inc_btn = inc_btn.on_click_raw(inc_cb);
                }
                let nav = cx.arena().alloc(move |key: KeyCode, scope: &mut ActionScope| {
                    let next = match key {
                        KeyCode::Up | KeyCode::Right => {
                            step_value(val, step, min, max, NumberStep::Increment)
                        }
                        KeyCode::Down | KeyCode::Left => {
                            step_value(val, step, min, max, NumberStep::Decrement)
                        }
                        _ => return,
                    };
                    if next != val {
                        cb(next, scope);
                    }
                });
                Some(nav as &'a dyn Fn(KeyCode, &mut ActionScope))
            } else {
                None
            }
        } else {
            None
        };

        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let display_str = fmt!(cx, "{}{}{}", self.prefix, self.value, self.suffix);

        let dec_el = if let Some(btn_fn) = self.step_button_renderer {
            let mut slot = div().id(crate::scoped_child_id(self.id, 0x9039_CF33, 1));
            if let Some(cb) = dec_btn.on_click {
                slot = slot.on_click_raw(cb);
            }
            btn_fn(slot, NumberStep::Decrement, !can_dec, cx)
        } else {
            dec_btn.into_element(cx)
        };
        let inc_el = if let Some(btn_fn) = self.step_button_renderer {
            let mut slot = div().id(crate::scoped_child_id(self.id, 0x9039_CF33, 2));
            if let Some(cb) = inc_btn.on_click {
                slot = slot.on_click_raw(cb);
            }
            btn_fn(slot, NumberStep::Increment, !can_inc, cx)
        } else {
            inc_btn.into_element(cx)
        };

        let mut root = div()
            .id(self.id)
            .flex_row()
            .w(eff_w)
            .h(h)
            .p(pad as i8)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .child(cx, dec_el)
            .child(
                cx,
                div()
                    .flex_grow(1)
                    .h(btn_sz)
                    .px(6)
                    .items_center()
                    .justify_center()
                    .child(
                        cx,
                        text(display_str)
                            .size(self.style_ext.text_size.unwrap_or(font_sz))
                            .color(self.style_ext.text_color.unwrap_or(text_color))
                            .text_center()
                            .ellipsis(),
                    ),
            )
            .child(cx, inc_el);

        if self.border_color != Color::TRANSPARENT {
            root = root.border(1, self.border_color);
        }

        if self.disabled {
            root = root.opacity(115).cursor(CursorShape::NotAllowed);
        } else {
            root = root.focusable();
            if !self.focus_ring {
                root = root.no_focus_border();
            }
            if let Some(nav) = nav_cb {
                root = root.on_key_nav(nav);
            }
        }

        self.style_ext
            .apply_to_div(root, self.style)
            .into_element(cx)
    }
}
