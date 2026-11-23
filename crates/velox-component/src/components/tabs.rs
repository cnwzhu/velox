//! 主题化选项卡切换组件 `Tabs`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::tabs::{TabItemState, TabsVariant};

crate::define_themed_component! {
    name: Tabs<'a>,
    base: velox_base::components::tabs::Tabs<'a>,
    new: (id: u32, labels: &'a [&'a str], active: usize),
    styled: false,
    variants: false,
    sizable: true,
    disableable: false,
    init: |base| {
        base.animated = true;
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        if !base.has_custom_height() {
            base.height = match base.size {
                velox_base::ComponentSize::Small => 28,
                velox_base::ComponentSize::Medium => 34,
                velox_base::ComponentSize::Large => 40,
            };
        }
        if !base.has_custom_appearance() {
            match base.variant {
                TabsVariant::Segmented => {
                    fill_color(&mut base.bar_bg, theme.muted);
                    fill_color(&mut base.bar_border_color, theme.border);
                    if base.bar_pad == 0 {
                        base.bar_pad = 3;
                    }
                    fill_color(&mut base.active_bg, theme.card);
                    fill_color(&mut base.active_fg, theme.foreground);
                    fill_color(&mut base.inactive_fg, theme.muted_foreground);
                    fill_rounded(&mut base.bar_rounded, 8);
                    fill_rounded(&mut base.item_rounded, 6);
                    if base.hover_bg.is_none() {
                        base.hover_bg = Some(theme.accent);
                    }
                    if base.active_press_bg.is_none() {
                        base.active_press_bg = Some(theme.list_active);
                    }
                    if base.active_shadow.is_none() {
                        base.active_shadow = Some((4, 0, 1, Color::rgba(0, 0, 0, 48)));
                    }
                }
                TabsVariant::Underline => {
                    fill_color(&mut base.active_fg, theme.foreground);
                    fill_color(&mut base.inactive_fg, theme.muted_foreground);
                    fill_color(&mut base.underline_active_color, theme.primary);
                    fill_color(&mut base.underline_inactive_color, theme.border);
                    if base.hover_bg.is_none() {
                        base.hover_bg = Some(theme.list_hover);
                    }
                    if base.active_press_bg.is_none() {
                        base.active_press_bg = Some(theme.list_active);
                    }
                }
                TabsVariant::Pill => {
                    fill_color(&mut base.active_bg, theme.primary);
                    fill_color(&mut base.inactive_bg, theme.secondary);
                    fill_color(&mut base.active_fg, theme.primary_foreground);
                    fill_color(&mut base.inactive_fg, theme.secondary_foreground);
                    fill_rounded(&mut base.item_rounded, 16);
                    if base.hover_bg.is_none() {
                        base.hover_bg = Some(theme.accent);
                    }
                    if base.active_press_bg.is_none() {
                        base.active_press_bg = Some(theme.list_active);
                    }
                }
            }
        }
    },
    forward: [
        variant(variant: TabsVariant),
        underline(),
        pill(),
        segmented(),
        h(h: i16),
        on_select(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Tabs, item, (Div<'a>, TabItemState<'a>));

crate::forward_slot_str!(Tabs, label, (usize, &'a str));

impl<'a> Tabs<'a> {
    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，切换页签时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, labels: &'a [&'a str], sig: Signal<usize>) -> Self {
        Self::from_base(velox_base::Tabs::bind(cx, id, labels, sig))
    }
}
