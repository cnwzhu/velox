//! `Checkbox & Radio` 故事页：完全基于 `velox-component` 的三态原生矢量 SVG 复选框 + 键盘方向键循环切换 `RadioGroup`。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{Checkbox, Radio, RadioGroup};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_selection<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let on_parent_cb = cx.listener(|this, next: bool, _acx| {
            this.cb_items = [next, next, next];
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cb_0 = cx.listener(|this, next: bool, _acx| {
            this.cb_items[0] = next;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cb_1 = cx.listener(|this, next: bool, _acx| {
            this.cb_items[1] = next;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cb_2 = cx.listener(|this, next: bool, _acx| {
            this.cb_items[2] = next;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_radio_plan = cx.listener(|this, idx: usize, _acx| {
            this.radio_plan = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_radio_density = cx.listener(|this, idx: usize, _acx| {
            this.radio_density = idx;
            this.event_count = this.event_count.saturating_add(1);
        });

        let checked_count = self.cb_items.iter().filter(|&&v| v).count();
        let all_checked = checked_count == self.cb_items.len();
        let is_indeterminate = checked_count > 0 && !all_checked;

        let cb_section = story_section(
            cx,
            theme,
            "Tri-State Checkbox (velox-component Native SVG Check & Dash)",
            "Click child items to see parent automatically switch between Unchecked / Indeterminate / Checked",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(36)
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(10)
                        .child(
                            cx,
                            Checkbox::new(1201, all_checked)
                                .indeterminate(is_indeterminate)
                                .label("Select All Render Features")
                                .on_change(on_parent_cb),
                        )
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .pl(22)
                                .gap(8)
                                .child(
                                    cx,
                                    Checkbox::new(1202, self.cb_items[0])
                                        .label("wgpu 2D SDF Hardware Acceleration")
                                        .on_change(on_cb_0),
                                )
                                .child(
                                    cx,
                                    Checkbox::new(1203, self.cb_items[1])
                                        .label("Native Vector SVG Path Engine")
                                        .on_change(on_cb_1),
                                )
                                .child(
                                    cx,
                                    Checkbox::new(1204, self.cb_items[2])
                                        .label("Fractional HiDPI Subpixel Crispness")
                                        .on_change(on_cb_2),
                                ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(10)
                        .child(
                            cx,
                            text("Sizes & Disabled States")
                                .size(12)
                                .color(theme.muted_foreground),
                        )
                        .child(
                            cx,
                            Checkbox::new(1205, true)
                                .small()
                                .label("Small (14px)")
                                .on_change(on_cb_0),
                        )
                        .child(
                            cx,
                            Checkbox::new(1206, true)
                                .medium()
                                .label("Medium (16px)")
                                .on_change(on_cb_1),
                        )
                        .child(
                            cx,
                            Checkbox::new(1207, true)
                                .large()
                                .label("Large (20px)")
                                .on_change(on_cb_2),
                        )
                        .child(
                            cx,
                            Checkbox::new(1208, true)
                                .disabled(true)
                                .label("Disabled Checked"),
                        )
                        .child(
                            cx,
                            Checkbox::new(1209, false)
                                .disabled(true)
                                .label("Disabled Unchecked"),
                        ),
                ),
        )
        .child(
            cx,
            div().flex_row().child(
                cx,
                state_pill(
                    cx,
                    theme,
                    fmt!(
                        cx,
                        "Selected: {}/3 (Parent: {})",
                        checked_count,
                        if all_checked {
                            "Checked"
                        } else if is_indeterminate {
                            "Indeterminate"
                        } else {
                            "Unchecked"
                        }
                    ),
                ),
            ),
        );

        let radio_section = story_section(
            cx,
            theme,
            "Radio & RadioGroup (velox-component Arrow-Key Roving Selection)",
            "Focus any radio item with Tab, then press Arrow Left/Right/Up/Down or Home/End",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(48)
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(10)
                        .child(
                            cx,
                            text("Vertical Group")
                                .size(12)
                                .color(theme.muted_foreground),
                        )
                        .child(
                            cx,
                            RadioGroup::new(
                                1220,
                                &[
                                    "Auto (GPU wgpu + Software Fallback)",
                                    "Force Built-in Software SDF",
                                    "Headless Testing Surface",
                                ],
                                Some(self.radio_plan),
                            )
                            .vertical()
                            .on_change(on_radio_plan),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(12)
                        .child(
                            cx,
                            text("Horizontal Group & Disabled")
                                .size(12)
                                .color(theme.muted_foreground),
                        )
                        .child(
                            cx,
                            RadioGroup::new(
                                1221,
                                &["Compact", "Default", "Comfortable"],
                                Some(self.radio_density),
                            )
                            .horizontal()
                            .on_change(on_radio_density),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(16)
                                .child(cx, Radio::new(1222, "Disabled On", true).disabled(true))
                                .child(cx, Radio::new(1223, "Disabled Off", false).disabled(true)),
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
                    "Checkbox & Radio",
                    "Themed velox-component tri-state SVG checkboxes & arrow-key roving RadioGroup.",
                    "velox_component::{Checkbox, Radio, RadioGroup}",
                ),
            )
            .child(cx, cb_section)
            .child(cx, radio_section)
    }
}
