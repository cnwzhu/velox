//! 主题化菜单与右键上下文菜单组件 `Menu`、`ContextMenu`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::menu::{MenuItem, MenuItemState};

crate::define_themed_component! {
    name: Menu<'a>,
    base: velox_base::Menu<'a>,
    new: (id: u32, items: &'a [MenuItem<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.separator_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.disabled_color, theme.muted_foreground);
        fill_color(&mut base.destructive_color, theme.danger);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        if base.shadow.is_none() {
            base.shadow = Some((12, 0, 4, Color::rgba(0, 0, 0, 85)));
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        width(width: i16),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Menu, item, (Div<'a>, MenuItemState, &'a FrameScope));

crate::define_themed_component! {
    name: ContextMenu<'a>,
    base: velox_base::ContextMenu<'a>,
    new: (id: u32, x: i16, y: i16, open: bool, items: &'a [MenuItem<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        let menu = &mut base.menu;
        fill_color(&mut menu.bg, theme.popover);
        fill_color(&mut menu.border_color, theme.border);
        fill_color(&mut menu.separator_color, theme.border);
        fill_color(&mut menu.text_color, theme.foreground);
        fill_color(&mut menu.muted_color, theme.muted_foreground);
        fill_color(&mut menu.disabled_color, theme.muted_foreground);
        fill_color(&mut menu.destructive_color, theme.danger);
        if menu.hover_bg.is_none() {
            menu.hover_bg = Some(theme.list_hover);
        }
        if menu.active_bg.is_none() {
            menu.active_bg = Some(theme.list_active);
        }
        if menu.shadow.is_none() {
            menu.shadow = Some((12, 0, 4, Color::rgba(0, 0, 0, 85)));
        }
        fill_rounded(&mut menu.rounded, 6);
    },
    forward: [
        width(width: i16),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
        on_dismiss(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(ContextMenu, item, (Div<'a>, MenuItemState, &'a FrameScope));
