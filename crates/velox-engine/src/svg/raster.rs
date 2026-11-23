//! 高精度抗锯齿矢量 SVG 扫描线填充器（4× 垂直 + 解析水平覆盖率）与逐行最大覆盖率 SDF 描边器。

use crate::display::PixelFormat;
use crate::solver::Region;
use crate::style::Color;

use super::math::{abs_f32, ceil_f32, floor_f32, round_f32, sqrt_f32};
use super::path::FlattenedPath;

use std::vec::Vec;


/// SVG 填充绕数规则（`fill-rule`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SvgFillRule {
    /// 非零绕数规则（默认）。
    #[default]
    NonZero,
    /// 奇偶规则（支持同向子路径镂空孔洞）。
    EvenOdd,
}

/// SVG 描边端点样式（`stroke-linecap`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SvgLineCap {
    /// 平头截断（默认）。
    #[default]
    Butt,
    /// 半圆端帽（Lucide 图标默认风格）。
    Round,
    /// 方形延伸端帽。
    Square,
}

/// SVG 描边拐角连接样式（`stroke-linejoin`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SvgLineJoin {
    /// 尖角连接（默认，受 `stroke-miterlimit` 限制）。
    #[default]
    Miter,
    /// 圆角连接（Lucide 图标默认风格）。
    Round,
    /// 斜切连接。
    Bevel,
}

#[derive(Copy, Clone)]
struct ScanEdge {
    y0: f32,
    y1: f32,
    x0: f32,
    dx_dy: f32,
    dir: i8,
}

#[derive(Copy, Clone)]
struct Crossing {
    x: f32,
    dir: i8,
}

/// 对已变换至物理像素空间的 `path` 执行 4× 垂直 + 解析水平覆盖率抗锯齿扫描线填充。
#[allow(clippy::too_many_arguments)]
pub fn fill_path<P: PixelFormat>(
    path: &FlattenedPath,
    color: Color,
    fill_rule: SvgFillRule,
    clip: Region,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if color.a() == 0 || clip.is_empty() || stride == 0 || path.is_empty() {
        return;
    }

    let mut edges: Vec<ScanEdge> = Vec::with_capacity(path.points().len());

    let pts = path.points();
    let mut global_min_y = f32::MAX;
    let mut global_max_y = f32::MIN;

    for sp in path.subpaths() {
        if sp.len < 2 {
            continue;
        }
        let end = (sp.start + sp.len).min(pts.len());
        if end <= sp.start + 1 {
            continue;
        }
        let sub = &pts[sp.start..end];
        let seg_count = sub.len();
        for i in 0..seg_count {
            let p0 = sub[i];
            let p1 = sub[(i + 1) % seg_count];
            let dy = p1.1 - p0.1;
            if abs_f32(dy) < 1e-6 {
                continue;
            }
            let edge = if dy > 0.0 {
                ScanEdge {
                    y0: p0.1,
                    y1: p1.1,
                    x0: p0.0,
                    dx_dy: (p1.0 - p0.0) / dy,
                    dir: 1,
                }
            } else {
                ScanEdge {
                    y0: p1.1,
                    y1: p0.1,
                    x0: p1.0,
                    dx_dy: (p0.0 - p1.0) / (-dy),
                    dir: -1,
                }
            };
            if edge.y0 < global_min_y {
                global_min_y = edge.y0;
            }
            if edge.y1 > global_max_y {
                global_max_y = edge.y1;
            }
            edges.push(edge);
        }
    }

    if edges.is_empty() {
        return;
    }

    let y_start = (floor_f32(global_min_y) as i32)
        .max(clip.y as i32)
        .min(clip.bottom() as i32);
    let y_end = (ceil_f32(global_max_y) as i32)
        .max(clip.y as i32)
        .min(clip.bottom() as i32);
    if y_start >= y_end {
        return;
    }

    let clip_x0 = clip.x as i32;
    let clip_x1 = clip.right() as i32;
    let clip_w = (clip_x1 - clip_x0).max(0) as usize;
    if clip_w == 0 {
        return;
    }

    const SUB_OFFSETS: [f32; 4] = [0.125, 0.375, 0.625, 0.875];

    let mut row_cov = std::vec![0.0f32; clip_w];
    let mut crossings: Vec<Crossing> = Vec::with_capacity(32);


    for py_i32 in y_start..y_end {
        let py = py_i32 as i16;
        let row_offset = ((py - origin_y) as usize) * stride;

        {
            row_cov.fill(0.0);
            for &sub_y in &SUB_OFFSETS {
                let sy = (py_i32 as f32) + sub_y;
                crossings.clear();
                for e in &edges {
                    if e.y0 <= sy && sy < e.y1 {
                        let x = e.x0 + (sy - e.y0) * e.dx_dy;
                        crossings.push(Crossing { x, dir: e.dir });
                    }
                }
                if crossings.len() < 2 {
                    continue;
                }
                sort_crossings(&mut crossings);
                accumulate_scanline_spans(&crossings, fill_rule, clip_x0, clip_x1, &mut row_cov);
            }
            flush_row_coverage(
                &row_cov,
                clip_x0,
                origin_x,
                row_offset,
                color,
                1.0,
                target,
            );
        }

    }
}

