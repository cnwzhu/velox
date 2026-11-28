//! 可选零拷贝 OpenType GSUB/GPOS 高级文本塑形、多字体 Itemizer、可变字体轴（Variable Fonts）
//! 与双代轮转 Run 缓存（仅在启用 `--features swash` 时编译）。
//!
//! 设计要点：
//! - **多字体覆盖切分（Itemizer）**：逐字符检测所属已映射字体身份键 `font_key`，自动把中英混排
//!   （如 `"Hello 你好 Velox"`）按字体边界切分为多个连续 Run 分别塑形再拼装，彻底消除 CJK/符号
//!   变 `.notdef` 豆腐块问题；
//! - **零拷贝共享底层字体字节**：直接复用 `display::font_ttf` 中已通过 OS 只读 `mmap` /
//!   `MapViewOfFile` 映射的系统字体及自定义注册字体切片，不重复读取磁盘或二次分配字节缓冲区；
//! - **OpenType GSUB/GPOS 与可变字体轴**：支持标准连字（`liga`/`clig`/`calt`）、精确字距（`kern`）
//!   及动态可变字体轴（`wght`/`wdth`/`slnt`/`opsz`），并支持提取经可变轴插值后的矢量轮廓直出 Slug 2.0；
//! - **双代轮转 `SwashRunCache`**：使用 `hot` + `cold` 两代 `HashMap` 轮转淘汰，命中 `cold` 自动
//!   提升回 `hot`，彻底消除固定上限全量 `clear()` 造成的周期性重塑形尖峰。

use core::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::vec::Vec;

use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
#[cfg(feature = "gpu")]
use swash::zeno::PathData;
use swash::zeno::Format;
use swash::{tag_from_bytes, FontRef, Setting};

use crate::display::font_ttf::{with_font_bytes_by_key, with_styled_font_bytes, CachedGlyph};
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
    pub(crate) fn to_setting(self) -> Setting<f32> {
        Setting {
            tag: tag_from_bytes(&self.tag),
            value: self.value,
        }
    }
}

/// 经 OpenType GSUB/GPOS 塑形后的单个字形位置、步进与所属字体身份信息。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SwashShapedGlyph {
    /// 字体内的真实 `GlyphId`（已包含连字替换、变体替换等 GSUB 结果）。
    pub glyph_id: u16,
    /// 该字形所属底层字体只读切片的稳定身份键（中英混排时每个字形精确指向对应拉丁/CJK 字体）。
    pub font_key: u64,
    /// 相对当前游标的水平微调偏移（像素，来自 GPOS `kern` / `mark` 等）。
    pub x_offset: f32,
    /// 相对基线的垂直微调偏移（像素，正值向上）。
    pub y_offset: f32,
    /// 字形水平前进宽度（像素）。
    pub advance: f32,
    /// 对应的源字符串起始字节偏移（用于光标命中与选区反查）。
    pub cluster: u32,
}

/// 一次 OpenType 多字体塑形输出的完整字形序列与行度量（支持双代轮转缓存跨折行复用）。
#[derive(Clone, Debug)]
pub struct ShapedGlyphRun {
    /// 塑形后的有序字形序列（已按多字体 Itemizer 拼接并重定位 `cluster` 字节偏移）。
    pub glyphs: Vec<SwashShapedGlyph>,
    /// 整段文本的总水平前进宽度（像素）。
    pub total_advance: f32,
    /// 字体升部高度（像素，取参与塑形各字体的最大值）。
    pub ascent: f32,
    /// 字体降部深度（像素，正值表示向下延伸，取最大值）。
    pub descent: f32,
    /// 建议行距附加值（像素）。
    pub leading: f32,
    /// 首个参与塑形字体的身份键（单字体段可直接据此判断）。
    pub font_key: u64,
}

/// `SwashRunCache` 命中统计信息。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SwashRunCacheStats {
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}

const MAX_SWASH_RUN_CACHE_GEN_ENTRIES: usize = 1024;

