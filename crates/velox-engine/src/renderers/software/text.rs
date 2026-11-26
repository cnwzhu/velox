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

#[inline(always)]
fn round_f32_nostd(x: f32) -> f32 {
    if x >= 0.0 {
        (x + 0.5) as i32 as f32
    } else {
        (x - 0.5) as i32 as f32
    }
}

#[inline(always)]
fn ceil_f32_nostd(x: f32) -> f32 {
    let i = x as i32;
    if (i as f32) < x {
        (i + 1) as f32
    } else {
        i as f32
    }
}

/// 将行内富文本 [`RichTextSpec`](crate::text_rich::RichTextSpec) 光栅化至目标像素缓冲（支持多字族/字重/斜体、
/// Unicode BiDi 双向重排、编程连字、下划线/删除线、行内背景高亮、彩色 Emoji 与任意 HiDPI 缩放，全 `#![no_std]` 兼容）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_rich_text<P: PixelFormat>(
    spec: &crate::text_rich::RichTextSpec<'_>,
    bounds: Region,
    clip: Region,
    sf: f32,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let vis = bounds.intersect(clip);
    if vis.is_empty() || spec.spans.is_empty() {
        return;
    }
    let sf = sf.clamp(0.5, 4.0);
    let avail_w = if spec.nowrap || bounds.w <= 0 {
        f32::INFINITY
    } else {
        bounds.w as f32
    };
    let line_h_px = round_f32_nostd((spec.line_height_px(spec.default_font_size) as f32) * sf).max(1.0) as i16;
    let mut cur_x = bounds.x as f32;
    let mut cur_y = bounds.y;
    let mut line_idx = 1u8;

    let fill_rect = |r: Region, col: Color, target: &mut [P]| {
        let rc = r.intersect(vis);
        if rc.is_empty() || col.a() == 0 {
            return;
        }
        for py in rc.y..rc.bottom() {
            let row_off = ((py - origin_y) as usize) * stride;
            for px in rc.x..rc.right() {
                let idx = row_off + ((px - origin_x) as usize);
                if let Some(dst) = target.get_mut(idx) {
                    *dst = dst.blend(col, 255);
                }
            }
        }
    };

    for sp in spec.spans {
        let color = sp.color.unwrap_or(spec.default_color);
        let logical_fs = sp.font_size.unwrap_or(spec.default_font_size).max(8);
        let phys_fs = ((logical_fs as f32) * sf).clamp(8.0, 255.0);
        let fs_q4 = round_f32_nostd(phys_fs * 4.0).clamp(32.0, 1020.0) as u16;
        let ls = (sp.letter_spacing_q4 as f32) * 0.25 * sf;

        // 按 BiDi 视觉顺序重排当前 Span 文本
        let bidi_runs = crate::text_rich::reorder_bidi_visual(sp.text.as_str(), spec.rtl_paragraph);
        for run in bidi_runs.as_slice() {
            let sub = &sp.text.as_str()[run.start..run.end];
            let mut shaped = crate::text_rich::shape_text_run(sub, sp.ligatures);
            if run.is_rtl {
                shaped.as_mut_slice().reverse();
            }

            let span_start_x = cur_x;
            let mut prev_ch: Option<char> = None;

            for g in shaped.as_slice() {
                let ch = if run.is_rtl {
                    crate::text_rich::mirror_bidi_char(g.ch)
                } else {
                    g.ch
                };
                if ch == '\n' {
                    cur_x = bounds.x as f32;
                    cur_y = cur_y.saturating_add(line_h_px);
                    prev_ch = None;
                    line_idx = line_idx.saturating_add(1);
                    if spec.max_lines > 0 && line_idx > spec.max_lines {
                        return;
                    }
                    continue;
                }
                let adv_logical = if g.is_combining {
                    0.0
                } else {
                    crate::display::measure_char_advance_styled_f32(ch, logical_fs, sp.font)
                };
                let adv_phys = adv_logical * sf;
                if !g.is_combining
                    && cur_x > bounds.x as f32 + 0.5
                    && (cur_x - bounds.x as f32) + adv_phys > avail_w
                {
                    cur_x = bounds.x as f32;
                    cur_y = cur_y.saturating_add(line_h_px);
                    prev_ch = None;
                    line_idx = line_idx.saturating_add(1);
                    if spec.max_lines > 0 && line_idx > spec.max_lines {
                        return;
                    }
                }
                if let Some(p) = prev_ch {
                    if !g.is_combining {
                        cur_x += crate::display::measure_char_kerning_f32(p, ch, logical_fs) * sf + ls;
                    }
                }

                // 若配置了行内背景高亮，先铺背景条
                if let Some(bg) = sp.bg_color {
                    if !g.is_combining && adv_phys > 0.0 {
                        fill_rect(
                            Region::new(cur_x as i16, cur_y, (ceil_f32_nostd(adv_phys) as i16).max(1), line_h_px),
                            bg,
                            target,
                        );
                    }
                }

                {
                    if let Some(glyph) =
                        crate::display::get_cached_glyph_styled_q4(ch, fs_q4, sp.font)
                    {
                        let base_y = cur_y.saturating_add(
                            crate::display::ttf_atlas::get_font_ascent_f32(phys_fs) as i16,
                        );
                        if let Some(rgba) = &glyph.color_rgba {
                            let gx = (round_f32_nostd(cur_x) as i16).saturating_add(glyph.xmin as i16);
                            let gy = base_y
                                .saturating_sub(glyph.height as i16)
                                .saturating_sub(glyph.ymin as i16);
                            let g_clip =
                                vis.intersect(Region::new(gx, gy, glyph.width as i16, glyph.height as i16));
                            for py in g_clip.y..g_clip.bottom() {
                                let row_off = ((py - origin_y) as usize) * stride;
                                let by = (py - gy) as usize;
                                for px in g_clip.x..g_clip.right() {
                                    let bx = (px - gx) as usize;
                                    let si = (by * glyph.width + bx) * 4;
                                    if si + 3 < rgba.len() && rgba[si + 3] > 0 {
                                        let a = rgba[si + 3];
                                        // 还原直通色进行混合
                                        let r = ((rgba[si] as u16 * 255) / (a as u16)).min(255) as u8;
                                        let gv = ((rgba[si + 1] as u16 * 255) / (a as u16)).min(255) as u8;
                                        let b = ((rgba[si + 2] as u16 * 255) / (a as u16)).min(255) as u8;
                                        let di = row_off + ((px - origin_x) as usize);
                                        if let Some(dst) = target.get_mut(di) {
                                            *dst = dst.blend(Color::rgba(r, gv, b, color.a()), a);
                                        }
                                    }
                                }
                            }
                        } else {
                            draw_ttf_glyph(
                                &glyph,
                                round_f32_nostd(cur_x) as i16,
                                base_y,
                                vis,
                                color,
                                origin_x,
                                origin_y,
                                stride,
                                target,
                            );
                        }
                    }
                }

                if !g.is_combining {
                    cur_x += adv_phys;
                    prev_ch = Some(ch);
                }
            }

            // 绘制下划线 / 删除线装饰
            let run_w = round_f32_nostd(cur_x - span_start_x) as i16;
            if run_w > 0 {
                let thickness = (round_f32_nostd(sf) as i16).max(1);
                if sp.decoration.has_underline() {
                    let uy = cur_y.saturating_add((round_f32_nostd(phys_fs * 0.92) as i16).min(line_h_px - 1));
                    fill_rect(
                        Region::new(span_start_x as i16, uy, run_w, thickness),
                        color,
                        target,
                    );
                }
                if sp.decoration.has_strikethrough() {
                    let sy = cur_y.saturating_add(round_f32_nostd(phys_fs * 0.52) as i16);
                    fill_rect(
                        Region::new(span_start_x as i16, sy, run_w, thickness),
                        color,
                        target,
                    );
                }
            }
        }
    }
}

