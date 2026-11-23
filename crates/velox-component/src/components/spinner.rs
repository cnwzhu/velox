//! 主题化加载旋转指示器组件 `Spinner`。

use velox_core::prelude::*;

crate::define_themed_component! {
    name: Spinner,
    base: velox_base::Spinner,
    new: (id: u32, diameter: i16, angle_deg: u16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        if base.color.is_none() {
            base.color = Some(theme.primary);
        }
        if base.track_color.is_none() {
            base.track_color = Some(theme.secondary);
        }
    },
    forward: [
        thickness(px: i16),
        arc_len(deg: u16),
        spinning(cx: &FrameScope, period_ms: u32),
        color(color: Color),
        track_color(color: Color),
    ],
}
