//! 窗口级根容器基础组件 `Root`（默认无样式，支持向子树注入主题与承载对话框/通知浮层）。

use crate::components::Theme;
use velox_engine::prelude::*;

/// 窗口级根容器基础组件（`Root`，默认无样式，统一承载页面主视图、模态对话框叠层与通知浮层栈）。
pub struct Root<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub theme: Theme,
    pub bg: Color,
    pub text_color: Color,
    pub dialog_backdrop_bg: Color,
    pub has_custom_theme: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub content: Option<Element<'a>>,
    pub dialog: Option<Element<'a>>,
    pub notification: Option<Element<'a>>,
}

impl<'a> Root<'a> {
    /// 创建一个默认无样式的窗口级根容器组件。
    #[inline(always)]
    pub const fn new(id: u32, width: i16, height: i16) -> Self {
        Self {
            id,
            width,
            height,
            theme: Theme::dark(),
            bg: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            dialog_backdrop_bg: Color::TRANSPARENT,
            has_custom_theme: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            content: None,
            dialog: None,
            notification: None,
        }
    }

    /// 设置根容器及整个子树使用的主题配色表。
    #[inline(always)]
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self.bg = theme.background;
        self.text_color = theme.foreground;
        self.has_custom_theme = true;
        self
    }

    /// 设置根容器主题并立即将其注入当前帧作用域 `cx`（确保后续链式构造参数中的内联子组件也能立即读取新主题）。
    #[inline(always)]
    pub fn theme_in(mut self, cx: &FrameScope, theme: Theme) -> Self {
        self.theme = theme;
        self.bg = theme.background;
        self.text_color = theme.foreground;
        self.has_custom_theme = true;
        cx.override_theme(theme);
        self
    }

    /// 设置主窗口内容区域元素。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        if self.has_custom_theme {
            cx.override_theme(self.theme);
        }
        self.content = Some(child.into_element(cx));
        self
    }

    /// 通过闭包惰性构造主窗口内容区域（在执行闭包前自动向 `cx` 注入当前 `Root` 主题）。
    #[inline(always)]
    pub fn content_with<E: IntoElement<'a>>(
        mut self,
        cx: &'a FrameScope,
        build: impl FnOnce(&'a FrameScope) -> E,
    ) -> Self {
        if self.has_custom_theme {
            cx.override_theme(self.theme);
        }
        self.content = Some(build(cx).into_element(cx));
        self
    }

    /// 挂载当前激活的全局对话框或抽屉层。
    #[inline(always)]
    pub fn dialog(mut self, cx: &'a FrameScope, dialog: impl IntoElement<'a>) -> Self {
        if self.has_custom_theme {
            cx.override_theme(self.theme);
        }
        self.dialog = Some(dialog.into_element(cx));
        self
    }

    /// 挂载当前激活的全局通知横幅或轻提示层。
    #[inline(always)]
    pub fn notification(mut self, cx: &'a FrameScope, notification: impl IntoElement<'a>) -> Self {
        if self.has_custom_theme {
            cx.override_theme(self.theme);
        }
        self.notification = Some(notification.into_element(cx));
        self
    }
}

crate::impl_base_styled_box!(Root<'a>);

impl<'a> Widget<'a> for Root<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        if self.has_custom_theme {
            cx.override_theme(self.theme);
        }

        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);

        let mut window = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(height)
            .bg(self.bg);

        if let Some(c) = self.content.take() {
            window = window.child(cx, c);
        }
        if let Some(dlg) = self.dialog.take() {
            let dlg_layer = div()
                .id(self.id ^ 0x5254_444C)
                .pos(0, 0)
                .w(width)
                .h(height)
                .overlay()
                .occlude_hit()
                .flex_col()
                .items_center()
                .justify_center()
                .bg(self.dialog_backdrop_bg)
                .child(cx, dlg);
            window = window.child(cx, dlg_layer);
        }
        if let Some(notif) = self.notification.take() {
            let nx = (width - 336).max(8);
            let notif_layer = div()
                .id(self.id ^ 0x5254_4E54)
                .pos(nx, 12)
                .overlay()
                .child(cx, notif);
            window = window.child(cx, notif_layer);
        }

        window = self.style_ext.apply_to_div(window, self.style);
        window.into_element(cx)
    }
}