#[derive(Clone, Eq, PartialEq, Hash)]
struct SwashRunCacheKey {
    text_hash: u64,
    text_len: usize,
    font_spec_packed: u32,
    font_size_q4: u16,
    ligatures: bool,
    variations_hash: u64,
}

/// 双代轮转缓存：`hot` 满 `MAX_SWASH_RUN_CACHE_GEN_ENTRIES` 时整体降级为 `cold` 并开启新 `hot`，
/// 避免全量 `clear()` 带来的周期性冷启动重塑形尖峰。
struct TwoGenRunCache {
    hot: HashMap<SwashRunCacheKey, Arc<ShapedGlyphRun>>,
    cold: HashMap<SwashRunCacheKey, Arc<ShapedGlyphRun>>,
}

impl TwoGenRunCache {
    fn new() -> Self {
        Self {
            hot: HashMap::with_capacity(128),
            cold: HashMap::new(),
        }
    }

    #[inline]
    fn get_hot(&self, key: &SwashRunCacheKey) -> Option<Arc<ShapedGlyphRun>> {
        self.hot.get(key).map(Arc::clone)
    }

    #[inline]
    fn contains_cold(&self, key: &SwashRunCacheKey) -> bool {
        self.cold.contains_key(key)
    }

    fn promote_from_cold(&mut self, key: &SwashRunCacheKey) -> Option<Arc<ShapedGlyphRun>> {
        let entry = self.cold.remove(key)?;
        self.insert(key.clone(), Arc::clone(&entry));
        Some(entry)
    }

    fn insert(&mut self, key: SwashRunCacheKey, val: Arc<ShapedGlyphRun>) {
        if self.hot.len() >= MAX_SWASH_RUN_CACHE_GEN_ENTRIES {
            self.cold = core::mem::take(&mut self.hot);
        }
        self.hot.insert(key, val);
    }

    fn clear(&mut self) {
        self.hot.clear();
        self.cold.clear();
    }

    #[inline]
    fn len(&self) -> usize {
        self.hot.len() + self.cold.len()
    }
}

static SWASH_RUN_CACHE: OnceLock<RwLock<TwoGenRunCache>> = OnceLock::new();
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

