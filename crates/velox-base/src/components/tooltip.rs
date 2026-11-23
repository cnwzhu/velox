//! 悬停文字提示浮层基础组件 `Tooltip`（默认无样式）。

use velox_core::prelude::*;

/// 轻量级文字提示浮层基础组件（`Tooltip`，默认无样式）。
pub struct Tooltip<'a> {
    pub id: u32,
    pub content: &'a str,
    pub visible: bool,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub style: Style,
    pub style_ext: StyleExt,
    pub child: Option<Element<'a>>,
    pub popup_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Tooltip<'a> {
    /// 创建一个默认无样式的文字提示浮层。
    #[inline(always)]
    pub const fn new(id: u32, content: &'a str, visible: bool) -> Self {
        Self {
            id,
            content,
            visible,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            rounded: 0,
            shadow: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            child: None,
            popup_renderer: None,
        }
    }

    /// 设置触发宿主子元素。
    #[inline(always)]
    pub fn trigger(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.child = Some(child.into_element(cx));
        self
    }

    /// `gpui-base` 风格气泡浮层插槽回调：自定义提示气泡容器外观。
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

crate::impl_base_styled_box!(Tooltip<'a>);

impl<'a> Widget<'a> for Tooltip<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            cx.theme().foreground
        };

        let has_trigger = self.child.is_some();
        let mut wrap = div().id(self.id).flex_col();
        if let Some(c) = self.child {
            wrap = wrap.child(cx, c);
        }
        if self.visible && !self.content.is_empty() {
            let offset_y = if has_trigger { 32 } else { 0 };
            let mut tip = div()
                .id(self.id ^ 0x5454_4950)
                .pos(0, offset_y)
                .overlay()
                .px(8)
                .py(4)
                .rounded(self.rounded)
                .bg(self.bg)
                .child(cx, text(self.content).size(11).color(text_color).nowrap());
            if self.border_color.a() > 0 {
                tip = tip.border(1, self.border_color);
            }
            if let Some((blur, ox, oy, col)) = self.shadow {
                tip = tip.shadow(blur, ox, oy, col);
            }
            if let Some(renderer) = self.popup_renderer {
                wrap = wrap.child(cx, renderer(tip, cx));
            } else {
                wrap = wrap.child(cx, tip);
            }
        }
        wrap = self.style_ext.apply_to_div(wrap, self.style);
        wrap.into_element(cx)
    }
}
