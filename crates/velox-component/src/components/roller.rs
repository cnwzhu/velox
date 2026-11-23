//! 主题化滚轮选择器组件 `Roller`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: Roller<'a>,
    base: velox_base::Roller<'a>,
    new: (id: u32, options: &'a [&'a str], selected: usize),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.animated = true;
    },
    apply_tw: |base, theme| {
        let p = theme.primary;
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.center_bg, Color::rgba(p.r(), p.g(), p.b(), 48));
        fill_color(&mut base.center_border_color, theme.primary);
        fill_color(&mut base.active_fg, theme.foreground);
        fill_color(&mut base.inactive_fg, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        w(width: i16),
        row_h(row_height: i16),
        visible_rows(rows: u8),
        on_change(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

impl<'a> Roller<'a> {
    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，滚动选定新项时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, options: &'a [&'a str], sig: Signal<usize>) -> Self {
        Self::from_base(velox_base::Roller::bind(cx, id, options, sig))
    }
}
