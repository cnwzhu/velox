//! 主题化可排序数据表格组件 `DataTable`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::components::table::{TableCellState, TableColumn, TableHeaderState};
use velox_core::prelude::*;

pub use velox_base::components::data_table::SortDirection;

crate::define_themed_component! {
    name: DataTable<'a>,
    base: velox_base::DataTable<'a>,
    new: (id: u32, columns: &'a [TableColumn<'a>], rows: &'a [&'a [&'a str]]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.stripe_bg, theme.secondary.lerp(theme.card, 160));
        fill_color(&mut base.header_bg, theme.muted);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.accent_color, theme.primary);
        fill_color(&mut base.selected_bg, theme.list_active);
        fill_color(&mut base.selected_fg, theme.foreground);
        if base.header_hover_bg.is_none() {
            base.header_hover_bg = Some(theme.list_hover);
        }
        if base.header_active_bg.is_none() {
            base.header_active_bg = Some(theme.list_active);
        }
        if base.row_hover_bg.is_none() {
            base.row_hover_bg = Some(theme.list_hover);
        }
        if base.row_active_bg.is_none() {
            base.row_active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        selected_row(row: Option<usize>),
        sort_by(col_idx: usize, dir: SortDirection),
        on_row_click(handler: &'a dyn Fn(usize, &mut ActionScope)),
        on_sort_column(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(DataTable, header_cell, (Div<'a>, TableHeaderState, &'a FrameScope));

crate::forward_slot_el!(DataTable, cell, (Div<'a>, TableCellState, &'a FrameScope));
