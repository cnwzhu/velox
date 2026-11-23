//! 无样式内联警告提示原语 `Alert` 与语义变体 `AlertVariant`（默认零背景、零边框、零圆角与零语义底色）。

use velox_core::prelude::*;

/// 警告与通知语义等级变体（`AlertVariant`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlertVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl AlertVariant {
    /// 获取对应变体的图标符号。
    #[inline(always)]
    pub const fn icon_str(self) -> &'static str {
        match self {
            Self::Info => "i",
            Self::Success => "✓",
            Self::Warning => "!",
            Self::Error => "✕",
        }
    }

    /// 获取对应变体的参考色板 `(背景色, 边框色, 标题色, 图标符号)`。
    #[inline(always)]
    pub const fn palette(self) -> (Color, Color, Color, &'static str) {
        match self {
            Self::Info => (
                Color::rgb(15, 23, 42),
                Color::rgb(59, 130, 246),
                Color::rgb(147, 197, 253),
                "i",
            ),
            Self::Success => (
                Color::rgb(15, 34, 28),
                Color::rgb(34, 197, 94),
                Color::rgb(134, 239, 172),
                "✓",
            ),
            Self::Warning => (
                Color::rgb(38, 28, 15),
                Color::rgb(234, 179, 8),
                Color::rgb(253, 224, 71),
                "!",
            ),
            Self::Error => (
                Color::rgb(42, 18, 21),
                Color::rgb(239, 68, 68),
                Color::rgb(252, 165, 165),
                "✕",
            ),
        }
    }
}

/// 无样式内联警告提示原语（`Alert`）。
pub struct Alert<'a> {
    pub id: u32,
    pub title: &'a str,
    pub message: &'a str,
    pub variant: AlertVariant,
    pub width: i16,
    pub closable: bool,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub text_color: Color,
    pub close_color: Color,
    pub close_hover_bg: Option<Color>,
    pub close_active_bg: Option<Color>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_close: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Alert<'a> {
    /// 创建一个无样式内联警告提示原语。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, message: &'a str) -> Self {
        Self {
            id,
            title,
            message,
            variant: AlertVariant::Info,
            width: 340,
            closable: false,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            close_color: Color::TRANSPARENT,
            close_hover_bg: None,
            close_active_bg: None,
            focus_ring: false,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_close: None,
        }
    }

    /// 设置警告语义变体（`Info`、`Success`、`Warning`、`Error`）。
    #[inline(always)]
    pub const fn variant(mut self, variant: AlertVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 设置宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定关闭按钮回调并启用关闭按钮。
    #[inline(always)]
    pub fn on_close(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.closable = true;
        self.on_close = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Alert<'a>);

impl<'a> Widget<'a> for Alert<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let icon_d = match self.variant {
            AlertVariant::Info => "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM12 16v-4M12 8h.01",
            AlertVariant::Success => "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM9 12l2 2 4-4",
            AlertVariant::Warning => {
                "M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0ZM12 9v4M12 17h.01"
            }
            AlertVariant::Error => "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM15 9l-6 6M9 9l6 6",
        };
        let title_color = if self.title_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.title_color
        };
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let close_color = if self.close_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.close_color
        };

        let icon_el = div()
            .w(20)
            .h(20)
            .items_center()
            .justify_center()
            .child(cx, svg_icon(16, 16, icon_d).tint(title_color).stroke_width(20));

        let mut text_col = div()
            .flex_col()
            .gap(3)
            .child(cx, text(self.title).size(13).color(title_color));
        if !self.message.is_empty() {
            text_col = text_col.child(cx, text(self.message).size(12).color(text_color));
        }

        let mut left_part = div()
            .flex_row()
            .gap(10)
            .items_center()
            .child(cx, icon_el)
            .child(cx, text_col);

        let width = StyleExt::resolve_width(self.style, self.width);
        if !self.closable {
            let mut root = left_part
                .id(self.id)
                .w(width)
                .p(12)
                .rounded(self.rounded)
                .bg(self.bg);
            if self.border_color != Color::TRANSPARENT {
                root = root.border(1, self.border_color);
            }
            root = self.style_ext.apply_to_div(root, self.style);
            return root.into_element(cx);
        }

        let mut close_btn = div()
            .id(crate::scoped_child_id(self.id, 0xB7C3_58F9, 1))
            .w(22)
            .h(22)
            .rounded(if self.rounded > 0 {
                self.rounded.saturating_sub(4).max(4)
            } else {
                0
            })
            .items_center()
            .justify_center()
            .focusable()
            .child(
                cx,
                svg_icon(12, 12, "M18 6 6 18M6 6l12 12")
                    .tint(close_color)
                    .stroke_width(20),
            );
        if !self.focus_ring {
            close_btn = close_btn.no_focus_border();
        }
        if let Some(h) = self.close_hover_bg {
            close_btn = close_btn.hover_bg(h);
        }
        if let Some(a) = self.close_active_bg {
            close_btn = close_btn.active_bg(a);
        }
        if let Some(cb) = self.on_close {
            close_btn = close_btn.on_click_raw(cb);
        }

        left_part = left_part.w((width - 52).max(40));

        let mut root = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .p(12)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .justify_between()
            .child(cx, left_part)
            .child(cx, close_btn);
        if self.border_color != Color::TRANSPARENT {
            root = root.border(1, self.border_color);
        }
        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
