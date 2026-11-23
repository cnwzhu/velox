//! 主题化文件附件卡片组件 `Attachment`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::attachment::AttachmentKind;

crate::define_themed_component! {
    name: Attachment<'a>,
    base: velox_base::Attachment<'a>,
    new: (id: u32, name: &'a str, meta: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.badge_bg, theme.secondary);
        fill_color(&mut base.badge_border_color, theme.primary);
        fill_color(&mut base.badge_text_color, theme.primary);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.track_bg, theme.muted);
        fill_color(&mut base.accent_color, theme.primary);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        kind(kind: AttachmentKind),
        progress(percent: u8),
        width(width: i16),
        on_remove(handler: &'a dyn Fn(&mut ActionScope)),
        on_click(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