/// 计算可变字体轴数组的稳定量化哈希（量化至 `1/16` 轴单位，空切片固定返回 `0`）。
#[inline]
pub(crate) fn hash_variations(variations: &[FontVariationAxis]) -> u64 {
    if variations.is_empty() {
        return 0;
    }
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in variations {
        h = fnv1a_bytes(h, &v.tag);
        let q = (v.value * 16.0).round() as i32;
        h = fnv1a_bytes(h, &q.to_le_bytes());
    }
    if h == 0 {
        1
    } else {
        h
    }
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
                    font_key,
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

/// 多字体 Itemizer：将混合脚本字符串按连续相同底层字体身份键 `font_key` 切分为 `(byte_start, byte_end, sample_char)`。
///
/// 空白字符优先继承前一段活跃字体，保持英文词间连贯性并避免无意义断段。
fn itemize_font_runs(text: &str, font_spec: FontSpec) -> Vec<(usize, usize, char, u64)> {
    let mut runs: Vec<(usize, usize, char, u64)> = Vec::with_capacity(4);
    let mut cur_start = 0usize;
    let mut cur_key: Option<u64> = None;
    let mut cur_sample = 'A';

    for (byte_idx, ch) in text.char_indices() {
        let ch_key = if ch.is_whitespace() {
            if let Some(k) = cur_key {
                k
            } else {
                with_styled_font_bytes(ch, font_spec, |_, k| k).unwrap_or(0)
            }
        } else {
            with_styled_font_bytes(ch, font_spec, |_, k| k).unwrap_or(0)
        };
        match cur_key {
            None => {
                cur_start = byte_idx;
                cur_key = Some(ch_key);
                cur_sample = ch;
            }
            Some(k) if k == ch_key => {
                if cur_sample.is_whitespace() && !ch.is_whitespace() {
                    cur_sample = ch;
                }
            }
            Some(k) => {
                runs.push((cur_start, byte_idx, cur_sample, k));
                cur_start = byte_idx;
                cur_key = Some(ch_key);
                cur_sample = ch;
            }
        }
    }
    if let Some(k) = cur_key {
        runs.push((cur_start, text.len(), cur_sample, k));
    }
    runs
}

/// 零拷贝多字体 OpenType GSUB/GPOS 文本塑形（带双代轮转 `Arc<ShapedGlyphRun>` 缓存与可变字体轴支持）。
///
/// 内置多字体 Itemizer：对中英/符号混排（如 `"Hello 你好 Velox"`）自动按字体覆盖边界切分 Run
/// 分别塑形再拼装，每个 [`SwashShapedGlyph`] 均精确携带其真实的 `font_key` 与 `cluster` 字节偏移。
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
    let cache = SWASH_RUN_CACHE.get_or_init(|| RwLock::new(TwoGenRunCache::new()));
    let mut found_in_cold = false;
    if let Ok(read_guard) = cache.read() {
        if let Some(hit) = read_guard.get_hot(&key) {
            SWASH_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            return Some(hit);
        }
        found_in_cold = read_guard.contains_cold(&key);
    }
    if found_in_cold {
        if let Ok(mut write_guard) = cache.write() {
            if let Some(promoted) = write_guard.promote_from_cold(&key) {
                SWASH_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                return Some(promoted);
            }
        }
    }

    let segments = itemize_font_runs(text, font_spec);
    if segments.is_empty() {
        return None;
    }

    let run = if segments.len() == 1 {
        let (_, _, sample_char, _) = segments[0];
        with_styled_font_bytes(sample_char, font_spec, |bytes, _| {
            shape_text_run_with_font_bytes(text, bytes, 0, font_size, ligatures, variations)
        })??
    } else {
        let mut combined_glyphs = Vec::with_capacity(text.chars().count());
        let mut total_advance = 0.0f32;
        let mut max_ascent = 0.0f32;
        let mut max_descent = 0.0f32;
        let mut max_leading = 0.0f32;
        let mut primary_key = 0u64;

        for (b_start, b_end, sample_char, _seg_key) in segments {
            let sub = &text[b_start..b_end];
            let sub_run_opt = with_styled_font_bytes(sample_char, font_spec, |bytes, _| {
                shape_text_run_with_font_bytes(sub, bytes, 0, font_size, ligatures, variations)
            })
            .flatten();
            if let Some(sub_run) = sub_run_opt {
                if primary_key == 0 {
                    primary_key = sub_run.font_key;
                }
                total_advance += sub_run.total_advance;
                max_ascent = max_ascent.max(sub_run.ascent);
                max_descent = max_descent.max(sub_run.descent);
                max_leading = max_leading.max(sub_run.leading);
                for mut g in sub_run.glyphs {
                    g.cluster = g.cluster.saturating_add(b_start as u32);
                    combined_glyphs.push(g);
                }
            }
        }
        if combined_glyphs.is_empty() {
            return None;
        }
        ShapedGlyphRun {
            glyphs: combined_glyphs,
            total_advance,
            ascent: max_ascent,
            descent: max_descent,
            leading: max_leading,
            font_key: primary_key,
        }
    };

    let arc_run = Arc::new(run);
    SWASH_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut write_guard) = cache.write() {
        write_guard.insert(key, Arc::clone(&arc_run));
    }
    Some(arc_run)
}

