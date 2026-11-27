//! 顶层浮动通知卡片基础组件 `Notification`（默认无样式）。

use crate::components::AlertVariant;
use velox_engine::prelude::*;

/// 通知语义变体别名（与 [`AlertVariant`] 等价）。
pub type NotificationVariant = AlertVariant;

/// 顶层浮动通知卡片基础组件（`Notification`，默认无样式，支持标题、正文、语义图标、操作按钮与关闭回调）。
pub struct Notification<'a> {
    pub id: u32,
    pub title: &'a str,
    pub message: &'a str,
    pub variant: NotificationVariant,
    pub action_label: Option<&'a str>,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub text_color: Color,
    pub accent_color: Color,
    pub action_bg: Color,
    pub action_rounded: u8,
    pub action_hover_bg: Option<Color>,
    pub action_active_bg: Option<Color>,
    pub close_color: Color,
    pub close_rounded: u8,
    pub close_hover_bg: Option<Color>,
    pub close_active_bg: Option<Color>,
    pub rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub animated: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_action: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_dismiss: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Notification<'a> {
    /// 创建一个默认无样式的通知卡片组件。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, message: &'a str) -> Self {
        Self {
            id,
            title,
            message,
            variant: NotificationVariant::Info,
            action_label: None,
            width: 300,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            accent_color: Color::TRANSPARENT,
            action_bg: Color::TRANSPARENT,
            action_rounded: 0,
            action_hover_bg: None,
            action_active_bg: None,
            close_color: Color::TRANSPARENT,
            close_rounded: 0,
            close_hover_bg: None,
            close_active_bg: None,
            rounded: 0,
            shadow: None,
            focus_ring: false,
            animated: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_action: None,
            on_dismiss: None,
        }
    }

    /// 设置通知语义变体标记。
    #[inline(always)]
    pub const fn variant(mut self, variant: NotificationVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 启用或禁用滑入淡入入场动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 设置可选的操作按钮文案与回调。
    #[inline(always)]
    pub fn action(mut self, label: &'a str, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.action_label = Some(label);
        self.on_action = Some(handler);
        self
    }

    /// 绑定关闭通知回调。
    #[inline(always)]
    pub fn on_dismiss(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_dismiss = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Notification<'a>);

impl<'a> Widget<'a> for Notification<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let icon_d = match self.variant {
            NotificationVariant::Info => {
                "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM12 16v-4M12 8h.01"
            }
            NotificationVariant::Success => {
                "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM9 12l2 2 4-4"
            }
            NotificationVariant::Warning => {
                "M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0ZM12 9v4M12 17h.01"
            }
            NotificationVariant::Error => {
                "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM15 9l-6 6M9 9l6 6"
            }
        };
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let accent_color = if self.accent_color.a() > 0 {
            self.accent_color
        } else {
            fg
        };
        let close_color = if self.close_color.a() > 0 {
            self.close_color
        } else {
            fg
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut header = div()
            .flex_row()
            .w(width - 24)
            .items_center()
            .justify_between()
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(8)
                    .items_center()
                    .child(
                        cx,
                        svg_icon(15, 15, icon_d)
                            .tint(accent_color)
                            .stroke_width(20),
                    )
                    .child(cx, text(self.title).size(13).color(title_color)),
            );

        let mut close_btn = div()
            .id(crate::scoped_child_id(self.id, 0xEA3D_3046, 1))
            .w(20)
            .h(20)
            .rounded(self.close_rounded)
            .items_center()
            .justify_center()
            .focusable();
        if let Some(h) = self.close_hover_bg {
            close_btn = close_btn.hover_bg(h);
        }
        if let Some(a) = self.close_active_bg {
            close_btn = close_btn.active_bg(a);
        }
        if !self.focus_ring {
            close_btn = close_btn.no_focus_border();
        }
        close_btn = close_btn.child(
            cx,
            svg_icon(12, 12, "M18 6 6 18M6 6l12 12")
                .tint(close_color)
                .stroke_width(20),
        );
        if let Some(cb) = self.on_dismiss {
            close_btn = close_btn.on_click_raw(cb);
        }
        header = header.child(cx, close_btn);

        let mut card = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .p(12)
            .gap(6)
            .rounded(self.rounded)
            .bg(self.bg);
        if self.border_color.a() > 0 {
            card = card.border(1, self.border_color);
        }
        if let Some((blur, ox, oy, col)) = self.shadow {
            card = card.shadow(blur, ox, oy, col);
        }
        card = card
            .child(cx, header)
            .child(cx, text(self.message).size(12).color(text_color));

        if let Some(act_lbl) = self.action_label {
            let mut act_btn = div()
                .id(crate::scoped_child_id(self.id, 0xEA3D_3046, 2))
                .px(10)
                .py(4)
                .rounded(self.action_rounded)
                .bg(self.action_bg)
                .focusable();
            if let Some(h) = self.action_hover_bg {
                act_btn = act_btn.hover_bg(h);
            }
            if let Some(a) = self.action_active_bg {
                act_btn = act_btn.active_bg(a);
            }
            if self.border_color.a() > 0 {
                act_btn = act_btn.border(1, self.border_color);
            }
            if !self.focus_ring {
                act_btn = act_btn.no_focus_border();
            }
            act_btn = act_btn.child(
                cx,
                text(act_lbl)
                    .size(11)
                    .color(accent_color)
                    .text_center()
                    .nowrap(),
            );
            if let Some(cb) = self.on_action {
                act_btn = act_btn.on_click_raw(cb);
            }
            card = card.child(cx, div().flex_row().justify_end().child(cx, act_btn));
        }

        if self.animated {
            card = card
                .enter_from(255, 0, -6, Duration::from_millis(210))
                .ease(Easing::Spring)
                .exit_to(0, 28, 0, Duration::from_millis(160));
        }

        card = self.style_ext.apply_to_div(card, self.style);
        card.into_element(cx)
    }
}
