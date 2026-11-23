//! 四向边缘抽屉面板基础组件 `Sheet`（默认无样式，支持顶层浮层遮罩命中拦截）。

use velox_core::prelude::*;

/// 边缘抽屉滑入方向枚举（`SheetSide`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SheetSide {
    #[default]
    Right,
    Left,
    Top,
    Bottom,
}

/// 四向边缘抽屉面板基础组件（`Sheet`，默认无样式，支持从左、右、上、下任意边缘滑出）。
pub struct Sheet<'a> {
    pub id: u32,
    pub title: &'a str,
    pub side: SheetSide,
    pub open: bool,
    pub size: i16,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub close_color: Color,
    pub close_rounded: u8,
    pub close_hover_bg: Option<Color>,
    pub close_active_bg: Option<Color>,
    pub rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub content: Option<Element<'a>>,
    pub on_dismiss: Option<&'a dyn Fn(&mut ActionScope)>,
    pub backdrop_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Sheet<'a> {
    /// 创建一个默认无样式的边缘抽屉面板组件。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, side: SheetSide, open: bool) -> Self {
        Self {
            id,
            title,
            side,
            open,
            size: 260,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            close_color: Color::TRANSPARENT,
            close_rounded: 0,
            close_hover_bg: None,
            close_active_bg: None,
            rounded: 0,
            shadow: None,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            content: None,
            on_dismiss: None,
            backdrop_renderer: None,
        }
    }

    /// 设置抽屉在对应方向上的宽度（Left/Right）或高度（Top/Bottom）。
    #[inline(always)]
    pub const fn size(mut self, size: i16) -> Self {
        self.size = size;
        self
    }

    /// 设置抽屉主体内容。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.content = Some(child.into_element(cx));
        self
    }

    /// 绑定关闭抽屉回调。
    #[inline(always)]
    pub fn on_dismiss(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_dismiss = Some(handler);
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

crate::impl_base_styled_box!(Sheet<'a>);

impl<'a> Widget<'a> for Sheet<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        if !self.open {
            return div().id(self.id).w(0).h(0).overlay().into_element(cx);
        }

        let fg = cx.theme().foreground;
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };
        let close_color = if self.close_color.a() > 0 {
            self.close_color
        } else {
            fg
        };

        let mut close_btn = div()
            .id(crate::scoped_child_id(self.id, 0x03CC_1FA8, 1))
            .w(24)
            .h(24)
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
            svg_icon(14, 14, "M18 6 6 18M6 6l12 12")
                .tint(close_color)
                .stroke_width(20),
        );
        if let Some(cb) = self.on_dismiss {
            close_btn = close_btn.on_click_raw(cb);
        }

        let (panel_w, panel_h) = match self.side {
            SheetSide::Left | SheetSide::Right => (self.size, 280i16),
            SheetSide::Top | SheetSide::Bottom => (360i16, self.size),
        };

        let header = div()
            .flex_row()
            .w(panel_w - 28)
            .items_center()
            .justify_between()
            .child(cx, text(self.title).size(14).color(title_color))
            .child(cx, close_btn);

        let mut panel = div()
            .id(self.id)
            .flex_col()
            .w(panel_w)
            .h(panel_h)
            .p(16)
            .gap(10)
            .rounded(self.rounded)
            .bg(self.bg)
            .occlude_hit()
            .child(cx, header);
        if self.backdrop_renderer.is_none() {
            panel = panel.overlay();
        }
        if self.border_color.a() > 0 {
            panel = panel.border(1, self.border_color);
        }
        if let Some((blur, ox, oy, col)) = self.shadow {
            panel = panel.shadow(blur, ox, oy, col);
        }

        if let Some(cb) = self.on_dismiss {
            panel = panel.on_navigate_back(cb);
        }

        if let Some(body) = self.content {
            panel = panel.child(cx, body);
        }

        panel = self.style_ext.apply_to_div(panel, self.style);
        if let Some(renderer) = self.backdrop_renderer {
            let mut backdrop = div()
                .pos(0, 0)
                .size_full()
                .overlay()
                .occlude_hit()
                .child(cx, panel);
            if let Some(cb) = self.on_dismiss {
                backdrop = backdrop.on_click_raw(cb).on_navigate_back(cb);
            }
            return renderer(backdrop, cx);
        }
        panel.into_element(cx)
    }
}
