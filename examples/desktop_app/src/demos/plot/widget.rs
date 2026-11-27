//! 声明式高频实时波形与金融 K 线组件（`RealtimePlot`）。

use velox_kit::prelude::*;

use super::downsample::lttb_downsample;
use super::state::RealtimePlotState;

/// 声明式高频实时波形与 K 线组件（`RealtimePlot`）。
pub struct RealtimePlot<'s, 'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub state: &'s RealtimePlotState,
    pub show_candles: bool,
    pub on_wheel: Option<&'a dyn Fn(WheelEvent, &mut ActionScope)>,
    pub on_drag: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    pub on_pointer_move: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
}

impl<'s, 'a> RealtimePlot<'s, 'a> {
    pub fn new(id: u32, width: i16, height: i16, state: &'s RealtimePlotState) -> Self {
        Self {
            id,
            width,
            height,
            state,
            show_candles: false,
            on_wheel: None,
            on_drag: None,
            on_pointer_move: None,
        }
    }

    #[inline]
    pub const fn show_candles(mut self, show: bool) -> Self {
        self.show_candles = show;
        self
    }

    #[inline]
    pub fn on_wheel(mut self, cb: &'a dyn Fn(WheelEvent, &mut ActionScope)) -> Self {
        self.on_wheel = Some(cb);
        self
    }

    #[inline]
    pub fn on_drag(mut self, cb: &'a dyn Fn(DragEvent, &mut ActionScope)) -> Self {
        self.on_drag = Some(cb);
        self
    }

    #[inline]
    pub fn on_pointer_move(mut self, cb: &'a dyn Fn(DragEvent, &mut ActionScope)) -> Self {
        self.on_pointer_move = Some(cb);
        self
    }
}

