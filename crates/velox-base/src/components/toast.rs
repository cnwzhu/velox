//! 顶层轻提示通知横幅基础组件 `Toast`（默认无样式，保留定位、`.overlay()` 浮层与可选入场动画）。

use core::time::Duration;
use velox_core::prelude::*;

/// 浮动轻提示通知基础组件（`Toast`，默认无样式）。
pub struct Toast<'a> {
    pub id: u32,
    pub message: &'a str,
    pub x: i16,
    pub y: i16,
    pub width: i16,
    pub height: i16,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    /// `None` 表示按胶囊圆角 `height / 2` 计算，默认 `Some(0)`（直角无圆角）。
    pub rounded: Option<u8>,
    pub shadow_alpha: u8,
    /// 是否启用入场动画（自上方滑入并淡入，基础组件默认 `false`）。
    pub animated: bool,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Toast<'a> {
    /// 创建一个位于顶层 `(x, y)` 的默认无样式轻提示通知。
    #[inline(always)]
    pub const fn new(id: u32, message: &'a str, x: i16, y: i16) -> Self {
        Self {
            id,
            message,
            x,
            y,
            width: 180,
            height: 30,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            rounded: Some(0),
            shadow_alpha: 0,
            animated: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置提示框宽高（像素）。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 启用或禁用入场动画（自上方滑入并淡入）。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }
}

crate::impl_base_styled_box!(Toast<'a>);

impl<'a> Widget<'a> for Toast<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            cx.theme().foreground
        };
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let rounded = self
            .rounded
            .unwrap_or_else(|| (height / 2).clamp(0, 127) as u8);

        let mut d = div()
            .id(self.id)
            .pos(self.x, self.y)
            .w(width)
            .h(height)
            .px(12)
            .flex_row()
            .items_center()
            .justify_center()
            .rounded(rounded)
            .bg(self.bg)
            .overlay();

        if self.border_color.a() > 0 {
            d = d.border(1, self.border_color);
        }

        if self.shadow_alpha > 0 {
            d = d.shadow(4, 0, 2, Color::rgba(0, 0, 0, self.shadow_alpha));
        }

        if self.animated {
            d = d.slide_in(0, -12, Duration::from_millis(220));
        }

        d = self.style_ext.apply_to_div(d, self.style);
        d.child(cx, text(self.message).size(12).color(text_color))
            .into_element(cx)
    }
}
