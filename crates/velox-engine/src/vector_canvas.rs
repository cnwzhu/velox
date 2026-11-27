//! 原生 2D 矢量画布原语（`VectorCanvas` / `Polyline` / `Polygon` / `Circle` / `Sector`）。
//!
//! 面向实时波形图、折线图、仪表盘、雷达/海图矢量轨迹等高频动态矢量绘制场景：
//! - 100% 无堆兼容，数据全部驻留当前帧 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::Vector`；
//! - 彻底规避每帧拼接 SVG 字符串的开销，软件端与 GPU 端均提供原生抗锯齿绘制。

use crate::display::PixelFormat;
use crate::element::{Element, IntoElement};
use crate::pod::FrameScope;
use crate::solver::Region;
use crate::style::Color;
use crate::svg::math::{atan2_f32, floor_f32, sqrt_f32};

/// 二维局部浮点坐标点（相对于 `VectorCanvas` 盒子左上角，单位逻辑像素）。
#[derive(Copy, Clone, Debug, Default)]
pub struct VecPoint {
    pub x: f32,
    pub y: f32,
}

impl VecPoint {
    #[inline(always)]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[inline(always)]
fn canon_f32(v: f32) -> u32 {
    if v == 0.0 { 0 } else { v.to_bits() }
}

impl PartialEq for VecPoint {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        canon_f32(self.x) == canon_f32(other.x) && canon_f32(self.y) == canon_f32(other.y)
    }
}

impl Eq for VecPoint {}

/// 单个矢量画布绘制图元。
#[derive(Copy, Clone, Debug)]
pub enum VectorPrimitive<'a> {
    /// 抗锯齿折线 / 虚线 / 闭合折线环（支持自定义线宽、圆头与虚线样式）。
    Polyline {
        points: &'a [VecPoint],
        color: Color,
        width: f32,
        dash_len: f32,
        gap_len: f32,
        closed: bool,
    },
    /// 任意多边形填充与可选描边（扫描线奇偶规则抗锯齿填充）。
    Polygon {
        points: &'a [VecPoint],
        fill: Color,
        stroke: Color,
        stroke_width: f32,
    },
    /// 解析 SDF 抗锯齿实心圆 / 圆环。
    Circle {
        cx: f32,
        cy: f32,
        radius: f32,
        fill: Color,
        stroke: Color,
        stroke_width: f32,
    },
    /// 解析 SDF 扇形 / 环扇（角度单位度，0° 为 12 点钟方向顺时针递增）。
    Sector {
        cx: f32,
        cy: f32,
        inner_r: f32,
        outer_r: f32,
        start_deg: f32,
        sweep_deg: f32,
        fill: Color,
    },
    /// 光滑三次贝塞尔曲线（24 段解析细分抗锯齿描边 + 虚线支持）。
    CubicBezier {
        p0: VecPoint,
        p1: VecPoint,
        p2: VecPoint,
        p3: VecPoint,
        color: Color,
        width: f32,
        dash_len: f32,
        gap_len: f32,
    },
    /// 任意坐标系完整 W3C SVG Path `d` 矢量路径（突破 24×24 限制，支持 20 条贝塞尔/椭圆弧指令、`NonZero`/`EvenOdd` 4× AA 填充与任意宽度圆/斜/平头描边）。
    SvgPath {
        d: &'a str,
        fill: Color,
        fill_rule: crate::svg::SvgFillRule,
        stroke: Color,
        stroke_width: f32,
        line_cap: crate::svg::SvgLineCap,
        line_join: crate::svg::SvgLineJoin,
    },
}