#[inline]
fn sort_crossings(list: &mut [Crossing]) {
    for i in 1..list.len() {
        let key = list[i];
        let mut j = i;
        while j > 0 && list[j - 1].x > key.x {
            list[j] = list[j - 1];
            j -= 1;
        }
        list[j] = key;
    }
}

#[inline]
fn accumulate_scanline_spans(
    crossings: &[Crossing],
    fill_rule: SvgFillRule,
    tile_x0: i32,
    tile_x1: i32,
    row_cov: &mut [f32],
) {
    let mut winding: i32 = 0;
    let mut span_start = 0.0f32;
    let mut inside = false;
    let tile_x0_f = tile_x0 as f32;
    let tile_x1_f = tile_x1 as f32;

    for c in crossings {
        winding += c.dir as i32;
        let now_inside = match fill_rule {
            SvgFillRule::NonZero => winding != 0,
            SvgFillRule::EvenOdd => (winding & 1) != 0,
        };
        if !inside && now_inside {
            span_start = c.x;
            inside = true;
        } else if inside && !now_inside {
            let x0 = span_start.max(tile_x0_f);
            let x1 = c.x.min(tile_x1_f);
            if x1 > x0 {
                let ix0 = floor_f32(x0) as i32;
                let ix1 = (ceil_f32(x1) as i32).min(tile_x1);
                for px in ix0..ix1 {
                    let px_f = px as f32;
                    let cov = (x1.min(px_f + 1.0) - x0.max(px_f)).clamp(0.0, 1.0);
                    let idx = (px - tile_x0) as usize;
                    if let Some(slot) = row_cov.get_mut(idx) {
                        *slot += cov * 0.25;
                    }
                }
            }
            inside = false;
        }
    }
}

#[derive(Copy, Clone)]
struct StrokeSeg {
    ax: f32,
    ay: f32,
    ux: f32,
    uy: f32,
    len: f32,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    cap_start: SvgLineCap,
    cap_end: SvgLineCap,
}

