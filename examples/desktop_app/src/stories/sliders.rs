//! `Switch & Slider` 故事页：完全基于 `velox-component` 的 3 尺寸平滑弹簧 Switch + 步进/连续 Slider + ProgressBar + ArcGauge。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{ArcGauge, ProgressBar, Slider, Switch, SwitchLabelSide};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_sliders<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let on_sw0 = cx.listener(|this, v: bool, _acx| {
            this.switch_states[0] = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sw1 = cx.listener(|this, v: bool, _acx| {
            this.switch_states[1] = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sw2 = cx.listener(|this, v: bool, _acx| {
            this.switch_states[2] = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sw3 = cx.listener(|this, v: bool, _acx| {
            this.switch_states[3] = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cont = cx.listener(|this, v: i32, _acx| {
            this.slider_continuous = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_stepped = cx.listener(|this, v: i32, _acx| {
            this.slider_stepped = v;
            this.event_count = this.event_count.saturating_add(1);
        });

        let switch_section = story_section(
            cx,
            theme,
            "Switch (velox-component 3 Sizes & Spring Thumb Motion)",
            "Small 28x16 • Medium 36x20 • Large 44x24 • Left/Right labels • Custom accent",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(24)
                .items_center()
                .child(
                    cx,
                    Switch::new(1301, self.switch_states[0])
                        .small()
                        .label("Small (28x16)")
                        .on_change(on_sw0),
                )
                .child(
                    cx,
                    Switch::new(1302, self.switch_states[1])
                        .medium()
                        .label("Medium (36x20)")
                        .on_change(on_sw1),
                )
                .child(
                    cx,
                    Switch::new(1303, self.switch_states[2])
                        .large()
                        .label("Large (44x24)")
                        .on_change(on_sw2),
                ),
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(24)
                .items_center()
                .child(
                    cx,
                    Switch::new(1304, self.switch_states[3])
                        .label("Left Label")
                        .label_side(SwitchLabelSide::Left)
                        .color(theme.success)
                        .on_change(on_sw3),
                )
                .child(
                    cx,
                    Switch::new(1305, true)
                        .disabled(true)
                        .label("Disabled On"),
                )
                .child(
                    cx,
                    Switch::new(1306, false)
                        .disabled(true)
                        .label("Disabled Off"),
                ),
        );

        let slider_section = story_section(
            cx,
            theme,
            "Slider, ProgressBar & ArcGauge (velox-component)",
            "Click anywhere on track, drag thumb, or focus with Tab & press Arrows / Home / End",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(28)
                .items_center()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .flex_grow(1)
                        .gap(14)
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .gap(6)
                                .child(
                                    cx,
                                    div()
                                        .flex_row()
                                        .w(340)
                                        .justify_between()
                                        .child(
                                            cx,
                                            text("Continuous (0..=100, step=1)")
                                                .size(12)
                                                .color(theme.foreground),
                                        )
                                        .child(
                                            cx,
                                            state_pill(
                                                cx,
                                                theme,
                                                fmt!(cx, "{}%", self.slider_continuous),
                                            ),
                                        ),
                                )
                                .child(
                                    cx,
                                    Slider::new(1310, self.slider_continuous)
                                        .range(0, 100)
                                        .w(340)
                                        .on_change(on_cont),
                                ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .gap(6)
                                .child(
                                    cx,
                                    div()
                                        .flex_row()
                                        .w(340)
                                        .justify_between()
                                        .child(
                                            cx,
                                            text("Stepped Discrete (0..=100, step=10)")
                                                .size(12)
                                                .color(theme.foreground),
                                        )
                                        .child(
                                            cx,
                                            state_pill(
                                                cx,
                                                theme,
                                                fmt!(cx, "{}", self.slider_stepped),
                                            ),
                                        ),
                                )
                                .child(
                                    cx,
                                    Slider::new(1311, self.slider_stepped)
                                        .range(0, 100)
                                        .step(10)
                                        .accent(theme.success)
                                        .w(340)
                                        .on_change(on_stepped),
                                ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .gap(6)
                                .child(
                                    cx,
                                    text("Linked ProgressBar")
                                        .size(12)
                                        .color(theme.muted_foreground),
                                )
                                .child(
                                    cx,
                                    ProgressBar::new(
                                        1312,
                                        340,
                                        self.slider_continuous.clamp(0, 100) as u8,
                                    )
                                    .height(8),
                                ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .items_center()
                        .gap(8)
                        .child(
                            cx,
                            ArcGauge::new(
                                1313,
                                112,
                                self.slider_continuous.clamp(0, 100) as u8,
                            )
                            .thickness(10)
                            .label(fmt!(cx, "{}%", self.slider_continuous)),
                        )
                        .child(
                            cx,
                            text("SDF ArcGauge")
                                .size(11)
                                .color(theme.muted_foreground),
                        ),
                ),
        );

        let gpui_slot_section = story_section(
            cx,
            theme,
            "Slot-Customized velox-component Controls",
            "Themed components still expose full .track/.indicator/.thumb slot callbacks & StyledExt syntax",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap_6()
                .items_center()
                .child(
                    cx,
                    Switch::new(1320, self.switch_states[1])
                        .label("Slot Switch")
                        .on_change(on_sw1)
                        .track(cx, move |track, st| {
                            track
                                .w(px(40.))
                                .h(px(22.))
                                .rounded_full()
                                .p_0_5()
                                .transition_colors()
                                .duration_150()
                                .when(st.is_checked(), |s| s.bg(theme.primary))
                                .when(!st.is_checked(), |s| {
                                    s.bg(theme.secondary).border_1().border_color(theme.border)
                                })
                        })
                        .thumb(cx, move |thumb, st| {
                            thumb
                                .size(px(16.))
                                .rounded_full()
                                .when(st.is_checked(), |s| s.bg(theme.primary_foreground))
                                .when(!st.is_checked(), |s| s.bg(theme.foreground))
                        }),
                )
                .child(
                    cx,
                    Slider::new(1321, self.slider_continuous)
                        .range(0, 100)
                        .w(px(220.))
                        .on_change(on_cont)
                        .track(cx, move |tr, _| {
                            tr.h(px(6.))
                                .rounded_full()
                                .bg(theme.secondary)
                                .border_1()
                                .border_color(theme.border)
                        })
                        .indicator(cx, move |ind, _| ind.rounded_full().bg(theme.primary))
                        .thumb(cx, move |th, _| {
                            th.size_4()
                                .rounded_full()
                                .bg(theme.primary)
                                .border_2()
                                .border_color(theme.card)
                                .hover(|s| s.opacity_f(0.85))
                        }),
                )
                .child(
                    cx,
                    ProgressBar::new(1322, 160, self.slider_continuous.clamp(0, 100) as u8)
                        .height(8)
                        .color(theme.primary)
                        .track_color(theme.secondary)
                        .rounded_full()
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden(),
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
                    "Switch & Slider",
                    "Themed velox-component switches with spring thumb motion & keyboard-accessible sliders.",
                    "velox_component::{Switch, Slider, ProgressBar, ArcGauge}",
                ),
            )
            .child(cx, switch_section)
            .child(cx, slider_section)
            .child(cx, gpui_slot_section)
    }
}
