//! TrueType 矢量字体按需解析、轮廓光栅化与 `GlyphAtlas` 内存缓存（仅在 `feature = "std"` 下启用）。
//!
//! 设计原则：
//! - **零启动全量解码**：使用 `ttf_parser::Face` 直接零拷贝读取字体偏移表，按需解析实际渲染字符的字形轮廓；
//! - **主字体与 CJK 回退分离**：默认仅加载轻量级主 UI 字体（如 `segoeui.ttf` / `DejaVuSans.ttf`），仅在遇到缺失的非 ASCII 字符时才按需惰性加载 CJK 大字库（如 `msyh.ttc`）；
//! - **布局测宽零位图分配**：`measure_ttf_char_advance` 直接读取 `hmtx` 步进宽度并缓存，不再为测宽触发字形光栅化。

use core::cell::RefCell;
use std::boxed::Box;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use std::vec;
use std::vec::Vec;

use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// 缓存的单字形 256 级抗锯齿位图（及可选彩色 Emoji RGBA8888 位图）与度量信息。
#[derive(Clone)]
pub struct CachedGlyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    #[allow(dead_code)]
    pub advance_width: i16,
    pub bitmap: Vec<u8>,
    /// 彩色 Emoji / 彩色位图字形的预乘 RGBA8888 像素流（`width * height * 4`；普通单色字形为 `None`）。
    pub color_rgba: Option<Vec<u8>>,
}

struct LoadedFont {
    bytes: &'static [u8],
    face: Face<'static>,
}

#[cfg(not(target_arch = "wasm32"))]
const PRIMARY_FONT_CANDIDATES: &[&str] = &[
    "/system.ttf",
    "/system/fonts/RobotoStatic-Regular.ttf",
    "/system/fonts/Roboto-Regular.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
];

#[cfg(not(target_arch = "wasm32"))]
const FALLBACK_CJK_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\msyhbd.ttc",
    "C:\\Windows\\Fonts\\simhei.ttf",
    "/System/Library/Fonts/PingFang.ttc",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
];

#[cfg(not(target_arch = "wasm32"))]
static PRIMARY_BYTES: OnceLock<Option<Box<[u8]>>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static PRIMARY_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
static FALLBACK_CJK_BYTES: OnceLock<Option<Box<[u8]>>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static FALLBACK_CJK_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();

#[cfg(target_arch = "wasm32")]
static WASM_SUBSET_TTF_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wasm_subset.ttf"));
#[cfg(target_arch = "wasm32")]
static WASM_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();

static GLYPH_CACHE: OnceLock<RwLock<HashMap<(char, u16), Arc<CachedGlyph>>>> = OnceLock::new();

const TLS_SLOT_COUNT: usize = 256;
const MAX_GLYPH_CACHE_ENTRIES: usize = 4096;

std::thread_local! {
    /// 线程本地直接映射字形位图缓存（键为 `(char, font_size_q4)`，支持 0.25px 亚像素精度字号）。
    static TLS_GLYPHS: RefCell<[Option<(char, u16, Arc<CachedGlyph>)>; TLS_SLOT_COUNT]> =
        RefCell::new([const { None }; TLS_SLOT_COUNT]);

    /// 线程本地高精度浮点步进宽度缓存：文本布局测宽、折行与字形排布共享同一真源，零位图光栅化开销。
    static TLS_ADVANCES: RefCell<[Option<(char, u8, f32)>; TLS_SLOT_COUNT]> =
        RefCell::new([const { None }; TLS_SLOT_COUNT]);

    /// 线程本地 TrueType `kern` 高精度浮点字偶距微调缓存（`(prev, curr, font_size, kern_px_f32)`）。
    static TLS_KERNING: RefCell<[Option<(char, char, u8, f32)>; TLS_SLOT_COUNT]> =
        RefCell::new([const { None }; TLS_SLOT_COUNT]);
}

#[cfg(not(target_arch = "wasm32"))]
fn read_first_valid_font(candidates: &[&str]) -> Option<Box<[u8]>> {
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            if Face::parse(bytes.as_slice(), 0).is_ok() {
                return Some(bytes.into_boxed_slice());
            }
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn primary_font() -> Option<&'static LoadedFont> {
    PRIMARY_FONT
        .get_or_init(|| {
            let bytes = PRIMARY_BYTES
                .get_or_init(|| {
                    read_first_valid_font(PRIMARY_FONT_CANDIDATES)
                        .or_else(|| read_first_valid_font(FALLBACK_CJK_CANDIDATES))
                })
                .as_deref()?;
            let face = Face::parse(bytes, 0).ok()?;
            Some(LoadedFont { bytes, face })
        })
        .as_ref()
}

#[cfg(not(target_arch = "wasm32"))]
fn fallback_cjk_font() -> Option<&'static LoadedFont> {
    FALLBACK_CJK_FONT
        .get_or_init(|| {
            let bytes = FALLBACK_CJK_BYTES
                .get_or_init(|| read_first_valid_font(FALLBACK_CJK_CANDIDATES))
                .as_deref()?;
            let face = Face::parse(bytes, 0).ok()?;
            Some(LoadedFont { bytes, face })
        })
        .as_ref()
}

#[cfg(target_arch = "wasm32")]
fn primary_font() -> Option<&'static LoadedFont> {
    WASM_FONT
        .get_or_init(|| {
            if WASM_SUBSET_TTF_BYTES.is_empty() {
                return None;
            }
            let face = Face::parse(WASM_SUBSET_TTF_BYTES, 0).ok()?;
            Some(LoadedFont {
                bytes: WASM_SUBSET_TTF_BYTES,
                face,
            })
        })
        .as_ref()
}

