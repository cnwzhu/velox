//! `Engine Lab` 故事页：
//! 全方位交互式展示 Velox 引擎最新底层能力：
//! 1. 富文本排版（`RichText` + 多字族/粗斜体/编程连字/下划线高亮/彩色 Emoji + BiDi 视觉重排）；
//! 2. 纯 Rust PNG 解码管线 + `LruImageCache` + 5 档 `ObjectFit` 与 `NineSlice` 九宫格无变形拉伸；
//! 3. 原生 2D 矢量画布（`VectorCanvas`：解析抗锯齿雷达扇形、同心圆、虚线与多边形航迹）；
//! 4. 高级 CSS 布局（Grid `fr` + `col_span`、`aspect_ratio`、`RTL` 一键切换与嵌套滚动链）及 DevTools Inspector HUD。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{Badge, Button, Switch};
use velox_kit::prelude::*;

const DEMO_IMG_ASSET: ImageAssetId = ImageAssetId(0x5645_4C58);

const FIT_MODES: [(&str, ObjectFit); 5] = [
    ("Fill", ObjectFit::Fill),
    ("Contain", ObjectFit::Contain),
    ("Cover", ObjectFit::Cover),
    ("ScaleDown", ObjectFit::ScaleDown),
    ("None", ObjectFit::None),
];

