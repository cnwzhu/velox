//! 主题化颜色选择器与色板组件 `ColorPicker`、`ColorSwatch`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::color_picker::{ColorPickerState, ColorSwatchState};

crate::define_themed_component! {
    name: ColorSwatch<'a>,
    base: velox_base::ColorSwatch<'a>,
    new: (id: u32, color: Color),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.selected_border_color, theme.foreground);
        fill_rounded(&mut base.rounded, 4);
    },
    forward: [
        selected(selected: bool),
        on_click(handler: &'a dyn Fn(Color, &mut ActionScope)),
    ],
}

crate::define_themed_component! {
    name: ColorPicker<'a>,
    base: velox_base::ColorPicker<'a>,
    new: (id: u32, state: ColorPickerState),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.animated = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.secondary);
        fill_color(&mut base.popup_bg, theme.popover);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.swatch_border_color, theme.border);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        animated(enabled: bool),
        on_toggle_open(handler: &'a dyn Fn(&mut ActionScope)),
        on_select_color(handler: &'a dyn Fn(Color, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(ColorPicker, trigger, (Div<'a>, Color, bool));

crate::forward_slot_el!(ColorPicker, swatch, (Div<'a>, ColorSwatchState));
