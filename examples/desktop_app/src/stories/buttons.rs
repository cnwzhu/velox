//! `Button & Toggle` 故事页：直观对比无样式裸 `velox_base::Button` 与 `gpui-base` 同款手工样式化按钮
//! （纯白黑字直角 `"Save changes"` + 暗底灰框白字直角 `"Cancel"`），以及交互状态与 `ToggleGroup`。

use super::{state_pill, story_header, story_section};
use crate::style::{self, BasePalette};
use crate::GalleryApp;
use velox_kit::base::{Button, Toggle, ToggleGroup};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_buttons<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let p = BasePalette::from_theme(theme);
        let on_btn = cx.listener(|this, _: ClickEvent, _acx| {
            this.button_clicks = this.button_clicks.saturating_add(1);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_sel = cx.listener(|this, _: ClickEvent, _acx| {
            this.button_selected = !this.button_selected;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_load = cx.listener(|this, _: ClickEvent, _acx| {
            this.button_loading = !this.button_loading;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_bold = cx.listener(|this, v: bool, _acx| {
            this.toggle_single = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_grp_inner = cx.listener(|this, (idx, on): (usize, bool), _acx| {
            if on {
                this.toggle_group_mask |= 1u32 << idx;
            } else {
                this.toggle_group_mask &= !(1u32 << idx);
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_grp = cx.arena().alloc(move |idx: usize, on: bool, acx: &mut ActionScope| {
            on_toggle_grp_inner((idx, on), acx);
        });

        // 1. gpui-base 哲学核心对照：纯手工极简直角（Save changes / Cancel）vs 零样式裸原语
        let gpui_showcase_section = story_section(
            cx,
            theme,
            "Headless Primitive vs Hand-Styled (gpui-base Style)",
            "velox_base::Button ships with 0px radius, transparent bg & no border — style explicitly with .bg()/.border()",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(16)
                .items_center()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .flex_grow(1)
                        .p(16)
                        .gap(12)
                        .bg(p.canvas)
                        .border(1, p.border)
                        .child(
                            cx,
                            text("Hand-Styled Action Pair (0px radius, high-contrast)")
                                .size(11)
                                .color(p.muted_fg),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .flex_wrap()
                                .gap(10)
                                .items_center()
                                .child(
                                    cx,
                                    style::btn_solid(
                                        Button::new(1101, "Save changes").on_click(on_btn),
                                        theme,
                                    ),
                                )
                                .child(
                                    cx,
                                    style::btn_outline(
                                        Button::new(1102, "Cancel").on_click(on_btn),
                                        theme,
                                    ),
                                )
                                .child(
                                    cx,
                                    style::btn_ghost(
                                        Button::new(1103, "Reset").on_click(on_btn),
                                        theme,
                                    ),
                                )
                                .child(
                                    cx,
                                    style::btn_danger(
                                        Button::new(1104, "Delete").on_click(on_btn),
                                        theme,
                                    ),
                                ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .w(260)
                        .p(16)
                        .gap(10)
                        .bg(p.canvas)
                        .border(1, p.border)
                        .child(
                            cx,
                            text("Bare velox_base::Button::new() (Zero Style)")
                                .size(11)
                                .color(p.muted_fg),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(16)
                                .items_center()
                                .child(cx, Button::new(1105, "Bare Button").on_click(on_btn))
                                .child(
                                    cx,
                                    Button::new(1106, "Bare + Pad")
                                        .px(8)
                                        .py(4)
                                        .on_click(on_btn),
                                ),
                        ),
                ),
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(8)
                .items_center()
                .child(
                    cx,
                    state_pill(cx, theme, fmt!(cx, "Clicks: {}", self.button_clicks)),
                )
                .child(
                    cx,
                    text("Try Tab + Enter/Space — keyboard activation & cursor work out of the box even when unstyled.")
                        .size(11)
                        .color(p.muted_fg),
                ),
        );

        let sizes_states_section = story_section(
            cx,
            theme,
            "Custom Sizes & Interactive States",
            "App-defined heights • Selected • Loading (SVG/SDF spinner guard) • Disabled • Custom Pill",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(10)
                .items_center()
                .child(
                    cx,
                    style::btn_solid(
                        Button::new(1111, "Compact")
                            .font_size(11)
                            .h(26)
                            .px(10)
                            .on_click(on_btn),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::btn_solid(
                        Button::new(1112, "Default")
                            .font_size(13)
                            .h(34)
                            .px(16)
                            .on_click(on_btn),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::btn_solid(
                        Button::new(1113, "Large")
                            .font_size(14)
                            .h(40)
                            .px(20)
                            .on_click(on_btn),
                        theme,
                    ),
                )
                .child(cx, div().w(1).h(22).bg(p.border))
                .child(
                    cx,
                    if self.button_selected {
                        style::btn_solid(
                            Button::new(1114, "Selected")
                                .selected(true)
                                .on_click(on_toggle_sel),
                            theme,
                        )
                    } else {
                        style::btn_outline(
                            Button::new(1114, "Select Me")
                                .selected(false)
                                .on_click(on_toggle_sel),
                            theme,
                        )
                    },
                )
                .child(
                    cx,
                    style::btn_solid(
                        Button::new(1115, "Submitting")
                            .loading(self.button_loading)
                            .on_click(on_btn),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::btn_outline(
                        Button::new(1116, "Toggle Loading")
                            .font_size(12)
                            .h(28)
                            .px(10)
                            .on_click(on_toggle_load),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::btn_subtle(
                        Button::new(1117, "Disabled")
                            .disabled(true)
                            .on_click(on_btn),
                        theme,
                    ),
                )
                .child(
                    cx,
                    Button::new(1118, "Custom Pill")
                        .px(12)
                        .py(6)
                        .rounded(16)
                        .bg(p.surface_subtle)
                        .border(1, p.solid_bg)
                        .text_color(p.fg)
                        .hover_bg(p.hover)
                        .on_click(on_btn),
                ),
        );

        let toggle_section = story_section(
            cx,
            theme,
            "Toggle & ToggleGroup",
            "Headless pressed state styled with high-contrast monochrome borders & active inversions",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(12)
                .items_center()
                .child(
                    cx,
                    style::toggle_ghost(
                        Toggle::new(1120, "Ghost Toggle", self.toggle_single)
                            .on_change(on_toggle_bold),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::toggle_outline(
                        Toggle::new(1121, "Outline Toggle", self.toggle_single)
                            .on_change(on_toggle_bold),
                        theme,
                    ),
                )
                .child(
                    cx,
                    style::toggle_solid(
                        Toggle::new(1122, "Solid Toggle", self.toggle_single)
                            .on_change(on_toggle_bold),
                        theme,
                    ),
                )
                .child(cx, div().w(1).h(22).bg(p.border))
                .child(
                    cx,
                    style::toggle_group(
                        ToggleGroup::new(
                            1123,
                            &["Bold", "Italic", "Underline", "Code"],
                            self.toggle_group_mask,
                        )
                        .multiple(true)
                        .on_toggle(on_toggle_grp),
                        theme,
                    ),
                )
                .child(
                    cx,
                    state_pill(cx, theme, fmt!(cx, "Mask: 0b{:04b}", self.toggle_group_mask)),
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
                    "Button & Toggle",
                    "Headless behavioral primitives (pointer + Enter/Space, loading guard, disabled cursor) styled explicitly by the app.",
                    "velox_base::{Button, Toggle, ToggleGroup}",
                ),
            )
            .child(cx, gpui_showcase_section)
            .child(cx, sizes_states_section)
            .child(cx, toggle_section)
    }
}
