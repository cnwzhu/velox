//! 主题化富文本与可选文本组件 `SelectableText`、`MarkdownView`、`TextView`。

use crate::styled::{fill_color, fill_rounded};

pub use velox_base::components::text_view::TextViewFormat;

crate::define_themed_component! {
    name: SelectableText<'a>,
    base: velox_base::SelectableText<'a>,
    new: (id: u32, content: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.selection_bg, theme.primary);
        fill_color(&mut base.selection_fg, theme.primary_foreground);
        fill_rounded(&mut base.rounded, 2);
    },
    forward: [
        selection(selection: Option<(usize, usize)>),
    ],
}

crate::define_themed_component! {
    name: MarkdownView<'a>,
    base: velox_base::MarkdownView<'a>,
    new: (id: u32, source: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.h1_color, theme.foreground);
        fill_color(&mut base.h2_color, theme.foreground);
        fill_color(&mut base.h3_color, theme.secondary_foreground);
        fill_color(&mut base.text_color, theme.secondary_foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.accent_color, theme.primary);
        fill_color(&mut base.code_bg, theme.card);
        fill_color(&mut base.code_fg, theme.primary);
        fill_rounded(&mut base.rounded, 4);
    },
    forward: [
        width(width: i16),
    ],
}

crate::define_themed_component! {
    name: TextView<'a>,
    base: velox_base::TextView<'a>,
    new: (id: u32, source: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.h1_color, theme.foreground);
        fill_color(&mut base.h2_color, theme.foreground);
        fill_color(&mut base.h3_color, theme.secondary_foreground);
        fill_color(&mut base.text_color, theme.secondary_foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.accent_color, theme.primary);
        fill_color(&mut base.code_bg, theme.card);
        fill_color(&mut base.code_fg, theme.primary);
        fill_rounded(&mut base.rounded, 4);
    },
    forward: [
        format(format: TextViewFormat),
        width(width: i16),
    ],
}

impl<'a> TextView<'a> {
    /// 创建一个 HTML 渲染模式的主题化 `TextView` 组件。
    #[inline(always)]
    pub fn html(id: u32, source: &'a str) -> Self {
        Self::from_base(velox_base::TextView::html(id, source))
    }
}
