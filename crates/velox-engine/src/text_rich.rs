//! 现代多字族/字重富文本（`RichText` / `TextSpan`）、Unicode BiDi 双向视觉重排与 OpenType 简易塑形。
//!
//! 全模块 100% 无堆兼容，可在所有硬件与桌面目标上直接运行：
//! - **多字族 / 字重 / 斜体**：[`FontFamily`]、[`FontWeight`]、[`FontStyle`]、[`FontSpec`]；
//! - **行内富文本**：[`TextSpan`]、[`RichTextSpec`]、[`RichText`]（支持多色、多字号、多字重、斜体、等宽、
//!   下划线、删除线、背景高亮、`line_height`、`letter_spacing` 与连字开关）；
//! - **Unicode BiDi（UAX #9 核心视觉重排）**：支持阿拉伯语 / 希伯来语 RTL 段落、数字与拉丁内嵌运行及对称括号镜像；
//! - **轻量化塑形器（Shaper）**：支持编程连字（`->` / `<-` / `!=` / `<=` / `>=` / `=>`）与组合音标零步进叠加。

use crate::arena::{SmallVec, StrRef};
use crate::element::{Element, IntoElement};
use crate::pod::FrameScope;
use crate::style::{Color, TextAlign};

/// 字体家族枚举（4 字节紧凑表示，支持系统通用族与自定义注册字族）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum FontFamily {
    /// 系统默认无衬线 UI 字体（如 Segoe UI / SF Pro / Roboto / Noto Sans）。
    #[default]
    Sans,
    /// 等宽代码字体（如 Consolas / Cascadia Mono / Menlo / DejaVu Sans Mono）。
    Mono,
    /// 衬线阅读字体（如 Georgia / Times New Roman / Noto Serif）。
    Serif,
    /// 用户通过 `register_font_bytes` 注册的自定义字族 ID。
    Custom(u16),
}

impl FontFamily {
    #[inline(always)]
    pub const fn tag(self) -> u32 {
        match self {
            Self::Sans => 0,
            Self::Mono => 1,
            Self::Serif => 2,
            Self::Custom(id) => 0x1000 | (id as u32),
        }
    }
}

/// 字体字重（对应 CSS `font-weight: 300..=700`）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum FontWeight {
    Light = 0,
    #[default]
    Normal = 1,
    Medium = 2,
    SemiBold = 3,
    Bold = 4,
}

impl FontWeight {
    #[inline(always)]
    pub const fn is_bold(self) -> bool {
        matches!(self, Self::SemiBold | Self::Bold)
    }
}

/// 字体样式（常规 / 斜体）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum FontStyle {
    #[default]
    Normal = 0,
    Italic = 1,
}

/// 文本装饰线位掩码（下划线 / 删除线）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct TextDecoration(pub u8);

impl TextDecoration {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1 << 0);
    pub const STRIKETHROUGH: Self = Self(1 << 1);

    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline(always)]
    pub const fn has_underline(self) -> bool {
        (self.0 & Self::UNDERLINE.0) != 0
    }

    #[inline(always)]
    pub const fn has_strikethrough(self) -> bool {
        (self.0 & Self::STRIKETHROUGH.0) != 0
    }
}

/// 完整字体匹配规格（字族 + 字重 + 斜体）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct FontSpec {
    pub family: FontFamily,
    pub weight: FontWeight,
    pub style: FontStyle,
}

impl FontSpec {
    pub const SANS: Self = Self {
        family: FontFamily::Sans,
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };

    pub const SANS_BOLD: Self = Self {
        family: FontFamily::Sans,
        weight: FontWeight::Bold,
        style: FontStyle::Normal,
    };

    pub const SANS_ITALIC: Self = Self {
        family: FontFamily::Sans,
        weight: FontWeight::Normal,
        style: FontStyle::Italic,
    };

    pub const MONO: Self = Self {
        family: FontFamily::Mono,
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };

    pub const MONO_BOLD: Self = Self {
        family: FontFamily::Mono,
        weight: FontWeight::Bold,
        style: FontStyle::Normal,
    };

    #[inline(always)]
    pub const fn custom(id: u16) -> Self {
        Self {
            family: FontFamily::Custom(id),
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        }
    }

