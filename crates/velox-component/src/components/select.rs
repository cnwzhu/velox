//! 主题化下拉选择器组件 `Select`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::select::{SelectItemState, SelectOption};

crate::define_themed_component! {
    name: Select<'a>,
    base: velox_base::Select<'a>,
    new: (id: u32, options: &'a [SelectOption<'a>], selected_index: Option<usize>, open: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.active_border_color, theme.ring);
        fill_color(&mut base.popup_bg, theme.popover);
        fill_color(&mut base.popup_border_color, theme.border);
        fill_color(&mut base.selected_bg, theme.list_active);
        fill_color(&mut base.selected_fg, theme.foreground);
        fill_color(&mut base.check_color, theme.primary);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.placeholder_color, theme.muted_foreground);
        fill_color(&mut base.disabled_color, theme.muted_foreground);
        fill_color(&mut base.arrow_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        if base.shadow.is_none() {
            base.shadow = Some((12, 0, 4, Color::rgba(0, 0, 0, 85)));
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        placeholder(placeholder: &'a str),
        width(width: i16),
        on_toggle(handler: &'a dyn Fn(&mut ActionScope)),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Select, item, (Div<'a>, SelectItemState<'a>));

crate::forward_slot_el!(Select, trigger, (Div<'a>, bool));

crate::forward_slot_div!(Select, popup, (Div<'a>));

crate::forward_slot_str!(Select, label, (SelectOption<'a>));
