//! 像素格式（`PixelFormat`）、字模图集与显示表面抽象（`DisplaySurface`）。

mod font_bitmap;
mod font_sdf;
pub(crate) mod font_ttf;
mod pixel;

pub(crate) use font_ttf as ttf_atlas;
pub use font_ttf::{
    font_memory_stats, get_cached_glyph_styled_q4, register_font_bytes, register_font_vec,
    registered_custom_font_bytes, registered_custom_font_count, unregister_font, FontMemoryStats,
};
pub use pixel::{PixelFormat, Rgb565, Rgba8888, SpriteFormat, Xrgb8888};

use crate::solver::Region;

/// 硬件显示表面抽象接口。
pub trait DisplaySurface {
    type Pixel: PixelFormat;
    /// 屏幕视口区域。
    fn viewport(&self) -> Region;
}

/// 测量单个字符在指定字号下的水平步进像素宽度（`advance_width`）。
#[inline]
pub fn measure_char_advance(ch: char, font_size: u8) -> i16 {
    {
        if let Some(adv) = ttf_atlas::measure_ttf_char_advance(ch, font_size) {
            return adv.max(2);
        }
    }
    let cw = if ch.is_ascii() {
        ((font_size as i16) * 6 + 5) / 10
    } else if ch == '°' {
        ((font_size as i16) * 5 + 5) / 10
    } else {
        font_size as i16
    }
    .max(4);
    cw
}

/// 测量相邻字符对 `(prev, curr)` 在指定字号下的水平字距微调像素（TrueType `kern` 查表）。
#[inline]
pub fn measure_char_kerning(prev: char, curr: char, font_size: u8) -> i16 {
    {
        return ttf_atlas::measure_ttf_kerning(prev, curr, font_size);
    }
}

/// 测量单个字符在指定字号下的高精度浮点水平步进宽度（用于多行文本编辑器亚像素排版与命中测试）。
#[inline]
pub fn measure_char_advance_f32(ch: char, font_size: u8) -> f32 {
    {
        if let Some(adv) = ttf_atlas::measure_ttf_char_advance_f32(ch, font_size) {
            return adv.max(1.0);
        }
    }
    measure_char_advance(ch, font_size) as f32
}

/// 按 [`FontSpec`](crate::text_rich::FontSpec) 测量单个字符的高精度浮点水平步进宽度（全平台无堆可用）。
#[inline]
pub fn measure_char_advance_styled_f32(
    ch: char,
    font_size: u8,
    spec: crate::text_rich::FontSpec,
) -> f32 {
    {
        if let Some(adv) = ttf_atlas::measure_ttf_char_advance_styled_f32(ch, font_size, spec) {
            return adv.max(1.0);
        }
    }
    let base = measure_char_advance_f32(ch, font_size);
    if spec.weight.is_bold() {
        base + 0.8
    } else {
        base
    }
}

/// 测量相邻字符对 `(prev, curr)` 在指定字号下的高精度浮点水平字距微调像素。
#[inline]
pub fn measure_char_kerning_f32(prev: char, curr: char, font_size: u8) -> f32 {
    {
        return ttf_atlas::measure_ttf_kerning_f32(prev, curr, font_size);
    }
}

/// 测量单行文本（不含 `\n`）的高精度浮点水平排版宽度（不做向上取整）。
///
/// 专供输入框/代码编辑器光标定位、选区边界与鼠标点击命中测试使用，
/// 与 `WgpuRenderer` 和软件渲染器的浮点字形推进共享完全一致的几何真源。
pub fn measure_line_width_f32(line: &str, font_size: u8) -> f32 {
    if line.is_empty() {
        return 0.0;
    }
    if ttf_atlas::has_system_font() {
        if let Some(lw) = ttf_atlas::measure_ttf_line_width_f32(line, font_size) {
            return lw;
        }
    }
    let mut cur_w: f32 = 0.0;
    let mut prev_ch: Option<char> = None;
    for ch in line.chars() {
        if let Some(p) = prev_ch {
            cur_w += measure_char_kerning_f32(p, ch, font_size);
        }
        cur_w = (cur_w + measure_char_advance_f32(ch, font_size)).max(0.0);
        prev_ch = Some(ch);
    }
    cur_w
}

/// 测量单行文本（不含 `\n`）的精确水平排版像素宽度（不含容器右安全余量）。
pub fn measure_line_width_px(line: &str, font_size: u8) -> i16 {
    if line.is_empty() {
        return 0;
    }
    if ttf_atlas::has_system_font() {
        if let Some(lw) = ttf_atlas::measure_ttf_line_width_exact(line, font_size) {
            return lw;
        }
    }
    let mut cur_w: i16 = 0;
    let mut prev_ch: Option<char> = None;
    for ch in line.chars() {
        if let Some(p) = prev_ch {
            cur_w = cur_w.saturating_add(measure_char_kerning(p, ch, font_size));
        }
        cur_w = cur_w.saturating_add(measure_char_advance(ch, font_size)).max(0);
        prev_ch = Some(ch);
    }
    cur_w
}

