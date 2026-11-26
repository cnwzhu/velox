//! 主题化通知卡片组件 `Notification`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::notification::NotificationVariant;

crate::define_themed_component! {
    name: Notification<'a>,
    base: velox_base::Notification<'a>,
    new: (id: u32, title: &'a str, message: &'a str),
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
        let def_accent = match base.variant {
            NotificationVariant::Info => theme.info,
            NotificationVariant::Success => theme.success,
            NotificationVariant::Warning => theme.warning,
            NotificationVariant::Error => theme.danger,
        };
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.text_color, theme.muted_foreground);
        fill_color(&mut base.accent_color, def_accent);
        fill_color(&mut base.action_bg, theme.secondary);
        fill_color(&mut base.close_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 8);
        fill_rounded(&mut base.close_rounded, 4);
        fill_rounded(&mut base.action_rounded, base.rounded.saturating_sub(4).max(4));
        if base.close_hover_bg.is_none() {
            base.close_hover_bg = Some(theme.accent);
        }
        if base.close_active_bg.is_none() {
            base.close_active_bg = Some(theme.list_active);
        }
        if base.action_hover_bg.is_none() {
            base.action_hover_bg = Some(theme.list_hover);
        }
        if base.action_active_bg.is_none() {
            base.action_active_bg = Some(theme.list_active);
        }
        if base.shadow.is_none() {
            base.shadow = Some((14, 0, 6, Color::rgba(0, 0, 0, 95)));
        }
    },
    forward: [
        variant(variant: NotificationVariant),
        animated(enabled: bool),
        action(label: &'a str, handler: &'a dyn Fn(&mut ActionScope)),
        on_dismiss(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
