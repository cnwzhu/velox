//! 主题化悬停卡片与确认警告对话框组件 `HoverCard`、`AlertDialog`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: HoverCard<'a>,
    base: velox_base::HoverCard<'a>,
    new: (id: u32, title: &'a str, description: &'a str, open: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.description_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 8);
        if base.shadow.is_none() {
            base.shadow = Some((12, 0, 4, Color::rgba(0, 0, 0, 85)));
        }
    },
    forward: [
        trigger(cx: &'a FrameScope, el: impl IntoElement<'a>),
    ],
}

crate::forward_slot_el!(HoverCard, popup, (Div<'a>, &'a FrameScope));

crate::define_themed_component! {
    name: AlertDialog<'a>,
    base: velox_base::AlertDialog<'a>,
    new: (id: u32, title: &'a str, description: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        let (def_confirm_bg, def_confirm_fg) = if base.destructive {
            (theme.danger, theme.danger_foreground)
        } else {
            (theme.primary, theme.primary_foreground)
        };
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.description_color, theme.muted_foreground);
        fill_color(&mut base.cancel_bg, theme.secondary);
        fill_color(&mut base.cancel_border_color, theme.border);
        fill_color(&mut base.cancel_fg, theme.secondary_foreground);
        fill_color(&mut base.confirm_bg, def_confirm_bg);
        fill_color(&mut base.confirm_fg, def_confirm_fg);
        fill_rounded(&mut base.rounded, 10);
        fill_rounded(&mut base.btn_rounded, base.rounded.saturating_sub(4).max(4));
        if base.cancel_hover_bg.is_none() {
            base.cancel_hover_bg = Some(theme.list_active);
        }
        if base.cancel_active_bg.is_none() {
            base.cancel_active_bg = Some(theme.secondary.lerp(Color::BLACK, 24));
        }
        if base.confirm_hover_bg.is_none() {
            base.confirm_hover_bg = Some(base.confirm_bg.lerp(Color::WHITE, 26));
        }
        if base.confirm_active_bg.is_none() {
            base.confirm_active_bg = Some(base.confirm_bg.lerp(Color::BLACK, 38));
        }
        if base.confirm_focus_border.is_none() {
            base.confirm_focus_border = Some((1, base.confirm_bg.lerp(Color::WHITE, 128)));
        }
        if base.shadow.is_none() {
            base.shadow = Some((20, 0, 8, Color::rgba(0, 0, 0, 110)));
        }
    },
    forward: [
        destructive(destructive: bool),
        on_confirm(handler: &'a dyn Fn(&mut ActionScope)),
        on_cancel(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(AlertDialog, backdrop, (Div<'a>, &'a FrameScope));
