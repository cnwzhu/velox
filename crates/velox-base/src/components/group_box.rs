//! 带标题内容分组框基础组件 `GroupBox`（默认无样式）。

use velox_engine::prelude::*;

/// 带标题内容分组框基础组件（`GroupBox`，默认无样式）。
pub struct GroupBox<'a> {
    pub id: u32,
    pub title: &'a str,
    pub description: &'a str,
    pub width: i16,
    pub bg: Color,
    pub header_bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub description_color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub content: Option<Element<'a>>,
}

impl<'a> GroupBox<'a> {
    /// 创建一个默认无样式的带标题分组框容器。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str) -> Self {
        Self {
            id,
            title,
            description: "",
            width: 300,
            bg: Color::TRANSPARENT,
            header_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            description_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            content: None,
        }
    }

    /// 设置副标题描述。
    #[inline(always)]
    pub const fn description(mut self, description: &'a str) -> Self {
        self.description = description;
        self
    }

    /// 设置容器宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 设置分组框内部子内容。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.content = Some(child.into_element(cx));
        self
    }
}

crate::impl_base_styled_box!(GroupBox<'a>);

impl<'a> Widget<'a> for GroupBox<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };
        let description_color = if self.description_color.a() > 0 {
            self.description_color
        } else {
            fg.scale_alpha(150)
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut header = div()
            .flex_col()
            .w(width - 2)
            .px(12)
            .py(8)
            .bg(self.header_bg)
            .gap(2)
            .child(cx, text(self.title).size(13).color(title_color));

        if !self.description.is_empty() {
            header = header.child(
                cx,
                text(self.description).size(11).color(description_color),
            );
        }

        let mut box_el = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .rounded(self.rounded)
            .bg(self.bg)
            .overflow_hidden()
            .child(cx, header);
        if self.border_color.a() > 0 {
            box_el = box_el.border(1, self.border_color);
        }

        if let Some(body) = self.content {
            box_el = box_el.child(cx, div().w(width).p(12).child(cx, body));
        }

        box_el = self.style_ext.apply_to_div(box_el, self.style);
        box_el.into_element(cx)
    }
}