impl<'a> PartialEq for VectorPrimitive<'a> {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (
                Self::Polyline {
                    points: p1,
                    color: c1,
                    width: w1,
                    dash_len: d1,
                    gap_len: g1,
                    closed: cl1,
                },
                Self::Polyline {
                    points: p2,
                    color: c2,
                    width: w2,
                    dash_len: d2,
                    gap_len: g2,
                    closed: cl2,
                },
            ) => {
                p1 == p2
                    && c1 == c2
                    && canon_f32(w1) == canon_f32(w2)
                    && canon_f32(d1) == canon_f32(d2)
                    && canon_f32(g1) == canon_f32(g2)
                    && cl1 == cl2
            }
            (
                Self::Polygon {
                    points: p1,
                    fill: f1,
                    stroke: s1,
                    stroke_width: sw1,
                },
                Self::Polygon {
                    points: p2,
                    fill: f2,
                    stroke: s2,
                    stroke_width: sw2,
                },
            ) => p1 == p2 && f1 == f2 && s1 == s2 && canon_f32(sw1) == canon_f32(sw2),
            (
                Self::Circle {
                    cx: x1,
                    cy: y1,
                    radius: r1,
                    fill: f1,
                    stroke: s1,
                    stroke_width: sw1,
                },
                Self::Circle {
                    cx: x2,
                    cy: y2,
                    radius: r2,
                    fill: f2,
                    stroke: s2,
                    stroke_width: sw2,
                },
            ) => {
                canon_f32(x1) == canon_f32(x2)
                    && canon_f32(y1) == canon_f32(y2)
                    && canon_f32(r1) == canon_f32(r2)
                    && f1 == f2
                    && s1 == s2
                    && canon_f32(sw1) == canon_f32(sw2)
            }
            (
                Self::Sector {
                    cx: x1,
                    cy: y1,
                    inner_r: ir1,
                    outer_r: or1,
                    start_deg: sd1,
                    sweep_deg: sw1,
                    fill: f1,
                },
                Self::Sector {
                    cx: x2,
                    cy: y2,
                    inner_r: ir2,
                    outer_r: or2,
                    start_deg: sd2,
                    sweep_deg: sw2,
                    fill: f2,
                },
            ) => {
                canon_f32(x1) == canon_f32(x2)
                    && canon_f32(y1) == canon_f32(y2)
                    && canon_f32(ir1) == canon_f32(ir2)
                    && canon_f32(or1) == canon_f32(or2)
                    && canon_f32(sd1) == canon_f32(sd2)
                    && canon_f32(sw1) == canon_f32(sw2)
                    && f1 == f2
            }
            (
                Self::CubicBezier {
                    p0: a0,
                    p1: a1,
                    p2: a2,
                    p3: a3,
                    color: c1,
                    width: w1,
                    dash_len: d1,
                    gap_len: g1,
                },
                Self::CubicBezier {
                    p0: b0,
                    p1: b1,
                    p2: b2,
                    p3: b3,
                    color: c2,
                    width: w2,
                    dash_len: d2,
                    gap_len: g2,
                },
            ) => {
                a0 == b0
                    && a1 == b1
                    && a2 == b2
                    && a3 == b3
                    && c1 == c2
                    && canon_f32(w1) == canon_f32(w2)
                    && canon_f32(d1) == canon_f32(d2)
                    && canon_f32(g1) == canon_f32(g2)
            }
            (
                Self::SvgPath {
                    d: d1,
                    fill: f1,
                    fill_rule: r1,
                    stroke: s1,
                    stroke_width: sw1,
                    line_cap: lc1,
                    line_join: lj1,
                },
                Self::SvgPath {
                    d: d2,
                    fill: f2,
                    fill_rule: r2,
                    stroke: s2,
                    stroke_width: sw2,
                    line_cap: lc2,
                    line_join: lj2,
                },
            ) => {
                d1 == d2
                    && f1 == f2
                    && r1 == r2
                    && s1 == s2
                    && canon_f32(sw1) == canon_f32(sw2)
                    && lc1 == lc2
                    && lj1 == lj2
            }
            _ => false,
        }
    }
}

impl<'a> Eq for VectorPrimitive<'a> {}

/// 矢量画布绘制载荷（分配于 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::Vector`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct VectorDrawSpec<'a> {
    pub primitives: &'a [VectorPrimitive<'a>],
    pub width: i16,
    pub height: i16,
    pub opacity: u8,
}

