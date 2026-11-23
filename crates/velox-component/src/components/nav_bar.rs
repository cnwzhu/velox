//! 主题化顶部导航栏组件 `NavBar`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

crate::define_themed_component! {
    name: NavBar<'a>,
    base: velox_base::NavBar<'a>,
    new: (id: u32, title: impl Into<StrRef<'a>>),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.action_color, theme.primary);
        if base.action_hover_bg.is_none() {
            base.action_hover_bg = Some(theme.list_hover);
        }
        if base.action_active_bg.is_none() {
            base.action_active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.action_rounded, 6);
    },
    forward: [
        back(label: impl Into<StrRef<'a>>, handler: &'a dyn Fn(&mut ActionScope)),
        action(label: impl Into<StrRef<'a>>, handler: &'a dyn Fn(&mut ActionScope)),
        safe_top(enabled: bool),
        h(height: i16),
    ],
}
