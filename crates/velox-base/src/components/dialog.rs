//! 无样式通用对话框原语 `Dialog`（提供顶层 `.overlay().occlude_hit()` 遮挡与 `Escape` 关闭行为；
//! 默认零背景、零边框、零圆角、零阴影与零悬停渐变）。

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

/// 无样式通用对话框原语（`Dialog`）。
pub struct Dialog<'a> {
    pub id: u32,
    pub title: &'a str,
    pub description: &'a str,
    pub open: bool,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub description_color: Color,
    pub close_color: Color,
    pub close_hover_bg: Option<Color>,
    pub close_active_bg: Option<Color>,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub body: Option<Element<'a>>,
    pub footer: Option<Element<'a>>,
    pub on_close: Option<&'a dyn Fn(&mut ActionScope)>,
    pub backdrop_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Dialog<'a> {
    /// 创建一个无样式通用对话框原语。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, open: bool) -> Self {
        Self {
            id,
            title,
            description: "",
            open,
            width: 360,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: AUTO_COLOR,
            description_color: AUTO_COLOR,
            close_color: AUTO_COLOR,
            close_hover_bg: None,
            close_active_bg: None,
            shadow: None,
            focus_ring: false,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            body: None,
            footer: None,
            on_close: None,
            backdrop_renderer: None,
        }
    }

    /// 设置副标题说明描述。
    #[inline(always)]
    pub const fn description(mut self, description: &'a str) -> Self {
        self.description = description;
        self
    }

    /// 设置对话框宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 设置对话框主体内容。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.body = Some(child.into_element(cx));
        self
    }

    /// 设置对话框底部操作栏元素。
    #[inline(always)]
    pub fn footer(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.footer = Some(child.into_element(cx));
        self
    }

    /// 绑定点击右上角关闭按钮的回调。
    #[inline(always)]
    pub fn on_close(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_close = Some(handler);
        self
    }

    /// `gpui-base` 风格背景遮罩插槽回调：包裹并定制对话框全屏遮罩层样式。
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

crate::impl_base_styled_box!(Dialog<'a>);

impl<'a> Widget<'a> for Dialog<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        if !self.open {
            return div().id(self.id).w(0).h(0).overlay().into_element(cx);
        }

        let def_fg = cx.theme().foreground;
        let title_color = resolve_color(self.title_color, def_fg);
        let description_color = resolve_color(self.description_color, def_fg.scale_alpha(160));
        let close_color = resolve_color(self.close_color, def_fg);

        let mut close_btn = div()
            .id(crate::scoped_child_id(self.id, 0x0AA7_F0B9, 1))
            .w(24)
            .h(24)
            .rounded(if self.rounded > 0 {
                self.rounded.saturating_sub(6).max(4)
            } else {
                0
            })
            .items_center()
            .justify_center()
            .focusable()
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
        if let Some(cb) = self.on_close {
            close_btn = close_btn.on_click_raw(cb);
        }

        let width = StyleExt::resolve_width(self.style, self.width);
        let header = div()
            .flex_row()
            .w(width - 32)
            .items_center()
            .justify_between()
            .child(cx, text(self.title).size(15).color(title_color))
            .child(cx, close_btn);

        let mut panel = div()
            .id(self.id)
            .occlude_hit()
            .flex_col()
            .w(width)
            .p(16)
            .gap(10)
            .rounded(self.rounded)
            .bg(self.bg)
            .child(cx, header);
        if self.backdrop_renderer.is_none() {
            panel = panel.overlay();
        }
        if self.border_color != Color::TRANSPARENT {
            panel = panel.border(1, self.border_color);
        }
        if let Some((blur, ox, oy, col)) = self.shadow {
            panel = panel.shadow(blur, ox, oy, col);
        }

        if let Some(cb) = self.on_close {
            panel = panel.on_navigate_back(cb);
        }

        if !self.description.is_empty() {
            panel = panel.child(cx, text(self.description).size(12).color(description_color));
        }

        if let Some(b) = self.body {
            panel = panel.child(cx, b);
        }

        if let Some(f) = self.footer {
            panel = panel.child(cx, div().flex_row().justify_end().gap(8).child(cx, f));
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
            if let Some(cb) = self.on_close {
                backdrop = backdrop.on_click_raw(cb).on_navigate_back(cb);
            }
            return renderer(backdrop, cx);
        }
        panel.into_element(cx)
    }
}
