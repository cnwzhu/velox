//! 主题化软键盘组件 `VirtualKeyboard`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::virtual_keyboard::{KeyAction, KeyboardMode};

crate::define_themed_component! {
    name: VirtualKeyboard<'a>,
    base: velox_base::VirtualKeyboard<'a>,
    new: (id: u32, mode: KeyboardMode),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.board_bg, theme.card);
        fill_color(&mut base.board_border_color, theme.border);
        fill_color(&mut base.key_bg, theme.secondary);
        fill_color(&mut base.key_fg, theme.foreground);
        fill_color(&mut base.enter_bg, theme.primary);
        fill_color(&mut base.enter_fg, theme.primary_foreground);
        fill_color(&mut base.active_bg, theme.list_active);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        size(width: i16, height: i16),
        on_key(handler: &'a dyn Fn(KeyAction, &mut ActionScope)),
    ],
}
