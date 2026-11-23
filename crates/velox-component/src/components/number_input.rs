//! 主题化数值步进输入框组件 `NumberInput`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::number_input::{step_value, NumberStep};

crate::define_themed_component! {
    name: NumberInput<'a>,
    base: velox_base::NumberInput<'a>,
    new: (id: u32, value: i64),
    styled: false,
    variants: false,
    sizable: true,
    disableable: true,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.button_bg, theme.secondary);
        fill_color(&mut base.icon_color, theme.foreground);
        if base.button_hover_bg.is_none() {
            base.button_hover_bg = Some(theme.list_hover);
        }
        if base.button_active_bg.is_none() {
            base.button_active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        range(min: i64, max: i64),
        step(step: i64),
        prefix(prefix: &'a str),
        suffix(suffix: &'a str),
        width(width: i16),
        on_change(handler: &'a dyn Fn(i64, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(NumberInput, step_button, (Div<'a>, NumberStep, bool));

impl<'a> NumberInput<'a> {
    /// 零堆分配双向绑定至 [`Signal<i64>`]：渲染期自动建立细粒度订阅，加减步进时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<i64>) -> Self {
        Self::from_base(velox_base::NumberInput::bind(cx, id, sig))
    }
}