    #[inline(always)]
    pub const fn weight(mut self, w: FontWeight) -> Self {
        self.weight = w;
        self
    }

    #[inline(always)]
    pub const fn bold(mut self) -> Self {
        self.weight = FontWeight::Bold;
        self
    }

    #[inline(always)]
    pub const fn italic(mut self) -> Self {
        self.style = FontStyle::Italic;
        self
    }

    #[inline(always)]
    pub const fn packed_key(self) -> u32 {
        (self.family.tag() << 4) | ((self.weight as u32) << 1) | (self.style as u32)
    }
}

/// 富文本段落内的单个样式化片段（Span）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TextSpan<'a> {
    pub text: StrRef<'a>,
    pub color: Option<Color>,
    pub bg_color: Option<Color>,
    pub font_size: Option<u8>,
    pub font: FontSpec,
    pub decoration: TextDecoration,
    /// 字间距增量（单位 `0.25px`，范围 `-32px..+31.75px`）。
    pub letter_spacing_q4: i8,
    /// 是否启用编程连字塑形（如 `->` / `!=` / `<=` / `>=` / `=>`）。
    pub ligatures: bool,
    /// 可选交互链接 ID（供业务标识可点击超链接片段）。
    pub link_id: Option<u32>,
    /// 可选 OpenType 可变字体字重轴（`wght × 10`，`0` 表示未覆写；启用 `swash` 时可由弹簧驱动连续字重过渡）。
    pub var_weight_x10: u16,
    /// 可选 OpenType 可变字体字宽轴（`wdth × 10`，`0` 表示未覆写）。
    pub var_width_x10: u16,
}

impl<'a> TextSpan<'a> {
    /// 构造默认无覆写样式的文本片段。
    #[inline(always)]
    pub fn new(text: impl Into<StrRef<'a>>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bg_color: None,
            font_size: None,
            font: FontSpec::SANS,
            decoration: TextDecoration::NONE,
            letter_spacing_q4: 0,
            ligatures: false,
            link_id: None,
            var_weight_x10: 0,
            var_width_x10: 0,
        }
    }

    #[inline(always)]
    pub const fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    #[inline(always)]
    pub const fn bg(mut self, c: Color) -> Self {
        self.bg_color = Some(c);
        self
    }

    #[inline(always)]
    pub const fn size(mut self, px: u8) -> Self {
        self.font_size = Some(px);
        self
    }

    #[inline(always)]
    pub const fn font(mut self, font: FontSpec) -> Self {
        self.font = font;
        self
    }

    #[inline(always)]
    pub const fn bold(mut self) -> Self {
        self.font.weight = FontWeight::Bold;
        self
    }

    #[inline(always)]
    pub const fn italic(mut self) -> Self {
        self.font.style = FontStyle::Italic;
        self
    }

    #[inline(always)]
    pub const fn mono(mut self) -> Self {
        self.font.family = FontFamily::Mono;
        self
    }

    #[inline(always)]
    pub const fn underline(mut self) -> Self {
        self.decoration = self.decoration.union(TextDecoration::UNDERLINE);
        self
    }

    #[inline(always)]
    pub const fn strikethrough(mut self) -> Self {
        self.decoration = self.decoration.union(TextDecoration::STRIKETHROUGH);
        self
    }

    #[inline(always)]
    pub const fn letter_spacing_px(mut self, px: f32) -> Self {
        let q = (px * 4.0) as i32;
        self.letter_spacing_q4 = if q < -127 {
            -127
        } else if q > 127 {
            127
        } else {
            q as i8
        };
        self
    }

    #[inline(always)]
    pub const fn ligatures(mut self, enable: bool) -> Self {
        self.ligatures = enable;
        self
    }

    #[inline(always)]
    pub const fn link_id(mut self, id: u32) -> Self {
        self.link_id = Some(id);
        self
    }

    /// 设置 OpenType 可变字体字重轴（`wght`，通常 `100.0 ~ 900.0`，支持弹簧连续过渡）。
    #[inline(always)]
    pub const fn var_weight(mut self, wght: f32) -> Self {
        let clamped = if wght < 1.0 {
            1.0
        } else if wght > 1000.0 {
            1000.0
        } else {
            wght
        };
        self.var_weight_x10 = (clamped * 10.0 + 0.5) as u16;
        self
    }

    /// 设置 OpenType 可变字体字宽轴（`wdth`，通常 `50.0 ~ 200.0`）。
    #[inline(always)]
    pub const fn var_width(mut self, wdth: f32) -> Self {
        let clamped = if wdth < 25.0 {
            25.0
        } else if wdth > 400.0 {
            400.0
        } else {
            wdth
        };
        self.var_width_x10 = (clamped * 10.0 + 0.5) as u16;
        self
    }

    /// 是否携带任意 OpenType 可变字体变化轴覆写。
    #[inline(always)]
    pub const fn has_variable_axes(&self) -> bool {
        self.var_weight_x10 != 0 || self.var_width_x10 != 0
    }
}

