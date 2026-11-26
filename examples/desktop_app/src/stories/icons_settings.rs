//! `Settings & Icons` 故事页：完全基于 `velox-component` 的 `Settings` 分组面板与全部 19 个内置 Lucide 矢量 `IconName` 展板。

use super::{story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{
    Icon, IconName, Settings, SettingsGroup, SettingsItem, SettingsValue,
};
use velox_kit::prelude::*;

const ALL_ICONS: &[(&str, IconName<'static>)] = &[
    ("Info", IconName::Info),
    ("Check", IconName::Check),
    ("Close", IconName::Close),
    ("Plus", IconName::Plus),
    ("Minus", IconName::Minus),
    ("Search", IconName::Search),
    ("Settings", IconName::Settings),
    ("Star", IconName::Star),
    ("StarOut", IconName::StarOutline),
    ("Warning", IconName::Warning),
    ("Error", IconName::Error),
    ("Up", IconName::ChevronUp),
    ("Down", IconName::ChevronDown),
    ("Left", IconName::ChevronLeft),
    ("Right", IconName::ChevronRight),
    ("File", IconName::File),
    ("Folder", IconName::Folder),
    ("Mic", IconName::Mic),
    ("Trash", IconName::Trash),
    ("Eye", IconName::Eye),
    ("EyeOff", IconName::EyeOff),
];

impl GalleryApp {
    pub(crate) fn render_story_settings_icons<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let on_settings_inner = cx.listener(|this, (grp, item): (usize, usize), _acx| {
            match (grp, item) {
                (0, 0) => {
                    let next = !this.signals.vsync_enabled.get_untracked();
                    this.vsync_enabled = next;
                    this.signals.vsync_enabled.set_if_changed(next);
                }
                (0, 1) => this.hidpi_enabled = !this.hidpi_enabled,
                (0, 2) => {
                    let next = !this.signals.damage_overlay_hint.get_untracked();
                    this.damage_overlay_hint = next;
                    this.signals.damage_overlay_hint.set_if_changed(next);
                }
                (1, 0) => {
                    this.theme_mode = this.theme_mode.toggle();
                    this.signals.theme_mode.set_if_changed(this.theme_mode);
                }
                _ => {}
            }
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            this.signals
                .event_count
                .update(|n| *n = n.saturating_add(1));
        });
        let on_settings_click =
            cx.arena().alloc(move |grp: usize, item: usize, acx: &mut ActionScope| {
                on_settings_inner((grp, item), acx);
            });

        let grp0_items = cx.arena().alloc_iter([
            SettingsItem::new(
                "VSync Frame Pacing",
                "Synchronize presentation with display refresh rate",
                SettingsValue::Bool(self.signals.vsync_enabled.get_untracked()),
            ),
            SettingsItem::new(
                "HiDPI Vector Supersampling",
                "Supersample SVG icons & SDF borders at 1.5x / 2x",
                SettingsValue::Bool(self.hidpi_enabled),
            ),
            SettingsItem::new(
                "Partial Damage Tracking",
                "Skip full-frame redraws and sleep at 0% CPU",
                SettingsValue::Bool(self.signals.damage_overlay_hint.get_untracked()),
            ),
        ]);
        let grp1_items = cx.arena().alloc_iter([
            SettingsItem::new(
                "Active Theme Mode",
                "Switch between Dark and Light monochrome contrast",
                SettingsValue::Action(if self.theme_mode.is_dark() {
                    "Switch to Light"
                } else {
                    "Switch to Dark"
                }),
            ),
            SettingsItem::new(
                "Renderer Backend",
                "Active hardware / software presentation surface",
                SettingsValue::Text("wgpu 2D SDF"),
            ),
        ]);
        let groups = cx.arena().alloc_iter([
            SettingsGroup::new("Display & Renderer", grp0_items),
            SettingsGroup::new("Appearance & Engine", grp1_items),
        ]);

        let mut icon_grid = div().flex_row().flex_wrap().gap(8).w_full();
        for (idx, &(name, icon_name)) in ALL_ICONS.iter().enumerate() {
            let col = if matches!(icon_name, IconName::Star) {
                theme.warning
            } else if matches!(icon_name, IconName::Error | IconName::Trash) {
                theme.danger
            } else if matches!(icon_name, IconName::Check) {
                theme.success
            } else {
                theme.foreground
            };
            icon_grid = icon_grid.child(
                cx,
                div()
                    .flex_col()
                    .w(68)
                    .h(48)
                    .p(4)
                    .gap(4)
                    .rounded(6)
                    .bg(theme.background)
                    .hover_bg(theme.list_hover)
                    .border(1, theme.border)
                    .items_center()
                    .justify_center()
                    .child(cx, Icon::new(1900 + idx as u32, icon_name).px(15).color(col))
                    .child(cx, text(name).size(10).color(theme.muted_foreground)),
            );
        }

        let icons_section = story_section(
            cx,
            theme,
            "Built-in Lucide Vector SVG Icon Catalog (19 Icons)",
            "Native 24x24 SVG paths with anti-aliased stroke joins & GPU SpriteAtlas cache",
        )
        .child(cx, icon_grid);

        div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Settings Panel & Vector Icons",
                    "Themed velox-component grouped preference cards and Lucide SVG vector icon catalog.",
                    "velox_component::{Settings, Icon, IconName}",
                ),
            )
            .child(
                cx,
                Settings::new(1950, groups)
                    .width(740)
                    .on_item_click(on_settings_click),
            )
            .child(cx, icons_section)
    }
}
