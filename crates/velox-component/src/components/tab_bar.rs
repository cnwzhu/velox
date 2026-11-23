//! 主题化底部标签栏组件 `TabBar`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::tab_bar::TabBarItemState;

crate::define_themed_component! {
    name: TabBar<'a>,
    base: velox_base::TabBar<'a>,
    new: (id: u32, labels: &'a [&'a str], active: usize),
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
        fill_color(&mut base.active_pill_bg, Color::rgba(p.r(), p.g(), p.b(), 44));
        fill_color(&mut base.active_fg, theme.primary);
        fill_color(&mut base.inactive_fg, theme.muted_foreground);
        fill_rounded(&mut base.item_rounded, 10);
    },
    forward: [
        safe_bottom(enabled: bool),
        h(height: i16),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(TabBar, item, (Div<'a>, TabBarItemState, &'a FrameScope));

impl<'a> TabBar<'a> {
    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，切换底部标签时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, labels: &'a [&'a str], sig: Signal<usize>) -> Self {
        Self::from_base(velox_base::TabBar::bind(cx, id, labels, sig))
    }
}
