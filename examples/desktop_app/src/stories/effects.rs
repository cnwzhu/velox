//! `Effects & Shaders` 交互式高级渲染实验室故事页：
//! 完整展示并实时调试 Velox 渲染引擎的高级特性：
//! 1. 多色标渐变（任意角度 Linear / 焦点 Radial / Conic + Pad/Repeat/Reflect + OKLab/LinearRGB/sRGB）与四角独立圆角；
//! 2. 实时毛玻璃（`backdrop-filter: blur(...)` 与 Acrylic 颗粒/提饱和材质）；
//! 3. 27 种 Porter-Duff / W3C 混合模式实时对比 + 组级隔离透明度（Group Opacity）；
//! 4. 图层滤镜（Blur / ColorMatrix / 任意子树 DropShadow）、2D 仿射变换与任意 SVG Path 矢量裁剪遮罩。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{Button, Slider, Switch};
use velox_kit::prelude::*;

/// 实验室中可交互切换的 8 种代表性混合模式（覆盖 Porter-Duff、Separable 与 HSL 组）。
pub(crate) const FX_BLEND_MODES: [(&str, BlendMode); 8] = [
    ("Multiply", BlendMode::Multiply),
    ("Screen", BlendMode::Screen),
    ("Overlay", BlendMode::Overlay),
    ("Difference", BlendMode::Difference),
    ("Plus", BlendMode::Plus),
    ("Dodge", BlendMode::ColorDodge),
    ("Hue", BlendMode::Hue),
    ("Luminosity", BlendMode::Luminosity),
];

/// 实验室中可交互切换的 6 种图层滤镜预设。
pub(crate) const FX_FILTER_PRESETS: [&str; 6] = [
    "Original",
    "Grayscale",
    "Sepia",
    "Invert",
    "Hue +120°",
    "Blur 4px",
];

/// 实验室中可交互切换的 4 种 SVG Path 裁剪形状（标准 24×24 归一化矢量路径）。
pub(crate) const FX_CLIP_SHAPES: [(&str, Option<&'static str>); 4] = [
    (
        "Star",
        Some("M12 1.5 L15.1 8.4 L22.5 9.2 L16.9 14.2 L18.5 21.6 L12 17.8 L5.5 21.6 L7.1 14.2 L1.5 9.2 L8.9 8.4 Z"),
    ),
    ("Triangle", Some("M12 2 L22.5 21.5 L1.5 21.5 Z")),
    ("Diamond", Some("M12 1.5 L22.5 12 L12 22.5 L1.5 12 Z")),
    ("None", None),
];

static AURORA_STOPS: [ColorStop; 4] = [
    ColorStop::new(0.0, Color::rgb(244, 63, 94)),
    ColorStop::new(0.35, Color::rgb(139, 92, 246)),
    ColorStop::new(0.7, Color::rgb(14, 165, 233)),
    ColorStop::new(1.0, Color::rgb(16, 185, 129)),
];
static RG_COMPARE_STOPS: [ColorStop; 2] = [
    ColorStop::new(0.0, Color::rgb(255, 42, 42)),
    ColorStop::new(1.0, Color::rgb(34, 211, 88)),
];
static SPHERE_STOPS: [ColorStop; 3] = [
    ColorStop::new(0.0, Color::rgb(255, 255, 255)),
    ColorStop::new(0.32, Color::rgb(56, 189, 248)),
    ColorStop::new(1.0, Color::rgb(30, 27, 75)),
];
static CONIC_STOPS: [ColorStop; 7] = [
    ColorStop::new(0.0, Color::rgb(239, 68, 68)),
    ColorStop::new(0.17, Color::rgb(245, 158, 11)),
    ColorStop::new(0.33, Color::rgb(34, 197, 94)),
    ColorStop::new(0.50, Color::rgb(6, 182, 212)),
    ColorStop::new(0.67, Color::rgb(59, 130, 246)),
    ColorStop::new(0.83, Color::rgb(168, 85, 247)),
    ColorStop::new(1.0, Color::rgb(239, 68, 68)),
];

#[inline]
fn fx_mode_btn<'a>(
    id: u32,
    label: &'static str,
    active: bool,
    cb: &'a dyn Fn(ClickEvent, &mut ActionScope),
) -> Button<'a> {
    let b = Button::new(id, label)
        .small()
        .h(26)
        .px(10)
        .font_size(11)
        .on_click(cb);
    if active {
        b.primary()
    } else {
        b.outline()
    }
}

