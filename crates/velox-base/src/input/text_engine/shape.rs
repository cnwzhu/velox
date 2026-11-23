//! 字体度量（[`Metrics`]）、折行模式（[`Wrap`]）、对齐（[`Align`]）与行级排版产出结构
//! （[`LayoutGlyph`]、[`LayoutLine`]、[`LayoutRun`]）。
//!
//! 参照 `pop-os/cosmic-text/src/shape.rs` 与 `buffer.rs` 设计，采用高精度浮点水平推进
//! （`velox_core::display::measure_char_advance_f32` / `measure_char_kerning_f32`）构建字形几何，
//! 使鼠标命中测试、字符级选区高亮与 0 抖动光标定位与渲染器逐像素严格一致。

use velox_core::arena::SmallVec;
use velox_core::style::Color;

use super::cursor::Affinity;

/// 字体字号与行高排版度量（对应 `cosmic-text` `Metrics`）。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Metrics {
    /// 逻辑字号（像素）。
    pub font_size: f32,
    /// 逻辑行高（像素）。
    pub line_height: f32,
}

impl Metrics {
    #[inline(always)]
    pub const fn new(font_size: f32, line_height: f32) -> Self {
        Self {
            font_size,
            line_height,
        }
    }

    #[inline(always)]
    pub const fn relative(font_size: f32, line_height_scale: f32) -> Self {
        Self {
            font_size,
            line_height: font_size * line_height_scale,
        }
    }

    #[inline(always)]
    pub const fn scale(self, scale: f32) -> Self {
        Self {
            font_size: self.font_size * scale,
            line_height: self.line_height * scale,
        }
    }

    #[inline(always)]
    pub fn font_size_u8(self) -> u8 {
        let fs = (self.font_size + 0.5) as i32;
        fs.clamp(8, 255) as u8
    }

    #[inline(always)]
    pub fn line_height_i16(self) -> i16 {
        let lh = (self.line_height + 0.5) as i32;
        lh.clamp(10, i16::MAX as i32) as i16
    }
}

impl Default for Metrics {
    #[inline(always)]
    fn default() -> Self {
        Self::new(13.0, 20.0)
    }
}

/// 文本软折行策略（对应 `cosmic-text` `Wrap`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Wrap {
    /// 不自动折行（支持横向滚动）。
    None,
    /// 在任意字符边界折行。
    Glyph,
    /// 优先在词/空白边界折行（超长单词保持完整不拆断）。
    Word,
    /// 优先在词边界折行，若单个超长词超出整行宽度则自动回退到字符边界折行（默认）。
    #[default]
    WordOrGlyph,
}

/// 段落水平对齐方式（对应 `cosmic-text` `Align`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
    Center,
    End,
}

/// 单视觉行内单个已定位字形的几何与样式描述符（对应 `cosmic-text` `LayoutGlyph`）。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LayoutGlyph {
    /// 该字形在所属 `BufferLine` 文本中的起始 UTF-8 字节索引。
    pub start: usize,
    /// 该字形在所属 `BufferLine` 文本中的结束 UTF-8 字节索引。
    pub end: usize,
    /// 相对于所属视觉行左起点的精确水平像素坐标。
    pub x: f32,
    /// 该字形的精确水平步进宽度（含字偶距微调或制表符对齐宽度）。
    pub w: f32,
    /// 可选颜色覆盖（来自 `BufferLine::attrs_list`）。
    pub color_opt: Option<Color>,
    /// 自定义元数据（来自 `Attrs::metadata`）。
    pub metadata: usize,
}

/// 一条经过软折行与字形定位后的视觉行（对应 `cosmic-text` `LayoutLine`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutLine {
    /// 本视觉行的精确内容总宽度（像素）。
    pub w: f32,
    /// 本视觉行内所有已排版字形列表（按视觉从左到右升序排列；`std` 下无限扩容，`#![no_std]` 下零堆上限 64）。
    pub glyphs: SmallVec<LayoutGlyph, 64>,
}

impl LayoutLine {
    /// 返回本视觉行覆盖的最小起始字节索引（空行返回 `0`）。
    #[inline]
    pub fn start_index(&self) -> usize {
        self.glyphs.as_slice().first().map_or(0, |g| g.start)
    }

