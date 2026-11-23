//! 主题化提示横幅组件 `Alert`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::alert::AlertVariant;

crate::define_themed_component! {
    name: Alert<'a>,
    base: velox_base::components::alert::Alert<'a>,
    new: (id: u32, title: &'a str, message: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        let accent = match base.variant {
            AlertVariant::Info => theme.info,
            AlertVariant::Success => theme.success,
            AlertVariant::Warning => theme.warning,
            AlertVariant::Error => theme.danger,
        };
        let def_bg = accent.lerp(theme.card, 224);
        fill_color(&mut base.bg, def_bg);
        fill_color(&mut base.border_color, accent.lerp(theme.border, 128));
        fill_color(&mut base.title_color, accent);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.close_color, theme.muted_foreground);
        if base.close_hover_bg.is_none() {
            base.close_hover_bg = Some(theme.accent);
        }
        if base.close_active_bg.is_none() {
            base.close_active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        variant(variant: velox_base::AlertVariant),
        width(width: i16),
        on_close(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