impl GalleryApp {
    #[inline(never)]
    pub(crate) fn render_story_effects<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let gradient_section = self.render_fx_gradients_section(cx, theme);
        let backdrop_section = self.render_fx_backdrop_section(cx, theme);
        let blend_section = self.render_fx_blend_section(cx, theme);
        let filters_section = self.render_fx_filters_section(cx, theme);

        div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Effects & Shaders Lab",
                    "Interactive showcase of OKLab gradients, per-corner radii, backdrop acrylic, 27 blend modes, filters, Transform2D & SVG clip paths.",
                    "velox_engine::paint",
                ),
            )
            .child(cx, gradient_section)
            .child(cx, backdrop_section)
            .child(cx, blend_section)
            .child(cx, filters_section)
    }

    #[inline(never)]
    fn render_fx_gradients_section<'a>(&self, cx: &'a Scope<Self>, theme: CoreTheme) -> Div<'a> {
        // ── 1. Multi-Stop Gradients & Per-Corner Radii ────────────────────────
        let on_space_oklab = cx.listener(|this, _: ClickEvent, _acx| {
            this.fx_grad_space = GradientSpace::Oklab;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_space_linear = cx.listener(|this, _: ClickEvent, _acx| {
            this.fx_grad_space = GradientSpace::LinearRgb;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_space_srgb = cx.listener(|this, _: ClickEvent, _acx| {
            this.fx_grad_space = GradientSpace::Srgb;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ext_pad = cx.listener(|this, _: ClickEvent, _acx| {
            this.fx_grad_extend = ExtendMode::Pad;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ext_repeat = cx.listener(|this, _: ClickEvent, _acx| {
            this.fx_grad_extend = ExtendMode::Repeat;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ext_reflect = cx.listener(|this, _: ClickEvent, _acx| {
            this.fx_grad_extend = ExtendMode::Reflect;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_angle_change = cx.listener(|this, v: i32, _acx| {
            this.fx_grad_angle = v.clamp(0, 360);
            this.event_count = this.event_count.saturating_add(1);
        });

        // 当选择 Repeat / Reflect 时缩短渐变轴至 45% 跨度，使重复/镜像条纹一目了然
        let linear_card_paint = if self.fx_grad_extend == ExtendMode::Pad {
            Paint::linear_angle(self.fx_grad_angle as f32, &AURORA_STOPS)
                .with_space(self.fx_grad_space)
                .with_extend(ExtendMode::Pad)
        } else {
            let rad = (self.fx_grad_angle as f32).to_radians();
            let dx = rad.sin() * 0.22;
            let dy = -rad.cos() * 0.22;
            Paint::LinearGradient {
                start: [0.5 - dx, 0.5 - dy],
                end: [0.5 + dx, 0.5 + dy],
                stops: &AURORA_STOPS,
                extend: self.fx_grad_extend,
                space: self.fx_grad_space,
            }
        };

        let linear_card = div()
            .flex_col()
            .flex_grow(1)
            .gap(6)
            .child(
                cx,
                div()
                    .id(2150)
                    .w_full()
                    .h(96)
                    .px(14)
                    .py(10)
                    .flex_col()
                    .justify_between()
                    .bg_paint(linear_card_paint)
                    .rounded_corners(CornerRadii::new(22, 4, 22, 4))
                    .border(1, Color::rgba(255, 255, 255, 70))
                    .child(
                        cx,
                        text("Linear • (22,4,22,4)")
                            .size(11)
                            .color(Color::WHITE),
                    )
                    .child(
                        cx,
                        text(fmt!(cx, "Angle {}°", self.fx_grad_angle))
                            .size(11)
                            .color(Color::rgba(255, 255, 255, 225)),
                    ),
            )
            .child(
                cx,
                text("Multi-stop Linear & Asymmetric Corners")
                    .size(11)
                    .color(theme.muted_foreground),
            );

        let oklab_compare_card = div()
            .flex_col()
            .flex_grow(1)
            .gap(6)
            .child(
                cx,
                div()
                    .flex_col()
                    .w_full()
                    .h(96)
                    .gap(6)
                    .child(
                        cx,
                        div()
                            .id(2151)
                            .w_full()
                            .h(45)
                            .px(10)
                            .items_center()
                            .flex_row()
                            .bg_linear_gradient(cx, 90.0, &RG_COMPARE_STOPS)
                            .bg_gradient_space(GradientSpace::Oklab)
                            .rounded_t(10)
                            .child(cx, text("OKLab (Vivid Midpoint)").size(11).color(Color::BLACK)),
                    )
                    .child(
                        cx,
                        div()
                            .id(2152)
                            .w_full()
                            .h(45)
                            .px(10)
                            .items_center()
                            .flex_row()
                            .bg_linear_gradient(cx, 90.0, &RG_COMPARE_STOPS)
                            .bg_gradient_space(GradientSpace::Srgb)
                            .rounded_b(10)
                            .child(cx, text("sRGB (Muddy Midpoint)").size(11).color(Color::WHITE)),
                    ),
            )
            .child(
                cx,
                text("Perceptual OKLab vs Classic sRGB")
                    .size(11)
                    .color(theme.muted_foreground),
            );

        let (radial_r, radial_focal) = if self.fx_grad_extend == ExtendMode::Pad {
            (0.62, [0.30, 0.26])
        } else {
            // Repeat / Reflect 缩小半径时同步收束焦点偏移，确保焦点严格保持在主圆内部，呈现完整同心波纹而非切线锥面
            (0.28, [0.42, 0.40])
        };
        let radial_card = div()
            .flex_col()
            .flex_grow(1)
            .gap(6)
            .child(
                cx,
                div()
                    .id(2153)
                    .w_full()
                    .h(96)
                    .p(10)
                    .flex_col()
                    .justify_end()
                    .bg_radial_focal_gradient(
                        cx,
                        [0.5, 0.5],
                        radial_r,
                        radial_focal,
                        0.02,
                        &SPHERE_STOPS,
                    )
                    .bg_gradient_space(self.fx_grad_space)
                    .bg_gradient_extend(self.fx_grad_extend)
                    .rounded(18)
                    .border(1, Color::rgba(255, 255, 255, 55))
                    .child(cx, text("Focal Radial Orb").size(11).color(Color::WHITE)),
            )
            .child(
                cx,
                text("Two-Circle Focal Radial Gradient")
                    .size(11)
                    .color(theme.muted_foreground),
            );

        let conic_card = div()
            .flex_col()
            .flex_grow(1)
            .gap(6)
            .child(
                cx,
                div()
                    .id(2154)
                    .w_full()
                    .h(96)
                    .p(10)
                    .flex_col()
                    .justify_end()
                    .bg_conic_gradient(cx, self.fx_grad_angle as f32, &CONIC_STOPS)
                    .bg_gradient_space(self.fx_grad_space)
                    .rounded_corners(CornerRadii::new(4, 22, 4, 22))
                    .border(1, Color::rgba(255, 255, 255, 70))
                    .child(cx, text("Conic Spectrum").size(11).color(Color::WHITE)),
            )
            .child(
                cx,
                text("360° Conic Sweep + CornerRadii(4,22)")
                    .size(11)
                    .color(theme.muted_foreground),
            );

        story_section(
            cx,
            theme,
            "1. Multi-Stop Gradients (Linear / Focal Radial / Conic) & Per-Corner Radii",
            "GPU Ramp Atlas + IGN Dithering • Zero-heap LUT in Software",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .w_full()
                .gap(12)
                .items_center()
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(4)
                        .items_center()
                        .child(cx, text("Space:").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            fx_mode_btn(
                                2101,
                                "OKLab",
                                self.fx_grad_space == GradientSpace::Oklab,
                                on_space_oklab,
                            ),
                        )
                        .child(
                            cx,
                            fx_mode_btn(
                                2102,
                                "LinearRGB",
                                self.fx_grad_space == GradientSpace::LinearRgb,
                                on_space_linear,
                            ),
                        )
                        .child(
                            cx,
                            fx_mode_btn(
                                2103,
                                "sRGB",
                                self.fx_grad_space == GradientSpace::Srgb,
                                on_space_srgb,
                            ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(4)
                        .items_center()
                        .child(cx, text("Extend:").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            fx_mode_btn(
                                2104,
                                "Pad",
                                self.fx_grad_extend == ExtendMode::Pad,
                                on_ext_pad,
                            ),
                        )
                        .child(
                            cx,
                            fx_mode_btn(
                                2105,
                                "Repeat",
                                self.fx_grad_extend == ExtendMode::Repeat,
                                on_ext_repeat,
                            ),
                        )
                        .child(
                            cx,
                            fx_mode_btn(
                                2106,
                                "Reflect",
                                self.fx_grad_extend == ExtendMode::Reflect,
                                on_ext_reflect,
                            ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(8)
                        .items_center()
                        .child(cx, text("Angle:").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            Slider::new(2107, self.fx_grad_angle)
                                .range(0, 360)
                                .step(15)
                                .w(120)
                                .on_change(on_angle_change),
                        )
                        .child(cx, state_pill(cx, theme, fmt!(cx, "{}°", self.fx_grad_angle))),
                ),
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(12)
                .child(cx, linear_card)
                .child(cx, oklab_compare_card)
                .child(cx, radial_card)
                .child(cx, conic_card),
        )
    }

    #[inline(never)]
    fn render_fx_backdrop_section<'a>(&self, cx: &'a Scope<Self>, theme: CoreTheme) -> Div<'a> {
        let is_dark = theme.background.r() < 128;
        // ── 2. Real-Time Backdrop Filter (Frosted Glass & Acrylic) ────────────
        let on_blur_change = cx.listener(|this, v: i32, _acx| {
            this.fx_backdrop_blur = v.clamp(0, 20);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_acrylic_toggle = cx.listener(|this, v: bool, _acx| {
            this.fx_acrylic_enabled = v;
            this.event_count = this.event_count.saturating_add(1);
        });

        let blur_u8 = self.fx_backdrop_blur.clamp(0, 20) as u8;
        let glass_tint = if is_dark {
            Color::rgba(18, 22, 34, 72)
        } else {
            Color::rgba(255, 255, 255, 92)
        };
        let glass_text_col = if is_dark { Color::WHITE } else { Color::rgb(15, 23, 42) };
        let glass_sub_col = if is_dark {
            Color::rgba(241, 245, 249, 210)
        } else {
            Color::rgba(30, 41, 59, 210)
        };

        let mut frosted_card = div()
            .id(2160)
            .pos(74, 18)
            .w(342)
            .h(92)
            .p(14)
            .flex_col()
            .justify_between()
            .rounded(14)
            .border(1, Color::rgba(255, 255, 255, 115));
        frosted_card = if self.fx_acrylic_enabled {
            frosted_card.backdrop_acrylic(blur_u8, glass_tint)
        } else {
            frosted_card.backdrop_blur(blur_u8).bg(glass_tint)
        };
        frosted_card = frosted_card
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        cx,
                        text(if self.fx_acrylic_enabled {
                            "Acrylic Frosted Glass Surface"
                        } else {
                            "Pure Gaussian / Kawase Backdrop Blur"
                        })
                        .size(13)
                        .color(glass_text_col),
                    )
                    .child(
                        cx,
                        div()
                            .px(6)
                            .py(2)
                            .rounded(6)
                            .bg(Color::rgba(255, 255, 255, 45))
                            .child(
                                cx,
                                text(fmt!(cx, "r={}px", blur_u8))
                                    .size(10)
                                    .color(glass_text_col),
                            ),
                    ),
            )
            .child(
                cx,
                text("Live background sampling • Dual Kawase on GPU • 3-Pass Box Blur on CPU")
                    .size(11)
                    .color(glass_sub_col),
            );

        static BACKDROP_BG_STOPS: [ColorStop; 3] = [
            ColorStop::new(0.0, Color::rgb(15, 23, 42)),
            ColorStop::new(0.5, Color::rgb(79, 70, 229)),
            ColorStop::new(1.0, Color::rgb(14, 116, 144)),
        ];

        let backdrop_stage = div()
            .id(2159)
            .w(490)
            .h(128)
            .rounded(12)
            .overflow_hidden()
            .bg_linear_gradient(cx, 120.0, &BACKDROP_BG_STOPS)
            .bg_gradient_space(GradientSpace::Oklab)
            .border(1, theme.border)
            // 高对比彩色背景元素，便于肉眼直观验证毛玻璃折射模糊
            .child(
                cx,
                div()
                    .pos(24, 16)
                    .w(110)
                    .h(96)
                    .rounded(48)
                    .bg(Color::rgb(249, 115, 22)),
            )
            .child(
                cx,
                div()
                    .pos(170, 10)
                    .w(130)
                    .h(52)
                    .rounded(14)
                    .bg(Color::rgb(16, 185, 129)),
            )
            .child(
                cx,
                div()
                    .pos(240, 62)
                    .w(150)
                    .h(56)
                    .rounded(18)
                    .bg(Color::rgb(236, 72, 153)),
            )
            .child(
                cx,
                div()
                    .pos(390, 22)
                    .w(80)
                    .h(84)
                    .rounded(40)
                    .bg(Color::rgb(250, 204, 21)),
            )
            .child(
                cx,
                div()
                    .pos(16, 52)
                    .w(458)
                    .h(24)
                    .child(
                        cx,
                        text("VELOX REAL-TIME BACKDROP BLUR • DUAL KAWASE PIPELINE")
                            .size(14)
                            .color(Color::WHITE),
                    ),
            )
            .child(cx, frosted_card);

        let backdrop_controls = div()
            .flex_col()
            .flex_grow(1)
            .gap(12)
            .justify_center()
            .child(
                cx,
                div()
                    .flex_col()
                    .gap(6)
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(cx, text("Backdrop Blur Radius").size(12).color(theme.foreground))
                            .child(cx, state_pill(cx, theme, fmt!(cx, "{} px", blur_u8))),
                    )
                    .child(
                        cx,
                        Slider::new(2110, self.fx_backdrop_blur)
                            .range(0, 20)
                            .step(1)
                            .w(220)
                            .on_change(on_blur_change),
                    ),
            )
            .child(
                cx,
                Switch::new(2111, self.fx_acrylic_enabled)
                    .medium()
                    .label("Acrylic Vibrancy + Grain")
                    .on_change(on_acrylic_toggle),
            )
            .child(
                cx,
                text("Drag the slider to 0 px to compare sharp background vs frosted glass.")
                    .size(11)
                    .color(theme.muted_foreground),
            );

        story_section(
            cx,
            theme,
            "2. Real-Time Backdrop Filter (Frosted Glass & Acrylic Material)",
            "backdrop_blur(r) • backdrop_acrylic(r, tint) • Rounded Corner Clipping",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(18)
                .items_center()
                .child(cx, backdrop_stage)
                .child(cx, backdrop_controls),
        )
    }

    #[inline(never)]
    fn render_fx_blend_section<'a>(&self, cx: &'a Scope<Self>, theme: CoreTheme) -> Div<'a> {
        // ── 3. 27 Blend Modes & Group Opacity Isolation ───────────────────────
        let active_blend_idx = self.fx_blend_idx.min(FX_BLEND_MODES.len() - 1);
        let (blend_name, active_blend) = FX_BLEND_MODES[active_blend_idx];

        let on_blend_0 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 0;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_1 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 1;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_2 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 2;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_3 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 3;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_4 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 4;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_5 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 5;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_6 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 6;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_blend_7 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_blend_idx = 7;
            t.event_count = t.event_count.saturating_add(1);
        });

        static BLEND_BG_STOPS: [ColorStop; 3] = [
            ColorStop::new(0.0, Color::rgb(239, 68, 68)),
            ColorStop::new(0.5, Color::rgb(59, 130, 246)),
            ColorStop::new(1.0, Color::rgb(250, 204, 21)),
        ];

        let blend_stage = div()
            .flex_col()
            .w(430)
            .gap(10)
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(cx, text("W3C & Porter-Duff Blend Stage").size(12).color(theme.foreground))
                    .child(cx, state_pill(cx, theme, fmt!(cx, "BlendMode::{}", blend_name))),
            )
            .child(
                cx,
                div()
                    .id(2170)
                    .w_full()
                    .h(104)
                    .rounded(10)
                    .overflow_hidden()
                    .bg_linear_gradient(cx, 90.0, &BLEND_BG_STOPS)
                    .border(1, theme.border)
                    .child(
                        cx,
                        div()
                            .id(2171)
                            .pos(56, 14)
                            .w(150)
                            .h(76)
                            .rounded(38)
                            .bg(Color::rgb(34, 211, 238))
                            .blend_mode(active_blend),
                    )
                    .child(
                        cx,
                        div()
                            .id(2172)
                            .pos(176, 14)
                            .w(176)
                            .h(76)
                            .rounded(18)
                            .bg(Color::rgb(244, 114, 182))
                            .blend_mode(active_blend),
                    ),
            )
            .child(
                cx,
                div()
                    .flex_row()
                    .flex_wrap()
                    .gap(6)
                    .child(cx, fx_mode_btn(2120, FX_BLEND_MODES[0].0, active_blend_idx == 0, on_blend_0))
                    .child(cx, fx_mode_btn(2121, FX_BLEND_MODES[1].0, active_blend_idx == 1, on_blend_1))
                    .child(cx, fx_mode_btn(2122, FX_BLEND_MODES[2].0, active_blend_idx == 2, on_blend_2))
                    .child(cx, fx_mode_btn(2123, FX_BLEND_MODES[3].0, active_blend_idx == 3, on_blend_3))
                    .child(cx, fx_mode_btn(2124, FX_BLEND_MODES[4].0, active_blend_idx == 4, on_blend_4))
                    .child(cx, fx_mode_btn(2125, FX_BLEND_MODES[5].0, active_blend_idx == 5, on_blend_5))
                    .child(cx, fx_mode_btn(2126, FX_BLEND_MODES[6].0, active_blend_idx == 6, on_blend_6))
                    .child(cx, fx_mode_btn(2127, FX_BLEND_MODES[7].0, active_blend_idx == 7, on_blend_7)),
            );

        let make_opacity_demo = |id: u32, isolate: bool, title: &'static str, note: &'static str| {
            let mut group = div()
                .pos(18, 12)
                .w(116)
                .h(80)
                .opacity(145)
                .child(
                    cx,
                    div()
                        .pos(0, 4)
                        .w(64)
                        .h(64)
                        .rounded(32)
                        .bg(Color::rgb(239, 68, 68)),
                )
                .child(
                    cx,
                    div()
                        .pos(40, 12)
                        .w(64)
                        .h(64)
                        .rounded(32)
                        .bg(Color::rgb(37, 99, 235)),
                );
            if isolate {
                group = group.isolate();
            }
            div()
                .flex_col()
                .flex_grow(1)
                .gap(6)
                .child(
                    cx,
                    div()
                        .id(id)
                        .w_full()
                        .h(104)
                        .rounded(10)
                        .bg(Color::rgb(248, 250, 252))
                        .border(1, theme.border)
                        .child(cx, group),
                )
                .child(cx, text(title).size(11).color(theme.foreground))
                .child(cx, text(note).size(10).color(theme.muted_foreground))
        };

        let opacity_compare = div()
            .flex_col()
            .flex_grow(1)
            .gap(10)
            .child(
                cx,
                text("Group Opacity Isolation (.isolate().opacity(145))")
                    .size(12)
                    .color(theme.foreground),
            )
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .gap(12)
                    .child(
                        cx,
                        make_opacity_demo(
                            2173,
                            true,
                            "Isolated Group",
                            "Zero bleed-through in overlap",
                        ),
                    )
                    .child(
                        cx,
                        make_opacity_demo(
                            2174,
                            false,
                            "Per-Node Opacity",
                            "Red bleeds through blue circle",
                        ),
                    ),
            );

        story_section(
            cx,
            theme,
            "3. Blend Modes (27 Porter-Duff & W3C) & Group Opacity Isolation",
            "Single-pass leaf blending • Offscreen group isolation without double-alpha bleed",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(20)
                .child(cx, blend_stage)
                .child(cx, opacity_compare),
        )
    }

    #[inline(never)]
    fn render_fx_filters_section<'a>(&self, cx: &'a Scope<Self>, theme: CoreTheme) -> Div<'a> {
        let is_dark = theme.background.r() < 128;
        // ── 4. Layer Filters (Blur / ColorMatrix / DropShadow), Transform2D & SVG ClipPath ──
        let on_flt_0 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_filter_preset = 0;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_flt_1 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_filter_preset = 1;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_flt_2 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_filter_preset = 2;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_flt_3 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_filter_preset = 3;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_flt_4 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_filter_preset = 4;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_flt_5 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_filter_preset = 5;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_rot_change = cx.listener(|t, v: i32, _| {
            t.fx_rotation_deg = v.clamp(-180, 180);
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_clip_0 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_clip_shape = 0;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_clip_1 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_clip_shape = 1;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_clip_2 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_clip_shape = 2;
            t.event_count = t.event_count.saturating_add(1);
        });
        let on_clip_3 = cx.listener(|t, _: ClickEvent, _| {
            t.fx_clip_shape = 3;
            t.event_count = t.event_count.saturating_add(1);
        });

        let active_flt = self.fx_filter_preset.min(FX_FILTER_PRESETS.len() - 1);
        let mut filtered_card = div()
            .id(2180)
            .pos(24, 18)
            .w(310)
            .h(78)
            .p(12)
            .flex_row()
            .gap(12)
            .items_center()
            .rounded(14)
            .bg_linear_gradient(cx, 115.0, &AURORA_STOPS)
            .bg_gradient_space(GradientSpace::Oklab)
            .drop_shadow(6, 5, 6, Color::rgba(0, 0, 0, 185))
            .child(
                cx,
                div()
                    .w(46)
                    .h(46)
                    .rounded(23)
                    .bg(Color::rgb(250, 204, 21))
                    .border(2, Color::WHITE),
            )
            .child(
                cx,
                div()
                    .flex_col()
                    .gap(3)
                    .child(
                        cx,
                        text(fmt!(cx, "Filter: {}", FX_FILTER_PRESETS[active_flt]))
                            .size(13)
                            .color(Color::WHITE),
                    )
                    .child(
                        cx,
                        text("ColorMatrix + Subtree DropShadow")
                            .size(11)
                            .color(Color::rgba(255, 255, 255, 225)),
                    ),
            );
        filtered_card = match active_flt {
            1 => filtered_card.grayscale(1.0),
            2 => filtered_card.sepia(1.0),
            3 => filtered_card.invert(1.0),
            4 => filtered_card.hue_rotate(120.0),
            5 => filtered_card.blur(4),
            _ => filtered_card,
        };

        let filter_col = div()
            .flex_col()
            .w(390)
            .gap(10)
            .child(
                cx,
                div()
                    .flex_row()
                    .flex_wrap()
                    .gap(6)
                    .child(cx, fx_mode_btn(2130, FX_FILTER_PRESETS[0], active_flt == 0, on_flt_0))
                    .child(cx, fx_mode_btn(2131, FX_FILTER_PRESETS[1], active_flt == 1, on_flt_1))
                    .child(cx, fx_mode_btn(2132, FX_FILTER_PRESETS[2], active_flt == 2, on_flt_2))
                    .child(cx, fx_mode_btn(2133, FX_FILTER_PRESETS[3], active_flt == 3, on_flt_3))
                    .child(cx, fx_mode_btn(2134, FX_FILTER_PRESETS[4], active_flt == 4, on_flt_4))
                    .child(cx, fx_mode_btn(2135, FX_FILTER_PRESETS[5], active_flt == 5, on_flt_5)),
            )
            .child(
                cx,
                div()
                    .w_full()
                    .h(120)
                    .rounded(10)
                    .bg(if is_dark {
                        Color::rgb(28, 30, 38)
                    } else {
                        Color::rgb(241, 245, 249)
                    })
                    .border(1, theme.border)
                    .child(cx, filtered_card),
            );

        let active_clip = self.fx_clip_shape.min(FX_CLIP_SHAPES.len() - 1);
        let (clip_name, clip_d) = FX_CLIP_SHAPES[active_clip];

        let mut clipped_badge = div()
            .id(2185)
            .pos(130, 14)
            .w(92)
            .h(92)
            .flex_col()
            .items_center()
            .justify_center()
            .bg_conic_gradient(cx, 0.0, &CONIC_STOPS)
            .bg_gradient_space(GradientSpace::Oklab)
            .rotate_deg(self.fx_rotation_deg as f32)
            .drop_shadow(5, 4, 5, Color::rgba(0, 0, 0, 175))
            .child(
                cx,
                div()
                    .w(28)
                    .h(28)
                    .rounded(14)
                    .bg(Color::rgba(255, 255, 255, 220)),
            );
        if let Some(d) = clip_d {
            clipped_badge = clipped_badge.clip_path(d);
        }

        let transform_col = div()
            .flex_col()
            .flex_grow(1)
            .gap(10)
            .child(
                cx,
                div()
                    .flex_row()
                    .flex_wrap()
                    .gap(6)
                    .items_center()
                    .child(cx, fx_mode_btn(2141, FX_CLIP_SHAPES[0].0, active_clip == 0, on_clip_0))
                    .child(cx, fx_mode_btn(2142, FX_CLIP_SHAPES[1].0, active_clip == 1, on_clip_1))
                    .child(cx, fx_mode_btn(2143, FX_CLIP_SHAPES[2].0, active_clip == 2, on_clip_2))
                    .child(cx, fx_mode_btn(2144, FX_CLIP_SHAPES[3].0, active_clip == 3, on_clip_3))
                    .child(
                        cx,
                        Slider::new(2140, self.fx_rotation_deg)
                            .range(-180, 180)
                            .step(15)
                            .w(108)
                            .on_change(on_rot_change),
                    )
                    .child(
                        cx,
                        state_pill(cx, theme, fmt!(cx, "{}°", self.fx_rotation_deg)),
                    ),
            )
            .child(
                cx,
                div()
                    .w_full()
                    .h(120)
                    .rounded(10)
                    .bg(theme.background)
                    .border(1, theme.border)
                    .child(
                        cx,
                        div()
                            .pos(12, 10)
                            .w(110)
                            .h(90)
                            .flex_col()
                            .gap(4)
                            .child(cx, text("Transform2D +").size(11).color(theme.foreground))
                            .child(cx, text("SVG ClipPath").size(11).color(theme.foreground))
                            .child(
                                cx,
                                text(fmt!(cx, "Mask: {}", clip_name))
                                    .size(10)
                                    .color(theme.muted_foreground),
                            ),
                    )
                    .child(cx, clipped_badge),
            );

        story_section(
            cx,
            theme,
            "4. Layer Filters (ColorMatrix / Blur / DropShadow), Transform2D & SVG ClipPath",
            "4×5 ColorMatrix • Alpha Contour DropShadow • Anti-Aliased SVG Vector Mask",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(18)
                .child(cx, filter_col)
                .child(cx, transform_col),
        )
    }
}
