//! 主题化设置面板组件 `Settings`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::settings::{SettingsGroup, SettingsItem, SettingsValue};

crate::define_themed_component! {
    name: Settings<'a>,
    base: velox_base::Settings<'a>,
    new: (id: u32, groups: &'a [SettingsGroup<'a>]),
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
        fill_color(&mut base.control_bg, theme.secondary);
        fill_color(&mut base.active_bg, theme.primary);
        fill_color(&mut base.active_fg, theme.primary_foreground);
        fill_color(&mut base.switch_off_bg, theme.input);
        fill_color(&mut base.switch_thumb_bg, Color::WHITE);
        fill_rounded(&mut base.switch_rounded, 9);
        if base.switch_thumb_shadow.is_none() {
            base.switch_thumb_shadow = Some((2, 0, 1, Color::rgba(0, 0, 0, 55)));
        }
        if base.row_hover_bg.is_none() {
            base.row_hover_bg = Some(theme.list_hover);
        }
        if base.row_active_bg.is_none() {
            base.row_active_bg = Some(theme.list_active);
        }
        if base.action_hover_bg.is_none() {
            base.action_hover_bg = Some(base.active_bg.lerp(Color::WHITE, 24));
        }
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        width(width: i16),
        on_item_click(handler: &'a dyn Fn(usize, usize, &mut ActionScope)),
    ],
}
