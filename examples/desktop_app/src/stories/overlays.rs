//! `Dialog & Modal` 故事页：使用 `velox-component` 交互式唤起自带弹簧/回弹入场动画的
//! `Dialog`、`AlertDialog (Destructive)`、`Modal`、`Sheet` 与 `Notification`（支持遮罩拦截与 Esc 关闭）。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::Button;
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_overlays<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
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

        let trigger_card = |id: u32, title: &'a str, desc: &'a str, btn: Element<'a>| -> Div<'a> {
            div()
                .id(id)
                .flex_col()
                .w(240)
                .p(16)
                .gap(10)
                .rounded(8)
                .bg(theme.card)
                .hover_bg(theme.secondary)
                .border(1, theme.border)
                .animate_color(Duration::from_millis(160))
                .child(cx, text(title).size(14).color(theme.foreground))
                .child(cx, text(desc).size(12).color(theme.muted_foreground))
                .child(cx, btn)
        };

        let triggers = div()
            .id(1650)
            .flex_row()
            .flex_wrap()
            .gap(14)
            .auto_animate_with(Duration::from_millis(220))
            .stagger(Duration::from_millis(40))
            .child(
                cx,
                trigger_card(
                    1651,
                    "Standard Dialog",
                    "EaseOutBack spring entry, SVG close icon, ghost fade-out & Escape dismiss.",
                    Button::new(1601, "Open Dialog")
                        .primary()
                        .on_click(on_open_dialog)
                        .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    1652,
                    "AlertDialog (Destructive)",
                    "Destructive confirm button, spring entry, ghost fade-out & Escape cancel.",
                    Button::new(1602, "Delete Workspace")
                        .danger()
                        .on_click(on_open_alert)
                        .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    1653,
                    "Centered Modal",
                    "Backdrop fade-in occlusion & click-outside / Escape dismissal.",
                    Button::new(1603, "Open Modal")
                        .outline()
                        .on_click(on_open_modal)
                        .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    1654,
                    "Side Sheet Drawer",
                    "Directional Spring slide-in & ghost slide-out drawer with Escape dismiss.",
                    Button::new(1604, "Open Sheet")
                        .outline()
                        .on_click(on_open_sheet)
                        .into_element(cx),
                ),
            )
            .child(
                cx,
                trigger_card(
                    1655,
                    "Floating Notification",
                    "Spring slide-in top-right toast with zero-config ghost slide-out exit.",
                    Button::new(1605, "Show Notification")
                        .secondary()
                        .on_click(on_open_notif)
                        .into_element(cx),
                ),
            );

        let status_section = story_section(
            cx,
            theme,
            "Overlay Lifecycle & Escape Handling",
            "All overlays use .overlay().occlude_hit() + std spring enter animations + Escape dismissal",
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(10)
                .items_center()
                .child(cx, text("Last Action:").size(12).color(theme.muted_foreground))
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
                    "Themed velox-component overlays with std spring entry animations, hit occlusion & Escape dismissal.",
                    "velox_component::{Dialog, AlertDialog, Modal, Sheet}",
                ),
            )
            .child(cx, triggers)
            .child(cx, status_section)
    }
}
