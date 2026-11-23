//! 主题化头像与链接组件 `Avatar`、`Link`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::ComponentSize;
use velox_core::prelude::*;

pub use velox_base::components::avatar::AvatarStatus;

crate::define_themed_component! {
    name: Avatar<'a>,
    base: velox_base::Avatar<'a>,
    new: (id: u32, initials: &'a str),
    styled: false,
    variants: false,
    sizable: true,
    disableable: false,
    apply_tw: |base, theme| {
        let dim = match base.size {
            ComponentSize::Small => 28i16,
            ComponentSize::Medium => 36i16,
            ComponentSize::Large => 46i16,
        };
        fill_color(&mut base.bg, theme.primary);
        fill_color(&mut base.fg, theme.primary_foreground);
        fill_color(&mut base.border_color, theme.border);
        fill_rounded(&mut base.rounded, (dim / 2) as u8);
        if base.status_color.is_none() {
            base.status_color = match base.status {
                AvatarStatus::Online => Some(theme.success),
                AvatarStatus::Busy => Some(theme.danger),
                AvatarStatus::Away => Some(theme.warning),
                AvatarStatus::Offline => Some(theme.muted_foreground),
                AvatarStatus::None => None,
            };
        }
    },
    forward: [
        status(status: AvatarStatus),
        bg(bg: Color),
    ],
}

crate::define_themed_component! {
    name: Link<'a>,
    base: velox_base::Link<'a>,
    new: (id: u32, label: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: true,
    init: |base| {
        base.underline = true;
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.color, theme.primary);
        fill_color(&mut base.disabled_color, theme.muted_foreground);
    },
    forward: [
        href(href: &'a str),
        on_click(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