/// 供共享 `SlugTextEngine` 提取经可变字体轴插值后的标准 EM 归一化矢量轮廓命令序列
/// （返回 `(units_per_em, advance_em, commands)`，零拷贝借用已映射字体切片）。
#[cfg(feature = "gpu")]
pub(crate) fn extract_swash_glyph_outline_em(
    font_key: u64,
    glyph_id: u16,
    variations: &[FontVariationAxis],
) -> Option<(u16, f32, Vec<swash::zeno::Command>)> {
    with_font_bytes_by_key(font_key, |bytes| {
        let font = FontRef::from_index(bytes, 0)?;
        let upem = font.metrics(&[]).units_per_em.max(1);
        let mut settings: Vec<Setting<f32>> = Vec::with_capacity(variations.len());
        for v in variations {
            settings.push(v.to_setting());
        }
        let coords: Vec<swash::NormalizedCoord> = font
            .variations()
            .normalized_coords(settings.iter().copied())
            .collect();
        let raw_adv = font.glyph_metrics(&coords).advance_width(glyph_id);
        let advance_em = raw_adv / (upem as f32);

        TLS_SCALE_CTX.with(|cell| {
            let mut ctx = cell.borrow_mut();
            // size(0.0) 在 swash 中表示直接以原始字体单位（Font Units / unscaled EM）输出轮廓，
            // 与字号无关，可被 SlugTextEngine 缓存一次后任意 0.1x~1000x 缩放复用。
            let mut builder = ctx.builder(font).size(0.0).hint(false);
            if !variations.is_empty() {
                builder = builder.variations(variations.iter().copied().map(|v| v.to_setting()));
            }
            let mut scaler = builder.build();
            let outline = scaler.scale_outline(glyph_id)?;
            let cmds: Vec<swash::zeno::Command> = outline.path().commands().collect();
            Some((upem, advance_em, cmds))
        })
    })?
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
    fn swash_shapes_latin_and_cjk_mixed_without_notdef_and_hits_cache() {
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

        // 关键回归：中英混排 `"Hello 你好 Velox 世界"` 必须经多字体 Itemizer 切分，绝不能把中文塑形成 .notdef (gid=0)！
        let mixed = shape_text_run_swash("Hello 你好 Velox 世界", FontSpec::SANS, 16.0, true, &[])
            .expect("mixed Latin+CJK shaping");
        assert!(mixed.glyphs.len() >= 14);
        assert!(
            mixed.glyphs.iter().all(|g| g.glyph_id != 0),
            "mixed Latin+CJK run must not contain .notdef (glyph_id=0): {:?}",
            mixed.glyphs
        );
        // 验证 cluster 字节偏移单调不减且覆盖中文字节区间
        let mut last_cluster = 0u32;
        for g in &mixed.glyphs {
            assert!(g.cluster >= last_cluster);
            last_cluster = g.cluster;
        }
        assert!(last_cluster >= 12);
    }

    #[test]
    fn swash_two_gen_cache_rotates_without_full_cliff() {
        let mut cache = TwoGenRunCache::new();
        let dummy = Arc::new(ShapedGlyphRun {
            glyphs: Vec::new(),
            total_advance: 0.0,
            ascent: 12.0,
            descent: 4.0,
            leading: 0.0,
            font_key: 1,
        });
        for i in 0..(MAX_SWASH_RUN_CACHE_GEN_ENTRIES + 10) {
            let k = SwashRunCacheKey {
                text_hash: i as u64,
                text_len: 4,
                font_spec_packed: 0,
                font_size_q4: 64,
                ligatures: true,
                variations_hash: 0,
            };
            cache.insert(k, Arc::clone(&dummy));
        }
        // 触发一次代际轮转后，cold 仍保留上一代全部 1024 条，不发生全量清空尖峰
        assert_eq!(cache.hot.len(), 10);
        assert_eq!(cache.cold.len(), MAX_SWASH_RUN_CACHE_GEN_ENTRIES);
        let old_key = SwashRunCacheKey {
            text_hash: 42,
            text_len: 4,
            font_spec_packed: 0,
            font_size_q4: 64,
            ligatures: true,
            variations_hash: 0,
        };
        assert!(cache.promote_from_cold(&old_key).is_some());
        assert_eq!(cache.hot.len(), 11);
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