#[inline]
fn select_font_and_glyph(ch: char) -> Option<(&'static LoadedFont, GlyphId)> {
    let primary = primary_font()?;
    if let Some(gid) = primary.face.glyph_index(ch) {
        if gid.0 != 0 {
            return Some((primary, gid));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !ch.is_ascii() && !ch.is_control() {
        if let Some(cjk) = fallback_cjk_font() {
            if let Some(gid) = cjk.face.glyph_index(ch) {
                if gid.0 != 0 {
                    return Some((cjk, gid));
                }
            }
        }
    }
    let fallback_gid = primary.face.glyph_index(ch).unwrap_or(GlyphId(0));
    Some((primary, fallback_gid))
}

/// 在后台线程中并行预热系统主 TrueType 字体（可选同时预热 CJK 字体），
/// 使其磁盘读取与桌面 GPU 管线初始化并发重叠，缩短冷启动首屏耗时。
#[cfg(all(feature = "winit", not(target_arch = "wasm32")))]
pub(crate) fn spawn_font_preload(preload_cjk: bool) {
    if PRIMARY_FONT.get().is_some() && (!preload_cjk || FALLBACK_CJK_FONT.get().is_some()) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name(std::string::String::from("velox-font-preload"))
        .stack_size(128 * 1024)
        .spawn(move || {
            let _ = primary_font();
            if preload_cjk {
                let _ = fallback_cjk_font();
            }
        });
}

/// 判断宿主是否存在可用的系统 TrueType 矢量字体（绝不触发 CJK 回退字库加载）。
#[inline]
pub fn has_system_font() -> bool {
    primary_font().is_some()
}

/// 获取自动加载的系统主 TrueType 字体原始字节流。
#[allow(dead_code)]
pub fn get_system_font_bytes() -> Option<&'static [u8]> {
    primary_font().map(|f| f.bytes)
}

/// 解析指定字符应当使用的系统字体原始字节切片与字形 ID（支持惰性 CJK 回退）。
#[allow(dead_code)]
#[inline]
pub fn resolve_char_font_and_glyph(ch: char) -> Option<(&'static [u8], u16)> {
    let (font, gid) = select_font_and_glyph(ch)?;
    Some((font.bytes, gid.0))
}

/// 获取指定精确浮点字号（`fs`）在单行行盒内的光学垂直居中基线偏移（像素），
/// 基于字体真实 `capital_height`（大写字母高度，缺省回退 `0.70 * upem`）居中，避免 `hhea.ascender` 过大导致按钮文字偏下。
pub fn get_font_ascent_f32(fs: f32) -> i32 {
    let fs = fs.max(8.0);
    let fs_i = fs.round() as i32;
    if let Some(font) = primary_font() {
        let upem = font.face.units_per_em().max(1) as f32;
        let scale = fs / upem;
        let cap_h = font
            .face
            .capital_height()
            .filter(|&h| h > 0)
            .map(|h| (h as f32) * scale)
            .unwrap_or(fs * 0.70);
        // 光学居中：让大写字母墨水区间 `[base_y - cap_h, base_y]` 精确居中于 `[0, fs]` 行盒中
        let centered = ((fs + cap_h) * 0.5 + 0.5) as i32;
        return centered.clamp(1, (fs_i - 2).max(1));
    }
    (fs_i * 4) / 5
}

/// 获取指定整数字号（`font_size`）在单行行盒内的光学垂直居中基线偏移（像素）。
#[allow(dead_code)]
#[inline]
pub fn get_font_ascent(font_size: u8) -> i32 {
    get_font_ascent_f32((font_size.max(8)) as f32)
}

/// 在浮点 Font Units 空间内精确累加整行 TrueType 水平步进宽度与 `kern` 字偶距（不做向上取整），
/// 供光标定位、选区测量与鼠标命中测试使用，与 `WgpuRenderer` / 软件渲染字形排布共享同一浮点真源。
pub fn measure_ttf_line_width_f32(line: &str, font_size: u8) -> Option<f32> {
    if primary_font().is_none() {
        return None;
    }
    if line.is_empty() {
        return Some(0.0);
    }
    let mut total_px = 0.0f32;
    let mut prev: Option<char> = None;

    for ch in line.chars() {
        if let Some(p) = prev {
            total_px += measure_ttf_kerning_f32(p, ch, font_size);
        }
        let adv = measure_ttf_char_advance_f32(ch, font_size)?;
        total_px = (total_px + adv).max(0.0);
        prev = Some(ch);
    }
    Some(total_px)
}

/// 在浮点 Font Units 空间内精确累加整行 TrueType 水平步进宽度与 `kern` 字偶距（最后单次向上取整），
/// 与 `measure_ttf_char_advance_f32` / `measure_ttf_kerning_f32` 复用同一 TLS 浮点缓存真源，
/// 彻底消除布局测宽、自动折行与渲染字形推进之间的累加误差。
pub fn measure_ttf_line_width_exact(line: &str, font_size: u8) -> Option<i16> {
    let total_px = measure_ttf_line_width_f32(line, font_size)?;
    if total_px <= 0.0 {
        return Some(0);
    }
    Some((total_px.ceil() as i32).clamp(1, i16::MAX as i32) as i16)
}

#[inline]
fn compute_scaled_advance_f32(face: &Face<'_>, gid: GlyphId, font_size: u8) -> f32 {
    let fs = font_size.max(8) as f32;
    let upem = face.units_per_em().max(1) as f32;
    let raw_adv = face.glyph_hor_advance(gid).unwrap_or(0) as f32;
    ((raw_adv * fs) / upem).max(1.0)
}

/// 零位图光栅化、零锁查询指定字符在目标字号下的高精度浮点步进宽度（`hmtx` TLS 缓存直出）。
#[inline]
pub fn measure_ttf_char_advance_f32(ch: char, font_size: u8) -> Option<f32> {
    let slot_idx = (((ch as usize).wrapping_mul(2654435761)) ^ (font_size as usize))
        & (TLS_SLOT_COUNT - 1);
    if let Some(adv) = TLS_ADVANCES.with(|tls| {
        let borrow = tls.borrow();
        match borrow[slot_idx] {
            Some((c, fs, a)) if c == ch && fs == font_size => Some(a),
            _ => None,
        }
    }) {
        return Some(adv);
    }

    let (font, gid) = select_font_and_glyph(ch)?;
    let adv = compute_scaled_advance_f32(&font.face, gid, font_size);
    TLS_ADVANCES.with(|tls| {
        tls.borrow_mut()[slot_idx] = Some((ch, font_size, adv));
    });
    Some(adv)
}

/// 零位图光栅化、零原子引用计数开销地查询指定字符在目标字号下的整数步进宽度（`hmtx` 缓存直出）。
#[inline]
pub fn measure_ttf_char_advance(ch: char, font_size: u8) -> Option<i16> {
    measure_ttf_char_advance_f32(ch, font_size).map(|a| (a + 0.5) as i16)
}

/// 零拷贝查询 TrueType `kern` 表中相邻字符 `(prev, curr)` 在目标字号下的高精度浮点水平字距微调像素（带 TLS 直接映射缓存）。
#[inline]
pub fn measure_ttf_kerning_f32(prev: char, curr: char, font_size: u8) -> f32 {
    if prev.is_control() || curr.is_control() {
        return 0.0;
    }
    let slot_idx = (((prev as usize).wrapping_mul(2654435761))
        ^ ((curr as usize).wrapping_mul(2246822519))
        ^ ((font_size as usize) << 3))
        & (TLS_SLOT_COUNT - 1);

    if let Some(k) = TLS_KERNING.with(|tls| {
        let borrow = tls.borrow();
        match borrow[slot_idx] {
            Some((p, c, fs, val)) if p == prev && c == curr && fs == font_size => Some(val),
            _ => None,
        }
    }) {
        return k;
    }

    let kern_px = (|| -> Option<f32> {
        let (f1, g1) = select_font_and_glyph(prev)?;
        let (f2, g2) = select_font_and_glyph(curr)?;
        if !core::ptr::eq(f1, f2) {
            return Some(0.0);
        }
        let kern_table = f1.face.tables().kern?;
        let mut raw_kern: i32 = 0;
        for subtable in kern_table.subtables {
            if subtable.horizontal && !subtable.variable {
                if let Some(v) = subtable.glyphs_kerning(g1, g2) {
                    raw_kern += v as i32;
                }
            }
        }
        if raw_kern == 0 {
            return Some(0.0);
        }
        let fs = font_size.max(8) as f32;
        let upem = f1.face.units_per_em().max(1) as f32;
        Some(((raw_kern as f32) * fs / upem).clamp(-32.0, 32.0))
    })()
    .unwrap_or(0.0);

    TLS_KERNING.with(|tls| {
        tls.borrow_mut()[slot_idx] = Some((prev, curr, font_size, kern_px));
    });
    kern_px
}

/// 零拷贝查询 TrueType `kern` 表中相邻字符 `(prev, curr)` 在目标字号下的整数水平字距微调像素（带 TLS 直接映射缓存）。
///
/// 对 `"AV"`、`"To"`、`"Wa"` 等典型字偶距返回负像素收紧量；非同字体或无 `kern` 表时快速返回 `0`。
#[inline]
pub fn measure_ttf_kerning(prev: char, curr: char, font_size: u8) -> i16 {
    measure_ttf_kerning_f32(prev, curr, font_size).round() as i16
}

#[derive(Copy, Clone)]
struct EdgeSegment {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

struct ScaledOutlineCollector {
    scale: f32,
    subpath_start: Option<(f32, f32)>,
    current: (f32, f32),
    edges: Vec<EdgeSegment>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl ScaledOutlineCollector {
    fn new(scale: f32) -> Self {
        Self {
            scale,
            subpath_start: None,
            current: (0.0, 0.0),
            edges: Vec::with_capacity(32),
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    #[inline]
    fn to_screen(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.scale, -y * self.scale)
    }

    #[inline]
    fn push_edge(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.min_x = self.min_x.min(x0).min(x1);
        self.max_x = self.max_x.max(x0).max(x1);
        self.min_y = self.min_y.min(y0).min(y1);
        self.max_y = self.max_y.max(y0).max(y1);
        if (y1 - y0).abs() > 1e-6 {
            self.edges.push(EdgeSegment { x0, y0, x1, y1 });
        }
    }

    fn finish_subpath(&mut self) {
        if let Some((sx, sy)) = self.subpath_start.take() {
            let (cx, cy) = self.current;
            if (cx - sx).abs() > 1e-5 || (cy - sy).abs() > 1e-5 {
                self.push_edge(cx, cy, sx, sy);
            }
            self.current = (sx, sy);
        }
    }
}

impl OutlineBuilder for ScaledOutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_subpath();
        let pt = self.to_screen(x, y);
        self.subpath_start = Some(pt);
        self.current = pt;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x0, y0) = self.current;
        let (x1, y1) = self.to_screen(x, y);
        if (x1 - x0).abs() > 1e-6 || (y1 - y0).abs() > 1e-6 {
            self.push_edge(x0, y0, x1, y1);
        }
        self.current = (x1, y1);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (sx0, sy0) = self.current;
        let (cx, cy) = self.to_screen(x1, y1);
        let (sx1, sy1) = self.to_screen(x, y);

        let err = (cx - 0.5 * (sx0 + sx1)).abs() + (cy - 0.5 * (sy0 + sy1)).abs();
        let steps = if err <= 0.1 {
            1usize
        } else {
            ((err * 2.5).sqrt().ceil() as usize).clamp(2, 32)
        };

        let mut px = sx0;
        let mut py = sy0;
        let inv = 1.0 / (steps as f32);
        for i in 1..=steps {
            let t = (i as f32) * inv;
            let u = 1.0 - t;
            let nx = if i == steps {
                sx1
            } else {
                u * u * sx0 + 2.0 * u * t * cx + t * t * sx1
            };
            let ny = if i == steps {
                sy1
            } else {
                u * u * sy0 + 2.0 * u * t * cy + t * t * sy1
            };
            self.push_edge(px, py, nx, ny);
            px = nx;
            py = ny;
        }
        self.current = (sx1, sy1);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (sx0, sy0) = self.current;
        let (cx1, cy1) = self.to_screen(x1, y1);
        let (cx2, cy2) = self.to_screen(x2, y2);
        let (sx1, sy1) = self.to_screen(x, y);

        let lx1 = sx0 + (sx1 - sx0) * (1.0 / 3.0);
        let ly1 = sy0 + (sy1 - sy0) * (1.0 / 3.0);
        let lx2 = sx0 + (sx1 - sx0) * (2.0 / 3.0);
        let ly2 = sy0 + (sy1 - sy0) * (2.0 / 3.0);
        let err1 = (cx1 - lx1).abs() + (cy1 - ly1).abs();
        let err2 = (cx2 - lx2).abs() + (cy2 - ly2).abs();
        let err = err1.max(err2);
        let steps = if err <= 0.1 {
            1usize
        } else {
            ((err * 3.0).sqrt().ceil() as usize).clamp(2, 48)
        };

        let mut px = sx0;
        let mut py = sy0;
        let inv = 1.0 / (steps as f32);
        for i in 1..=steps {
            let t = (i as f32) * inv;
            let u = 1.0 - t;
            let u2 = u * u;
            let t2 = t * t;
            let nx = if i == steps {
                sx1
            } else {
                u2 * u * sx0 + 3.0 * u2 * t * cx1 + 3.0 * u * t2 * cx2 + t2 * t * sx1
            };
            let ny = if i == steps {
                sy1
            } else {
                u2 * u * sy0 + 3.0 * u2 * t * cy1 + 3.0 * u * t2 * cy2 + t2 * t * sy1
            };
            self.push_edge(px, py, nx, ny);
            px = nx;
            py = ny;
        }
        self.current = (sx1, sy1);
    }

    fn close(&mut self) {
        self.finish_subpath();
    }
}

/// 使用非零缠绕解析梯形有向面积积分光栅化单个字符轮廓为 `A8` 抗锯齿位图（支持 0.25px 亚像素精度字号）。
fn rasterize_glyph_outline(face: &Face<'_>, gid: GlyphId, fs: f32) -> CachedGlyph {
    let fs = fs.max(8.0);
    let upem = face.units_per_em().max(1) as f32;
    let raw_adv = face.glyph_hor_advance(gid).unwrap_or(0) as f32;
    let adv = (((raw_adv * fs) / upem).max(1.0) + 0.5) as i16;
    let scale = fs / upem;

    let mut collector = ScaledOutlineCollector::new(scale);
    let outlined = face.outline_glyph(gid, &mut collector).is_some();
    collector.finish_subpath();

    if !outlined || collector.edges.is_empty() || !collector.min_x.is_finite() {
        return CachedGlyph {
            width: 0,
            height: 0,
            xmin: 0,
            ymin: 0,
            advance_width: adv,
            bitmap: Vec::new(),
            color_rgba: None,
        };
    }

    let px_min_x = collector.min_x.floor() as i32;
    let px_max_x = collector.max_x.ceil() as i32;
    let px_min_y = collector.min_y.floor() as i32;
    let px_max_y = collector.max_y.ceil() as i32;

    let w = (px_max_x - px_min_x).clamp(0, 512) as usize;
    let h = (px_max_y - px_min_y).clamp(0, 512) as usize;
    if w == 0 || h == 0 {
        return CachedGlyph {
            width: 0,
            height: 0,
            xmin: 0,
            ymin: 0,
            advance_width: adv,
            bitmap: Vec::new(),
            color_rgba: None,
        };
    }

    let ox = px_min_x as f32;
    let oy = px_min_y as f32;
    let wf = w as f32;
    let area_stride = w + 1;
    let mut area = vec![0.0f32; area_stride * h];

    for seg in &collector.edges {
        let sx0 = seg.x0 - ox;
        let sy0 = seg.y0 - oy;
        let sx1 = seg.x1 - ox;
        let sy1 = seg.y1 - oy;

        let (dir, ax0, ay0, ax1, ay1) = if sy1 > sy0 {
            (1.0f32, sx0, sy0, sx1, sy1)
        } else {
            (-1.0f32, sx1, sy1, sx0, sy0)
        };
        let dy_total = ay1 - ay0;
        if dy_total <= 1e-6 {
            continue;
        }

        let r_start = (ay0.floor() as isize).max(0) as usize;
        let r_end = ((ay1.ceil() as isize).min(h as isize)).max(0) as usize;
        let inv_dy = 1.0 / dy_total;

        for row in r_start..r_end {
            let rf0 = row as f32;
            let rf1 = (row + 1) as f32;
            let ry0 = ay0.max(rf0);
            let ry1 = ay1.min(rf1);
            let dh = ry1 - ry0;
            if dh <= 1e-6 {
                continue;
            }

            let t0 = (ry0 - ay0) * inv_dy;
            let t1 = (ry1 - ay0) * inv_dy;
            let rx0 = (ax0 + (ax1 - ax0) * t0).clamp(0.0, wf);
            let rx1 = (ax0 + (ax1 - ax0) * t1).clamp(0.0, wf);
            let row_dh = dir * dh;
            let row_off = row * area_stride;

            let x_min = rx0.min(rx1);
            let x_max = rx0.max(rx1);
            if x_min >= wf {
                continue;
            }

            let dx_abs = x_max - x_min;
            if dx_abs <= 1e-6 {
                let x_mid = 0.5 * (rx0 + rx1);
                if x_mid < wf {
                    let col = (x_mid.floor() as usize).min(w - 1);
                    let frac = (x_mid - (col as f32)).clamp(0.0, 1.0);
                    area[row_off + col] += row_dh * (1.0 - frac);
                    area[row_off + col + 1] += row_dh * frac;
                }
            } else {
                let c_first = (x_min.floor() as usize).min(w - 1);
                let c_last = ((x_max.ceil() as usize).saturating_sub(1)).min(w - 1);
                let inv_dx = 1.0 / dx_abs;
                for col in c_first..=c_last {
                    let cf0 = col as f32;
                    let cf1 = (col + 1) as f32;
                    let cx0 = x_min.max(cf0);
                    let cx1 = x_max.min(cf1);
                    if cx1 <= cx0 {
                        continue;
                    }
                    let sub_dh = row_dh * ((cx1 - cx0) * inv_dx);
                    let mid_frac = (0.5 * (cx0 + cx1) - cf0).clamp(0.0, 1.0);
                    area[row_off + col] += sub_dh * (1.0 - mid_frac);
                    area[row_off + col + 1] += sub_dh * mid_frac;
                }
            }
        }
    }

    let mut bitmap = vec![0u8; w * h];
    for row in 0..h {
        let row_off = row * area_stride;
        let dst_off = row * w;
        let mut acc = 0.0f32;
        for col in 0..w {
            acc += area[row_off + col];
            let cov = acc.abs().min(1.0);
            bitmap[dst_off + col] = (cov * 255.0 + 0.5) as u8;
        }
    }

    CachedGlyph {
        width: w,
        height: h,
        xmin: px_min_x,
        // 对齐 draw_ttf_glyph: gy = base_y - height - ymin == base_y + px_min_y
        ymin: -px_max_y,
        advance_width: adv,
        bitmap,
        color_rgba: None,
    }
}

// ============================================================================
// 多字族 / 多字重 / 斜体 / 自定义字体注册表与彩色 Emoji 支持
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
const BOLD_FONT_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\segoeuib.ttf",
    "C:\\Windows\\Fonts\\arialbd.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
];

