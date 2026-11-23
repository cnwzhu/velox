//! 时间分段输入框组件 `TimeField` 与状态结构 `TimeFieldState`（支持按需回退 `Theme` 样式）。

use velox_core::prelude::*;

/// 小时制式（12 小时制 / 24 小时制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HourCycle {
    H12,
    #[default]
    H24,
}

/// 时间精度（时:分 或 时:分:秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TimePrecision {
    #[default]
    Minute,
    Second,
}

/// 当前激活的时间编辑分段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TimeSegment {
    #[default]
    Hour,
    Minute,
    Second,
    Meridiem,
}

/// 面向 [`TimeField`] 的时间字段状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeFieldState {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub hour_cycle: HourCycle,
    pub precision: TimePrecision,
    pub active_segment: TimeSegment,
}

impl Default for TimeFieldState {
    fn default() -> Self {
        Self::new(9, 0, 0)
    }
}

impl TimeFieldState {
    /// 创建指定 `hour:minute:second`（24 小时制）的时间状态。
    #[inline]
    pub const fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour: if hour < 24 { hour } else { 0 },
            minute: if minute < 60 { minute } else { 0 },
            second: if second < 60 { second } else { 0 },
            hour_cycle: HourCycle::H24,
            precision: TimePrecision::Minute,
            active_segment: TimeSegment::Hour,
        }
    }

    /// 递增当前激活分段的值。
    pub fn increment_active(&mut self) {
        match self.active_segment {
            TimeSegment::Hour => self.hour = (self.hour + 1) % 24,
            TimeSegment::Minute => self.minute = (self.minute + 1) % 60,
            TimeSegment::Second => self.second = (self.second + 1) % 60,
            TimeSegment::Meridiem => self.hour = (self.hour + 12) % 24,
        }
    }

    /// 递减当前激活分段的值。
    pub fn decrement_active(&mut self) {
        match self.active_segment {
            TimeSegment::Hour => self.hour = (self.hour + 23) % 24,
            TimeSegment::Minute => self.minute = (self.minute + 59) % 60,
            TimeSegment::Second => self.second = (self.second + 59) % 60,
            TimeSegment::Meridiem => self.hour = (self.hour + 12) % 24,
        }
    }

    /// 切换到下一个时间分段。
    pub fn next_segment(&mut self) {
        self.active_segment = match (self.active_segment, self.precision, self.hour_cycle) {
            (TimeSegment::Hour, _, _) => TimeSegment::Minute,
            (TimeSegment::Minute, TimePrecision::Second, _) => TimeSegment::Second,
            (TimeSegment::Minute, TimePrecision::Minute, HourCycle::H12) => TimeSegment::Meridiem,
            (TimeSegment::Second, _, HourCycle::H12) => TimeSegment::Meridiem,
            _ => TimeSegment::Hour,
        };
    }

    /// 处理方向键导航与步进调整。
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Up => {
                self.increment_active();
                true
            }
            KeyCode::Down => {
                self.decrement_active();
                true
            }
            KeyCode::Right | KeyCode::Tab => {
                self.next_segment();
                true
            }
            _ => false,
        }
    }
}

/// 面向 [`TimeField::segment`] 插槽回调的时间分段状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeSegmentState {
    pub segment: TimeSegment,
    pub value: u8,
    pub is_active: bool,
    pub is_focused: bool,
}

impl TimeSegmentState {
    #[inline(always)]
    pub const fn segment(&self) -> TimeSegment {
        self.segment
    }

    #[inline(always)]
    pub const fn value(&self) -> u8 {
        self.value
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }

    #[inline(always)]
    pub const fn is_focused(&self) -> bool {
        self.is_focused
    }
}

