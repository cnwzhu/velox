//! 无样式模态对话框叠层原语 `Modal`（定义顶层遮罩、命中遮挡、居中容器、点击外部与 `Escape` 关闭行为；
//! 默认零卡片底色、零边框、零圆角、零阴影与零入场动画）。

use core::time::Duration;
use velox_engine::prelude::*;

const AUTO_COLOR: Color = Color(0x0000_0001);

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

/// 无样式模态弹窗叠层原语（`Modal`）。
pub struct Modal<'a> {
    pub id: u32,
    pub title: StrRef<'a>,
    pub width: i16,
    pub height: i16,
    pub backdrop_bg: Color,
    pub card_bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub close_color: Color,
    pub close_hover_bg: Option<Color>,
    pub close_active_bg: Option<Color>,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub rounded: u8,
    /// 是否启用入场动画（无样式原语默认关闭）。
    pub animated: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub body: Option<Element<'a>>,
    pub on_close: Option<&'a dyn Fn(&mut ActionScope)>,
    pub backdrop_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Modal<'a> {
    /// 创建一个无样式居中模态弹窗原语。
    #[inline(always)]
    pub fn new(id: u32, title: impl Into<StrRef<'a>>) -> Self {
        Self {
            id,
            title: title.into(),
            width: 240,
            height: 140,
            backdrop_bg: Color::TRANSPARENT,
            card_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: AUTO_COLOR,
            close_color: AUTO_COLOR,
            close_hover_bg: None,
            close_active_bg: None,
            shadow: None,
            focus_ring: false,
            rounded: 0,
            animated: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            body: None,
            on_close: None,
            backdrop_renderer: None,
        }
    }

    /// 设置弹窗卡片尺寸 `(width, height)`。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 启用或禁用入场动画（遮罩淡入 + 卡片上滑回弹）。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 设置弹窗内部主体内容元素。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.body = Some(child.into_element(cx));
        self
    }

    /// 绑定关闭弹窗回调（支持右上角矢量关闭按钮、按 `Escape` 键与点击遮罩关闭）。
    #[inline(always)]
    pub fn on_close(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_close = Some(handler);
        self
    }

    /// `gpui-base` 风格背景遮罩插槽回调：自定义模态弹窗外层遮罩样式。
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

crate::impl_base_styled_box!(Modal<'a>);

impl<'a> Widget<'a> for Modal<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let title_color = resolve_color(self.title_color, def_fg);
        let close_color = resolve_color(self.close_color, def_fg);

        let header = if let Some(close_cb) = self.on_close {
            let mut close_btn = div()
                .id(crate::scoped_child_id(self.id, 0x4D4F_444C, 1))
                .w(24)
                .h(24)
                .rounded(if self.rounded > 0 { 4 } else { 0 })
                .items_center()
                .justify_center()
                .focusable()
                .on_click_raw(close_cb)
                .child(
                    cx,
                    svg_icon(14, 14, "M18 6 6 18M6 6l12 12")
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
            div()
                .flex_row()
                .w_full()
                .items_center()
                .justify_between()
                .child(cx, text(self.title).size(15).color(title_color))
                .child(cx, close_btn)
                .into_element(cx)
        } else {
            text(self.title).size(15).color(title_color).into_element(cx)
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let mut card = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(height)
            .p(16)
            .gap(10)
            .rounded(self.rounded)
            .bg(self.card_bg)
            .occlude_hit()
            .child(cx, header);
        if self.border_color != Color::TRANSPARENT {
            card = card.border(1, self.border_color);
        }
        if let Some((blur, ox, oy, col)) = self.shadow {
            card = card.shadow(blur, ox, oy, col);
        }

        if let Some(body_elem) = self.body {
            card = card.child(cx, body_elem);
        }

        if self.animated {
            card = card
                .enter_from(0, 0, 20, Duration::from_millis(220))
                .ease(Easing::EaseOutBack);
        }

        card = self.style_ext.apply_to_div(card, self.style);

        let mut backdrop = div();
        if self.animated {
            backdrop = backdrop.fade_in(Duration::from_millis(160));
        }

        backdrop = backdrop
            .pos(0, 0)
            .size_full()
            .overlay()
            .occlude_hit()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(self.backdrop_bg);

        if let Some(close_cb) = self.on_close {
            backdrop = backdrop.on_click_raw(close_cb).on_navigate_back(close_cb);
        }

        backdrop = backdrop.child(cx, card);
        if let Some(renderer) = self.backdrop_renderer {
            return renderer(backdrop, cx);
        }
        backdrop.into_element(cx)
    }
}
