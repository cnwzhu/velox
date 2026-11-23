//! 主题化轻量提示条组件 `Toast`。

use crate::styled::fill_color;

crate::define_themed_component! {
    name: Toast<'a>,
    base: velox_base::Toast<'a>,
    new: (id: u32, message: &'a str, x: i16, y: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.rounded = None;
        base.animated = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.primary);
        fill_color(&mut base.text_color, theme.foreground);
        if base.shadow_alpha == 0 {
            base.shadow_alpha = 140;
        }
    },
    forward: [
        size(width: i16, height: i16),
        animated(enabled: bool),
    ],
}
