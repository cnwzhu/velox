//! 主题化底部状态栏组件 `StatusBar`。

use crate::styled::fill_color;

pub use velox_base::components::status_bar::StatusBarItem;

crate::define_themed_component! {
    name: StatusBar<'a>,
    base: velox_base::StatusBar<'a>,
    new: (id: u32, width: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.muted_foreground);
    },
    forward: [
        left(items: &'a [StatusBarItem<'a>]),
        center(items: &'a [StatusBarItem<'a>]),
        right(items: &'a [StatusBarItem<'a>]),
    ],
}
