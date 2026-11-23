//! 主题化表单标签组件 `Label`。

use crate::styled::fill_color;

crate::define_themed_component! {
    name: Label<'a>,
    base: velox_base::Label<'a>,
    new: (id: u32, text: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: true,
    apply_tw: |base, theme| {
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.disabled_color, theme.muted_foreground);
        fill_color(&mut base.description_color, theme.muted_foreground);
        fill_color(&mut base.required_color, theme.danger);
    },
    forward: [
        required(required: bool),
        description(desc: &'a str),
    ],
}
