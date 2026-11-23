//! 无样式基础徽章原语 `Badge`（默认零背景、零边框、零圆角、零内边距，支持全套 `Styled` 链式定制）。

use velox_engine::prelude::*;

/// 无样式状态徽章原语（`Badge`）。
///
/// 默认不注入背景色、边框、圆角或内边距；语义变体与主题外观由 `velox-component::Badge` 提供。
pub struct Badge<'a> {
    pub label: StrRef<'a>,
    pub font_size: u8,
    pub text_color: Color,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Badge<'a> {
    /// 创建一个无样式基础徽章原语。
    #[inline(always)]
    pub fn new(label: impl Into<StrRef<'a>>) -> Self {
        let mut style = Style::default();
        style.set_row();
        style.set_items_center();
        style.set_justify_center();
        Self {
            label: label.into(),
            font_size: 0,
            text_color: Color::TRANSPARENT,
            style,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置文本字号（默认 `12`）。
    #[inline(always)]
    pub const fn font_size(mut self, size: u8) -> Self {
        self.font_size = size;
        self
    }

    /// 设置文本颜色（未设置时仅继承 `cx.theme().foreground`）。
    #[inline(always)]
    pub const fn text_color(mut self, color: Color) -> Self {
        self.text_color = color;
        self
    }
}

crate::impl_base_styled_box!(Badge<'a>);

impl<'a> Widget<'a> for Badge<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let font_sz = if self.font_size != 0 {
            self.font_size
        } else {
            self.style_ext.text_size.unwrap_or(12)
        };
        let text_col = if self.text_color != Color::TRANSPARENT {
            self.text_color
        } else {
            self.style_ext.text_color.unwrap_or(cx.theme().foreground)
        };

        let mut d = div();
        d.style = self.style;
        d = self.style_ext.apply_ext_only(d);
        d.child(
            cx,
            text(self.label)
                .size(font_sz)
                .color(text_col)
                .text_center()
                .nowrap(),
        )
        .into_element(cx)
    }
}
