//! 定点数 2D 有向距离场（SDF）几何原语、查表三角函数与 `SurfacePainter` 自定义画布接口。
//!
//! 专为 `#![no_std]` 与无硬件 FPU 的 MCU（如 Cortex-M3 / Cortex-M4）设计，
//! 提供零浮点三角函数、线段/圆弧距离场、抗锯齿覆盖率映射与无衬线矢量字符原语，
//! 供上层 `velox-ui` 组件库通过 `Element::Canvas` 构建任意自定义矢量控件（如仪表盘、图表、旋钮）。

use crate::solver::Region;
use crate::style::Color;

/// 面向过程化自定义矢量精灵（[`crate::element::img_shader`]）的抽象画布接口。
///
/// 屏蔽底层像素格式（`Rgb565` / `Argb8888`）与帧缓冲坐标偏移细节。
pub trait SurfacePainter {
    /// 获取当前 `Canvas` 在屏幕坐标系下的完整布局边界。
    fn bounds(&self) -> Region;

    /// 获取当前光栅化批次的有效裁剪区（已与脏矩形及微条带窗口求交）。
    fn clip(&self) -> Region;

    /// 在屏幕坐标 `(px, py)` 处以覆盖率 `alpha` (`0..=255`) 混合写入像素。
    fn blend_pixel(&mut self, px: i16, py: i16, color: Color, alpha: u8);

    /// 在指定矩形区域 `bounds` 内绘制抗锯齿文本（自动受当前 `clip()` 裁剪）。
    fn draw_text(&mut self, bounds: Region, text: &str, color: Color, font_size: u8);
}

/// 第 1 象限 `0°..=90°` 的余弦查找表（缩放因子 `1024`，即 `Q10` 定点数）。
pub const COS_Q10: [i16; 91] = [
    1024, 1024, 1023, 1023, 1022, 1020, 1018, 1016, 1014, 1011, 1008, 1005, 1002, 998, 994, 989,
    984, 979, 974, 968, 962, 956, 949, 943, 935, 928, 920, 912, 904, 896, 887, 878, 868, 859, 849,
    839, 828, 818, 807, 796, 784, 773, 761, 749, 737, 724, 711, 698, 685, 672, 658, 644, 630, 616,
    602, 587, 573, 558, 543, 527, 512, 496, 481, 465, 449, 433, 416, 400, 384, 367, 350, 333, 316,
    299, 282, 265, 248, 230, 213, 195, 178, 160, 143, 125, 107, 89, 71, 54, 36, 18, 0,
];

/// 输入十分之一度 `deg10`（`0..3600`），返回 `(sin_q10, cos_q10)`（范围 `-1024..=1024`）。
#[inline(always)]
pub fn sin_cos_deg10(deg10: i32) -> (i32, i32) {
    let mut d = deg10 % 3600;
    if d < 0 {
        d += 3600;
    }
    let deg = (d / 10) as usize;
    let frac = d % 10;

    let cos_interp = |a: usize, frac_part: i32| -> i32 {
        let q = (a / 90) % 4;
        let idx = a % 90;
        let lookup = |ang: usize| -> i32 {
            match q {
                0 => COS_Q10[ang] as i32,
                1 => -(COS_Q10[90 - ang] as i32),
                2 => -(COS_Q10[ang] as i32),
                _ => COS_Q10[90 - ang] as i32,
            }
        };
        let c0 = lookup(idx);
        if frac_part == 0 {
            return c0;
        }
        let next_a = (a + 1) % 360;
        let nq = (next_a / 90) % 4;
        let nidx = next_a % 90;
        let c1 = match nq {
            0 => COS_Q10[nidx] as i32,
            1 => -(COS_Q10[90 - nidx] as i32),
            2 => -(COS_Q10[nidx] as i32),
            _ => COS_Q10[90 - nidx] as i32,
        };
        c0 + ((c1 - c0) * frac_part) / 10
    };

    let cos_v = cos_interp(deg, frac);
    let sin_deg = (deg + 270) % 360;
    let sin_v = cos_interp(sin_deg, frac);
    (sin_v, cos_v)
}

/// 根据表盘顺时针角度 `deg10`（`0` 为 12 点钟方向，`900` 为 3 点钟方向）计算单位方向向量 `(dx_q10, dy_q10)`。
#[inline(always)]
pub fn dir_from_deg(deg10: u16) -> (i32, i32) {
    let (sin_v, cos_v) = sin_cos_deg10(deg10 as i32);
    (sin_v, -cos_v)
}

