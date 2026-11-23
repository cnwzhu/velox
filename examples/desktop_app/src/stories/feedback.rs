//! `Alerts & Badges` 故事页：`Alert`（4 语义矢量图标）、`Badge`、`Tag`、`Tooltip`、`Popover`、`Kbd`、`Avatar`。

use super::{story_header, story_section};
use crate::style::{self, BasePalette};
use crate::GalleryApp;
use velox_kit::base::{
    Alert, AlertVariant, Avatar, AvatarStatus, Badge, Button, Kbd, Popover, Tag, TagVariant,
    Tooltip,
};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_feedback<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let p = BasePalette::from_theme(theme);
        let on_toggle_tip = cx.listener(|this, _: ClickEvent, _acx| {
            this.tooltip_open = !this.tooltip_open;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_pop = cx.listener(|this, _: ClickEvent, _acx| {
            this.popover_open = !this.popover_open;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_close_pop = cx.action(|this, _acx| {
            this.popover_open = false;
        });
        let on_close_tag0 = cx.action(|this, _acx| {
            this.tag_visible_mask &= !(1 << 0);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_close_tag1 = cx.action(|this, _acx| {
            this.tag_visible_mask &= !(1 << 1);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_reset_tags = cx.listener(|this, _: ClickEvent, _acx| {
            this.tag_visible_mask = 0b1111;
            this.event_count = this.event_count.saturating_add(1);
        });

        let alerts_section = story_section(
            cx,
            theme,
            "Alerts (Info • Success • Warning • Error)",
            "Native 24x24 Lucide vector icons with app-styled sharp 1px semantic borders",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(10)
                .child(
                    cx,
                    style::alert(
                        Alert::new(1701, "System Ready", "wgpu 2D SDF pipeline active at 1.5x HiDPI.")
                            .variant(AlertVariant::Info)
                            .width(360),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::alert(
                        Alert::new(1702, "Build Succeeded", "All workspace tests passed with 0 warnings.")
                            .variant(AlertVariant::Success)
                            .width(360),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::alert(
                        Alert::new(1703, "High Frame Budget", "Animated transition running; will settle to 0% CPU.")
                            .variant(AlertVariant::Warning)
                            .width(360),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::alert(
                        Alert::new(1704, "Validation Error", "Target port is currently occupied by another process.")
                            .variant(AlertVariant::Error)
                            .width(360),
                        theme,
                    ),
                ),
        );

        let mut tags_row = div().flex_row().flex_wrap().gap(8).items_center();
        if (self.tag_visible_mask & (1 << 0)) != 0 {
            tags_row = tags_row.child(
                cx,
                style::tag(
                    Tag::new(1720, "rust-gui")
                        .variant(TagVariant::Primary)
                        .on_close(on_close_tag0),
                    theme,
                ),
            );
        }
        if (self.tag_visible_mask & (1 << 1)) != 0 {
            tags_row = tags_row.child(
                cx,
                style::tag(
                    Tag::new(1721, "velox-base")
                        .variant(TagVariant::Success)
                        .on_close(on_close_tag1),
                    theme,
                ),
            );
        }
        tags_row = tags_row
            .child(cx, style::tag(Tag::new(1722, "beta").variant(TagVariant::Warning), theme))
            .child(cx, style::tag(Tag::new(1723, "deprecated").variant(TagVariant::Danger), theme))
            .child(cx, style::tag(Tag::new(1724, "outline").variant(TagVariant::Outline), theme));
        if self.tag_visible_mask != 0b1111 {
            tags_row = tags_row.child(
                cx,
                style::btn_ghost(
                    Button::new(1725, "Restore Tags")
                        .font_size(12)
                        .h(26)
                        .on_click(on_reset_tags),
                    theme,
                ),
            );
        }

        let badges_section = story_section(
            cx,
            theme,
            "Badges, Closable Tags, Kbd & Avatars",
            "Headless primitives styled explicitly with monochrome & semantic accents",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .items_center()
                .child(cx, style::badge_solid(Badge::new("Solid"), theme))
                .child(cx, style::badge_outline(Badge::new("Outline"), theme))
                .child(cx, style::badge_subtle(Badge::new("Subtle"), theme))
                .child(cx, style::badge_tint(Badge::new("Success"), theme.success, theme))
                .child(cx, style::badge_tint(Badge::new("Warning"), theme.warning, theme))
                .child(cx, style::badge_tint(Badge::new("Danger"), theme.danger, theme))
                .child(cx, style::badge_tint(Badge::new("Info"), theme.info, theme))
                .child(cx, Badge::new("Bare Badge")),
        )
        .child(cx, tags_row)
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(18)
                .items_center()
                .child(cx, style::kbd(Kbd::new(1730, "Ctrl+K"), theme))
                .child(cx, style::kbd(Kbd::new(1731, "Shift+Tab"), theme))
                .child(cx, style::kbd(Kbd::new(1732, "Esc"), theme))
                .child(cx, div().w(1).h(22).bg(p.border))
                .child(cx, style::avatar(Avatar::new(1733, "VX").status(AvatarStatus::Online), theme))
                .child(cx, style::avatar(Avatar::new(1734, "GP").status(AvatarStatus::Busy), theme))
                .child(cx, style::avatar(Avatar::new(1735, "UI").status(AvatarStatus::Away), theme))
                .child(cx, style::avatar(Avatar::new(1736, "OS").status(AvatarStatus::Offline), theme)),
        );

        let popups_section = story_section(
            cx,
            theme,
            "Tooltip (Inverted Contrast) & Popover (Escape Dismiss)",
            "Click triggers to preview floating Tooltip and Popover cards",
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(20)
                .items_start()
                .child(
                    cx,
                    style::tooltip(
                        Tooltip::new(1740, "High-contrast floating tooltip", self.tooltip_open)
                            .trigger(
                                cx,
                                style::btn_outline(
                                    Button::new(
                                        1741,
                                        if self.tooltip_open {
                                            "Hide Tooltip"
                                        } else {
                                            "Show Tooltip"
                                        },
                                    )
                                    .font_size(12)
                                    .h(28)
                                    .on_click(on_toggle_tip),
                                    theme,
                                ),
                            ),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::popover(
                        Popover::new(1742, self.popover_open)
                            .on_close(on_close_pop)
                            .trigger(
                                cx,
                                style::btn_solid(
                                    Button::new(
                                        1743,
                                        if self.popover_open {
                                            "Hide Popover"
                                        } else {
                                            "Show Popover"
                                        },
                                    )
                                    .font_size(12)
                                    .h(28)
                                    .on_click(on_toggle_pop),
                                    theme,
                                ),
                            )
                            .content(
                                cx,
                                div()
                                    .flex_col()
                                    .gap(4)
                                    .child(cx, text("Popover Content").size(13).color(p.fg))
                                    .child(
                                        cx,
                                        text("Press Escape to dismiss this popover card.")
                                            .size(11)
                                            .color(p.muted_fg),
                                    ),
                            ),
                        theme,
                    ),
                ),
        );

        div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Alerts, Badges, Tags & Tooltips",
                    "Headless feedback banners, closable vector tags, keyboard shortcuts, avatars & popovers.",
                    "velox_base::{Alert, Badge, Tag, Tooltip, Popover, Kbd, Avatar}",
                ),
            )
            .child(cx, alerts_section)
            .child(cx, badges_section)
            .child(cx, popups_section)
    }
}
