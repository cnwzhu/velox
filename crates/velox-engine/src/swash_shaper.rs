//! 可选零拷贝 OpenType GSUB/GPOS 高级文本塑形、可变字体轴（Variable Fonts）与跨折行 Run 缓存
//! （仅在启用 `--features swash` 时编译）。
//!
//! 设计要点：
//! - **零拷贝共享底层字体字节**：直接复用 `display::font_ttf` 中已通过 OS 只读 `mmap` /
//!   `MapViewOfFile` 映射的系统字体及自定义注册字体切片，不重复读取磁盘或二次分配字节缓冲区；
//! - **OpenType GSUB/GPOS 完整支持**：标准连字（`liga` / `clig` / `calt`）、精确字距（`kern`）、
//!   复杂标记定位（`mark` / `mkmk`）以及阿拉伯文/印地语等复杂脚本连写；
//! - **可变字体轴（Variable Fonts）**：支持动态传入 `wght`（字重）、`wdth`（字宽）、`slnt`（倾斜）、
//!   `opsz`（光学字号）等任意 OpenType 变化轴，可由弹簧物理引擎驱动字重与字宽的丝滑过渡；
//! - **跨折行塑形 Run 缓存**：以 `(text_hash, font_spec, font_size_q4, ligatures, variations_hash)`
//!   缓存 `Arc<ShapedGlyphRun>`，窗口 Resize 或多次测量同一文本时零重复塑形；
//! - **线程本地零瞬态分配上下文**：每线程复用 `swash::shape::ShapeContext` 与
//!   `swash::scale::ScaleContext`，避免每次塑形/光栅化重新分配内部工作缓冲。

use core::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::vec::Vec;

use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
use swash::zeno::Format;
use swash::{tag_from_bytes, FontRef, Setting};

use crate::display::font_ttf::{with_styled_font_bytes, CachedGlyph};
use crate::text_rich::FontSpec;

/// 单个 OpenType 可变字体轴设置（如 `wght=650.0`、`wdth=100.0`、`slnt=-8.0`、`opsz=24.0`）。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontVariationAxis {
    /// 4 字节 OpenType 轴标签（例如 `*b"wght"`）。
    pub tag: [u8; 4],
    /// 轴目标值（按字体 `fvar` 表定义范围自动钳位）。
    pub value: f32,
}

impl FontVariationAxis {
    /// 以 4 字节标签和浮点值构造可变字体轴。
    #[inline]
    pub const fn new(tag: [u8; 4], value: f32) -> Self {
        Self { tag, value }
    }

    /// 字重轴（`wght`，通常 100.0 ~ 900.0）。
    #[inline]
    pub const fn weight(value: f32) -> Self {
        Self::new(*b"wght", value)
    }

    /// 字宽轴（`wdth`，通常 50.0 ~ 200.0）。
    #[inline]
    pub const fn width(value: f32) -> Self {
        Self::new(*b"wdth", value)
    }

    /// 倾斜轴（`slnt`，通常 -15.0 ~ 0.0）。
    #[inline]
    pub const fn slant(value: f32) -> Self {
        Self::new(*b"slnt", value)
    }

    /// 光学字号轴（`opsz`，通常等于当前渲染像素字号）。
    #[inline]
    pub const fn optical_size(value: f32) -> Self {
        Self::new(*b"opsz", value)
    }

    #[inline]
    fn to_setting(self) -> Setting<f32> {
        Setting {
            tag: tag_from_bytes(&self.tag),
            value: self.value,
        }
    }
}

/// 经 OpenType GSUB/GPOS 塑形后的单个字形位置与步进信息。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SwashShapedGlyph {
    /// 字体内的真实 `GlyphId`（已包含连字替换、变体替换等 GSUB 结果）。
    pub glyph_id: u16,
    /// 相对当前游标的水平微调偏移（像素，来自 GPOS `kern` / `mark` 等）。
    pub x_offset: f32,
    /// 相对基线的垂直微调偏移（像素，正值向上）。
    pub y_offset: f32,
    /// 字形水平前进宽度（像素）。
    pub advance: f32,
    /// 对应的源字符串起始字节偏移（用于光标命中与选区反查）。
    pub cluster: u32,
}