/// 富文本段落完整规格（分配于 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::RichText`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RichTextSpec<'a> {
    pub spans: &'a [TextSpan<'a>],
    pub default_color: Color,
    pub default_font_size: u8,
    /// 行高百分比（如 `135` 表示 `1.35×` 基准字号，默认 `130`）。
    pub line_height_pct: u16,
    pub align: TextAlign,
    /// 是否以 RTL（右至左）为段落基准方向（自动启用 Unicode BiDi 视觉重排）。
    pub rtl_paragraph: bool,
    /// 最大显示行数（`0` 表示不限制）。
    pub max_lines: u8,
    /// 是否禁止自动折行。
    pub nowrap: bool,
}

impl<'a> RichTextSpec<'a> {
    #[inline]
    pub fn mix_hash(&self, h: &mut u32) {
        let mut mix = |b: u8| {
            *h ^= b as u32;
            *h = h.wrapping_mul(0x0100_0193);
        };
        for b in self.default_color.0.to_le_bytes() {
            mix(b);
        }
        mix(self.default_font_size);
        for b in self.line_height_pct.to_le_bytes() {
            mix(b);
        }
        mix(self.align as u8);
        mix(if self.rtl_paragraph { 1 } else { 0 });
        mix(self.max_lines);
        mix(if self.nowrap { 1 } else { 0 });
        for sp in self.spans {
            for b in sp.font.packed_key().to_le_bytes() {
                mix(b);
            }
            mix(sp.decoration.0);
            mix(sp.letter_spacing_q4 as u8);
            mix(if sp.ligatures { 1 } else { 0 });
            for b in sp.var_weight_x10.to_le_bytes() {
                mix(b);
            }
            for b in sp.var_width_x10.to_le_bytes() {
                mix(b);
            }
            if let Some(c) = sp.color {
                mix(1);
                for b in c.0.to_le_bytes() {
                    mix(b);
                }
            } else {
                mix(0);
            }
            if let Some(c) = sp.bg_color {
                mix(1);
                for b in c.0.to_le_bytes() {
                    mix(b);
                }
            } else {
                mix(0);
            }
            mix(sp.font_size.unwrap_or(0));
            for &b in sp.text.as_str().as_bytes() {
                mix(b);
            }
        }
    }

    /// 计算基准行高（像素）。
    #[inline(always)]
    pub fn line_height_px(&self, max_span_fs: u8) -> i16 {
        let fs = max_span_fs.max(self.default_font_size).max(8) as i32;
        let pct = self.line_height_pct.max(100) as i32;
        ((fs * pct + 50) / 100).max(fs) as i16
    }
}

/// 声明式富文本构建器（支持 `rich_text(cx, &[...]).line_height_pct(140).rtl(true)`）。
#[derive(Copy, Clone, Debug)]
pub struct RichText<'a> {
    pub spec: RichTextSpec<'a>,
}

