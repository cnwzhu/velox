//! 局部脏区光栅化调度、多线程条带并行合成与离屏图层缓存。

use crate::display::PixelFormat;
use crate::element::SpriteSource;
use crate::motion::{DrawOp, DrawPayload};
use crate::solver::Region;
use crate::style::Color;

use super::effects::{ops_have_layers_or_blend, rasterize_ops_with_layers};
use super::effects::rasterize_styled_quad;
use super::sdf::{
    clip_against_opaque_rects, compute_sdf_coverage, compute_unoccluded_clip, opaque_core_rect,
    rasterize_quad,
};
use super::sdf::{compute_sdf_coverage_r16, rasterize_quad_precise};
use super::sprite::rasterize_arc_sdf_scaled;
use super::sprite::rasterize_sprite_source;
use super::text::rasterize_text;
use super::text::{compute_scaled_text_box, rasterize_text_scaled};
use super::{snap_bounds_region_scaled, snap_clip_region_scaled};

#[cfg(not(target_arch = "wasm32"))]
use super::{SpatialOcclusionGrid, with_spatial_occlusion};

/// 针对局部像素缓冲（全屏 Framebuffer 或 MCU 条带缓冲区）光栅化指定脏区 `dirty_region` 内的所有 `DrawOp`。
pub fn rasterize_region<P: PixelFormat>(
    ops: &[DrawOp<'_>],
    dirty_region: Region,
    buffer_origin_x: i16,
    buffer_origin_y: i16,
    buffer_stride: usize,
    target: &mut [P],
) {
    rasterize_region_inner(
        ops,
        dirty_region,
        buffer_origin_x,
        buffer_origin_y,
        buffer_stride,
        target,
        #[cfg(not(target_arch = "wasm32"))]
        None,
    );
}

fn rasterize_region_inner<P: PixelFormat>(
    ops: &[DrawOp<'_>],
    dirty_region: Region,
    buffer_origin_x: i16,
    buffer_origin_y: i16,
    buffer_stride: usize,
    target: &mut [P],
    #[cfg(not(target_arch = "wasm32"))]
    occlusion: Option<&SpatialOcclusionGrid>,
) {
    if dirty_region.is_empty() || buffer_stride == 0 || target.is_empty() {
        return;
    }
    let buf_w = buffer_stride.min(i16::MAX as usize) as i16;
    let buf_h = (target.len() / buffer_stride).min(i16::MAX as usize) as i16;
    let buf_bounds = Region::new(buffer_origin_x, buffer_origin_y, buf_w, buf_h);
    let dirty_region = dirty_region.intersect(buf_bounds);
    if dirty_region.is_empty() {
        return;
    }

    if ops_have_layers_or_blend(ops) {
        rasterize_ops_with_layers(
            ops,
            buf_bounds,
            1.0,
            dirty_region,
            buffer_origin_x,
            buffer_origin_y,
            buffer_stride,
            target,
        );
        return;
    }

    let mut occluders: crate::arena::SmallVec<(usize, Region), 32> = crate::arena::SmallVec::new();
    let mut occluders_complete = true;
    #[cfg(not(target_arch = "wasm32"))]
    let build_local = occlusion.is_none();
    #[cfg(target_arch = "wasm32")]
    let build_local = true;
    if build_local {
        for (idx, op) in ops.iter().enumerate() {
            let occ = opaque_core_rect(op).intersect(dirty_region);
            if !occ.is_empty() && !occluders.push((idx, occ)) {
                occluders_complete = false;
                break;
            }
        }
    }

    for (idx, op) in ops.iter().enumerate() {
        let raw_clip = match op.payload {
            DrawPayload::StyledQuad(sq) if sq.transform.is_some() => dirty_region.intersect(op.clip),
            _ => dirty_region.intersect(op.clip).intersect(op.bounds),
        };
        if raw_clip.is_empty() {
            continue;
        }
        let effective_clip = {
            #[cfg(not(target_arch = "wasm32"))]
            let indexed = occlusion.map(|grid| grid.clip_region(raw_clip, idx));
            #[cfg(target_arch = "wasm32")]
            let indexed: Option<Region> = None;
            indexed.unwrap_or_else(|| {
                if occluders_complete {
                    clip_against_opaque_rects(raw_clip, occluders.as_slice(), idx)
                } else {
                    compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..])
                }
            })
        };
        if effective_clip.is_empty() {
            continue;
        }

        rasterize_op_in_clip(
            op,
            effective_clip,
            buffer_origin_x,
            buffer_origin_y,
            buffer_stride,
            target,
        );
    }
}

