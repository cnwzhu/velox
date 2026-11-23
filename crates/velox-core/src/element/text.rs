//! 文本原语节点 `Text<'a>` 与排版修饰符（字号、颜色、对齐、溢出省略、单行不折行）。

use crate::arena::StrRef;
use crate::pod::FrameScope;
use crate::style::{Color, TextAlign};

use super::{Element, IntoElement};

impl<'a> IntoElement<'a> for Text<'a> {
    #[inline(always)]
    fn into_element(self, _cx: &'a FrameScope) -> Element<'a> {
        Element::Text(self)
    }
}


/// 文本原语节点。
#[derive(Copy, Clone, Debug)]
pub struct Text<'a> {
    pub content: StrRef<'a>,
    pub font_size: u8,
    /// 位域：`align(2b: 0=Left, 1=Center, 2=Right) | ellipsis(1b: 0x04) | nowrap(1b: 0x08) | explicit_size(1b: 0x10) | explicit_color(1b: 0x20)`。
    pub flags: u8,
    pub color: Color,
}

const TEXT_FLAG_EXPLICIT_SIZE: u8 = 0x10;
const TEXT_FLAG_EXPLICIT_COLOR: u8 = 0x20;

/// 构造一个文本原子节点（同时支持静态字符串 `&'static str` 与 `fmt!(cx, ...)` 生成的 `StrRef<'a>`）。
///
/// 未显式调用 `.size(...)` / `.color(...)` 时，自动继承父容器 `Div` 上设置的 `.text_xs()` / `.text_color(...)`。
#[inline(always)]
pub fn text<'a>(content: impl Into<StrRef<'a>>) -> Text<'a> {
    Text {
        content: content.into(),
        font_size: 14,
        flags: 0,
        color: Color::WHITE,
    }
}

impl<'a> Text<'a> {
    /// 设置字号大小（像素）。
    #[inline(always)]
    pub const fn size(mut self, px: u8) -> Self {
        self.font_size = px;
        self.flags |= TEXT_FLAG_EXPLICIT_SIZE;
        self
    }

    /// 设置字号大小（与 `Div::text_size` 对齐的别名）。
    #[inline(always)]
    pub const fn text_size(self, px: u8) -> Self {
        self.size(px)
    }

    #[inline(always)] pub const fn text_xs(self) -> Self { self.size(11) }
    #[inline(always)] pub const fn text_sm(self) -> Self { self.size(12) }
    #[inline(always)] pub const fn text_base(self) -> Self { self.size(14) }
    #[inline(always)] pub const fn text_lg(self) -> Self { self.size(16) }
    #[inline(always)] pub const fn text_xl(self) -> Self { self.size(18) }
    #[inline(always)] pub const fn text_2xl(self) -> Self { self.size(22) }
    #[inline(always)] pub const fn text_3xl(self) -> Self { self.size(28) }

    /// 设置文本颜色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = color;
        self.flags |= TEXT_FLAG_EXPLICIT_COLOR;
        self
    }

    /// 设置文本颜色（与 GPUI `.text_color(...)` 对齐的别名）。
    #[inline(always)]
    pub const fn text_color(self, color: Color) -> Self {
        self.color(color)
    }

    /// 是否显式覆盖了字号（否则继承父 `Div` 的 `text_size`）。
    #[inline(always)]
    pub const fn has_explicit_size(&self) -> bool {
        (self.flags & TEXT_FLAG_EXPLICIT_SIZE) != 0
    }

    /// 是否显式覆盖了颜色（否则继承父 `Div` 的 `text_color`）。
    #[inline(always)]
    pub const fn has_explicit_color(&self) -> bool {
        (self.flags & TEXT_FLAG_EXPLICIT_COLOR) != 0
    }

    /// 设置文本水平对齐方式。
    #[inline(always)]
    pub const fn align(mut self, align: TextAlign) -> Self {
        self.flags = (self.flags & !0x03) | (align as u8 & 0x03);
        self
    }

    /// 设置文本左对齐。
    #[inline(always)]
    pub const fn text_left(self) -> Self {
        self.align(TextAlign::Left)
    }

    /// 设置文本水平居中对齐。
    #[inline(always)]
    pub const fn text_center(self) -> Self {
        self.align(TextAlign::Center)
    }

    /// 设置文本右对齐。
    #[inline(always)]
    pub const fn text_right(self) -> Self {
        self.align(TextAlign::Right)
    }

    /// 按当前语言的书写方向自动设置“逻辑起始端对齐”（LTR 为左对齐，RTL 如阿拉伯语/希伯来语自动镜像为右对齐）。
    #[inline(always)]
    pub fn text_start(self, cx: &FrameScope) -> Self {
        if cx.is_rtl() {
            self.align(TextAlign::Right)
        } else {
            self.align(TextAlign::Left)
        }
    }

    /// 按当前语言的书写方向自动设置“逻辑末端对齐”（LTR 为右对齐，RTL 自动镜像为左对齐）。
    #[inline(always)]
    pub fn text_end(self, cx: &FrameScope) -> Self {
        if cx.is_rtl() {
            self.align(TextAlign::Left)
        } else {
            self.align(TextAlign::Right)
        }
    }

    /// 开启单行省略号截断（超出容器宽度时自动截断并追加 `...`）。
    #[inline(always)]
    pub const fn ellipsis(mut self) -> Self {
        self.flags |= 0x04 | 0x08;
        self
    }

    /// 禁用自动多行折行（强制单行排版）。
    #[inline(always)]
    pub const fn nowrap(mut self) -> Self {
        self.flags |= 0x08;
        self
    }

    /// 读取水平对齐方式。
    #[inline(always)]
    pub const fn text_align(&self) -> TextAlign {
        match self.flags & 0x03 {
            1 => TextAlign::Center,
            2 => TextAlign::Right,
            _ => TextAlign::Left,
        }
    }

    /// 是否启用省略号截断。
    #[inline(always)]
    pub const fn is_ellipsis(&self) -> bool {
        (self.flags & 0x04) != 0
    }

    /// 是否禁用自动多行换行。
    #[inline(always)]
    pub const fn is_nowrap(&self) -> bool {
        (self.flags & 0x08) != 0
    }
}

