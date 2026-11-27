//! 基础移动端底部抽屉弹窗组件 `BottomSheet`（默认无样式，支持 `.overlay().occlude_hit()` 遮罩、拖拽指示条与底部安全区/软键盘自动避让）。

use core::time::Duration;
use velox_engine::prelude::*;

/// 基础移动端底部弹出抽屉组件（`BottomSheet`，默认无样式）。
pub struct BottomSheet<'a> {
    pub id: u32,
    pub title: StrRef<'a>,
    pub sheet_height: i16,
    pub backdrop_bg: Color,
    pub sheet_bg: Color,
    pub border_color: Color,
    pub handle_color: Color,
    pub handle_rounded: u8,
    pub title_color: Color,
    pub rounded: u8,
    /// 是否启用入场动画（遮罩淡入 + 抽屉自底部滑出，基础组件默认 `false`）。
    pub animated: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub body: Option<Element<'a>>,
    pub on_dismiss: Option<&'a dyn Fn(&mut ActionScope)>,
    pub backdrop_renderer: Option<&'a dyn Fn(Div<'a>, &'a FrameScope) -> Element<'a>>,
}

impl<'a> BottomSheet<'a> {
    /// 创建一个默认无样式的底部弹出抽屉组件。
    #[inline(always)]
    pub fn new(id: u32, title: impl Into<StrRef<'a>>, sheet_height: i16) -> Self {
        Self {
            id,
            title: title.into(),
            sheet_height: if sheet_height < 80 { 80 } else { sheet_height },
            backdrop_bg: Color::TRANSPARENT,
            sheet_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            handle_color: Color::TRANSPARENT,
            handle_rounded: 0,
            title_color: Color::TRANSPARENT,
            rounded: 0,
            animated: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            body: None,
            on_dismiss: None,
            backdrop_renderer: None,
        }
    }

    /// 设置抽屉主体内容元素。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.body = Some(child.into_element(cx));
        self
    }

    /// 启用或禁用入场动画（遮罩淡入 + 抽屉自底部滑出）。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定点击背景遮罩或下滑关闭回调。
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

crate::impl_base_styled_box!(BottomSheet<'a>);

impl<'a> Widget<'a> for BottomSheet<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let handle_color = if self.handle_color.a() > 0 {
            self.handle_color
        } else {
            fg.scale_alpha(96)
        };
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };

        let bottom_extra = cx
            .safe_area()
            .bottom
            .saturating_add(cx.keyboard_inset())
            .max(0);
        let eff_h = self.sheet_height.saturating_add(bottom_extra);

        let handle_bar = div()
            .w(36)
            .h(4)
            .rounded(self.handle_rounded)
            .bg(handle_color);

        let mut sheet = div()
            .id(self.id)
            .flex_col()
            .items_center()
            .w_full()
            .h(eff_h)
            .p(14)
            .pb((14 + bottom_extra).clamp(0, 127) as i8)
            .gap(10)
            .rounded(self.rounded)
            .bg(self.sheet_bg)
            .occlude_hit()
            .child(cx, handle_bar)
            .child(cx, text(self.title).size(15).color(title_color));
        if self.border_color.a() > 0 {
            sheet = sheet.border(1, self.border_color);
        }

        if let Some(body_elem) = self.body {
            sheet = sheet.child(cx, body_elem);
        }

        let mut backdrop = div()
            .id(crate::scoped_child_id(self.id, 0xFD20_A252, 1))
            .pos(0, 0)
            .size_full()
            .overlay()
            .occlude_hit()
            .flex_col()
            .justify_end()
            .bg(self.backdrop_bg);

        if self.animated {
            backdrop = backdrop
                .enter_from(24, 0, 0, Duration::from_millis(180))
                .fade_out(Duration::from_millis(160));
            sheet = sheet
                .enter_from(255, 0, eff_h, Duration::from_millis(260))
                .exit_to(0, 0, eff_h.min(120), Duration::from_millis(200));
        }

        if let Some(dismiss_cb) = self.on_dismiss {
            let swipe_cb = cx.arena().alloc(move |dir: SwipeDir, acx: &mut ActionScope| {
                if matches!(dir, SwipeDir::Down | SwipeDir::Right) {
                    dismiss_cb(acx);
                }
            });
            backdrop = backdrop.on_click_raw(dismiss_cb).on_swipe(cx, swipe_cb);
            sheet = sheet.on_swipe(cx, swipe_cb);
        }

        sheet = self.style_ext.apply_to_div(sheet, self.style);
        backdrop = backdrop.child(cx, sheet);
        if let Some(renderer) = self.backdrop_renderer {
            return renderer(backdrop, cx);
        }
        backdrop.into_element(cx)
    }
}
