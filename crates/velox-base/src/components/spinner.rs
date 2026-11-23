//! 基础环形加载旋转指示器组件 `Spinner`（基于 `ArcGauge` 渲染）。

use crate::components::ArcGauge;
use velox_core::prelude::*;

/// 基础环形旋转加载指示器组件（`Spinner`）。
pub struct Spinner {
    pub id: u32,
    pub diameter: i16,
    pub thickness: i16,
    pub angle_deg: u16,
    pub arc_len_deg: u16,
    pub color: Option<Color>,
    pub track_color: Option<Color>,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl Spinner {
    /// 创建一个环形加载指示器，传入组件 `id`、直径 `diameter` 与当前旋转角度 `angle_deg` (`0..360`)。
    #[inline(always)]
    pub const fn new(id: u32, diameter: i16, angle_deg: u16) -> Self {
        Self {
            id,
            diameter: if diameter < 24 { 24 } else { diameter },
            thickness: 4,
            angle_deg: angle_deg % 360,
            arc_len_deg: 100,
            color: None,
            track_color: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置圆环线条粗细（默认 `4px`）。
    #[inline(always)]
    pub const fn thickness(mut self, px: i16) -> Self {
        self.thickness = if px < 2 { 2 } else { px };
        self
    }

    /// 设置高亮圆弧角度跨度（默认 `100°`）。
    #[inline(always)]
    pub const fn arc_len(mut self, deg: u16) -> Self {
        self.arc_len_deg = if deg < 30 {
            30
        } else if deg > 300 {
            300
        } else {
            deg
        };
        self
    }

    /// 由引擎动画时钟自动驱动旋转（每 period_ms 毫秒转一圈），并自动请求连续重绘帧。
    ///
    /// 无需在外部维护 ngle_deg 状态或定时器：Spinner::new(id, 32, 0).spinning(cx, 1000)。
    #[inline]
    pub fn spinning(mut self, cx: &FrameScope, period_ms: u32) -> Self {
        let period = period_ms.max(1);
        self.angle_deg = (((cx.now_ms() % period) as u64 * 360) / period as u64) as u16;
        cx.request_frame();
        self
    }

    /// 自定义加载圆弧颜色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// 自定义背景轨道颜色。
    #[inline(always)]
    pub const fn track_color(mut self, color: Color) -> Self {
        self.track_color = Some(color);
        self
    }
}

crate::impl_base_styled_box!(Spinner);

impl<'a> Widget<'a> for Spinner {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let color = self.color.unwrap_or(fg);
        let track_color = self.track_color.unwrap_or_else(|| fg.scale_alpha(48));
        let mut gauge = ArcGauge::new(self.id, self.diameter, 100)
            .thickness(self.thickness)
            .angles(self.angle_deg, self.arc_len_deg)
            .color(color)
            .track_color(track_color);
        gauge.style = self.style;
        gauge.style_ext = self.style_ext;
        gauge.render_once(cx)
    }
}
