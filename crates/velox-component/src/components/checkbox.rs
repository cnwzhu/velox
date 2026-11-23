//! 主题化复选框组件 `Checkbox`。

use crate::styled::fill_color;
use velox_core::prelude::*;

pub use velox_base::components::checkbox::CheckboxState;

crate::define_themed_component! {
    name: Checkbox<'a>,
    base: velox_base::components::checkbox::Checkbox<'a>,
    new: (id: u32, checked: bool),
    styled: false,
    variants: false,
    sizable: true,
    disableable: true,
    init: |base| {
        base.focus_ring = true;
        base.animated = true;
    },
    apply_tw: |base, theme| {
        if !base.has_custom_rounded() {
            base.box_rounded = 4;
        }
        let active_box = base.checked || base.indeterminate;
        if !base.has_custom_colors() {
            if base.disabled {
                fill_color(&mut base.box_bg, if active_box { theme.muted } else { theme.input });
                fill_color(&mut base.border_color, theme.border);
                fill_color(&mut base.check_color, theme.muted_foreground);
                fill_color(&mut base.label_color, theme.muted_foreground);
            } else if active_box {
                fill_color(&mut base.box_bg, theme.primary);
                fill_color(&mut base.border_color, theme.primary);
                fill_color(&mut base.check_color, theme.primary_foreground);
                fill_color(&mut base.label_color, theme.foreground);
            } else {
                fill_color(&mut base.box_bg, theme.input);
                fill_color(&mut base.border_color, theme.border);
                fill_color(&mut base.check_color, theme.primary_foreground);
                fill_color(&mut base.label_color, theme.foreground);
            }
        } else {
            fill_color(&mut base.label_color, theme.foreground);
        }
        if base.hover_bg.is_none() {
            base.hover_bg = Some(if active_box {
                base.box_bg.lerp(Color::WHITE, 24)
            } else {
                theme.accent
            });
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(if active_box {
                base.box_bg.lerp(Color::BLACK, 32)
            } else {
                theme.list_active
            });
        }
    },
    forward: [
        indeterminate(indeterminate: bool),
        focus_ring(enabled: bool),
        label(label: impl Into<StrRef<'a>>),
        colors(box_bg: Color, border_color: Color, check_color: Color, label_color: Color),
        rounded(radius: u8),
        animated(enabled: bool),
        on_change(handler: &'a dyn Fn(bool, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Checkbox, indicator, (Div<'a>, CheckboxState));

crate::forward_slot_el!(Checkbox, label_slot, (StrRef<'a>, CheckboxState));

impl<'a> Checkbox<'a> {
    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，点击时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<bool>) -> Self {
        Self::from_base(velox_base::Checkbox::bind(cx, id, sig))
    }
}
