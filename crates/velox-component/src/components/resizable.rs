//! 主题化可调分栏布局组件 `ResizableSplit` / `Resizable`。

use crate::styled::fill_color;
use velox_engine::prelude::*;

pub use velox_base::components::resizable::{DockWorkspaceState, SplitAxis};

crate::define_themed_component! {
    name: ResizableSplit<'a>,
    base: velox_base::ResizableSplit<'a>,
    new: (id: u32, axis: SplitAxis, width: i16, height: i16, primary_size: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.handle_color, theme.border);
        if base.handle_hover_color.is_none() {
            base.handle_hover_color = Some(theme.primary);
        }
        if base.handle_active_color.is_none() {
            base.handle_active_color = Some(theme.primary);
        }
    },
    forward: [
        first(cx: &'a FrameScope, el: impl IntoElement<'a>),
        second(cx: &'a FrameScope, el: impl IntoElement<'a>),
        on_resize(handler: &'a dyn Fn(i16, &mut ActionScope)),
    ],
}

/// `ResizableSplit` 主题化简短别名。

pub type Resizable<'a> = ResizableSplit<'a>;
