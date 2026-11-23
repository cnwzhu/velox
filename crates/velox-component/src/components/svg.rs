//! 主题化矢量图形与图标路径组件 `Svg`。

use velox_engine::prelude::*;

crate::define_themed_component! {
    name: Svg<'a>,
    base: velox_base::Svg<'a>,
    new: (id: u32, xml: impl Into<StrRef<'a>>),
    styled: false,
    variants: false,
    sizable: true,
    disableable: false,
    apply_tw: |_base, _theme| {},
    forward: [
        size(width: i16, height: i16),
        square(dim: i16),
        color(color: Color),
        stroke_width(width_x10: u8),
        bg(bg: Color),
        rounded(radius: u8),
    ],
}

impl<'a> Svg<'a> {
    /// 使用 24×24 SVG Path `d` 数据创建实心填充矢量图形组件。
    #[inline(always)]
    pub fn from_path(id: u32, d: impl Into<StrRef<'a>>) -> Self {
        Self::from_base(velox_base::Svg::from_path(id, d))
    }

    /// 使用 24×24 SVG Path `d` 数据创建 Lucide 风格圆头描边线条图标组件。
    #[inline(always)]
    pub fn icon_path(id: u32, d: impl Into<StrRef<'a>>) -> Self {
        Self::from_base(velox_base::Svg::icon_path(id, d))
    }
}
