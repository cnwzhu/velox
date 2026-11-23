//! 位图、RLE 掩码、旋转精灵、过程化 SDF 与纯整数解析圆弧光栅化。

use crate::display::{PixelFormat, Rgb565, SpriteFormat};
use crate::element::SpriteSource;
use crate::sdf::SurfacePainter;
use crate::solver::Region;
use crate::style::Color;

use super::text::rasterize_text;

struct SliceSurface<'t, P: PixelFormat> {
    bounds: Region,
    clip: Region,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &'t mut [P],
}

impl<'t, P: PixelFormat> SurfacePainter for SliceSurface<'t, P> {
    #[inline(always)]
    fn bounds(&self) -> Region {
        self.bounds
    }

    #[inline(always)]
    fn clip(&self) -> Region {
        self.clip
    }

    #[inline(always)]
    fn blend_pixel(&mut self, px: i16, py: i16, color: Color, alpha: u8) {
        if alpha == 0 {
            return;
        }
        let row_offset = ((py - self.origin_y) as usize) * self.stride;
        let dst_x = (px - self.origin_x) as usize;
        let idx = row_offset + dst_x;
        if let Some(slot) = self.target.get_mut(idx) {
            *slot = slot.blend(color, alpha);
        }
    }

    fn draw_text(&mut self, bounds: Region, text: &str, color: Color, font_size: u8) {
        let clip_sub = self.clip.intersect(bounds);
        if !clip_sub.is_empty() {
            rasterize_text(
                bounds,
                clip_sub,
                text,
                color,
                font_size,
                1, // 默认居中对齐
                self.origin_x,
                self.origin_y,
                self.stride,
                self.target,
            );
        }
    }
}


