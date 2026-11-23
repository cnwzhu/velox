//! 主题化矢量图标组件 `Icon`。

use velox_engine::prelude::*;

pub use velox_base::components::icon::IconName;

crate::define_themed_component! {
    name: Icon<'a>,
    base: velox_base::Icon<'a>,
    new: (id: u32, name: IconName<'a>),
    styled: false,
    variants: false,
    sizable: true,
    disableable: false,
    apply_tw: |_base, _theme| {},
    forward: [
        color(color: Color),
        px(px_size: u8),
        stroke_width(width_x10: u8),
    ],
}

impl<'a> Icon<'a> {
    /// 使用自定义符号字符串创建主题化图标组件。
    #[inline(always)]
    pub fn symbol(id: u32, glyph: &'a str) -> Self {
        Self::from_base(velox_base::Icon::symbol(id, glyph))
    }
}
