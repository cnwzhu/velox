//! `Button & Toggle` 故事页：展示 `velox-component` 全套 8 档语义变体、3 档尺寸、`std` 无闪烁平滑悬停动效、
//! 交互状态与 `ToggleGroup`。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{Button, Toggle, ToggleGroup};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_buttons<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
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

        // 1. velox-component 全套 8 档语义变体（自带 std 无闪烁平滑配色微动效）与 unstyled 模式展示
        let gpui_showcase_section = story_section(
            cx,
            theme,
            "velox-component Semantic Variants (8 Variants + Zero-Flicker Hover Motion)",
            "Primary / Secondary / Outline / Ghost / Success / Warning / Danger / Info with automatic theme tokens",
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
                        .p(14)
                        .gap(10)
                        .rounded(8)
                        .bg(theme.background)
                        .border(1, theme.border)
                        .child(
                            cx,
                            text("velox_component::Button (8 Shadcn Semantic Variants)")
                                .size(11)
                                .color(theme.muted_foreground),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .flex_wrap()
                                .gap(8)
                                .items_center()
                                .child(cx, Button::new(1101, "Primary").primary().on_click(on_btn))
                                .child(cx, Button::new(1102, "Secondary").secondary().on_click(on_btn))
                                .child(cx, Button::new(1103, "Outline").outline().on_click(on_btn))
                                .child(cx, Button::new(1104, "Ghost").ghost().on_click(on_btn))
                                .child(cx, Button::new(1107, "Success").success().on_click(on_btn))
                                .child(cx, Button::new(1108, "Warning").warning().on_click(on_btn))
                                .child(cx, Button::new(1109, "Danger").danger().on_click(on_btn))
                                .child(cx, Button::new(1110, "Info").info().on_click(on_btn)),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .w(230)
                        .p(14)
                        .gap(10)
                        .rounded(8)
                        .bg(theme.background)
                        .border(1, theme.border)
                        .child(
                            cx,
                            text("velox_component::Button::unstyled()")
                                .size(11)
                                .color(theme.muted_foreground),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(12)
                                .items_center()
                                .child(cx, Button::new(1105, "Bare").unstyled().on_click(on_btn))
                                .child(
                                    cx,
                                    Button::new(1106, "Bare + Pad")
                                        .unstyled()
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
                    text("Try Tab + Enter/Space — instant focus-visible ring with smooth 140ms hover transitions.")
                        .size(11)
                        .color(theme.muted_foreground),
                ),
        );

        let sizes_states_section = story_section(
            cx,
            theme,
            "Standard Sizes & Interactive States (velox-component)",
            "Small / Medium / Large • Selected • Loading (SDF spinner) • Disabled • Custom Pill",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(10)
                .items_center()
                .child(cx, Button::new(1111, "Small").primary().small().on_click(on_btn))
                .child(cx, Button::new(1112, "Medium").primary().medium().on_click(on_btn))
                .child(cx, Button::new(1113, "Large").primary().large().on_click(on_btn))
                .child(cx, div().w(1).h(22).bg(theme.border))
                .child(
                    cx,
                    if self.button_selected {
                        Button::new(1114, "Selected")
                            .primary()
                            .selected(true)
                            .on_click(on_toggle_sel)
                    } else {
                        Button::new(1114, "Select Me")
                            .outline()
                            .selected(false)
                            .on_click(on_toggle_sel)
                    },
                )
                .child(
                    cx,
                    Button::new(1115, "Submitting")
                        .primary()
                        .loading(self.button_loading)
                        .on_click(on_btn),
                )
                .child(
                    cx,
                    Button::new(1116, "Toggle Loading")
                        .outline()
                        .small()
                        .on_click(on_toggle_load),
                )
                .child(
                    cx,
                    Button::new(1117, "Disabled")
                        .secondary()
                        .disabled(true)
                        .on_click(on_btn),
                )
                .child(
                    cx,
                    Button::new(1118, "Custom Pill")
                        .outline()
                        .rounded(16)
                        .on_click(on_btn),
                ),
        );

        let toggle_section = story_section(
            cx,
            theme,
            "Toggle & ToggleGroup (velox-component)",
            "Themed toggle buttons & multi-select formatting toolbar group",
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
                    Toggle::new(1120, "Ghost Toggle", self.toggle_single)
                        .ghost()
                        .on_change(on_toggle_bold),
                )
                .child(
                    cx,
                    Toggle::new(1121, "Outline Toggle", self.toggle_single)
                        .outline()
                        .on_change(on_toggle_bold),
                )
                .child(
                    cx,
                    Toggle::new(1122, "Primary Toggle", self.toggle_single)
                        .primary()
                        .on_change(on_toggle_bold),
                )
                .child(cx, div().w(1).h(22).bg(theme.border))
                .child(
                    cx,
                    ToggleGroup::new(
                        1123,
                        &["Bold", "Italic", "Underline", "Code"],
                        self.toggle_group_mask,
                    )
                    .multiple(true)
                    .on_toggle(on_toggle_grp),
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
                    "8-variant themed velox-component buttons with std hover motion, loading spinner & ToggleGroup.",
                    "velox_component::{Button, Toggle, ToggleGroup}",
                ),
            )
            .child(cx, gpui_showcase_section)
            .child(cx, sizes_states_section)
            .child(cx, toggle_section)
    }
}
