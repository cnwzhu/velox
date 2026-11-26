//! 主题化可折叠容器组件 `Collapsible`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::collapsible::CollapsibleState;

crate::define_themed_component! {
    name: Collapsible<'a>,
    base: velox_base::Collapsible<'a>,
    new: (id: u32, title: &'a str, open: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
        {
            base.animated = true;
        }
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.header_bg, theme.card);
        fill_color(&mut base.content_bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.arrow_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.list_hover);
        }
        if base.active_bg.is_none() {
            base.active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        width(width: i16),
        animated(enabled: bool),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
        on_toggle(handler: &'a dyn Fn(bool, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Collapsible, trigger, (Div<'a>, CollapsibleState<'a>));

crate::forward_slot_el!(Collapsible, panel, (Div<'a>, CollapsibleState<'a>));

impl<'a> Collapsible<'a> {
    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，展开/折叠时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, title: &'a str, sig: Signal<bool>) -> Self {
        Self::from_base(velox_base::Collapsible::bind(cx, id, title, sig))
    }
}
