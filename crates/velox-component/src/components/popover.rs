//! 主题化气泡浮层组件 `Popover`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

crate::define_themed_component! {
    name: Popover<'a>,
    base: velox_base::Popover<'a>,
    new: (id: u32, open: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_rounded(&mut base.rounded, 8);
        if base.shadow.is_none() {
            base.shadow = Some((12, 0, 4, Color::rgba(0, 0, 0, 85)));
        }
    },
    forward: [
        trigger(cx: &'a FrameScope, el: impl IntoElement<'a>),
        content(cx: &'a FrameScope, el: impl IntoElement<'a>),
        on_close(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(Popover, popup, (Div<'a>, &'a FrameScope));
