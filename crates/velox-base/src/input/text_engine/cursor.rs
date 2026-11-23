//! 多行文本光标（[`Cursor`]）、亲和方向（[`Affinity`]）、视觉排版光标（[`LayoutCursor`]）、
//! 导航原语（[`Motion`]）与视口滚动模型（[`Scroll`]）。
//!
//! 参照 `pop-os/cosmic-text/src/cursor.rs` 设计，完整兼容 `#![no_std]`。

use core::cmp::Ordering;

use crate::input::cursor::BufferPoint;

/// 光标在自动折行或字符边界处的亲和方向（对应 `cosmic-text` `Affinity`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Affinity {
    /// 关联前一个字形/软折行行尾。
    Before,
    /// 关联后一个字形/软折行行首（默认）。
    #[default]
    After,
}

impl Affinity {
    #[inline(always)]
    pub const fn before(self) -> bool {
        matches!(self, Self::Before)
    }

    #[inline(always)]
    pub const fn after(self) -> bool {
        matches!(self, Self::After)
    }
}

/// 逻辑文档光标位置（对应 `cosmic-text` `Cursor`）。
///
/// 使用 `(line, index)`（0 起始逻辑行号 + 该行内 UTF-8 字节偏移）加亲和方向定位，
/// 天然解耦文档总字节长度，单行编辑不影响其他行光标定位效率。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Cursor {
    /// 0 起始逻辑行号（对应 `Buffer::lines` 索引）。
    pub line: usize,
    /// 行内 UTF-8 字节偏移（不含行尾换行符）。
    pub index: usize,
    /// 在软折行边界上的亲和方向。
    pub affinity: Affinity,
}

impl Cursor {
    /// 创建默认亲和方向（`Affinity::After`）的光标。
    #[inline(always)]
    pub const fn new(line: usize, index: usize) -> Self {
        Self {
            line,
            index,
            affinity: Affinity::After,
        }
    }

    /// 创建显式指定亲和方向的光标。
    #[inline(always)]
    pub const fn new_with_affinity(line: usize, index: usize, affinity: Affinity) -> Self {
        Self {
            line,
            index,
            affinity,
        }
    }

    /// 从兼容的二维行列坐标 [`BufferPoint`] 构造光标。
    #[inline(always)]
    pub const fn from_point(pt: BufferPoint) -> Self {
        Self::new(pt.row, pt.column)
    }

    /// 转换为兼容的二维行列坐标 [`BufferPoint`]。
    #[inline(always)]
    pub const fn to_point(self) -> BufferPoint {
        BufferPoint::new(self.line, self.index)
    }
}

impl PartialOrd for Cursor {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cursor {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.line
            .cmp(&other.line)
            .then_with(|| self.index.cmp(&other.index))
    }
}

/// 视觉排版空间的三维光标定位（对应 `cosmic-text` `LayoutCursor`）：
/// `(逻辑行号, 软折行视觉子行号, 行内字形槽位索引)`。
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LayoutCursor {
    pub line: usize,
    pub layout: usize,
    pub glyph: usize,
}

impl LayoutCursor {
    #[inline(always)]
    pub const fn new(line: usize, layout: usize, glyph: usize) -> Self {
        Self {
            line,
            layout,
            glyph,
        }
    }
}

/// 光标移动与导航动作枚举（对应 `cosmic-text` `Motion`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// 向左移动一个字符（行首时回退至上一行行尾）。
    Left,
    /// 向右移动一个字符（行尾时前进至下一行行首）。
    Right,
    /// 向上移动一个视觉行（支持软折行并保持记忆像素列 `cursor_x_opt`）。
    Up,
    /// 向下移动一个视觉行（支持软折行并保持记忆像素列 `cursor_x_opt`）。
    Down,
    /// 移动到当前视觉行行首（第 0 列）。
    Home,
    /// 智能行首：若当前不在首个非空白字符则跳至首个非空白字符，否则跳至行首第 0 列。
    SoftHome,
    /// 移动到当前视觉行行尾。
    End,
    /// 移动到当前逻辑段落（逻辑行）起始处。
    ParagraphStart,
    /// 移动到当前逻辑段落（逻辑行）末尾处。
    ParagraphEnd,
    /// 向上翻一页（按视口高度推进视觉行）。
    PageUp,
    /// 向下翻一页（按视口高度推进视觉行）。
    PageDown,
    /// 按指定像素增量垂直移动（负值向上，正值向下）。
    Vertical(i32),
    /// 向左跳转至上一个词边界（等价 `Ctrl+Left`）。
    PreviousWord,
    /// 向右跳转至下一个词边界（等价 `Ctrl+Right`）。
    NextWord,
    /// 向左按词跳转（与 `PreviousWord` 对称，对齐 `cosmic-text` 命名）。
    LeftWord,
    /// 向右按词跳转（与 `NextWord` 对称，对齐 `cosmic-text` 命名）。
    RightWord,
    /// 逻辑顺序前移一个字符（不区分视觉 RTL 方向）。
    Previous,
    /// 逻辑顺序后移一个字符。
    Next,
    /// 跳转至整个文档开头（`Ctrl+Home`）。
    BufferStart,
    /// 跳转至整个文档末尾（`Ctrl+End`）。
    BufferEnd,
    /// 跳转至指定 0 起始逻辑行首。
    GotoLine(usize),
}

/// 多行文本缓冲区视口滚动状态（对应 `cosmic-text` `Scroll`）。
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Scroll {
    /// 顶部首个可见逻辑行索引。
    pub line: usize,
    /// 相对于 `line` 顶部的垂直像素微偏移（支持平滑像素滚动与软折行半行滚出）。
    pub vertical: f32,
    /// 水平滚动像素偏移（`Wrap::None` 长行横向滚动时使用）。
    pub horizontal: f32,
}

impl Scroll {
    #[inline(always)]
    pub const fn new(line: usize, vertical: f32) -> Self {
        Self {
            line,
            vertical,
            horizontal: 0.0,
        }
    }

    #[inline(always)]
    pub const fn with_horizontal(mut self, horizontal: f32) -> Self {
        self.horizontal = horizontal;
        self
    }
}
