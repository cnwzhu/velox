//! `Alerts & Badges` 故事页：全面使用 `velox-component` 展示 `Alert`、`Badge`、`Tag`、`Tooltip`、`Popover`、
//! `Kbd`、`Avatar`，并新增 `Rating`、`Skeleton`、`Spinner` 与 `Bubble` 对话气泡组件！

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{
    Alert, AlertVariant, Avatar, AvatarStatus, Badge, Bubble, BubbleAlign, Button, Kbd, Popover,
    Rating, Skeleton, Spinner, Tag, TagVariant, Tooltip,
};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_feedback<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
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
        let on_rating = cx.listener(|this, v: u8, _acx| {
            this.feedback_rating = v;
            this.event_count = this.event_count.saturating_add(1);
        });

        let alerts_section = story_section(
            cx,
            theme,
            "Alerts (velox-component: Info • Success • Warning • Error)",
            "Native 24x24 Lucide vector icons with themed semantic borders & surface tints",
        )
        .child(
            cx,
            div()
                .id(1700)
                .flex_row()
                .flex_wrap()
                .gap(10)
                .auto_animate_with(Duration::from_millis(220))
                .stagger(Duration::from_millis(45))
                .child(
                    cx,
                    Alert::new(1701, "System Ready", "wgpu 2D SDF pipeline active at 1.5x HiDPI.")
                        .variant(AlertVariant::Info)
                        .width(360),
                )
                .child(
                    cx,
                    Alert::new(1702, "Build Succeeded", "All workspace tests passed with 0 warnings.")
                        .variant(AlertVariant::Success)
                        .width(360),
                )
                .child(
                    cx,
                    Alert::new(1703, "High Frame Budget", "Spring animation running; will settle to 0% CPU.")
                        .variant(AlertVariant::Warning)
                        .width(360),
                )
                .child(
                    cx,
                    Alert::new(1704, "Validation Error", "Target port is currently occupied by another process.")
                        .variant(AlertVariant::Error)
                        .width(360),
                ),
        );

        let mut tags_row = div()
            .id(1719)
            .flex_row()
            .flex_wrap()
            .gap(8)
            .items_center()
            .auto_animate_with(Duration::from_millis(220));
        if (self.tag_visible_mask & (1 << 0)) != 0 {
            tags_row = tags_row.child(
                cx,
                Tag::new(1720, "rust-gui")
                    .variant(TagVariant::Primary)
                    .on_close(on_close_tag0),
            );
        }
        if (self.tag_visible_mask & (1 << 1)) != 0 {
            tags_row = tags_row.child(
                cx,
                Tag::new(1721, "velox-component")
                    .variant(TagVariant::Success)
                    .on_close(on_close_tag1),
            );
        }
        tags_row = tags_row
            .child(cx, Tag::new(1722, "beta").variant(TagVariant::Warning))
            .child(cx, Tag::new(1723, "deprecated").variant(TagVariant::Danger))
            .child(cx, Tag::new(1724, "outline").variant(TagVariant::Outline));
        if self.tag_visible_mask != 0b1111 {
            tags_row = tags_row.child(
                cx,
                Button::new(1725, "Restore Tags")
                    .ghost()
                    .small()
                    .on_click(on_reset_tags),
            );
        }

        let badges_section = story_section(
            cx,
            theme,
            "Badges, Closable Tags, Kbd & Avatars (velox-component)",
            "Full 8-variant badges, closable vector tags, keyboard shortcuts & status avatars",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .items_center()
                .child(cx, Badge::new("Primary").primary())
                .child(cx, Badge::new("Secondary").secondary())
                .child(cx, Badge::new("Outline").outline())
                .child(cx, Badge::new("Success").success())
                .child(cx, Badge::new("Warning").warning())
                .child(cx, Badge::new("Danger").danger())
                .child(cx, Badge::new("Info").info()),
        )
        .child(cx, tags_row)
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(16)
                .items_center()
                .child(cx, Kbd::new(1730, "Ctrl+K"))
                .child(cx, Kbd::new(1731, "Shift+Tab"))
                .child(cx, Kbd::new(1732, "Esc"))
                .child(cx, div().w(1).h(22).bg(theme.border))
                .child(cx, Avatar::new(1733, "VX").status(AvatarStatus::Online))
                .child(cx, Avatar::new(1734, "GP").status(AvatarStatus::Busy))
                .child(cx, Avatar::new(1735, "UI").status(AvatarStatus::Away))
                .child(cx, Avatar::new(1736, "OS").status(AvatarStatus::Offline)),
        );

        let rich_feedback_section = story_section(
            cx,
            theme,
            "Rating, Skeleton, Spinner & Chat Bubbles (velox-component)",
            "Interactive star Rating, loading Skeleton & Spinner, and conversational Bubble widgets",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(20)
                .items_start()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(10)
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(10)
                                .items_center()
                                .child(
                                    cx,
                                    Rating::new(1750, self.feedback_rating)
                                        .max(5)
                                        .on_change(on_rating),
                                )
                                .child(
                                    cx,
                                    state_pill(cx, theme, fmt!(cx, "{} / 5 Stars", self.feedback_rating)),
                                ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(12)
                                .items_center()
                                .child(cx, Skeleton::circle(1751, 34))
                                .child(cx, Skeleton::text_lines(1752, 160, 2))
                                .child(cx, Spinner::new(1753, 24, 45).thickness(3)),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .flex_grow(1)
                        .gap(8)
                        .child(
                            cx,
                            Bubble::new(1755, "Velox Component ships 75+ themed widgets with spring motion!")
                                .align(BubbleAlign::Start)
                                .max_width(340),
                        )
                        .child(
                            cx,
                            Bubble::new(1756, "Awesome — zero-heap frame layout + GPU SDF parity.")
                                .align(BubbleAlign::End)
                                .max_width(340),
                        ),
                ),
        );

        let popups_section = story_section(
            cx,
            theme,
            "Tooltip & Animated Popover (velox-component)",
            "Click triggers to preview floating Tooltip and EaseOutBack animated Popover",
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(20)
                .items_start()
                .child(
                    cx,
                    Tooltip::new(1740, "Themed floating tooltip", self.tooltip_open).trigger(
                        cx,
                        Button::new(
                            1741,
                            if self.tooltip_open {
                                "Hide Tooltip"
                            } else {
                                "Show Tooltip"
                            },
                        )
                        .outline()
                        .small()
                        .on_click(on_toggle_tip),
                    ),
                )
                .child(
                    cx,
                    Popover::new(1742, self.popover_open)
                        .on_close(on_close_pop)
                        .trigger(
                            cx,
                            Button::new(
                                1743,
                                if self.popover_open {
                                    "Hide Popover"
                                } else {
                                    "Show Popover"
                                },
                            )
                            .primary()
                            .small()
                            .on_click(on_toggle_pop),
                        )
                        .content(
                            cx,
                            div()
                                .flex_col()
                                .gap(4)
                                .child(cx, text("Animated Popover").size(13).color(theme.foreground))
                                .child(
                                    cx,
                                    text("Press Escape to dismiss this themed popover card.")
                                        .size(11)
                                        .color(theme.muted_foreground),
                                ),
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
                    "Alerts, Badges, Rating & Bubbles",
                    "Themed feedback banners, badges, closable tags, Rating, Skeleton, Spinner, Bubble & Popover.",
                    "velox_component::{Alert, Badge, Rating, Skeleton, Bubble, Popover}",
                ),
            )
            .child(cx, alerts_section)
            .child(cx, badges_section)
            .child(cx, rich_feedback_section)
            .child(cx, popups_section)
    }
}