    /// 返回本视觉行覆盖的最大结束字节索引（空行返回 `0`）。
    #[inline]
    pub fn end_index(&self) -> usize {
        self.glyphs.as_slice().last().map_or(0, |g| g.end)
    }

    /// 查询行内字节偏移 `byte_index` 在本视觉行内的精确水平像素 X 坐标。
    pub fn x_for_index(&self, byte_index: usize) -> f32 {
        let glyphs = self.glyphs.as_slice();
        if glyphs.is_empty() {
            return 0.0;
        }
        for g in glyphs {
            if byte_index <= g.start {
                return g.x;
            }
            if byte_index < g.end {
                return g.x;
            }
        }
        self.w
    }

    /// 根据本视觉行内局部水平坐标 `x` 执行亚像素级精确命中测试，返回 `(byte_index, Affinity)`。
    pub fn hit_x(&self, x: f32) -> (usize, Affinity) {
        let glyphs = self.glyphs.as_slice();
        if glyphs.is_empty() {
            return (0, Affinity::After);
        }
        if x <= glyphs[0].x {
            return (glyphs[0].start, Affinity::After);
        }
        for g in glyphs {
            let right = g.x + g.w;
            if x < right {
                let mid = g.x + g.w * 0.5;
                if x < mid {
                    return (g.start, Affinity::After);
                } else {
                    return (g.end, Affinity::Before);
                }
            }
        }
        let last = glyphs[glyphs.len() - 1];
        (last.end, Affinity::Before)
    }
}

/// 面向渲染器消费的只读单视觉行投影（对应 `cosmic-text` `LayoutRun`）。
#[derive(Copy, Clone, Debug)]
pub struct LayoutRun<'a> {
    /// 所属逻辑行索引（0 起始）。
    pub line_i: usize,
    /// 所属逻辑行内的软折行视觉子行索引（0 表示首行，>0 表示折行续行）。
    pub layout_i: usize,
    /// 所属逻辑行的完整原始文本切片。
    pub text: &'a str,
    /// 本视觉行内所有已定位字形切片。
    pub glyphs: &'a [LayoutGlyph],
    /// 本视觉行相对视口顶部的基线 Y 像素偏移。
    pub line_y: f32,
    /// 本视觉行相对视口顶部的上边缘 Y 像素偏移。
    pub line_top: f32,
    /// 本视觉行的行高（像素）。
    pub line_height: f32,
    /// 本视觉行的内容总宽度（像素）。
    pub line_w: f32,
}

impl<'a> LayoutRun<'a> {
    /// 获取本视觉行在所属逻辑行 `text` 中的字节起止区间 `start..end`。
    #[inline]
    pub fn byte_range(&self) -> (usize, usize) {
        if self.glyphs.is_empty() {
            (0, 0)
        } else {
            (
                self.glyphs[0].start,
                self.glyphs[self.glyphs.len() - 1].end,
            )
        }
    }

    /// 返回本视觉行对应的文本子切片。
    #[inline]
    pub fn slice_str(&self) -> &'a str {
        let (s, e) = self.byte_range();
        if s <= e && e <= self.text.len() && self.text.is_char_boundary(s) && self.text.is_char_boundary(e) {
            &self.text[s..e]
        } else {
            ""
        }
    }

    /// 计算本视觉行中字节偏移 `index` 对应的精确像素 X 坐标。
    #[inline]
    pub fn x_for_index(&self, index: usize) -> f32 {
        if self.glyphs.is_empty() {
            return 0.0;
        }
        for g in self.glyphs {
            if index <= g.start || index < g.end {
                return g.x;
            }
        }
        self.line_w
    }

    /// 计算本视觉行与选区字节区间 `[sel_start, sel_end)` 相交后的水平像素高亮区间 `(x0, x1)`。
    pub fn highlight_x_range(&self, sel_start: usize, sel_end: usize) -> Option<(f32, f32)> {
        if sel_start >= sel_end {
            return None;
        }
        if self.glyphs.is_empty() {
            return None;
        }
        let (line_s, line_e) = self.byte_range();
        let hs = sel_start.max(line_s);
        let he = sel_end.min(line_e);
        if hs >= he {
            return None;
        }
        let x0 = self.x_for_index(hs);
        let x1 = self.x_for_index(he);
        if x1 > x0 {
            Some((x0, x1))
        } else {
            None
        }
    }
}
