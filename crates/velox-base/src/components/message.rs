//! 可组合聊天消息结构组件 `Message`。

use crate::components::{Attachment, Avatar, Bubble, BubbleAlign, BubbleReaction};
use crate::traits::Sizable;
use velox_core::prelude::*;

/// 消息发送方角色枚举（`MessageRole`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MessageRole {
    #[default]
    User,
    Assistant,
    System,
    Tool,
}

/// 可组合聊天消息结构组件（`Message`，整合头像、发送者名称、时间戳、气泡、附件与表情回应）。
#[derive(Clone, Copy)]
pub struct Message<'a> {
    pub id: u32,
    pub role: MessageRole,
    pub author: &'a str,
    pub avatar_initials: &'a str,
    pub timestamp: &'a str,
    pub content: &'a str,
    pub attachments: &'a [Attachment<'a>],
    pub reactions: &'a [BubbleReaction<'a>],
    pub width: i16,
    pub author_color: Color,
    pub timestamp_color: Color,
    pub bubble_bg: Color,
    pub bubble_border_color: Color,
    pub bubble_text_color: Color,
    pub avatar_bg: Color,
    pub avatar_fg: Color,
    pub avatar_rounded: u8,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Message<'a> {
    /// 创建一个可组合聊天消息组件（未显式设置的颜色与圆角在渲染时按 `role` 自动回退至 `Theme`）。
    #[inline(always)]
    pub const fn new(
        id: u32,
        role: MessageRole,
        author: &'a str,
        content: &'a str,
    ) -> Self {
        Self {
            id,
            role,
            author,
            avatar_initials: "AI",
            timestamp: "",
            content,
            attachments: &[],
            reactions: &[],
            width: 360,
            author_color: Color::TRANSPARENT,
            timestamp_color: Color::TRANSPARENT,
            bubble_bg: Color::TRANSPARENT,
            bubble_border_color: Color::TRANSPARENT,
            bubble_text_color: Color::TRANSPARENT,
            avatar_bg: Color::TRANSPARENT,
            avatar_fg: Color::TRANSPARENT,
            avatar_rounded: 0,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置头像缩写文本。
    #[inline(always)]
    pub const fn avatar(mut self, initials: &'a str) -> Self {
        self.avatar_initials = initials;
        self
    }

    /// 设置时间戳文本。
    #[inline(always)]
    pub const fn timestamp(mut self, timestamp: &'a str) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// 设置消息携带的附件列表。
    #[inline(always)]
    pub const fn attachments(mut self, attachments: &'a [Attachment<'a>]) -> Self {
        self.attachments = attachments;
        self
    }

    /// 设置消息气泡的表情回应列表。
    #[inline(always)]
    pub const fn reactions(mut self, reactions: &'a [BubbleReaction<'a>]) -> Self {
        self.reactions = reactions;
        self
    }

    /// 设置消息容器总宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }
}

crate::impl_base_styled_box!(Message<'a>);

impl<'a> Widget<'a> for Message<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let is_user = self.role == MessageRole::User;
        let align = if is_user {
            BubbleAlign::End
        } else {
            BubbleAlign::Start
        };

        let author_color = if self.author_color.a() > 0 {
            self.author_color
        } else {
            fg
        };
        let timestamp_color = if self.timestamp_color.a() > 0 {
            self.timestamp_color
        } else {
            fg.scale_alpha(150)
        };
        let bubble_text_color = if self.bubble_text_color.a() > 0 {
            self.bubble_text_color
        } else {
            fg
        };
        let avatar_fg = if self.avatar_fg.a() > 0 {
            self.avatar_fg
        } else {
            fg
        };

        let mut meta_row = div()
            .flex_row()
            .gap(6)
            .items_center()
            .child(cx, text(self.author).size(11).color(author_color));
        if !self.timestamp.is_empty() {
            meta_row = meta_row.child(
                cx,
                text(self.timestamp)
                    .size(10)
                    .color(timestamp_color),
            );
        }

        let width = StyleExt::resolve_width(self.style, self.width);
        let bubble_w = (width - 48).max(120);
        let mut bubble = Bubble::new(crate::scoped_child_id(self.id, 0x24F2_08E4, 1), self.content)
            .align(align)
            .max_width(bubble_w)
            .reactions(self.reactions);
        bubble.bg = self.bubble_bg;
        bubble.border_color = self.bubble_border_color;
        bubble.text_color = bubble_text_color;
        bubble.rounded = self.rounded;

        let mut body_col = div()
            .flex_col()
            .gap(4)
            .child(cx, meta_row)
            .child(cx, bubble);

        if is_user {
            body_col = body_col.items_end();
        }

        for att in self.attachments {
            body_col = body_col.child(cx, *att);
        }

        let mut avatar = Avatar::new(crate::scoped_child_id(self.id, 0x24F2_08E4, 2), self.avatar_initials).small();
        avatar.bg = self.avatar_bg;
        avatar.fg = avatar_fg;
        avatar.rounded = self.avatar_rounded;

        let mut row = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .gap(8)
            .items_start();

        if is_user {
            row = row
                .justify_end()
                .child(cx, body_col)
                .child(cx, avatar);
        } else {
            row = row.child(cx, avatar).child(cx, body_col);
        }

        row = self.style_ext.apply_to_div(row, self.style);
        row.into_element(cx)
    }
}
