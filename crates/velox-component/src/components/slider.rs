//! 主题化滑动条组件 `Slider`。

use crate::styled::fill_color;
use velox_engine::prelude::*;

pub use velox_base::components::slider::SliderState;

crate::define_themed_component! {
    name: Slider<'a>,
    base: velox_base::components::slider::Slider<'a>,
    new: (id: u32, value: i32),
    styled: false,
    variants: false,
    sizable: true,
    disableable: true,
    apply_tw: |base, theme| {
        if !base.has_custom_colors() {
            fill_color(&mut base.track_bg, theme.input);
            fill_color(&mut base.track_border_color, theme.border);
            fill_color(&mut base.fill_bg, theme.primary);
            fill_color(&mut base.thumb_bg, Color::WHITE);
            fill_color(&mut base.thumb_border_color, base.fill_bg);
        }
        if base.thumb_border_width == 0 {
            base.thumb_border_width = 2;
        }
        if base.track_rounded.is_none() {
            let track_h = match base.size {
                velox_base::ComponentSize::Small => 4i16,
                velox_base::ComponentSize::Medium => 6i16,
                velox_base::ComponentSize::Large => 8i16,
            };
            base.track_rounded = Some((track_h / 2) as u8);
        }
        if base.thumb_rounded.is_none() {
            let thumb_sz = match base.size {
                velox_base::ComponentSize::Small => 14i16,
                velox_base::ComponentSize::Medium => 16i16,
                velox_base::ComponentSize::Large => 20i16,
            };
            base.thumb_rounded = Some((thumb_sz / 2) as u8);
        }
        if base.thumb_shadow.is_none() {
            base.thumb_shadow = Some((4, 0, 1, Color::rgba(0, 0, 0, 75)));
        }
        if base.thumb_hover_bg.is_none() {
            base.thumb_hover_bg = Some(base.thumb_bg.lerp(base.fill_bg, 22));
        }
        if base.thumb_active_bg.is_none() {
            base.thumb_active_bg = Some(base.thumb_bg.lerp(base.fill_bg, 44));
        }
    },
    forward: [
        range(min: i32, max: i32),
        step(step: i32),
        accent(color: Color),
        colors(track_bg: Color, track_border_color: Color, fill_bg: Color, thumb_bg: Color, thumb_border_color: Color),
        w(w: i16),
        h(h: i16),
        on_change(handler: &'a dyn Fn(i32, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Slider, track, (Div<'a>, SliderState));

crate::forward_slot_el!(Slider, indicator, (Div<'a>, SliderState));

crate::forward_slot_el!(Slider, thumb, (Div<'a>, SliderState));

impl<'a> Slider<'a> {
    /// 零堆分配双向绑定至 [`Signal<i32>`]：渲染期自动建立细粒度订阅，拖动或按键时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<i32>) -> Self {
        Self::from_base(velox_base::Slider::bind(cx, id, sig))
    }
}
