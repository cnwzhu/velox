//! 主题化对话气泡组件 `Bubble`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::bubble::{BubbleAlign, BubbleReaction};

crate::define_themed_component! {
    name: Bubble<'a>,
    base: velox_base::Bubble<'a>,
    new: (id: u32, content: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        let (def_bg, def_border, def_fg) = match base.align {
            BubbleAlign::Start => (theme.secondary, theme.border, theme.secondary_foreground),
            BubbleAlign::End => (theme.primary, theme.primary, theme.primary_foreground),
        };
        fill_color(&mut base.bg, def_bg);
        fill_color(&mut base.border_color, def_border);
        fill_color(&mut base.text_color, def_fg);
        fill_color(&mut base.reaction_bg, theme.card);
        fill_color(&mut base.reaction_active_bg, theme.secondary);
        fill_color(&mut base.reaction_border_color, theme.border);
        fill_color(&mut base.reaction_active_border_color, theme.primary);
        fill_color(&mut base.reaction_text_color, theme.muted_foreground);
        fill_color(&mut base.reaction_active_text_color, theme.primary);
        fill_rounded(&mut base.rounded, 10);
    },
    forward: [
        align(align: BubbleAlign),
        reactions(reactions: &'a [BubbleReaction<'a>]),
        max_width(max_width: i16),
        on_reaction_click(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}