impl<'a> VectorDrawSpec<'a> {
    pub fn mix_hash(&self, h: &mut u32) {
        #[inline(always)]
        fn mix_u8(h: &mut u32, b: u8) {
            *h ^= b as u32;
            *h = h.wrapping_mul(0x0100_0193);
        }
        #[inline(always)]
        fn mix_u32(h: &mut u32, v: u32) {
            for b in v.to_le_bytes() {
                mix_u8(h, b);
            }
        }
        mix_u32(
            h,
            ((self.width as u16 as u32) << 16) | (self.height as u16 as u32),
        );
        mix_u8(h, self.opacity);
        for p in self.primitives {
            match *p {
                VectorPrimitive::Polyline {
                    points,
                    color,
                    width,
                    dash_len,
                    gap_len,
                    closed,
                } => {
                    mix_u8(h, 1);
                    mix_u32(h, color.0);
                    mix_u32(h, canon_f32(width));
                    mix_u32(h, canon_f32(dash_len));
                    mix_u32(h, canon_f32(gap_len));
                    mix_u8(h, if closed { 1 } else { 0 });
                    for pt in points {
                        mix_u32(h, canon_f32(pt.x));
                        mix_u32(h, canon_f32(pt.y));
                    }
                }
                VectorPrimitive::Polygon {
                    points,
                    fill,
                    stroke,
                    stroke_width,
                } => {
                    mix_u8(h, 2);
                    mix_u32(h, fill.0);
                    mix_u32(h, stroke.0);
                    mix_u32(h, canon_f32(stroke_width));
                    for pt in points {
                        mix_u32(h, canon_f32(pt.x));
                        mix_u32(h, canon_f32(pt.y));
                    }
                }
                VectorPrimitive::Circle {
                    cx,
                    cy,
                    radius,
                    fill,
                    stroke,
                    stroke_width,
                } => {
                    mix_u8(h, 3);
                    mix_u32(h, canon_f32(cx));
                    mix_u32(h, canon_f32(cy));
                    mix_u32(h, canon_f32(radius));
                    mix_u32(h, fill.0);
                    mix_u32(h, stroke.0);
                    mix_u32(h, canon_f32(stroke_width));
                }
                VectorPrimitive::Sector {
                    cx,
                    cy,
                    inner_r,
                    outer_r,
                    start_deg,
                    sweep_deg,
                    fill,
                } => {
                    mix_u8(h, 4);
                    mix_u32(h, canon_f32(cx));
                    mix_u32(h, canon_f32(cy));
                    mix_u32(h, canon_f32(inner_r));
                    mix_u32(h, canon_f32(outer_r));
                    mix_u32(h, canon_f32(start_deg));
                    mix_u32(h, canon_f32(sweep_deg));
                    mix_u32(h, fill.0);
                }
                VectorPrimitive::CubicBezier {
                    p0,
                    p1,
                    p2,
                    p3,
                    color,
                    width,
                    dash_len,
                    gap_len,
                } => {
                    mix_u8(h, 5);
                    for pt in [p0, p1, p2, p3] {
                        mix_u32(h, canon_f32(pt.x));
                        mix_u32(h, canon_f32(pt.y));
                    }
                    mix_u32(h, color.0);
                    mix_u32(h, canon_f32(width));
                    mix_u32(h, canon_f32(dash_len));
                    mix_u32(h, canon_f32(gap_len));
                }
                VectorPrimitive::SvgPath {
                    d,
                    fill,
                    fill_rule,
                    stroke,
                    stroke_width,
                    line_cap,
                    line_join,
                } => {
                    mix_u8(h, 6);
                    mix_u32(h, fill.0);
                    mix_u8(h, fill_rule as u8);
                    mix_u32(h, stroke.0);
                    mix_u32(h, canon_f32(stroke_width));
                    mix_u8(h, line_cap as u8);
                    mix_u8(h, line_join as u8);
                    for &b in d.as_bytes() {
                        mix_u8(h, b);
                    }
                }
            }
        }
    }
}

/// 声明式原生 2D 矢量画布元素。
#[derive(Copy, Clone, Debug)]
pub struct VectorCanvas<'a> {
    pub spec: VectorDrawSpec<'a>,
}

/// 构造一个 2D 矢量画布元素（图元切片自动分配于当前帧 `ScratchArena`）。
#[inline]
pub fn vector_canvas<'a>(
    cx: &'a FrameScope,
    width: i16,
    height: i16,
    primitives: &[VectorPrimitive<'a>],
) -> VectorCanvas<'a> {
    let alloc = cx.arena().alloc_iter(primitives.iter().copied());
    VectorCanvas {
        spec: VectorDrawSpec {
            primitives: alloc,
            width,
            height,
            opacity: 255,
        },
    }
}

impl<'a> VectorCanvas<'a> {
    #[inline(always)]
    pub const fn opacity(mut self, opacity: u8) -> Self {
        self.spec.opacity = opacity;
        self
    }
}

