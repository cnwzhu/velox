//! 主题化分页导航组件 `Pagination`。

use crate::styled::{fill_color, fill_rounded};
use velox_core::prelude::*;

pub use velox_base::components::pagination::{PaginationItemKind, PaginationItemState};

crate::define_themed_component! {
    name: Pagination<'a>,
    base: velox_base::Pagination<'a>,
    new: (id: u32, current_page: usize, total_pages: usize),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.active_bg, theme.primary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.active_text_color, theme.primary_foreground);
        fill_color(&mut base.disabled_text_color, theme.muted_foreground);
        if base.hover_bg.is_none() {
            base.hover_bg = Some(theme.accent);
        }
        if base.press_bg.is_none() {
            base.press_bg = Some(theme.list_active);
        }
        if base.active_hover_bg.is_none() {
            base.active_hover_bg = Some(base.active_bg.lerp(Color::WHITE, 24));
        }
        if base.active_press_bg.is_none() {
            base.active_press_bg = Some(base.active_bg.lerp(Color::BLACK, 34));
        }
        if base.active_focus_border.is_none() {
            base.active_focus_border = Some((1, base.active_bg.lerp(Color::WHITE, 128)));
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        visible_pages(count: usize),
        on_change(handler: &'a dyn Fn(usize, &mut ActionScope)),
    ],
}

crate::forward_slot_el!(Pagination, item, (Div<'a>, PaginationItemState));

crate::forward_slot_str!(Pagination, label, (PaginationItemKind, usize));

impl<'a> Pagination<'a> {
    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，翻页时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, total_pages: usize, sig: Signal<usize>) -> Self {
        Self::from_base(velox_base::Pagination::bind(cx, id, total_pages, sig))
    }
}
