//! 文本多行折行切分、对齐偏移与点阵/TrueType 矢量软件文字光栅化。

use crate::display::{
    measure_char_advance, measure_line_width_px, sample_glyph_coverage, PixelFormat,
};
use crate::solver::Region;
use crate::style::Color;

pub(crate) fn split_text_lines(
    text: &str,
    font_size: u8,
    avail_w: i16,
    single_line: bool,
) -> crate::arena::SmallVec<(usize, usize, i16), 32> {
    let mut lines: crate::arena::SmallVec<(usize, usize, i16), 32> =
        crate::arena::SmallVec::new();
    if text.is_empty() {
        return lines;
    }
    if single_line || avail_w <= 0 {
        let w = measure_line_width_px(text, font_size);
        let _ = lines.push((0, text.len(), w));
        return lines;
    }

    let base_ptr = text.as_ptr() as usize;
    let mut rem = text;
    while !rem.is_empty() {
        let (line_slice, next_rem) = crate::display::next_wrapped_line(rem, font_size, avail_w);
        let start_idx = (line_slice.as_ptr() as usize) - base_ptr;
        let end_idx = start_idx + line_slice.len();
        let line_w = measure_line_width_px(line_slice, font_size);
        if !lines.push((start_idx, end_idx, line_w)) {
            break;
        }
        if next_rem.len() == rem.len() {
            break;
        }
        rem = next_rem;
    }
    lines
}

#[inline(always)]
pub(crate) fn align_offset_x(avail_w: i16, line_w: i16, align_bits: u8) -> i16 {
    let rem = (avail_w - line_w).max(0);
    match align_bits & 0x03 {
        1 => rem >> 1,
        2 => rem,
        _ => 0,
    }
}

/// 按逻辑字号与 0.25px 量化物理字号（`font_size_q4 = round(logical_fs * sf * 4)`）计算分数 DPI 下
/// 调整后的文本边界框、裁剪框与标志位（与 `WgpuRenderer` 共享唯一真源，确保软硬渲染 0.00px 零漂移）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_scaled_text_box(
    orig_bounds: Region,
    orig_clip: Region,
    viewport: Region,
    sb: Region,
    sc: Region,
    text: &str,
    font_size: u8,
    text_flags: u8,
    sf: f32,
) -> (Region, Region, u8, u8, u16) {
    let mut adj_bounds = sb;
    let mut adj_sc = sc;
    let mut adj_flags = text_flags;
    let logical_fs = font_size.max(8);
    let exact_fs = ((logical_fs as f32) * sf).clamp(8.0, 255.0);
    let fs_q4 = (exact_fs * 4.0).round().clamp(32.0, 1020.0) as u16;
    let render_fs = (fs_q4 as f32) * 0.25;
    let scale_ratio = render_fs / (logical_fs as f32);

    if sf > 1.01 {
        let is_ellipsis = (text_flags & 0x04) != 0;
        let is_nowrap = (text_flags & 0x08) != 0;
        let lines_1x = crate::display::count_text_lines(
            text,
            logical_fs,
            orig_bounds.w,
            is_nowrap || is_ellipsis,
        );
        let scaled_line_w =
            (crate::display::measure_line_width_f32(text, logical_fs) * scale_ratio).ceil() as i16;
        let parent_clip = super::snap_clip_region_scaled(orig_clip.intersect(viewport), sf);

        if lines_1x <= 1 && !is_ellipsis {
            adj_flags |= 0x08;
            let tw_s = scaled_line_w;
            let th_s = render_fs.round() as i16;
            let mut sc_x0 = adj_sc.x;
            let mut sc_y0 = adj_sc.y;
            let mut sc_x1 = adj_sc.right();
            let mut sc_y1 = adj_sc.bottom();

            if tw_s > adj_bounds.w {
                let diff_w = tw_s - adj_bounds.w;
                match adj_flags & 0x03 {
                    1 => {
                        adj_bounds.x = adj_bounds.x.saturating_sub(diff_w / 2);
                    }
                    2 => {
                        adj_bounds.x = adj_bounds.x.saturating_sub(diff_w);
                    }
                    _ => {}
                }
                adj_bounds.w = tw_s.saturating_add(4);
                let clip_r = parent_clip.right();
                let clip_l = parent_clip.x;
                if adj_bounds.right() > clip_r {
                    let over = adj_bounds.right().saturating_sub(clip_r);
                    adj_bounds.x = adj_bounds.x.saturating_sub(over).max(clip_l);
                }
                let new_x0 = adj_bounds.x.max(parent_clip.x);
                let new_x1 = adj_bounds.right().min(parent_clip.right());
                if new_x1 > new_x0 {
                    sc_x0 = sc_x0.min(new_x0);
                    sc_x1 = sc_x1.max(new_x1);
                }
            }
            if th_s > adj_bounds.h {
                let diff_h = th_s - adj_bounds.h;
                adj_bounds.y = adj_bounds.y.saturating_sub(diff_h / 2);
                adj_bounds.h = th_s.saturating_add(2);
                let new_y0 = adj_bounds.y.max(parent_clip.y);
                let new_y1 = adj_bounds.bottom().min(parent_clip.bottom());
                if new_y1 > new_y0 {
                    sc_y0 = sc_y0.min(new_y0);
                    sc_y1 = sc_y1.max(new_y1);
                }
            }
            adj_sc = Region::new(
                sc_x0,
                sc_y0,
                sc_x1.saturating_sub(sc_x0).max(0),
                sc_y1.saturating_sub(sc_y0).max(0),
            );
        } else if is_ellipsis {
            let logic_w = crate::display::measure_line_width_px(text, logical_fs);
            if logic_w <= orig_bounds.w && scaled_line_w > adj_bounds.w {
                let avail_clip_w = parent_clip
                    .right()
                    .saturating_sub(adj_bounds.x)
                    .max(adj_bounds.w);
                let target_w = scaled_line_w.saturating_add(2);
                if target_w <= avail_clip_w {
                    adj_bounds.w = target_w;
                    let new_x1 = adj_bounds.right().min(parent_clip.right());
                    if new_x1 > adj_sc.right() {
                        adj_sc.w = new_x1.saturating_sub(adj_sc.x).max(0);
                    }
                }
            }
        }
    }
    (adj_bounds, adj_sc, adj_flags, logical_fs, fs_q4)
}

