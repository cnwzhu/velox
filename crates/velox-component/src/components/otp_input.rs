//! 主题化一次性验证码输入框组件 `OtpInput`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::otp_input::{OtpSlotState, OtpState};

crate::define_themed_component! {
    name: OtpInput<'a>,
    base: velox_base::OtpInput<'a>,
    new: (id: u32, value: &'a str, length: usize),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = false;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.background);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.active_border_color, theme.ring);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.separator_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        masked(masked: bool),
        focused(focused: bool),
        groups(group_size: usize),
        on_focus(handler: &'a dyn Fn(&mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(OtpInput, slot, (Div<'a>, OtpSlotState, &'a FrameScope));