impl<'a> IntoElement<'a> for VectorCanvas<'a> {
    #[inline]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        let spec_ref = cx.arena().alloc_temp(self.spec);
        Element::Vector(spec_ref)
    }
}

// ============================================================================
// 跨平台抗锯齿软件光栅器（纯数学 SDF + 奇偶填充，100% 无堆）
// ============================================================================

#[inline]
fn blend_cov_pixel<P: PixelFormat>(target: &mut [P], idx: usize, color: Color, cov: f32) {
    let a = ((color.a() as f32) * cov.clamp(0.0, 1.0) + 0.5) as u8;
    if a == 0 {
        return;
    }
    if let Some(dst) = target.get_mut(idx) {
        let blended = crate::sdf::blend_color(dst.to_color(), color, a);
        *dst = P::from_color(blended);
    }
}

#[inline]
fn dist_point_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> (f32, f32) {
    let vx = bx - ax;
    let vy = by - ay;
    let len2 = vx * vx + vy * vy;
    if len2 <= 1e-6 {
        let dx = px - ax;
        let dy = py - ay;
        return (sqrt_f32(dx * dx + dy * dy), 0.0);
    }
    let t = (((px - ax) * vx + (py - ay) * vy) / len2).clamp(0.0, 1.0);
    let dx = px - (ax + vx * t);
    let dy = py - (ay + vy * t);
    (sqrt_f32(dx * dx + dy * dy), t * sqrt_f32(len2))
}

