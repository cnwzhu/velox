//! 主题化聊天消息项组件 `Message`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::components::bubble::BubbleReaction;

pub use velox_base::components::message::MessageRole;

crate::define_themed_component! {
    name: Message<'a>,
    base: velox_base::Message<'a>,
    new: (id: u32, role: MessageRole, author: &'a str, content: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        let (def_b_bg, def_b_border, def_b_fg, def_a_bg, def_a_fg) = match base.role {
            MessageRole::User => (
                theme.primary,
                theme.primary,
                theme.primary_foreground,
                theme.primary,
                theme.primary_foreground,
            ),
            MessageRole::System => (
                theme.muted,
                theme.border,
                theme.muted_foreground,
                theme.muted,
                theme.muted_foreground,
            ),
            MessageRole::Assistant | MessageRole::Tool => (
                theme.secondary,
                theme.border,
                theme.secondary_foreground,
                theme.secondary,
                theme.secondary_foreground,
            ),
        };
        fill_color(&mut base.author_color, theme.foreground);
        fill_color(&mut base.timestamp_color, theme.muted_foreground);
        fill_color(&mut base.bubble_bg, def_b_bg);
        fill_color(&mut base.bubble_border_color, def_b_border);
        fill_color(&mut base.bubble_text_color, def_b_fg);
        fill_color(&mut base.avatar_bg, def_a_bg);
        fill_color(&mut base.avatar_fg, def_a_fg);
        fill_rounded(&mut base.avatar_rounded, 14);
        fill_rounded(&mut base.rounded, 10);
    },
    forward: [
        avatar(initials: &'a str),
        timestamp(timestamp: &'a str),
        attachments(attachments: &'a [velox_base::Attachment<'a>]),
        reactions(reactions: &'a [BubbleReaction<'a>]),
        width(width: i16),
    ],
}