impl<'s, 'a> Widget<'a> for RealtimePlot<'s, 'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = *cx.theme();
        let arena = cx.arena();
        let pad_l = 12.0f32;
        let pad_r = 12.0f32;
        let pad_t = 14.0f32;
        let pad_b = 18.0f32;
        let pw = (self.width as f32 - pad_l - pad_r).max(16.0);
        let ph = (self.height as f32 - pad_t - pad_b).max(16.0);

        let mut prims: Vec<VectorPrimitive<'a>> = Vec::with_capacity(96);

        // 水平参考网格线
        for i in 0..=4 {
            let y = pad_t + ph * (i as f32 / 4.0);
            let pts = arena.alloc_iter([VecPoint::new(pad_l, y), VecPoint::new(pad_l + pw, y)]);
            prims.push(VectorPrimitive::Polyline {
                points: pts,
                color: theme.border.scale_alpha(95),
                width: 1.0,
                dash_len: 4.0,
                gap_len: 4.0,
                closed: false,
            });
        }

        let mut raw_buf = Vec::new();
        let mut lttb_buf = Vec::new();
        let target_buckets = ((pw * 0.75) as usize).clamp(32, 320);

        if self.show_candles && !self.state.candles.is_empty() {
            let n = self.state.candles.len();
            let i0 = ((self.state.x_window.0 * n as f64).floor() as usize).min(n - 1);
            let i1 = ((self.state.x_window.1 * n as f64).ceil() as usize).clamp(i0 + 1, n);
            let slice = &self.state.candles[i0..i1];
            let mut min_p = f64::MAX;
            let mut max_p = f64::MIN;
            for c in slice {
                min_p = min_p.min(c.low);
                max_p = max_p.max(c.high);
            }
            let span_p = (max_p - min_p).max(1e-3);
            let step_x = pw / slice.len().max(1) as f32;
            let body_w = (step_x * 0.62).clamp(3.0, 14.0);
            for (idx, c) in slice.iter().enumerate() {
                let x = pad_l + (idx as f32 + 0.5) * step_x;
                let map_y = |v: f64| pad_t + ph - ((v - min_p) / span_p) as f32 * ph;
                let up = c.close >= c.open;
                let col = if up {
                    Color::rgb(16, 185, 129)
                } else {
                    Color::rgb(239, 68, 68)
                };
                // 影线
                let wick = arena.alloc_iter([
                    VecPoint::new(x, map_y(c.high)),
                    VecPoint::new(x, map_y(c.low)),
                ]);
                prims.push(VectorPrimitive::Polyline {
                    points: wick,
                    color: col,
                    width: 1.4,
                    dash_len: 0.0,
                    gap_len: 0.0,
                    closed: false,
                });
                // 实体
                let y0 = map_y(c.open.max(c.close));
                let y1 = map_y(c.open.min(c.close)).max(y0 + 1.5);
                let rect = arena.alloc_iter([
                    VecPoint::new(x - body_w * 0.5, y0),
                    VecPoint::new(x + body_w * 0.5, y0),
                    VecPoint::new(x + body_w * 0.5, y1),
                    VecPoint::new(x - body_w * 0.5, y1),
                ]);
                prims.push(VectorPrimitive::Polygon {
                    points: rect,
                    fill: col,
                    stroke: col,
                    stroke_width: 0.0,
                });
            }
        } else {
            // 多通道 LTTB 保形降采样波形曲线
            for s in &self.state.series {
                s.copy_ordered_into(&mut raw_buf);
                if raw_buf.len() < 2 {
                    continue;
                }
                let n = raw_buf.len();
                let i0 = ((self.state.x_window.0 * n as f64).floor() as usize).min(n - 2);
                let i1 = ((self.state.x_window.1 * n as f64).ceil() as usize).clamp(i0 + 2, n);
                let window_slice = &raw_buf[i0..i1];
                lttb_downsample(window_slice, target_buckets, &mut lttb_buf);
                if lttb_buf.len() < 2 {
                    continue;
                }
                let x0 = lttb_buf.first().map(|p| p.0).unwrap_or(0.0);
                let x1 = lttb_buf.last().map(|p| p.0).unwrap_or(1.0);
                let dx = (x1 - x0).max(1e-6);
                let mut ymin = f64::MAX;
                let mut ymax = f64::MIN;
                for &(_, y) in &lttb_buf {
                    ymin = ymin.min(y);
                    ymax = ymax.max(y);
                }
                let dy = (ymax - ymin).max(1e-3);
                let pts = arena.alloc_iter(lttb_buf.iter().map(|&(px, py)| {
                    let sx = pad_l + ((px - x0) / dx) as f32 * pw;
                    let sy = pad_t + ph - ((py - ymin) / dy) as f32 * ph;
                    VecPoint::new(sx, sy)
                }));
                prims.push(VectorPrimitive::Polyline {
                    points: pts,
                    color: s.color,
                    width: 1.8,
                    dash_len: 0.0,
                    gap_len: 0.0,
                    closed: false,
                });
            }
        }

        // 十字准星悬停线
        if let Some((mx, my)) = self.state.crosshair_local {
            let x = (mx as f32).clamp(pad_l, pad_l + pw);
            let y = (my as f32).clamp(pad_t, pad_t + ph);
            let vline = arena.alloc_iter([VecPoint::new(x, pad_t), VecPoint::new(x, pad_t + ph)]);
            let hline = arena.alloc_iter([VecPoint::new(pad_l, y), VecPoint::new(pad_l + pw, y)]);
            let cross_col = theme.primary.scale_alpha(150);
            prims.push(VectorPrimitive::Polyline {
                points: vline,
                color: cross_col,
                width: 1.0,
                dash_len: 3.0,
                gap_len: 3.0,
                closed: false,
            });
            prims.push(VectorPrimitive::Polyline {
                points: hline,
                color: cross_col,
                width: 1.0,
                dash_len: 3.0,
                gap_len: 3.0,
                closed: false,
            });
        }

        let mut root = div()
            .id(self.id)
            .w(self.width)
            .h(self.height)
            .rounded(8)
            .bg(theme.card)
            .border(1, theme.border)
            .overflow_hidden()
            .cursor(CursorShape::Crosshair)
            .child(cx, vector_canvas(cx, self.width, self.height, &prims));

        if let Some(readout) = self.state.crosshair_readout(self.width, self.show_candles) {
            let txt = arena.alloc_str(&readout).as_str();
            root = root.child(
                cx,
                div()
                    .pos(12, 10)
                    .px(8)
                    .py(4)
                    .rounded(6)
                    .bg(theme.popover.scale_alpha(228))
                    .border(1, theme.primary.scale_alpha(150))
                    .child(cx, text(txt).size(11).color(theme.popover_foreground).nowrap()),
            );
        }

        if let Some(cb) = self.on_wheel {
            root = root.on_wheel(cx, cb);
        }
        if let Some(cb) = self.on_drag {
            root = root.on_drag(cx, cb);
        }
        if let Some(cb) = self.on_pointer_move {
            root = root.on_pointer_move(cx, cb);
        }
        root.into_element(cx)
    }
}
