//! 主题化树形层级视图组件 `Tree`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::tree::{TreeItemState, TreeNode, TreeState};

crate::define_themed_component! {
    name: Tree<'a>,
    base: velox_base::Tree<'a>,
    new: (id: u32, nodes: &'a [TreeNode<'a>]),
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
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.selected_bg, theme.list_active);
        fill_color(&mut base.selected_fg, theme.foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        selected(selected_id: Option<u32>),
        on_select(handler: &'a dyn Fn(u32, &mut ActionScope)),
        on_toggle_expand(handler: &'a dyn Fn(u32, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Tree, item, (Div<'a>, TreeItemState, &'a FrameScope));