/// 一次 OpenType 塑形输出的完整字形序列与行度量（支持跨折行缓存复用）。
#[derive(Clone, Debug)]
pub struct ShapedGlyphRun {
    /// 塑形后的有序字形序列。
    pub glyphs: Vec<SwashShapedGlyph>,
    /// 整段文本的总水平前进宽度（像素）。
    pub total_advance: f32,
    /// 字体升部高度（像素）。
    pub ascent: f32,
    /// 字体降部深度（像素，正值表示向下延伸）。
    pub descent: f32,
    /// 建议行距附加值（像素）。
    pub leading: f32,
    /// 底层字体内存映射身份键（与 `font_ttf` 指针一致，可用于区分字体变体）。
    pub font_key: u64,
}

/// `SwashRunCache` 命中统计信息。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SwashRunCacheStats {
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}

const MAX_SWASH_RUN_CACHE_ENTRIES: usize = 1024;

#[derive(Clone, Eq, PartialEq, Hash)]
struct SwashRunCacheKey {
    text_hash: u64,
    text_len: usize,
    font_spec_packed: u32,
    font_size_q4: u16,
    ligatures: bool,
    variations_hash: u64,
}

static SWASH_RUN_CACHE: OnceLock<RwLock<HashMap<SwashRunCacheKey, Arc<ShapedGlyphRun>>>> =
    OnceLock::new();
static SWASH_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static SWASH_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

std::thread_local! {
    static TLS_SHAPE_CTX: RefCell<ShapeContext> = RefCell::new(ShapeContext::new());
    static TLS_SCALE_CTX: RefCell<ScaleContext> = RefCell::new(ScaleContext::new());
}

#[inline]
fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[inline]
fn hash_variations(variations: &[FontVariationAxis]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in variations {
        h = fnv1a_bytes(h, &v.tag);
        // 量化至 1/16 轴单位以避免微弱浮点噪声击穿缓存，同时保留动画精度
        let q = (v.value * 16.0).round() as i32;
        h = fnv1a_bytes(h, &q.to_le_bytes());
    }
    h
}

#[inline]
fn make_cache_key(
    text: &str,
    font_spec: FontSpec,
    font_size: f32,
    ligatures: bool,
    variations: &[FontVariationAxis],
) -> SwashRunCacheKey {
    let size_q4 = ((font_size.clamp(6.0, 255.0) * 4.0).round() as u16).max(24);
    SwashRunCacheKey {
        text_hash: fnv1a_bytes(0xcbf2_9ce4_8422_2325, text.as_bytes()),
        text_len: text.len(),
        font_spec_packed: font_spec.packed_key(),
        font_size_q4: size_q4,
        ligatures,
        variations_hash: hash_variations(variations),
    }
}

/// 直接在指定字体原始字节切片上执行零拷贝 OpenType GSUB/GPOS 塑形（不经全局字体回退表）。
///
/// 适合已有自定义字体内存映射或需要显式指定字体集合索引（`font_index`）的场景。
pub fn shape_text_run_with_font_bytes(
    text: &str,
    font_bytes: &[u8],
    font_index: usize,
    font_size: f32,
    ligatures: bool,
    variations: &[FontVariationAxis],
) -> Option<ShapedGlyphRun> {
    let font = FontRef::from_index(font_bytes, font_index)?;
    let fs = font_size.clamp(6.0, 255.0);
    let font_key = font_bytes.as_ptr() as usize as u64;

    let mut glyphs = Vec::with_capacity(text.chars().count());
    let mut total_advance = 0.0f32;
    let (ascent, descent, leading) = TLS_SHAPE_CTX.with(|cell| {
        let mut ctx = cell.borrow_mut();
        let mut builder = ctx.builder(font).size(fs);
        if !variations.is_empty() {
            builder = builder.variations(variations.iter().copied().map(|v| v.to_setting()));
        }
        let feature_list: [Setting<u16>; 4] = [
            Setting {
                tag: tag_from_bytes(b"kern"),
                value: 1,
            },
            Setting {
                tag: tag_from_bytes(b"liga"),
                value: u16::from(ligatures),
            },
            Setting {
                tag: tag_from_bytes(b"clig"),
                value: u16::from(ligatures),
            },
            Setting {
                tag: tag_from_bytes(b"calt"),
                value: u16::from(ligatures),
            },
        ];
        builder = builder.features(feature_list.iter().copied());
        let mut shaper = builder.build();
        shaper.add_str(text);
        let metrics = shaper.metrics();
        shaper.shape_with(|cluster| {
            let cluster_start = cluster.source.start;
            for g in cluster.glyphs {
                glyphs.push(SwashShapedGlyph {
                    glyph_id: g.id,
                    x_offset: g.x,
                    y_offset: g.y,
                    advance: g.advance,
                    cluster: cluster_start,
                });
                total_advance += g.advance;
            }
        });
        (metrics.ascent, metrics.descent, metrics.leading)
    });

    Some(ShapedGlyphRun {
        glyphs,
        total_advance,
        ascent,
        descent,
        leading,
        font_key,
    })
}

