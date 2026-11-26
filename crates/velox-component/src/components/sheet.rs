//! 主题化侧边抽屉弹层组件 `Sheet`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::sheet::SheetSide;

crate::define_themed_component! {
    name: Sheet<'a>,
    base: velox_base::Sheet<'a>,
    new: (id: u32, title: &'a str, side: SheetSide, open: bool),
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
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.close_color, theme.muted_foreground);
        fill_rounded(&mut base.close_rounded, 4);
        if base.close_hover_bg.is_none() {
            base.close_hover_bg = Some(theme.accent);
        }
        if base.close_active_bg.is_none() {
            base.close_active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 8);
        if base.shadow.is_none() {
            base.shadow = Some((20, 0, 4, Color::rgba(0, 0, 0, 110)));
        }
    },
    forward: [
        size(size: i16),
        animated(enabled: bool),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
        on_dismiss(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(Sheet, backdrop, (Div<'a>, &'a FrameScope));
