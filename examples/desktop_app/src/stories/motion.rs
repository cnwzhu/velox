//! `Motion & Physics` 故事页：
//! 展示 `velox-component` 在 `std` 下增强的二阶物理弹簧动画引擎与更多主题化交互组件：
//! 1. [`SpringAnimator`] 多通道物理弹簧交互台（5 档 `MotionPreset` 实时切换 + 中途打断保持速度连续 + Impulse 速度冲量）；
//! 2. 原生 [`VectorCanvas`] 实时物理阻尼振荡波形示波器；
//! 3. `SpringProgress`、`SpringCard`、`SpringReveal`、`Carousel` 与 `Roller` 主题化动效组件展台。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{
    Accordion, AccordionItem, Badge, Button, Carousel, MotionPreset, Roller, SpringAnimator,
    SpringCard, SpringProgress, SpringReveal, Switch, Tabs,
};
use velox_kit::prelude::*;

pub(crate) const MOTION_CH_TARGET: u64 = SpringAnimator::key(2400, 0);
pub(crate) const MOTION_CH_REVEAL: u64 = SpringAnimator::key(2400, 1);
pub(crate) const MOTION_CH_PROGRESS: u64 = SpringAnimator::key(2400, 2);

const PRESETS: [MotionPreset; 5] = [
    MotionPreset::Snappy,
    MotionPreset::Bouncy,
    MotionPreset::Gentle,
    MotionPreset::Subtle,
    MotionPreset::Instant,
];

const CAROUSEL_SLIDES: &[&str] = &[
    "01 • Second-Order Spring Physics (Interruptible Velocity)",
    "02 • Zero-Heap ScratchArena Transient Frame Layout",
    "03 • 2D SDF Single-Pass GPU + Software Pixel Parity",
    "04 • Cross-API Zero-Copy DX12 / Vulkan / Metal Interop",
];

const ROLLER_OPTIONS: &[&str] = &[
    "Snappy (280 / 22)",
    "Bouncy (240 / 12)",
    "Gentle (140 / 20)",
    "Stiff (420 / 34)",
    "Custom 60Hz",
    "Custom 120Hz",
];

const MOTION_FAQ: &[AccordionItem<'static>] = &[
    AccordionItem::new(
        "Why interruptible springs instead of fixed timers?",
        "When a user reverses direction mid-flight, SpringAnimator preserves instantaneous velocity for natural physics with zero snap.",
    ),
    AccordionItem::new(
        "Does std component motion wake CPU while idle?",
        "Never. Once position & velocity settle below rest_eps, step_all() returns false and Velox returns to 0% CPU ControlFlow::Wait.",
    ),
];

impl GalleryApp {
    pub(crate) fn render_story_motion<'a>(
        &mut self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        // 驱动二阶物理弹簧步进：仅在仍有活跃通道时请求下一帧，收敛后自动 0% CPU 休眠
        let now = cx.now_ms();
        let dt_ms = if self.motion_last_ms == 0 {
            16
        } else {
            now.saturating_sub(self.motion_last_ms).clamp(1, 33) as u16
        };
        self.motion_last_ms = now;
        if self.spring_anim.step_all(dt_ms) {
            cx.request_frame();
        }

        let pos_pct = self
            .spring_anim
            .value_or(MOTION_CH_TARGET, self.motion_target_pct);
        let vel_now = self.spring_anim.velocity_or(MOTION_CH_TARGET, 0.0);
        let reveal_p = self
            .spring_anim
            .value_or(MOTION_CH_REVEAL, if self.motion_reveal_open { 1.0 } else { 0.0 });
        let prog_val = self
            .spring_anim
            .value_or(MOTION_CH_PROGRESS, self.motion_target_pct);
        let cur_preset = self.spring_anim.preset();