/// 构造一个行内富文本元素（片段切片分配于当前帧 `ScratchArena`）。
#[inline]
pub fn rich_text<'a>(cx: &'a FrameScope, spans: &[TextSpan<'a>]) -> RichText<'a> {
    let alloc_spans = cx.arena().alloc_iter(spans.iter().copied());
    RichText {
        spec: RichTextSpec {
            spans: alloc_spans,
            default_color: Color::WHITE,
            default_font_size: 14,
            line_height_pct: 135,
            align: TextAlign::Left,
            rtl_paragraph: cx.is_rtl(),
            max_lines: 0,
            nowrap: false,
        },
    }
}

impl<'a> RichText<'a> {
    #[inline(always)]
    pub const fn color(mut self, c: Color) -> Self {
        self.spec.default_color = c;
        self
    }

    #[inline(always)]
    pub const fn size(mut self, px: u8) -> Self {
        self.spec.default_font_size = px;
        self
    }

    #[inline(always)]
    pub const fn line_height_pct(mut self, pct: u16) -> Self {
        self.spec.line_height_pct = pct;
        self
    }

    #[inline(always)]
    pub const fn align(mut self, align: TextAlign) -> Self {
        self.spec.align = align;
        self
    }

    #[inline(always)]
    pub const fn rtl(mut self, rtl: bool) -> Self {
        self.spec.rtl_paragraph = rtl;
        self
    }

    #[inline(always)]
    pub const fn max_lines(mut self, n: u8) -> Self {
        self.spec.max_lines = n;
        self
    }

    #[inline(always)]
    pub const fn nowrap(mut self) -> Self {
        self.spec.nowrap = true;
        self
    }
}

impl<'a> IntoElement<'a> for RichText<'a> {
    #[inline]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        let spec_ref = cx.arena().alloc_temp(self.spec);
        Element::RichText(spec_ref)
    }
}

// ============================================================================
// Unicode BiDi 双向文本视觉重排器（UAX #9 核心子集，零堆分配）
// ============================================================================

/// 单个 BiDi 字符类别。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BidiClass {
    Ltr,
    Rtl,
    Number,
    Neutral,
}

/// 判定单个 Unicode 字符是否为强 RTL 字符（阿拉伯语 / 希伯来语 / 叙利亚语等）。
#[inline]
pub const fn is_rtl_char(ch: char) -> bool {
    let cp = ch as u32;
    (cp >= 0x0590 && cp <= 0x05FF)
        || (cp >= 0x0600 && cp <= 0x06FF)
        || (cp >= 0x0750 && cp <= 0x077F)
        || (cp >= 0x08A0 && cp <= 0x08FF)
        || (cp >= 0xFB1D && cp <= 0xFDFF)
        || (cp >= 0xFE70 && cp <= 0xFEFF)
}

/// 判定字符是否为 Unicode 组合音标（零水平步进，叠加在前一基字符之上）。
#[inline]
pub const fn is_combining_mark(ch: char) -> bool {
    let cp = ch as u32;
    (cp >= 0x0300 && cp <= 0x036F)
        || (cp >= 0x1AB0 && cp <= 0x1AFF)
        || (cp >= 0x20D0 && cp <= 0x20FF)
        || (cp >= 0xFE20 && cp <= 0xFE2F)
}

/// 判定字符是否为常见 Emoji 码点（用于触发彩色位图或 Emoji 回退字库）。
#[inline]
pub const fn is_emoji_char(ch: char) -> bool {
    let cp = ch as u32;
    (cp >= 0x1F300 && cp <= 0x1FAFF)
        || (cp >= 0x2600 && cp <= 0x27BF)
        || (cp >= 0x2300 && cp <= 0x23FF)
}

/// 对 RTL 视觉运行中的对称括号执行 Unicode Bidi_Mirrored 镜像。
#[inline]
pub const fn mirror_bidi_char(ch: char) -> char {
    match ch {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        _ => ch,
    }
}

#[inline]
pub const fn classify_bidi_char(ch: char) -> BidiClass {
    if is_rtl_char(ch) {
        BidiClass::Rtl
    } else if ch.is_ascii_digit() {
        BidiClass::Number
    } else if ch.is_ascii_alphabetic() || (ch as u32 >= 0x00C0 && !ch.is_whitespace() && !ch.is_ascii_punctuation()) {
        // CJK / Latin / Cyrillic / Greek 均归为强 LTR
        if is_rtl_char(ch) {
            BidiClass::Rtl
        } else {
            BidiClass::Ltr
        }
    } else {
        BidiClass::Neutral
    }
}

/// 判断字符串是否包含任意强 RTL 字符（纯 LTR 字符串直接走零开销快路径）。
#[inline]
pub fn contains_rtl(s: &str) -> bool {
    s.chars().any(is_rtl_char)
}

