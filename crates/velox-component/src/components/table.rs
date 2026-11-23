//! 主题化基础数据表格组件 `Table`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::table::{TableCellState, TableColumn, TableHeaderState};

crate::define_themed_component! {
    name: Table<'a>,
    base: velox_base::Table<'a>,
    new: (id: u32, columns: &'a [TableColumn<'a>], rows: &'a [&'a [&'a str]]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.stripe_bg, theme.secondary.lerp(theme.card, 160));
        fill_color(&mut base.header_bg, theme.muted);
        fill_color(&mut base.header_fg, theme.muted_foreground);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.selected_bg, theme.list_active);
        fill_color(&mut base.selected_fg, theme.foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        selected_row(row: Option<usize>),
        on_row_click(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Table, header_cell, (Div<'a>, TableHeaderState, &'a FrameScope));

crate::forward_slot_el!(Table, cell, (Div<'a>, TableCellState, &'a FrameScope));
