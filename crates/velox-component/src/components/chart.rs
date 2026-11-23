//! 主题化数据图表组件 `Chart`。

use velox_engine::prelude::*;

pub use velox_base::components::chart::{CandlestickBar, ChartKind};

crate::define_themed_component! {
    name: Chart<'a>,
    base: velox_base::Chart<'a>,
    new: (id: u32, width: i16, height: i16, data: &'a [i16], min_val: i16, max_val: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        if base.series_color.is_none() {
            base.series_color = Some(theme.primary);
        }
        if base.grid_color.is_none() {
            base.grid_color = Some(theme.border);
        }
        if base.bg_color.is_none() {
            base.bg_color = Some(theme.card);
        }
    },
    forward: [
        kind(kind: ChartKind),
        candles(candles: &'a [CandlestickBar]),
        color(color: Color),
        background_and_grid(bg: Color, grid: Color),
        grid_lines(count: u8),
    ],
}
