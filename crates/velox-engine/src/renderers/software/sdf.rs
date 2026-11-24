//! 纯整数定点数 SDF 覆盖率、前向遮挡剔除（Front-to-Back Culling）与圆角/渐变/阴影矩形光栅化。

use crate::display::{PixelFormat, SpriteFormat};
use crate::element::SpriteSource;
use crate::motion::{DrawOp, DrawPayload};
use crate::sdf::isqrt_i32;
use crate::solver::Region;
use crate::style::Color;

/// 16 倍定点数（$1/16\text{ px}$ 亚像素精度）有向距离场（SDF）计算（支持 1/16px 亚像素圆角半径 `r_16`）。
#[inline(always)]
pub(super) fn compute_sdf_dist_16_r16(px: i16, py: i16, box_reg: Region, r_16: i32) -> i32 {
    let max_r_16 = (box_reg.w.min(box_reg.h).max(0) as i32) * 8;
    let r_16 = r_16.clamp(0, max_r_16);

    let px_16 = (px as i32) * 16 + 8;
    let py_16 = (py as i32) * 16 + 8;

    let half_w_16 = (box_reg.w as i32) * 8;
    let half_h_16 = (box_reg.h as i32) * 8;
    let cx_16 = (box_reg.x as i32) * 16 + half_w_16;
    let cy_16 = (box_reg.y as i32) * 16 + half_h_16;

    let qx = (px_16 - cx_16).abs() - (half_w_16 - r_16);
    let qy = (py_16 - cy_16).abs() - (half_h_16 - r_16);

    let out_x = qx.max(0);
    let out_y = qy.max(0);
    // 非四角圆弧象限（out_x == 0 或 out_y == 0）无需执行牛顿迭代开方，数学结果 100% 等价
    let outside_dist = if out_x == 0 {
        out_y
    } else if out_y == 0 {
        out_x
    } else {
        isqrt_i32(out_x * out_x + out_y * out_y)
    };
    let inside_dist = qx.max(qy).min(0);

    outside_dist + inside_dist - r_16
}

/// 支持 1/16px 亚像素圆角半径 `r_16` 的定点数 SDF 圆角矩形单像素 Alpha 覆盖率计算。
#[inline(always)]
pub(super) fn compute_sdf_coverage_r16(px: i16, py: i16, box_reg: Region, r_16: i32) -> u8 {
    if box_reg.w <= 0 || box_reg.h <= 0 {
        return 0;
    }
    if px < box_reg.x || px >= box_reg.right() || py < box_reg.y || py >= box_reg.bottom() {
        return 0;
    }
    if r_16 <= 0 {
        return 255;
    }

    let sd_16 = compute_sdf_dist_16_r16(px, py, box_reg, r_16);
    if sd_16 <= -8 {
        255
    } else if sd_16 >= 8 {
        0
    } else {
        (((8 - sd_16) * 255) / 16) as u8
    }
}

/// 定点数 SDF（有向距离场）圆角矩形单像素 Alpha 覆盖率计算（零浮点，纯整数运算）。
#[inline(always)]
pub fn compute_sdf_coverage(px: i16, py: i16, box_reg: Region, r: i16) -> u8 {
    compute_sdf_coverage_r16(px, py, box_reg, (r.max(0) as i32) * 16)
}