#[derive(Copy, Clone)]
struct MiterTri {
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

/// 对已变换至物理像素空间的 `path` 执行逐行最大覆盖率解析 SDF 抗锯齿描边（彻底杜绝半透明折点双重叠加）。
#[allow(clippy::too_many_arguments)]
pub fn stroke_path<P: PixelFormat>(
    path: &FlattenedPath,
    color: Color,
    stroke_width_px: f32,
    line_cap: SvgLineCap,
    line_join: SvgLineJoin,
    miter_limit: f32,
    clip: Region,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if color.a() == 0 || stroke_width_px <= 1e-4 || clip.is_empty() || stride == 0 || path.is_empty()
    {
        return;
    }

    // 次像素细线能量守恒：宽度 < 1px 时保持 0.5px 最小解析半径并等比衰减不透明度，防止断线发虚
    let (half_w, alpha_scale) = if stroke_width_px < 1.0 {
        (0.5, stroke_width_px.clamp(0.0, 1.0))
    } else {
        (stroke_width_px * 0.5, 1.0)
    };
    let pad = half_w + 1.0;

    let mut segs: Vec<StrokeSeg> = Vec::with_capacity(path.points().len());

    let mut miters: Vec<MiterTri> = Vec::new();

    let pts = path.points();
    let mut global_min_y = f32::MAX;
    let mut global_max_y = f32::MIN;

    let internal_cap = match line_join {
        SvgLineJoin::Round => SvgLineCap::Round,
        SvgLineJoin::Bevel | SvgLineJoin::Miter => SvgLineCap::Butt,
    };

    for sp in path.subpaths() {
        let end = (sp.start + sp.len).min(pts.len());
        let sub = &pts[sp.start..end];
        if sub.is_empty() {
            continue;
        }
        if sub.len() == 1 {
            // 单点路径在 Round/Square 端帽下绘制一个点/方点
            if line_cap != SvgLineCap::Butt {
                let (x, y) = sub[0];
                let seg = StrokeSeg {
                    ax: x,
                    ay: y,
                    ux: 1.0,
                    uy: 0.0,
                    len: 0.0,
                    min_x: x - pad,
                    max_x: x + pad,
                    min_y: y - pad,
                    max_y: y + pad,
                    cap_start: line_cap,
                    cap_end: line_cap,
                };
                global_min_y = global_min_y.min(seg.min_y);
                global_max_y = global_max_y.max(seg.max_y);
                segs.push(seg);
            }
            continue;
        }

        let seg_count = if sp.closed { sub.len() } else { sub.len() - 1 };
        let sub_seg_base = segs.len();
        for i in 0..seg_count {
            let p0 = sub[i];
            let p1 = sub[(i + 1) % sub.len()];
            let dx = p1.0 - p0.0;
            let dy = p1.1 - p0.1;
            let len = sqrt_f32(dx * dx + dy * dy);
            if len < 1e-5 {
                continue;
            }
            let cap_start = if !sp.closed && i == 0 {
                line_cap
            } else {
                internal_cap
            };
            let cap_end = if !sp.closed && i + 1 == seg_count {
                line_cap
            } else {
                internal_cap
            };
            let ext = if cap_start == SvgLineCap::Square || cap_end == SvgLineCap::Square {
                pad + half_w
            } else {
                pad
            };
            let seg = StrokeSeg {
                ax: p0.0,
                ay: p0.1,
                ux: dx / len,
                uy: dy / len,
                len,
                min_x: p0.0.min(p1.0) - ext,
                max_x: p0.0.max(p1.0) + ext,
                min_y: p0.1.min(p1.1) - ext,
                max_y: p0.1.max(p1.1) + ext,
                cap_start,
                cap_end,
            };
            global_min_y = global_min_y.min(seg.min_y);
            global_max_y = global_max_y.max(seg.max_y);
            segs.push(seg);
        }

        // 为 Bevel / Miter 内部拐角补充外侧连接三角形，消除粗线折角缺口
        let added = segs.len() - sub_seg_base;
        if added >= 2 && line_join != SvgLineJoin::Round {
            let join_count = if sp.closed { added } else { added - 1 };
            for j in 0..join_count {
                let s0 = segs.as_slice()[sub_seg_base + j];
                let s1 = segs.as_slice()[sub_seg_base + ((j + 1) % added)];
                let vx = s1.ax;
                let vy = s1.ay;
                let cross = s0.ux * s1.uy - s0.uy * s1.ux;
                if abs_f32(cross) < 1e-4 {
                    continue;
                }
                // 外侧法向量指向拐角凸出侧
                let sign = if cross > 0.0 { 1.0 } else { -1.0 };
                let n0x = sign * s0.uy;
                let n0y = -sign * s0.ux;
                let n1x = sign * s1.uy;
                let n1y = -sign * s1.ux;
                let p0 = (vx + n0x * half_w, vy + n0y * half_w);
                let p1 = (vx + n1x * half_w, vy + n1y * half_w);
                // 斜切基座三角形 (V, P0, P1)
                push_tri(&mut miters, (vx, vy), p0, p1, &mut global_min_y, &mut global_max_y);

                if line_join == SvgLineJoin::Miter {
                    // 求两条外侧偏移直线的交点作为 Miter 尖角顶点
                    let mx = n0x + n1x;
                    let my = n0y + n1y;
                    let m_len_sq = mx * mx + my * my;
                    if m_len_sq > 1e-5 {
                        let inv_cos = 2.0 / sqrt_f32(m_len_sq);
                        if inv_cos <= miter_limit.max(1.0) {
                            let scale = half_w * inv_cos / sqrt_f32(m_len_sq);
                            let apex = (vx + mx * scale, vy + my * scale);
                            push_tri(
                                &mut miters,
                                p0,
                                apex,
                                p1,
                                &mut global_min_y,
                                &mut global_max_y,
                            );
                        }
                    }
                }
            }
        }
    }

    if segs.is_empty() {
        return;
    }

    let y_start = (floor_f32(global_min_y) as i32)
        .max(clip.y as i32)
        .min(clip.bottom() as i32);
    let y_end = (ceil_f32(global_max_y) as i32)
        .max(clip.y as i32)
        .min(clip.bottom() as i32);
    if y_start >= y_end {
        return;
    }

    let clip_x0 = clip.x as i32;
    let clip_x1 = clip.right() as i32;
    let clip_w = (clip_x1 - clip_x0).max(0) as usize;
    if clip_w == 0 {
        return;
    }

    let mut row_cov = std::vec![0.0f32; clip_w];

    for py_i32 in y_start..y_end {
        let py = py_i32 as i16;
        let py_c = (py_i32 as f32) + 0.5;
        let row_offset = ((py - origin_y) as usize) * stride;

        {
            row_cov.fill(0.0);
            raster_stroke_row(
                py_c,
                clip_x0,
                clip_x1,
                half_w,
                &segs,
                &miters,
                &mut row_cov,
            );
            flush_row_coverage(
                &row_cov,
                clip_x0,
                origin_x,
                row_offset,
                color,
                alpha_scale,
                target,
            );
        }

    }
}

fn push_tri(
    out: &mut Vec<MiterTri>,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    g_min_y: &mut f32,
    g_max_y: &mut f32,
) {
    let min_x = a.0.min(b.0).min(c.0) - 1.0;
    let max_x = a.0.max(b.0).max(c.0) + 1.0;
    let min_y = a.1.min(b.1).min(c.1) - 1.0;
    let max_y = a.1.max(b.1).max(c.1) + 1.0;
    *g_min_y = g_min_y.min(min_y);
    *g_max_y = g_max_y.max(max_y);
    out.push(MiterTri {
        ax: a.0,
        ay: a.1,
        bx: b.0,
        by: b.1,
        cx: c.0,
        cy: c.1,
        min_x,
        max_x,
        min_y,
        max_y,
    });
}


fn raster_stroke_row(
    py_c: f32,
    tile_x0: i32,
    tile_x1: i32,
    half_w: f32,
    segs: &[StrokeSeg],
    miters: &[MiterTri],
    row_cov: &mut [f32],
) {
    for seg in segs {
        if py_c < seg.min_y || py_c > seg.max_y {
            continue;
        }
        let sx0 = (floor_f32(seg.min_x) as i32).max(tile_x0);
        let sx1 = (ceil_f32(seg.max_x) as i32).min(tile_x1);
        if sx0 >= sx1 {
            continue;
        }
        let dy = py_c - seg.ay;
        for px in sx0..sx1 {
            let dx = (px as f32 + 0.5) - seg.ax;
            let along = dx * seg.ux + dy * seg.uy;
            let perp = abs_f32(dx * (-seg.uy) + dy * seg.ux);

            let sd = if along < 0.0 {
                match seg.cap_start {
                    SvgLineCap::Round => sqrt_f32(dx * dx + dy * dy) - half_w,
                    SvgLineCap::Butt => box_outside_sd(-along, perp - half_w),
                    SvgLineCap::Square => box_outside_sd(-along - half_w, perp - half_w),
                }
            } else if along > seg.len {
                let over = along - seg.len;
                match seg.cap_end {
                    SvgLineCap::Round => {
                        let ex = dx - seg.ux * seg.len;
                        let ey = dy - seg.uy * seg.len;
                        sqrt_f32(ex * ex + ey * ey) - half_w
                    }
                    SvgLineCap::Butt => box_outside_sd(over, perp - half_w),
                    SvgLineCap::Square => box_outside_sd(over - half_w, perp - half_w),
                }
            } else {
                perp - half_w
            };

            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov > 0.0 {
                let idx = (px - tile_x0) as usize;
                if let Some(slot) = row_cov.get_mut(idx) {
                    if cov > *slot {
                        *slot = cov;
                    }
                }
            }
        }
    }

    for tri in miters {
        if py_c < tri.min_y || py_c > tri.max_y {
            continue;
        }
        let sx0 = (floor_f32(tri.min_x) as i32).max(tile_x0);
        let sx1 = (ceil_f32(tri.max_x) as i32).min(tile_x1);
        if sx0 >= sx1 {
            continue;
        }
        for px in sx0..sx1 {
            let sd = sd_triangle(
                px as f32 + 0.5,
                py_c,
                tri.ax,
                tri.ay,
                tri.bx,
                tri.by,
                tri.cx,
                tri.cy,
            );
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov > 0.0 {
                let idx = (px - tile_x0) as usize;
                if let Some(slot) = row_cov.get_mut(idx) {
                    if cov > *slot {
                        *slot = cov;
                    }
                }
            }
        }
    }
}

/// 有向矩形外侧有符号距离（`dx_out` 为轴向外凸距离，`dy_out` 为法向外凸距离）。
#[inline(always)]
fn box_outside_sd(dx_out: f32, dy_out: f32) -> f32 {
    if dx_out > 0.0 && dy_out > 0.0 {
        sqrt_f32(dx_out * dx_out + dy_out * dy_out)
    } else {
        dx_out.max(dy_out)
    }
}

/// 点 `(px, py)` 到任意三角形 `(ax, ay)-(bx, by)-(cx, cy)` 的精确有符号距离（内部为负）。
#[allow(clippy::too_many_arguments)]
fn sd_triangle(
    px: f32,
    py: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
) -> f32 {
    let e0x = bx - ax;
    let e0y = by - ay;
    let e1x = cx - bx;
    let e1y = cy - by;
    let e2x = ax - cx;
    let e2y = ay - cy;

    let v0x = px - ax;
    let v0y = py - ay;
    let v1x = px - bx;
    let v1y = py - by;
    let v2x = px - cx;
    let v2y = py - cy;

    let t0 = ((v0x * e0x + v0y * e0y) / (e0x * e0x + e0y * e0y).max(1e-10)).clamp(0.0, 1.0);
    let t1 = ((v1x * e1x + v1y * e1y) / (e1x * e1x + e1y * e1y).max(1e-10)).clamp(0.0, 1.0);
    let t2 = ((v2x * e2x + v2y * e2y) / (e2x * e2x + e2y * e2y).max(1e-10)).clamp(0.0, 1.0);

    let pq0x = v0x - e0x * t0;
    let pq0y = v0y - e0y * t0;
    let pq1x = v1x - e1x * t1;
    let pq1y = v1y - e1y * t1;
    let pq2x = v2x - e2x * t2;
    let pq2y = v2y - e2y * t2;

    let s = if e0x * e2y - e0y * e2x >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let d0 = pq0x * pq0x + pq0y * pq0y;
    let d1 = pq1x * pq1x + pq1y * pq1y;
    let d2 = pq2x * pq2x + pq2y * pq2y;
    let min_d2 = d0.min(d1).min(d2);

    let c0 = s * (v0x * e0y - v0y * e0x);
    let c1 = s * (v1x * e1y - v1y * e1x);
    let c2 = s * (v2x * e2y - v2y * e2x);
    let min_c = c0.min(c1).min(c2);

    let dist = sqrt_f32(min_d2);
    if min_c >= 0.0 {
        -dist
    } else {
        dist
    }
}

#[inline]
fn flush_row_coverage<P: PixelFormat>(
    row_cov: &[f32],
    tile_x0: i32,
    origin_x: i16,
    row_offset: usize,
    color: Color,
    alpha_scale: f32,
    target: &mut [P],
) {
    let base_dst = tile_x0 - (origin_x as i32);
    for (i, &cov_raw) in row_cov.iter().enumerate() {
        if cov_raw <= 1e-4 {
            continue;
        }
        let cov_f = (cov_raw * alpha_scale).clamp(0.0, 1.0);
        let cov_u8 = round_f32(cov_f * 255.0) as u8;
        if cov_u8 == 0 {
            continue;
        }
        let dst_x = base_dst + (i as i32);
        if dst_x >= 0 {
            let idx = row_offset + (dst_x as usize);
            if let Some(dst) = target.get_mut(idx) {
                *dst = dst.blend(color, cov_u8);
            }
        }
    }
}
