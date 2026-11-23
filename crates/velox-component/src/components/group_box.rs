//! 主题化分组框容器组件 `GroupBox`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: GroupBox<'a>,
    base: velox_base::GroupBox<'a>,
    new: (id: u32, title: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.header_bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.description_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        description(description: &'a str),
        width(width: i16),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
    ],
}
