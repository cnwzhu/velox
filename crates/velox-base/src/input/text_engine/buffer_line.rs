//! 单逻辑行文本缓冲与惰性单行排版缓存（[`BufferLine`]）。
//!
//! 参照 `pop-os/cosmic-text/src/buffer_line.rs` 设计：
//! - 每行独立持有文本（`SmallString`，`std` 下无限扩容，`#![no_std]` 下零堆分配）、行尾结束符 [`LineEnding`]、
//!   样式属性表 [`AttrsList`] 与惰性视觉折行缓存 `layout_opt`；
//! - 单行修改只失效当前行缓存，其余行 100% 零重排复用；
//! - 使用与 `velox-core` 渲染器完全一致的高精度浮点字形度量（`measure_char_advance_f32` / `measure_char_kerning_f32`）
//!   计算每个 [`LayoutGlyph`] 的精确 `(x, w)` 与制表符对齐列宽。

use velox_core::arena::{SmallString, SmallVec};
use velox_core::display::{measure_char_advance_f32, measure_char_kerning_f32};

use super::attrs::AttrsList;
use super::line_ending::LineEnding;
use super::shape::{Align, LayoutGlyph, LayoutLine, Wrap};
use crate::input::text_boundary::{clip_offset_left, CharacterKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct LayoutCacheKey {
    font_size: u8,
    width_tenths: i32,
    wrap: Wrap,
    tab_width: u16,
    align: Align,
}

/// 文本缓冲区中的单个逻辑行（对应 `cosmic-text` `BufferLine`）。
#[derive(Clone, Debug)]
pub struct BufferLine {
    text: SmallString<128>,
    ending: LineEnding,
    attrs_list: AttrsList,
    align: Option<Align>,
    layout_opt: Option<SmallVec<LayoutLine, 4>>,
    layout_key: Option<LayoutCacheKey>,
}

impl BufferLine {
    /// 创建一个逻辑行。
    pub fn new(text: &str, ending: LineEnding, attrs_list: AttrsList) -> Self {
        Self {
            text: SmallString::from_str_trunc(text),
            ending,
            attrs_list,
            align: None,
            layout_opt: None,
            layout_key: None,
        }
    }

    /// 返回当前逻辑行不含行尾换行符的 UTF-8 切片。
    #[inline]
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// 返回行尾结束符。
    #[inline(always)]
    pub const fn ending(&self) -> LineEnding {
        self.ending
    }

    /// 设置行尾结束符。
    #[inline]
    pub fn set_ending(&mut self, ending: LineEnding) {
        self.ending = ending;
    }

    /// 返回当前行样式属性列表的只读引用。
    #[inline(always)]
    pub fn attrs_list(&self) -> &AttrsList {
        &self.attrs_list
    }

    /// 替换当前行样式属性列表（仅在属性变更时失效视觉排版缓存）。
    pub fn set_attrs_list(&mut self, attrs_list: AttrsList) -> bool {
        if self.attrs_list != attrs_list {
            self.attrs_list = attrs_list;
            self.reset_layout();
            true
        } else {
            false
        }
    }

    /// 返回可选的段落水平对齐覆盖。
    #[inline(always)]
    pub const fn align(&self) -> Option<Align> {
        self.align
    }

    /// 设置段落水平对齐覆盖（仅在变化时失效缓存）。
    pub fn set_align(&mut self, align: Option<Align>) -> bool {
        if self.align != align {
            self.align = align;
            self.reset_layout();
            true
        } else {
            false
        }
    }

    /// 更新当前行文本、行尾符与样式属性，若内容发生变化则失效缓存并返回 `true`。
    pub fn set_text(&mut self, text: &str, ending: LineEnding, attrs_list: AttrsList) -> bool {
        let changed = self.text.as_str() != text
            || self.ending != ending
            || self.attrs_list != attrs_list;
        if changed {
            self.text.clear();
            self.text.push_str_trunc(text);
            self.ending = ending;
            self.attrs_list = attrs_list;
            self.reset_layout();
        }
        changed
    }

    /// 在行内字节偏移 `index` 处拆分本行（回车换行时调用）：
    /// 本行保留 `[..index]`，返回包含 `[index..]`、对应属性切片与原 `ending` 的新逻辑行。
    pub fn split_off(&mut self, index: usize) -> Self {
        let split_at = clip_offset_left(self.text.as_str(), index);
        let tail_text = self.text.split_off(split_at);
        let tail_attrs = self.attrs_list.split_off(split_at);
        let tail_ending = self.ending;
        self.ending = LineEnding::Lf;
        self.reset_layout();
        Self {
            text: tail_text,
            ending: tail_ending,
            attrs_list: tail_attrs,
            align: self.align,
            layout_opt: None,
            layout_key: None,
        }
    }

    /// 将另一行 `other` 拼接到当前行末尾（退格合并两行时调用），并继承 `other` 的行尾符。
    pub fn append(&mut self, other: &Self) {
        let base_len = self.text.len();
        self.text.push_str_trunc(other.text.as_str());
        self.attrs_list.append(&other.attrs_list, base_len);
        self.ending = other.ending;
        self.reset_layout();
    }

    /// 在行内字节区间 `start..end` 原地替换为 `replacement`（不含换行符），并自动维护属性偏移。
    pub fn replace_range(&mut self, start: usize, end: usize, replacement: &str) -> usize {
        let s = clip_offset_left(self.text.as_str(), start);
        let e = clip_offset_left(self.text.as_str(), end).max(s);
        let inserted = self.text.replace_range(s, e, replacement);
        // 拆分并平移覆盖区间，保持编辑后的相邻高亮偏移无撕裂
        let right_attrs = self.attrs_list.split_off(e);
        let _ = self.attrs_list.split_off(s);
        self.attrs_list.append(&right_attrs, s.saturating_add(inserted));
        self.reset_layout();
        inserted
    }

    /// 清除当前行的惰性视觉排版缓存。
    #[inline]
    pub fn reset_layout(&mut self) {
        self.layout_opt = None;
        self.layout_key = None;
    }

    /// 获取已缓存的视觉排版行切片（若尚未排版则返回 `None`）。
    #[inline]
    pub fn layout_opt(&self) -> Option<&[LayoutLine]> {
        self.layout_opt.as_ref().map(|v| v.as_slice())
    }

    /// 按需惰性执行单行字形定位与软折行排版（命中缓存时 $O(1)$ 零计算直接返回）。
    pub fn layout(
        &mut self,
        font_size: u8,
        width_opt: Option<f32>,
        wrap: Wrap,
        tab_width: u16,
    ) -> &[LayoutLine] {
        let eff_align = self.align.unwrap_or(Align::Left);
        let eff_wrap = if width_opt.is_some() { wrap } else { Wrap::None };
        let width_tenths = match width_opt {
            Some(w) if w > 0.0 => (w * 10.0) as i32,
            _ => -1,
        };
        let key = LayoutCacheKey {
            font_size,
            width_tenths,
            wrap: eff_wrap,
            tab_width: tab_width.max(1),
            align: eff_align,
        };
        if self.layout_key == Some(key) && self.layout_opt.is_some() {
            return self.layout_opt.as_ref().unwrap().as_slice();
        }

        let lines = build_layout_lines(
            self.text.as_str(),
            &self.attrs_list,
            font_size,
            width_opt,
            eff_wrap,
            tab_width.max(1),
            eff_align,
        );
        self.layout_key = Some(key);
        self.layout_opt = Some(lines);
        self.layout_opt.as_ref().unwrap().as_slice()
    }
}

/// 将单逻辑行排版为 1 条或多条软折行 [`LayoutLine`]（支持 Tab 制表位、字偶距与 4 种 [`Wrap`] 策略）。
fn build_layout_lines(
    text: &str,
    attrs_list: &AttrsList,
    font_size: u8,
    width_opt: Option<f32>,
    wrap: Wrap,
    tab_width: u16,
    align: Align,
) -> SmallVec<LayoutLine, 4> {
    let mut out: SmallVec<LayoutLine, 4> = SmallVec::new();
    if text.is_empty() {
        let _ = out.push(LayoutLine::default());
        return out;
    }

    let max_w = width_opt.unwrap_or(f32::MAX);
    let space_w = measure_char_advance_f32(' ', font_size).max(1.0);
    let tab_stop_w = space_w * (tab_width.max(1) as f32);

    // 1. 先按单行度量所有字符，生成原始字形流
    let mut raw: SmallVec<(usize, usize, char, f32, f32), 128> = SmallVec::new();
    let mut prev_ch: Option<char> = None;
    let mut pen_x = 0.0f32;
    for (byte_idx, ch) in text.char_indices() {
        let end_idx = byte_idx + ch.len_utf8();
        let (kern, adv) = if ch == '\t' {
            let next_stop = (((pen_x / tab_stop_w) as i32) + 1) as f32 * tab_stop_w;
            let step = (next_stop - pen_x).max(space_w);
            (0.0, step)
        } else {
            let k = prev_ch
                .map(|p| measure_char_kerning_f32(p, ch, font_size))
                .unwrap_or(0.0);
            let a = measure_char_advance_f32(ch, font_size);
            (k, a)
        };
        let _ = raw.push((byte_idx, end_idx, ch, kern, adv));
        pen_x = (pen_x + kern + adv).max(0.0);
        prev_ch = if ch == '\t' { None } else { Some(ch) };
    }

    // 2. 依据 Wrap 模式切分为 1 条或多条视觉行
    let slice = raw.as_slice();
    let mut line_start_g = 0usize;

    while line_start_g < slice.len() {
        let mut cur_x = 0.0f32;
        let mut last_break_g: Option<usize> = None;
        let mut split_g = slice.len();

        for idx in line_start_g..slice.len() {
            let (_s, _e, ch, kern, adv) = slice[idx];
            // 视觉行首字符不叠加来自上一行尾字符的字偶距；制表符按行内局部坐标重新计算制表位
            let eff_kern = if idx == line_start_g { 0.0 } else { kern };
            let eff_adv = if ch == '\t' {
                let next_stop = (((cur_x / tab_stop_w) as i32) + 1) as f32 * tab_stop_w;
                (next_stop - cur_x).max(space_w)
            } else {
                adv
            };
            let next_x = (cur_x + eff_kern + eff_adv).max(0.0);

            if matches!(wrap, Wrap::Word | Wrap::WordOrGlyph) {
                if idx > line_start_g && is_word_break_after(slice[idx - 1].2, ch) {
                    last_break_g = Some(idx);
                }
            }

            if !matches!(wrap, Wrap::None) && idx > line_start_g && next_x > max_w + 0.01 {
                split_g = match wrap {
                    Wrap::None => slice.len(),
                    Wrap::Glyph => idx,
                    Wrap::Word => {
                        if let Some(brk) = last_break_g {
                            brk
                        } else {
                            // Word 模式下超长词延续至下一个可断点（不强拆词中间）
                            cur_x = next_x;
                            continue;
                        }
                    }
                    Wrap::WordOrGlyph => last_break_g.unwrap_or(idx),
                };
                break;
            }
            cur_x = next_x;
        }

        // 构造该视觉行的 LayoutLine（精确局部 x 与样式覆盖）
        let mut glyphs: SmallVec<LayoutGlyph, 64> = SmallVec::new();
        let mut gx = 0.0f32;
        for idx in line_start_g..split_g {
            let (start, end, ch, kern, adv) = slice[idx];
            let eff_kern = if idx == line_start_g { 0.0 } else { kern };
            let eff_adv = if ch == '\t' {
                let next_stop = (((gx / tab_stop_w) as i32) + 1) as f32 * tab_stop_w;
                (next_stop - gx).max(space_w)
            } else {
                adv
            };
            gx = (gx + eff_kern).max(0.0);
            let a = attrs_list.get_span(start);
            let _ = glyphs.push(LayoutGlyph {
                start,
                end,
                x: gx,
                w: eff_adv,
                color_opt: a.color_opt,
                metadata: a.metadata,
            });
            gx += eff_adv;
        }

        // 水平对齐偏移（仅在指定有效宽度时应用 Center / Right / End）
        if let Some(avail_w) = width_opt {
            if avail_w > gx {
                let shift = match align {
                    Align::Left => 0.0,
                    Align::Center => (avail_w - gx) * 0.5,
                    Align::Right | Align::End => avail_w - gx,
                };
                if shift > 0.0 {
                    for g in glyphs.as_mut_slice() {
                        g.x += shift;
                    }
                }
            }
        }

        let _ = out.push(LayoutLine { w: gx, glyphs });
        if split_g <= line_start_g {
            break;
        }
        line_start_g = split_g;
    }

    if out.is_empty() {
        let _ = out.push(LayoutLine::default());
    }
    out
}

#[inline]
fn is_word_break_after(prev: char, curr: char) -> bool {
    // 空白之后可折行；非 ASCII CJK 汉字之间可自然折行
    if prev == ' ' || prev == '\t' {
        return true;
    }
    if !prev.is_ascii() && CharacterKind::from(prev) == CharacterKind::Word {
        return true;
    }
    if !curr.is_ascii() && CharacterKind::from(curr) == CharacterKind::Word {
        return true;
    }
    false
}
