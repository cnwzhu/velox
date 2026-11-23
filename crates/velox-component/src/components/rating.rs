//! 主题化星级评分组件 `Rating`。

use crate::styled::fill_color;
use velox_core::prelude::*;

pub use velox_base::components::rating::RatingItemState;

crate::define_themed_component! {
    name: Rating<'a>,
    base: velox_base::Rating<'a>,
    new: (id: u32, value: u8),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.active_color, theme.warning);
        fill_color(&mut base.inactive_color, theme.muted_foreground);
    },
    forward: [
        max(max: u8),
        readonly(readonly: bool),
        on_change(handler: &'a dyn Fn(u8, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Rating, item, (Div<'a>, RatingItemState));

impl<'a> Rating<'a> {
    /// 零堆分配双向绑定至 [`Signal<u8>`]：渲染期自动建立细粒度订阅，点击评分时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<u8>) -> Self {
        Self::from_base(velox_base::Rating::bind(cx, id, sig))
    }
}
