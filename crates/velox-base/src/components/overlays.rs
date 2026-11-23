//! 浮层提示基础组件：`HoverCard`、确认对话框 `AlertDialog` 与相关重导出（默认无样式）。

pub use crate::components::avatar::Link;
pub use crate::components::popover::Popover;
pub use crate::components::scrollable::{Scrollbar, ScrollbarAxis};
pub use crate::components::tooltip::Tooltip;
use velox_engine::prelude::*;

/// 悬停详情卡片基础组件（`HoverCard`，默认无样式）。
pub struct HoverCard<'a> {
    pub id: u32,
    pub title: &'a str,
    pub description: &'a str,
    pub open: bool,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub description_color: Color,
    pub rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub style: Style,
    pub style_ext: StyleExt,
    pub trigger: Option<Element<'a>>,
    pub popup_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> HoverCard<'a> {
    /// 创建一个默认无样式的悬停详情卡片。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, description: &'a str, open: bool) -> Self {
        Self {
            id,
            title,
            description,
            open,
            width: 240,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            description_color: Color::TRANSPARENT,
            rounded: 0,
            shadow: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            trigger: None,
            popup_renderer: None,
        }
    }

    /// 设置触发元素。
    #[inline(always)]
    pub fn trigger(mut self, cx: &'a FrameScope, el: impl IntoElement<'a>) -> Self {
        self.trigger = Some(el.into_element(cx));
        self
    }

    /// `gpui-base` 风格弹出卡片插槽回调：自定义悬停详情浮层容器外观。
    #[inline(always)]
    pub fn popup<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(move |el: Div<'a>, scope: &'a FrameScope| {
            f(el, scope).into_element(scope)
        });
        self.popup_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(HoverCard<'a>);

impl<'a> Widget<'a> for HoverCard<'a> {
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
            fg
        };

        let has_trigger = self.trigger.is_some();
        let mut wrap = div().id(self.id).flex_col();
        if let Some(t) = self.trigger {
            wrap = wrap.child(cx, t);
        }
        if self.open {
            let offset_y = if has_trigger { 28 } else { 0 };
            let mut card = div()
                .id(self.id ^ 0x4856_4344)
                .pos(0, offset_y)
                .overlay()
                .occlude_hit()
                .flex_col()
                .w(self.width)
                .p(12)
                .gap(4)
                .rounded(self.rounded)
                .bg(self.bg);
            if self.border_color.a() > 0 {
                card = card.border(1, self.border_color);
            }
            if let Some((blur, ox, oy, col)) = self.shadow {
                card = card.shadow(blur, ox, oy, col);
            }
            card = card
                .child(cx, text(self.title).size(13).color(title_color))
                .child(cx, text(self.description).size(12).color(description_color));
            if let Some(renderer) = self.popup_renderer {
                wrap = wrap.child(cx, renderer(card, cx));
            } else {
                wrap = wrap.child(cx, card);
            }
        }
        wrap = self.style_ext.apply_to_div(wrap, self.style);
        wrap.into_element(cx)
    }
}

