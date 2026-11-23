//! 主题化可搜索下拉选择框组件 `Combobox`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use crate::components::text_view::{MarkdownView, SelectableText};
pub use velox_base::components::combobox::ComboboxItemState;

crate::define_themed_component! {
    name: Combobox<'a>,
    base: velox_base::Combobox<'a>,
    new: (id: u32, query: &'a str, options: &'a [&'a str], selected_index: Option<usize>, open: bool),
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
        fill_color(&mut base.selected_bg, theme.primary);
        fill_color(&mut base.selected_fg, theme.primary_foreground);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        on_toggle_open(handler: &'a dyn Fn(&mut ActionScope)),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Combobox, item, (Div<'a>, ComboboxItemState<'a>));

crate::forward_slot_el!(Combobox, trigger, (Div<'a>, bool));

crate::forward_slot_div!(Combobox, popup, (Div<'a>));
