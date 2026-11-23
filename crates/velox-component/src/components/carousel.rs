//! 主题化轮播组件 `Carousel`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: Carousel<'a>,
    base: velox_base::Carousel<'a>,
    new: (id: u32, slides: &'a [&'a str], active_index: usize),
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
        fill_color(&mut base.btn_bg, theme.secondary);
        fill_color(&mut base.dot_bg, theme.muted);
        fill_color(&mut base.active_dot_bg, theme.primary);
        fill_rounded(&mut base.rounded, 8);
        fill_rounded(&mut base.btn_rounded, 14);
        fill_rounded(&mut base.dot_rounded, 3);
    },
    forward: [
        size(width: i16, height: i16),
        on_change(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}