/// 确保演示用 32x32 渐进色棋盘纹理通过纯 Rust PNG 编码 -> 解码注册进全局 LRU 缓存（幂等一次）。
fn ensure_demo_png_registered() {
    if matches!(image_load_state(DEMO_IMG_ASSET), Some(ImageLoadState::Ready(_))) {
        return;
    }
    let w = 32usize;
    let h = 32usize;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let border = x < 4 || y < 4 || x >= w - 4 || y >= h - 4;
            let checker = ((x / 4) + (y / 4)) % 2 == 0;
            let (r, g, b) = if border {
                (59u8, 130, 246) // 蓝色九宫格边框
            } else if checker {
                ((x * 7).min(255) as u8, 165, (255 - y * 6) as u8)
            } else {
                (30, 41, 59)
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let _ = register_image_rgba(DEMO_IMG_ASSET, w as u32, h as u32, rgba);
}

impl GalleryApp {
    pub(crate) fn render_story_engine_lab<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        ensure_demo_png_registered();

        let on_toggle_rtl = cx.listener(|this, v: bool, _acx| {
            this.lab_rtl_enabled = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_hud = cx.listener(|this, v: bool, _acx| {
            this.inspector_enabled = v;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_step_radar = cx.listener(|this, _: ClickEvent, _acx| {
            this.lab_radar_angle = (this.lab_radar_angle + 30) % 360;
            this.event_count = this.event_count.saturating_add(1);
        });

        // 1. RichText 富文本 / 多字族 / 编程连字 / 彩色 Emoji / BiDi
        let bidi_src = "Hello שלום (Velox) 123";
        let bidi_runs = reorder_bidi_visual(bidi_src, self.lab_rtl_enabled);
        let bidi_summary = fmt!(
            cx,
            "\"{}\"  ->  {} visual runs (base={})",
            bidi_src,
            bidi_runs.len(),
            if self.lab_rtl_enabled { "RTL" } else { "LTR" }
        );

        let richtext_section = story_section(
            cx,
            theme,
            "RichText, Font Styles, OpenType Ligatures, Color Emoji & BiDi",
            "Multi-family inline spans (Bold / Italic / Mono / Highlight / Underline / Strike) + Unicode BiDi",
        )
        .child(
            cx,
            div()
                .w_full()
                .p(12)
                .rounded(8)
                .bg(theme.background)
                .border(1, theme.border)
                .flex_col()
                .gap(8)
                .child(
                    cx,
                    rich_text(
                        cx,
                        &[
                            TextSpan::new("Velox ")
                                .bold()
                                .size(15)
                                .color(theme.foreground),
                            TextSpan::new("RichText ")
                                .italic()
                                .size(15)
                                .color(theme.primary),
                            TextSpan::new("fn(x) -> a != b <= c => ok ")
                                .mono()
                                .ligatures(true)
                                .size(13)
                                .bg(theme.secondary)
                                .color(theme.success),
                            TextSpan::new("Underline ")
                                .underline()
                                .size(13)
                                .color(theme.info),
                            TextSpan::new("Deprecated ")
                                .strikethrough()
                                .size(13)
                                .color(theme.muted_foreground),
                            TextSpan::new("🚀 🎨 ⚡").size(15),
                        ],
                    ),
                )
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(10)
                        .items_center()
                        .child(cx, Badge::new("BiDi Reordered").info())
                        .child(cx, text(bidi_summary).size(12).color(theme.foreground)),
                ),
        );

        // 2. DecodedImage (ObjectFit + NineSlice) & VectorCanvas 实时雷达图
        let fit_mode = FIT_MODES[self.lab_object_fit_idx.min(FIT_MODES.len() - 1)].1;
        let fit_name = FIT_MODES[self.lab_object_fit_idx.min(FIT_MODES.len() - 1)].0;
        let mut fit_btns = div().flex_row().flex_wrap().gap(6).items_center();
        for (idx, &(label, _)) in FIT_MODES.iter().enumerate() {
            let on_pick = cx.listener(move |this, _: ClickEvent, _acx| {
                this.lab_object_fit_idx = idx;
                this.event_count = this.event_count.saturating_add(1);
            });
            let mut b = Button::new(2510 + idx as u32, label).small().on_click(on_pick);
            b = if self.lab_object_fit_idx == idx {
                b.primary()
            } else {
                b.outline()
            };
            fit_btns = fit_btns.child(cx, b);
        }

        let poly_pts = cx.arena().alloc_iter([
            VecPoint::new(96.0, 22.0),
            VecPoint::new(146.0, 56.0),
            VecPoint::new(125.0, 92.0),
            VecPoint::new(62.0, 86.0),
            VecPoint::new(50.0, 46.0),
        ]);
        let axis_h = cx
            .arena()
            .alloc_iter([VecPoint::new(16.0, 58.0), VecPoint::new(176.0, 58.0)]);
        let axis_v = cx
            .arena()
            .alloc_iter([VecPoint::new(96.0, 10.0), VecPoint::new(96.0, 106.0)]);
        let radar_start = self.lab_radar_angle as f32;
        let radar_prims = cx.arena().alloc_iter([
            VectorPrimitive::Circle {
                cx: 96.0,
                cy: 58.0,
                radius: 42.0,
                fill: Color::rgba(15, 23, 42, 180),
                stroke: theme.border,
                stroke_width: 1.2,
            },
            VectorPrimitive::Circle {
                cx: 96.0,
                cy: 58.0,
                radius: 23.0,
                fill: Color::TRANSPARENT,
                stroke: theme.border,
                stroke_width: 1.0,
            },
            VectorPrimitive::Polyline {
                points: axis_h,
                color: theme.border,
                width: 1.0,
                dash_len: 4.0,
                gap_len: 4.0,
                closed: false,
            },
            VectorPrimitive::Polyline {
                points: axis_v,
                color: theme.border,
                width: 1.0,
                dash_len: 4.0,
                gap_len: 4.0,
                closed: false,
            },
            VectorPrimitive::Sector {
                cx: 96.0,
                cy: 58.0,
                inner_r: 0.0,
                outer_r: 41.0,
                start_deg: radar_start,
                sweep_deg: 55.0,
                fill: Color::rgba(16, 185, 129, 130),
            },
            VectorPrimitive::Polygon {
                points: poly_pts,
                fill: Color::rgba(59, 130, 246, 95),
                stroke: theme.primary,
                stroke_width: 2.0,
            },
        ]);

        let media_vector_row = div()
            .flex_row()
            .w_full()
            .gap(14)
            .child(
                cx,
                story_section(
                    cx,
                    theme,
                    "Image Pipeline (ObjectFit & NineSlice)",
                    "LRU cached RGBA texture with live fit mode & distortion-free 9-slice borders",
                )
                .flex_grow(1)
                .child(cx, fit_btns)
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(14)
                        .items_center()
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .gap(4)
                                .child(
                                    cx,
                                    div()
                                        .w(136)
                                        .h(78)
                                        .rounded(8)
                                        .bg(theme.background)
                                        .border(1, theme.border)
                                        .overflow_hidden()
                                        .child(
                                            cx,
                                            decoded_image(DEMO_IMG_ASSET, 136, 78)
                                                .fit(fit_mode)
                                                .rounded(8),
                                        ),
                                )
                                .child(
                                    cx,
                                    text(fmt!(cx, "ObjectFit::{fit_name}"))
                                        .size(11)
                                        .color(theme.muted_foreground),
                                ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .gap(4)
                                .child(
                                    cx,
                                    div()
                                        .w(156)
                                        .h(78)
                                        .rounded(8)
                                        .bg(theme.background)
                                        .border(1, theme.border)
                                        .overflow_hidden()
                                        .child(
                                            cx,
                                            decoded_image(DEMO_IMG_ASSET, 156, 78)
                                                .nine_slice(NineSliceInsets::all(4)),
                                        ),
                                )
                                .child(
                                    cx,
                                    text("NineSlice (4px crisp frame)")
                                        .size(11)
                                        .color(theme.muted_foreground),
                                ),
                        ),
                ),
            )
            .child(
                cx,
                story_section(
                    cx,
                    theme,
                    "Native 2D VectorCanvas",
                    "Antialiased Sector, Circle & Polygon",
                )
                .id(2519)
                .w(348)
                .overflow_hidden()
                .child(
                    cx,
                    div()
                        .flex_row()
                        .w_full()
                        .gap(12)
                        .items_center()
                        .justify_between()
                        .child(
                            cx,
                            div()
                                .rounded(8)
                                .bg(theme.background)
                                .border(1, theme.border)
                                .child(cx, vector_canvas(cx, 192, 116, radar_prims)),
                        )
                        .child(
                            cx,
                            div()
                                .flex_col()
                                .flex_grow(1)
                                .gap(8)
                                .items_center()
                                .child(
                                    cx,
                                    Button::new(2520, "Sweep +30°")
                                        .primary()
                                        .small()
                                        .w(96)
                                        .on_click(on_step_radar),
                                )
                                .child(
                                    cx,
                                    state_pill(cx, theme, fmt!(cx, "{}°", self.lab_radar_angle)),
                                ),
                        ),
                ),
            );

        // 3. 高级布局（Grid fr + col_span、aspect_ratio、RTL 镜像）+ DevTools HUD 开关
        let mut rtl_demo_row = div()
            .flex_row()
            .w_full()
            .gap(8)
            .p(8)
            .rounded(6)
            .bg(theme.background)
            .border(1, theme.border)
            .child(cx, Badge::new("1 • Start").primary())
            .child(cx, Badge::new("2 • Middle").info())
            .child(cx, Badge::new("3 • End").success());
        if self.lab_rtl_enabled {
            rtl_demo_row = rtl_demo_row.rtl(true);
        }

        let layout_hud_section = story_section(
            cx,
            theme,
            "Advanced Layout (Grid fr / col_span / aspect_ratio / RTL) & DevTools HUD",
            "Zero-bloat LayoutExtras cold path + live inspector overlay toggle",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(16)
                        .items_center()
                        .child(
                            cx,
                            Switch::new(2530, self.lab_rtl_enabled)
                                .label("RTL Mirror Layout")
                                .on_change(on_toggle_rtl),
                        )
                        .child(
                            cx,
                            Switch::new(2534, self.inspector_enabled)
                                .label("DevTools Inspector HUD")
                                .on_change(on_toggle_hud),
                        ),
                )
                .child(
                    cx,
                    state_pill(
                        cx,
                        theme,
                        if self.inspector_enabled {
                            "HUD: ACTIVE (Dirty + Hover Box)"
                        } else {
                            "HUD: Off • Toggle to inspect live"
                        },
                    ),
                ),
        )
        .child(cx, rtl_demo_row)
        .child(
            cx,
            div()
                .grid(3)
                .grid_template_fr(cx, &[1, 2, 1])
                .w_full()
                .gap(8)
                .child(
                    cx,
                    div()
                        .grid_col_span(2)
                        .h(52)
                        .p(10)
                        .rounded(6)
                        .bg(theme.secondary)
                        .border(1, theme.border)
                        .items_center()
                        .flex_row()
                        .justify_between()
                        .child(cx, text("Grid Span 2 (1fr + 2fr = 75%)").size(12).color(theme.foreground))
                        .child(cx, Badge::new("col_span(2)").primary()),
                )
                .child(
                    cx,
                    div()
                        .w(120)
                        .aspect_ratio(2.0)
                        .p(8)
                        .rounded(6)
                        .bg(theme.card)
                        .border(1, theme.primary)
                        .items_center()
                        .justify_center()
                        .child(cx, text("2:1 Aspect").size(11).color(theme.primary)),
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
                    "Engine Lab",
                    "RichText BiDi/Emoji, Pure-Rust PNG ObjectFit/NineSlice, VectorCanvas, Grid fr/RTL & DevTools HUD",
                    "velox_engine + velox_component",
                ),
            )
            .child(cx, richtext_section)
            .child(cx, media_vector_row)
            .child(cx, layout_hud_section)
    }
}
