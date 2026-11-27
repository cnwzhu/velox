//! 软件光栅高级效果引擎：
//! - 图元级：四角独立圆角 SDF、多色标 Linear/Radial/Conic 渐变（含 OKLab / Linear RGB 与 ExtendMode）、2D 仿射变换、27 种混合模式与防色带抖动；
//! - 离屏合成层：`PushLayer` / `PopLayer` 嵌套图层栈、组级隔离透明度、3-Pass $O(1)$ 高斯近似模糊、实时毛玻璃（`backdrop-filter`）、4×5 `ColorMatrix`、任意子树 `DropShadow` 与 SVG Path 裁剪遮罩。

use crate::display::PixelFormat;
use crate::motion::{DrawOp, DrawPayload};
use crate::paint::{
    BlendMode, Paint, StyledQuad, bake_gradient_lut, blend_premul_f32, color_to_premul_f32, premul_f32_to_color,
};
use crate::solver::Region;
use crate::style::Color;
use crate::svg::math::{abs_f32, floor_f32, sqrt_f32};

/// 四角独立圆角矩形的有向距离场（SDF，局部浮点坐标 `lx, ly`，四角顺序 `[tl, tr, br, bl]`）。
#[inline(always)]
pub(crate) fn sdf_rounded_box_4_f32(
    lx: f32,
    ly: f32,
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    radii: [f32; 4],
) -> f32 {
    let px = lx - cx;
    let py = ly - cy;
    let raw_r = if px < 0.0 {
        if py < 0.0 {
            radii[0] // tl
        } else {
            radii[3] // bl
        }
    } else if py < 0.0 {
        radii[1] // tr
    } else {
        radii[2] // br
    };
    let r = raw_r.clamp(0.0, hw.min(hh));
    let qx = abs_f32(px) - (hw - r);
    let qy = abs_f32(py) - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    sqrt_f32(ox * ox + oy * oy) + qx.max(qy).min(0.0) - r
}

/// Interleaved Gradient Noise（Jimenez 2014）：返回 `[-0.5, +0.5]` 用于 1-LSB 消除渐变 8-bit 色带。
#[inline(always)]
fn ign_dither(px: i16, py: i16) -> f32 {
    let x = px as f32;
    let y = py as f32;
    let t = 0.067_110_56 * x + 0.005_837_15 * y;
    let f1 = t - floor_f32(t);
    let v = 52.982_918 * f1;
    (v - floor_f32(v)) - 0.5
}

/// 从已烘焙的 1D 渐变查找表中执行线性插值采样（返回 sRGB 直通 `Color`）。
#[inline(always)]
fn sample_lut<const N: usize>(lut: &[[u8; 4]; N], t: f32, dither: f32) -> Color {
    let max_idx = (N - 1) as f32;
    let pos = (t.clamp(0.0, 1.0)) * max_idx;
    let i0 = (pos as usize).min(N - 1);
    let i1 = (i0 + 1).min(N - 1);
    let frac = pos - (i0 as f32);
    let inv = 1.0 - frac;
    let c0 = lut[i0];
    let c1 = lut[i1];
    let r = (c0[0] as f32 * inv + c1[0] as f32 * frac + dither).clamp(0.0, 255.0) as u8;
    let g = (c0[1] as f32 * inv + c1[1] as f32 * frac + dither).clamp(0.0, 255.0) as u8;
    let b = (c0[2] as f32 * inv + c1[2] as f32 * frac + dither).clamp(0.0, 255.0) as u8;
    let a = (c0[3] as f32 * inv + c1[3] as f32 * frac + 0.5).clamp(0.0, 255.0) as u8;
    Color::rgba(r, g, b, a)
}

/// 按给定 [`BlendMode`] 与抗锯齿覆盖率 `cov ∈ [0.0, 1.0]` 将直通源色 `src` 合成到任意 `PixelFormat` 目标像素上。
#[inline(always)]
fn composite_pixel<P: PixelFormat>(dst: &mut P, src_pm: [f32; 4], mode: BlendMode) {
    if mode.is_normal() {
        let sa = src_pm[3].clamp(0.0, 1.0);
        if sa <= 1e-5 {
            return;
        }
        if sa >= 0.9999 {
            *dst = P::from_color(premul_f32_to_color(src_pm));
            return;
        }
        let dst_pm = color_to_premul_f32(dst.to_color());
        let inv = 1.0 - sa;
        let out = [
            src_pm[0] + dst_pm[0] * inv,
            src_pm[1] + dst_pm[1] * inv,
            src_pm[2] + dst_pm[2] * inv,
            sa + dst_pm[3] * inv,
        ];
        *dst = P::from_color(premul_f32_to_color(out));
    } else {
        let dst_pm = color_to_premul_f32(dst.to_color());
        let out = blend_premul_f32(src_pm, dst_pm, mode);
        *dst = P::from_color(premul_f32_to_color(out));
    }
}