/// 按 0.25px 亚像素量化字号（`font_size_q4`）直接将文本光栅化到像素缓冲区（与 `WgpuRenderer::encode_text_run` 逐像素对齐）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_text_scaled<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    text: &str,
    color: Color,
    logical_font_size: u8,
    font_size_q4: u16,
    text_flags: u8,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if color.a() == 0 || text.is_empty() || clip.is_empty() {
        return;
    }
    if crate::display::ttf_atlas::has_system_font() {
        rasterize_text_ttf_q4(
            bounds,
            clip,
            text,
            color,
            logical_font_size,
            font_size_q4,
            text_flags,
            origin_x,
            origin_y,
            stride,
            target,
        );
        return;
    }
    let fs_u8 = ((font_size_q4.clamp(32, 1020) + 2) >> 2).clamp(8, 255) as u8;
    rasterize_text_bitmap(
        bounds,
        clip,
        text,
        color,
        fs_u8,
        text_flags,
        origin_x,
        origin_y,
        stride,
        target,
    );
}

/// 直接将文本光栅化到像素缓冲区（优先使用系统 TTF 矢量字体，无系统字体时回退到内建点阵字形）。
#[allow(clippy::too_many_arguments)]
pub fn rasterize_text<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    text: &str,
    color: Color,
    font_size: u8,
    text_flags: u8,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if color.a() == 0 || text.is_empty() {
        return;
    }

    {
        if crate::display::ttf_atlas::has_system_font() {
            let logical_fs = font_size.max(8);
            rasterize_text_ttf_q4(
                bounds,
                clip,
                text,
                color,
                logical_fs,
                (logical_fs as u16) << 2,
                text_flags,
                origin_x,
                origin_y,
                stride,
                target,
            );
            return;
        }
    }

    rasterize_text_bitmap(
        bounds,
        clip,
        text,
        color,
        font_size,
        text_flags,
        origin_x,
        origin_y,
        stride,
        target,
    );
}