/// 视觉顺序下的单个定向运行段（指向原字符串字节区间 `[start..end]`，`is_rtl` 表示需逐字符逆序绘制并镜像括号）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct BidiVisualRun {
    pub start: usize,
    pub end: usize,
    pub is_rtl: bool,
}

/// 对单行文本执行轻量化 UAX #9 双向视觉重排，返回从左到右绘制顺序的 [`BidiVisualRun`] 列表（无堆）。
pub fn reorder_bidi_visual(line: &str, base_rtl: bool) -> SmallVec<BidiVisualRun, 32> {
    let mut runs: SmallVec<BidiVisualRun, 32> = SmallVec::new();
    if line.is_empty() {
        return runs;
    }
    if !base_rtl && !contains_rtl(line) {
        let _ = runs.push(BidiVisualRun {
            start: 0,
            end: line.len(),
            is_rtl: false,
        });
        return runs;
    }

    // 1. 按逻辑顺序切分为同向运行（中性字符跟随当前有效方向，数字始终保持 LTR 内部顺序）
    let mut cur_start = 0usize;
    let mut cur_rtl = base_rtl;
    let mut cur_is_num = false;
    let mut initialized = false;

    for (idx, ch) in line.char_indices() {
        let cls = classify_bidi_char(ch);
        let (want_rtl, want_num) = match cls {
            BidiClass::Rtl => (true, false),
            BidiClass::Ltr => (false, false),
            BidiClass::Number => (false, true),
            BidiClass::Neutral => {
                if initialized {
                    if cur_is_num {
                        (base_rtl, false)
                    } else {
                        (cur_rtl, false)
                    }
                } else {
                    (base_rtl, false)
                }
            }
        };
        if !initialized {
            cur_start = idx;
            cur_rtl = want_rtl;
            cur_is_num = want_num;
            initialized = true;
        } else if want_rtl != cur_rtl || want_num != cur_is_num {
            let _ = runs.push(BidiVisualRun {
                start: cur_start,
                end: idx,
                is_rtl: cur_rtl,
            });
            cur_start = idx;
            cur_rtl = want_rtl;
            cur_is_num = want_num;
        }
    }
    if initialized && cur_start < line.len() {
        let _ = runs.push(BidiVisualRun {
            start: cur_start,
            end: line.len(),
            is_rtl: cur_rtl,
        });
    }

    // 2. 当基准方向为 RTL 时，整行运行按视觉从左到右逆排（其中 LTR 与 Number 运行内部保持正序）
    if base_rtl && runs.len() > 1 {
        let slice = runs.as_mut_slice();
        slice.reverse();
    }
    runs
}

// ============================================================================
// 轻量化 OpenType 塑形与编程连字（无堆）
// ============================================================================

/// 塑形后的单个字形槽位（含原始字符/连字替代字符、是否组合音标零步进）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ShapedGlyph {
    pub ch: char,
    pub is_combining: bool,
    pub byte_offset: usize,
}

/// 对一段文本执行轻量化塑形（支持可选编程连字映射与组合音标标记）。
pub fn shape_text_run(text: &str, ligatures: bool) -> SmallVec<ShapedGlyph, 128> {
    let mut out: SmallVec<ShapedGlyph, 128> = SmallVec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if ligatures && i + 1 < bytes.len() {
            let lig = match (bytes[i], bytes[i + 1]) {
                (b'-', b'>') => Some('→'),
                (b'<', b'-') => Some('←'),
                (b'!', b'=') => Some('≠'),
                (b'<', b'=') => Some('≤'),
                (b'>', b'=') => Some('≥'),
                (b'=', b'>') => Some('⇒'),
                _ => None,
            };
            if let Some(ch) = lig {
                if !out.push(ShapedGlyph {
                    ch,
                    is_combining: false,
                    byte_offset: i,
                }) {
                    break;
                }
                i += 2;
                continue;
            }
        }
        let Some(ch) = text[i..].chars().next() else {
            break;
        };
        if !out.push(ShapedGlyph {
            ch,
            is_combining: is_combining_mark(ch),
            byte_offset: i,
        }) {
            break;
        }
        i += ch.len_utf8();
    }
    out
}