/// 计算单个 `DrawOp` 的 100% 不透明内切核心矩形（用于前向遮挡剔除 Front-to-Back Culling）。
#[inline]
pub fn opaque_core_rect(op: &DrawOp<'_>) -> Region {
    match op.payload {
        DrawPayload::Quad {
            bg,
            bg_grad,
            border_color,
            rounded,
            border_width,
        } if bg.a() == 255
            && bg_grad.a() == 255
            && !(border_color.a() == 0 && (border_width & 0x7F) > 0) =>
        {
            let bw = (border_width & 0x7F) as i16;
            let inset = if bw > 0 && border_color.a() < 255 {
                (rounded as i16).max(bw)
            } else {
                rounded as i16
            };
            let inner_w = op.bounds.w.saturating_sub(inset.saturating_mul(2));
            let inner_h = op.bounds.h.saturating_sub(inset.saturating_mul(2));
            if inner_w <= 0 || inner_h <= 0 {
                Region::EMPTY
            } else {
                Region::new(
                    op.bounds.x.saturating_add(inset),
                    op.bounds.y.saturating_add(inset),
                    inner_w,
                    inner_h,
                )
                .intersect(op.clip)
            }
        }
        DrawPayload::Sprite(SpriteSource::Bitmap {
            format: SpriteFormat::Rgb565,
            bytes,
            tint,
        }) if tint.a() == 255
            && bytes.len() >= 2
            && {
                let px_n = bytes.len() / 2;
                let area = (op.bounds.w.max(0) as usize).saturating_mul(op.bounds.h.max(0) as usize);
                area > 0 && (px_n >= area || area % px_n == 0)
            } =>
        {
            op.bounds.intersect(op.clip)
        }
        DrawPayload::StyledQuad(sq)
            if sq.blend_mode.is_normal()
                && sq.transform.is_none()
                && sq.opacity == 255
                && sq.paint.is_fully_opaque() =>
        {
            let max_r = sq.radii.max_radius() as i16;
            let bw = sq.border_width as i16;
            let inset = if bw > 0 && sq.border_color.a() < 255 {
                max_r.max(bw)
            } else {
                max_r
            };
            let inner_w = op.bounds.w.saturating_sub(inset.saturating_mul(2));
            let inner_h = op.bounds.h.saturating_sub(inset.saturating_mul(2));
            if inner_w <= 0 || inner_h <= 0 {
                Region::EMPTY
            } else {
                Region::new(
                    op.bounds.x.saturating_add(inset),
                    op.bounds.y.saturating_add(inset),
                    inner_w,
                    inner_h,
                )
                .intersect(op.clip)
            }
        }
        _ => Region::EMPTY,
    }
}