/// 光栅化高级 [`StyledQuad`]：
/// 支持四角独立圆角、多色标 Linear/Radial/Conic 渐变（含 OKLab / Linear RGB）、2D 仿射变换、边框与 27 种混合模式。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_styled_quad<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    sq: &StyledQuad<'_>,
    scale_factor: f32,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if bounds.w <= 0 || bounds.h <= 0 || clip.is_empty() || stride == 0 {
        return;
    }

    let sf = scale_factor.clamp(0.5, 4.0);
    let radii = [
        (sq.radii.tl as f32) * sf,
        (sq.radii.tr as f32) * sf,
        (sq.radii.br as f32) * sf,
        (sq.radii.bl as f32) * sf,
    ];
    let bw = if sq.border_width == 0 {
        0.0
    } else {
        floor_f32((sq.border_width as f32) * sf + 0.5).max(1.0)
    };
    let has_border = bw > 0.0 && sq.border_color.a() > 0;
    let border_pm = if has_border {
        color_to_premul_f32(sq.border_color)
    } else {
        [0.0; 4]
    };

    let hw = (bounds.w as f32) * 0.5;
    let hh = (bounds.h as f32) * 0.5;
    let cx = (bounds.x as f32) + hw;
    let cy = (bounds.y as f32) + hh;
    let inv_w = 1.0 / (bounds.w.max(1) as f32);
    let inv_h = 1.0 / (bounds.h.max(1) as f32);

    // 若携带 2D 仿射变换，以当前物理包围盒中心为原点缩放平移分量并求逆矩阵
    let (draw_clip, inv_xform) = if let Some(mut t) = sq.transform {
        if !t.is_identity() {
            t.e *= sf;
            t.f *= sf;
            let screen_t = t.centered_at(cx, cy);
            let Some(inv) = screen_t.invert() else {
                return;
            };
            let xformed = screen_t.transform_region(bounds);
            let eff = xformed.intersect(clip);
            if eff.is_empty() {
                return;
            }
            (eff, Some(inv))
        } else {
            let eff = bounds.intersect(clip);
            if eff.is_empty() {
                return;
            }
            (eff, None)
        }
    } else {
        let eff = bounds.intersect(clip);
        if eff.is_empty() {
            return;
        }
        (eff, None)
    };

    const LUT_N: usize = 256;

    let mut lut = [[0u8; 4]; LUT_N];
    let is_gradient = match sq.paint {
        Paint::Solid(_) => false,
        Paint::LinearGradient { stops, space, .. }
        | Paint::RadialGradient { stops, space, .. }
        | Paint::ConicGradient { stops, space, .. } => {
            bake_gradient_lut(stops, space, sq.opacity, &mut lut);
            true
        }
    };
    let solid_col = match sq.paint {
        Paint::Solid(c) => c.scale_alpha(sq.opacity),
        _ => Color::TRANSPARENT,
    };
    let solid_pm = color_to_premul_f32(solid_col);

    for py in draw_clip.y..draw_clip.bottom() {
        let row_off = ((py - origin_y) as usize) * stride;
        let sy = (py as f32) + 0.5;
        for px in draw_clip.x..draw_clip.right() {
            let sx = (px as f32) + 0.5;
            let (lx, ly) = match inv_xform {
                Some(inv) => inv.apply(sx, sy),
                None => (sx, sy),
            };
            let sd_outer = sdf_rounded_box_4_f32(lx, ly, cx, cy, hw, hh, radii);
            let outer_cov = (0.5 - sd_outer).clamp(0.0, 1.0);
            if outer_cov <= 0.0 {
                continue;
            }

            let fill_pm = if is_gradient {
                let u = (lx - bounds.x as f32) * inv_w;
                let v = (ly - bounds.y as f32) * inv_h;
                if let Some(t) = sq.paint.eval_gradient_t(u, v) {
                    let c = sample_lut(&lut, t, ign_dither(px, py));
                    color_to_premul_f32(c)
                } else {
                    [0.0; 4]
                }
            } else {
                solid_pm
            };

            let src_pm = if has_border {
                let sd_inner = sd_outer + bw;
                let inner_cov = (0.5 - sd_inner).clamp(0.0, 1.0);
                if fill_pm[3] > 0.0 {
                    let inv_ic = 1.0 - inner_cov;
                    [
                        (border_pm[0] * inv_ic + fill_pm[0] * inner_cov) * outer_cov,
                        (border_pm[1] * inv_ic + fill_pm[1] * inner_cov) * outer_cov,
                        (border_pm[2] * inv_ic + fill_pm[2] * inner_cov) * outer_cov,
                        (border_pm[3] * inv_ic + fill_pm[3] * inner_cov) * outer_cov,
                    ]
                } else {
                    let ring_cov = outer_cov * (1.0 - inner_cov);
                    if ring_cov <= 0.0 {
                        continue;
                    }
                    [
                        border_pm[0] * ring_cov,
                        border_pm[1] * ring_cov,
                        border_pm[2] * ring_cov,
                        border_pm[3] * ring_cov,
                    ]
                }
            } else {
                if fill_pm[3] <= 0.0 {
                    continue;
                }
                [
                    fill_pm[0] * outer_cov,
                    fill_pm[1] * outer_cov,
                    fill_pm[2] * outer_cov,
                    fill_pm[3] * outer_cov,
                ]
            };

            let idx = row_off + ((px - origin_x) as usize);
            if let Some(dst) = target.get_mut(idx) {
                composite_pixel(dst, src_pm, sq.blend_mode);
            }
        }
    }
}

// ============================================================================
// `std` 完整离屏合成图层引擎（3-Pass 盒式高斯近似模糊 / 毛玻璃 / ColorMatrix / DropShadow / SVG Path Mask / 27 种混合）
// ============================================================================

mod std_layers {
    use std::vec;
    use std::vec::Vec;

    use crate::display::{PixelFormat, Rgba8888};
    use crate::element::SpriteSource;
    use crate::motion::{DrawOp, DrawPayload};
    use crate::paint::{
        blend_premul_f32, color_to_premul_f32, premul_f32_to_color, LayerSpec,
    };
    use crate::solver::Region;
    use crate::style::Color;
    use crate::svg::math::floor_f32;
    use crate::svg::{rasterize_svg, rasterize_svg_path, SvgRenderOptions};

