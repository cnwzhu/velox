//! 基础实时数据可视化图表组件 `Chart`（支持 `Line`、`Bar`、`Area`、`Pie`、`Candlestick` 五种图表类型，纯整数 SDF 抗锯齿绘制，0 字节常驻 RAM）。

use velox_engine::prelude::*;
use velox_engine::sdf::dist_to_segment;

/// 图表展示类型枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ChartKind {
    /// 连续抗锯齿折线图（默认）。
    #[default]
    Line,
    /// 垂直柱状图。
    Bar,
    /// 面积填充折线图。
    Area,
    /// 饼图 / 环形占比图。
    Pie,
    /// K 线图（蜡烛图，OHLC）。
    Candlestick,
}

/// K 线图（`Candlestick`）单根蜡烛的开高低收（OHLC）数据点。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CandlestickBar {
    pub open: i16,
    pub high: i16,
    pub low: i16,
    pub close: i16,
}

impl CandlestickBar {
    /// 创建一根 OHLC 蜡烛数据点。
    #[inline(always)]
    pub const fn new(open: i16, high: i16, low: i16, close: i16) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }
}

/// 基础实时数据图表组件（`Chart`）。
pub struct Chart<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub data: &'a [i16],
    pub min_val: i16,
    pub max_val: i16,
    pub kind: ChartKind,
    pub series_color: Option<Color>,
    pub grid_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub grid_lines: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Chart<'a> {
    /// 创建一个实时数据图表，传入组件 `id`、宽高、数据点切片 `data` 与数值量程 `(min_val, max_val)`。
    #[inline(always)]
    pub const fn new(
        id: u32,
        width: i16,
        height: i16,
        data: &'a [i16],
        min_val: i16,
        max_val: i16,
    ) -> Self {
        Self {
            id,
            width: if width < 24 { 24 } else { width },
            height: if height < 24 { 24 } else { height },
            data,
            min_val,
            max_val: if max_val <= min_val {
                min_val.saturating_add(1)
            } else {
                max_val
            },
            kind: ChartKind::Line,
            series_color: None,
            grid_color: None,
            bg_color: None,
            grid_lines: 3,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置图表类型（[`ChartKind::Line`]、[`ChartKind::Bar`]、[`ChartKind::Area`]、[`ChartKind::Pie`] 或 [`ChartKind::Candlestick`]）。
    #[inline(always)]
    pub const fn kind(mut self, kind: ChartKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置 K 线图（[`ChartKind::Candlestick`]）的 OHLC 数据序列并切换为蜡烛图模式。
    #[inline(always)]
    pub const fn candles(mut self, candles: &'a [CandlestickBar]) -> Self {
        self.data = unsafe {
            core::slice::from_raw_parts(candles.as_ptr() as *const i16, candles.len() * 4)
        };
        self.kind = ChartKind::Candlestick;
        self
    }

    /// 设置数据序列颜色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.series_color = Some(color);
        self
    }

    /// 设置背景与网格线颜色。
    #[inline(always)]
    pub const fn background_and_grid(mut self, bg: Color, grid: Color) -> Self {
        self.bg_color = Some(bg);
        self.grid_color = Some(grid);
        self
    }

    /// 设置水平参考网格线数量（默认 `3` 条）。
    #[inline(always)]
    pub const fn grid_lines(mut self, count: u8) -> Self {
        self.grid_lines = count;
        self
    }
}

crate::impl_base_styled_box!(Chart<'a>);

impl<'a> Widget<'a> for Chart<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let line_col = self.series_color.unwrap_or(fg);
        let grid_col = self.grid_color.unwrap_or_else(|| fg.scale_alpha(48));
        let bg_col = self.bg_color.unwrap_or(Color::TRANSPARENT);

        let w = StyleExt::resolve_width(self.style, self.width).max(24);
        let h = StyleExt::resolve_height(self.style, self.height).max(24);
        let data = self.data;
        let min_v = self.min_val as i32;
        let range_v = ((self.max_val as i32) - min_v).max(1);
        let kind = self.kind;
        let grids = self.grid_lines;

        let mut hash = self.id
            ^ ((w as u32) << 16)
            ^ (h as u32)
            ^ ((kind as u32) << 8)
            ^ (grids as u32);
        hash = hash.rotate_left(5) ^ line_col.0;
        hash = hash.rotate_left(5) ^ bg_col.0;
        hash = hash.rotate_left(5) ^ grid_col.0;
        hash = hash.rotate_left(5) ^ ((self.min_val as u16 as u32) << 16 | (self.max_val as u16 as u32));
        for &v in data {
            hash = hash.rotate_left(5) ^ (v as u16 as u32);
        }

        let elem = img_shader(cx, w, h, hash, move |painter: &mut dyn SurfacePainter| {
            let b = painter.bounds();
            let c = painter.clip();
            if c.is_empty() || b.w <= 4 || b.h <= 4 {
                return;
            }

            let draw_bg = bg_col.a() > 0;
            for py in c.y..c.bottom() {
                let rel_y = py - b.y;
                let mut is_grid = false;
                if grids > 0 && kind != ChartKind::Pie {
                    for g in 1..=grids {
                        let gy = ((b.h as i32) * (g as i32)) / ((grids as i32) + 1);
                        if (rel_y as i32) == gy {
                            is_grid = true;
                            break;
                        }
                    }
                }
                for px in c.x..c.right() {
                    if is_grid && ((px - b.x) & 1) == 0 {
                        painter.blend_pixel(px, py, grid_col, 180);
                    } else if draw_bg {
                        painter.blend_pixel(px, py, bg_col, bg_col.a());
                    }
                }
            }

            let pad = 4i32;
            let plot_w = ((b.w as i32) - pad * 2).max(1);
            let plot_h = ((b.h as i32) - pad * 2).max(1);

            match kind {
                ChartKind::Bar => {
                    if data.is_empty() {
                        return;
                    }
                    let n = data.len() as i32;
                    let slot_w = (plot_w / n).max(2);
                    let bar_w = (slot_w - 2).max(1);
                    for (i, &val) in data.iter().enumerate() {
                        let norm = (((val as i32) - min_v).clamp(0, range_v) * plot_h) / range_v;
                        let bx0 = (b.x as i32) + pad + (i as i32) * slot_w;
                        let bx1 = bx0 + bar_w;
                        let by1 = (b.bottom() as i32) - pad;
                        let by0 = (by1 - norm).min(by1 - 1);

                        let x_start = (bx0 as i16).max(c.x);
                        let x_end = (bx1 as i16).min(c.right());
                        let y_start = (by0 as i16).max(c.y);
                        let y_end = (by1 as i16).min(c.bottom());
                        for py in y_start..y_end {
                            for px in x_start..x_end {
                                painter.blend_pixel(px, py, line_col, 255);
                            }
                        }
                    }
                }
                ChartKind::Line | ChartKind::Area => {
                    if data.len() < 2 {
                        return;
                    }
                    let last_idx = (data.len() - 1) as i32;
                    let bottom_y = (b.bottom() as i32) - pad;
                    for seg in 0..(data.len() - 1) {
                        let v0 = ((data[seg] as i32) - min_v).clamp(0, range_v);
                        let v1 = ((data[seg + 1] as i32) - min_v).clamp(0, range_v);

                        let x0 = (b.x as i32) + pad + ((seg as i32) * plot_w) / last_idx;
                        let x1 = (b.x as i32) + pad + (((seg + 1) as i32) * plot_w) / last_idx;
                        let y0 = bottom_y - (v0 * plot_h) / range_v;
                        let y1 = bottom_y - (v1 * plot_h) / range_v;

                        if kind == ChartKind::Area && x1 > x0 {
                            let fill_x0 = (x0 as i16).max(c.x);
                            let fill_x1 = (x1 as i16).min(c.right());
                            for px in fill_x0..fill_x1 {
                                let t = (px as i32) - x0;
                                let top_y = y0 + ((y1 - y0) * t) / (x1 - x0);
                                let fy0 = (top_y as i16).max(c.y);
                                let fy1 = (bottom_y as i16).min(c.bottom());
                                for py in fy0..fy1 {
                                    painter.blend_pixel(px, py, line_col, 72);
                                }
                            }
                        }

                        let min_x = (x0.min(x1) - 3).max(c.x as i32) as i16;
                        let max_x = (x0.max(x1) + 4).min(c.right() as i32) as i16;
                        let min_y = (y0.min(y1) - 3).max(c.y as i32) as i16;
                        let max_y = (y0.max(y1) + 4).min(c.bottom() as i32) as i16;

                        for py in min_y..max_y {
                            for px in min_x..max_x {
                                let d16 = dist_to_segment(
                                    (px as i32) * 16,
                                    (py as i32) * 16,
                                    x0 * 16,
                                    y0 * 16,
                                    x1 * 16,
                                    y1 * 16,
                                );
                                if d16 < 28 {
                                    let alpha = if d16 <= 12 {
                                        255u8
                                    } else {
                                        (((28 - d16) * 255) / 16) as u8
                                    };
                                    painter.blend_pixel(px, py, line_col, alpha);
                                }
                            }
                        }
                    }
                }
                ChartKind::Pie => {
                    if data.is_empty() {
                        return;
                    }
                    let cx_px = (b.x as i32) + (b.w as i32) / 2;
                    let cy_px = (b.y as i32) + (b.h as i32) / 2;
                    let radius = (plot_w.min(plot_h) / 2).max(4);
                    let r_sq = radius * radius;
                    let inner_r_sq = (radius - 1).max(0) * (radius - 1).max(0);
                    let palette = [
                        line_col,
                        Color::rgb(16, 185, 129),
                        Color::rgb(245, 158, 11),
                        Color::rgb(239, 68, 68),
                        Color::rgb(168, 85, 247),
                        Color::rgb(14, 165, 233),
                        Color::rgb(236, 72, 153),
                        Color::rgb(132, 204, 22),
                    ];
                    let total: i64 = data.iter().map(|&v| (v as i64).max(0)).sum();

                    for py in c.y..c.bottom() {
                        let dy = (py as i32) - cy_px;
                        for px in c.x..c.right() {
                            let dx = (px as i32) - cx_px;
                            let d2 = dx * dx + dy * dy;
                            if d2 <= r_sq {
                                let ux = dx.abs();
                                let uy = dy.abs();
                                let angle_1024 = if ux == 0 && uy == 0 {
                                    0i32
                                } else if dx >= 0 && dy < 0 {
                                    if uy >= ux {
                                        (128 * ux) / uy.max(1)
                                    } else {
                                        256 - (128 * uy) / ux
                                    }
                                } else if dx > 0 && dy >= 0 {
                                    if ux >= uy {
                                        256 + (128 * uy) / ux.max(1)
                                    } else {
                                        512 - (128 * ux) / uy
                                    }
                                } else if dx <= 0 && dy > 0 {
                                    if uy >= ux {
                                        512 + (128 * ux) / uy.max(1)
                                    } else {
                                        768 - (128 * uy) / ux
                                    }
                                } else if ux >= uy {
                                    768 + (128 * uy) / ux.max(1)
                                } else {
                                    1024 - (128 * ux) / uy
                                }
                                .clamp(0, 1023);

                                let slice_idx = if total > 0 {
                                    let target = ((angle_1024 as i64) * total) / 1024;
                                    let mut acc = 0i64;
                                    let mut chosen = data.len() - 1;
                                    for (idx, &v) in data.iter().enumerate() {
                                        acc += (v as i64).max(0);
                                        if target < acc {
                                            chosen = idx;
                                            break;
                                        }
                                    }
                                    chosen
                                } else {
                                    ((angle_1024 as usize) * data.len()) / 1024
                                };

                                let col = palette[slice_idx % palette.len()];
                                let alpha = if d2 <= inner_r_sq {
                                    255u8
                                } else {
                                    let span = (r_sq - inner_r_sq).max(1);
                                    (((r_sq - d2) * 255) / span).clamp(0, 255) as u8
                                };
                                painter.blend_pixel(px, py, col, alpha);
                            }
                        }
                    }
                }
                ChartKind::Candlestick => {
                    let count = data.len() / 4;
                    if count == 0 {
                        return;
                    }
                    let slot_w = (plot_w / (count as i32)).max(4);
                    let body_w = (slot_w - 2).max(2);
                    let bottom_y = (b.bottom() as i32) - pad;
                    let bull_col = Color::rgb(34, 197, 94);
                    let bear_col = Color::rgb(239, 68, 68);

                    for i in 0..count {
                        let bar = CandlestickBar::new(
                            data[i * 4],
                            data[i * 4 + 1],
                            data[i * 4 + 2],
                            data[i * 4 + 3],
                        );
                        let col = if bar.close >= bar.open {
                            bull_col
                        } else {
                            bear_col
                        };
                        let map_y = |v: i16| -> i32 {
                            let clamped = ((v as i32) - min_v).clamp(0, range_v);
                            bottom_y - (clamped * plot_h) / range_v
                        };
                        let y_high = map_y(bar.high);
                        let y_low = map_y(bar.low);
                        let y_open = map_y(bar.open);
                        let y_close = map_y(bar.close);

                        let bx0 = (b.x as i32) + pad + (i as i32) * slot_w;
                        let wick_x = (bx0 + body_w / 2) as i16;
                        if wick_x >= c.x && wick_x < c.right() {
                            let wy0 = (y_high.min(y_low) as i16).max(c.y);
                            let wy1 = (y_high.max(y_low) as i16).min(c.bottom());
                            for py in wy0..wy1 {
                                painter.blend_pixel(wick_x, py, col, 255);
                            }
                        }

                        let by0 = (y_open.min(y_close) as i16).max(c.y);
                        let by1 = ((y_open.max(y_close) + 1) as i16).min(c.bottom());
                        let x0 = (bx0 as i16).max(c.x);
                        let x1 = ((bx0 + body_w) as i16).min(c.right());
                        for py in by0..by1 {
                            for px in x0..x1 {
                                painter.blend_pixel(px, py, col, 255);
                            }
                        }
                    }
                }
            }
        })
        .into_element(cx);
        self.style_ext.wrap_element(cx, elem, self.style)
    }
}