/// 在 Pass 4 末尾预先对全帧 `DrawOp` 执行一次前向遮挡裁剪，收缩或清空被上层全覆盖的不透明图层裁剪框。
pub fn precompute_occlusion_clips(ops: &mut [DrawOp<'_>]) {
    // 若本帧包含隔离合成图层、毛玻璃或可编程混合模式，则保留各节点原始裁剪框，避免误裁剪毛玻璃/混合模式依赖的下层像素。
    if super::effects::ops_have_layers_or_blend(ops) {
        return;
    }
    let len = ops.len();
    for idx in 0..len {
        let base_clip = ops[idx].clip.intersect(ops[idx].bounds);
        if base_clip.is_empty() {
            ops[idx].clip = Region::EMPTY;
            continue;
        }
        let unoccluded = compute_unoccluded_clip(base_clip, &ops[(idx + 1)..]);
        if unoccluded.is_empty() {
            ops[idx].clip = Region::EMPTY;
            continue;
        }
        // `TextRun` 需保留父级裁剪盒（仅裁剪上层遮挡）以便在 1.25x/1.50x/1.75x 分数 DPI 下
        // 按实际物理字号自由扩展单行宽而不被 1x 逻辑 `bounds` 切掉末尾字符；
        // 带圆角裁切（`clip_radius > 0`）或 2D 变换的节点亦保留原父级裁剪盒。
        let keep_parent_clip = matches!(ops[idx].payload, DrawPayload::TextRun { .. })
            || matches!(ops[idx].payload, DrawPayload::StyledQuad(sq) if sq.transform.is_some())
            || ops[idx].clip_radius > 0;
        if keep_parent_clip {
            ops[idx].clip = compute_unoccluded_clip(ops[idx].clip, &ops[(idx + 1)..]);
        } else {
            ops[idx].clip = unoccluded;
        }
    }
}

#[inline]
pub(super) fn clip_against_opaque_rects(mut clip: Region, occluders: &[(usize, Region)], min_idx: usize) -> Region {
    for &(occ_idx, occ) in occluders {
        if occ_idx <= min_idx {
            continue;
        }
        if clip.is_empty() {
            return Region::EMPTY;
        }
        let spans_x = occ.x <= clip.x && occ.right() >= clip.right();
        let spans_y = occ.y <= clip.y && occ.bottom() >= clip.bottom();
        if spans_x && spans_y {
            return Region::EMPTY;
        }
        if spans_x {
            if occ.y <= clip.y && occ.bottom() > clip.y {
                let new_y = occ.bottom();
                clip.h = clip.bottom().saturating_sub(new_y);
                clip.y = new_y;
            } else if occ.bottom() >= clip.bottom() && occ.y < clip.bottom() {
                clip.h = occ.y.saturating_sub(clip.y);
            }
        }
        if spans_y {
            if occ.x <= clip.x && occ.right() > clip.x {
                let new_x = occ.right();
                clip.w = clip.right().saturating_sub(new_x);
                clip.x = new_x;
            } else if occ.right() >= clip.right() && occ.x < clip.right() {
                clip.w = occ.x.saturating_sub(clip.x);
            }
        }
    }
    clip
}

/// 利用上层（`Z > current`）的不透明 `DrawOp` 对当前指令的待光栅化区域 `clip` 进行前向遮挡裁剪。
#[inline]
pub fn compute_unoccluded_clip(mut clip: Region, ops_above: &[DrawOp<'_>]) -> Region {
    for above in ops_above {
        if clip.is_empty() {
            return Region::EMPTY;
        }
        let occ = opaque_core_rect(above);
        if occ.is_empty() {
            continue;
        }
        let spans_x = occ.x <= clip.x && occ.right() >= clip.right();
        let spans_y = occ.y <= clip.y && occ.bottom() >= clip.bottom();

        if spans_x && spans_y {
            return Region::EMPTY;
        }
        if spans_x {
            if occ.y <= clip.y && occ.bottom() > clip.y {
                let new_y = occ.bottom();
                clip.h = clip.bottom().saturating_sub(new_y);
                clip.y = new_y;
            } else if occ.bottom() >= clip.bottom() && occ.y < clip.bottom() {
                clip.h = occ.y.saturating_sub(clip.y);
            }
        }
        if spans_y {
            if occ.x <= clip.x && occ.right() > clip.x {
                let new_x = occ.right();
                clip.w = clip.right().saturating_sub(new_x);
                clip.x = new_x;
            } else if occ.right() >= clip.right() && occ.x < clip.right() {
                clip.w = occ.x.saturating_sub(clip.x);
            }
        }
    }
    clip
}


#[inline(always)]
fn erf_approx(x: f32) -> f32 {
    let s = if x < 0.0 { -1.0 } else if x > 0.0 { 1.0 } else { 0.0 };
    let a = x.abs();
    let t = 1.0 + (0.278_393 + (0.230_389 + 0.078_108 * (a * a)) * a) * a;
    let t2 = t * t;
    s - s / (t2 * t2)
}

#[inline(always)]
fn gaussian_1d(x: f32, sigma: f32) -> f32 {
    let inv = x / sigma;
    (-0.5 * inv * inv).exp() / (2.506_628_3 * sigma)
}

#[inline(always)]
fn rounded_box_shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half_w: f32, half_h: f32) -> f32 {
    let delta = (half_h - corner - y.abs()).min(0.0);
    let curved = half_w - corner + (corner * corner - delta * delta).max(0.0).sqrt();
    let k = 0.707_106_77 / sigma;
    let i0 = 0.5 + 0.5 * erf_approx((x - curved) * k);
    let i1 = 0.5 + 0.5 * erf_approx((x + curved) * k);
    i1 - i0
}

/// 与 `wgpu` `shader.wgsl::rounded_box_shadow` 逐像素等价的 Evan Wallace 4 点解析高斯圆角盒阴影卷积。
#[inline]
fn eval_gaussian_rounded_box_shadow(
    center_x: f32,
    center_y: f32,
    half_w: f32,
    half_h: f32,
    px: f32,
    py: f32,
    sigma: f32,
    corner: f32,
) -> f32 {
    let ptx = px - center_x;
    let pty = py - center_y;
    let low = pty - half_h;
    let high = pty + half_h;
    let start = (-3.0 * sigma).clamp(low, high);
    let end = (3.0 * sigma).clamp(low, high);
    let step = (end - start) * 0.25;
    if step <= 0.0001 {
        return 0.0;
    }
    let mut y = start + step * 0.5;
    let mut val = 0.0f32;
    for _ in 0..4 {
        val += rounded_box_shadow_x(ptx, pty - y, sigma, corner, half_w, half_h)
            * gaussian_1d(y, sigma)
            * step;
        y += step;
    }
    val.clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_quad<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    bg: Color,
    bg_grad: Color,
    border_color: Color,
    rounded: i16,
    encoded_border_width: u8,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let r_16 = (rounded.max(0) as i32) * 16;
    let raw_bw = encoded_border_width & 0x7F;
    rasterize_quad_precise(
        bounds,
        clip,
        bg,
        bg_grad,
        border_color,
        r_16,
        raw_bw as i16,
        raw_bw as f32,
        (encoded_border_width & 0x80) != 0,
        origin_x,
        origin_y,
        stride,
        target,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_quad_precise<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    bg: Color,
    bg_grad: Color,
    border_color: Color,
    rounded_16: i32,
    border_width: i16,
    #[allow(unused_variables)] blur_px_f32: f32,
    grad_horiz: bool,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let is_soft_shadow = border_color.a() == 0 && border_width > 0;

    if is_soft_shadow {
        if bg.a() == 0 {
            return;
        }
        {
            let half_w = (bounds.w as f32) * 0.5;
            let half_h = (bounds.h as f32) * 0.5;
            let center_x = (bounds.x as f32) + half_w;
            let center_y = (bounds.y as f32) + half_h;
            let blur = blur_px_f32.max(1.0);
            let inner_half_w = (half_w - blur).max(0.5);
            let inner_half_h = (half_h - blur).max(0.5);
            let sigma = (blur * 0.45).max(0.5);
            let radius = (rounded_16.max(0) as f32) * (1.0 / 16.0);
            let corner = radius.min(inner_half_w.min(inner_half_h));
            let safe_margin = corner + 3.5 * sigma;
            let core_hw = (inner_half_w - safe_margin).max(0.0);
            let core_hh = (inner_half_h - safe_margin).max(0.0);

            for py in clip.y..clip.bottom() {
                let row_offset = ((py - origin_y) as usize) * stride;
                let py_f = (py as f32) + 0.5;
                let in_core_y = core_hh > 0.0 && (py_f - center_y).abs() < core_hh;
                for px in clip.x..clip.right() {
                    let px_f = (px as f32) + 0.5;
                    let cov_u8 = if in_core_y && core_hw > 0.0 && (px_f - center_x).abs() < core_hw
                    {
                        255u8
                    } else {
                        let cov = eval_gaussian_rounded_box_shadow(
                            center_x,
                            center_y,
                            inner_half_w,
                            inner_half_h,
                            px_f,
                            py_f,
                            sigma,
                            corner,
                        );
                        if cov <= 0.001 {
                            continue;
                        }
                        (cov * 255.0 + 0.5) as u8
                    };
                    if cov_u8 > 0 {
                        let idx = row_offset + ((px - origin_x) as usize);
                        if let Some(dst) = target.get_mut(idx) {
                            *dst = dst.blend(bg, cov_u8);
                        }
                    }
                }
            }
            return;
        }
    }

    let has_grad = bg != bg_grad;
    let has_border = border_width > 0 && border_color.a() > 0;

    if rounded_16 <= 0 && !has_border && !has_grad && bg.a() == 255 {
        let px_val = P::from_color(bg);
        for py in clip.y..clip.bottom() {
            let row_idx = ((py - origin_y) as usize) * stride;
            let start_col = (clip.x - origin_x) as usize;
            let end_col = (clip.right() - origin_x) as usize;
            if let Some(row_slice) = target.get_mut((row_idx + start_col)..(row_idx + end_col)) {
                row_slice.fill(px_val);
            }
        }
        return;
    }

    let bw_16 = (border_width as i32) * 16;
    let h_denom = bounds.h.max(1) as i32;
    let w_denom = bounds.w.max(1) as i32;
    // 圆角与边框内侧的 100% 满覆盖平坦核心水平跨度 [flat_x0, flat_x1)
    let rounded_ceil = ((rounded_16.max(0) + 15) >> 4) as i16;
    let edge_inset = rounded_ceil.saturating_add(border_width).saturating_add(1);
    let flat_y0 = bounds.y.saturating_add(edge_inset);
    let flat_y1 = bounds.bottom().saturating_sub(edge_inset);
    let flat_x0 = bounds.x.saturating_add(edge_inset).max(clip.x);
    let flat_x1 = bounds.right().saturating_sub(edge_inset).min(clip.right());

    for py in clip.y..clip.bottom() {
        let row_offset = ((py - origin_y) as usize) * stride;
        let row_bg = if has_grad && !grad_horiz {
            let ty_256 =
                ((((py - bounds.y).max(0) as i32 * 2 + 1) * 128) / h_denom).clamp(0, 256) as u16;
            bg.lerp(bg_grad, ty_256)
        } else {
            bg
        };

        // 垂直平坦核心区内且非水平渐变时，中间连续段 [flat_x0, flat_x1) 覆盖率恒为 255，可整段填充无需逐像素计算 SDF
        let use_flat_span =
            py >= flat_y0 && py < flat_y1 && flat_x0 < flat_x1 && !(has_grad && grad_horiz);

        let mut px = clip.x;
        let px_end = clip.right();
        while px < px_end {
            if use_flat_span && px == flat_x0 {
                if row_bg.a() == 255 {
                    let s_col = (flat_x0 - origin_x) as usize;
                    let e_col = (flat_x1 - origin_x) as usize;
                    if let Some(slice) = target.get_mut((row_offset + s_col)..(row_offset + e_col))
                    {
                        slice.fill(P::from_color(row_bg));
                    }
                } else if row_bg.a() > 0 {
                    for fpx in flat_x0..flat_x1 {
                        let idx = row_offset + ((fpx - origin_x) as usize);
                        if let Some(dst) = target.get_mut(idx) {
                            *dst = dst.blend(row_bg, 255);
                        }
                    }
                }
                px = flat_x1;
                continue;
            }

            let sd_outer_16 = compute_sdf_dist_16_r16(px, py, bounds, rounded_16);
            if sd_outer_16 >= 8 {
                px += 1;
                continue;
            }
            let outer_cov = if sd_outer_16 <= -8 {
                255u8
            } else {
                (((8 - sd_outer_16) * 255) / 16) as u8
            };

            let idx = row_offset + ((px - origin_x) as usize);
            let Some(dst) = target.get_mut(idx) else {
                px += 1;
                continue;
            };

            let cur_bg = if has_grad && grad_horiz {
                let tx_256 = ((((px - bounds.x).max(0) as i32 * 2 + 1) * 128) / w_denom)
                    .clamp(0, 256) as u16;
                bg.lerp(bg_grad, tx_256)
            } else {
                row_bg
            };

            if has_border {
                let sd_inner_16 = sd_outer_16 + bw_16;
                let inner_cov = if sd_inner_16 <= -8 {
                    255u32
                } else if sd_inner_16 >= 8 {
                    0u32
                } else {
                    (((8 - sd_inner_16) * 255) / 16) as u32
                };

                if inner_cov == 255 {
                    if cur_bg.a() > 0 {
                        *dst = dst.blend(cur_bg, outer_cov);
                    }
                } else if inner_cov == 0 {
                    *dst = dst.blend(border_color, outer_cov);
                } else if cur_bg.a() == 0 {
                    let ring_cov = (((outer_cov as u32) * (255 - inner_cov) + 127) / 255) as u8;
                    if ring_cov > 0 {
                        *dst = dst.blend(border_color, ring_cov);
                    }
                } else {
                    // 预乘 Alpha 混合边框与填充色，严格等价于 GPU shader.wgsl PREMULTIPLIED_ALPHA_BLENDING
                    let ba = border_color.a() as u32;
                    let fa = cur_bg.a() as u32;
                    let inv_ic = 255 - inner_cov;
                    let pm_a = (ba * inv_ic + fa * inner_cov + 127) / 255;
                    if pm_a > 0 {
                        let pm_r = ((border_color.r() as u32) * ba * inv_ic
                            + (cur_bg.r() as u32) * fa * inner_cov
                            + 127 * 255)
                            / (255 * 255);
                        let pm_g = ((border_color.g() as u32) * ba * inv_ic
                            + (cur_bg.g() as u32) * fa * inner_cov
                            + 127 * 255)
                            / (255 * 255);
                        let pm_b = ((border_color.b() as u32) * ba * inv_ic
                            + (cur_bg.b() as u32) * fa * inner_cov
                            + 127 * 255)
                            / (255 * 255);
                        let out_a = ((pm_a * (outer_cov as u32) + 127) / 255).min(255);
                        if out_a > 0 {
                            let sr = ((pm_r * (outer_cov as u32) + 127) / 255).min(255);
                            let sg = ((pm_g * (outer_cov as u32) + 127) / 255).min(255);
                            let sb = ((pm_b * (outer_cov as u32) + 127) / 255).min(255);
                            let bg_px = dst.to_color();
                            let inv_a = 255 - out_a;
                            let r = (sr + ((bg_px.r() as u32) * inv_a + 127) / 255).min(255) as u8;
                            let g = (sg + ((bg_px.g() as u32) * inv_a + 127) / 255).min(255) as u8;
                            let b = (sb + ((bg_px.b() as u32) * inv_a + 127) / 255).min(255) as u8;
                            let da = bg_px.a() as u32;
                            let final_a = (out_a + ((da * inv_a + 127) / 255)).min(255) as u8;
                            *dst = P::from_color(Color::rgba(r, g, b, final_a));
                        }
                    }
                }
            } else if cur_bg.a() > 0 {
                *dst = dst.blend(cur_bg, outer_cov);
            }
            px += 1;
        }
    }
}

