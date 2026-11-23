//! 基础圆弧进度仪表组件 `ArcGauge`（原生解析 SDF 圆弧图元：GPU 单 Draw Call 片元直算零上传，软件端多线程并行光栅）。

use velox_core::prelude::*;

/// 基础圆弧进度环与旋钮仪表组件（`ArcGauge`）。
pub struct ArcGauge<'a> {
    pub id: u32,
    pub diameter: i16,
    pub thickness: i16,
    pub percentage: u8,
    pub start_deg: u16,
    pub sweep_deg: u16,
    pub fill_color: Option<Color>,
    pub track_color: Option<Color>,
    pub label_color: Color,
    pub label: Option<StrRef<'a>>,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> ArcGauge<'a> {
    /// 创建一个给定元素 `id`、直径 `diameter` 与进度百分比 `percentage` (`0..=100`) 的圆弧仪表。
    #[inline(always)]
    pub fn new(id: u32, diameter: i16, percentage: u8) -> Self {
        Self {
            id,
            diameter: diameter.max(24),
            thickness: 8,
            percentage: percentage.min(100),
            start_deg: 225,
            sweep_deg: 270,
            fill_color: None,
            track_color: None,
            label_color: Color::TRANSPARENT,
            label: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置圆弧线条像素宽度（默认 `8px`）。
    #[inline(always)]
    pub const fn thickness(mut self, px: i16) -> Self {
        self.thickness = if px < 2 { 2 } else { px };
        self
    }

    /// 设置圆弧起始顺时针角度（`0°` 为 12 点钟方向）与总跨度角度（默认 `225°` 起，跨度 `270°`）。
    #[inline(always)]
    pub const fn angles(mut self, start_deg: u16, sweep_deg: u16) -> Self {
        self.start_deg = start_deg % 360;
        self.sweep_deg = if sweep_deg > 350 { 350 } else { sweep_deg };
        self
    }

    /// 自定义前景进度弧颜色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.fill_color = Some(color);
        self
    }

    /// 自定义背景轨道弧颜色。
    #[inline(always)]
    pub const fn track_color(mut self, color: Color) -> Self {
        self.track_color = Some(color);
        self
    }

    /// 设置中心标签颜色。
    #[inline(always)]
    pub const fn label_color(mut self, color: Color) -> Self {
        self.label_color = color;
        self
    }

    /// 设置圆弧中心显示的文本标签（如 `"86%"`）。
    #[inline(always)]
    pub fn label(mut self, text_str: impl Into<StrRef<'a>>) -> Self {
        self.label = Some(text_str.into());
        self
    }
}

crate::impl_base_styled_box!(ArcGauge<'a>);

impl<'a> Widget<'a> for ArcGauge<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let fill_col = self.fill_color.unwrap_or(fg);
        let track_col = self.track_color.unwrap_or_else(|| fg.scale_alpha(48));
        let fg_col = if self.label_color.a() > 0 {
            self.label_color
        } else {
            fg
        };

        let diam = self.diameter;
        let thick = self.thickness.clamp(2, u8::MAX as i16) as u8;
        let start_deg10 = ((self.start_deg as u32 * 10) % 3600) as u16;
        let sweep_deg10 = (self.sweep_deg as u32 * 10).clamp(20, 3500) as u16;
        let fill_sweep_deg10 = ((sweep_deg10 as u32 * self.percentage as u32) / 100) as u16;

        let arc_elem = img_arc(
            diam,
            diam,
            thick,
            start_deg10,
            sweep_deg10,
            fill_sweep_deg10,
            track_col,
            fill_col,
        );

        let mut container = div().id(self.id).w(diam).h(diam).child(cx, arc_elem);
        if let Some(lbl) = self.label {
            container = container.child(
                cx,
                div()
                    .pos(0, 0)
                    .w(diam)
                    .h(diam)
                    .items_center()
                    .justify_center()
                    .child(cx, text(lbl).size(14).color(fg_col).align(TextAlign::Center)),
            );
        }
        container = self.style_ext.apply_to_div(container, self.style);
        container.into_element(cx)
    }
}
