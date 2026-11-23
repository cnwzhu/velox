//! 键值对详情展示列表基础组件 `DescriptionList`。

use velox_core::prelude::*;

/// 键值对描述列表条目（`DescriptionItem`）。
#[derive(Clone, Copy, Debug)]
pub struct DescriptionItem<'a> {
    pub label: &'a str,
    pub value: &'a str,
}

impl<'a> DescriptionItem<'a> {
    /// 创建一条键值对描述项。
    #[inline(always)]
    pub const fn new(label: &'a str, value: &'a str) -> Self {
        Self { label, value }
    }
}

/// 键值对详情展示列表基础组件（`DescriptionList`）。
pub struct DescriptionList<'a> {
    pub id: u32,
    pub title: &'a str,
    pub items: &'a [DescriptionItem<'a>],
    pub width: i16,
    pub label_width: i16,
    pub bordered: bool,
    pub bg: Color,
    pub alt_bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub label_color: Color,
    pub value_color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> DescriptionList<'a> {
    /// 创建一个键值对描述列表（未显式设置配色与圆角时回退 `Theme` 默认样式）。
    #[inline(always)]
    pub const fn new(id: u32, items: &'a [DescriptionItem<'a>]) -> Self {
        Self {
            id,
            title: "",
            items,
            width: 320,
            label_width: 110,
            bordered: true,
            bg: Color::TRANSPARENT,
            alt_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            label_color: Color::TRANSPARENT,
            value_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置描述列表标题。
    #[inline(always)]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// 设置总宽度与键名列宽度。
    #[inline(always)]
    pub const fn widths(mut self, width: i16, label_width: i16) -> Self {
        self.width = width;
        self.label_width = label_width;
        self
    }

    /// 设置是否显示网格边框。
    #[inline(always)]
    pub const fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }
}

crate::impl_base_styled_box!(DescriptionList<'a>);

impl<'a> Widget<'a> for DescriptionList<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };
        let label_color = if self.label_color.a() > 0 {
            self.label_color
        } else {
            fg.scale_alpha(150)
        };
        let value_color = if self.value_color.a() > 0 {
            self.value_color
        } else {
            fg
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut root = div().id(self.id).flex_col().w(width).gap(4);

        if !self.title.is_empty() {
            root = root.child(cx, text(self.title).size(14).color(title_color));
        }

        let mut box_col = div()
            .flex_col()
            .w(width)
            .rounded(self.rounded)
            .overflow_hidden();
        if self.bordered && self.border_color.a() > 0 {
            box_col = box_col.border(1, self.border_color);
        }

        let val_w = (width - self.label_width).max(60);
        for (idx, item) in self.items.iter().enumerate() {
            let row_bg = if idx % 2 == 0 { self.bg } else { self.alt_bg };
            let row = div()
                .flex_row()
                .w(width)
                .h(28)
                .bg(row_bg)
                .items_center()
                .child(
                    cx,
                    div()
                        .w(self.label_width)
                        .px(10)
                        .items_center()
                        .child(
                            cx,
                            text(item.label)
                                .size(12)
                                .color(label_color)
                                .ellipsis(),
                        ),
                )
                .child(
                    cx,
                    div()
                        .w(val_w)
                        .px(10)
                        .items_center()
                        .child(
                            cx,
                            text(item.value)
                                .size(12)
                                .color(value_color)
                                .ellipsis(),
                        ),
                );
            box_col = box_col.child(cx, row);
        }

        root = root.child(cx, box_col);
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
