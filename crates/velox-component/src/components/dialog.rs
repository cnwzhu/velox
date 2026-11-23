//! 主题化对话框组件 `Dialog`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

crate::define_themed_component! {
    name: Dialog<'a>,
    base: velox_base::Dialog<'a>,
    new: (id: u32, title: &'a str, open: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.description_color, theme.muted_foreground);
        fill_color(&mut base.close_color, theme.muted_foreground);
        if base.close_hover_bg.is_none() {
            base.close_hover_bg = Some(theme.accent);
        }
        if base.close_active_bg.is_none() {
            base.close_active_bg = Some(theme.list_active);
        }
        if base.shadow.is_none() {
            base.shadow = Some((20, 0, 8, Color::rgba(0, 0, 0, 110)));
        }
        fill_rounded(&mut base.rounded, 10);
    },
    forward: [
        description(description: &'a str),
        width(width: i16),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
        footer(cx: &'a FrameScope, child: impl IntoElement<'a>),
        on_close(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(Dialog, backdrop, (Div<'a>, &'a FrameScope));
