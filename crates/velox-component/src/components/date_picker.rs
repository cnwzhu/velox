//! 主题化日期选择器组件 `DatePicker`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::date_picker::{DateSelection, SimpleDate};

crate::define_themed_component! {
    name: DatePicker<'a>,
    base: velox_base::DatePicker<'a>,
    new: (id: u32, selected: Option<SimpleDate>, open: bool),
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
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.popup_bg, theme.popover);
        fill_color(&mut base.popup_border_color, theme.border);
        fill_color(&mut base.selected_bg, theme.primary);
        fill_color(&mut base.selected_fg, theme.primary_foreground);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        on_toggle_open(handler: &'a dyn Fn(&mut ActionScope)),
        on_select_day(handler: &'a dyn Fn(SimpleDate, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(DatePicker, trigger, (Div<'a>, bool));
