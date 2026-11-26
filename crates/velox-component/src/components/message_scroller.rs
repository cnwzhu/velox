//! 主题化消息滚动容器组件 `MessageScroller`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::message_scroller::MessageScrollerState;

crate::define_themed_component! {
    name: MessageScroller<'a>,
    base: velox_base::MessageScroller<'a>,
    new: (id: u32, messages: &'a [velox_base::Message<'a>], state: MessageScrollerState, width: i16, height: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
        {
            base.animated = true;
        }
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.jump_bg, theme.primary);
        fill_color(&mut base.jump_text_color, theme.primary_foreground);
        fill_rounded(&mut base.rounded, 8);
        fill_rounded(&mut base.jump_rounded, 12);
    },
    forward: [
        rounded(radius: u8),
        animated(enabled: bool),
        on_jump_bottom(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
