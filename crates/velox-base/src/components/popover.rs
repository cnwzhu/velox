//! 弹出浮层容器基础组件 `Popover`（默认无样式）。

use velox_core::prelude::*;

/// 弹出卡片容器基础组件（`Popover`，默认无样式）。
pub struct Popover<'a> {
    pub id: u32,
    pub open: bool,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub style: Style,
    pub style_ext: StyleExt,
    pub trigger: Option<Element<'a>>,
    pub content: Option<Element<'a>>,
    pub on_close: Option<&'a dyn Fn(&mut ActionScope)>,
    pub popup_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Popover<'a> {
    /// 创建一个默认无样式的弹出面板。
    #[inline(always)]
    pub const fn new(id: u32, open: bool) -> Self {
        Self {
            id,
            open,
            width: 220,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            rounded: 0,
            shadow: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            trigger: None,
            content: None,
            on_close: None,
            popup_renderer: None,
        }
    }

    /// 设置触发元素。
    #[inline(always)]
    pub fn trigger(mut self, cx: &'a FrameScope, el: impl IntoElement<'a>) -> Self {
        self.trigger = Some(el.into_element(cx));
        self
    }

    /// 设置弹出层内部内容。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, el: impl IntoElement<'a>) -> Self {
        self.content = Some(el.into_element(cx));
        self
    }

    /// 绑定关闭弹出层回调（响应 `Escape` / 返回键）。
    #[inline(always)]
    pub fn on_close(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_close = Some(handler);
        self
    }

    /// `gpui-base` 风格浮层卡片插槽回调：自定义弹出面板容器外观。
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

crate::impl_base_styled_box!(Popover<'a>);

impl<'a> Widget<'a> for Popover<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let has_trigger = self.trigger.is_some();
        let mut wrap = div().id(self.id).flex_col();
        if let Some(t) = self.trigger {
            wrap = wrap.child(cx, t);
        }
        if self.open {
            if let Some(body) = self.content {
                let offset_y = if has_trigger { 34 } else { 0 };
                let mut panel = div()
                    .id(self.id ^ 0x504F_5056)
                    .pos(0, offset_y)
                    .overlay()
                    .occlude_hit()
                    .w(self.width)
                    .p(10)
                    .rounded(self.rounded)
                    .bg(self.bg);
                if self.border_color.a() > 0 {
                    panel = panel.border(1, self.border_color);
                }
                if let Some((blur, ox, oy, col)) = self.shadow {
                    panel = panel.shadow(blur, ox, oy, col);
                }
                panel = panel.child(cx, body);
                if let Some(cb) = self.on_close {
                    panel = panel.on_navigate_back(cb);
                }
                if let Some(renderer) = self.popup_renderer {
                    wrap = wrap.child(cx, renderer(panel, cx));
                } else {
                    wrap = wrap.child(cx, panel);
                }
            }
        }
        wrap = self.style_ext.apply_to_div(wrap, self.style);
        wrap.into_element(cx)
    }
}
