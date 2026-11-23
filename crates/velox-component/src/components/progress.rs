//! 主题化进度条组件 `ProgressBar` / `Progress`。

use crate::styled::fill_color;
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: ProgressBar,
    base: velox_base::ProgressBar,
    new: (id: u32, width: i16, percent: u8),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.rounded = None;
        base.animated = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.track_color, theme.secondary);
        if base.fill_color.is_none() {
            base.fill_color = Some(theme.primary);
        }
    },
    forward: [
        height(h: i16),
        color(color: Color),
        track_color(color: Color),
        rounded(radius: u8),
        animated(enabled: bool),
    ],
}

/// `ProgressBar` 主题化简短别名。

pub type Progress = ProgressBar;
