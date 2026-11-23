//! 无样式头像原语 `Avatar` 与超链接行为原语 `Link`。

use crate::traits::ComponentSize;
use velox_core::prelude::*;

/// 头像在线状态语义枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AvatarStatus {
    #[default]
    None,
    Online,
    Busy,
    Away,
    Offline,
}

/// 无样式头像原语（`Avatar`，默认透明底、0 圆角、不注入彩色主题状态框）。
pub struct Avatar<'a> {
    pub id: u32,
    pub initials: &'a str,
    pub size: ComponentSize,
    pub status: AvatarStatus,
    pub bg: Color,
    pub fg: Color,
    pub border_color: Color,
    pub status_color: Option<Color>,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Avatar<'a> {
    /// 使用姓名首字母缩写创建无样式头像原语。
    #[inline(always)]
    pub const fn new(id: u32, initials: &'a str) -> Self {
        Self {
            id,
            initials,
            size: ComponentSize::Medium,
            status: AvatarStatus::None,
            bg: Color::TRANSPARENT,
            fg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            status_color: None,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置在线状态标记。
    #[inline(always)]
    pub const fn status(mut self, status: AvatarStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置背景色。
    #[inline(always)]
    pub const fn bg(mut self, bg: Color) -> Self {
        self.bg = bg;
        self
    }
}

crate::impl_base_styled_box!(Avatar<'a>);
crate::impl_base_sizable!(Avatar<'a>);

impl<'a> Widget<'a> for Avatar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let (dim, font_sz) = match self.size {
            ComponentSize::Small => (28i16, 11u8),
            ComponentSize::Medium => (36i16, 13u8),
            ComponentSize::Large => (46i16, 16u8),
        };
        let fg = if self.fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.fg
        };

        let mut frame = div()
            .id(self.id)
            .w(dim)
            .h(dim)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .justify_center()
            .child(cx, text(self.initials).size(font_sz).color(fg));

        if let Some(status_col) = self.status_color {
            frame = frame.border(2, status_col);
        } else if self.border_color != Color::TRANSPARENT {
            frame = frame.border(1, self.border_color);
        }

        frame = self.style_ext.apply_to_div(frame, self.style);
        frame.into_element(cx)
    }
}

/// 无样式可点击超链接行为原语（`Link`，默认纯文本与点击/焦点行为，不注入主题蓝与默认边框）。
pub struct Link<'a> {
    pub id: u32,
    pub label: &'a str,
    pub href: &'a str,
    pub disabled: bool,
    pub color: Color,
    pub disabled_color: Color,
    pub underline: bool,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_click: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Link<'a> {
    /// 创建一个无样式超链接原语。
    #[inline(always)]
    pub const fn new(id: u32, label: &'a str) -> Self {
        Self {
            id,
            label,
            href: "",
            disabled: false,
            color: Color::TRANSPARENT,
            disabled_color: Color::TRANSPARENT,
            underline: false,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_click: None,
        }
    }

    /// 设置目标链接标识。
    #[inline(always)]
    pub const fn href(mut self, href: &'a str) -> Self {
        self.href = href;
        self
    }

    /// 绑定点击回调。
    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Link<'a>);
crate::impl_base_disableable!(Link<'a>);

impl<'a> Widget<'a> for Link<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let active_col = if self.color == Color::TRANSPARENT {
            def_fg
        } else {
            self.color
        };
        let dis_col = if self.disabled_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.disabled_color
        };
        let fg = if self.disabled { dis_col } else { active_col };

        let mut d = div()
            .id(self.id)
            .flex_col()
            .child(cx, text(self.label).size(13).color(fg));

        if self.underline {
            d = d.border(1, fg);
        }

        if self.disabled {
            d = d.opacity(115).cursor(CursorShape::NotAllowed);
        } else {
            d = d.focusable();
            if !self.focus_ring {
                d = d.no_focus_border();
            }
            if let Some(cb) = self.on_click {
                d = d.on_click_raw(cb);
            }
        }

        d = self.style_ext.apply_to_div(d, self.style);
        d.into_element(cx)
    }
}
