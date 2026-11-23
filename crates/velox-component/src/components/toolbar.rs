//! 主题化顶部工具栏组件 `Toolbar`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::toolbar::ToolbarGroup;

crate::define_themed_component! {
    name: Toolbar<'a>,
    base: velox_base::Toolbar<'a>,
    new: (id: u32, groups: &'a [ToolbarGroup<'a>]),
    styled: false,
    variants: false,
    sizable: true,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.active_bg, theme.list_active);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.active_fg, theme.foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.accent);
        }
        if base.press_bg.is_none() {
            base.press_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        right_groups(groups: &'a [ToolbarGroup<'a>]),
        width(width: i16),
        active(group_idx: usize, item_idx: usize),
        on_action(handler: &'a dyn Fn(usize, usize, &mut ActionScope)),
    ],
}