/// 测量单段富文本单行逻辑像素宽度（支持自定义字族/字重、字距微调与 `letter_spacing`）。
pub fn measure_rich_text_span_width_f32(span: &TextSpan<'_>, default_fs: u8) -> f32 {
    let fs = span.font_size.unwrap_or(default_fs).max(8);
    let ls = (span.letter_spacing_q4 as f32) * 0.25;
    let shaped = shape_text_run(span.text.as_str(), span.ligatures);
    let mut total = 0.0f32;
    let mut prev: Option<char> = None;
    for g in shaped.as_slice() {
        if g.is_combining {
            continue;
        }
        if let Some(p) = prev {
            total += crate::display::measure_char_kerning_f32(p, g.ch, fs);
            total += ls;
        }
        total += crate::display::measure_char_advance_styled_f32(g.ch, fs, span.font);
        prev = Some(g.ch);
    }
    total.max(0.0)
}

#[inline(always)]
fn ceil_i16(v: f32) -> i16 {
    let i = v as i32;
    let c = if (i as f32) < v { i + 1 } else { i };
    c.clamp(0, i16::MAX as i32) as i16
}

/// 测量完整 [`RichTextSpec`] 在给定最大可用宽度下的尺寸 `(width_px, height_px)`。
pub fn measure_rich_text_box(spec: &RichTextSpec<'_>, max_width: i16) -> (i16, i16) {
    if spec.spans.is_empty() {
        return (0, 0);
    }
    let avail_w = if spec.nowrap || max_width <= 0 {
        f32::INFINITY
    } else {
        max_width as f32
    };
    let mut cur_line_w = 0.0f32;
    let mut cur_line_max_fs = spec.default_font_size.max(8);
    let mut max_line_w = 0.0f32;
    let mut total_h = 0i32;
    let mut line_count = 1u8;

    for sp in spec.spans {
        let fs = sp.font_size.unwrap_or(spec.default_font_size).max(8);
        let ls = (sp.letter_spacing_q4 as f32) * 0.25;
        let shaped = shape_text_run(sp.text.as_str(), sp.ligatures);
        let mut prev: Option<char> = None;
        for g in shaped.as_slice() {
            if g.ch == '\n' {
                max_line_w = max_line_w.max(cur_line_w);
                total_h += spec.line_height_px(cur_line_max_fs) as i32;
                cur_line_w = 0.0;
                cur_line_max_fs = spec.default_font_size.max(8);
                prev = None;
                line_count = line_count.saturating_add(1);
                if spec.max_lines > 0 && line_count > spec.max_lines {
                    return (ceil_i16(max_line_w), total_h.min(i16::MAX as i32) as i16);
                }
                continue;
            }
            if g.is_combining {
                continue;
            }
            let adv = crate::display::measure_char_advance_styled_f32(g.ch, fs, sp.font)
                + if prev.is_some() { ls } else { 0.0 };
            if cur_line_w > 0.0 && cur_line_w + adv > avail_w {
                max_line_w = max_line_w.max(cur_line_w);
                total_h += spec.line_height_px(cur_line_max_fs) as i32;
                cur_line_w = adv;
                cur_line_max_fs = fs;
                prev = Some(g.ch);
                line_count = line_count.saturating_add(1);
                if spec.max_lines > 0 && line_count > spec.max_lines {
                    return (ceil_i16(max_line_w), total_h.min(i16::MAX as i32) as i16);
                }
            } else {
                cur_line_w += adv;
                cur_line_max_fs = cur_line_max_fs.max(fs);
                prev = Some(g.ch);
            }
        }
    }
    max_line_w = max_line_w.max(cur_line_w);
    total_h += spec.line_height_px(cur_line_max_fs) as i32;
    (
        ceil_i16(max_line_w),
        total_h.min(i16::MAX as i32) as i16,
    )
}

// ============================================================================
// 国际多脚本分类、Emoji 序列聚类、多字体回退链与富文本内联真实组件流（InlineFlow）
// ============================================================================