#[cfg(not(target_arch = "wasm32"))]
const ITALIC_FONT_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\segoeuii.ttf",
    "C:\\Windows\\Fonts\\ariali.ttf",
    "/System/Library/Fonts/Supplemental/Arial Italic.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
    "/usr/share/fonts/noto/NotoSans-Italic.ttf",
];

#[cfg(not(target_arch = "wasm32"))]
const MONO_FONT_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\consola.ttf",
    "C:\\Windows\\Fonts\\CascadiaMono.ttf",
    "C:\\Windows\\Fonts\\cour.ttf",
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/SFNSMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
];

#[cfg(not(target_arch = "wasm32"))]
const MONO_BOLD_FONT_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\consolab.ttf",
    "C:\\Windows\\Fonts\\courbd.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
];

#[cfg(not(target_arch = "wasm32"))]
const SERIF_FONT_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\georgia.ttf",
    "C:\\Windows\\Fonts\\times.ttf",
    "/System/Library/Fonts/Times.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
];

#[cfg(not(target_arch = "wasm32"))]
static BOLD_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static ITALIC_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static MONO_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static MONO_BOLD_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
static SERIF_FONT: OnceLock<Option<LoadedFont>> = OnceLock::new();

enum FontBacking {
    Static,
    Owned(Box<[u8]>),
}