#[allow(clippy::too_many_arguments)]
fn rasterize_text_bitmap<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    text: &str,
    color: Color,
    font_size: u8,
    text_flags: u8,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let is_ellipsis = (text_flags & 0x04) != 0;
    let is_nowrap = (text_flags & 0x08) != 0;
    let char_h = (font_size as i16).max(8);
    let line_step = ((font_size as i16) * 5 / 4).max(char_h);
    let dot_w = measure_char_advance('.', font_size);

    let lines = split_text_lines(text, font_size, bounds.w, is_nowrap || is_ellipsis);
    let line_count = lines.len();
    if line_count == 0 {
        return;
    }

    let total_block_h = char_h.saturating_add(line_step.saturating_mul((line_count as i16) - 1));
    let base_y = bounds.y + ((bounds.h - total_block_h).max(0) >> 1);

    for (line_idx, &(b_start, b_end, raw_line_w)) in lines.as_slice().iter().enumerate() {
        let line_str = &text[b_start..b_end];
        let cur_y = base_y.saturating_add((line_idx as i16).saturating_mul(line_step));
        let need_ellipsis = is_ellipsis && raw_line_w > bounds.w && bounds.w > dot_w * 3;
        let eff_line_w = if need_ellipsis {
            bounds.w
        } else {
            raw_line_w.min(bounds.w)
        };
        let mut cur_x = bounds
            .x
            .saturating_add(align_offset_x(bounds.w, eff_line_w, text_flags));
        let max_x_before_dots = bounds.x.saturating_add(bounds.w).saturating_sub(dot_w * 3);

        for ch in line_str.chars() {
            let char_w = measure_char_advance(ch, font_size);
            if need_ellipsis && cur_x.saturating_add(char_w) > max_x_before_dots {
                for _ in 0..3 {
                    rasterize_single_bitmap_glyph(
                        '.', cur_x, cur_y, dot_w, char_h, clip, color, origin_x, origin_y, stride,
                        target,
                    );
                    cur_x = cur_x.saturating_add(dot_w);
                }
                break;
            }
            if ch != ' ' {
                rasterize_single_bitmap_glyph(
                    ch, cur_x, cur_y, char_w, char_h, clip, color, origin_x, origin_y, stride,
                    target,
                );
            }
            cur_x = cur_x.saturating_add(char_w);
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn rasterize_single_bitmap_glyph<P: PixelFormat>(
    ch: char,
    cur_x: i16,
    cur_y: i16,
    char_w: i16,
    char_h: i16,
    clip: Region,
    color: Color,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let glyph_reg = Region::new(cur_x, cur_y, char_w, char_h);
    let glyph_clip = clip.intersect(glyph_reg);
    if !glyph_clip.is_empty() {
        for py in glyph_clip.y..glyph_clip.bottom() {
            let row_offset = ((py - origin_y) as usize) * stride;
            let rel_y = py - cur_y;
            for px in glyph_clip.x..glyph_clip.right() {
                let rel_x = px - cur_x;
                let cov = sample_glyph_coverage(ch, rel_x, rel_y, char_w, char_h);
                if cov > 0 {
                    let idx = row_offset + ((px - origin_x) as usize);
                    if let Some(dst) = target.get_mut(idx) {
                        *dst = dst.blend(color, cov);
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_text_ttf_q4<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    text: &str,
    color: Color,
    logical_font_size: u8,
    font_size_q4: u16,
    text_flags: u8,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    use crate::display::ttf_atlas::{
        get_cached_glyph_q4, get_font_ascent_f32, measure_ttf_char_advance_f32,
        measure_ttf_kerning_f32,
    };

    let logical_fs = logical_font_size.max(8);
    let q4 = font_size_q4.clamp(32, 1020);
    let render_fs = (q4 as f32) * 0.25;
    let fs_u8 = ((q4 + 2) >> 2).clamp(8, 255) as u8;
    let scale_ratio = render_fs / (logical_fs as f32);

    let is_ellipsis = (text_flags & 0x04) != 0;
    let is_nowrap = (text_flags & 0x08) != 0;
    let logical_max_w = ((bounds.w as f32) / scale_ratio)
        .round()
        .clamp(1.0, i16::MAX as f32) as i16;
    let lines = split_text_lines(text, logical_fs, logical_max_w, is_nowrap || is_ellipsis);
    let line_count = lines.len();
    if line_count == 0 {
        return;
    }

    let char_h = (render_fs.round() as i16).max(8);
    let line_step = ((render_fs * 1.25).round() as i16).max(char_h);
    let ascent = get_font_ascent_f32(render_fs) as i16;
    let dot_w = measure_char_advance('.', fs_u8);
    let dot_adv_f = measure_ttf_char_advance_f32('.', logical_fs)
        .map(|a| a * scale_ratio)
        .unwrap_or(dot_w as f32);

    let total_h = char_h.saturating_add(line_step.saturating_mul((line_count as i16) - 1));
    let top_y = bounds
        .y
        .saturating_add(bounds.h.saturating_sub(total_h).max(0) >> 1);

    for (line_idx, &(b_start, b_end, _raw_line_w_orig)) in lines.as_slice().iter().enumerate() {
        let line_str = &text[b_start..b_end];
        let raw_line_w =
            (crate::display::measure_line_width_f32(line_str, logical_fs) * scale_ratio).ceil()
                as i16;
        let need_ellipsis = is_ellipsis && raw_line_w > bounds.w && bounds.w > dot_w * 3;
        let eff_w = if need_ellipsis {
            bounds.w
        } else {
            raw_line_w.min(bounds.w)
        };
        let start_rel_x = align_offset_x(bounds.w, eff_w, text_flags);
        let mut pen_x = start_rel_x as f32;
        let base_y = top_y
            .saturating_add(ascent)
            .saturating_add((line_idx as i16).saturating_mul(line_step));
        let max_x_before_dots_f = (bounds.w as f32) - dot_adv_f * 3.0;

        let mut prev_ch: Option<char> = None;
        for ch in line_str.chars() {
            if let Some(p) = prev_ch {
                pen_x += measure_ttf_kerning_f32(p, ch, logical_fs) * scale_ratio;
            }
            let adv_f = measure_ttf_char_advance_f32(ch, logical_fs)
                .map(|a| a * scale_ratio)
                .unwrap_or_else(|| (measure_char_advance(ch, logical_fs) as f32) * scale_ratio);
            if need_ellipsis && pen_x + adv_f > max_x_before_dots_f + 0.01 {
                if let Some(dot_g) = get_cached_glyph_q4('.', q4) {
                    for _ in 0..3 {
                        draw_ttf_glyph(
                            &dot_g,
                            bounds.x.saturating_add(pen_x.round() as i16),
                            base_y,
                            clip,
                            color,
                            origin_x,
                            origin_y,
                            stride,
                            target,
                        );
                        pen_x += dot_adv_f;
                    }
                }
                break;
            }
            if ch != ' ' {
                if let Some(g) = get_cached_glyph_q4(ch, q4) {
                    draw_ttf_glyph(
                        &g,
                        bounds.x.saturating_add(pen_x.round() as i16),
                        base_y,
                        clip,
                        color,
                        origin_x,
                        origin_y,
                        stride,
                        target,
                    );
                }
            }
            pen_x += adv_f;
            prev_ch = Some(ch);
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn draw_ttf_glyph<P: PixelFormat>(
    g: &crate::display::ttf_atlas::CachedGlyph,
    cur_x: i16,
    base_y: i16,
    clip: Region,
    color: Color,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if g.width > 0 && g.height > 0 {
        let gx = cur_x.saturating_add(g.xmin as i16);
        let gy = base_y
            .saturating_sub(g.height as i16)
            .saturating_sub(g.ymin as i16);
        let g_reg = Region::new(gx, gy, g.width as i16, g.height as i16);
        let g_clip = clip.intersect(g_reg);
        if !g_clip.is_empty() {
            for py in g_clip.y..g_clip.bottom() {
                let row_offset = ((py - origin_y) as usize) * stride;
                let bmp_y = (py - gy) as usize;
                let bmp_row = bmp_y * g.width;
                for px in g_clip.x..g_clip.right() {
                    let bmp_x = (px - gx) as usize;
                    if let Some(&cov) = g.bitmap.get(bmp_row + bmp_x) {
                        if cov > 0 {
                            let dst_idx = row_offset + ((px - origin_x) as usize);
                            if let Some(dst) = target.get_mut(dst_idx) {
                                *dst = dst.blend(color, cov);
                            }
                        }
                    }
                }
            }
        }
    }
}

