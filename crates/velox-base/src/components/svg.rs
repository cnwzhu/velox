//! 声明式原生矢量 SVG 图形组件（`Svg<'a>`），支持完整 `<svg>` 多色 XML 与 24×24 Path `d` 矢量图标。

use crate::traits::{ComponentSize, Sizable};
use velox_engine::prelude::*;

/// 声明式原生矢量 SVG 展示组件（`Svg<'a>`）。
///
/// - `Svg::new(id, xml)` 渲染完整多色 `<svg viewBox="...">...</svg>` 文档，支持 `currentColor` 着色；
/// - `Svg::from_path(id, d)` 渲染 24×24 实心填充路径；
/// - `Svg::icon_path(id, d)` 渲染 24×24 圆头圆角描边矢量线条图标（默认 `2.0px`）。
#[derive(Clone, Copy, Debug)]
pub struct Svg<'a> {
    pub id: u32,
    pub source: StrRef<'a>,
    pub is_path_only: bool,
    pub width: i16,
    pub height: i16,
    pub color: Color,
    pub stroke_width_x10: u8,
    pub bg: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    has_custom_color: bool,
}

impl<'a> Svg<'a> {
    /// 使用完整 `<svg>...</svg>` XML 字符串创建矢量图形组件（默认 24×24，保留多色且支持 `currentColor`）。
    #[inline(always)]
    pub fn new(id: u32, xml: impl Into<StrRef<'a>>) -> Self {
        Self {
            id,
            source: xml.into(),
            is_path_only: false,
            width: 24,
            height: 24,
            color: Color::WHITE,
            stroke_width_x10: 0,
            bg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            has_custom_color: false,
        }
    }

    /// 使用 24×24 SVG Path `d` 数据创建实心填充矢量图形组件（未设颜色时回退 `Theme::foreground`）。
    #[inline(always)]
    pub fn from_path(id: u32, d: impl Into<StrRef<'a>>) -> Self {
        Self {
            id,
            source: d.into(),
            is_path_only: true,
            width: 20,
            height: 20,
            color: Color::TRANSPARENT,
            stroke_width_x10: 0,
            bg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            has_custom_color: false,
        }
    }

    /// 使用 24×24 SVG Path `d` 数据创建 Lucide 风格圆头描边线条图标组件（默认 `2.0px` 线宽）。
    #[inline(always)]
    pub fn icon_path(id: u32, d: impl Into<StrRef<'a>>) -> Self {
        Self {
            id,
            source: d.into(),
            is_path_only: true,
            width: 20,
            height: 20,
            color: Color::TRANSPARENT,
            stroke_width_x10: 20,
            bg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            has_custom_color: false,
        }
    }

    /// 设置组件逻辑宽高。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 设置正方形逻辑边长。
    #[inline(always)]
    pub const fn square(mut self, dim: i16) -> Self {
        self.width = dim;
        self.height = dim;
        self
    }

    /// 设置前景色（XML 中映射为 `currentColor` 及整体 Alpha；纯路径模式映射为填充/描边颜色）。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = color;
        self.has_custom_color = true;
        self
    }

    /// 设置描边宽度（单位 `0.1px`，例如 `20` = `2.0px`，设为 `0` 在纯路径模式下切换为实心填充）。
    #[inline(always)]
    pub const fn stroke_width(mut self, width_x10: u8) -> Self {
        self.stroke_width_x10 = width_x10;
        self
    }

    /// 设置可选容器背景色。
    #[inline(always)]
    pub const fn bg(mut self, bg: Color) -> Self {
        self.bg = bg;
        self
    }

    /// 设置可选容器圆角半径。
    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.rounded = radius;
        self
    }
}

crate::impl_base_styled_box!(Svg<'a>);

impl<'a> Sizable for Svg<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        let dim = match size {
            ComponentSize::Small => 16,
            ComponentSize::Medium => 24,
            ComponentSize::Large => 32,
        };
        self.width = dim;
        self.height = dim;
        self
    }
}

impl<'a> Widget<'a> for Svg<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let tint = if self.has_custom_color {
            self.color
        } else if self.is_path_only {
            cx.theme().foreground
        } else {
            // 完整 XML 默认保持多色原貌，同时允许 `currentColor` 跟随主题前景色
            cx.theme().foreground
        };
        let w = StyleExt::resolve_width(self.style, self.width).max(1);
        let h = StyleExt::resolve_height(self.style, self.height).max(1);
        let node = if self.is_path_only {
            if self.stroke_width_x10 == 0 {
                svg_path(w, h, self.source).tint(tint)
            } else {
                svg_icon(w, h, self.source)
                    .tint(tint)
                    .stroke_width(self.stroke_width_x10)
            }
        } else {
            svg_str(w, h, self.source)
                .tint(tint)
                .stroke_width(self.stroke_width_x10)
        };
        if self.bg.a() == 0 && self.rounded == 0 {
            let elem = node.into_element(cx);
            return self.style_ext.wrap_element(cx, elem, self.style);
        }
        let mut container = div()
            .id(self.id)
            .w(w)
            .h(h)
            .items_center()
            .justify_center();
        if self.bg.a() > 0 {
            container = container.bg(self.bg);
        }
        if self.rounded > 0 {
            container = container.rounded(self.rounded).overflow_hidden();
        }
        container = self.style_ext.apply_to_div(container, self.style);
        container.child(cx, node).into_element(cx)
    }
}