/// Unicode 文字脚本分类（供多字体回退链与复杂混排脚本分段使用）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum ScriptKind {
    #[default]
    Latin = 0,
    Cjk = 1,
    Arabic = 2,
    Hebrew = 3,
    Devanagari = 4,
    Cyrillic = 5,
    Emoji = 6,
    Symbol = 7,
}

/// 分类单个 Unicode 标量值所属文字脚本（100% `#![no_std]` 零查表分支）。
#[inline]
pub const fn classify_script(ch: char) -> ScriptKind {
    let cp = ch as u32;
    if is_emoji_char(ch)
        || cp == 0x200D
        || cp == 0xFE0F
        || (cp >= 0x1F3FB && cp <= 0x1F3FF)
        || (cp >= 0x1F1E6 && cp <= 0x1F1FF)
    {
        return ScriptKind::Emoji;
    }
    if (cp >= 0x4E00 && cp <= 0x9FFF)
        || (cp >= 0x3400 && cp <= 0x4DBF)
        || (cp >= 0x3000 && cp <= 0x30FF)
        || (cp >= 0xAC00 && cp <= 0xD7AF)
        || (cp >= 0xFF00 && cp <= 0xFFEF)
    {
        return ScriptKind::Cjk;
    }
    if (cp >= 0x0600 && cp <= 0x06FF)
        || (cp >= 0x0750 && cp <= 0x077F)
        || (cp >= 0x08A0 && cp <= 0x08FF)
    {
        return ScriptKind::Arabic;
    }
    if cp >= 0x0590 && cp <= 0x05FF {
        return ScriptKind::Hebrew;
    }
    if cp >= 0x0900 && cp <= 0x097F {
        return ScriptKind::Devanagari;
    }
    if cp >= 0x0400 && cp <= 0x052F {
        return ScriptKind::Cyrillic;
    }
    if cp < 0x0250 || (cp >= 0x1E00 && cp <= 0x1EFF) {
        return ScriptKind::Latin;
    }
    ScriptKind::Symbol
}

/// 按脚本与 Emoji ZWJ 聚类后的文本分段。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ScriptRun<'a> {
    pub script: ScriptKind,
    pub text: &'a str,
}

/// 将任意 UTF-8 文本零堆切分为同脚本运行段（自动合并 Emoji ZWJ / 肤色修饰符 / VS-16 簇）。
pub fn split_script_runs<'a>(text: &'a str) -> SmallVec<ScriptRun<'a>, 32> {
    let mut runs: SmallVec<ScriptRun<'a>, 32> = SmallVec::new();
    let mut iter = text.char_indices().peekable();
    let mut cur_start: Option<usize> = None;
    let mut cur_script = ScriptKind::Latin;

    while let Some((idx, ch)) = iter.next() {
        let sc = if ch.is_whitespace() && cur_start.is_some() {
            cur_script
        } else {
            classify_script(ch)
        };
        match cur_start {
            None => {
                cur_start = Some(idx);
                cur_script = sc;
            }
            Some(start) if sc != cur_script => {
                let _ = runs.push(ScriptRun {
                    script: cur_script,
                    text: &text[start..idx],
                });
                cur_start = Some(idx);
                cur_script = sc;
            }
            Some(_) => {}
        }
    }
    if let Some(start) = cur_start {
        let _ = runs.push(ScriptRun {
            script: cur_script,
            text: &text[start..],
        });
    }
    runs
}

/// 8 槽位零堆多脚本字体回退链（支持为 CJK / Arabic / Emoji / Devanagari 等独立绑定自定义或系统字族）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FontFallbackChain {
    slots: [Option<FontFamily>; 8],
}

impl Default for FontFallbackChain {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl FontFallbackChain {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { slots: [None; 8] }
    }

    /// 为指定文字脚本绑定回退字族。
    #[inline(always)]
    pub const fn with_script(mut self, script: ScriptKind, family: FontFamily) -> Self {
        self.slots[script as usize] = Some(family);
        self
    }

    /// 查询指定字符最终应使用的 [`FontSpec`]（保持原始字重与斜体）。
    #[inline]
    pub const fn resolve_for_char(&self, ch: char, base: FontSpec) -> FontSpec {
        let script = classify_script(ch);
        if let Some(fam) = self.slots[script as usize] {
            FontSpec {
                family: fam,
                weight: base.weight,
                style: base.style,
            }
        } else {
            base
        }
    }
}

