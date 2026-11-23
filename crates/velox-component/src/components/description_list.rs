//! 主题化键值对描述列表组件 `DescriptionList`。

use crate::styled::{fill_color, fill_rounded};

pub use velox_base::components::description_list::DescriptionItem;

crate::define_themed_component! {
    name: DescriptionList<'a>,
    base: velox_base::DescriptionList<'a>,
    new: (id: u32, items: &'a [DescriptionItem<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.alt_bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.label_color, theme.muted_foreground);
        fill_color(&mut base.value_color, theme.foreground);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        title(title: &'a str),
        widths(width: i16, label_width: i16),
        bordered(bordered: bool),
    ],
}
