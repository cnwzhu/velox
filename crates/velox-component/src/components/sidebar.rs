//! 主题化侧边栏导航组件 `Sidebar`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::sidebar::{SidebarItem, SidebarItemState, SidebarSection};

crate::define_themed_component! {
    name: Sidebar<'a>,
    base: velox_base::Sidebar<'a>,
    new: (id: u32, header_title: &'a str, sections: &'a [SidebarSection<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.sidebar);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.sidebar_foreground);
        fill_color(&mut base.section_title_color, theme.muted_foreground);
        fill_color(&mut base.text_color, theme.sidebar_foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.active_bg, theme.list_active);
        fill_color(&mut base.active_fg, theme.foreground);
        fill_color(&mut base.active_icon_color, theme.primary);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.press_bg.is_none() {
            base.press_bg = Some(theme.list_active);
        }
        fill_color(&mut base.badge_bg, theme.secondary);
        fill_color(&mut base.badge_fg, theme.secondary_foreground);
        fill_color(&mut base.collapse_btn_bg, theme.secondary);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        active(active_id: Option<u32>),
        collapsed(collapsed: bool),
        size(width: i16, height: i16),
        on_select(handler: &'a dyn Fn(u32, &mut ActionScope)),
        on_toggle_collapse(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

crate::forward_slot_el!(Sidebar, item, (Div<'a>, SidebarItemState, &'a FrameScope));
