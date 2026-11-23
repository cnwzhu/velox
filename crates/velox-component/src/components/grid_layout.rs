//! 主题化网格磁贴布局组件 `GridLayout`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::grid_layout::GridTile;

crate::define_themed_component! {
    name: GridLayout<'a>,
    base: velox_base::GridLayout<'a>,
    new: (id: u32, columns: u8, cell_width: i16, cell_height: i16, tiles: &'a [GridTile<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.tile_bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.active_border_color, theme.primary);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        active_tile(tile_id: Option<u32>),
        on_tile_click(handler: &'a dyn Fn(u32, &mut ActionScope)),
    ],
}