/// 在给定最终可见裁剪区域 `effective_clip` 内光栅化单个 `DrawOp`（含分瓦片圆角裁切与全部载荷变体）。
#[inline]
pub(super) fn rasterize_op_in_clip<P: PixelFormat>(
    op: &DrawOp<'_>,
    effective_clip: Region,
    buffer_origin_x: i16,
    buffer_origin_y: i16,
    buffer_stride: usize,
    target: &mut [P],
) {
    let clip_r = (op.clip_radius as i16).min(op.clip.w / 2).min(op.clip.h / 2);
    let corner_clip_active = clip_r > 0 && touches_rounded_corners(effective_clip, op.clip, clip_r);

    let draw_in_clip = |sub_clip: Region, dst: &mut [P]| match op.payload {
        DrawPayload::Quad {
            bg,
            bg_grad,
            border_color,
            rounded,
            border_width,
        } => {
            rasterize_quad(
                op.bounds,
                sub_clip,
                bg,
                bg_grad,
                border_color,
                rounded as i16,
                border_width,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::TextRun {
            text,
            color,
            font_size,
            text_flags,
        } => {
            rasterize_text(
                op.bounds,
                sub_clip,
                text.as_str(),
                color,
                font_size,
                text_flags,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::Sprite(sprite) => {
            rasterize_sprite_source(
                op.bounds,
                sub_clip,
                sprite,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::StyledQuad(sq) => {
            rasterize_styled_quad(
                op.bounds,
                sub_clip,
                sq,
                1.0,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::PushLayer(layer) => {
            let _ = layer;
        }
        DrawPayload::PopLayer => {}
        DrawPayload::ExternalTexture(ext) => {
            let sq = crate::paint::StyledQuad {
                paint: crate::paint::Paint::Solid(ext.fallback_bg),
                border_color: Color::TRANSPARENT,
                border_width: 0,
                radii: ext.radii,
                opacity: ext.opacity,
                blend_mode: ext.blend_mode,
                transform: None,
            };
            rasterize_styled_quad(
                op.bounds,
                sub_clip,
                &sq,
                1.0,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::RichText(rt) => {
            super::text::rasterize_rich_text(
                rt,
                op.bounds,
                sub_clip,
                1.0,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::DecodedImage(img) => {
            super::text::rasterize_decoded_image(
                img,
                op.bounds,
                sub_clip,
                1.0,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::Vector(vec_spec) => {
            crate::vector_canvas::rasterize_vector_spec(
                vec_spec,
                op.bounds,
                sub_clip,
                1.0,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
        DrawPayload::SceneCanvas(spec) => {
            crate::canvas_scene::rasterize_scene_canvas(
                op.bounds,
                sub_clip,
                spec,
                1.0,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                dst,
            );
        }
    };

    if corner_clip_active {
        draw_op_with_tiled_rounded_clip(
            effective_clip,
            op.clip,
            clip_r,
            buffer_origin_x,
            buffer_origin_y,
            buffer_stride,
            target,
            draw_in_clip,
            |px, py| compute_sdf_coverage(px, py, op.clip, clip_r),
        );
    } else {
        draw_in_clip(effective_clip, target);
    }
}

/// 面向 `#![no_std]` 低栈预算与任意圆角半径的分瓦片圆角裁切冷路径（`#[inline(never)]` 隔离栈帧）：
/// - 99% 无圆角裁切的常规 `DrawOp` 绝不承担一字节栈备份开销；
/// - 触发圆角裁切时先零开销直出中间安全区，再对 4 个圆角区域按 `16×16` 微瓦片（`Rgb565` 仅 512B、`Rgba8888` 仅 1KB）
///   分块备份/绘制/SDF 抗锯齿还原，同时支持任意大半径圆角裁切且零堆分配。
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_op_with_tiled_rounded_clip<P: PixelFormat>(
    effective_clip: Region,
    clip_box: Region,
    clip_r: i16,
    buffer_origin_x: i16,
    buffer_origin_y: i16,
    buffer_stride: usize,
    target: &mut [P],
    mut draw_in_clip: impl FnMut(Region, &mut [P]),
    coverage_at: impl Fn(i16, i16) -> u8,
) {
    const TILE: i16 = 16;
    let cr = clip_r.min(clip_box.w / 2).min(clip_box.h / 2);
    if cr <= 0 {
        draw_in_clip(effective_clip, target);
        return;
    }

    // 1. 先直接绘制不与 4 个圆角盒重叠的安全区域（上中、全宽中段、下中），完全零备份开销
    let mid_w = (clip_box.w - cr.saturating_mul(2)).max(0);
    let mid_h = (clip_box.h - cr.saturating_mul(2)).max(0);
    let safe_regions = [
        Region::new(clip_box.x.saturating_add(cr), clip_box.y, mid_w, cr),
        Region::new(clip_box.x, clip_box.y.saturating_add(cr), clip_box.w, mid_h),
        Region::new(
            clip_box.x.saturating_add(cr),
            clip_box.bottom().saturating_sub(cr),
            mid_w,
            cr,
        ),
    ];
    for safe in safe_regions {
        let sub = effective_clip.intersect(safe);
        if !sub.is_empty() {
            draw_in_clip(sub, target);
        }
    }

    // 2. 对 4 个角矩形按 16×16 瓦片分块：备份旧背景 -> 在瓦片内光栅化 -> 按 SDF 覆盖率抗锯齿还原
    let raw_corners = [
        Region::new(clip_box.x, clip_box.y, cr, cr),
        Region::new(clip_box.right().saturating_sub(cr), clip_box.y, cr, cr),
        Region::new(clip_box.x, clip_box.bottom().saturating_sub(cr), cr, cr),
        Region::new(
            clip_box.right().saturating_sub(cr),
            clip_box.bottom().saturating_sub(cr),
            cr,
            cr,
        ),
    ];

    let mut tile_backup = [P::from_color(Color::TRANSPARENT); (TILE as usize) * (TILE as usize)];

    for raw in raw_corners {
        let r = effective_clip.intersect(raw);
        if r.is_empty() {
            continue;
        }
        let mut ty = r.y;
        while ty < r.bottom() {
            let th = (r.bottom() - ty).min(TILE);
            let mut tx = r.x;
            while tx < r.right() {
                let tw = (r.right() - tx).min(TILE);
                let tile = Region::new(tx, ty, tw, th);

                let mut cursor = 0usize;
                for py in tile.y..tile.bottom() {
                    let row = ((py - buffer_origin_y) as usize) * buffer_stride;
                    for px in tile.x..tile.right() {
                        let idx = row + ((px - buffer_origin_x) as usize);
                        tile_backup[cursor] = target
                            .get(idx)
                            .copied()
                            .unwrap_or_else(|| P::from_color(Color::TRANSPARENT));
                        cursor += 1;
                    }
                }

                draw_in_clip(tile, target);

                let mut cursor = 0usize;
                for py in tile.y..tile.bottom() {
                    let row = ((py - buffer_origin_y) as usize) * buffer_stride;
                    for px in tile.x..tile.right() {
                        let old_px = tile_backup[cursor];
                        cursor += 1;
                        let cov = coverage_at(px, py);
                        if cov < 255 {
                            let idx = row + ((px - buffer_origin_x) as usize);
                            if let Some(dst) = target.get_mut(idx) {
                                if cov == 0 {
                                    *dst = old_px;
                                } else {
                                    let new_col = dst.to_color();
                                    let old_col = old_px.to_color();
                                    let t_256 = (cov as u16) + ((cov >> 7) as u16);
                                    *dst = P::from_color(old_col.lerp(new_col, t_256));
                                }
                            }
                        }
                    }
                }

                tx = tx.saturating_add(tw);
            }
            ty = ty.saturating_add(th);
        }
    }
}

#[inline]
fn touches_rounded_corners(eff: Region, clip: Region, r: i16) -> bool {
    let x_near = eff.x < clip.x + r || eff.right() > clip.right() - r;
    let y_near = eff.y < clip.y + r || eff.bottom() > clip.bottom() - r;
    x_near && y_near
}

/// 多条带高 DPI 软件光栅化前一次性预计算的 `TextRun` 缩放盒与量化字号，
/// 避免每个 16 行并行条带对全屏 `TextRun` 重复调用 `compute_scaled_text_box`（`count_text_lines` + `measure_line_width_f32`）。
#[derive(Copy, Clone)]
pub(super) struct PrecomputedScaledTextBox {
    adj_bounds: Region,
    adj_sc: Region,
    adj_flags: u8,
    logical_fs: u8,
    fs_q4: u16,
}

fn precompute_scaled_text_boxes(
    ops: &[DrawOp<'_>],
    viewport: Region,
    sf: f32,
    #[cfg(not(target_arch = "wasm32"))] occlusion: Option<&SpatialOcclusionGrid>,
) -> std::vec::Vec<Option<PrecomputedScaledTextBox>> {
    let mut table = std::vec![None; ops.len()];
    if ops.is_empty() || viewport.is_empty() {
        return table;
    }
    let has_text = ops
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::TextRun { .. }));
    if !has_text {
        return table;
    }

    let vp_clip_phys = snap_clip_region_scaled(viewport, sf);
    let mut occluders: crate::arena::SmallVec<(usize, Region), 32> = crate::arena::SmallVec::new();
    let mut occluders_complete = true;
    #[cfg(not(target_arch = "wasm32"))]
    let build_local = occlusion.is_none();
    #[cfg(target_arch = "wasm32")]
    let build_local = true;
    if build_local {
        for (idx, op) in ops.iter().enumerate() {
            let occ = opaque_core_rect(op).intersect(viewport);
            if !occ.is_empty() && !occluders.push((idx, occ)) {
                occluders_complete = false;
                break;
            }
        }
    }

    for (idx, op) in ops.iter().enumerate() {
        let DrawPayload::TextRun {
            text,
            color,
            font_size,
            text_flags,
        } = op.payload
        else {
            continue;
        };
        if text.is_empty() || color.a() == 0 {
            continue;
        }
        let raw_clip = viewport.intersect(op.clip).intersect(op.bounds);
        if raw_clip.is_empty() {
            continue;
        }
        let eff_clip = {
            #[cfg(not(target_arch = "wasm32"))]
            let indexed = occlusion.map(|grid| grid.clip_region(raw_clip, idx));
            #[cfg(target_arch = "wasm32")]
            let indexed: Option<Region> = None;
            indexed.unwrap_or_else(|| {
                if occluders_complete {
                    clip_against_opaque_rects(raw_clip, occluders.as_slice(), idx)
                } else {
                    compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..])
                }
            })
        };
        if eff_clip.is_empty() {
            continue;
        }
        let sb = snap_bounds_region_scaled(op.bounds, sf);
        let clip_region = if op.clip_radius > 0 { op.clip } else { eff_clip };
        let mut sc = snap_clip_region_scaled(clip_region, sf);
        if (op.bounds.w == 1 && eff_clip.w == 1) || (op.bounds.h == 1 && eff_clip.h == 1) {
            if op.bounds.w == 1 && eff_clip.w == 1 {
                let new_r = sc.right().max(sb.right()).min(vp_clip_phys.right());
                sc.w = new_r.saturating_sub(sc.x).max(0);
            }
            if op.bounds.h == 1 && eff_clip.h == 1 {
                let new_b = sc.bottom().max(sb.bottom()).min(vp_clip_phys.bottom());
                sc.h = new_b.saturating_sub(sc.y).max(0);
            }
        }
        let (adj_bounds, adj_sc, adj_flags, logical_fs, fs_q4) = compute_scaled_text_box(
            op.bounds,
            op.clip,
            viewport,
            sb,
            sc,
            text.as_str(),
            font_size,
            text_flags,
            sf,
        );
        table[idx] = Some(PrecomputedScaledTextBox {
            adj_bounds,
            adj_sc,
            adj_flags,
            logical_fs,
            fs_q4,
        });
    }
    table
}

/// 在 `std` 下直接按精确分数/整数 DPI 缩放因子 `scale_factor` 光栅化原始逻辑 `DrawOp` 流，
/// 与 `WgpuRenderer` 共享同一套 Pixel Snapping、0.25px 亚像素字号 (`font_size_q4`)、
/// Evan Wallace 高斯软阴影、解析圆弧 SDF 与预乘边框混合真源，消除软件首屏切 GPU 时的视觉抖动。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_region_scaled<P: PixelFormat>(
    ops: &[DrawOp<'_>],
    viewport: Region,
    scale_factor: f32,
    dirty_region_phys: Region,
    buffer_origin_x: i16,
    buffer_origin_y: i16,
    buffer_stride: usize,
    target: &mut [P],
    #[cfg(not(target_arch = "wasm32"))] occlusion: Option<&SpatialOcclusionGrid>,
    precomputed_text: Option<&[Option<PrecomputedScaledTextBox>]>,
) {
    if dirty_region_phys.is_empty() || buffer_stride == 0 || target.is_empty() || viewport.is_empty()
    {
        return;
    }
    let buf_w = buffer_stride.min(i16::MAX as usize) as i16;
    let buf_h = (target.len() / buffer_stride).min(i16::MAX as usize) as i16;
    let buf_bounds = Region::new(buffer_origin_x, buffer_origin_y, buf_w, buf_h);
    let dirty_phys = dirty_region_phys.intersect(buf_bounds);
    if dirty_phys.is_empty() {
        return;
    }

    if ops_have_layers_or_blend(ops) {
        rasterize_ops_with_layers(
            ops,
            viewport,
            scale_factor,
            dirty_phys,
            buffer_origin_x,
            buffer_origin_y,
            buffer_stride,
            target,
        );
        return;
    }

    let sf = scale_factor.clamp(0.5, 4.0);
    let vp_clip_phys = snap_clip_region_scaled(viewport, sf);

    let mut occluders: crate::arena::SmallVec<(usize, Region), 32> = crate::arena::SmallVec::new();
    let mut occluders_complete = true;
    #[cfg(not(target_arch = "wasm32"))]
    let build_local = occlusion.is_none();
    #[cfg(target_arch = "wasm32")]
    let build_local = true;
    if build_local {
        for (idx, op) in ops.iter().enumerate() {
            let occ = opaque_core_rect(op).intersect(viewport);
            if !occ.is_empty() && !occluders.push((idx, occ)) {
                occluders_complete = false;
                break;
            }
        }
    }

    for (idx, op) in ops.iter().enumerate() {
        let raw_clip = match op.payload {
            DrawPayload::StyledQuad(sq) if sq.transform.is_some() => viewport.intersect(op.clip),
            _ => viewport.intersect(op.clip).intersect(op.bounds),
        };
        if raw_clip.is_empty() {
            continue;
        }
        let eff_clip = {
            #[cfg(not(target_arch = "wasm32"))]
            let indexed = occlusion.map(|grid| grid.clip_region(raw_clip, idx));
            #[cfg(target_arch = "wasm32")]
            let indexed: Option<Region> = None;
            indexed.unwrap_or_else(|| {
                if occluders_complete {
                    clip_against_opaque_rects(raw_clip, occluders.as_slice(), idx)
                } else {
                    compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..])
                }
            })
        };
        if eff_clip.is_empty() {
            continue;
        }

        let sb = snap_bounds_region_scaled(op.bounds, sf);
        let clip_region = if op.clip_radius > 0 { op.clip } else { eff_clip };
        let mut sc = snap_clip_region_scaled(clip_region, sf);

        // 对未被遮挡的 1px 细线（光标、分割线、缩进参考线）锁定恒定物理线宽，防止被父 clip 奇数坐标截断
        if (op.bounds.w == 1 && eff_clip.w == 1) || (op.bounds.h == 1 && eff_clip.h == 1) {
            if op.bounds.w == 1 && eff_clip.w == 1 {
                let new_r = sc.right().max(sb.right()).min(vp_clip_phys.right());
                sc.w = new_r.saturating_sub(sc.x).max(0);
            }
            if op.bounds.h == 1 && eff_clip.h == 1 {
                let new_b = sc.bottom().max(sb.bottom()).min(vp_clip_phys.bottom());
                sc.h = new_b.saturating_sub(sc.y).max(0);
            }
        }

        let clip_r_16 = if op.clip_radius > 0 {
            let raw_r16 = ((op.clip_radius as f32) * sf * 16.0).round() as i32;
            let max_r16 = (sc.w.min(sc.h).max(0) as i32) * 8;
            raw_r16.clamp(0, max_r16)
        } else {
            0
        };
        let clip_r_ceil = ((clip_r_16 + 15) >> 4) as i16;

        // 预先确定该 op 在当前条带内的实际绘制裁剪盒
        let (eff_phys, text_params) = match op.payload {
            DrawPayload::TextRun {
                text,
                color,
                font_size,
                text_flags,
            } => {
                if text.is_empty() || color.a() == 0 {
                    continue;
                }
                let (adj_bounds, adj_sc, adj_flags, logical_fs, fs_q4) = if let Some(pre) =
                    precomputed_text.and_then(|t| t.get(idx)).and_then(|&slot| slot)
                {
                    (
                        pre.adj_bounds,
                        pre.adj_sc,
                        pre.adj_flags,
                        pre.logical_fs,
                        pre.fs_q4,
                    )
                } else {
                    // 未启用多条带预计算表时，先用保守垂直包围盒快速剔除完全不落入当前脏区的文本
                    let logical_fs = font_size.max(8);
                    let render_fs = ((logical_fs as f32) * sf).clamp(8.0, 255.0).ceil() as i16;
                    let expand_y = render_fs.saturating_sub(sb.h).max(0) / 2 + 4;
                    let est_y0 = sc.y.min(sb.y.saturating_sub(expand_y));
                    let est_y1 = sc.bottom().max(sb.bottom().saturating_add(expand_y));
                    if est_y1 <= dirty_phys.y || est_y0 >= dirty_phys.bottom() {
                        continue;
                    }
                    compute_scaled_text_box(
                        op.bounds,
                        op.clip,
                        viewport,
                        sb,
                        sc,
                        text.as_str(),
                        font_size,
                        text_flags,
                        sf,
                    )
                };
                let eff = adj_sc.intersect(dirty_phys);
                if eff.is_empty() {
                    continue;
                }
                (eff, Some((adj_bounds, adj_flags, logical_fs, fs_q4)))
            }
            DrawPayload::StyledQuad(sq) if sq.transform.is_some() => {
                let eff = sc.intersect(dirty_phys);
                if eff.is_empty() {
                    continue;
                }
                (eff, None)
            }
            _ => {
                let eff = sc.intersect(sb).intersect(dirty_phys);
                if eff.is_empty() {
                    continue;
                }
                (eff, None)
            }
        };

        let corner_clip_active = clip_r_16 > 0 && touches_rounded_corners(eff_phys, sc, clip_r_ceil);

        let draw_scaled_in_clip = |sub_clip: Region, dst: &mut [P]| match op.payload {
            DrawPayload::Quad {
                bg,
                bg_grad,
                border_color,
                rounded,
                border_width,
            } => {
                let raw_bw = border_width & 0x7F;
                let bw_px = if raw_bw == 0 {
                    0i16
                } else {
                    ((raw_bw as f32) * sf).round().clamp(1.0, 127.0) as i16
                };
                let horiz = (border_width & 0x80) != 0;
                let r_16 = ((rounded as f32) * sf * 16.0).round() as i32;
                let blur_px = (raw_bw as f32) * sf;
                rasterize_quad_precise(
                    sb,
                    sub_clip,
                    bg,
                    bg_grad,
                    border_color,
                    r_16,
                    bw_px,
                    blur_px,
                    horiz,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::TextRun { text, color, .. } => {
                if let Some((adj_bounds, adj_flags, logical_fs, fs_q4)) = text_params {
                    rasterize_text_scaled(
                        adj_bounds,
                        sub_clip,
                        text.as_str(),
                        color,
                        logical_fs,
                        fs_q4,
                        adj_flags,
                        buffer_origin_x,
                        buffer_origin_y,
                        buffer_stride,
                        dst,
                    );
                }
            }
            DrawPayload::Sprite(SpriteSource::Arc {
                thickness,
                start_deg10,
                sweep_deg10,
                fill_sweep_deg10,
                track_color,
                fill_color,
            }) => {
                rasterize_arc_sdf_scaled(
                    sb,
                    sub_clip,
                    ((thickness.max(1) as f32) * sf).max(1.5),
                    start_deg10,
                    sweep_deg10,
                    fill_sweep_deg10,
                    track_color,
                    fill_color,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::Sprite(sprite) => {
                rasterize_sprite_source(
                    sb,
                    sub_clip,
                    sprite,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::StyledQuad(sq) => {
                rasterize_styled_quad(
                    sb,
                    sub_clip,
                    sq,
                    sf,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::PushLayer(_) | DrawPayload::PopLayer => {}
            DrawPayload::ExternalTexture(ext) => {
                let sq = crate::paint::StyledQuad {
                    paint: crate::paint::Paint::Solid(ext.fallback_bg),
                    border_color: Color::TRANSPARENT,
                    border_width: 0,
                    radii: ext.radii,
                    opacity: ext.opacity,
                    blend_mode: ext.blend_mode,
                    transform: None,
                };
                rasterize_styled_quad(
                    sb,
                    sub_clip,
                    &sq,
                    sf,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::RichText(rt) => {
                super::text::rasterize_rich_text(
                    rt,
                    sb,
                    sub_clip,
                    sf,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::DecodedImage(img) => {
                super::text::rasterize_decoded_image(
                    img,
                    sb,
                    sub_clip,
                    sf,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::Vector(vec_spec) => {
                crate::vector_canvas::rasterize_vector_spec(
                    vec_spec,
                    sb,
                    sub_clip,
                    sf,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
            DrawPayload::SceneCanvas(spec) => {
                crate::canvas_scene::rasterize_scene_canvas(
                    sb,
                    sub_clip,
                    spec,
                    sf,
                    buffer_origin_x,
                    buffer_origin_y,
                    buffer_stride,
                    dst,
                );
            }
        };

        if corner_clip_active {
            draw_op_with_tiled_rounded_clip(
                eff_phys,
                sc,
                clip_r_ceil,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                target,
                draw_scaled_in_clip,
                |px, py| compute_sdf_coverage_r16(px, py, sc, clip_r_16),
            );
        } else {
            draw_scaled_in_clip(eff_phys, target);
        }
    }
}

/// 多核按条带并行执行分数/整数 DPI 对齐光栅化（`std` 下软件首屏直出入口，与 `WgpuRenderer` 像素级一致）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_region_scaled_parallel<P: PixelFormat>(
    ops: &[DrawOp<'_>],
    viewport: Region,
    scale_factor: f32,
    dirty_region: Region,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if dirty_region.is_empty() || stride == 0 || ops.is_empty() || viewport.is_empty() {
        return;
    }
    let sf = scale_factor.clamp(0.5, 4.0);
    let dx0 = ((dirty_region.x as f32) * sf)
        .floor()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let dy0 = ((dirty_region.y as f32) * sf)
        .floor()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let dx1 = ((dirty_region.right() as f32) * sf)
        .ceil()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let dy1 = ((dirty_region.bottom() as f32) * sf)
        .ceil()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let dirty_phys = Region::new(dx0, dy0, dx1.saturating_sub(dx0).max(0), dy1.saturating_sub(dy0).max(0));
    let ox_phys = ((origin_x as f32) * sf)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let oy_phys = ((origin_y as f32) * sf)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;

    #[cfg(not(target_arch = "wasm32"))]
    {
        let area = (dirty_phys.w.max(0) as usize) * (dirty_phys.h.max(0) as usize);
        if dirty_phys.h >= 32
            && area >= 65_536
            && can_rasterize_parallel(ops)
        {
            if parallel_pool::dispatch_parallel_strips_scaled(
                ops,
                viewport,
                sf,
                dirty_phys,
                ox_phys,
                oy_phys,
                stride,
                target,
            ) {
                return;
            }
        }
    }

    rasterize_region_scaled(
        ops,
        viewport,
        sf,
        dirty_phys,
        ox_phys,
        oy_phys,
        stride,
        target,
        #[cfg(not(target_arch = "wasm32"))]
        None,
        None,
    );
}


/// 将已预渲染的离屏像素缓冲 `src`（宽高 `src_w x src_h`）按屏幕偏移 `(offset_x, offset_y)` 与整体不透明度 `alpha`（`0..=255`）
/// 高速合成（Blit）到目标帧缓冲 `dst` 的裁剪区 `clip` 内。
///
/// 用于 `Navigator` 页面切换等整屏转场动画：转场期间无需每帧重新光栅化复杂的 `DrawOp` 树，
/// 直接对离屏页面缓冲执行行拷贝或定点 Alpha 混合。
pub fn blit_translated_layer<P: PixelFormat>(
    src: &[P],
    src_w: i16,
    src_h: i16,
    offset_x: i16,
    offset_y: i16,
    alpha: u8,
    clip: Region,
    dst_origin_x: i16,
    dst_origin_y: i16,
    dst_stride: usize,
    dst: &mut [P],
) {
    if alpha == 0 || src_w <= 0 || src_h <= 0 || clip.is_empty() || dst_stride == 0 {
        return;
    }
    let placed = Region::new(offset_x, offset_y, src_w, src_h).intersect(clip);
    if placed.is_empty() {
        return;
    }
    let src_stride = src_w as usize;
    let copy_w = placed.w as usize;

    for py in placed.y..placed.bottom() {
        let sy = (py - offset_y) as usize;
        let sx = (placed.x - offset_x) as usize;
        let dy = (py - dst_origin_y) as usize;
        let dx = (placed.x - dst_origin_x) as usize;
        let src_start = sy * src_stride + sx;
        let dst_start = dy * dst_stride + dx;
        let Some(src_row) = src.get(src_start..src_start + copy_w) else {
            continue;
        };
        let Some(dst_row) = dst.get_mut(dst_start..dst_start + copy_w) else {
            continue;
        };
        if alpha == 255 {
            dst_row.copy_from_slice(src_row);
        } else {
            for (d, &s) in dst_row.iter_mut().zip(src_row.iter()) {
                *d = d.blend(s.to_color(), alpha);
            }
        }
    }
}

/// 面向整屏页面转场动画（如 `Navigator` 推入/弹出切换）的双槽位离屏图层缓存与合成器。
///
/// 核心优势：
/// - 页面首次进入或标记脏（`force_reraster = true`）时仅执行 **1 次** 完整光栅化并缓存至离屏帧缓冲；
/// - 在转场动画的每一帧（如 60FPS 连续 15~20 帧）中，不再重新求解与光栅化整屏 `DrawOp`，
///   而是直接通过 [`blit_translated_layer`] 对前后两个离屏页面缓冲做平移与 Alpha 合成，
///   将整屏转场每帧耗时从毫秒级降低至微秒级内存拷贝。
#[derive(Clone, Debug)]
pub struct OffscreenCompositor<P: PixelFormat = crate::display::Xrgb8888> {
    width: i16,
    height: i16,
    slots: [OffscreenLayerSlot<P>; 2],
    next_evict: usize,
    total_rasters: usize,
}

#[derive(Clone, Debug)]
struct OffscreenLayerSlot<P: PixelFormat> {
    key: Option<u32>,
    width: i16,
    height: i16,
    pixels: std::vec::Vec<P>,
}

impl<P: PixelFormat> Default for OffscreenLayerSlot<P> {
    fn default() -> Self {
        Self {
            key: None,
            width: 0,
            height: 0,
            pixels: std::vec::Vec::new(),
        }
    }
}

impl<P: PixelFormat> OffscreenCompositor<P> {
    /// 创建指定视口宽高 `(width, height)` 的双槽位离屏合成器。
    pub fn new(width: i16, height: i16) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            slots: [OffscreenLayerSlot::default(), OffscreenLayerSlot::default()],
            next_evict: 0,
            total_rasters: 0,
        }
    }

    /// 调整离屏合成器视口尺寸并失效旧尺寸缓存。
    pub fn resize(&mut self, width: i16, height: i16) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width != w || self.height != h {
            self.width = w;
            self.height = h;
            self.invalidate_all();
        }
    }

    /// 使指定 `page_key` 的离屏缓存失效。
    pub fn invalidate(&mut self, page_key: u32) {
        for slot in &mut self.slots {
            if slot.key == Some(page_key) {
                slot.key = None;
            }
        }
    }

    /// 清空全部离屏页面缓存标识（保留底层已分配内存以便复用）。
    pub fn invalidate_all(&mut self) {
        for slot in &mut self.slots {
            slot.key = None;
        }
    }

    /// 返回自创建以来实际执行离屏整页光栅化的累计次数（用于验证转场期间的 0 重绘复用）。
    #[inline]
    pub const fn total_rasterizations(&self) -> usize {
        self.total_rasters
    }

    /// 查询指定 `page_key` 是否已缓存在离屏槽位中。
    #[inline]
    pub fn has_cached_layer(&self, page_key: u32) -> bool {
        self.slots.iter().any(|s| {
            s.key == Some(page_key) && s.width == self.width && s.height == self.height
        })
    }

    /// 获取已缓存的 `page_key` 离屏像素切片。
    #[inline]
    pub fn layer_pixels(&self, page_key: u32) -> Option<&[P]> {
        self.slots
            .iter()
            .find(|s| s.key == Some(page_key) && s.width == self.width && s.height == self.height)
            .map(|s| s.pixels.as_slice())
    }

    /// 确保 `page_key` 对应的离屏缓冲已就绪：若命中缓存且 `!force_reraster`，直接 O(1) 返回缓存切片；
    /// 否则调用一次 `raster_fn` 渲染到离屏槽位并缓存。
    pub fn cache_layer_with<F>(&mut self, page_key: u32, force_reraster: bool, raster_fn: F) -> &[P]
    where
        F: FnOnce(&mut [P], i16, i16),
    {
        let w = self.width;
        let h = self.height;
        let len = (w as usize) * (h as usize);

        if let Some(idx) = self.slots.iter().position(|s| s.key == Some(page_key)) {
            if !force_reraster && self.slots[idx].width == w && self.slots[idx].height == h {
                self.next_evict = 1 - idx;
                return &self.slots[idx].pixels;
            }
            self.slots[idx].width = w;
            self.slots[idx].height = h;
            self.slots[idx].pixels.resize(len, P::from_color(Color::TRANSPARENT));
            raster_fn(&mut self.slots[idx].pixels, w, h);
            self.slots[idx].key = Some(page_key);
            self.total_rasters += 1;
            self.next_evict = 1 - idx;
            return &self.slots[idx].pixels;
        }

        let target_idx = self
            .slots
            .iter()
            .position(|s| s.key.is_none())
            .unwrap_or(self.next_evict & 1);
        self.slots[target_idx].width = w;
        self.slots[target_idx].height = h;
        self.slots[target_idx]
            .pixels
            .resize(len, P::from_color(Color::TRANSPARENT));
        raster_fn(&mut self.slots[target_idx].pixels, w, h);
        self.slots[target_idx].key = Some(page_key);
        self.total_rasters += 1;
        self.next_evict = 1 - target_idx;
        &self.slots[target_idx].pixels
    }

    /// 将 [`crate::engine::VeloxEngine`] 当前视图光栅化并缓存至 `page_key` 对应的离屏缓冲（若已缓存且 `!force_reraster` 则跳过光栅化）。
    pub fn cache_engine_page(
        &mut self,
        page_key: u32,
        force_reraster: bool,
        engine: &mut crate::engine::VeloxEngine,
    ) -> &[P] {
        self.cache_layer_with(page_key, force_reraster, |buf, w, h| {
            let _ = engine.step_frame_dt(16);
            engine.rasterize_region(Region::new(0, 0, w, h), buf);
        })
    }

    /// 将前后两个缓存图层 `prev_layer` `(key, offset_x, offset_y, alpha)` 与 `curr_layer` `(key, offset_x, offset_y, alpha)`
    /// 合成至目标帧缓冲 `target`。
    pub fn composite_transition(
        &self,
        prev_layer: Option<(u32, i16, i16, u8)>,
        curr_layer: (u32, i16, i16, u8),
        backdrop: Color,
        target: &mut [P],
    ) {
        let viewport = Region::new(0, 0, self.width, self.height);
        let stride = self.width as usize;
        let full_covered_by_curr = curr_layer.1 == 0
            && curr_layer.2 == 0
            && curr_layer.3 == 255
            && self.has_cached_layer(curr_layer.0);
        let full_covered_by_slide_pair = if let Some((pk, pox, poy, pa)) = prev_layer {
            pa == 255
                && curr_layer.3 == 255
                && self.has_cached_layer(pk)
                && self.has_cached_layer(curr_layer.0)
                && ((poy == 0
                    && curr_layer.2 == 0
                    && (pox + self.width == curr_layer.1 || curr_layer.1 + self.width == pox))
                    || (pox == 0 && poy == 0))
        } else {
            false
        };

        if !full_covered_by_curr && !full_covered_by_slide_pair {
            let bg_px = P::from_color(backdrop);
            let total = (self.width as usize) * (self.height as usize);
            let fill_len = target.len().min(total);
            target[..fill_len].fill(bg_px);
        }

        if let Some((prev_key, ox, oy, alpha)) = prev_layer {
            if let Some(prev_px) = self.layer_pixels(prev_key) {
                blit_translated_layer(
                    prev_px,
                    self.width,
                    self.height,
                    ox,
                    oy,
                    alpha,
                    viewport,
                    0,
                    0,
                    stride,
                    target,
                );
            }
        }

        let (curr_key, ox, oy, alpha) = curr_layer;
        if let Some(curr_px) = self.layer_pixels(curr_key) {
            blit_translated_layer(
                curr_px,
                self.width,
                self.height,
                ox,
                oy,
                alpha,
                viewport,
                0,
                0,
                stride,
                target,
            );
        }
    }
}

/// 获取当前多线程条带光栅化器的并行工作线程数（`no_std` 下恒为 `1`）。
#[inline]
pub fn rasterize_parallel_threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        parallel_pool::worker_threads_count()
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// 多核按条带并行光栅化入口。
///
/// - 在 `std` 多核宿主上，当脏矩形面积与高度达到阈值且不含非 `Sync` 的过程化 `Shader` 闭包时，
///   自动将 `dirty_region` 沿垂直方向切分为不相交的水平条带，分发给常驻工作线程池并行光栅化；
/// - 每个条带写入严格不相交的扫描行区间，无需像素级互斥锁，输出结果与 [`rasterize_region`] 逐像素 100% 一致；
/// - 在 `#![no_std]` / MCU 或小尺寸局部脏区下零开销直通单线程 [`rasterize_region`]。
pub fn rasterize_region_parallel<P: PixelFormat>(
    ops: &[DrawOp<'_>],
    dirty_region: Region,
    buffer_origin_x: i16,
    buffer_origin_y: i16,
    buffer_stride: usize,
    target: &mut [P],
) {
    if dirty_region.is_empty() || buffer_stride == 0 || ops.is_empty() {
        return;
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let area = (dirty_region.w.max(0) as usize) * (dirty_region.h.max(0) as usize);
        if dirty_region.h >= 32
            && area >= 65_536
            && can_rasterize_parallel(ops)
        {
            if parallel_pool::dispatch_parallel_strips(
                ops,
                dirty_region,
                buffer_origin_x,
                buffer_origin_y,
                buffer_stride,
                target,
            ) {
                return;
            }
        }
    }

    rasterize_region(
        ops,
        dirty_region,
        buffer_origin_x,
        buffer_origin_y,
        buffer_stride,
        target,
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn can_rasterize_parallel(ops: &[DrawOp<'_>]) -> bool {
    !ops_have_layers_or_blend(ops)
        && !ops.iter().any(|op| match op.payload {
            DrawPayload::Sprite(SpriteSource::Shader { .. }) => true,
            DrawPayload::SceneCanvas(spec) => {
                spec.software_bg_fallback.is_some() || spec.software_fg_fallback.is_some()
            }
            _ => false,
        })
}

#[cfg(not(target_arch = "wasm32"))]
mod parallel_pool {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::boxed::Box;
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
    use std::sync::{Arc, Condvar, Mutex, OnceLock};
    use std::vec::Vec;

    /// 单个并行光栅化批次描述符（类型擦除像素函数指针以支持任意 `P: PixelFormat`）。
    #[derive(Copy, Clone)]
    struct BatchJob {
        ops_ptr: usize,
        ops_len: usize,
        occlusion_ptr: usize,
        precomputed_text_ptr: usize,
        precomputed_text_len: usize,
        viewport: Region,
        scale_factor: f32,
        dirty_region: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target_ptr: usize,
        target_len: usize,
        strip_h: i16,
        total_strips: usize,
        strip_fn: unsafe fn(&BatchJob, usize),
    }

    unsafe fn run_strip_typed<P: PixelFormat>(job: &BatchJob, strip_idx: usize) {
        let y0 = job
            .dirty_region
            .y
            .saturating_add((strip_idx as i16).saturating_mul(job.strip_h));
        let y1 = y0
            .saturating_add(job.strip_h)
            .min(job.dirty_region.bottom());
        if y0 >= y1 {
            return;
        }
        let strip_reg = Region::new(job.dirty_region.x, y0, job.dirty_region.w, y1 - y0);
        let row_start = ((y0 - job.origin_y).max(0) as usize) * job.stride;
        let row_end = (((y1 - job.origin_y).max(0) as usize) * job.stride).min(job.target_len);
        if row_start >= row_end {
            return;
        }
        // SAFETY:
        // 1. `ops_ptr..ops_len` 在整次 `dispatch_parallel_strips` 阻塞期间有效且不含 `SpriteSource::Shader`，纯只读；
        // 2. 每个 `strip_idx` 对应的 `[row_start..row_end)` 扫描行区间严格互不相交，任意两个线程绝不会访问重叠的像素槽位。
        unsafe {
            let ops: &[DrawOp<'static>] =
                core::slice::from_raw_parts(job.ops_ptr as *const DrawOp<'static>, job.ops_len);
            let strip_target: &mut [P] = core::slice::from_raw_parts_mut(
                (job.target_ptr as *mut P).add(row_start),
                row_end - row_start,
            );
            let occlusion = (job.occlusion_ptr as *const SpatialOcclusionGrid).as_ref();
            let precomputed_text = if job.precomputed_text_ptr != 0 && job.precomputed_text_len > 0 {
                Some(core::slice::from_raw_parts(
                    job.precomputed_text_ptr as *const Option<PrecomputedScaledTextBox>,
                    job.precomputed_text_len,
                ))
            } else {
                None
            };
            if job.scale_factor > 0.0 {
                rasterize_region_scaled(
                    ops,
                    job.viewport,
                    job.scale_factor,
                    strip_reg,
                    job.origin_x,
                    y0,
                    job.stride,
                    strip_target,
                    occlusion,
                    precomputed_text,
                );
            } else {
                rasterize_region_inner(
                    ops,
                    strip_reg,
                    job.origin_x,
                    y0,
                    job.stride,
                    strip_target,
                    occlusion,
                );
            }
        }
    }

    struct PoolShared {
        state: Mutex<(u64, Option<BatchJob>)>,
        work_cv: Condvar,
        done_mutex: Mutex<usize>,
        done_cv: Condvar,
        next_strip: AtomicUsize,
        num_workers: AtomicUsize,
        panics: Mutex<Vec<Box<dyn core::any::Any + Send>>>,
    }

    fn process_strips(shared: &PoolShared, job: &BatchJob) {
        let result = catch_unwind(AssertUnwindSafe(|| loop {
            let idx = shared.next_strip.fetch_add(1, Ordering::Relaxed);
            if idx >= job.total_strips {
                break;
            }
            // The dispatching thread waits for every participant before returning.
            unsafe { (job.strip_fn)(job, idx) };
        }));
        if let Err(payload) = result {
            shared.next_strip.fetch_max(job.total_strips, Ordering::Relaxed);
            let mut panics = shared.panics.lock().unwrap_or_else(|e| e.into_inner());
            panics.push(payload);
        }
    }

    struct WorkerDoneGuard<'a>(&'a PoolShared);
    impl Drop for WorkerDoneGuard<'_> {
        fn drop(&mut self) {
            let mut rem = self.0.done_mutex.lock().unwrap_or_else(|e| e.into_inner());
            *rem = rem.saturating_sub(1);
            if *rem == 0 {
                self.0.done_cv.notify_one();
            }
        }
    }

    static POOL: OnceLock<Arc<PoolShared>> = OnceLock::new();
    /// 防止递归重入并行池的互斥锁（若多线程同时调用引擎，后来者回退单线程或串行使用池）。
    static DISPATCH_LOCK: Mutex<()> = Mutex::new(());

    fn get_pool() -> &'static Arc<PoolShared> {
        POOL.get_or_init(|| {
            let total_threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(1, 4);
            let target_workers = total_threads.saturating_sub(1);
            let shared = Arc::new(PoolShared {
                state: Mutex::new((0, None)),
                work_cv: Condvar::new(),
                done_mutex: Mutex::new(0),
                done_cv: Condvar::new(),
                next_strip: AtomicUsize::new(0),
                num_workers: AtomicUsize::new(0),
                panics: Mutex::new(Vec::new()),
            });

            let mut spawned = 0usize;
            for w_idx in 0..target_workers {
                let sh = Arc::clone(&shared);
                if std::thread::Builder::new()
                    .name(std::format!("velox-raster-{w_idx}"))
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        let mut seen_gen = 0u64;
                        loop {
                            let job = {
                                let mut guard =
                                    sh.state.lock().unwrap_or_else(|e| e.into_inner());
                                while guard.0 == seen_gen {
                                    guard =
                                        sh.work_cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                                }
                                seen_gen = guard.0;
                                guard.1
                            };
                            if let Some(j) = job {
                                let _done_guard = WorkerDoneGuard(&sh);
                                process_strips(&sh, &j);
                            }
                        }
                    })
                    .is_ok()
                {
                    spawned += 1;
                }
            }
            shared.num_workers.store(spawned, Ordering::Release);
            shared
        })
    }

    pub fn worker_threads_count() -> usize {
        get_pool().num_workers.load(Ordering::Acquire) + 1
    }

    pub fn dispatch_parallel_strips<P: PixelFormat>(
        ops: &[DrawOp<'_>],
        dirty_region: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) -> bool {
        dispatch_parallel_strips_inner(
            ops,
            Region::EMPTY,
            0.0,
            dirty_region,
            origin_x,
            origin_y,
            stride,
            target,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_parallel_strips_scaled<P: PixelFormat>(
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        dirty_region_phys: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) -> bool {
        dispatch_parallel_strips_inner(
            ops,
            viewport,
            scale_factor,
            dirty_region_phys,
            origin_x,
            origin_y,
            stride,
            target,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_parallel_strips_inner<P: PixelFormat>(
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        dirty_region: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) -> bool {
        if stride == 0 || target.is_empty() {
            return false;
        }
        let buf_w = stride.min(i16::MAX as usize) as i16;
        let buf_h = (target.len() / stride).min(i16::MAX as usize) as i16;
        let dirty_region = dirty_region.intersect(Region::new(origin_x, origin_y, buf_w, buf_h));
        if dirty_region.is_empty() {
            return true;
        }

        let pool = get_pool();
        let num_workers = pool.num_workers.load(Ordering::Acquire);
        if num_workers == 0 {
            return false;
        }
        let Ok(_dispatch_guard) = DISPATCH_LOCK.try_lock() else {
            return false;
        };

        // 预热当前批次所有 TextRun 的 TrueType 字形到全局缓存（精确对齐 0.25px 量化字号 fs_q4），
        // 既使各工作线程直接走读锁/TLS 命中，又让随后热升舱接管的 GPU 管线 100% 零重光栅复用字形缓存
        if crate::display::ttf_atlas::has_system_font() {
            let sf = if scale_factor > 0.0 {
                scale_factor.clamp(0.5, 4.0)
            } else {
                1.0
            };
            for op in ops {
                if let DrawPayload::TextRun {
                    text, font_size, ..
                } = op.payload
                {
                    let logical_fs = font_size.max(8);
                    let fs_q4 = (((logical_fs as f32) * sf).clamp(8.0, 255.0) * 4.0)
                        .round()
                        .clamp(32.0, 1020.0) as u16;
                    for ch in text.as_str().chars() {
                        if ch != ' ' {
                            let _ = crate::display::ttf_atlas::get_cached_glyph_q4(ch, fs_q4);
                        }
                    }
                }
            }
        }

        let strip_h: i16 = 16;
        let total_strips = (dirty_region.h as usize).div_ceil(strip_h as usize);
        if total_strips <= 1 {
            return false;
        }

        with_spatial_occlusion(ops, |occlusion| {
            let precomputed_text = if scale_factor > 0.0 {
                precompute_scaled_text_boxes(ops, viewport, scale_factor.clamp(0.5, 4.0), occlusion)
            } else {
                Vec::new()
            };
            let job = BatchJob {
                ops_ptr: ops.as_ptr() as usize,
                ops_len: ops.len(),
                occlusion_ptr: occlusion.map_or(0, |grid| core::ptr::from_ref(grid) as usize),
                precomputed_text_ptr: if precomputed_text.is_empty() {
                    0
                } else {
                    precomputed_text.as_ptr() as usize
                },
                precomputed_text_len: precomputed_text.len(),
                viewport,
                scale_factor,
                dirty_region,
                origin_x,
                origin_y,
                stride,
                target_ptr: target.as_mut_ptr() as usize,
                target_len: target.len(),
                strip_h,
                total_strips,
                strip_fn: run_strip_typed::<P>,
            };

            pool.next_strip.store(0, Ordering::Release);
            {
                let mut rem = pool.done_mutex.lock().unwrap_or_else(|e| e.into_inner());
                *rem = num_workers;
            }
            {
                let mut st = pool.state.lock().unwrap_or_else(|e| e.into_inner());
                st.0 = st.0.wrapping_add(1);
                st.1 = Some(job);
                pool.work_cv.notify_all();
            }

            process_strips(pool, &job);
            {
                let mut rem = pool.done_mutex.lock().unwrap_or_else(|e| e.into_inner());
                while *rem > 0 {
                    rem = pool.done_cv.wait(rem).unwrap_or_else(|e| e.into_inner());
                }
            }
            pool.state.lock().unwrap_or_else(|e| e.into_inner()).1 = None;
            let mut panics = pool.panics.lock().unwrap_or_else(|e| e.into_inner());
            if !panics.is_empty() {
                let mut failures = core::mem::take(&mut *panics);
                drop(panics);
                drop(_dispatch_guard);
                if let Some(payload) = failures.pop() {
                    // Discard other errors only after no worker can access the buffers.
                    drop(failures);
                    resume_unwind(payload);
                }
            }
        });
        true
    }
}
