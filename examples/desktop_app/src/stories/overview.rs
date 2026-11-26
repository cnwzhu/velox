//! `Overview` 故事页：`velox-component` 语义主题色板矩阵 + 实时交互体验台 + 架构特性。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{ArcGauge, Badge, Button, Checkbox, ProgressBar, Slider, Switch};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_overview<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let swatch = |name: &'a str, bg: Color, fg: Color| -> Div<'a> {
            div()
                .flex_col()
                .w(108)
                .h(52)
                .p(8)
                .rounded(6)
                .bg(bg)
                .border(1, theme.border)
                .justify_between()
                .child(cx, text(name).size(11).color(fg))
                .child(
                    cx,
                    text(fmt!(cx, "#{:02X}{:02X}{:02X}", bg.r(), bg.g(), bg.b()))
                        .size(10)
                        .color(fg),
                )
        };

        let palette_section = story_section(
            cx,
            theme,
            "Semantic Theme Palette (velox-component Shadcn Zinc + Blue)",
            "Automatic Dark / Light semantic tokens shared across all 75+ velox-component widgets",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .child(cx, swatch("background", theme.background, theme.foreground))
                .child(cx, swatch("card", theme.card, theme.foreground))
                .child(cx, swatch("primary", theme.primary, theme.primary_foreground))
                .child(cx, swatch("secondary", theme.secondary, theme.secondary_foreground))
                .child(cx, swatch("success", theme.success, theme.success_foreground))
                .child(cx, swatch("warning", theme.warning, theme.warning_foreground))
                .child(cx, swatch("danger", theme.danger, theme.danger_foreground))
                .child(cx, swatch("info", theme.info, theme.info_foreground)),
        );

        let playground_elem: Element<'a> = if let Some(pg_handle) = self.overview_playground {
            pg_handle.into_element(cx)
        } else {
            let on_dec_load = cx.listener(|this, _: ClickEvent, _acx| {
                this.cpu_load_pct = this.cpu_load_pct.saturating_sub(10);
                this.signals.cpu_load_pct.set_if_changed(this.cpu_load_pct);
                this.event_count = this.event_count.saturating_add(1);
            });
            let on_inc_load = cx.listener(|this, _: ClickEvent, _acx| {
                this.cpu_load_pct = (this.cpu_load_pct + 10).min(100);
                this.signals.cpu_load_pct.set_if_changed(this.cpu_load_pct);
                this.event_count = this.event_count.saturating_add(1);
            });
            let on_budget = cx.listener(|this, val: i32, _acx| {
                this.frame_budget_pct = val;
                this.signals.frame_budget_pct.set_if_changed(val);
                this.event_count = this.event_count.saturating_add(1);
            });
            let on_quick_sw = cx.listener(|this, val: bool, _acx| {
                this.vsync_enabled = val;
                this.signals.vsync_enabled.set_if_changed(val);
                this.event_count = this.event_count.saturating_add(1);
            });
            let on_quick_cb = cx.listener(|this, val: bool, _acx| {
                this.damage_overlay_hint = val;
                this.signals.damage_overlay_hint.set_if_changed(val);
                this.event_count = this.event_count.saturating_add(1);
            });
            let cpu_load = self.signals.cpu_load_pct.get_untracked();
            let frame_budget = self.signals.frame_budget_pct.get_untracked();
            let vsync = self.signals.vsync_enabled.get_untracked();
            let dmg_hint = self.signals.damage_overlay_hint.get_untracked();
            div()
                .flex_row()
                .w_full()
                .h(210)
                .gap(16)
                .child(
                    cx,
                    div()
                        .flex_col()
                        .flex_grow(1)
                        .h_full()
                        .p(16)
                        .gap(12)
                        .rounded(8)
                        .bg(theme.card)
                        .border(1, theme.border)
                        .items_center()
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(6)
                                .items_center()
                                .child(
                                    cx,
                                    svg_icon(14, 14, "M22 12h-4l-3 9L9 3l-3 9H2")
                                        .tint(theme.primary),
                                )
                                .child(
                                    cx,
                                    text(cx.t("ov.load_title")).size(13).color(theme.foreground),
                                ),
                        )
                        .child(
                            cx,
                            ArcGauge::new(301, 108, cpu_load)
                                .thickness(10)
                                .label(fmt!(cx, "{}%", cpu_load)),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(8)
                                .child(
                                    cx,
                                    Button::new(304, "-10%")
                                        .outline()
                                        .small()
                                        .w(74)
                                        .on_click(on_dec_load),
                                )
                                .child(
                                    cx,
                                    Button::new(305, "+10%")
                                        .primary()
                                        .small()
                                        .w(74)
                                        .on_click(on_inc_load),
                                ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .flex_grow(1)
                        .h_full()
                        .p(16)
                        .gap(12)
                        .rounded(8)
                        .bg(theme.card)
                        .border(1, theme.border)
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .child(
                                    cx,
                                    div()
                                        .flex_row()
                                        .gap(6)
                                        .items_center()
                                        .child(
                                            cx,
                                            svg_icon(
                                                14,
                                                14,
                                                "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM12 6v6l4 2",
                                            )
                                            .tint(theme.primary),
                                        )
                                        .child(
                                            cx,
                                            text(cx.t("ov.budget_title"))
                                                .size(13)
                                                .color(theme.foreground),
                                        ),
                                )
                                .child(
                                    cx,
                                    state_pill(cx, theme, fmt!(cx, "{} FPS", frame_budget)),
                                ),
                        )
                        .child(cx, ProgressBar::new(302, 280, cpu_load).height(8))
                        .child(
                            cx,
                            Slider::new(303, frame_budget)
                                .range(30, 144)
                                .step(2)
                                .w(280)
                                .on_change(on_budget),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(16)
                                .items_center()
                                .child(
                                    cx,
                                    Switch::new(306, vsync)
                                        .label("VSync")
                                        .on_change(on_quick_sw),
                                )
                                .child(
                                    cx,
                                    Checkbox::new(307, dmg_hint)
                                        .label("Damage Tracking")
                                        .on_change(on_quick_cb),
                                ),
                        ),
                )
                .into_element(cx)
        };

        let arch_section = story_section(
            cx,
            theme,
            "Velox Component & Engine Highlights",
            "Built on velox-component themed widgets with std spring physics, RichText, PNG/VectorCanvas & zero-copy GPU",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .child(cx, Badge::new("velox-component 75+").primary())
                .child(cx, Badge::new("std Spring Physics").info())
                .child(cx, Badge::new("RichText • BiDi • Emoji").success())
                .child(cx, Badge::new("Pure-Rust PNG / NineSlice").secondary())
                .child(cx, Badge::new("Native 2D VectorCanvas").outline())
                .child(cx, Badge::new("0% CPU Idle Sleep").warning()),
        )
        .child(
            cx,
            text(cx.t("ov.desc_note"))
                .size(12)
                .color(theme.muted_foreground),
        );

        let mut root = div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    cx.t("ov.title"),
                    cx.t("ov.subtitle"),
                    "velox_component • Themed UI + Spring Motion",
                ),
            )
            .child(cx, playground_elem);
        if let Some(async_handle) = self.async_task_view {
            root = root.child(cx, async_handle);
        }
        root.child(cx, palette_section).child(cx, arch_section)
    }
}
