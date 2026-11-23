//! 主题化命令面板组件 `Command`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::command::{CommandGroup, CommandItem, CommandItemState};

crate::define_themed_component! {
    name: Command<'a>,
    base: velox_base::Command<'a>,
    new: (id: u32, query: &'a str, items: &'a [CommandItem<'a>], selected_index: usize),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.search_bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.selected_bg, theme.primary);
        fill_color(&mut base.selected_fg, theme.primary_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Command, item, (Div<'a>, CommandItemState, &'a FrameScope));