impl FontBacking {
    #[inline]
    fn owned_bytes(&self) -> usize {
        match self {
            Self::Static => 0,
            Self::Owned(b) => b.len(),
        }
    }
}

/// 持有自身字体字节所有权的动态注册字体，替换或注销时由 `Arc` 自动释放底层堆内存（零 `Box::leak`）。
struct OwnedCustomFont {
    // 声明顺序保证 `face` 先于 `_backing` 结束生命周期（`Face` 为纯只读借用切片视图，无自定义 `Drop`）。
    face: Face<'static>,
    _backing: FontBacking,
}

// SAFETY: `Face<'static>` 与不可变只读 `FontBacking` 均无内部可变性，跨线程只读共享安全。
unsafe impl Send for OwnedCustomFont {}
unsafe impl Sync for OwnedCustomFont {}

impl OwnedCustomFont {
    fn from_static(bytes: &'static [u8]) -> Option<Self> {
        let face = Face::parse(bytes, 0).ok()?;
        Some(Self {
            face,
            _backing: FontBacking::Static,
        })
    }

    fn from_vec(bytes: Vec<u8>) -> Option<Self> {
        let boxed = bytes.into_boxed_slice();
        let ptr = boxed.as_ptr();
        let len = boxed.len();
        // SAFETY: `boxed` 堆缓冲区地址稳定且由 `_backing` 与 `face` 绑定在同一对象生命周期内；
        // 本结构仅对外暴露与 `&self` 绑定的 `&Face<'_>`，不会泄露 `'static` 引用。
        let static_view: &'static [u8] = unsafe { core::slice::from_raw_parts(ptr, len) };
        let face = Face::parse(static_view, 0).ok()?;
        Some(Self {
            face,
            _backing: FontBacking::Owned(boxed),
        })
    }

