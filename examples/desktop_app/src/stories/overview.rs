//! `Overview` 故事页：无样式 `velox-base` 应用层单色直角令牌矩阵 + 实时交互快速体验台 + 架构特性。

use super::{state_pill, story_header, story_section};
use crate::style::{self, BasePalette};
use crate::GalleryApp;
use velox_kit::base::{ArcGauge, Badge, Button, Checkbox, ProgressBar, Slider, Switch};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_overview<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let p = BasePalette::from_theme(theme);

        let swatch = |name: &'a str, bg: Color, fg: Color| -> Div<'a> {
            div()
                .flex_col()
                .w(112)
                .h(52)
                .p(8)
                .bg(bg)
                .border(1, p.border_strong)
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
            "App-Owned High-Contrast Palette (velox-base is Headless)",
            "All surfaces, borders & solid accents are styled explicitly in desktop_app/src/style.rs",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .child(cx, swatch("canvas", p.canvas, p.fg))
                .child(cx, swatch("surface", p.surface, p.fg))
                .child(cx, swatch("subtle", p.surface_subtle, p.fg))
                .child(cx, swatch("solid", p.solid_bg, p.solid_fg))
                .child(cx, swatch("danger", p.danger_bg, p.danger_fg))
                .child(cx, swatch("success", theme.success, theme.success_foreground))
                .child(cx, swatch("warning", theme.warning, theme.warning_foreground)),
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
                        .bg(p.surface)
                        .border(1, p.border)
                        .items_center()
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(6)
                                .items_center()
                                .child(
                                    cx,
                                    svg_icon(14, 14, "M22 12h-4l-3 9L9 3l-3 9H2").tint(p.fg),
                                )
                                .child(cx, text(cx.t("ov.load_title")).size(13).color(p.fg)),
                        )
                        .child(
                            cx,
                            style::arc_gauge(
                                ArcGauge::new(301, 108, cpu_load)
                                    .thickness(10)
                                    .label(fmt!(cx, "{}%", cpu_load)),
                                theme,
                            ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(8)
                                .child(
                                    cx,
                                    style::btn_outline(
                                        Button::new(304, "-10%")
                                            .font_size(12)
                                            .w(74)
                                            .h(28)
                                            .on_click(on_dec_load),
                                        theme,
                                    ),
                                )
                                .child(
                                    cx,
                                    style::btn_solid(
                                        Button::new(305, "+10%")
                                            .font_size(12)
                                            .w(74)
                                            .h(28)
                                            .on_click(on_inc_load),
                                        theme,
                                    ),
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
                        .bg(p.surface)
                        .border(1, p.border)
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
                                            .tint(p.fg),
                                        )
                                        .child(
                                            cx,
                                            text(cx.t("ov.budget_title")).size(13).color(p.fg),
                                        ),
                                )
                                .child(
                                    cx,
                                    state_pill(cx, theme, fmt!(cx, "{} FPS", frame_budget)),
                                ),
                        )
                        .child(
                            cx,
                            style::progress_bar(
                                ProgressBar::new(302, 280, cpu_load).height(8),
                                theme,
                            ),
                        )
                        .child(
                            cx,
                            style::slider(
                                Slider::new(303, frame_budget)
                                    .range(30, 144)
                                    .step(2)
                                    .w(280)
                                    .on_change(on_budget),
                                theme,
                            ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(16)
                                .items_center()
                                .child(
                                    cx,
                                    style::switch(
                                        Switch::new(306, vsync)
                                            .label("VSync")
                                            .on_change(on_quick_sw),
                                        theme,
                                    ),
                                )
                                .child(
                                    cx,
                                    style::checkbox(
                                        Checkbox::new(307, dmg_hint)
                                            .label("Damage Tracking")
                                            .on_change(on_quick_cb),
                                        theme,
                                    ),
                                ),
                        ),
                )
                .into_element(cx)
        };

        let arch_section = story_section(
            cx,
            theme,
            "Headless Architecture Highlights (gpui-base Philosophy)",
            "Zero default chrome in velox-base; Shadcn theme lives exclusively in velox-component",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .child(cx, style::badge_solid(Badge::new("Zero Default Style"), theme))
                .child(cx, style::badge_outline(Badge::new("Tri-State SVG Checkbox"), theme))
                .child(cx, style::badge_outline(Badge::new("28x16 / 36x20 / 44x24 Switch"), theme))
                .child(cx, style::badge_subtle(Badge::new("Keyboard Roving Nav"), theme))
                .child(cx, style::badge_subtle(Badge::new("Escape Overlay Dismiss"), theme))
                .child(cx, style::badge_subtle(Badge::new("0% CPU Idle Sleep"), theme)),
        )
        .child(
            cx,
            text(cx.t("ov.desc_note")).size(12).color(p.muted_fg),
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
                    "velox_base • Headless Primitives",
                ),
            )
            .child(cx, playground_elem);
        if let Some(async_handle) = self.async_task_view {
            root = root.child(cx, async_handle);
        }
        root.child(cx, palette_section).child(cx, arch_section)
    }
}