/// 将 [`DecodedImageSpec`](crate::image_pipeline::DecodedImageSpec) 光栅化至目标像素缓冲（支持 `ObjectFit`、
/// `NineSlice` 九宫格、四角独立圆角 SDF 裁切与双线性采样；未加载或无 `std` 时优雅降级为 `fallback_bg` 圆角块）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_decoded_image<P: PixelFormat>(
    spec: &crate::image_pipeline::DecodedImageSpec,
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

    if let Some(px_data) = crate::image_pipeline::get_cached_image(spec.id) {
        let sw = px_data.width as f32;
        let sh = px_data.height as f32;
        let (draw_rect, uv) = if spec.nine_slice.is_active() {
            (bounds, [0.0, 0.0, 1.0, 1.0])
        } else {
            crate::image_pipeline::compute_object_fit(px_data.width, px_data.height, bounds, spec.fit)
        };
        let eff = vis.intersect(draw_rect);
        if eff.is_empty() {
            return;
        }
        let cx = bounds.x as f32 + bounds.w as f32 * 0.5;
        let cy = bounds.y as f32 + bounds.h as f32 * 0.5;
        let hw = bounds.w as f32 * 0.5;
        let hh = bounds.h as f32 * 0.5;
        let max_r = hw.min(hh);
        let r4 = [
            (spec.radii.tl as f32 * sf).min(max_r),
            (spec.radii.tr as f32 * sf).min(max_r),
            (spec.radii.br as f32 * sf).min(max_r),
            (spec.radii.bl as f32 * sf).min(max_r),
        ];
        let has_radii = !spec.radii.is_zero();
        let dw = draw_rect.w.max(1) as f32;
        let dh = draw_rect.h.max(1) as f32;

        for py in eff.y..eff.bottom() {
            let row_off = ((py - origin_y) as usize) * stride;
            let fy = py as f32 + 0.5;
            let sy = if spec.nine_slice.is_active() {
                crate::image_pipeline::map_nine_slice_coord(
                    fy - draw_rect.y as f32,
                    dh,
                    sh,
                    spec.nine_slice.top,
                    spec.nine_slice.bottom,
                )
            } else {
                let v = uv[1] + ((fy - draw_rect.y as f32) / dh) * (uv[3] - uv[1]);
                v * sh
            };
            let iy = (sy.floor() as i32).clamp(0, px_data.height as i32 - 1) as usize;

            for px in eff.x..eff.right() {
                let fx = px as f32 + 0.5;
                let r_cov = if has_radii {
                    let d = super::effects::sdf_rounded_box_4_f32(fx, fy, cx, cy, hw, hh, r4);
                    (0.5 - d).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                if r_cov <= 0.0 {
                    continue;
                }
                let sx = if spec.nine_slice.is_active() {
                    crate::image_pipeline::map_nine_slice_coord(
                        fx - draw_rect.x as f32,
                        dw,
                        sw,
                        spec.nine_slice.left,
                        spec.nine_slice.right,
                    )
                } else {
                    let u = uv[0] + ((fx - draw_rect.x as f32) / dw) * (uv[2] - uv[0]);
                    u * sw
                };
                let ix = (sx.floor() as i32).clamp(0, px_data.width as i32 - 1) as usize;
                let si = (iy * (px_data.width as usize) + ix) * 4;
                if si + 3 < px_data.rgba.len() {
                    let r = ((px_data.rgba[si] as u16 * spec.tint.r() as u16) / 255) as u8;
                    let g = ((px_data.rgba[si + 1] as u16 * spec.tint.g() as u16) / 255) as u8;
                    let b = ((px_data.rgba[si + 2] as u16 * spec.tint.b() as u16) / 255) as u8;
                    let a_raw = ((px_data.rgba[si + 3] as u32
                        * spec.tint.a() as u32
                        * spec.opacity as u32)
                        / (255 * 255)) as f32;
                    let a = (a_raw * r_cov + 0.5).min(255.0) as u8;
                    if a > 0 {
                        let di = row_off + ((px - origin_x) as usize);
                        if let Some(dst) = target.get_mut(di) {
                            *dst = dst.blend(Color::rgba(r, g, b, 255), a);
                        }
                    }
                }
            }
        }
        return;
    }

    // 未加载或无 `std` 时优雅降级为 `fallback_bg` 圆角占位
    let sq = crate::paint::StyledQuad {
        paint: crate::paint::Paint::Solid(spec.fallback_bg),
        border_color: Color::TRANSPARENT,
        border_width: 0,
        radii: spec.radii,
        opacity: spec.opacity,
        blend_mode: crate::paint::BlendMode::SrcOver,
        transform: None,
    };
    super::effects::rasterize_styled_quad(bounds, vis, &sq, sf, origin_x, origin_y, stride, target);
}

