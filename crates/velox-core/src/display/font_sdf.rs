//! 解析式定点数 SDF 矢量字符渲染器（面向 `#![no_std]` 无外部字库时的数字与符号平滑缩放）。

/// 解析式定点数 SDF 矢量字符采样（归一化坐标空间 `x: 0..256, y: 0..256`）。
pub fn sample_vector_sdf_char(
    ch: char,
    rel_x: i16,
    rel_y: i16,
    char_w: i16,
    char_h: i16,
) -> Option<u8> {
    let nx = ((rel_x as i32 * 256 + 128) / (char_w as i32)).clamp(0, 256);
    let ny = ((rel_y as i32 * 256 + 128) / (char_h as i32)).clamp(0, 256);
    let half_stroke = 20i32;
    let aa_band = (256 / (char_h.max(8) as i32)).max(6);

    let dist = match ch {
        '-' => sdf_segment(nx, ny, 48, 128, 208, 128),
        '+' => sdf_segment(nx, ny, 44, 128, 212, 128)
            .min(sdf_segment(nx, ny, 128, 68, 128, 188)),
        '.' => sdf_circle(nx, ny, 128, 198, 6),
        ':' => sdf_circle(nx, ny, 128, 96, 4).min(sdf_circle(nx, ny, 128, 176, 4)),
        '°' => sdf_ring(nx, ny, 128, 68, 42),
        '1' => sdf_segment(nx, ny, 136, 40, 136, 204)
            .min(sdf_segment(nx, ny, 88, 68, 136, 40))
            .min(sdf_segment(nx, ny, 84, 204, 188, 204)),
        '2' => sdf_segment(nx, ny, 60, 68, 100, 42)
            .min(sdf_segment(nx, ny, 100, 42, 168, 42))
            .min(sdf_segment(nx, ny, 168, 42, 198, 76))
            .min(sdf_segment(nx, ny, 198, 76, 192, 112))
            .min(sdf_segment(nx, ny, 192, 112, 58, 202))
            .min(sdf_segment(nx, ny, 58, 202, 202, 202)),
        '3' => sdf_segment(nx, ny, 62, 44, 192, 44)
            .min(sdf_segment(nx, ny, 192, 44, 128, 114))
            .min(sdf_segment(nx, ny, 128, 114, 188, 136))
            .min(sdf_segment(nx, ny, 188, 136, 194, 176))
            .min(sdf_segment(nx, ny, 194, 176, 156, 204))
            .min(sdf_segment(nx, ny, 156, 204, 66, 196)),
        '5' => sdf_segment(nx, ny, 192, 44, 68, 44)
            .min(sdf_segment(nx, ny, 68, 44, 62, 118))
            .min(sdf_segment(nx, ny, 62, 118, 166, 114))
            .min(sdf_segment(nx, ny, 166, 114, 196, 152))
            .min(sdf_segment(nx, ny, 196, 152, 168, 202))
            .min(sdf_segment(nx, ny, 168, 202, 64, 198)),
        '6' => sdf_rounded_rect_ring(nx, ny, 58, 114, 198, 204, 42)
            .min(sdf_segment(nx, ny, 58, 150, 96, 44))
            .min(sdf_segment(nx, ny, 96, 44, 176, 44)),
        '7' => sdf_segment(nx, ny, 52, 44, 204, 44)
            .min(sdf_segment(nx, ny, 204, 44, 104, 204)),
        '8' => sdf_rounded_rect_ring(nx, ny, 64, 42, 192, 118, 36)
            .min(sdf_rounded_rect_ring(nx, ny, 56, 118, 200, 204, 40)),
        '9' => sdf_rounded_rect_ring(nx, ny, 58, 42, 198, 132, 42)
            .min(sdf_segment(nx, ny, 198, 96, 160, 202))
            .min(sdf_segment(nx, ny, 160, 202, 80, 202)),
        '4' => sdf_segment(nx, ny, 164, 40, 52, 156)
            .min(sdf_segment(nx, ny, 52, 156, 212, 156))
            .min(sdf_segment(nx, ny, 164, 40, 164, 204)),
        '0' => sdf_rounded_rect_ring(nx, ny, 56, 42, 200, 202, 64),
        '%' => sdf_segment(nx, ny, 196, 44, 60, 204)
            .min(sdf_ring(nx, ny, 82, 76, 26))
            .min(sdf_ring(nx, ny, 174, 172, 26)),
        'C' => {
            if nx > 176 && ny > 82 && ny < 162 {
                sdf_circle(nx, ny, 184, 76, 0).min(sdf_circle(nx, ny, 184, 168, 0))
            } else {
                sdf_rounded_rect_ring(nx, ny, 52, 42, 204, 202, 68)
            }
        }
        _ => return None,
    };

    let sd = dist - half_stroke;
    if sd <= -aa_band {
        Some(255)
    } else if sd >= aa_band {
        Some(0)
    } else {
        Some((((aa_band - sd) * 255) / (aa_band * 2)).clamp(0, 255) as u8)
    }
}

use crate::sdf::{dist_to_segment, isqrt_i32};

#[inline(always)]
fn sdf_segment(px: i32, py: i32, ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    dist_to_segment(px, py, ax, ay, bx, by)
}

fn sdf_circle(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    (isqrt_i32(dx * dx + dy * dy) - r).max(0)
}

fn sdf_ring(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    (isqrt_i32(dx * dx + dy * dy) - r).abs()
}

fn sdf_rounded_rect_ring(px: i32, py: i32, x1: i32, y1: i32, x2: i32, y2: i32, r: i32) -> i32 {
    let cx = (x1 + x2) >> 1;
    let cy = (y1 + y2) >> 1;
    let hw = ((x2 - x1) >> 1) - r;
    let hh = ((y2 - y1) >> 1) - r;
    let dx = (px - cx).abs() - hw;
    let dy = (py - cy).abs() - hh;
    let outside = isqrt_i32(dx.max(0) * dx.max(0) + dy.max(0) * dy.max(0));
    let inside = dx.max(dy).min(0);
    (outside + inside - r).abs()
}