/// 计算 `text` 在给定可用宽度 `avail_w` 下的下一行切片与剩余文本切片（优先在 ASCII 空格词边界折行，含字偶距微调）。
pub fn next_wrapped_line<'a>(text: &'a str, font_size: u8, avail_w: i16) -> (&'a str, &'a str) {
    if text.is_empty() {
        return ("", "");
    }
    if ttf_atlas::has_system_font() {
        let avail_f = (avail_w.max(0) as f32) + 0.01;
        let mut line_w = 0.0f32;
        let mut prev_ch: Option<char> = None;
        let mut last_space_end: Option<usize> = None;
        let mut last_space_next: usize = 0;

        for (byte_idx, ch) in text.char_indices() {
            if ch == '\n' {
                return (&text[..byte_idx], &text[byte_idx + 1..]);
            }
            let kern = prev_ch
                .map(|p| ttf_atlas::measure_ttf_kerning_f32(p, ch, font_size))
                .unwrap_or(0.0);
            let cw = ttf_atlas::measure_ttf_char_advance_f32(ch, font_size)
                .unwrap_or_else(|| measure_char_advance(ch, font_size) as f32);
            let step_w = cw + kern;
            if ch == ' ' {
                last_space_end = Some(byte_idx);
                last_space_next = byte_idx + 1;
            }
            if line_w > 0.0 && (line_w + step_w) > avail_f {
                if let Some(sp_end) = last_space_end {
                    if sp_end > 0 {
                        return (
                            &text[..sp_end],
                            text[last_space_next..].trim_start_matches(' '),
                        );
                    }
                }
                return (&text[..byte_idx], &text[byte_idx..]);
            }
            line_w = (line_w + step_w).max(0.0);
            prev_ch = Some(ch);
        }
        return (text, "");
    }

    let mut line_w: i16 = 0;
    let mut prev_ch: Option<char> = None;
    let mut last_space_end: Option<usize> = None;
    let mut last_space_next: usize = 0;

    for (byte_idx, ch) in text.char_indices() {
        if ch == '\n' {
            return (&text[..byte_idx], &text[byte_idx + 1..]);
        }
        let kern = prev_ch
            .map(|p| measure_char_kerning(p, ch, font_size))
            .unwrap_or(0);
        let cw = measure_char_advance(ch, font_size);
        let step_w = cw.saturating_add(kern);
        if ch == ' ' {
            last_space_end = Some(byte_idx);
            last_space_next = byte_idx + 1;
        }
        if line_w > 0 && line_w.saturating_add(step_w) > avail_w {
            if let Some(sp_end) = last_space_end {
                if sp_end > 0 {
                    return (&text[..sp_end], text[last_space_next..].trim_start_matches(' '));
                }
            }
            return (&text[..byte_idx], &text[byte_idx..]);
        }
        line_w = line_w.saturating_add(step_w).max(0);
        prev_ch = Some(ch);
    }
    (text, "")
}

/// 计算指定字号的单行标准行高（像素）。
#[inline(always)]
pub const fn text_line_height_px(font_size: u8) -> i16 {
    let h = ((font_size as i16) * 5) / 4;
    if h > 10 { h } else { 10 }
}

#[derive(Copy, Clone)]
struct TextMetricCacheEntry {
    key_hash: u64,
    text_len: u32,
    font_size: u8,
    avail_w: i16,
    single_line: bool,
    atlas_ptr: usize,
    width: i16,
    height: i16,
    lines: i16,
}

impl TextMetricCacheEntry {
    const EMPTY: Self = Self {
        key_hash: 0,
        text_len: u32::MAX,
        font_size: 0,
        avail_w: 0,
        single_line: false,
        atlas_ptr: 0,
        width: 0,
        height: 0,
        lines: 0,
    };
}

const TEXT_CACHE_SLOTS: usize = 256;

std::thread_local! {
    static TLS_TEXT_METRICS: core::cell::RefCell<[TextMetricCacheEntry; TEXT_CACHE_SLOTS]> =
        const { core::cell::RefCell::new([TextMetricCacheEntry::EMPTY; TEXT_CACHE_SLOTS]) };
}

