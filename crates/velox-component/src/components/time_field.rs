//! 主题化分段式时间输入框组件 `TimeField`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::time_field::{
    HourCycle, TimeFieldState, TimePrecision, TimeSegment, TimeSegmentState,
};

crate::define_themed_component! {
    name: TimeField<'a>,
    base: velox_base::TimeField<'a>,
    new: (id: u32, state: TimeFieldState),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.focus_border_color, theme.ring);
        fill_color(&mut base.active_segment_bg, theme.primary);
        fill_color(&mut base.active_segment_fg, theme.primary_foreground);
        fill_color(&mut base.text_color, theme.foreground);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        focused(focused: bool),
        on_segment_click(handler: &'a dyn Fn(TimeSegment, &mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(TimeField, segment, (Div<'a>, TimeSegmentState, &'a FrameScope));
