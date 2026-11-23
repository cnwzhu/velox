//! 主题化图片展示组件 `Image`。

use crate::styled::{fill_color, fill_rounded};

pub use velox_base::components::image::ImageFit;

crate::define_themed_component! {
    name: Image<'a>,
    base: velox_base::Image<'a>,
    new: (id: u32, width: i16, height: i16, src: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        fallback(fallback: &'a str),
        fit(fit: ImageFit),
        rounded(radius: u8),
        pixels(pixels: &'a [u32], src_w: u16, src_h: u16),
    ],
}
