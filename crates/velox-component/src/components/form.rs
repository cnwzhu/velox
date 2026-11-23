//! 主题化表单容器与字段组件 `Form`、`FormField`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::form::FormLayout;

crate::define_themed_component! {
    name: FormField<'a>,
    base: velox_base::FormField<'a>,
    new: (id: u32, label: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.label_color, theme.foreground);
        fill_color(&mut base.required_color, theme.danger);
        fill_color(&mut base.help_color, theme.muted_foreground);
        fill_color(&mut base.error_color, theme.danger);
    },
    forward: [
        required(required: bool),
        help(help_text: &'a str),
        error(error_text: &'a str),
        layout(layout: FormLayout),
        control(cx: &'a FrameScope, child: impl IntoElement<'a>),
    ],
}

crate::define_themed_component! {
    name: Form<'a>,
    base: velox_base::Form<'a>,
    new: (id: u32, width: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.submit_bg, theme.primary);
        fill_color(&mut base.submit_fg, theme.primary_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        title(title: &'a str),
        layout(layout: FormLayout),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
        on_submit(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