        // 预设切换按钮回调
        let preset_btn = |idx: usize, preset: MotionPreset| -> Element<'a> {
            let on_pick = cx.listener(move |this, _: ClickEvent, _acx| {
                this.spring_anim.set_preset(preset);
                // 立即触发一次反向冲量让用户瞬间感受新预设物理阻尼质感
                this.spring_anim.impulse(MOTION_CH_TARGET, this.motion_target_pct, 180.0);
                this.event_count = this.event_count.saturating_add(1);
            });
            let active = cur_preset == preset;
            let mut b = Button::new(2401 + idx as u32, preset.label())
                .small()
                .on_click(on_pick);
            b = if active { b.primary() } else { b.outline() };
            b.into_element(cx)
        };

        // 目标位置预设（支持运动中途任意打断）
        let target_btn = |idx: usize, pct: f32, label: &'static str| -> Element<'a> {
            let on_go = cx.listener(move |this, _: ClickEvent, _acx| {
                this.motion_target_pct = pct;
                this.spring_anim
                    .animate_to(MOTION_CH_TARGET, this.motion_target_pct, pct);
                this.spring_anim
                    .animate_to(MOTION_CH_PROGRESS, this.motion_target_pct, pct);
                this.event_count = this.event_count.saturating_add(1);
            });
            let active = (self.motion_target_pct - pct).abs() < 1.0;
            let mut b = Button::new(2410 + idx as u32, label)
                .small()
                .on_click(on_go);
            b = if active { b.info() } else { b.secondary() };
            b.into_element(cx)
        };

        let on_impulse = cx.listener(|this, _: ClickEvent, _acx| {
            this.spring_anim
                .impulse(MOTION_CH_TARGET, this.motion_target_pct, 420.0);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_reveal = cx.listener(|this, open: bool, _acx| {
            this.motion_reveal_open = open;
            let target = if open { 1.0 } else { 0.0 };
            this.spring_anim
                .animate_to(MOTION_CH_REVEAL, if open { 0.0 } else { 1.0 }, target);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_carousel = cx.listener(|this, idx: usize, _acx| {
            this.motion_carousel_idx = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_roller = cx.listener(|this, idx: usize, _acx| {
            this.motion_roller_idx = idx;
            let mapped = match idx {
                0 => MotionPreset::Snappy,
                1 => MotionPreset::Bouncy,
                2 => MotionPreset::Gentle,
                3 => MotionPreset::Subtle,
                _ => MotionPreset::Snappy,
            };
            this.spring_anim.set_preset(mapped);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_acc = cx.listener(|this, (idx, open): (usize, bool), _acx| {
            if open {
                this.motion_accordion_mask |= 1u32 << idx;
            } else {
                this.motion_accordion_mask &= !(1u32 << idx);
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_acc_raw = cx.arena().alloc(move |idx: usize, open: bool, acx: &mut ActionScope| {
            on_acc((idx, open), acx);
        });
        let on_tab_mode = cx.listener(move |this, idx: usize, _acx| {
            this.motion_tab_idx = idx;
            let targets = [20.0f32, 58.0, 92.0];
            let pct = targets[idx.min(2)];
            this.motion_target_pct = pct;
            this.spring_anim.animate_to(MOTION_CH_TARGET, pos_pct, pct);
            this.spring_anim.animate_to(MOTION_CH_PROGRESS, prog_val, pct);
            this.event_count = this.event_count.saturating_add(1);
        });

        // 实时预测当前 SpringConfig 的阻尼响应曲线，用原生 VectorCanvas 绘制示波器波形
        let mut wave_pts: [VecPoint; 36] = [VecPoint::new(0.0, 0.0); 36];
        let mut sim = Spring::new(0.0, cur_preset.spring_config());
        sim.animate_to(1.0);
        for (i, pt) in wave_pts.iter_mut().enumerate() {
            let x = 10.0 + (i as f32) * (310.0 / 35.0);
            let y = 78.0 - sim.value().clamp(-0.2, 1.35) * 52.0;
            *pt = VecPoint::new(x, y);
            sim.step(20);
        }
        let wave_slice = cx.arena().alloc_iter(wave_pts.iter().copied());
        let ref_line = cx
            .arena()
            .alloc_iter([VecPoint::new(10.0, 26.0), VecPoint::new(320.0, 26.0)]);
        let dot_x = 10.0 + (pos_pct.clamp(0.0, 100.0) / 100.0) * 310.0;

        let wave_prims = cx.arena().alloc_iter([
            VectorPrimitive::Polyline {
                points: ref_line,
                color: theme.border,
                width: 1.5,
                dash_len: 5.0,
                gap_len: 4.0,
                closed: false,
            },
            VectorPrimitive::Polyline {
                points: wave_slice,
                color: theme.primary,
                width: 2.4,
                dash_len: 0.0,
                gap_len: 0.0,
                closed: false,
            },
            VectorPrimitive::Circle {
                cx: dot_x,
                cy: 26.0,
                radius: 5.0,
                fill: theme.success,
                stroke: Color::WHITE,
                stroke_width: 1.5,
            },
        ]);

        // 轨道滑块球偏移像素
        let track_w = 340i16;
        let knob_sz = 26i16;
        let knob_x = (((track_w - knob_sz) as f32) * (pos_pct.clamp(0.0, 115.0) / 100.0))
            .clamp(0.0, (track_w - knob_sz) as f32) as i16;

        let spring_lab_section = story_section(
            cx,
            theme,
            "Interactive Second-Order Spring Lab (SpringAnimator)",
            "Click targets mid-flight to observe continuous velocity interruption & automatic 0% CPU settle",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(8)
                .items_center()
                .child(cx, text("Preset:").size(12).color(theme.muted_foreground))
                .child(cx, preset_btn(0, PRESETS[0]))
                .child(cx, preset_btn(1, PRESETS[1]))
                .child(cx, preset_btn(2, PRESETS[2]))
                .child(cx, preset_btn(3, PRESETS[3]))
                .child(cx, preset_btn(4, PRESETS[4]))
                .child(cx, div().w(1).h(18).bg(theme.border))
                .child(cx, target_btn(0, 10.0, "10%"))
                .child(cx, target_btn(1, 45.0, "45%"))
                .child(cx, target_btn(2, 80.0, "80%"))
                .child(cx, target_btn(3, 100.0, "100%"))
                .child(
                    cx,
                    Button::new(2415, "Impulse +420")
                        .warning()
                        .small()
                        .on_click(on_impulse),
                ),
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
                        .gap(12)
                        .child(
                            cx,
                            div()
                                .id(2420)
                                .w(track_w)
                                .h(38)
                                .p(6)
                                .rounded(19)
                                .bg(theme.muted)
                                .border(1, theme.border)
                                .child(
                                    cx,
                                    div()
                                        .id(2421)
                                        .pos(knob_x, 0)
                                        .w(knob_sz)
                                        .h(knob_sz)
                                        .rounded(13)
                                        .bg(theme.primary)
                                        .shadow(8, 0, 2, Color::rgba(0, 0, 0, 90)),
                                ),
                        )
                        .child(
                            cx,
                            SpringProgress::new(2422, prog_val)
                                .size(track_w, 12)
                                .label("SpringProgress (Live Oscillator)")
                                .color(theme.primary)
                                .preset(cur_preset),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .gap(8)
                                .items_center()
                                .child(
                                    cx,
                                    state_pill(
                                        cx,
                                        theme,
                                        fmt!(cx, "Pos: {}% | Vel: {} u/s", pos_pct as i32, vel_now as i32),
                                    ),
                                )
                                .child(
                                    cx,
                                    Badge::new(if self.spring_anim.is_active() {
                                        "ANIMATING"
                                    } else {
                                        "SETTLED • 0% CPU"
                                    })
                                    .with_variant(if self.spring_anim.is_active() {
                                        ComponentVariant::Warning
                                    } else {
                                        ComponentVariant::Success
                                    }),
                                ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .p(10)
                        .rounded(8)
                        .bg(theme.background)
                        .border(1, theme.border)
                        .child(
                            cx,
                            text(fmt!(cx, "Step Response Curve ({})", cur_preset.label()))
                                .size(11)
                                .color(theme.muted_foreground),
                        )
                        .child(cx, vector_canvas(cx, 330, 92, wave_prims)),
                ),
        );

        // 2. SpringCard / SpringReveal / Tabs / Accordion / Carousel / Roller 综合展台
        let reveal_card = SpringCard::new(2430)
            .preset(cur_preset)
            .flex_grow(1)
            .child(
                cx,
                div()
                    .flex_col()
                    .gap(10)
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .child(cx, text("SpringReveal & Animated Accordion").size(13).color(theme.foreground))
                            .child(
                                cx,
                                Switch::new(2431, self.motion_reveal_open)
                                    .label("Reveal Panel")
                                    .on_change(on_toggle_reveal),
                            ),
                    )
                    .child(
                        cx,
                        SpringReveal::new(2432, 68, reveal_p).child(
                            cx,
                            div()
                                .w_full()
                                .p(10)
                                .rounded(6)
                                .bg(theme.secondary)
                                .border(1, theme.border)
                                .flex_col()
                                .gap(4)
                                .child(
                                    cx,
                                    text("Smooth physics-driven height & opacity reveal")
                                        .size(12)
                                        .color(theme.foreground),
                                )
                                .child(
                                    cx,
                                    text("Powered by velox_component::SpringReveal + SpringAnimator")
                                        .size(11)
                                        .color(theme.muted_foreground),
                                ),
                        ),
                    )
                    .child(
                        cx,
                        Tabs::new(2433, &["20% Low", "58% Mid", "92% Peak"], self.motion_tab_idx)
                            .on_select(on_tab_mode),
                    )
                    .child(
                        cx,
                        Accordion::new(2434, MOTION_FAQ, self.motion_accordion_mask)
                            .width(340)
                            .on_toggle_item(on_acc_raw),
                    ),
            );

        let carousel_roller_card = SpringCard::new(2440)
            .preset(cur_preset)
            .w(360)
            .child(
                cx,
                div()
                    .flex_col()
                    .gap(12)
                    .child(
                        cx,
                        text("Carousel & Roller (velox-component)")
                            .size(13)
                            .color(theme.foreground),
                    )
                    .child(
                        cx,
                        Carousel::new(2441, CAROUSEL_SLIDES, self.motion_carousel_idx)
                            .size(330, 88)
                            .on_change(on_carousel),
                    )
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(12)
                            .items_center()
                            .child(
                                cx,
                                Roller::new(2442, ROLLER_OPTIONS, self.motion_roller_idx)
                                    .w(180)
                                    .visible_rows(3)
                                    .on_change(on_roller),
                            )
                            .child(
                                cx,
                                div()
                                    .flex_col()
                                    .gap(6)
                                    .child(cx, text("Active Roller").size(11).color(theme.muted_foreground))
                                    .child(
                                        cx,
                                        Badge::new(ROLLER_OPTIONS[self.motion_roller_idx.min(ROLLER_OPTIONS.len() - 1)])
                                            .info(),
                                    )
                                    .child(
                                        cx,
                                        text("Click rows to sync preset")
                                            .size(11)
                                            .color(theme.muted_foreground),
                                    ),
                            ),
                    ),
            );

        div()
            .flex_col()
            .w_full()
            .gap(14)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Motion & Physics",
                    "std-enhanced velox-component spring physics, interruptible SpringAnimator & animated widgets",
                    "velox_component::motion",
                ),
            )
            .child(cx, spring_lab_section)
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .gap(14)
                    .child(cx, reveal_card)
                    .child(cx, carousel_roller_card),
            )
    }
}
