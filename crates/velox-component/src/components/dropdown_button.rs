//! 主题化带下拉菜单的分割按钮组件 `DropdownButton`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

crate::define_themed_component! {
    name: DropdownButton<'a>,
    base: velox_base::DropdownButton<'a>,
    new: (id: u32, label: &'a str, options: &'a [&'a str], open: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.primary);
        fill_color(&mut base.arrow_bg, theme.primary.lerp(Color::BLACK, 24));
        fill_color(&mut base.divider_color, theme.primary.lerp(Color::WHITE, 56));
        fill_color(&mut base.text_color, theme.primary_foreground);
        fill_color(&mut base.menu_bg, theme.popover);
        fill_color(&mut base.menu_border_color, theme.border);
        fill_color(&mut base.menu_text_color, theme.foreground);
        fill_rounded(&mut base.rounded, 6);
        if base.shadow.is_none() {
            base.shadow = Some((12, 0, 4, Color::rgba(0, 0, 0, 85)));
        }
    },
    forward: [
        on_click(handler: &'a dyn Fn(&mut ActionScope)),
        on_toggle_menu(handler: &'a dyn Fn(&mut ActionScope)),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(DropdownButton, item, (Div<'a>, usize, &'a str));
