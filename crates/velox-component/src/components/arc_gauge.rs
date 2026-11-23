//! 主题化 SDF 圆弧仪表盘组件 `ArcGauge`。

use crate::styled::fill_color;
use velox_core::prelude::*;

crate::define_themed_component! {
    name: ArcGauge<'a>,
    base: velox_base::ArcGauge<'a>,
    new: (id: u32, diameter: i16, percentage: u8),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        if base.fill_color.is_none() {
            base.fill_color = Some(theme.primary);
        }
        if base.track_color.is_none() {
            base.track_color = Some(theme.secondary);
        }
        fill_color(&mut base.label_color, theme.foreground);
    },
    forward: [
        thickness(px: i16),
        angles(start_deg: u16, sweep_deg: u16),
        color(color: Color),
        track_color(color: Color),
        label_color(color: Color),
        label(text_str: impl Into<StrRef<'a>>),
    ],
}
