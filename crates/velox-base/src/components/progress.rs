//! 线性进度条基础组件 `ProgressBar`（默认无样式，支持百分比裁剪与可选平滑过渡动画）。

use core::time::Duration;
use velox_engine::prelude::*;

/// `Progress` 进度条组件类型别名（与 [`ProgressBar`] 等价）。
pub type Progress = ProgressBar;

/// 线性进度条基础组件（`ProgressBar`，默认无样式，支持完整 `Styled` / `StyledExt` 链式语法）。
#[derive(Clone, Copy, Debug)]
pub struct ProgressBar {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub percent: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub track_color: Color,
    pub fill_color: Option<Color>,
    /// `None` 表示按胶囊圆角 `height >> 1` 计算，默认 `Some(0)`（直角无圆角）。
    pub rounded: Option<u8>,
    pub animated: bool,
}

impl ProgressBar {
    /// 创建一个默认无样式的指定唯一 ID、总宽度与百分比（`0..=100`）的进度条。
    #[inline(always)]
    pub const fn new(id: u32, width: i16, percent: u8) -> Self {
        Self {
            id,
            width,
            height: 8,
            percent: if percent > 100 { 100 } else { percent },
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            track_color: Color::TRANSPARENT,
            fill_color: None,
            rounded: Some(0),
            animated: false,
        }
    }

    /// 设置进度条高度（像素）。
    #[inline(always)]
    pub const fn height(mut self, h: i16) -> Self {
        self.height = h;
        self
    }

    /// 自定义进度条前景色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.fill_color = Some(color);
        self
    }

    /// 设置背景轨道颜色。
    #[inline(always)]
    pub const fn track_color(mut self, color: Color) -> Self {
        self.track_color = color;
        self
    }

    /// 设置进度条圆角半径（像素）。
    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.rounded = Some(radius);
        self
    }

    /// 启用或禁用宽度平滑过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }
}

crate::impl_base_styled_box!(ProgressBar);

impl<'a> Widget<'a> for ProgressBar {
    #[inline(always)]
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let eff_h = StyleExt::resolve_height(self.style, self.height);
        let fg_base = cx.theme().foreground;
        let track_color = if self.style.bg != Color::TRANSPARENT {
            self.style.bg
        } else if self.track_color.a() > 0 {
            self.track_color
        } else {
            fg_base.scale_alpha(48)
        };
        let fg = self.fill_color.unwrap_or(fg_base);
        let radius = if self.style.rounded_radius() > 0 {
            self.style.rounded_radius()
        } else {
            self.rounded
                .unwrap_or_else(|| (eff_h >> 1).max(1) as u8)
        };
        let fill_w = ((eff_w as i32 * self.percent as i32) / 100) as i16;

        let mut fill = div()
            .id(self.id)
            .w(fill_w)
            .h(eff_h)
            .rounded(radius)
            .bg(fg);

        if self.animated {
            fill = fill.animate_bounds(Duration::from_millis(200));
        }

        let track = div()
            .w(eff_w)
            .h(eff_h)
            .rounded(radius)
            .bg(track_color)
            .overflow_hidden()
            .child(cx, fill);
        self.style_ext
            .apply_to_div(track, self.style)
            .into_element(cx)
    }
}
