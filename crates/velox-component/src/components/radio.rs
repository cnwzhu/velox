//! 主题化单选按钮与单选组组件 `Radio`、`RadioGroup`。

use crate::styled::fill_color;
use velox_core::prelude::*;

pub use velox_base::components::radio::{RadioItemState, RadioState};

crate::define_themed_component! {
    name: Radio<'a>,
    base: velox_base::Radio<'a>,
    new: (id: u32, label: &'a str, checked: bool),
    styled: false,
    variants: false,
    sizable: true,
    disableable: true,
    init: |base| {
        base.fill_on_checked = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.accent_color, theme.primary);
        fill_color(&mut base.unchecked_bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.dot_color, theme.primary_foreground);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.disabled_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(if base.checked {
                base.accent_color.lerp(Color::WHITE, 24)
            } else {
                theme.accent
            });
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(if base.checked {
                base.accent_color.lerp(Color::BLACK, 32)
            } else {
                theme.list_active
            });
        }
    },
    forward: [
        on_select(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(Radio, indicator, (Div<'a>, RadioState));

crate::define_themed_component! {
    name: RadioGroup<'a>,
    base: velox_base::RadioGroup<'a>,
    new: (id: u32, options: &'a [&'a str], selected_index: Option<usize>),
    styled: false,
    variants: false,
    sizable: true,
    disableable: true,
    init: |base| {
        base.fill_on_checked = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.accent_color, theme.primary);
        fill_color(&mut base.unchecked_bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.dot_color, theme.primary_foreground);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.disabled_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.accent);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        if base.checked_hover_bg.is_none() {
            base.checked_hover_bg = Some(base.accent_color.lerp(Color::WHITE, 24));
        }
        if base.checked_active_bg.is_none() {
            base.checked_active_bg = Some(base.accent_color.lerp(Color::BLACK, 32));
        }
    },
    forward: [
        horizontal(),
        vertical(),
        on_change(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(RadioGroup, item, (Div<'a>, RadioItemState<'a>));

crate::forward_slot_str!(RadioGroup, label, (usize, &'a str));

impl<'a> RadioGroup<'a> {
    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，切换选项时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, options: &'a [&'a str], sig: Signal<usize>) -> Self {
        Self::from_base(velox_base::RadioGroup::bind(cx, id, options, sig))
    }
}
