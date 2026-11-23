//! 主题化日历组件 `Calendar`（支持 `Calendar::from_state` 与 `gpui-base` 插槽回调）。

use crate::styled::{fill_color, fill_rounded};
use velox_base::components::date_picker::{DateSelection, SimpleDate};
use velox_core::prelude::*;

pub use velox_base::components::calendar::{
    CalendarItemKind, CalendarItemState, CalendarState, CalendarViewMode,
};

crate::define_themed_component! {
    name: Calendar<'a>,
    base: velox_base::Calendar<'a>,
    new: (id: u32, view_year: i32, view_month: u8),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.muted_color, theme.muted_foreground);
        fill_color(&mut base.selected_bg, theme.primary);
        fill_color(&mut base.selected_fg, theme.primary_foreground);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        selection(selection: DateSelection),
        on_select_day(handler: &'a dyn Fn(SimpleDate, &mut ActionScope)),
        on_prev_month(handler: &'a dyn Fn(&mut ActionScope)),
        on_next_month(handler: &'a dyn Fn(&mut ActionScope)),
    ],
}

impl<'a> Calendar<'a> {
    /// 从持久化 [`CalendarState`] 快速构造主题化日历组件。
    #[inline(always)]
    pub fn from_state(id: u32, state: &CalendarState) -> Self {
        Self::from_base(velox_base::Calendar::from_state(id, state))
    }

    #[inline(always)]
    pub fn view_mode(mut self, mode: CalendarViewMode) -> Self {
        self.inner = self.inner.view_mode(mode);
        self
    }

    #[inline(always)]
    pub fn today(mut self, today: SimpleDate) -> Self {
        self.inner = self.inner.today(today);
        self
    }

    #[inline(always)]
    pub fn date_bounds(mut self, min: Option<SimpleDate>, max: Option<SimpleDate>) -> Self {
        self.inner = self.inner.date_bounds(min, max);
        self
    }

    #[inline(always)]
    pub fn on_select_month(mut self, handler: &'a dyn Fn(u8, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_select_month(handler);
        self
    }

    #[inline(always)]
    pub fn on_select_year(mut self, handler: &'a dyn Fn(i32, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_select_year(handler);
        self
    }

    #[inline(always)]
    pub fn on_toggle_month_view(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.inner = self.inner.on_toggle_month_view(handler);
        self
    }

    #[inline(always)]
    pub fn on_toggle_year_view(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.inner = self.inner.on_toggle_year_view(handler);
        self
    }
}

crate::forward_slot_el!(Calendar, item, (Div<'a>, CalendarItemState));

crate::forward_slot_str!(Calendar, label, (CalendarItemKind, i32));