/// 模态确认对话框基础组件（`AlertDialog`，默认无样式）。
pub struct AlertDialog<'a> {
    pub id: u32,
    pub title: &'a str,
    pub description: &'a str,
    pub confirm_label: &'a str,
    pub cancel_label: &'a str,
    pub destructive: bool,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub description_color: Color,
    pub cancel_bg: Color,
    pub cancel_border_color: Color,
    pub cancel_fg: Color,
    pub cancel_hover_bg: Option<Color>,
    pub cancel_active_bg: Option<Color>,
    pub confirm_bg: Color,
    pub confirm_fg: Color,
    pub confirm_hover_bg: Option<Color>,
    pub confirm_active_bg: Option<Color>,
    pub confirm_focus_border: Option<(u8, Color)>,
    pub rounded: u8,
    pub btn_rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_confirm: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_cancel: Option<&'a dyn Fn(&mut ActionScope)>,
    pub backdrop_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> AlertDialog<'a> {
    /// 创建一个默认无样式的确认对话框。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, description: &'a str) -> Self {
        Self {
            id,
            title,
            description,
            confirm_label: "Confirm",
            cancel_label: "Cancel",
            destructive: false,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            description_color: Color::TRANSPARENT,
            cancel_bg: Color::TRANSPARENT,
            cancel_border_color: Color::TRANSPARENT,
            cancel_fg: Color::TRANSPARENT,
            cancel_hover_bg: None,
            cancel_active_bg: None,
            confirm_bg: Color::TRANSPARENT,
            confirm_fg: Color::TRANSPARENT,
            confirm_hover_bg: None,
            confirm_active_bg: None,
            confirm_focus_border: None,
            rounded: 0,
            btn_rounded: 0,
            shadow: None,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_confirm: None,
            on_cancel: None,
            backdrop_renderer: None,
        }
    }

    /// 设置为危险破坏性操作标记。
    #[inline(always)]
    pub const fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// 绑定确认按钮回调。
    #[inline(always)]
    pub fn on_confirm(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_confirm = Some(handler);
        self
    }

    /// 绑定取消按钮回调。
    #[inline(always)]
    pub fn on_cancel(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_cancel = Some(handler);
        self
    }

    /// `gpui-base` 风格背景遮罩插槽回调。
    #[inline(always)]
    pub fn backdrop<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(move |el: Div<'a>, scope: &'a FrameScope| {
            f(el, scope).into_element(scope)
        });
        self.backdrop_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(AlertDialog<'a>);

impl<'a> Widget<'a> for AlertDialog<'a> {
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
            fg
        };
        let cancel_fg = if self.cancel_fg.a() > 0 {
            self.cancel_fg
        } else {
            fg
        };
        let confirm_fg = if self.confirm_fg.a() > 0 {
            self.confirm_fg
        } else {
            fg
        };

        let cancel_text = if self.cancel_label == "Cancel" {
            crate::i18n::widget_t(cx, "velox.common.cancel")
        } else {
            StrRef::from(self.cancel_label)
        };
        let confirm_text = if self.confirm_label == "Confirm" {
            crate::i18n::widget_t(cx, "velox.common.confirm")
        } else {
            StrRef::from(self.confirm_label)
        };

        let mut cancel_btn = div()
            .id(crate::scoped_child_id(self.id, 0x704E_FF50, 1))
            .px(12)
            .py(6)
            .rounded(self.btn_rounded)
            .bg(self.cancel_bg)
            .focusable();
        if let Some(h) = self.cancel_hover_bg {
            cancel_btn = cancel_btn.hover_bg(h);
        }
        if let Some(a) = self.cancel_active_bg {
            cancel_btn = cancel_btn.active_bg(a);
        }
        if self.cancel_border_color.a() > 0 {
            cancel_btn = cancel_btn.border(1, self.cancel_border_color);
        }
        if !self.focus_ring {
            cancel_btn = cancel_btn.no_focus_border();
        }
        cancel_btn = cancel_btn.child(
            cx,
            text(cancel_text)
                .size(12)
                .color(cancel_fg)
                .text_center()
                .nowrap(),
        );
        if let Some(cb) = self.on_cancel {
            cancel_btn = cancel_btn.on_click_raw(cb);
        }

        let mut confirm_btn = div()
            .id(crate::scoped_child_id(self.id, 0x704E_FF50, 2))
            .px(12)
            .py(6)
            .rounded(self.btn_rounded)
            .bg(self.confirm_bg)
            .focusable();
        if let Some(h) = self.confirm_hover_bg {
            confirm_btn = confirm_btn.hover_bg(h);
        }
        if let Some(a) = self.confirm_active_bg {
            confirm_btn = confirm_btn.active_bg(a);
        }
        if let Some((w, col)) = self.confirm_focus_border {
            confirm_btn = confirm_btn.focus_border(w, col);
        } else if !self.focus_ring {
            confirm_btn = confirm_btn.no_focus_border();
        }
        confirm_btn = confirm_btn.child(
            cx,
            text(confirm_text)
                .size(12)
                .color(confirm_fg)
                .text_center()
                .nowrap(),
        );
        if let Some(cb) = self.on_confirm {
            confirm_btn = confirm_btn.on_click_raw(cb);
        }

        let mut panel = div()
            .id(self.id)
            .occlude_hit()
            .flex_col()
            .w(320)
            .p(16)
            .gap(12)
            .rounded(self.rounded)
            .bg(self.bg);
        if self.backdrop_renderer.is_none() {
            panel = panel.overlay();
        }
        if self.border_color.a() > 0 {
            panel = panel.border(1, self.border_color);
        }
        if let Some((blur, ox, oy, col)) = self.shadow {
            panel = panel.shadow(blur, ox, oy, col);
        }
        panel = panel
            .child(cx, text(self.title).size(15).color(title_color))
            .child(cx, text(self.description).size(12).color(description_color))
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(8)
                    .justify_end()
                    .child(cx, cancel_btn)
                    .child(cx, confirm_btn),
            );
        if let Some(cb) = self.on_cancel {
            panel = panel.on_navigate_back(cb);
        }
        panel = self.style_ext.apply_to_div(panel, self.style);
        if let Some(renderer) = self.backdrop_renderer {
            let mut backdrop = div()
                .pos(0, 0)
                .size_full()
                .overlay()
                .occlude_hit()
                .flex_col()
                .items_center()
                .justify_center()
                .child(cx, panel);
            if let Some(cb) = self.on_cancel {
                backdrop = backdrop.on_click_raw(cb).on_navigate_back(cb);
            }
            return renderer(backdrop, cx);
        }
        panel.into_element(cx)
    }
}
