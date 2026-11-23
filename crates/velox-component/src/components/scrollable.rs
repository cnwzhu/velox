//! 主题化滚动条与滚动容器组件 `Scrollbar`、`Scrollable`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::scrollable::ScrollbarAxis;

crate::define_themed_component! {
    name: Scrollbar,
    base: velox_base::Scrollbar,
    new: (id: u32, axis: ScrollbarAxis, track_length: i16, content_size: i32, viewport_size: i32, scroll_offset: i32),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.rounded = u8::MAX;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.track_color, theme.secondary);
        fill_color(&mut base.thumb_color, theme.muted_foreground);
    },
}

crate::define_themed_component! {
    name: Scrollable<'a>,
    base: velox_base::Scrollable<'a>,
    new: (id: u32, width: i16, height: i16, content_size: i32, scroll_offset: i32),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.track_color, theme.secondary);
        fill_color(&mut base.thumb_color, theme.muted_foreground);
        fill_rounded(&mut base.scrollbar_rounded, 3);
    },
    forward: [
        axis(axis: ScrollbarAxis),
        show_scrollbar(show: bool),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
    ],
}
