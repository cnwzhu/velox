//! 主题化手风琴折叠面板组件 `Accordion`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::accordion::{AccordionItem, AccordionItemState};

crate::define_themed_component! {
    name: Accordion<'a>,
    base: velox_base::Accordion<'a>,
    new: (id: u32, items: &'a [AccordionItem<'a>], open_mask: u32),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
        {
            base.animated = true;
        }
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.header_bg, theme.card);
        fill_color(&mut base.content_bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.body_text_color, theme.muted_foreground);
        fill_color(&mut base.arrow_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        width(width: i16),
        animated(enabled: bool),
        on_toggle_item(handler: &'a dyn Fn(usize, bool, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Accordion, item, (Div<'a>, AccordionItemState<'a>));