/// 32 位整数平方根（利用硬件 `CLZ` / `leading_zeros` 跳过前导零空转，适配无 FPU 的 Cortex-M0/M3/M4/M7）。
#[inline(always)]
pub fn isqrt_i32(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut x = n as u32;
    let mut res = 0u32;
    let mut bit = 1u32 << ((31 - x.leading_zeros()) & !1);
    while bit != 0 {
        let t = res + bit;
        if x >= t {
            x -= t;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res as i32
}

/// 64 位整数平方根（防止大坐标距离平方溢出 `i32`，利用前导零直达最高有效偶次位）。
#[inline(always)]
pub const fn isqrt_i64(n: i64) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut x = n as u64;
    let mut res = 0u64;
    let mut bit = 1u64 << ((63 - x.leading_zeros()) & !1);
    while bit != 0 {
        let t = res + bit;
        if x >= t {
            x -= t;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    if res > i32::MAX as u64 {
        i32::MAX
    } else {
        res as i32
    }
}

/// 点 `(px, py)` 到线段 `(ax, ay) - (bx, by)` 的精确欧氏距离（内部使用 `i64` 防止平方溢出）。
#[inline(always)]
pub fn dist_to_segment(px: i32, py: i32, ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let vx = (bx - ax) as i64;
    let vy = (by - ay) as i64;
    let wx = (px - ax) as i64;
    let wy = (py - ay) as i64;
    let c1 = wx * vx + wy * vy;
    if c1 <= 0 {
        return isqrt_i64(wx * wx + wy * wy);
    }
    let c2 = vx * vx + vy * vy;
    if c2 <= c1 {
        let dx = (px - bx) as i64;
        let dy = (py - by) as i64;
        return isqrt_i64(dx * dx + dy * dy);
    }
    let proj_x = (ax as i64) + (vx * c1) / c2;
    let proj_y = (ay as i64) + (vy * c1) / c2;
    let dx = (px as i64) - proj_x;
    let dy = (py as i64) - proj_y;
    isqrt_i64(dx * dx + dy * dy)
}

/// 将归一化距离场 `sd_norm`（`1000 = R`）转换为 `0..=255` 抗锯齿覆盖率 Alpha。
/// `inv_px_norm_8` 为半像素带宽对应的归一化距离（约 `500 / R`）。
#[inline(always)]
pub fn norm_sdf_to_alpha(sd_norm: i32, inv_px_norm_8: i32) -> u8 {
    let band = inv_px_norm_8.max(4);
    if sd_norm <= -band {
        255
    } else if sd_norm >= band {
        0
    } else {
        (((band - sd_norm) * 255) / (band * 2)) as u8
    }
}

/// 源颜色与前景颜色按 `alpha` (`0..=255`) 线性混合。
#[inline(always)]
pub fn blend_color(dst: Color, src: Color, alpha: u8) -> Color {
    if alpha == 0 {
        return dst;
    }
    if alpha == 255 {
        return src;
    }
    let a = alpha as u16;
    let inv = 255 - a;
    Color::rgb(
        ((src.r() as u16 * a + dst.r() as u16 * inv) / 255) as u8,
        ((src.g() as u16 * a + dst.g() as u16 * inv) / 255) as u8,
        ((src.b() as u16 * a + dst.b() as u16 * inv) / 255) as u8,
    )
}

/// 计算归一化点 `(nx, ny)`（已知到圆心距离 `r_dist`）到半径 `r_arc`、半宽 `half_thick`、
/// 法向向量 `(n0_x, n0_y)`（指向圆弧扇区中心，阈值为 `cos_half_span_q10`）圆弧胶囊的 SDF。
#[inline(always)]
pub fn dist_to_arc(
    nx: i32,
    ny: i32,
    r_dist: i32,
    r_arc: i32,
    half_thick: i32,
    n0_x: i32,
    n0_y: i32,
    cos_half_span_q10: i32,
    cap_a: (i32, i32),
    cap_b: (i32, i32),
) -> i32 {
    if r_dist == 0 {
        return r_arc;
    }
    let dot_q10 = (nx * n0_x + ny * n0_y) / r_dist;
    if dot_q10 >= cos_half_span_q10 {
        (r_dist - r_arc).abs() - half_thick
    } else {
        let da = isqrt_i32((nx - cap_a.0) * (nx - cap_a.0) + (ny - cap_a.1) * (ny - cap_a.1));
        let db = isqrt_i32((nx - cap_b.0) * (nx - cap_b.0) + (ny - cap_b.1) * (ny - cap_b.1));
        da.min(db) - half_thick
    }
}

/// 沿任意旋转切向坐标系 `(lx, ly)` 渲染无衬线大写字母矢量 SDF。
/// 局部坐标系 `lx in [-18, 18]`, `ly in [-24, 24]`，笔画半宽 `4`。
#[inline(always)]
pub fn sdf_uppercase_char(ch: u8, lx: i32, ly: i32) -> i32 {
    if lx.abs() > 26 || ly.abs() > 34 {
        return 1000;
    }
    let w = 4;
    match ch {
        b'O' => {
            let cy = ly.clamp(-9, 9);
            (isqrt_i32(lx * lx + (ly - cy) * (ly - cy)) - 13).abs() - w
        }
        b'P' => {
            let d1 = dist_to_segment(lx, ly, -12, -22, -12, 22);
            let d2 = dist_to_segment(lx, ly, -12, -22, 1, -22);
            let d3 = dist_to_segment(lx, ly, -12, 1, 1, 1);
            let d_loop = if lx >= 1 {
                (isqrt_i32((lx - 1) * (lx - 1) + (ly + 10) * (ly + 10)) - 11).abs()
            } else {
                1000
            };
            d1.min(d2).min(d3).min(d_loop) - w
        }
        b'R' => {
            let dp = sdf_uppercase_char(b'P', lx, ly) + w;
            let d_leg = dist_to_segment(lx, ly, 0, 1, 13, 22);
            dp.min(d_leg) - w
        }
        b'T' => {
            let d1 = dist_to_segment(lx, ly, -14, -22, 14, -22);
            let d2 = dist_to_segment(lx, ly, 0, -22, 0, 22);
            d1.min(d2) - w
        }
        b'E' => {
            let d1 = dist_to_segment(lx, ly, -12, -22, -12, 22);
            let d2 = dist_to_segment(lx, ly, -12, -22, 12, -22);
            let d3 = dist_to_segment(lx, ly, -12, 0, 9, 0);
            let d4 = dist_to_segment(lx, ly, -12, 22, 12, 22);
            d1.min(d2).min(d3).min(d4) - w
        }
        b'M' => {
            let d1 = dist_to_segment(lx, ly, -14, 22, -13, -22);
            let d2 = dist_to_segment(lx, ly, -13, -22, 0, 10);
            let d3 = dist_to_segment(lx, ly, 0, 10, 13, -22);
            let d4 = dist_to_segment(lx, ly, 13, -22, 14, 22);
            d1.min(d2).min(d3).min(d4) - w
        }
        b'A' => {
            let d1 = dist_to_segment(lx, ly, -14, 22, 0, -22);
            let d2 = dist_to_segment(lx, ly, 0, -22, 14, 22);
            let d3 = dist_to_segment(lx, ly, -9, 7, 9, 7);
            d1.min(d2).min(d3) - w
        }
        b'G' => {
            let cy = ly.clamp(-9, 9);
            let d_ring = if lx > 3 && (-8..=3).contains(&ly) {
                1000
            } else {
                (isqrt_i32(lx * lx + (ly - cy) * (ly - cy)) - 13).abs()
            };
            let d_bar = dist_to_segment(lx, ly, 2, 3, 13, 3);
            let d_stem = dist_to_segment(lx, ly, 13, 3, 13, 16);
            d_ring.min(d_bar).min(d_stem) - w
        }
        b'S' => {
            let d1 = dist_to_segment(lx, ly, 11, -18, -6, -22);
            let d2 = dist_to_segment(lx, ly, -6, -22, -12, -12);
            let d3 = dist_to_segment(lx, ly, -12, -12, 12, 8);
            let d4 = dist_to_segment(lx, ly, 12, 8, 8, 21);
            let d5 = dist_to_segment(lx, ly, 8, 21, -11, 18);
            d1.min(d2).min(d3).min(d4).min(d5) - w
        }
        _ => 1000,
    }
}
