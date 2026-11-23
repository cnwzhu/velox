//! 矢量 SVG 图标展示组件 `Icon` 与内置标准图标枚举 `IconName`（24×24 Lucide 风格原生抗锯齿矢量路径）。

use crate::traits::{ComponentSize, Sizable};
use velox_engine::prelude::*;

/// 内置常用图标名称枚举（`IconName`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IconName<'a> {
    #[default]
    Info,
    Check,
    Close,
    Plus,
    Minus,
    Search,
    Settings,
    Star,
    StarOutline,
    Warning,
    Error,
    ChevronUp,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    File,
    Folder,
    Mic,
    Trash,
    Custom(&'a str),
}

impl<'a> IconName<'a> {
    /// 获取图标对应的紧凑文本符号回退表示。
    #[inline(always)]
    pub const fn glyph(self) -> &'a str {
        match self {
            Self::Info => "i",
            Self::Check => "✓",
            Self::Close => "×",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Search => "⌕",
            Self::Settings => "⚙",
            Self::Star => "★",
            Self::StarOutline => "☆",
            Self::Warning => "!",
            Self::Error => "✕",
            Self::ChevronUp => "^",
            Self::ChevronDown => "v",
            Self::ChevronLeft => "<",
            Self::ChevronRight => ">",
            Self::File => "📄",
            Self::Folder => "📁",
            Self::Mic => "🎤",
            Self::Trash => "🗑",
            Self::Custom(s) => s,
        }
    }

    /// 返回内置图标对应的标准 24×24 SVG Path `d` 数据（`Custom` 返回 `None` 并自动回退文本渲染）。
    #[inline(always)]
    pub const fn svg_path(self) -> Option<&'static str> {
        match self {
            Self::Info => Some(
                "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM12 16v-4M12 8h.01",
            ),
            Self::Check => Some("M20 6 9 17l-5-5"),
            Self::Close => Some("M18 6 6 18M6 6l12 12"),
            Self::Plus => Some("M12 5v14M5 12h14"),
            Self::Minus => Some("M5 12h14"),
            Self::Search => Some(
                "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16ZM21 21l-4.35-4.35",
            ),
            Self::Settings => Some(
                "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 .99-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51.99H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z",
            ),
            Self::Star | Self::StarOutline => Some(
                "M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2Z",
            ),
            Self::Warning => Some(
                "M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0ZM12 9v4M12 17h.01",
            ),
            Self::Error => Some(
                "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM15 9l-6 6M9 9l6 6",
            ),
            Self::ChevronUp => Some("M18 15l-6-6-6 6"),
            Self::ChevronDown => Some("M6 9l6 6 6-6"),
            Self::ChevronLeft => Some("M15 18l-6-6 6-6"),
            Self::ChevronRight => Some("M9 18l6-6-6-6"),
            Self::File => Some(
                "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8ZM14 2v6h6",
            ),
            Self::Folder => Some(
                "M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2Z",
            ),
            Self::Mic => Some(
                "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3ZM19 10v2a7 7 0 0 1-14 0v-2M12 19v3M8 22h8",
            ),
            Self::Trash => Some(
                "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M10 11v6M14 11v6",
            ),
            Self::Custom(_) => None,
        }
    }

    /// 判断内置矢量 SVG 图标是否应以实心填充（如 `Star`）而非线条描边渲染。
    #[inline(always)]
    pub const fn is_filled_svg(self) -> bool {
        matches!(self, Self::Star)
    }
}

/// 矢量图标展示组件（`Icon`）。
#[derive(Clone, Copy, Debug)]
pub struct Icon<'a> {
    pub id: u32,
    pub name: IconName<'a>,
    pub size: ComponentSize,
    pub px_size: u8,
    pub color: Color,
    pub stroke_width_x10: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    has_custom_color: bool,
}

impl<'a> Icon<'a> {
    /// 创建一个指定图标名称的图标组件（未设置颜色时自动回退至 `Theme::foreground`）。
    #[inline(always)]
    pub const fn new(id: u32, name: IconName<'a>) -> Self {
        Self {
            id,
            name,
            size: ComponentSize::Medium,
            px_size: 14,
            color: Color::TRANSPARENT,
            stroke_width_x10: 20,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            has_custom_color: false,
        }
    }

    /// 使用自定义符号字符串创建图标组件。
    #[inline(always)]
    pub const fn symbol(id: u32, glyph: &'a str) -> Self {
        Self::new(id, IconName::Custom(glyph))
    }

    /// 设置图标颜色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = color;
        self.has_custom_color = true;
        self
    }

    /// 设置精确像素尺寸。
    #[inline(always)]
    pub const fn px(mut self, px_size: u8) -> Self {
        self.px_size = px_size;
        self
    }

    /// 设置矢量描边宽度（单位 `0.1px`，默认 `20` 即 `2.0px`）。
    #[inline(always)]
    pub const fn stroke_width(mut self, width_x10: u8) -> Self {
        self.stroke_width_x10 = width_x10;
        self
    }
}

crate::impl_base_styled_box!(Icon<'a>);

impl<'a> Sizable for Icon<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self.px_size = match size {
            ComponentSize::Small => 12,
            ComponentSize::Medium => 16,
            ComponentSize::Large => 20,
        };
        self
    }
}

impl<'a> Widget<'a> for Icon<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let color = if !self.has_custom_color && self.color == Color::TRANSPARENT {
            cx.theme().foreground
        } else {
            self.color
        };
        let dim = (self.px_size as i16).max(8);
        let box_dim = dim + 2;
        if let Some(d) = self.name.svg_path() {
            let graphic = if self.name.is_filled_svg() {
                svg_path(dim, dim, d).tint(color)
            } else {
                svg_icon(dim, dim, d)
                    .tint(color)
                    .stroke_width(self.stroke_width_x10.max(1))
            };
            let elem = graphic.into_element(cx);
            self.style_ext.wrap_element(cx, elem, self.style)
        } else {
            let d = div()
                .id(self.id)
                .w(box_dim + 2)
                .h(box_dim + 2)
                .items_center()
                .justify_center()
                .child(
                    cx,
                    text(self.name.glyph()).size(self.px_size).color(color),
                );
            self.style_ext.apply_to_div(d, self.style).into_element(cx)
        }
    }
}