#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_sprite_source<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    sprite: SpriteSource<'_>,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let eff_clip = clip.intersect(bounds);
    if eff_clip.is_empty() || stride == 0 {
        return;
    }
    match sprite {
        SpriteSource::Bitmap {
            bytes,
            format,
            tint,
        } => {
            rasterize_sprite(
                bounds, eff_clip, bytes, format, tint, origin_x, origin_y, stride, target,
            );
        }
        SpriteSource::Shader { painter, .. } => {
            let mut surface = SliceSurface {
                bounds,
                clip: eff_clip,
                origin_x,
                origin_y,
                stride,
                target,
            };
            painter(&mut surface);
        }
        SpriteSource::Svg {
            source,
            is_path_only,
            tint,
            stroke_width_x10,
        } => {
            if is_path_only {
                crate::svg::rasterize_svg_path(
                    source.as_str(),
                    bounds,
                    eff_clip,
                    tint,
                    stroke_width_x10,
                    origin_x,
                    origin_y,
                    stride,
                    target,
                );
            } else {
                let options = crate::svg::SvgRenderOptions {
                    tint,
                    stroke_width_override: if stroke_width_x10 > 0 {
                        Some((stroke_width_x10 as f32) * 0.1)
                    } else {
                        None
                    },
                };
                crate::svg::rasterize_svg(
                    source.as_str(),
                    bounds,
                    eff_clip,
                    options,
                    origin_x,
                    origin_y,
                    stride,
                    target,
                );
            }
        }
        SpriteSource::Arc {
            thickness,
            start_deg10,
            sweep_deg10,
            fill_sweep_deg10,
            track_color,
            fill_color,
        } => {
            rasterize_arc_sdf(
                bounds,
                eff_clip,
                thickness,
                start_deg10,
                sweep_deg10,
                fill_sweep_deg10,
                track_color,
                fill_color,
                origin_x,
                origin_y,
                stride,
                target,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_arc_sdf<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    thickness: u8,
    start_deg10: u16,
    sweep_deg10: u16,
    fill_sweep_deg10: u16,
    track_color: Color,
    fill_color: Color,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    {
        rasterize_arc_sdf_scaled(
            bounds,
            clip,
            (thickness.max(1) as f32).max(1.5),
            start_deg10,
            sweep_deg10,
            fill_sweep_deg10,
            track_color,
            fill_color,
            origin_x,
            origin_y,
            stride,
            target,
        );
    }

}

/// 与 `wgpu` `shader.wgsl::sdf_arc_round` 逐像素等价的解析圆弧 SDF 光栅化（`std` 下软件首屏与 GPU 零跳变对齐）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_arc_sdf_scaled<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    thickness_px: f32,
    start_deg10: u16,
    sweep_deg10: u16,
    fill_sweep_deg10: u16,
    track_color: Color,
    fill_color: Color,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let eff_clip = clip.intersect(bounds);
    if eff_clip.is_empty() || bounds.w <= 0 || bounds.h <= 0 || stride == 0 {
        return;
    }

    let half_w = (bounds.w as f32) * 0.5;
    let half_h = (bounds.h as f32) * 0.5;
    let cx = (bounds.x as f32) + half_w;
    let cy = (bounds.y as f32) + half_h;
    let r_max = half_w.min(half_h);
    let half_thick = (thickness_px * 0.5).clamp(0.75, r_max * 0.45);
    let r_arc = (r_max - half_thick - 1.0).max(1.0);
    let r_inner = (r_arc - half_thick - 1.0).max(0.0);
    let r_outer = r_arc + half_thick + 1.0;

    const DEG10_TO_RAD: f32 = core::f32::consts::PI / 1800.0;
    let start_rad = ((start_deg10 % 3600) as f32) * DEG10_TO_RAD;
    let sweep_rad = (sweep_deg10.clamp(20, 3590) as f32) * DEG10_TO_RAD;
    let fill_rad = (fill_sweep_deg10.min(sweep_deg10) as f32) * DEG10_TO_RAD;

    let cap_a = (start_rad.sin() * r_arc, -start_rad.cos() * r_arc);

    let track_half = sweep_rad * 0.5;
    let track_mid = start_rad + track_half;
    let track_dir = (track_mid.sin(), -track_mid.cos());
    let track_cos = track_half.cos();
    let track_end = start_rad + sweep_rad;
    let track_cap_b = (track_end.sin() * r_arc, -track_end.cos() * r_arc);

    let fill_half = fill_rad * 0.5;
    let fill_mid = start_rad + fill_half;
    let fill_dir = (fill_mid.sin(), -fill_mid.cos());
    let fill_cos = fill_half.cos();
    let fill_end = start_rad + fill_rad;
    let fill_cap_b = (fill_end.sin() * r_arc, -fill_end.cos() * r_arc);

    #[inline(always)]
    fn sdf_arc_eval(
        rx: f32,
        ry: f32,
        r_dist: f32,
        r_arc: f32,
        half_thick: f32,
        dir_mid: (f32, f32),
        cos_half: f32,
        cap_a: (f32, f32),
        cap_b: (f32, f32),
    ) -> f32 {
        if r_dist > 0.0001 && (rx * dir_mid.0 + ry * dir_mid.1) >= r_dist * cos_half {
            return (r_dist - r_arc).abs() - half_thick;
        }
        let dax = rx - cap_a.0;
        let day = ry - cap_a.1;
        let dbx = rx - cap_b.0;
        let dby = ry - cap_b.1;
        let da2 = dax * dax + day * day;
        let db2 = dbx * dbx + dby * dby;
        da2.min(db2).sqrt() - half_thick
    }

    let draw_track = track_color.a() > 0 && sweep_rad > 0.001;
    let draw_fill = fill_color.a() > 0 && fill_rad > 0.001;
    if !draw_track && !draw_fill {
        return;
    }

    for py in eff_clip.y..eff_clip.bottom() {
        let ry = (py as f32) + 0.5 - cy;
        let ry2 = ry * ry;
        let row_offset = ((py - origin_y) as usize) * stride;
        for px in eff_clip.x..eff_clip.right() {
            let rx = (px as f32) + 0.5 - cx;
            let r_dist = (rx * rx + ry2).sqrt();
            if r_dist < r_inner || r_dist > r_outer {
                continue;
            }
            let idx = row_offset + ((px - origin_x) as usize);
            let Some(dst) = target.get_mut(idx) else {
                continue;
            };
            if draw_track {
                let sd_t = sdf_arc_eval(
                    rx,
                    ry,
                    r_dist,
                    r_arc,
                    half_thick,
                    track_dir,
                    track_cos,
                    cap_a,
                    track_cap_b,
                );
                let cov_t = (0.5 - sd_t).clamp(0.0, 1.0);
                if cov_t > 0.0 {
                    let a_u8 = (cov_t * 255.0 + 0.5) as u8;
                    if a_u8 > 0 {
                        *dst = dst.blend(track_color, a_u8);
                    }
                }
            }
            if draw_fill {
                let sd_f = sdf_arc_eval(
                    rx,
                    ry,
                    r_dist,
                    r_arc,
                    half_thick,
                    fill_dir,
                    fill_cos,
                    cap_a,
                    fill_cap_b,
                );
                let cov_f = (0.5 - sd_f).clamp(0.0, 1.0);
                if cov_f > 0.0 {
                    let a_u8 = (cov_f * 255.0 + 0.5) as u8;
                    if a_u8 > 0 {
                        *dst = dst.blend(fill_color, a_u8);
                    }
                }
            }
        }
    }
}


#[inline]
fn infer_sprite_scale(bounds: Region, total_pixels: usize) -> usize {
    let bw = bounds.w.max(1) as usize;
    let bh = bounds.h.max(1) as usize;
    let area = bw.saturating_mul(bh);
    if total_pixels > 0 && area > total_pixels && area % total_pixels == 0 {
        match area / total_pixels {
            4 => 2,
            9 => 3,
            16 => 4,
            _ => 1,
        }
    } else {
        1
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rasterize_sprite<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    bytes: &[u8],
    format: SpriteFormat,
    tint: Color,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if tint.a() == 0 {
        return;
    }
    let w = bounds.w.max(1) as usize;

    let total_px = match format {
        SpriteFormat::A8 => bytes.len(),
        SpriteFormat::A4 => bytes.len() * 2,
        SpriteFormat::Rgb565 => bytes.len() / 2,
        SpriteFormat::Rgba8888 => bytes.len() / 4,
    };
    let scale = infer_sprite_scale(bounds, total_px);
    let src_w = (w / scale).max(1);

    for py in clip.y..clip.bottom() {
        let sy = ((py - bounds.y) as usize) / scale;
        let row_offset = ((py - origin_y) as usize) * stride;
        for px in clip.x..clip.right() {
            let sx = ((px - bounds.x) as usize) / scale;
            let pixel_idx = sy * src_w + sx;
            let dst_idx = row_offset + ((px - origin_x) as usize);
            let Some(dst) = target.get_mut(dst_idx) else {
                continue;
            };
            match format {
                SpriteFormat::A8 => {
                    if let Some(&cov) = bytes.get(pixel_idx) {
                        if cov > 0 {
                            *dst = dst.blend(tint, cov);
                        }
                    }
                }
                SpriteFormat::A4 => {
                    if let Some(&byte) = bytes.get(pixel_idx >> 1) {
                        let nibble = if (pixel_idx & 1) == 0 {
                            byte >> 4
                        } else {
                            byte & 0x0F
                        };
                        let cov = (nibble << 4) | nibble;
                        if cov > 0 {
                            *dst = dst.blend(tint, cov);
                        }
                    }
                }
                SpriteFormat::Rgb565 => {
                    let b_idx = pixel_idx * 2;
                    if let Some(pair) = bytes.get(b_idx..b_idx + 2) {
                        let raw = u16::from_le_bytes([pair[0], pair[1]]);
                        let c = Rgb565(raw).to_color();
                        if tint.a() == 255 {
                            *dst = P::from_color(c);
                        } else {
                            *dst = dst.blend(c, tint.a());
                        }
                    }
                }
                SpriteFormat::Rgba8888 => {
                    let b_idx = pixel_idx * 4;
                    if let Some(rgba) = bytes.get(b_idx..b_idx + 4) {
                        let a = (((rgba[3] as u16) * (tint.a() as u16) + 127) / 255) as u8;
                        if a == 255 {
                            *dst = P::from_color(Color::rgb(rgba[0], rgba[1], rgba[2]));
                        } else if a > 0 {
                            // 兼容预乘（rgba[c] <= rgba[3]）与直通 RGBA 输入
                            let is_premul = rgba[0] <= rgba[3]
                                && rgba[1] <= rgba[3]
                                && rgba[2] <= rgba[3];
                            if is_premul {
                                let bg = dst.to_color();
                                let inv = (255 - a) as u16;
                                let scale_a = tint.a() as u16;
                                let sr = ((rgba[0] as u16) * scale_a + 127) / 255;
                                let sg = ((rgba[1] as u16) * scale_a + 127) / 255;
                                let sb = ((rgba[2] as u16) * scale_a + 127) / 255;
                                let out_a =
                                    ((a as u16) + (((bg.a() as u16) * inv + 127) / 255)).min(255)
                                        as u8;
                                let r =
                                    (sr + (((bg.r() as u16) * inv + 127) / 255)).min(255) as u8;
                                let g =
                                    (sg + (((bg.g() as u16) * inv + 127) / 255)).min(255) as u8;
                                let b =
                                    (sb + (((bg.b() as u16) * inv + 127) / 255)).min(255) as u8;
                                *dst = P::from_color(Color::rgba(r, g, b, out_a));
                            } else {
                                *dst = dst.blend(Color::rgb(rgba[0], rgba[1], rgba[2]), a);
                            }
                        }
                    }
                }
            }
        }
    }
}

