//! 主题化数据图表组件 `Chart`（`std` 下默认开启 `gpui-kit` 风格入场展开、数据平滑变形与悬停/刷选交互动画）。

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
    init: |base| {
        base.rounded_bars = true;
        base.show_dots = true;
        base.interactive = true;
        {
            base.animated = true;
        }
    },
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
        labels(labels: &'a [&'a str]),
        unit(unit: &'a str),
        color(color: Color),
        background_and_grid(bg: Color, grid: Color),
        grid_lines(count: u8),
        animated(enabled: bool),
        interactive(enabled: bool),
        duration_ms(ms: u16),
        show_dots(show: bool),
        rounded_bars(rounded: bool),
        donut_ratio(pct: u8),
        active_index(idx: Option<usize>),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
        anim_epoch(epoch: u32),
        progress(progress_u8: u8),
    ],
}
