//! 主题化底部弹层组件 `BottomSheet`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: BottomSheet<'a>,
    base: velox_base::BottomSheet<'a>,
    new: (id: u32, title: impl Into<StrRef<'a>>, sheet_height: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.animated = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.backdrop_bg, Color::rgba(0, 0, 0, 160));
        fill_color(&mut base.sheet_bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.handle_color, theme.muted_foreground);
        fill_rounded(&mut base.handle_rounded, 2);
        fill_color(&mut base.title_color, theme.foreground);
        fill_rounded(&mut base.rounded, 16);
    },
    forward: [
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
        animated(enabled: bool),
        on_dismiss(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(BottomSheet, backdrop, (Div<'a>, &'a FrameScope));
