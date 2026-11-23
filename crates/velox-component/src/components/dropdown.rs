//! 主题化下拉选择组件 `Dropdown`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::dropdown::DropdownItemState;

crate::define_themed_component! {
    name: Dropdown<'a>,
    base: velox_base::Dropdown<'a>,
    new: (id: u32, options: &'a [&'a str], selected: usize, expanded: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.animated = true;
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.header_bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.active_border_color, theme.ring);
        fill_color(&mut base.popup_bg, theme.popover);
        fill_color(&mut base.popup_border_color, theme.border);
        fill_color(&mut base.selected_bg, theme.list_active);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.selected_text_color, theme.foreground);
        fill_color(&mut base.check_color, theme.primary);
        fill_color(&mut base.arrow_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
        if base.shadow_alpha == 0 {
            base.shadow_alpha = 85;
        }
    },
    forward: [
        size(width: i16, height: i16),
        animated(enabled: bool),
        on_toggle(handler: &'a dyn Fn(&mut ActionScope)),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Dropdown, item, (Div<'a>, DropdownItemState<'a>));

crate::forward_slot_el!(Dropdown, trigger, (Div<'a>, bool));

crate::forward_slot_div!(Dropdown, popup, (Div<'a>));