/// 富文本内联真实组件流（`InlineFlow`）：
/// 允许在自动换行的段落流中无缝混排多色文字、彩色 Emoji 与任意真实交互组件（`Button` / `Badge` / `Tag` / `Kbd` 等）。
pub struct InlineFlow<'a> {
    root: crate::element::Div<'a>,
    default_size: u8,
    default_color: Color,
}

/// 创建一个富文本内联组件混排流容器（支持自动换行、基线居中对齐与任意内联交互组件）。
#[inline]
pub fn inline_flow<'a>() -> InlineFlow<'a> {
    use crate::style::Styled;
    InlineFlow {
        root: crate::element::div()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(4),
        default_size: 13,
        default_color: Color::WHITE,
    }
}

impl<'a> InlineFlow<'a> {
    #[inline]
    pub fn w(mut self, w: i16) -> Self {
        use crate::style::Styled;
        self.root = self.root.w(w);
        self
    }

    #[inline]
    pub fn w_full(mut self) -> Self {
        use crate::style::Styled;
        self.root = self.root.w_full();
        self
    }

    #[inline]
    pub fn gap(mut self, px: i8) -> Self {
        use crate::style::Styled;
        self.root = self.root.gap(px);
        self
    }

    #[inline]
    pub const fn default_size(mut self, px: u8) -> Self {
        self.default_size = px;
        self
    }

    #[inline]
    pub const fn default_color(mut self, color: Color) -> Self {
        self.default_color = color;
        self
    }

    /// 追加一段可自动按词/CJK 字符边界拆行并与内联组件混排的文本。
    pub fn text(self, cx: &'a FrameScope, content: &'a str) -> Self {
        let sz = self.default_size;
        let col = self.default_color;
        self.styled_text(cx, content, col, sz, false)
    }

    /// 追加一段带自定义颜色/字号/加粗的混排文本（自动按空格与 CJK 边界切分，确保丝滑换行）。
    pub fn styled_text(
        mut self,
        cx: &'a FrameScope,
        content: &'a str,
        color: Color,
        font_size: u8,
        bold: bool,
    ) -> Self {
        let mut start = 0usize;
        let mut iter = content.char_indices().peekable();
        while let Some((idx, ch)) = iter.next() {
            let next_idx = iter.peek().map(|&(ni, _)| ni).unwrap_or(content.len());
            let is_cjk = matches!(
                classify_script(ch),
                ScriptKind::Cjk | ScriptKind::Emoji
            );
            if ch.is_whitespace() {
                if idx > start {
                    let word = &content[start..idx];
                    self = self.push_atom_text(cx, word, color, font_size, bold);
                }
                start = next_idx;
            } else if is_cjk {
                if idx > start {
                    self = self.push_atom_text(cx, &content[start..idx], color, font_size, bold);
                }
                self = self.push_atom_text(cx, &content[idx..next_idx], color, font_size, bold);
                start = next_idx;
            }
        }
        if start < content.len() {
            self = self.push_atom_text(cx, &content[start..], color, font_size, bold);
        }
        self
    }

    fn push_atom_text(
        mut self,
        cx: &'a FrameScope,
        word: &'a str,
        color: Color,
        font_size: u8,
        bold: bool,
    ) -> Self {
        if word.is_empty() {
            return self;
        }
        let sz = if bold { font_size.saturating_add(1) } else { font_size };
        let t = crate::element::text(word).size(sz).color(color).nowrap();
        self.root = self.root.child(cx, t);
        self
    }

    /// 追加任意真实可交互组件（如 `Button`、`Badge`、`Tag`、`Kbd`、图标、迷你进度条）作为行内原子元素。
    #[inline]
    pub fn inline_widget(mut self, cx: &'a FrameScope, widget: impl IntoElement<'a>) -> Self {
        self.root = self.root.child(cx, widget);
        self
    }

    /// 完成构建并输出底层 [`crate::element::Div`]。
    #[inline]
    pub fn build(self) -> crate::element::Div<'a> {
        self.root
    }
}

impl<'a> IntoElement<'a> for InlineFlow<'a> {
    #[inline]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        self.root.into_element(cx)
    }
}