/// 零拷贝 OpenType GSUB/GPOS 文本塑形（带跨帧/跨折行 `Arc<ShapedGlyphRun>` 缓存与可变字体轴支持）。
///
/// 底层自动从 `display::font_ttf` 借用已内存映射的字体字节（系统字体或通过 `register_font_*`
/// 注册的自定义字体），不额外拷贝或重读磁盘。
pub fn shape_text_run_swash(
    text: &str,
    font_spec: FontSpec,
    font_size: f32,
    ligatures: bool,
    variations: &[FontVariationAxis],
) -> Option<Arc<ShapedGlyphRun>> {
    if text.is_empty() {
        return Some(Arc::new(ShapedGlyphRun {
            glyphs: Vec::new(),
            total_advance: 0.0,
            ascent: font_size.max(8.0) * 0.8,
            descent: font_size.max(8.0) * 0.2,
            leading: 0.0,
            font_key: 0,
        }));
    }

    let key = make_cache_key(text, font_spec, font_size, ligatures, variations);
    let cache = SWASH_RUN_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(read_guard) = cache.read() {
        if let Some(hit) = read_guard.get(&key) {
            SWASH_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            return Some(Arc::clone(hit));
        }
    }

    let sample_char = text.chars().find(|c| !c.is_whitespace()).unwrap_or('A');
    let run = with_styled_font_bytes(sample_char, font_spec, |bytes, _font_key| {
        shape_text_run_with_font_bytes(text, bytes, 0, font_size, ligatures, variations)
    })??;

    let arc_run = Arc::new(run);
    SWASH_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut write_guard) = cache.write() {
        if write_guard.len() >= MAX_SWASH_RUN_CACHE_ENTRIES {
            write_guard.clear();
        }
        write_guard.insert(key, Arc::clone(&arc_run));
    }
    Some(arc_run)
}

/// 按 `GlyphId` 与可变字体轴直接光栅化单个字形为 [`CachedGlyph`]（支持彩色 Emoji 与可变字重轮廓）。
pub fn rasterize_swash_glyph(
    font_spec: FontSpec,
    sample_char: char,
    glyph_id: u16,
    font_size: f32,
    variations: &[FontVariationAxis],
) -> Option<CachedGlyph> {
    with_styled_font_bytes(sample_char, font_spec, |bytes, _key| {
        rasterize_swash_glyph_with_bytes(bytes, 0, glyph_id, font_size, variations)
    })?
}