    use super::{rasterize_styled_quad, sdf_rounded_box_4_f32};
    use crate::renderers::software::compositor::draw_op_with_tiled_rounded_clip;
    use crate::renderers::software::sdf::{
        compute_sdf_coverage_r16, rasterize_quad_precise,
    };
    use crate::renderers::software::sprite::{rasterize_arc_sdf_scaled, rasterize_sprite_source};
    use crate::renderers::software::text::{compute_scaled_text_box, rasterize_text_scaled};
    use crate::renderers::software::{snap_bounds_region_scaled, snap_clip_region_scaled};

    /// 离屏合成专用严格预乘 Alpha 32-bit 像素（内部保存 `[r*a, g*a, b*a, a]`）。
    #[repr(transparent)]
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
    struct PremulRgba(u32);

    impl PremulRgba {
        const TRANSPARENT: Self = Self(0);

        #[inline(always)]
        fn from_premul_f32(pm: [f32; 4]) -> Self {
            let a = (pm[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
            let r = (pm[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
            let g = (pm[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
            let b = (pm[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
            Self((a << 24) | (b << 16) | (g << 8) | r)
        }

        #[inline(always)]
        fn to_premul_f32(self) -> [f32; 4] {
            const INV: f32 = 1.0 / 255.0;
            [
                ((self.0 & 0xFF) as f32) * INV,
                (((self.0 >> 8) & 0xFF) as f32) * INV,
                (((self.0 >> 16) & 0xFF) as f32) * INV,
                (((self.0 >> 24) & 0xFF) as f32) * INV,
            ]
        }
    }

    impl PixelFormat for PremulRgba {
        const BLACK: Self = Self(0xFF00_0000);

        #[inline(always)]
        fn from_color(c: Color) -> Self {
            let a = c.a() as u32;
            if a == 0 {
                return Self::TRANSPARENT;
            }
            if a == 255 {
                return Self(
                    0xFF00_0000
                        | ((c.b() as u32) << 16)
                        | ((c.g() as u32) << 8)
                        | (c.r() as u32),
                );
            }
            let r = (c.r() as u32 * a + 127) / 255;
            let g = (c.g() as u32 * a + 127) / 255;
            let b = (c.b() as u32 * a + 127) / 255;
            Self((a << 24) | (b << 16) | (g << 8) | r)
        }

        #[inline(always)]
        fn to_color(self) -> Color {
            let a = ((self.0 >> 24) & 0xFF) as u32;
            if a == 0 {
                return Color::TRANSPARENT;
            }
            let r = self.0 & 0xFF;
            let g = (self.0 >> 8) & 0xFF;
            let b = (self.0 >> 16) & 0xFF;
            if a == 255 {
                return Color::rgba(r as u8, g as u8, b as u8, 255);
            }
            let ur = ((r * 255 + a / 2) / a).min(255) as u8;
            let ug = ((g * 255 + a / 2) / a).min(255) as u8;
            let ub = ((b * 255 + a / 2) / a).min(255) as u8;
            Color::rgba(ur, ug, ub, a as u8)
        }

        #[inline(always)]
        fn blend(self, fg: Color, coverage: u8) -> Self {
            let sa = (((fg.a() as u32) * (coverage as u32) + 127) / 255) as u32;
            if sa == 0 {
                return self;
            }
            if sa >= 255 {
                return Self(
                    0xFF00_0000
                        | ((fg.b() as u32) << 16)
                        | ((fg.g() as u32) << 8)
                        | (fg.r() as u32),
                );
            }
            let inv = 255 - sa;
            let dr = self.0 & 0xFF;
            let dg = (self.0 >> 8) & 0xFF;
            let db = (self.0 >> 16) & 0xFF;
            let da = (self.0 >> 24) & 0xFF;
            let sr = (fg.r() as u32 * sa + 127) / 255;
            let sg = (fg.g() as u32 * sa + 127) / 255;
            let sb = (fg.b() as u32 * sa + 127) / 255;
            let or = (sr + (dr * inv + 127) / 255).min(255);
            let og = (sg + (dg * inv + 127) / 255).min(255);
            let ob = (sb + (db * inv + 127) / 255).min(255);
            let oa = (sa + (da * inv + 127) / 255).min(255);
            Self((oa << 24) | (ob << 16) | (og << 8) | or)
        }
    }

    /// 3-Pass 滑窗盒式高斯近似模糊（严格 $O(w \cdot h)$，与模糊半径无关；在预乘 Alpha 浮点空间执行，无边缘黑晕）。
    pub(crate) fn blur_premul_in_place(buf: &mut [[f32; 4]], w: usize, h: usize, radius_px: f32) {
        if w == 0 || h == 0 || radius_px < 0.5 {
            return;
        }
        // 标准 W3C 3-box 近似高斯半径换算
        let r = ((radius_px * 0.65).round() as usize).max(1).min(w.max(h));
        let mut tmp = vec![[0.0f32; 4]; w * h];

        for _pass in 0..3 {
            // 水平滑窗
            let win_w = (2 * r + 1) as f32;
            let inv_w = 1.0 / win_w;
            for y in 0..h {
                let row = y * w;
                let first = buf[row];
                let mut acc = [
                    first[0] * (r as f32),
                    first[1] * (r as f32),
                    first[2] * (r as f32),
                    first[3] * (r as f32),
                ];
                for k in 0..=r {
                    let p = buf[row + k.min(w - 1)];
                    acc[0] += p[0];
                    acc[1] += p[1];
                    acc[2] += p[2];
                    acc[3] += p[3];
                }
                for x in 0..w {
                    tmp[row + x] = [
                        acc[0] * inv_w,
                        acc[1] * inv_w,
                        acc[2] * inv_w,
                        acc[3] * inv_w,
                    ];
                    let add_x = (x + r + 1).min(w - 1);
                    let sub_x = x.saturating_sub(r);
                    let pa = buf[row + add_x];
                    let ps = buf[row + sub_x];
                    acc[0] += pa[0] - ps[0];
                    acc[1] += pa[1] - ps[1];
                    acc[2] += pa[2] - ps[2];
                    acc[3] += pa[3] - ps[3];
                }
            }
            // 垂直滑窗
            let win_h = (2 * r + 1) as f32;
            let inv_h = 1.0 / win_h;
            for x in 0..w {
                let first = tmp[x];
                let mut acc = [
                    first[0] * (r as f32),
                    first[1] * (r as f32),
                    first[2] * (r as f32),
                    first[3] * (r as f32),
                ];
                for k in 0..=r {
                    let p = tmp[k.min(h - 1) * w + x];
                    acc[0] += p[0];
                    acc[1] += p[1];
                    acc[2] += p[2];
                    acc[3] += p[3];
                }
                for y in 0..h {
                    buf[y * w + x] = [
                        acc[0] * inv_h,
                        acc[1] * inv_h,
                        acc[2] * inv_h,
                        acc[3] * inv_h,
                    ];
                    let add_y = (y + r + 1).min(h - 1);
                    let sub_y = y.saturating_sub(r);
                    let pa = tmp[add_y * w + x];
                    let ps = tmp[sub_y * w + x];
                    acc[0] += pa[0] - ps[0];
                    acc[1] += pa[1] - ps[1];
                    acc[2] += pa[2] - ps[2];
                    acc[3] += pa[3] - ps[3];
                }
            }
        }
    }

    /// 确定性像素颗粒噪点（供 Acrylic 毛玻璃质感使用，返回 `[-0.5, +0.5]`）。
    #[inline(always)]
    fn acrylic_noise(px: i16, py: i16) -> f32 {
        let mut h = ((px as u16 as u32) << 16) | (py as u16 as u32);
        h ^= h >> 16;
        h = h.wrapping_mul(0x7feb_352d);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846c_a68b);
        h ^= h >> 16;
        ((h & 0xFFFF) as f32) * (1.0 / 65535.0) - 0.5
    }

    /// 预渲染可选 SVG Path 裁剪遮罩（输出 `w × h` 单通道抗锯齿覆盖率 `0.0..=1.0`）。
    /// 支持标准 24×24 SVG Path `d` 字符串，或以 `<svg` 开头的自定义 `viewBox` 矢量文档。
    fn build_svg_clip_mask(path_d: &str, w: usize, h: usize) -> Vec<f32> {
        let mut rgba = vec![Rgba8888(0); w * h];
        let reg = Region::new(0, 0, w as i16, h as i16);
        if path_d.trim_start().starts_with('<') {
            rasterize_svg(
                path_d,
                reg,
                reg,
                SvgRenderOptions::default(),
                0,
                0,
                w,
                &mut rgba,
            );
        } else {
            rasterize_svg_path(path_d, reg, reg, Color::WHITE, 0, 0, 0, w, &mut rgba);
        }
        rgba.into_iter()
            .map(|px| (px.to_color().a() as f32) * (1.0 / 255.0))
            .collect()
    }

    /// 双线性采样预乘离屏缓冲（越界返回全透明）。
    #[inline]
    fn sample_premul_bilinear(buf: &[[f32; 4]], w: usize, h: usize, fx: f32, fy: f32) -> [f32; 4] {
        let x = fx - 0.5;
        let y = fy - 0.5;
        if x < -1.0 || y < -1.0 || x > (w as f32) || y > (h as f32) {
            return [0.0; 4];
        }
        let x0 = floor_f32(x) as isize;
        let y0 = floor_f32(y) as isize;
        let tx = x - (x0 as f32);
        let ty = y - (y0 as f32);
        let fetch = |ix: isize, iy: isize| -> [f32; 4] {
            if ix < 0 || iy < 0 || (ix as usize) >= w || (iy as usize) >= h {
                [0.0; 4]
            } else {
                buf[(iy as usize) * w + (ix as usize)]
            }
        };
        let p00 = fetch(x0, y0);
        let p10 = fetch(x0 + 1, y0);
        let p01 = fetch(x0, y0 + 1);
        let p11 = fetch(x0 + 1, y0 + 1);
        let w00 = (1.0 - tx) * (1.0 - ty);
        let w10 = tx * (1.0 - ty);
        let w01 = (1.0 - tx) * ty;
        let w11 = tx * ty;
        [
            p00[0] * w00 + p10[0] * w10 + p01[0] * w01 + p11[0] * w11,
            p00[1] * w00 + p10[1] * w10 + p01[1] * w01 + p11[1] * w11,
            p00[2] * w00 + p10[2] * w10 + p01[2] * w01 + p11[2] * w11,
            p00[3] * w00 + p10[3] * w10 + p01[3] * w01 + p11[3] * w11,
        ]
    }

    struct ActiveLayer<'a> {
        spec: &'a LayerSpec<'a>,
        /// 未膨胀的图层自身物理包围盒（用于圆角/路径掩码与变换中心）。
        body_phys: Region,
        /// 离屏像素缓冲覆盖的物理坐标矩形（已按滤镜膨胀并限制在画布范围内）。
        buf_rect: Region,
        /// 父级有效物理裁剪矩形。
        parent_clip_phys: Region,
        pixels: Vec<PremulRgba>,
    }

    /// 递归执行包含 `PushLayer` / `PopLayer` 的完整离屏图层合成管道。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rasterize_ops_with_layers<P: PixelFormat>(
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        dirty_phys: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) {
        let sf = scale_factor.clamp(0.5, 4.0);
        let buf_w = stride.min(i16::MAX as usize) as i16;
        let buf_h = (target.len() / stride.max(1)).min(i16::MAX as usize) as i16;
        let buf_bounds = Region::new(origin_x, origin_y, buf_w, buf_h);
        let root_clip = dirty_phys.intersect(buf_bounds);
        if root_clip.is_empty() {
            return;
        }
        let vp_clip_phys = snap_clip_region_scaled(viewport, sf);

        let mut layer_stack: Vec<ActiveLayer<'_>> = Vec::new();

        for op in ops {
            match op.payload {
                DrawPayload::PushLayer(spec) => {
                    let body_phys = snap_bounds_region_scaled(spec.bounds, sf);
                    let parent_clip_phys = snap_clip_region_scaled(op.clip, sf).intersect(buf_bounds);

                    // 1. 先处理实时背景毛玻璃（`backdrop-filter`）：从当前父画布采样、模糊、调色并按圆角/路径掩码写回父画布
                    if !spec.backdrop_filter.is_empty() && !body_phys.is_empty() {
                        let bd = spec.backdrop_filter;
                        let bd_clip = body_phys.intersect(parent_clip_phys).intersect(root_clip);
                        if !bd_clip.is_empty() {
                            let blur_px = (bd.blur_radius as f32) * sf;
                            let pad = (blur_px * 2.0).ceil() as i16;
                            let sample_rect = Region::new(
                                body_phys.x.saturating_sub(pad),
                                body_phys.y.saturating_sub(pad),
                                body_phys.w.saturating_add(pad.saturating_mul(2)),
                                body_phys.h.saturating_add(pad.saturating_mul(2)),
                            )
                            .intersect(buf_bounds);
                            if !sample_rect.is_empty() {
                                let sw = sample_rect.w as usize;
                                let sh = sample_rect.h as usize;
                                let mut bd_buf = vec![[0.0f32; 4]; sw * sh];

                                // 从当前栈顶（父层离屏缓冲或主 `target`）采样已有背景像素
                                if let Some(parent) = layer_stack.last() {
                                    let pw = parent.buf_rect.w as usize;
                                    for py in sample_rect.y..sample_rect.bottom() {
                                        let dy = (py - sample_rect.y) as usize;
                                        if py >= parent.buf_rect.y && py < parent.buf_rect.bottom() {
                                            let sy = (py - parent.buf_rect.y) as usize;
                                            for px in sample_rect.x..sample_rect.right() {
                                                if px >= parent.buf_rect.x && px < parent.buf_rect.right() {
                                                    let sx = (px - parent.buf_rect.x) as usize;
                                                    let dx = (px - sample_rect.x) as usize;
                                                    bd_buf[dy * sw + dx] = parent.pixels[sy * pw + sx].to_premul_f32();
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    for py in sample_rect.y..sample_rect.bottom() {
                                        let dy = (py - sample_rect.y) as usize;
                                        let row = ((py - origin_y) as usize) * stride;
                                        for px in sample_rect.x..sample_rect.right() {
                                            let dx = (px - sample_rect.x) as usize;
                                            let idx = row + ((px - origin_x) as usize);
                                            if let Some(p) = target.get(idx) {
                                                bd_buf[dy * sw + dx] = color_to_premul_f32(p.to_color());
                                            }
                                        }
                                    }
                                }

                                if blur_px >= 0.5 {
                                    blur_premul_in_place(&mut bd_buf, sw, sh, blur_px);
                                }
                                if let Some(cm) = bd.color_matrix {
                                    for px in bd_buf.iter_mut() {
                                        *px = cm.apply_premul(*px);
                                    }
                                }

                                let radii = [
                                    (spec.clip_radii.tl as f32) * sf,
                                    (spec.clip_radii.tr as f32) * sf,
                                    (spec.clip_radii.br as f32) * sf,
                                    (spec.clip_radii.bl as f32) * sf,
                                ];
                                let hw = body_phys.w as f32 * 0.5;
                                let hh = body_phys.h as f32 * 0.5;
                                let cx = body_phys.x as f32 + hw;
                                let cy = body_phys.y as f32 + hh;
                                let path_mask = spec.clip_path.map(|p| {
                                    build_svg_clip_mask(
                                        p.as_str(),
                                        body_phys.w.max(1) as usize,
                                        body_phys.h.max(1) as usize,
                                    )
                                });
                                let tint_pm = color_to_premul_f32(bd.tint);
                                let noise_scale = (bd.noise_amount as f32) * (1.0 / 255.0) * 0.12;
                                let op_f = (spec.opacity as f32) * (1.0 / 255.0);

                                for py in bd_clip.y..bd_clip.bottom() {
                                    let sy = (py - sample_rect.y) as usize;
                                    let ly = (py - body_phys.y) as usize;
                                    for px in bd_clip.x..bd_clip.right() {
                                        let sx = (px - sample_rect.x) as usize;
                                        let lx = (px - body_phys.x) as usize;
                                        let mut cov = (0.5
                                            - sdf_rounded_box_4_f32(
                                                px as f32 + 0.5,
                                                py as f32 + 0.5,
                                                cx,
                                                cy,
                                                hw,
                                                hh,
                                                radii,
                                            ))
                                            .clamp(0.0, 1.0);
                                        if let Some(ref mask) = path_mask {
                                            cov *= mask[ly * (body_phys.w as usize) + lx];
                                        }
                                        cov *= op_f;
                                        if cov <= 0.0 {
                                            continue;
                                        }
                                        let mut col = bd_buf[sy * sw + sx];
                                        if bd.noise_amount > 0 {
                                            let n = acrylic_noise(px, py) * noise_scale;
                                            col[0] = (col[0] + n * col[3]).clamp(0.0, col[3]);
                                            col[1] = (col[1] + n * col[3]).clamp(0.0, col[3]);
                                            col[2] = (col[2] + n * col[3]).clamp(0.0, col[3]);
                                        }
                                        if tint_pm[3] > 0.0 {
                                            let inv_t = 1.0 - tint_pm[3];
                                            col = [
                                                tint_pm[0] + col[0] * inv_t,
                                                tint_pm[1] + col[1] * inv_t,
                                                tint_pm[2] + col[2] * inv_t,
                                                tint_pm[3] + col[3] * inv_t,
                                            ];
                                        }
                                        // 用处理后的毛玻璃背景按 `cov` 插值写回父画布
                                        if let Some(parent) = layer_stack.last_mut() {
                                            if py >= parent.buf_rect.y
                                                && py < parent.buf_rect.bottom()
                                                && px >= parent.buf_rect.x
                                                && px < parent.buf_rect.right()
                                            {
                                                let pw = parent.buf_rect.w as usize;
                                                let idx = ((py - parent.buf_rect.y) as usize) * pw
                                                    + ((px - parent.buf_rect.x) as usize);
                                                let dst = parent.pixels[idx].to_premul_f32();
                                                let inv = 1.0 - cov;
                                                let out = [
                                                    col[0] * cov + dst[0] * inv,
                                                    col[1] * cov + dst[1] * inv,
                                                    col[2] * cov + dst[2] * inv,
                                                    col[3] * cov + dst[3] * inv,
                                                ];
                                                parent.pixels[idx] = PremulRgba::from_premul_f32(out);
                                            }
                                        } else {
                                            let idx = ((py - origin_y) as usize) * stride
                                                + ((px - origin_x) as usize);
                                            if let Some(dst_px) = target.get_mut(idx) {
                                                let dst = color_to_premul_f32(dst_px.to_color());
                                                let inv = 1.0 - cov;
                                                let out = [
                                                    col[0] * cov + dst[0] * inv,
                                                    col[1] * cov + dst[1] * inv,
                                                    col[2] * cov + dst[2] * inv,
                                                    col[3] * cov + dst[3] * inv,
                                                ];
                                                *dst_px = P::from_color(premul_f32_to_color(out));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // 2. 为该图层分配透明离屏缓冲（按 `filter.outset()` 膨胀）
                    let (fl, ft, fr, fb) = spec.filter.outset();
                    let pl = ((fl as f32) * sf).ceil() as i16;
                    let pt = ((ft as f32) * sf).ceil() as i16;
                    let pr = ((fr as f32) * sf).ceil() as i16;
                    let pb = ((fb as f32) * sf).ceil() as i16;
                    let raw_buf = Region::new(
                        body_phys.x.saturating_sub(pl),
                        body_phys.y.saturating_sub(pt),
                        body_phys.w.saturating_add(pl).saturating_add(pr),
                        body_phys.h.saturating_add(pt).saturating_add(pb),
                    );
                    // 若带 transform，离屏绘制保持在未变换局部包围盒内，PopLayer 合成时再应用变换
                    let buf_rect = if raw_buf.w > 0 && raw_buf.h > 0 {
                        raw_buf
                    } else {
                        Region::new(body_phys.x, body_phys.y, 1, 1)
                    };
                    let len = (buf_rect.w.max(1) as usize) * (buf_rect.h.max(1) as usize);
                    layer_stack.push(ActiveLayer {
                        spec,
                        body_phys,
                        buf_rect,
                        parent_clip_phys,
                        pixels: vec![PremulRgba::TRANSPARENT; len],
                    });
                }

                DrawPayload::PopLayer => {
                    let Some(layer) = layer_stack.pop() else {
                        continue;
                    };
                    let lw = layer.buf_rect.w.max(1) as usize;
                    let lh = layer.buf_rect.h.max(1) as usize;
                    let mut pm_buf: Vec<[f32; 4]> =
                        layer.pixels.iter().map(|p| p.to_premul_f32()).collect();

                    // 1. 若有 `clip_path` 或非零圆角且无外溢子内容需要保留，按 SVG Path 裁剪离屏层
                    if let Some(cp) = layer.spec.clip_path {
                        let bw = layer.body_phys.w.max(1) as usize;
                        let bh = layer.body_phys.h.max(1) as usize;
                        let mask = build_svg_clip_mask(cp.as_str(), bw, bh);
                        for ly in 0..lh {
                            let gy = layer.buf_rect.y + ly as i16;
                            for lx in 0..lw {
                                let gx = layer.buf_rect.x + lx as i16;
                                let cov = if gx >= layer.body_phys.x
                                    && gx < layer.body_phys.right()
                                    && gy >= layer.body_phys.y
                                    && gy < layer.body_phys.bottom()
                                {
                                    let mx = (gx - layer.body_phys.x) as usize;
                                    let my = (gy - layer.body_phys.y) as usize;
                                    mask[my * bw + mx]
                                } else {
                                    0.0
                                };
                                let p = &mut pm_buf[ly * lw + lx];
                                p[0] *= cov;
                                p[1] *= cov;
                                p[2] *= cov;
                                p[3] *= cov;
                            }
                        }
                    }

                    let opacity_f = (layer.spec.opacity as f32) * (1.0 / 255.0);

                    // 2. 若启用任意形状 `drop_shadow`：提取离屏层 Alpha 轮廓 -> 模糊 -> 偏移着色先合成到父画布
                    if let Some(ds) = layer.spec.filter.drop_shadow {
                        if ds.is_active() {
                            let s_col = color_to_premul_f32(ds.color);
                            let s_alpha = (ds.color.a() as f32) * (1.0 / 255.0);
                            let mut sh_buf: Vec<[f32; 4]> = pm_buf
                                .iter()
                                .map(|p| {
                                    let a = p[3] * s_alpha;
                                    if s_alpha > 1e-5 {
                                        [
                                            (s_col[0] / s_alpha) * a,
                                            (s_col[1] / s_alpha) * a,
                                            (s_col[2] / s_alpha) * a,
                                            a,
                                        ]
                                    } else {
                                        [0.0; 4]
                                    }
                                })
                                .collect();
                            let ds_blur = (ds.blur as f32) * sf;
                            if ds_blur >= 0.5 {
                                blur_premul_in_place(&mut sh_buf, lw, lh, ds_blur);
                            }
                            let ox = ((ds.offset_x as f32) * sf).round() as i16;
                            let oy = ((ds.offset_y as f32) * sf).round() as i16;
                            let dst_rect = Region::new(
                                layer.buf_rect.x.saturating_add(ox),
                                layer.buf_rect.y.saturating_add(oy),
                                layer.buf_rect.w,
                                layer.buf_rect.h,
                            )
                            .intersect(layer.parent_clip_phys)
                            .intersect(root_clip);

                            for py in dst_rect.y..dst_rect.bottom() {
                                let sy = (py - (layer.buf_rect.y + oy)) as usize;
                                for px in dst_rect.x..dst_rect.right() {
                                    let sx = (px - (layer.buf_rect.x + ox)) as usize;
                                    let mut src = sh_buf[sy * lw + sx];
                                    if src[3] <= 1e-5 {
                                        continue;
                                    }
                                    src[0] *= opacity_f;
                                    src[1] *= opacity_f;
                                    src[2] *= opacity_f;
                                    src[3] *= opacity_f;
                                    write_to_parent(
                                        &mut layer_stack,
                                        target,
                                        origin_x,
                                        origin_y,
                                        stride,
                                        px,
                                        py,
                                        src,
                                        layer.spec.blend_mode,
                                    );
                                }
                            }
                        }
                    }

                    // 3. 对离屏层本体应用高斯模糊与 `ColorMatrix`
                    let blur_px = (layer.spec.filter.blur_radius as f32) * sf;
                    if blur_px >= 0.5 {
                        blur_premul_in_place(&mut pm_buf, lw, lh, blur_px);
                    }
                    if let Some(cm) = layer.spec.filter.color_matrix {
                        for p in pm_buf.iter_mut() {
                            *p = cm.apply_premul(*p);
                        }
                    }

                    // 4. 按 `transform` / `opacity` / `blend_mode` 将离屏层合成回父画布
                    if let Some(mut t) = layer.spec.transform {
                        if !t.is_identity() {
                            t.e *= sf;
                            t.f *= sf;
                            let cx = layer.body_phys.x as f32 + layer.body_phys.w as f32 * 0.5;
                            let cy = layer.body_phys.y as f32 + layer.body_phys.h as f32 * 0.5;
                            let screen_t = t.centered_at(cx, cy);
                            if let Some(inv) = screen_t.invert() {
                                let out_rect = screen_t
                                    .transform_region(layer.buf_rect)
                                    .intersect(layer.parent_clip_phys)
                                    .intersect(root_clip);
                                for py in out_rect.y..out_rect.bottom() {
                                    let sy = py as f32 + 0.5;
                                    for px in out_rect.x..out_rect.right() {
                                        let sx = px as f32 + 0.5;
                                        let (lx, ly) = inv.apply(sx, sy);
                                        let mut src = sample_premul_bilinear(
                                            &pm_buf,
                                            lw,
                                            lh,
                                            lx - layer.buf_rect.x as f32,
                                            ly - layer.buf_rect.y as f32,
                                        );
                                        if src[3] <= 1e-5 && layer.spec.blend_mode.is_normal() {
                                            continue;
                                        }
                                        src[0] *= opacity_f;
                                        src[1] *= opacity_f;
                                        src[2] *= opacity_f;
                                        src[3] *= opacity_f;
                                        write_to_parent(
                                            &mut layer_stack,
                                            target,
                                            origin_x,
                                            origin_y,
                                            stride,
                                            px,
                                            py,
                                            src,
                                            layer.spec.blend_mode,
                                        );
                                    }
                                }
                            }
                            continue;
                        }
                    }

                    let out_rect = layer
                        .buf_rect
                        .intersect(layer.parent_clip_phys)
                        .intersect(root_clip);
                    for py in out_rect.y..out_rect.bottom() {
                        let sy = (py - layer.buf_rect.y) as usize;
                        for px in out_rect.x..out_rect.right() {
                            let sx = (px - layer.buf_rect.x) as usize;
                            let mut src = pm_buf[sy * lw + sx];
                            if src[3] <= 1e-5 && layer.spec.blend_mode.is_normal() {
                                continue;
                            }
                            src[0] *= opacity_f;
                            src[1] *= opacity_f;
                            src[2] *= opacity_f;
                            src[3] *= opacity_f;
                            write_to_parent(
                                &mut layer_stack,
                                target,
                                origin_x,
                                origin_y,
                                stride,
                                px,
                                py,
                                src,
                                layer.spec.blend_mode,
                            );
                        }
                    }
                }

                _ => {
                    // 普通绘制指令：若有活跃离屏图层则绘制到栈顶 `PremulRgba` 缓冲，否则绘制到主 `target`
                    if let Some(top) = layer_stack.last_mut() {
                        let dst_stride = top.buf_rect.w.max(1) as usize;
                        rasterize_single_op_scaled(
                            op,
                            viewport,
                            sf,
                            vp_clip_phys,
                            top.buf_rect,
                            top.buf_rect.x,
                            top.buf_rect.y,
                            dst_stride,
                            &mut top.pixels,
                        );
                    } else {
                        rasterize_single_op_scaled(
                            op,
                            viewport,
                            sf,
                            vp_clip_phys,
                            root_clip,
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

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn write_to_parent<P: PixelFormat>(
        layer_stack: &mut [ActiveLayer<'_>],
        target: &mut [P],
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        px: i16,
        py: i16,
        src_pm: [f32; 4],
        mode: crate::paint::BlendMode,
    ) {
        if let Some(parent) = layer_stack.last_mut() {
            if py >= parent.buf_rect.y
                && py < parent.buf_rect.bottom()
                && px >= parent.buf_rect.x
                && px < parent.buf_rect.right()
            {
                let pw = parent.buf_rect.w as usize;
                let idx = ((py - parent.buf_rect.y) as usize) * pw
                    + ((px - parent.buf_rect.x) as usize);
                let dst_pm = parent.pixels[idx].to_premul_f32();
                let out = blend_premul_f32(src_pm, dst_pm, mode);
                parent.pixels[idx] = PremulRgba::from_premul_f32(out);
            }
        } else {
            let idx = ((py - origin_y) as usize) * stride + ((px - origin_x) as usize);
            if let Some(dst_px) = target.get_mut(idx) {
                let dst_pm = color_to_premul_f32(dst_px.to_color());
                let out = blend_premul_f32(src_pm, dst_pm, mode);
                *dst_px = P::from_color(premul_f32_to_color(out));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize_single_op_scaled<P: PixelFormat>(
        op: &DrawOp<'_>,
        viewport: Region,
        sf: f32,
        vp_clip_phys: Region,
        dirty_phys: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) {
        let sb = snap_bounds_region_scaled(op.bounds, sf);
        let mut sc = snap_clip_region_scaled(op.clip, sf);
        if (op.bounds.w == 1) || (op.bounds.h == 1) {
            if op.bounds.w == 1 {
                let new_r = sc.right().max(sb.right()).min(vp_clip_phys.right());
                sc.w = new_r.saturating_sub(sc.x).max(0);
            }
            if op.bounds.h == 1 {
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

        let (eff_phys, text_params) = match op.payload {
            DrawPayload::TextRun {
                text,
                color,
                font_size,
                text_flags,
            } => {
                if text.is_empty() || color.a() == 0 {
                    return;
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
                let eff = adj_sc.intersect(dirty_phys);
                if eff.is_empty() {
                    return;
                }
                (eff, Some((adj_bounds, adj_flags, logical_fs, fs_q4)))
            }
            DrawPayload::StyledQuad(sq) if sq.transform.is_some() => {
                let eff = sc.intersect(dirty_phys);
                if eff.is_empty() {
                    return;
                }
                (eff, None)
            }
            _ => {
                let eff = sc.intersect(sb).intersect(dirty_phys);
                if eff.is_empty() {
                    return;
                }
                (eff, None)
            }
        };

        let draw_fn = |sub_clip: Region, dst: &mut [P]| match op.payload {
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
                    origin_x,
                    origin_y,
                    stride,
                    dst,
                );
            }
            DrawPayload::StyledQuad(sq) => {
                rasterize_styled_quad(sb, sub_clip, sq, sf, origin_x, origin_y, stride, dst);
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
                        origin_x,
                        origin_y,
                        stride,
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
                    origin_x,
                    origin_y,
                    stride,
                    dst,
                );
            }
            DrawPayload::Sprite(sprite) => {
                rasterize_sprite_source(sb, sub_clip, sprite, origin_x, origin_y, stride, dst);
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
                rasterize_styled_quad(sb, sub_clip, &sq, sf, origin_x, origin_y, stride, dst);
            }
            DrawPayload::RichText(rt) => {
                crate::renderers::software::text::rasterize_rich_text(
                    rt, sb, sub_clip, sf, origin_x, origin_y, stride, dst,
                );
            }
            DrawPayload::DecodedImage(img) => {
                crate::renderers::software::text::rasterize_decoded_image(
                    img, sb, sub_clip, sf, origin_x, origin_y, stride, dst,
                );
            }
            DrawPayload::Vector(vec_spec) => {
                crate::vector_canvas::rasterize_vector_spec(
                    vec_spec, sb, sub_clip, sf, origin_x, origin_y, stride, dst,
                );
            }
            DrawPayload::SceneCanvas(spec) => {
                crate::canvas_scene::rasterize_scene_canvas(
                    sb, sub_clip, spec, sf, origin_x, origin_y, stride, dst,
                );
            }
        };

        if clip_r_16 > 0 {
            draw_op_with_tiled_rounded_clip(
                eff_phys,
                sc,
                clip_r_ceil,
                origin_x,
                origin_y,
                stride,
                target,
                draw_fn,
                |px, py| compute_sdf_coverage_r16(px, py, sc, clip_r_16),
            );
        } else {
            draw_fn(eff_phys, target);
        }
    }
}

pub(crate) use std_layers::rasterize_ops_with_layers;

/// 检测绘制指令流是否包含隔离合成图层、外部零拷贝纹理或非默认混合模式。
#[inline]
pub(crate) fn ops_have_layers_or_blend(ops: &[DrawOp<'_>]) -> bool {
    ops.iter().any(|op| match op.payload {
        DrawPayload::PushLayer(_) | DrawPayload::PopLayer | DrawPayload::ExternalTexture(_) => true,
        DrawPayload::StyledQuad(sq) => !sq.blend_mode.is_normal(),
        _ => false,
    })
}

