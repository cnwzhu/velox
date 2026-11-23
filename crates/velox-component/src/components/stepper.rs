//! 主题化步骤条组件 `Stepper`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::stepper::{StepItem, StepStatus, StepperItemState};

crate::define_themed_component! {
    name: Stepper<'a>,
    base: velox_base::Stepper<'a>,
    new: (id: u32, steps: &'a [StepItem<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_rounded(&mut base.badge_rounded, 12);
        fill_color(&mut base.active_bg, theme.primary);
        fill_color(&mut base.active_border, theme.primary);
        fill_color(&mut base.active_fg, theme.primary_foreground);
        fill_color(&mut base.completed_bg, theme.success);
        fill_color(&mut base.completed_border, theme.success);
        fill_color(&mut base.completed_fg, theme.success_foreground);
        fill_color(&mut base.error_bg, theme.danger);
        fill_color(&mut base.error_border, theme.danger);
        fill_color(&mut base.error_fg, theme.danger_foreground);
        fill_color(&mut base.pending_bg, theme.secondary);
        fill_color(&mut base.pending_border, theme.border);
        fill_color(&mut base.pending_fg, theme.muted_foreground);
        fill_color(&mut base.title_color, theme.foreground);
        fill_color(&mut base.desc_color, theme.muted_foreground);
        fill_color(&mut base.line_color, theme.border);
        fill_color(&mut base.completed_line_color, theme.success);
    },
    forward: [
        vertical(vertical: bool),
        on_step_click(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Stepper, item, (Div<'a>, StepperItemState, &'a FrameScope));
