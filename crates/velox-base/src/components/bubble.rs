//! 聊天消息气泡组件 `Bubble`。

use velox_engine::prelude::*;

/// 聊天气泡水平对齐方式（`BubbleAlign`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BubbleAlign {
    #[default]
    Start,
    End,
}

/// 消息气泡表情回应条目（`BubbleReaction`）。
#[derive(Clone, Copy, Debug)]
pub struct BubbleReaction<'a> {
    pub label: &'a str,
    pub count: u16,
    pub active: bool,
}

impl<'a> BubbleReaction<'a> {
    /// 创建一个消息表情回应条目（例如 `BubbleReaction::new("👍 3", 3, true)`）。
    #[inline(always)]
    pub const fn new(label: &'a str, count: u16, active: bool) -> Self {
        Self {
            label,
            count,
            active,
        }
    }
}

/// 聊天消息气泡表面基础组件（`Bubble`，默认无样式，支持左/右对齐与表情回应胶囊）。
pub struct Bubble<'a> {
    pub id: u32,
    pub content: &'a str,
    pub align: BubbleAlign,
    pub reactions: &'a [BubbleReaction<'a>],
    pub max_width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub reaction_bg: Color,
    pub reaction_active_bg: Color,
    pub reaction_border_color: Color,
    pub reaction_active_border_color: Color,
    pub reaction_text_color: Color,
    pub reaction_active_text_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_reaction_click: Option<&'a dyn Fn(usize, &mut ActionScope)>,
}

impl<'a> Bubble<'a> {
    /// 创建一个默认无样式的聊天消息气泡。
    #[inline(always)]
    pub const fn new(id: u32, content: &'a str) -> Self {
        Self {
            id,
            content,
            align: BubbleAlign::Start,
            reactions: &[],
            max_width: 280,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            reaction_bg: Color::TRANSPARENT,
            reaction_active_bg: Color::TRANSPARENT,
            reaction_border_color: Color::TRANSPARENT,
            reaction_active_border_color: Color::TRANSPARENT,
            reaction_text_color: Color::TRANSPARENT,
            reaction_active_text_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_reaction_click: None,
        }
    }

    /// 设置气泡对齐方向（`Start` 左侧他人/AI 消息，`End` 右侧当前用户消息）。
    #[inline(always)]
    pub const fn align(mut self, align: BubbleAlign) -> Self {
        self.align = align;
        self
    }

    /// 设置表情回应列表。
    #[inline(always)]
    pub const fn reactions(mut self, reactions: &'a [BubbleReaction<'a>]) -> Self {
        self.reactions = reactions;
        self
    }

    /// 设置最大宽度。
    #[inline(always)]
    pub const fn max_width(mut self, max_width: i16) -> Self {
        self.max_width = max_width;
        self
    }

    /// 绑定点击表情回应胶囊回调。
    #[inline(always)]
    pub fn on_reaction_click(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_reaction_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Bubble<'a>);

impl<'a> Widget<'a> for Bubble<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let reaction_text_color = if self.reaction_text_color.a() > 0 {
            self.reaction_text_color
        } else {
            fg.scale_alpha(150)
        };
        let reaction_active_text_color = if self.reaction_active_text_color.a() > 0 {
            self.reaction_active_text_color
        } else {
            fg
        };

        let mut surface = div()
            .w(self.max_width)
            .px(12)
            .py(8)
            .rounded(self.rounded)
            .bg(self.bg)
            .child(cx, text(self.content).size(13).color(text_color));
        if self.border_color.a() > 0 {
            surface = surface.border(1, self.border_color);
        }

        let mut col = div().id(self.id).flex_col().gap(4);
        if self.align == BubbleAlign::End {
            col = col.items_end();
        }
        col = col.child(cx, surface);

        if !self.reactions.is_empty() {
            let mut react_row = div().flex_row().gap(4).items_center();
            let on_react = self.on_reaction_click;
            for (idx, r) in self.reactions.iter().enumerate() {
                let r_bcol = if r.active {
                    self.reaction_active_border_color
                } else {
                    self.reaction_border_color
                };
                let mut pill = div()
                    .id(crate::scoped_child_id(self.id, 0x943F_DA6F, idx as u32 + 1))
                    .h(20)
                    .px(6)
                    .rounded(self.rounded)
                    .bg(if r.active {
                        self.reaction_active_bg
                    } else {
                        self.reaction_bg
                    })
                    .items_center()
                    .justify_center()
                    .focusable();
                if r_bcol.a() > 0 {
                    pill = pill.border(1, r_bcol);
                }
                if !self.focus_ring {
                    pill = pill.no_focus_border();
                }
                pill = pill.child(
                    cx,
                    text(r.label).size(11).color(if r.active {
                        reaction_active_text_color
                    } else {
                        reaction_text_color
                    }),
                );
                if let Some(cb) = on_react {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    pill = pill.on_click_raw(click_cb);
                }
                react_row = react_row.child(cx, pill);
            }
            col = col.child(cx, react_row);
        }

        col = self.style_ext.apply_to_div(col, self.style);
        col.into_element(cx)
    }
}
