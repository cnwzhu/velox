//! 无样式分类标签原语 `Tag` 与语义变体标记 `TagVariant`（默认零背景、零边框、零圆角，仅提供标签与关闭行为）。

use velox_core::prelude::*;

/// 分类标签颜色变体（语义标记，由 `velox-component::Tag` 映射为主题色板）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagVariant {
    #[default]
    Default,
    Primary,
    Success,
    Warning,
    Danger,
    Outline,
}

/// 无样式分类标签原语（`Tag`，支持点击与可关闭按钮行为）。
pub struct Tag<'a> {
    pub id: u32,
    pub label: &'a str,
    pub variant: TagVariant,
    pub closable: bool,
    pub height: i16,
    pub px: i8,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub close_hover_bg: Option<Color>,
    pub close_active_bg: Option<Color>,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_close: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_click: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Tag<'a> {
    /// 创建一个无样式分类标签原语。
    #[inline(always)]
    pub const fn new(id: u32, label: &'a str) -> Self {
        Self {
            id,
            label,
            variant: TagVariant::Default,
            closable: false,
            height: 0,
            px: 0,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            close_hover_bg: None,
            close_active_bg: None,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_close: None,
            on_click: None,
        }
    }

    /// 设置标签语义变体标记（在 `velox-component` 中映射为对应主题配色）。
    #[inline(always)]
    pub const fn variant(mut self, variant: TagVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 设置是否显示关闭按钮 `×`。
    #[inline(always)]
    pub const fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// 显式设置表面配色与圆角。
    #[inline(always)]
    pub const fn colors(mut self, bg: Color, border_color: Color, text_color: Color) -> Self {
        self.bg = bg;
        self.border_color = border_color;
        self.text_color = text_color;
        self
    }

    /// 绑定点击关闭按钮回调。
    #[inline(always)]
    pub fn on_close(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.closable = true;
        self.on_close = Some(handler);
        self
    }

    /// 绑定点击标签本体回调。
    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Tag<'a>);

impl<'a> Widget<'a> for Tag<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let text_color = if self.text_color == Color::TRANSPARENT {
            cx.theme().foreground
        } else {
            self.text_color
        };

        let mut pill = div()
            .id(self.id)
            .flex_row()
            .gap(4)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .child(
                cx,
                text(self.label)
                    .size(11)
                    .color(text_color)
                    .text_center()
                    .nowrap(),
            );

        if self.height > 0 {
            pill = pill.h(self.height);
        }
        if self.px > 0 {
            pill = pill.px(self.px);
        }
        if self.border_color != Color::TRANSPARENT {
            pill = pill.border(1, self.border_color);
        }

        if let Some(cb) = self.on_click {
            pill = pill.focusable().no_focus_border().on_click_raw(cb);
            if let Some(h) = self.hover_bg {
                pill = pill.hover_bg(h);
            }
            if let Some(a) = self.active_bg {
                pill = pill.active_bg(a);
            }
        }

        if self.closable {
            let mut close_btn = div()
                .id(crate::scoped_child_id(self.id, 0x95F7_2993, 1))
                .w(14)
                .h(14)
                .rounded(if self.rounded > 0 {
                    self.rounded.saturating_sub(1).max(2)
                } else {
                    0
                })
                .items_center()
                .justify_center()
                .child(
                    cx,
                    svg_icon(10, 10, "M18 6 6 18M6 6l12 12")
                        .tint(text_color)
                        .stroke_width(22),
                );
            if let Some(h) = self.close_hover_bg {
                close_btn = close_btn.hover_bg(h);
            }
            if let Some(a) = self.close_active_bg {
                close_btn = close_btn.active_bg(a);
            }
            if let Some(cb) = self.on_close {
                close_btn = close_btn.on_click_raw(cb);
            }
            pill = pill.child(cx, close_btn);
        }

        pill = self.style_ext.apply_to_div(pill, self.style);
        pill.into_element(cx)
    }
}
