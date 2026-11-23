//! 主题化可选择列表组件 `List`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::list::{ListItem, ListItemState};

crate::define_themed_component! {
    name: List<'a>,
    base: velox_base::List<'a>,
    new: (id: u32, items: &'a [ListItem<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.selected_bg, theme.list_active);
        fill_color(&mut base.selected_fg, theme.foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        selected(selected_index: Option<usize>),
        width(width: i16),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(List, item, (Div<'a>, ListItemState<'a>));