#[allow(clippy::too_many_arguments)]
fn rasterize_polyline<P: PixelFormat>(
    points: &[VecPoint],
    color: Color,
    width: f32,
    dash_len: f32,
    gap_len: f32,
    closed: bool,
    bounds: Region,
    vis: Region,
    sf: f32,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if points.len() < 2 || color.a() == 0 || width <= 0.0 {
        return;
    }
    let half_w = (width * sf * 0.5).max(0.5);
    let bx0 = bounds.x as f32;
    let by0 = bounds.y as f32;
    let seg_count = if closed { points.len() } else { points.len() - 1 };
    let dash_period = if dash_len > 0.0 && gap_len > 0.0 {
        (dash_len + gap_len) * sf
    } else {
        0.0
    };
    let dash_on = dash_len * sf;
    let mut accum_len = 0.0f32;

    for s in 0..seg_count {
        let p0 = points[s];
        let p1 = points[(s + 1) % points.len()];
        let ax = bx0 + p0.x * sf;
        let ay = by0 + p0.y * sf;
        let bx = bx0 + p1.x * sf;
        let by = by0 + p1.y * sf;
        let seg_len = sqrt_f32((bx - ax) * (bx - ax) + (by - ay) * (by - ay));

        let pad = half_w + 1.5;
        let min_x = (floor_f32(ax.min(bx) - pad) as i16).max(vis.x);
        let max_x = ((ax.max(bx) + pad) as i16 + 1).min(vis.right());
        let min_y = (floor_f32(ay.min(by) - pad) as i16).max(vis.y);
        let max_y = ((ay.max(by) + pad) as i16 + 1).min(vis.bottom());

        for py in min_y..max_y {
            let row_off = ((py - origin_y) as usize) * stride;
            let cy = py as f32 + 0.5;
            for px in min_x..max_x {
                let cx = px as f32 + 0.5;
                let (d, along) = dist_point_to_segment(cx, cy, ax, ay, bx, by);
                let cov = (half_w + 0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                if dash_period > 0.0 {
                    let phase = (accum_len + along) % dash_period;
                    if phase > dash_on {
                        continue;
                    }
                }
                blend_cov_pixel(target, row_off + ((px - origin_x) as usize), color, cov);
            }
        }
        accum_len += seg_len;
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_polygon<P: PixelFormat>(
    points: &[VecPoint],
    fill: Color,
    stroke: Color,
    stroke_width: f32,
    bounds: Region,
    vis: Region,
    sf: f32,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if points.len() < 3 {
        return;
    }
    let bx0 = bounds.x as f32;
    let by0 = bounds.y as f32;
    if fill.a() > 0 {
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for pt in points {
            let x = bx0 + pt.x * sf;
            let y = by0 + pt.y * sf;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        let x0 = (floor_f32(min_x) as i16).max(vis.x);
        let x1 = (max_x as i16 + 1).min(vis.right());
        let y0 = (floor_f32(min_y) as i16).max(vis.y);
        let y1 = (max_y as i16 + 1).min(vis.bottom());

        for py in y0..y1 {
            let row_off = ((py - origin_y) as usize) * stride;
            let cy = py as f32 + 0.5;
            for px in x0..x1 {
                let cx = px as f32 + 0.5;
                // 奇偶射线法
                let mut inside = false;
                let mut j = points.len() - 1;
                for i in 0..points.len() {
                    let xi = bx0 + points[i].x * sf;
                    let yi = by0 + points[i].y * sf;
                    let xj = bx0 + points[j].x * sf;
                    let yj = by0 + points[j].y * sf;
                    let cond = ((yi > cy) != (yj > cy))
                        && (cx < (xj - xi) * (cy - yi) / ((yj - yi) + 1e-7) + xi);
                    if cond {
                        inside = !inside;
                    }
                    j = i;
                }
                if inside {
                    blend_cov_pixel(target, row_off + ((px - origin_x) as usize), fill, 1.0);
                }
            }
        }
    }
    if stroke.a() > 0 && stroke_width > 0.0 {
        rasterize_polyline(
            points,
            stroke,
            stroke_width,
            0.0,
            0.0,
            true,
            bounds,
            vis,
            sf,
            origin_x,
            origin_y,
            stride,
            target,
        );
    }
}

/// 将 [`VectorDrawSpec`] 光栅化至目标像素缓冲（支持任意 HiDPI 缩放与裁剪框）。
#[allow(clippy::too_many_arguments)]
pub fn rasterize_vector_spec<P: PixelFormat>(
    spec: &VectorDrawSpec<'_>,
    bounds: Region,
    clip: Region,
    sf: f32,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let vis = bounds.intersect(clip);
    if vis.is_empty() || spec.opacity == 0 {
        return;
    }
    let scale_c = |c: Color| c.scale_alpha(spec.opacity);
    let bx0 = bounds.x as f32;
    let by0 = bounds.y as f32;

    for prim in spec.primitives {
        match *prim {
            VectorPrimitive::Polyline {
                points,
                color,
                width,
                dash_len,
                gap_len,
                closed,
            } => rasterize_polyline(
                points,
                scale_c(color),
                width,
                dash_len,
                gap_len,
                closed,
                bounds,
                vis,
                sf,
                origin_x,
                origin_y,
                stride,
                target,
            ),
            VectorPrimitive::Polygon {
                points,
                fill,
                stroke,
                stroke_width,
            } => rasterize_polygon(
                points,
                scale_c(fill),
                scale_c(stroke),
                stroke_width,
                bounds,
                vis,
                sf,
                origin_x,
                origin_y,
                stride,
                target,
            ),
            VectorPrimitive::Circle {
                cx,
                cy,
                radius,
                fill,
                stroke,
                stroke_width,
            } => {
                let ccx = bx0 + cx * sf;
                let ccy = by0 + cy * sf;
                let r = radius * sf;
                let sw = stroke_width * sf;
                let outer = r + sw * 0.5 + 1.5;
                let x0 = (floor_f32(ccx - outer) as i16).max(vis.x);
                let x1 = ((ccx + outer) as i16 + 1).min(vis.right());
                let y0 = (floor_f32(ccy - outer) as i16).max(vis.y);
                let y1 = ((ccy + outer) as i16 + 1).min(vis.bottom());
                let fcol = scale_c(fill);
                let scol = scale_c(stroke);

                for py in y0..y1 {
                    let row_off = ((py - origin_y) as usize) * stride;
                    let dy = py as f32 + 0.5 - ccy;
                    for px in x0..x1 {
                        let dx = px as f32 + 0.5 - ccx;
                        let dist = sqrt_f32(dx * dx + dy * dy);
                        let idx = row_off + ((px - origin_x) as usize);
                        if fcol.a() > 0 {
                            let cov = (r + 0.5 - dist).clamp(0.0, 1.0);
                            if cov > 0.0 {
                                blend_cov_pixel(target, idx, fcol, cov);
                            }
                        }
                        if scol.a() > 0 && sw > 0.0 {
                            let sd = crate::svg::math::abs_f32(dist - r);
                            let cov = (sw * 0.5 + 0.5 - sd).clamp(0.0, 1.0);
                            if cov > 0.0 {
                                blend_cov_pixel(target, idx, scol, cov);
                            }
                        }
                    }
                }
            }
            VectorPrimitive::Sector {
                cx,
                cy,
                inner_r,
                outer_r,
                start_deg,
                sweep_deg,
                fill,
            } => {
                let fcol = scale_c(fill);
                if fcol.a() == 0 || outer_r <= inner_r || sweep_deg <= 0.0 {
                    continue;
                }
                let ccx = bx0 + cx * sf;
                let ccy = by0 + cy * sf;
                let ir = inner_r.max(0.0) * sf;
                let or = outer_r * sf;
                let pad = or + 1.5;
                let x0 = (floor_f32(ccx - pad) as i16).max(vis.x);
                let x1 = ((ccx + pad) as i16 + 1).min(vis.right());
                let y0 = (floor_f32(ccy - pad) as i16).max(vis.y);
                let y1 = ((ccy + pad) as i16 + 1).min(vis.bottom());
                let start_n = ((start_deg % 360.0) + 360.0) % 360.0;
                let sweep_c = sweep_deg.clamp(0.0, 360.0);

                for py in y0..y1 {
                    let row_off = ((py - origin_y) as usize) * stride;
                    let dy = py as f32 + 0.5 - ccy;
                    for px in x0..x1 {
                        let dx = px as f32 + 0.5 - ccx;
                        let dist = sqrt_f32(dx * dx + dy * dy);
                        let cov_r = (or + 0.5 - dist).min(dist - ir + 0.5).clamp(0.0, 1.0);
                        if cov_r <= 0.0 {
                            continue;
                        }
                        if sweep_c < 359.9 {
                            // 0° 为 12 点钟方向顺时针
                            let mut ang = atan2_f32(dx, -dy) * (180.0 / core::f32::consts::PI);
                            if ang < 0.0 {
                                ang += 360.0;
                            }
                            let rel = ((ang - start_n) + 360.0) % 360.0;
                            if rel > sweep_c {
                                continue;
                            }
                        }
                        blend_cov_pixel(
                            target,
                            row_off + ((px - origin_x) as usize),
                            fcol,
                            cov_r,
                        );
                    }
                }
            }
            VectorPrimitive::CubicBezier {
                p0,
                p1,
                p2,
                p3,
                color,
                width,
                dash_len,
                gap_len,
            } => {
                let col = scale_c(color);
                if col.a() == 0 || width <= 0.0 {
                    continue;
                }
                let mut pts = [VecPoint::default(); 25];
                for i in 0..=24 {
                    let t = i as f32 / 24.0;
                    let u = 1.0 - t;
                    let uu = u * u;
                    let tt = t * t;
                    let uuu = uu * u;
                    let ttt = tt * t;
                    pts[i] = VecPoint::new(
                        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
                        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
                    );
                }
                rasterize_polyline(
                    &pts,
                    col,
                    width,
                    dash_len,
                    gap_len,
                    false,
                    bounds,
                    vis,
                    sf,
                    origin_x,
                    origin_y,
                    stride,
                    target,
                );
            }
            VectorPrimitive::SvgPath {
                d,
                fill,
                fill_rule,
                stroke,
                stroke_width,
                line_cap,
                line_join,
            } => {
                if d.is_empty() {
                    continue;
                }
                let xf = crate::svg::Transform2D::new(sf, 0.0, 0.0, sf, bx0, by0);
                let mut path = crate::svg::FlattenedPath::new();
                path.parse_svg_path_d(d, xf);
                if path.is_empty() {
                    continue;
                }
                let fcol = scale_c(fill);
                if fcol.a() > 0 {
                    crate::svg::fill_path(
                        &path, fcol, fill_rule, vis, origin_x, origin_y, stride, target,
                    );
                }
                let scol = scale_c(stroke);
                if scol.a() > 0 && stroke_width > 0.0 {
                    crate::svg::stroke_path(
                        &path,
                        scol,
                        stroke_width * sf,
                        line_cap,
                        line_join,
                        4.0,
                        vis,
                        origin_x,
                        origin_y,
                        stride,
                        target,
                    );
                }
            }
        }
    }
}