#[inline]
fn hash_text_key(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in text.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 统计文本在给定可用宽度 `avail_w` 下自动折行后的行数（支持 `\n` 与英文单词边界折行，带跨帧度量缓存）。
pub fn count_text_lines(text: &str, font_size: u8, avail_w: i16, single_line: bool) -> i16 {
    if text.is_empty() || single_line || avail_w <= 0 {
        return 1;
    }
    let (key_hash, slot_idx, atlas_ptr) = {
        let kh = hash_text_key(text);
        let ap = 0usize;
        let idx = ((kh as usize)
            ^ ((font_size as usize) << 4)
            ^ ((avail_w as usize) << 1)
            ^ (single_line as usize))
            & (TEXT_CACHE_SLOTS - 1);
        if let Some(cached_lines) = TLS_TEXT_METRICS.with(|tls| {
            let e = &tls.borrow()[idx];
            if e.key_hash == kh
                && e.text_len == text.len() as u32
                && e.font_size == font_size
                && e.avail_w == avail_w
                && e.single_line == single_line
                && e.atlas_ptr == ap
                && e.lines > 0
            {
                Some(e.lines)
            } else {
                None
            }
        }) {
            return cached_lines;
        }
        (kh, idx, ap)
    };

    let mut lines: i16 = 0;
    let mut rem = text;
    while !rem.is_empty() {
        lines = lines.saturating_add(1);
        let (_, next_rem) = next_wrapped_line(rem, font_size, avail_w);
        if next_rem.len() == rem.len() {
            break;
        }
        rem = next_rem;
    }
    let result = lines.max(1);

    TLS_TEXT_METRICS.with(|tls| {
        let mut borrow = tls.borrow_mut();
        borrow[slot_idx] = TextMetricCacheEntry {
            key_hash,
            text_len: text.len() as u32,
            font_size,
            avail_w,
            single_line,
            atlas_ptr,
            width: 0,
            height: text_line_height_px(font_size),
            lines: result,
        };
    });

    result
}

/// 测量单行文本在指定字号下的像素宽高 `(width, height)`（带跨帧度量缓存）。
pub fn measure_text_px(text: &str, font_size: u8) -> (i16, i16) {
    let h = text_line_height_px(font_size);
    if text.is_empty() {
        return (2, h);
    }

    let (key_hash, slot_idx, atlas_ptr) = {
        let kh = hash_text_key(text);
        let ap = 0usize;
        let idx = ((kh as usize) ^ ((font_size as usize) << 3) ^ 0x5A) & (TEXT_CACHE_SLOTS - 1);
        if let Some(hit) = TLS_TEXT_METRICS.with(|tls| {
            let e = &tls.borrow()[idx];
            if e.key_hash == kh
                && e.text_len == text.len() as u32
                && e.font_size == font_size
                && e.avail_w == -1
                && e.atlas_ptr == ap
                && e.width > 0
            {
                Some((e.width, e.height))
            } else {
                None
            }
        }) {
            return hit;
        }
        (kh, idx, ap)
    };

    let mut max_w: i16 = 0;
    for line in text.split('\n') {
        max_w = max_w.max(measure_line_width_px(line, font_size));
    }
    // 预留 2px 右侧字形抗锯齿 overhang 安全余量，既防止末字符（如 `)`、`}`）贴边裁切，
    // 又保证 `measure_line_width_px(line, font_size) < measure_text_px(line, font_size).0` 永不触发误折行或假性省略号。
    let w = max_w.saturating_add(2).max(2);

    TLS_TEXT_METRICS.with(|tls| {
        let mut borrow = tls.borrow_mut();
        borrow[slot_idx] = TextMetricCacheEntry {
            key_hash,
            text_len: text.len() as u32,
            font_size,
            avail_w: -1,
            single_line: true,
            atlas_ptr,
            width: w,
            height: h,
            lines: 1,
        };
    });

    (w, h)
}

/// 获取单个字符在给定像素盒子 `(char_w, char_h)` 内的抗锯齿覆盖率（`0..=255`）。
pub fn sample_glyph_coverage(ch: char, rel_x: i16, rel_y: i16, char_w: i16, char_h: i16) -> u8 {
    if rel_x < 0 || rel_y < 0 || rel_x >= char_w || rel_y >= char_h || char_w <= 0 || char_h <= 0 {
        return 0;
    }


    if let Some(cov) = font_sdf::sample_vector_sdf_char(ch, rel_x, rel_y, char_w, char_h) {
        return cov;
    }

    if ch.is_ascii() {
        let gx_fp = ((rel_x as i32) * 8 * 16) / (char_w as i32);
        let gy_fp = ((rel_y as i32) * 16 * 16) / (char_h as i32);
        font_bitmap::sample_8x16_bilinear(font_bitmap::ascii_or_deg_bitmap(ch), gx_fp, gy_fp)
    } else {
        let gx_fp = ((rel_x as i32) * 16 * 16) / (char_w as i32);
        let gy_fp = ((rel_y as i32) * 16 * 16) / (char_h as i32);
        font_bitmap::sample_16x16_bilinear(font_bitmap::fallback_unihan_bitmap(ch), gx_fp, gy_fp)
    }
}