    #[inline]
    fn face(&self) -> &Face<'_> {
        &self.face
    }

    #[inline]
    fn owned_bytes(&self) -> usize {
        self._backing.owned_bytes()
    }
}

enum SelectedFont {
    Static(&'static LoadedFont),
    Custom(Arc<OwnedCustomFont>),
}

impl SelectedFont {
    #[inline]
    fn face(&self) -> &Face<'_> {
        match self {
            Self::Static(f) => &f.face,
            Self::Custom(f) => f.face(),
        }
    }
}

static CUSTOM_FONTS: OnceLock<RwLock<HashMap<u32, Arc<OwnedCustomFont>>>> = OnceLock::new();
static STYLED_GLYPH_CACHE: OnceLock<RwLock<HashMap<(char, u16, u32), Arc<CachedGlyph>>>> =
    OnceLock::new();

#[inline]
fn font_spec_family_bits(packed_key: u32) -> u32 {
    packed_key >> 16
}

fn invalidate_styled_glyphs_for_spec(spec: crate::text_rich::FontSpec) {
    let target_family = font_spec_family_bits(spec.packed_key());
    if let Some(cache) = STYLED_GLYPH_CACHE.get() {
        if let Ok(mut c) = cache.write() {
            // 仅淘汰同字族（含其粗体/斜体回退条目）的字形缓存，保留其他字族的热缓存。
            c.retain(|&(_, _, key), _| font_spec_family_bits(key) != target_family);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_static_candidate(candidates: &[&str]) -> Option<LoadedFont> {
    let boxed = read_first_valid_font(candidates)?;
    let leaked: &'static [u8] = Box::leak(boxed);
    let face = Face::parse(leaked, 0).ok()?;
    Some(LoadedFont {
        bytes: leaked,
        face,
    })
}

/// 注册静态 TrueType/OpenType 字体字节流到指定 [`FontSpec`](crate::text_rich::FontSpec)（支持自定义字族、粗体、斜体与等宽字体）。
///
/// 重复注册同一 `FontSpec` 时会自动释放旧条目并仅失效对应字族的字形缓存（零 `Box::leak`）。
pub fn register_font_bytes(spec: crate::text_rich::FontSpec, bytes: &'static [u8]) -> bool {
    let Some(owned) = OwnedCustomFont::from_static(bytes) else {
        return false;
    };
    let map = CUSTOM_FONTS.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut w) = map.write() {
        w.insert(spec.packed_key(), Arc::new(owned));
    }
    invalidate_styled_glyphs_for_spec(spec);
    true
}

/// 注册堆分配 TrueType/OpenType 字体字节向量到指定 [`FontSpec`](crate::text_rich::FontSpec)。
///
/// 字体字节由内部 `Arc` 真正拥有所有权：重复注册同一 `FontSpec` 或调用 [`unregister_font`] 时旧字节立即释放，零内存泄漏。
pub fn register_font_vec(spec: crate::text_rich::FontSpec, bytes: Vec<u8>) -> bool {
    let Some(owned) = OwnedCustomFont::from_vec(bytes) else {
        return false;
    };
    let map = CUSTOM_FONTS.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut w) = map.write() {
        w.insert(spec.packed_key(), Arc::new(owned));
    }
    invalidate_styled_glyphs_for_spec(spec);
    true
}

/// 注销指定 [`FontSpec`](crate::text_rich::FontSpec) 的自定义字体，释放其堆字体字节并清理对应字族字形缓存。
pub fn unregister_font(spec: crate::text_rich::FontSpec) -> bool {
    let removed = CUSTOM_FONTS
        .get()
        .and_then(|m| m.write().ok().map(|mut w| w.remove(&spec.packed_key()).is_some()))
        .unwrap_or(false);
    if removed {
        invalidate_styled_glyphs_for_spec(spec);
    }
    removed
}

/// 查询当前已注册的自定义字体数量。
pub fn registered_custom_font_count() -> usize {
    CUSTOM_FONTS
        .get()
        .and_then(|m| m.read().ok().map(|r| r.len()))
        .unwrap_or(0)
}

/// 查询当前所有通过 [`register_font_vec`] 注册的自定义字体在堆上持有的总字节数。
pub fn registered_custom_font_bytes() -> usize {
    CUSTOM_FONTS
        .get()
        .and_then(|m| {
            m.read()
                .ok()
                .map(|r| r.values().map(|f| f.owned_bytes()).sum())
        })
        .unwrap_or(0)
}

fn select_styled_font_and_glyph(
    ch: char,
    spec: crate::text_rich::FontSpec,
) -> Option<(SelectedFont, GlyphId, bool, bool)> {
    use crate::text_rich::{FontFamily, FontStyle};
    // 返回 (font, gid, need_faux_bold, need_faux_italic)
    if let Some(map) = CUSTOM_FONTS.get() {
        if let Ok(r) = map.read() {
            if let Some(f) = r.get(&spec.packed_key()) {
                if let Some(gid) = f.face().glyph_index(ch).filter(|g| g.0 != 0) {
                    return Some((SelectedFont::Custom(Arc::clone(f)), gid, false, false));
                }
            }
            // 同字族常规体回退 + 合成粗/斜
            let base_key = crate::text_rich::FontSpec {
                family: spec.family,
                weight: crate::text_rich::FontWeight::Normal,
                style: FontStyle::Normal,
            }
            .packed_key();
            if let Some(f) = r.get(&base_key) {
                if let Some(gid) = f.face().glyph_index(ch).filter(|g| g.0 != 0) {
                    return Some((
                        SelectedFont::Custom(Arc::clone(f)),
                        gid,
                        spec.weight.is_bold(),
                        spec.style == FontStyle::Italic,
                    ));
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let (candidate, matched_bold, matched_italic): (Option<&'static LoadedFont>, bool, bool) =
            match spec.family {
                FontFamily::Mono => {
                    if spec.weight.is_bold() {
                        if let Some(f) = MONO_BOLD_FONT
                            .get_or_init(|| load_static_candidate(MONO_BOLD_FONT_CANDIDATES))
                            .as_ref()
                        {
                            (Some(f), true, false)
                        } else {
                            (
                                MONO_FONT
                                    .get_or_init(|| load_static_candidate(MONO_FONT_CANDIDATES))
                                    .as_ref(),
                                false,
                                false,
                            )
                        }
                    } else {
                        (
                            MONO_FONT
                                .get_or_init(|| load_static_candidate(MONO_FONT_CANDIDATES))
                                .as_ref(),
                            false,
                            false,
                        )
                    }
                }
                FontFamily::Serif => (
                    SERIF_FONT
                        .get_or_init(|| load_static_candidate(SERIF_FONT_CANDIDATES))
                        .as_ref(),
                    false,
                    false,
                ),
                FontFamily::Sans | FontFamily::Custom(_) => {
                    if spec.weight.is_bold() {
                        (
                            BOLD_FONT
                                .get_or_init(|| load_static_candidate(BOLD_FONT_CANDIDATES))
                                .as_ref(),
                            true,
                            false,
                        )
                    } else if spec.style == FontStyle::Italic {
                        (
                            ITALIC_FONT
                                .get_or_init(|| load_static_candidate(ITALIC_FONT_CANDIDATES))
                                .as_ref(),
                            false,
                            true,
                        )
                    } else {
                        (None, false, false)
                    }
                }
            };

        if let Some(f) = candidate {
            if let Some(gid) = f.face.glyph_index(ch).filter(|g| g.0 != 0) {
                return Some((
                    SelectedFont::Static(f),
                    gid,
                    spec.weight.is_bold() && !matched_bold,
                    spec.style == FontStyle::Italic && !matched_italic,
                ));
            }
        }
    }

    let (f, gid) = select_font_and_glyph(ch)?;
    Some((
        SelectedFont::Static(f),
        gid,
        spec.weight.is_bold(),
        spec.style == FontStyle::Italic,
    ))
}

fn apply_faux_bold(g: &mut CachedGlyph) {
    if g.width == 0 || g.height == 0 || g.bitmap.is_empty() {
        return;
    }
    let old_w = g.width;
    let new_w = old_w + 1;
    let mut out = vec![0u8; new_w * g.height];
    for y in 0..g.height {
        for x in 0..old_w {
            let v = g.bitmap[y * old_w + x];
            let dst0 = &mut out[y * new_w + x];
            if v > *dst0 {
                *dst0 = v;
            }
            let dst1 = &mut out[y * new_w + x + 1];
            let boost = ((v as u16 * 220) >> 8) as u8;
            if boost > *dst1 {
                *dst1 = boost;
            }
        }
    }
    g.width = new_w;
    g.advance_width = g.advance_width.saturating_add(1);
    g.bitmap = out;
}

fn apply_faux_italic(g: &mut CachedGlyph) {
    if g.width == 0 || g.height == 0 || g.bitmap.is_empty() {
        return;
    }
    let skew_px = ((g.height as f32) * 0.22).ceil() as usize;
    if skew_px == 0 {
        return;
    }
    let old_w = g.width;
    let new_w = old_w + skew_px;
    let mut out = vec![0u8; new_w * g.height];
    for y in 0..g.height {
        let shift = (((g.height - 1 - y) as f32) * 0.22).round() as usize;
        for x in 0..old_w {
            let nx = (x + shift).min(new_w - 1);
            let v = g.bitmap[y * old_w + x];
            let d = &mut out[y * new_w + nx];
            if v > *d {
                *d = v;
            }
        }
    }
    g.width = new_w;
    g.bitmap = out;
}

/// 为彩色 Emoji 码点合成柔和彩色 RGBA8888 徽章位图（当系统未提供内嵌彩色位图时的跨平台一致彩色回退）。
fn synthesize_color_emoji_glyph(ch: char, fs: f32) -> CachedGlyph {
    let dim = (fs.round() as usize).clamp(8, 128);
    let mut rgba = vec![0u8; dim * dim * 4];
    let mut alpha = vec![0u8; dim * dim];
    let cx = (dim as f32) * 0.5;
    let cy = (dim as f32) * 0.5;
    let r = (dim as f32) * 0.44;
    let cp = ch as u32;
    let (base_r, base_g, base_b) = match cp % 6 {
        0 => (255u8, 204u8, 51u8),  // 暖黄（笑脸/星星）
        1 => (239u8, 68u8, 68u8),   // 珊瑚红（心形/火箭）
        2 => (56u8, 189u8, 248u8),  // 天空蓝（海洋/地球）
        3 => (52u8, 211u8, 153u8),  // 翡翠绿（树叶/确认）
        4 => (168u8, 85u8, 247u8),  // 紫罗兰（宝石/魔法）
        _ => (251u8, 146u8, 60u8),  // 活力橙（火焰/太阳）
    };
    for y in 0..dim {
        for x in 0..dim {
            let dx = (x as f32 + 0.5) - cx;
            let dy = (y as f32 + 0.5) - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let cov = (r + 0.5 - dist).clamp(0.0, 1.0);
            if cov > 0.0 {
                // 简单高光渐变让 Emoji 呈现立体彩色质感
                let hi = (1.0 - ((dx + r * 0.35).hypot(dy + r * 0.35) / (r * 1.6))).clamp(0.0, 0.35);
                let rr = ((base_r as f32) + (255.0 - base_r as f32) * hi).min(255.0);
                let gg = ((base_g as f32) + (255.0 - base_g as f32) * hi).min(255.0);
                let bb = ((base_b as f32) + (255.0 - base_b as f32) * hi).min(255.0);
                let a = (cov * 255.0 + 0.5) as u8;
                let idx = (y * dim + x) * 4;
                rgba[idx] = (rr * cov + 0.5) as u8;
                rgba[idx + 1] = (gg * cov + 0.5) as u8;
                rgba[idx + 2] = (bb * cov + 0.5) as u8;
                rgba[idx + 3] = a;
                alpha[y * dim + x] = a;
            }
        }
    }
    CachedGlyph {
        width: dim,
        height: dim,
        xmin: 0,
        ymin: -((dim as i32) / 6),
        advance_width: dim as i16,
        bitmap: alpha,
        color_rgba: Some(rgba),
    }
}

/// 按 [`FontSpec`](crate::text_rich::FontSpec) 精确测量单字符浮点水平步进宽度。
pub fn measure_ttf_char_advance_styled_f32(
    ch: char,
    font_size: u8,
    spec: crate::text_rich::FontSpec,
) -> Option<f32> {
    if spec == crate::text_rich::FontSpec::SANS && !crate::text_rich::is_emoji_char(ch) {
        return measure_ttf_char_advance_f32(ch, font_size);
    }
    if crate::text_rich::is_emoji_char(ch) {
        return Some((font_size.max(8) as f32) * 1.05);
    }
    let (font, gid, faux_bold, _) = select_styled_font_and_glyph(ch, spec)?;
    let face = font.face();
    let upem = face.units_per_em().max(1) as f32;
    let raw_adv = face.glyph_hor_advance(gid).unwrap_or(0) as f32;
    let px = (raw_adv * (font_size.max(8) as f32)) / upem;
    Some((px + if faux_bold { 0.8 } else { 0.0 }).max(1.0))
}

/// 按 [`FontSpec`](crate::text_rich::FontSpec) 与 0.25px 量化字号获取光栅化字形（支持多字重/斜体/等宽/自定义字体与彩色 Emoji）。
pub fn get_cached_glyph_styled_q4(
    ch: char,
    font_size_q4: u16,
    spec: crate::text_rich::FontSpec,
) -> Option<Arc<CachedGlyph>> {
    if spec == crate::text_rich::FontSpec::SANS && !crate::text_rich::is_emoji_char(ch) {
        return get_cached_glyph_q4(ch, font_size_q4);
    }
    let q4 = font_size_q4.clamp(32, 1020);
    let key = (ch, q4, spec.packed_key());
    let cache = STYLED_GLYPH_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(r) = cache.read() {
        if let Some(g) = r.get(&key) {
            return Some(Arc::clone(g));
        }
    }
    let fs = (q4 as f32) * 0.25;
    let glyph = if crate::text_rich::is_emoji_char(ch) {
        synthesize_color_emoji_glyph(ch, fs)
    } else {
        let (font, gid, faux_bold, faux_italic) = select_styled_font_and_glyph(ch, spec)?;
        let mut g = rasterize_glyph_outline(font.face(), gid, fs);
        if faux_bold {
            apply_faux_bold(&mut g);
        }
        if faux_italic {
            apply_faux_italic(&mut g);
        }
        g
    };
    let entry = Arc::new(glyph);
    if let Ok(mut w) = cache.write() {
        if w.len() >= MAX_GLYPH_CACHE_ENTRIES {
            w.clear();
        }
        w.insert(key, Arc::clone(&entry));
    }
    Some(entry)
}

/// 从 `GlyphAtlas` 缓存中获取指定字符与 0.25px 量化字号（`font_size_q4 = round(fs * 4)`）的抗锯齿光栅化字形
/// （线程本地缓存 -> 全局读写锁 -> 按需轮廓光栅化），在 125% / 150% / 175% 等分数 DPI 下达到 0.00px 字体尺寸误差。
pub fn get_cached_glyph_q4(ch: char, font_size_q4: u16) -> Option<Arc<CachedGlyph>> {
    let q4 = font_size_q4.clamp(32, 1020);
    let slot_idx =
        (((ch as usize).wrapping_mul(2654435761)) ^ (q4 as usize)) & (TLS_SLOT_COUNT - 1);
    if let Some(hit) = TLS_GLYPHS.with(|tls| {
        let borrow = tls.borrow();
        match &borrow[slot_idx] {
            Some((c, fs, g)) if *c == ch && *fs == q4 => Some(Arc::clone(g)),
            _ => None,
        }
    }) {
        return Some(hit);
    }

    let (font, gid) = select_font_and_glyph(ch)?;
    let cache = GLYPH_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(read_guard) = cache.read() {
        if let Some(g) = read_guard.get(&(ch, q4)) {
            let cloned = Arc::clone(g);
            TLS_GLYPHS.with(|tls| {
                tls.borrow_mut()[slot_idx] = Some((ch, q4, Arc::clone(&cloned)));
            });
            return Some(cloned);
        }
    }
    if let Ok(mut write_guard) = cache.write() {
        if let Some(g) = write_guard.get(&(ch, q4)) {
            let cloned = Arc::clone(g);
            TLS_GLYPHS.with(|tls| {
                tls.borrow_mut()[slot_idx] = Some((ch, q4, Arc::clone(&cloned)));
            });
            return Some(cloned);
        }
        if write_guard.len() >= MAX_GLYPH_CACHE_ENTRIES {
            write_guard.clear();
        }
        let entry = Arc::new(rasterize_glyph_outline(&font.face, gid, (q4 as f32) * 0.25));
        write_guard.insert((ch, q4), Arc::clone(&entry));
        TLS_GLYPHS.with(|tls| {
            tls.borrow_mut()[slot_idx] = Some((ch, q4, Arc::clone(&entry)));
        });
        return Some(entry);
    }
    None
}

/// 从 `GlyphAtlas` 缓存中获取指定字符与整数字号的抗锯齿光栅化字形（线程本地缓存 -> 全局读写锁 -> 按需轮廓光栅化）。
#[allow(dead_code)]
#[inline]
pub fn get_cached_glyph(ch: char, font_size: u8) -> Option<Arc<CachedGlyph>> {
    get_cached_glyph_q4(ch, (font_size.max(8) as u16) << 2)
}
