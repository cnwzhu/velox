//! 主题化悬停提示气泡组件 `Tooltip`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

crate::define_themed_component! {
    name: Tooltip<'a>,
    base: velox_base::Tooltip<'a>,
    new: (id: u32, content: &'a str, visible: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.foreground);
        fill_color(&mut base.text_color, theme.background);
        fill_rounded(&mut base.rounded, 6);
        if base.shadow.is_none() {
            base.shadow = Some((8, 0, 2, Color::rgba(0, 0, 0, 65)));
        }
    },
    forward: [
        trigger(cx: &'a FrameScope, child: impl IntoElement<'a>),
    ],
}

crate::forward_slot_el!(Tooltip, popup, (Div<'a>, &'a FrameScope));
