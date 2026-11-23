//! `Dialog & Modal` 故事页：交互式唤起 `Dialog`、`AlertDialog (Destructive)`、`Modal` 与 `Sheet`（支持遮罩拦截与 Esc 关闭）。

use super::{state_pill, story_header, story_section};
use crate::style::{self, BasePalette};
use crate::GalleryApp;
use velox_kit::base::Button;
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_overlays<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let p = BasePalette::from_theme(theme);
        let on_open_dialog = cx.listener(|this, _: ClickEvent, _acx| {
            this.demo_dialog_open = true;
            this.last_overlay_result = "Opened Dialog (press Escape or Close)";
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_open_alert = cx.listener(|this, _: ClickEvent, _acx| {
            this.demo_alert_dialog_open = true;
            this.last_overlay_result = "Opened Destructive AlertDialog (press Escape to cancel)";
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_open_modal = cx.listener(|this, _: ClickEvent, _acx| {
            this.demo_modal_open = true;
            this.last_overlay_result = "Opened Modal (click backdrop or press Escape)";
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_open_sheet = cx.listener(|this, _: ClickEvent, _acx| {
            this.demo_sheet_open = true;
            this.last_overlay_result = "Opened Right Sheet Drawer (press Escape to dismiss)";
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_open_notif = cx.listener(|this, _: ClickEvent, _acx| {
            this.notification_open = true;
            this.last_overlay_result = "Spawned floating Notification toast";
            this.event_count = this.event_count.saturating_add(1);
        });

        let trigger_card = |title: &'a str, desc: &'a str, btn: Element<'a>| -> Div<'a> {
            div()
                .flex_col()
                .w(240)
                .p(16)
                .gap(10)
                .bg(p.surface)
                .border(1, p.border)
                .child(cx, text(title).size(14).color(p.fg))
                .child(cx, text(desc).size(12).color(p.muted_fg))
                .child(cx, btn)
        };

        let triggers = div()
            .flex_row()
            .flex_wrap()
            .gap(14)
            .child(
                cx,
                trigger_card(
                    "Standard Dialog",
                    "Header, description, SVG close icon, footer actions & sharp border.",
                    style::btn_solid(
                        Button::new(1601, "Open Dialog").on_click(on_open_dialog),
                        theme,
                    )
                    .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    "AlertDialog (Destructive)",
                    "Explicit destructive confirm button, i18n labels & Escape cancel.",
                    style::btn_danger(
                        Button::new(1602, "Delete Workspace").on_click(on_open_alert),
                        theme,
                    )
                    .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    "Centered Modal",
                    "Backdrop occlusion & click-outside / Escape dismissal.",
                    style::btn_outline(
                        Button::new(1603, "Open Modal").on_click(on_open_modal),
                        theme,
                    )
                    .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    "Side Sheet Drawer",
                    "Edge drawer panel with overlay hit occlusion and Escape dismiss.",
                    style::btn_outline(
                        Button::new(1604, "Open Sheet").on_click(on_open_sheet),
                        theme,
                    )
                    .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    "Floating Notification",
                    "Top-right notification card with vector SVG status icon.",
                    style::btn_subtle(
                        Button::new(1605, "Show Notification").on_click(on_open_notif),
                        theme,
                    )
                    .into_element(cx),
                ),
            );

        let status_section = story_section(
            cx,
            theme,
            "Overlay Lifecycle & Escape Handling",
            "All overlays use .overlay().occlude_hit() + .on_navigate_back() for Escape dismissal",
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(10)
                .items_center()
                .child(cx, text("Last Action:").size(12).color(p.muted_fg))
                .child(cx, state_pill(cx, theme, self.last_overlay_result)),
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
                    "Dialog, AlertDialog, Modal & Sheet",
                    "Hit-occluded headless overlays styled explicitly by the app with sharp 1px borders & Escape dismissal.",
                    "velox_base::{Dialog, AlertDialog, Modal, Sheet}",
                ),
            )
            .child(cx, triggers)
            .child(cx, status_section)
    }
}
