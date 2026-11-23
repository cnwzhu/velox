//! 主题化时间线/状态分割标记组件 `Marker`。

use crate::styled::{fill_color, fill_rounded};

pub use velox_base::components::marker::MarkerVariant;

crate::define_themed_component! {
    name: Marker<'a>,
    base: velox_base::Marker<'a>,
    new: (id: u32, label: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        let (def_line, def_pill_bg, def_text) = match base.variant {
            MarkerVariant::Default => (theme.border, theme.card, theme.muted_foreground),
            MarkerVariant::Unread => (theme.danger, theme.card, theme.danger),
            MarkerVariant::System => (theme.primary, theme.secondary, theme.primary),
            MarkerVariant::Error => (theme.danger, theme.card, theme.danger),
        };
        fill_color(&mut base.line_color, def_line);
        fill_color(&mut base.pill_bg, def_pill_bg);
        fill_color(&mut base.text_color, def_text);
        fill_rounded(&mut base.rounded, 10);
    },
    forward: [
        variant(variant: MarkerVariant),
        width(width: i16),
    ],
}