/// 直接基于字体原始字节光栅化指定 `GlyphId`（含可变字体轴插值与彩色位图/轮廓回退）。
pub fn rasterize_swash_glyph_with_bytes(
    font_bytes: &[u8],
    font_index: usize,
    glyph_id: u16,
    font_size: f32,
    variations: &[FontVariationAxis],
) -> Option<CachedGlyph> {
    let font = FontRef::from_index(font_bytes, font_index)?;
    let fs = font_size.clamp(6.0, 255.0);
    let adv_px = font
        .glyph_metrics(&[])
        .scale(fs)
        .advance_width(glyph_id)
        .round()
        .clamp(0.0, i16::MAX as f32) as i16;

    TLS_SCALE_CTX.with(|cell| {
        let mut ctx = cell.borrow_mut();
        let mut builder = ctx.builder(font).size(fs).hint(true);
        if !variations.is_empty() {
            builder = builder.variations(variations.iter().copied().map(|v| v.to_setting()));
        }
        let mut scaler = builder.build();
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, glyph_id)?;

        let width = image.placement.width as usize;
        let height = image.placement.height as usize;
        let xmin = image.placement.left;
        // swash placement.top 为基线到顶边向上距离，转换为 CachedGlyph 的基线到底边偏移 ymin
        let ymin = image.placement.top - height as i32;
        match image.content {
            swash::scale::image::Content::Color => Some(CachedGlyph {
                width,
                height,
                xmin,
                ymin,
                advance_width: adv_px,
                bitmap: Vec::new(),
                color_rgba: Some(image.data),
            }),
            _ => Some(CachedGlyph {
                width,
                height,
                xmin,
                ymin,
                advance_width: adv_px,
                bitmap: image.data,
                color_rgba: None,
            }),
        }
    })
}

/// 查询当前 `SwashRunCache` 的条目数与命中/未命中累计计数。
pub fn swash_run_cache_stats() -> SwashRunCacheStats {
    let entries = SWASH_RUN_CACHE
        .get()
        .and_then(|c| c.read().ok().map(|r| r.len()))
        .unwrap_or(0);
    SwashRunCacheStats {
        entries,
        hits: SWASH_CACHE_HITS.load(Ordering::Relaxed),
        misses: SWASH_CACHE_MISSES.load(Ordering::Relaxed),
    }
}

/// 清空全局 `SwashRunCache`（例如热替换自定义字体后主动失效缓存）。
pub fn clear_swash_run_cache() {
    if let Some(cache) = SWASH_RUN_CACHE.get() {
        if let Ok(mut w) = cache.write() {
            w.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swash_shapes_latin_and_cjk_with_run_cache_hits() {
        clear_swash_run_cache();
        let before = swash_run_cache_stats();

        let run1 = shape_text_run_swash("AVATAR fi fl -> Velox", FontSpec::SANS, 18.0, true, &[])
            .expect("system primary font should be available for swash shaping");
        assert!(!run1.glyphs.is_empty());
        assert!(run1.total_advance > 50.0);
        assert!(run1.font_key != 0);

        // 相同参数第二次调用应命中 Run 缓存且返回同一 Arc 指针（零重复塑形）
        let run2 = shape_text_run_swash("AVATAR fi fl -> Velox", FontSpec::SANS, 18.0, true, &[])
            .expect("cached run");
        assert!(Arc::ptr_eq(&run1, &run2));

        let after = swash_run_cache_stats();
        assert!(after.hits > before.hits);
        assert!(after.entries >= 1);

        // CJK 回退字体零拷贝借用塑形
        let cjk = shape_text_run_swash("你好世界 Velox", FontSpec::SANS, 16.0, true, &[]);
        if let Some(cjk_run) = cjk {
            assert!(!cjk_run.glyphs.is_empty());
            assert!(cjk_run.total_advance > 30.0);
        }
    }

    #[test]
    fn swash_variation_axes_differentiate_cache_and_rasterize_glyph() {
        clear_swash_run_cache();
        let regular = shape_text_run_swash(
            "Spring Weight",
            FontSpec::SANS,
            24.0,
            true,
            &[FontVariationAxis::weight(400.0)],
        )
        .expect("regular run");
        let bold = shape_text_run_swash(
            "Spring Weight",
            FontSpec::SANS,
            24.0,
            true,
            &[FontVariationAxis::weight(700.0)],
        )
        .expect("bold run");
        // 不同可变字体轴值必须映射到不同的缓存条目
        assert!(!Arc::ptr_eq(&regular, &bold));

        let first_gid = regular.glyphs[0].glyph_id;
        let glyph = rasterize_swash_glyph(
            FontSpec::SANS,
            'S',
            first_gid,
            24.0,
            &[FontVariationAxis::weight(600.0)],
        )
        .expect("swash rasterized glyph");
        assert!(glyph.width > 0 && glyph.height > 0);
        assert!(!glyph.bitmap.is_empty() || glyph.color_rgba.is_some());
    }
}
