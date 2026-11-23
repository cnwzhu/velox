//! 主题化可关闭标签组件 `Tag`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::tag::TagVariant;

crate::define_themed_component! {
    name: Tag<'a>,
    base: velox_base::components::tag::Tag<'a>,
    new: (id: u32, label: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        let (def_bg, def_border, def_fg) = match base.variant {
            TagVariant::Default => (theme.secondary, theme.border, theme.secondary_foreground),
            TagVariant::Primary => (theme.primary.lerp(theme.card, 190), theme.primary, theme.primary),
            TagVariant::Success => (theme.success.lerp(theme.card, 190), theme.success, theme.success),
            TagVariant::Warning => (theme.warning.lerp(theme.card, 190), theme.warning, theme.warning),
            TagVariant::Danger => (theme.danger.lerp(theme.card, 190), theme.danger, theme.danger),
            TagVariant::Outline => (Color::TRANSPARENT, theme.border, theme.foreground),
        };
        if base.height == 0 {
            base.height = 24;
        }
        if base.px == 0 {
            base.px = 8;
        }
        if base.variant != TagVariant::Outline {
            fill_color(&mut base.bg, def_bg);
        }
        fill_color(&mut base.border_color, def_border);
        fill_color(&mut base.text_color, def_fg);
        fill_rounded(&mut base.rounded, 6);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.accent);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        if base.close_hover_bg.is_none() {
            base.close_hover_bg = Some(theme.accent);
        }
        if base.close_active_bg.is_none() {
            base.close_active_bg = Some(theme.list_active);
        }
    },
    forward: [
        variant(variant: velox_base::TagVariant),
        closable(closable: bool),
        colors(bg: Color, border_color: Color, text_color: Color),
        on_close(handler: &'a dyn Fn(&mut ActionScope)),
        on_click(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
