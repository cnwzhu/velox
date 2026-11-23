//! 主题化语音转写控件组件 `Speech`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::speech::{SpeechState, SpeechStatus};

crate::define_themed_component! {
    name: Speech<'a>,
    base: velox_base::Speech<'a>,
    new: (id: u32, state: SpeechState, transcript: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.active_border_color, theme.danger);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.idle_mic_bg, theme.primary);
        fill_color(&mut base.listening_mic_bg, theme.danger);
        fill_color(&mut base.processing_mic_bg, theme.warning);
        fill_color(&mut base.error_mic_bg, theme.danger);
        fill_color(&mut base.mic_fg, Color::WHITE);
        fill_color(&mut base.meter_color, theme.success);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        width(width: i16),
        on_toggle_listen(handler: &'a dyn Fn(&mut ActionScope)),
        on_clear(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}
