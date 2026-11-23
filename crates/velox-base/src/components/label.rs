//! 无样式表单文本标签原语 `Label`（默认仅继承 `foreground` 文本色，不注入彩色主题星号或禁用灰）。

use velox_core::prelude::*;

/// 无样式表单字段文本标签原语（`Label`，支持必填星号标记与辅助说明文案）。
pub struct Label<'a> {
    pub id: u32,
    pub text: &'a str,
    pub description: &'a str,
    pub required: bool,
    pub disabled: bool,
    pub text_color: Color,
    pub disabled_color: Color,
    pub description_color: Color,
    pub required_color: Color,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Label<'a> {
    /// 创建一个无样式表单标签原语。
    #[inline(always)]
    pub const fn new(id: u32, text: &'a str) -> Self {
        Self {
            id,
            text,
            description: "",
            required: false,
            disabled: false,
            text_color: Color::TRANSPARENT,
            disabled_color: Color::TRANSPARENT,
            description_color: Color::TRANSPARENT,
            required_color: Color::TRANSPARENT,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置是否显示必填星号 `*`。
    #[inline(always)]
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// 设置字段下方的辅助说明描述文本。
    #[inline(always)]
    pub const fn description(mut self, desc: &'a str) -> Self {
        self.description = desc;
        self
    }
}

crate::impl_base_styled_box!(Label<'a>);
crate::impl_base_disableable!(Label<'a>);

impl<'a> Widget<'a> for Label<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let disabled_color = if self.disabled_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.disabled_color
        };
        let description_color = if self.description_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.description_color
        };
        let required_color = if self.required_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.required_color
        };

        let fg = if self.disabled {
            disabled_color
        } else {
            text_color
        };
        let mut main_row = div()
            .flex_row()
            .gap(4)
            .items_center()
            .child(cx, text(self.text).size(13).color(fg));

        if self.required {
            main_row = main_row.child(cx, text("*").size(13).color(required_color));
        }

        if self.disabled && self.disabled_color == Color::TRANSPARENT {
            main_row = main_row.opacity(115);
        }

        if self.description.is_empty() {
            let root = self.style_ext.apply_to_div(main_row.id(self.id), self.style);
            return root.into_element(cx);
        }

        let root = div()
            .id(self.id)
            .flex_col()
            .gap(2)
            .child(cx, main_row)
            .child(
                cx,
                text(self.description)
                    .size(11)
                    .color(description_color),
            );
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
