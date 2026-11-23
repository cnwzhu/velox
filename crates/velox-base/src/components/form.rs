//! 表单容器与布局基础组件 `Form` 及字段包装器 `FormField`。

use velox_core::prelude::*;

/// 表单排版布局模式（`FormLayout`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FormLayout {
    #[default]
    Vertical,
    Horizontal,
    Inline,
}

/// 表单字段项基础组件（`FormField`，包含标签、必填标记、输入控件槽位、帮助文本与校验错误信息）。
pub struct FormField<'a> {
    pub id: u32,
    pub label: &'a str,
    pub required: bool,
    pub help_text: &'a str,
    pub error_text: &'a str,
    pub layout: FormLayout,
    pub label_width: i16,
    pub label_color: Color,
    pub required_color: Color,
    pub help_color: Color,
    pub error_color: Color,
    pub style: Style,
    pub style_ext: StyleExt,
    pub control: Option<Element<'a>>,
}

impl<'a> FormField<'a> {
    /// 创建一个表单字段容器（未显式设置配色时回退 `Theme` 默认样式）。
    #[inline(always)]
    pub const fn new(id: u32, label: &'a str) -> Self {
        Self {
            id,
            label,
            required: false,
            help_text: "",
            error_text: "",
            layout: FormLayout::Vertical,
            label_width: 100,
            label_color: Color::TRANSPARENT,
            required_color: Color::TRANSPARENT,
            help_color: Color::TRANSPARENT,
            error_color: Color::TRANSPARENT,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            control: None,
        }
    }

    /// 设置是否为必填字段。
    #[inline(always)]
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// 设置底部辅助提示文案。
    #[inline(always)]
    pub const fn help(mut self, help_text: &'a str) -> Self {
        self.help_text = help_text;
        self
    }

    /// 设置校验失败错误文案。
    #[inline(always)]
    pub const fn error(mut self, error_text: &'a str) -> Self {
        self.error_text = error_text;
        self
    }

    /// 设置标签与控件的排列方向。
    #[inline(always)]
    pub const fn layout(mut self, layout: FormLayout) -> Self {
        self.layout = layout;
        self
    }

    /// 设置绑定的表单输入控件。
    #[inline(always)]
    pub fn control(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.control = Some(child.into_element(cx));
        self
    }
}

crate::impl_base_styled_box!(FormField<'a>);

impl<'a> Widget<'a> for FormField<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let label_color = if self.label_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.label_color
        };
        let required_color = if self.required_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.required_color
        };
        let help_color = if self.help_color == Color::TRANSPARENT {
            def_fg.scale_alpha(150)
        } else {
            self.help_color
        };
        let error_color = if self.error_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.error_color
        };

        let mut lbl_row = div()
            .flex_row()
            .gap(4)
            .items_center()
            .child(cx, text(self.label).size(12).color(label_color));
        if self.required {
            lbl_row = lbl_row.child(cx, text("*").size(12).color(required_color));
        }

        let mut ctrl_col = div().flex_col().gap(3);
        if let Some(c) = self.control {
            ctrl_col = ctrl_col.child(cx, c);
        }
        if !self.error_text.is_empty() {
            ctrl_col = ctrl_col.child(cx, text(self.error_text).size(11).color(error_color));
        } else if !self.help_text.is_empty() {
            ctrl_col = ctrl_col.child(cx, text(self.help_text).size(11).color(help_color));
        }

        let root = match self.layout {
            FormLayout::Vertical => div()
                .id(self.id)
                .flex_col()
                .gap(4)
                .child(cx, lbl_row)
                .child(cx, ctrl_col),
            FormLayout::Horizontal | FormLayout::Inline => div()
                .id(self.id)
                .flex_row()
                .gap(10)
                .items_start()
                .child(cx, lbl_row.w(self.label_width))
                .child(cx, ctrl_col),
        };
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}

/// 表单容器与布局基础组件（`Form`）。
pub struct Form<'a> {
    pub id: u32,
    pub title: &'a str,
    pub layout: FormLayout,
    pub width: i16,
    pub submit_label: &'a str,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub submit_bg: Color,
    pub submit_fg: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub body: Option<Element<'a>>,
    pub on_submit: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Form<'a> {
    /// 创建一个表单容器组件（未显式设置配色与圆角时回退 `Theme` 默认样式）。
    #[inline(always)]
    pub const fn new(id: u32, width: i16) -> Self {
        Self {
            id,
            title: "",
            layout: FormLayout::Vertical,
            width,
            submit_label: "Submit",
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            submit_bg: Color::TRANSPARENT,
            submit_fg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            body: None,
            on_submit: None,
        }
    }

    /// 设置表单标题。
    #[inline(always)]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// 设置表单布局模式。
    #[inline(always)]
    pub const fn layout(mut self, layout: FormLayout) -> Self {
        self.layout = layout;
        self
    }

    /// 设置表单内部字段集合。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.body = Some(child.into_element(cx));
        self
    }

    /// 绑定点击提交按钮回调。
    #[inline(always)]
    pub fn on_submit(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_submit = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Form<'a>);

impl<'a> Widget<'a> for Form<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let title_color = if self.title_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.title_color
        };
        let submit_bg = self.submit_bg;
        let submit_fg = if self.submit_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.submit_fg
        };
        let rounded = self.rounded;

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut root = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .p(14)
            .gap(12)
            .rounded(rounded)
            .bg(bg);
        if border_color != Color::TRANSPARENT {
            root = root.border(1, border_color);
        }

        if !self.title.is_empty() {
            root = root.child(cx, text(self.title).size(15).color(title_color));
        }

        if let Some(b) = self.body {
            root = root.child(cx, b);
        }

        if let Some(cb) = self.on_submit {
            let submit_btn = div()
                .id(crate::scoped_child_id(self.id, 0x4058_C747, 1))
                .px(14)
                .py(6)
                .rounded(rounded.saturating_sub(2))
                .bg(submit_bg)
                .focusable()
                .no_focus_border()
                .on_click_raw(cb)
                .child(cx, text(self.submit_label).size(12).color(submit_fg));
            root = root.child(cx, div().flex_row().justify_end().child(cx, submit_btn));
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
