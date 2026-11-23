//! 主题化工作区停靠面板组件 `Dock`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::components::resizable::DockWorkspaceState;
use velox_core::prelude::*;

pub use velox_base::components::dock::DockPanel;

crate::define_themed_component! {
    name: Dock<'a>,
    base: velox_base::Dock<'a>,
    new: (id: u32, width: i16, height: i16, state: DockWorkspaceState),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.background);
        fill_color(&mut base.panel_bg, theme.card);
        fill_color(&mut base.tab_bar_bg, theme.background);
        fill_color(&mut base.active_tab_bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        left_panel(panel: DockPanel<'a>),
        right_panel(panel: DockPanel<'a>),
        bottom_panel(panel: DockPanel<'a>),
        center(cx: &'a FrameScope, child: impl IntoElement<'a>),
    ],
}
