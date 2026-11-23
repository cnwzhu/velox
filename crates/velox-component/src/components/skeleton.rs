//! 主题化骨架屏占位组件 `Skeleton`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::skeleton::SkeletonShape;

crate::define_themed_component! {
    name: Skeleton,
    base: velox_base::Skeleton,
    new: (id: u32, width: i16, height: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.color, theme.muted);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        color(color: Color),
        rounded(rounded: u8),
    ],
}

impl Skeleton {
    /// 创建多行文本段落主题化骨架屏。
    #[inline(always)]
    pub fn text_lines(id: u32, width: i16, lines: u8) -> Self {
        Self::from_base(velox_base::Skeleton::text_lines(id, width, lines))
    }

    /// 创建圆形头像主题化骨架屏。
    #[inline(always)]
    pub fn circle(id: u32, diameter: i16) -> Self {
        Self::from_base(velox_base::Skeleton::circle(id, diameter))
    }
}
