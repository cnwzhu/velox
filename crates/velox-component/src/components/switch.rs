//! 主题化滑动开关组件 `Switch`。

use crate::styled::fill_color;
use velox_engine::prelude::*;

pub use velox_base::components::switch::{SwitchLabelSide, SwitchState};

crate::define_themed_component! {
    name: Switch<'a>,
    base: velox_base::components::switch::Switch<'a>,
    new: (id: u32, checked: bool),
    styled: false,
    variants: false,
    sizable: true,
    disableable: true,
    init: |base| {
        base.focus_ring = true;
        base.animated = true;
    },
    apply_tw: |base, theme| {
        let on_col = if base.has_custom_active_color() && base.active_color != Color::TRANSPARENT {
            base.active_color
        } else {
            theme.primary
        };
        if !base.has_custom_active_color() {
            fill_color(&mut base.active_color, theme.primary);
        }
        if !base.has_custom_track_bg() && base.track_bg == Color::TRANSPARENT {
            base.track_bg = if base.disabled {
                if base.checked { theme.muted } else { theme.input }
            } else if base.checked {
                on_col
            } else {
                theme.input
            };
        }
        if !base.has_custom_thumb_bg() {
            fill_color(
                &mut base.thumb_bg,
                if base.disabled { theme.muted_foreground } else { Color::WHITE },
            );
        }
        if !base.has_custom_border() && base.border_width == 0 && base.border_color == Color::TRANSPARENT {
            base.border_width = 1;
            base.border_color = if base.checked && !base.disabled {
                on_col
            } else {
                theme.border
            };
        }
        fill_color(
            &mut base.label_color,
            if base.disabled { theme.muted_foreground } else { theme.foreground },
        );
        let (tr_h, th_sz) = match base.size {
            velox_base::ComponentSize::Small => (16i16, 12i16),
            velox_base::ComponentSize::Medium => (20i16, 16i16),
            velox_base::ComponentSize::Large => (24i16, 20i16),
        };
        if base.track_rounded.is_none() {
            base.track_rounded = Some((tr_h / 2) as u8);
        }
        if base.thumb_rounded.is_none() {
            base.thumb_rounded = Some((th_sz / 2) as u8);
        }
        if base.hover_bg.is_none() {
            base.hover_bg = Some(if base.checked {
                on_col.lerp(Color::WHITE, 24)
            } else {
                theme.accent
            });
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(if base.checked {
                on_col.lerp(Color::BLACK, 32)
            } else {
                theme.list_active
            });
        }
        if base.thumb_shadow.is_none() {
            base.thumb_shadow = Some((2, 0, 1, Color::rgba(0, 0, 0, 55)));
        }
    },
    forward: [
        label(text: impl Into<StrRef<'a>>),
        label_side(side: SwitchLabelSide),
        color(color: Color),
        focus_ring(enabled: bool),
        geometry(track_width: i16, track_height: i16, thumb_size: i16, padding: i8),
        track_bg(color: Color),
        thumb_bg(color: Color),
        border(width: u8, color: Color),
        animated(enabled: bool),
        on_change(handler: &'a dyn Fn(bool, &mut ActionScope)),
    ],
}

crate::forward_slot_div!(Switch, track, (Div<'a>, SwitchState));

crate::forward_slot_el!(Switch, thumb, (Div<'a>, SwitchState));

impl<'a> Switch<'a> {
    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，切换时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<bool>) -> Self {
        Self::from_base(velox_base::Switch::bind(cx, id, sig))
    }
}