/// 分段时间输入框组件（`TimeField`）。
pub struct TimeField<'a> {
    pub id: u32,
    pub state: TimeFieldState,
    pub focused: bool,
    pub bg: Color,
    pub border_color: Color,
    pub focus_border_color: Color,
    pub active_segment_bg: Color,
    pub active_segment_fg: Color,
    pub text_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_segment_click: Option<&'a dyn Fn(TimeSegment, &mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    pub segment_renderer:
        Option<&'a dyn Fn(Div<'a>, TimeSegmentState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> TimeField<'a> {
    /// 创建一个无样式分段时间输入框原语。
    #[inline(always)]
    pub const fn new(id: u32, state: TimeFieldState) -> Self {
        Self {
            id,
            state,
            focused: false,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            focus_border_color: Color::TRANSPARENT,
            active_segment_bg: Color::TRANSPARENT,
            active_segment_fg: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_segment_click: None,
            on_key_input: None,
            segment_renderer: None,
        }
    }

    /// 设置焦点激活状态。
    #[inline(always)]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// 绑定点击某个时间分段的回调。
    #[inline(always)]
    pub fn on_segment_click(
        mut self,
        handler: &'a dyn Fn(TimeSegment, &mut ActionScope),
    ) -> Self {
        self.on_segment_click = Some(handler);
        self
    }

    /// 绑定键盘按键回调。
    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// `gpui-base` 风格时间分段插槽回调：自定义每个时/分/秒/AM-PM 片段外观。
    #[inline(always)]
    pub fn segment<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TimeSegmentState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |item: Div<'a>, state: TimeSegmentState, scope: &'a FrameScope| {
                f(item, state, scope).into_element(scope)
            },
        );
        self.segment_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(TimeField<'a>);

impl<'a> Widget<'a> for TimeField<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let focus_border_color = if self.focus_border_color != Color::TRANSPARENT {
            self.focus_border_color
        } else {
            border_color
        };
        let active_segment_bg = self.active_segment_bg;
        let active_segment_fg = if self.active_segment_fg == Color::TRANSPARENT {
            fg
        } else {
            self.active_segment_fg
        };
        let text_color = if self.text_color == Color::TRANSPARENT {
            fg
        } else {
            self.text_color
        };
        let rounded = self.rounded;

        let border_col = if self.focused {
            focus_border_color
        } else {
            border_color
        };

        let mut row = div()
            .id(self.id)
            .flex_row()
            .h(32)
            .px(8)
            .gap(4)
            .rounded(rounded)
            .bg(bg)
            .items_center()
            .focusable();
        if border_col != Color::TRANSPARENT {
            row = row.border(1, border_col);
        }
        if !self.focus_ring {
            row = row.no_focus_border();
        }

        if let Some(kcb) = self.on_key_input {
            row = row.on_key_input(kcb);
        }

        let display_hour = match self.state.hour_cycle {
            HourCycle::H24 => self.state.hour,
            HourCycle::H12 => {
                let h = self.state.hour % 12;
                if h == 0 { 12 } else { h }
            }
        };

        let hour_str = fmt!(cx, "{:02}", display_hour);
        let min_str = fmt!(cx, "{:02}", self.state.minute);
        let seg_r = rounded.saturating_sub(2);

        let make_seg = |seg_id: u32, seg: TimeSegment, val: u8, label: StrRef<'a>| -> Element<'a> {
            let active = self.focused && self.state.active_segment == seg;
            if let Some(renderer) = self.segment_renderer {
                let mut d = div().id(seg_id).child(cx, text(label));
                if let Some(cb) = self.on_segment_click {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(seg, scope);
                    });
                    d = d.on_click_raw(click_cb);
                }
                let seg_state = TimeSegmentState {
                    segment: seg,
                    value: val,
                    is_active: active,
                    is_focused: self.focused,
                };
                return renderer(d, seg_state, cx);
            }
            let mut d = div()
                .id(seg_id)
                .px(4)
                .py(2)
                .rounded(seg_r)
                .bg(if active {
                    active_segment_bg
                } else {
                    Color::TRANSPARENT
                })
                .child(
                    cx,
                    text(label).size(13).color(if active {
                        active_segment_fg
                    } else {
                        text_color
                    }),
                );
            if let Some(cb) = self.on_segment_click {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(seg, scope);
                });
                d = d.on_click_raw(click_cb);
            }
            d.into_element(cx)
        };

        row = row
            .child(cx, make_seg(crate::scoped_child_id(self.id, 0x1C86_C84B, 1), TimeSegment::Hour, display_hour, hour_str))
            .child(cx, text(":").size(13).color(text_color))
            .child(cx, make_seg(crate::scoped_child_id(self.id, 0x1C86_C84B, 2), TimeSegment::Minute, self.state.minute, min_str));

        if self.state.precision == TimePrecision::Second {
            let sec_str = fmt!(cx, "{:02}", self.state.second);
            row = row
                .child(cx, text(":").size(13).color(text_color))
                .child(cx, make_seg(crate::scoped_child_id(self.id, 0x1C86_C84B, 3), TimeSegment::Second, self.state.second, sec_str));
        }

        if self.state.hour_cycle == HourCycle::H12 {
            let ampm = if self.state.hour < 12 { "AM" } else { "PM" };
            let meridiem_val = if self.state.hour < 12 { 0 } else { 1 };
            row = row.child(
                cx,
                make_seg(crate::scoped_child_id(self.id, 0x1C86_C84B, 4), TimeSegment::Meridiem, meridiem_val, StrRef::from(ampm)),
            );
        }

        row = self.style_ext.apply_to_div(row, self.style);
        row.into_element(cx)
    }
}
