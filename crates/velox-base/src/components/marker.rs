//! 会话状态与分割线标记组件 `Marker`。

use velox_core::prelude::*;

/// 会话分隔标记样式变体（`MarkerVariant`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MarkerVariant {
    #[default]
    Default,
    Unread,
    System,
    Error,
}

/// 会话状态与时间/未读分隔线标记组件（`Marker`）。
pub struct Marker<'a> {
    pub id: u32,
    pub label: &'a str,
    pub variant: MarkerVariant,
    pub width: i16,
    pub line_color: Color,
    pub pill_bg: Color,
    pub text_color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Marker<'a> {
    /// 创建一个会话分隔标记组件（未显式设置的颜色与圆角根据 `variant` 自动回退至 `Theme`）。
    #[inline(always)]
    pub const fn new(id: u32, label: &'a str) -> Self {
        Self {
            id,
            label,
            variant: MarkerVariant::Default,
            width: 360,
            line_color: Color::TRANSPARENT,
            pill_bg: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置标记变体（如 `Unread` 未读消息分割线）。
    #[inline(always)]
    pub const fn variant(mut self, variant: MarkerVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 设置总宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }
}

crate::impl_base_styled_box!(Marker<'a>);

impl<'a> Widget<'a> for Marker<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let line_color = if self.line_color.a() > 0 {
            self.line_color
        } else {
            fg.scale_alpha(64)
        };
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let side_w = ((width - 120) / 2).max(20);

        let mut pill = div()
            .px(8)
            .py(2)
            .rounded(self.rounded)
            .bg(self.pill_bg)
            .child(cx, text(self.label).size(11).color(text_color));
        if self.line_color.a() > 0 {
            pill = pill.border(1, self.line_color);
        }

        let root = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .h(24)
            .gap(8)
            .items_center()
            .justify_center()
            .child(cx, div().w(side_w).h(1).bg(line_color))
            .child(cx, pill)
            .child(cx, div().w(side_w).h(1).bg(line_color));
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
