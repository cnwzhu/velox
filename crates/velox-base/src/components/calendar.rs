//! 无样式日历原语 `Calendar`（支持 `gpui-base` 风格的 `.item(...)` / `.label(...)` 插槽回调、
//! 日/月/年三视图切换、前后月补齐、`today` / `min_date` / `max_date` 状态以及外层 [`Styled`] 链式修饰）。

use velox_engine::prelude::*;

const AUTO_COLOR: Color = Color(0x0000_0001);

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

/// 轻量级公历日期结构（`year-month-day`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SimpleDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Default for SimpleDate {
    fn default() -> Self {
        Self::new(2026, 1, 1)
    }
}

impl SimpleDate {
    /// 创建一个日期。
    #[inline]
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self {
            year,
            month: if month >= 1 && month <= 12 { month } else { 1 },
            day: if day >= 1 && day <= 31 { day } else { 1 },
        }
    }

    /// 判断指定年份是否为闰年。
    #[inline]
    pub const fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    /// 获取指定年月的总天数（28..=31）。
    #[inline]
    pub const fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    /// 使用 Sakamoto 算法计算该日期是星期几（`0 = 周日`, `1 = 周一`, ..., `6 = 周六`）。
    pub const fn weekday(&self) -> u8 {
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let mut y = self.year;
        let m = self.month as usize;
        if m < 3 {
            y -= 1;
        }
        let w = (y + y / 4 - y / 100 + y / 400 + T[(m - 1) % 12] + self.day as i32) % 7;
        ((w + 7) % 7) as u8
    }

    /// 切换到上一个月。
    #[inline]
    pub const fn prev_month(&self) -> Self {
        if self.month <= 1 {
            Self::new(self.year - 1, 12, 1)
        } else {
            Self::new(self.year, self.month - 1, 1)
        }
    }

    /// 切换到下一个月。
    #[inline]
    pub const fn next_month(&self) -> Self {
        if self.month >= 12 {
            Self::new(self.year + 1, 1, 1)
        } else {
            Self::new(self.year, self.month + 1, 1)
        }
    }
}

/// 日历选区模式（单日选中或起止日期区间选中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DateSelection {
    #[default]
    None,
    Single(SimpleDate),
    Range {
        start: SimpleDate,
        end: SimpleDate,
    },
}

impl DateSelection {
    /// 判断某一天是否处于选中状态或区间内部。
    #[inline]
    pub fn contains(&self, date: SimpleDate) -> bool {
        match *self {
            Self::None => false,
            Self::Single(d) => d == date,
            Self::Range { start, end } => date >= start && date <= end,
        }
    }

    /// 判断某一天是否为单日选中或区间的起止端点。
    #[inline]
    pub fn is_endpoint(&self, date: SimpleDate) -> bool {
        match *self {
            Self::None => false,
            Self::Single(d) => d == date,
            Self::Range { start, end } => date == start || date == end,
        }
    }
}

/// 日历当前展示的层级视图模式（日视图 / 月视图 / 年视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CalendarViewMode {
    #[default]
    Days,
    Months,
    Years,
}

/// 日历插槽单元格类型（对齐 `gpui-base::CalendarItemKind`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarItemKind {
    Previous,
    Next,
    MonthToggle,
    YearToggle,
    Weekday,
    Day,
    Month,
    Year,
}

/// 传递给 `.item(...)` 插槽渲染回调的单元格状态描述（对齐 `gpui-base::CalendarItemState`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarItemState {
    kind: CalendarItemKind,
    value: i32,
    date: Option<SimpleDate>,
    is_today: bool,
    is_active: bool,
    is_in_range: bool,
    is_muted: bool,
    is_disabled: bool,
}

impl CalendarItemState {
    #[inline(always)]
    pub const fn kind(&self) -> CalendarItemKind {
        self.kind
    }

    /// 返回当前单元格对应的数值（日：`1..=31`，月：`1..=12`，年：如 `2026`，星期：`0..=6`，前后翻页为 `0`）。
    #[inline(always)]
    pub const fn value(&self) -> i32 {
        self.value
    }

    /// 若为日期格（`CalendarItemKind::Day`），返回其完整 [`SimpleDate`]。
    #[inline(always)]
    pub const fn date(&self) -> Option<SimpleDate> {
        self.date
    }

    #[inline(always)]
    pub const fn is_today(&self) -> bool {
        self.is_today
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }

    #[inline(always)]
    pub const fn is_in_range(&self) -> bool {
        self.is_in_range
    }

    /// 是否为前后月补齐的弱化日期。
    #[inline(always)]
    pub const fn is_muted(&self) -> bool {
        self.is_muted
    }

    #[inline(always)]
    pub const fn is_disabled(&self) -> bool {
        self.is_disabled
    }
}

/// 日历持久化受控状态（配合 [`Calendar::from_state`] 使用，一行驱动日/月/年切换与日期选择）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarState {
    pub view_year: i32,
    pub view_month: u8,
    pub view_mode: CalendarViewMode,
    pub selection: DateSelection,
    pub today: Option<SimpleDate>,
    pub min_date: Option<SimpleDate>,
    pub max_date: Option<SimpleDate>,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self::new(2026, 11)
    }
}

impl CalendarState {
    #[inline]
    pub const fn new(year: i32, month: u8) -> Self {
        Self {
            view_year: year,
            view_month: if month >= 1 && month <= 12 { month } else { 1 },
            view_mode: CalendarViewMode::Days,
            selection: DateSelection::None,
            today: None,
            min_date: None,
            max_date: None,
        }
    }

    #[inline]
    pub const fn with_today(mut self, today: SimpleDate) -> Self {
        self.today = Some(today);
        self
    }

    #[inline]
    pub const fn with_selection(mut self, selection: DateSelection) -> Self {
        self.selection = selection;
        self
    }

    /// 按当前视图模式向前翻页（日视图：上一月；月视图：上一年；年视图：上 12 年）。
    #[inline]
    pub fn prev(&mut self) {
        match self.view_mode {
            CalendarViewMode::Days => {
                let p = SimpleDate::new(self.view_year, self.view_month, 1).prev_month();
                self.view_year = p.year;
                self.view_month = p.month;
            }
            CalendarViewMode::Months => {
                self.view_year = self.view_year.saturating_sub(1);
            }
            CalendarViewMode::Years => {
                self.view_year = self.view_year.saturating_sub(12);
            }
        }
    }

    /// 按当前视图模式向后翻页（日视图：下一月；月视图：下一年；年视图：下 12 年）。
    #[inline]
    pub fn next(&mut self) {
        match self.view_mode {
            CalendarViewMode::Days => {
                let n = SimpleDate::new(self.view_year, self.view_month, 1).next_month();
                self.view_year = n.year;
                self.view_month = n.month;
            }
            CalendarViewMode::Months => {
                self.view_year = self.view_year.saturating_add(1);
            }
            CalendarViewMode::Years => {
                self.view_year = self.view_year.saturating_add(12);
            }
        }
    }

    /// 切换月份选择视图。
    #[inline]
    pub fn toggle_months(&mut self) {
        self.view_mode = if self.view_mode == CalendarViewMode::Months {
            CalendarViewMode::Days
        } else {
            CalendarViewMode::Months
        };
    }

    /// 切换年份选择视图。
    #[inline]
    pub fn toggle_years(&mut self) {
        self.view_mode = if self.view_mode == CalendarViewMode::Years {
            CalendarViewMode::Days
        } else {
            CalendarViewMode::Years
        };
    }

    /// 选中某一天（并自动跳转视图到该日期所在年月）。
    #[inline]
    pub fn select_day(&mut self, date: SimpleDate) {
        self.selection = DateSelection::Single(date);
        self.view_year = date.year;
        self.view_month = date.month;
        self.view_mode = CalendarViewMode::Days;
    }

    /// 在月视图选中月份并切回日视图。
    #[inline]
    pub fn select_month(&mut self, month: u8) {
        self.view_month = month.clamp(1, 12);
        self.view_mode = CalendarViewMode::Days;
    }

    /// 在年视图选中年份并切回日视图。
    #[inline]
    pub fn select_year(&mut self, year: i32) {
        self.view_year = year;
        self.view_mode = CalendarViewMode::Days;
    }
}

type CalendarItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, CalendarItemState, &'a FrameScope) -> Element<'a>;
type CalendarLabelFormatter<'a> = &'a dyn Fn(CalendarItemKind, i32) -> StrRef<'a>;

/// 无样式月历网格原语（`Calendar`）。
///
/// 支持两种用法：
/// 1. **`gpui-base` 插槽风格**：直接链式调用 [`Styled`]（`.w(px(250.)).p_3().border_1().border_color(...)`）
///    配合 `.item(cx, |item, state| ...)` 与 `.label(cx, |kind, val| ...)` 完整掌控每一类单元格外观；
/// 2. **字段配置风格**：兼容 `velox-component::Calendar` 与常规字段覆写。
pub struct Calendar<'a> {
    pub id: u32,
    pub view_year: i32,
    pub view_month: u8,
    pub view_mode: CalendarViewMode,
    pub selection: DateSelection,
    pub today: Option<SimpleDate>,
    pub min_date: Option<SimpleDate>,
    pub max_date: Option<SimpleDate>,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub focus_ring: bool,
    pub rounded: u8,
    pub on_select_day: Option<&'a dyn Fn(SimpleDate, &mut ActionScope)>,
    pub on_select_month: Option<&'a dyn Fn(u8, &mut ActionScope)>,
    pub on_select_year: Option<&'a dyn Fn(i32, &mut ActionScope)>,
    pub on_toggle_month_view: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_toggle_year_view: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_prev_month: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_next_month: Option<&'a dyn Fn(&mut ActionScope)>,
    item_renderer: Option<CalendarItemRenderer<'a>>,
    label_formatter: Option<CalendarLabelFormatter<'a>>,
}

impl<'a> Calendar<'a> {
    /// 创建一个无样式月历网格原语。
    #[inline(always)]
    pub const fn new(id: u32, view_year: i32, view_month: u8) -> Self {
        Self {
            id,
            view_year,
            view_month,
            view_mode: CalendarViewMode::Days,
            selection: DateSelection::None,
            today: None,
            min_date: None,
            max_date: None,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 1, // Col
            },
            style_ext: StyleExt::DEFAULT,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: AUTO_COLOR,
            muted_color: AUTO_COLOR,
            selected_bg: Color::TRANSPARENT,
            selected_fg: AUTO_COLOR,
            focus_ring: false,
            rounded: 0,
            on_select_day: None,
            on_select_month: None,
            on_select_year: None,
            on_toggle_month_view: None,
            on_toggle_year_view: None,
            on_prev_month: None,
            on_next_month: None,
            item_renderer: None,
            label_formatter: None,
        }
    }

    /// 从持久化 [`CalendarState`] 快速构造日历原语。
    #[inline(always)]
    pub const fn from_state(id: u32, state: &CalendarState) -> Self {
        let mut c = Self::new(id, state.view_year, state.view_month);
        c.view_mode = state.view_mode;
        c.selection = state.selection;
        c.today = state.today;
        c.min_date = state.min_date;
        c.max_date = state.max_date;
        c
    }

    /// 设置当前日期选中态。
    #[inline(always)]
    pub const fn selection(mut self, selection: DateSelection) -> Self {
        self.selection = selection;
        self
    }

    /// 设置当前展示的视图模式（日 / 月 / 年）。
    #[inline(always)]
    pub const fn view_mode(mut self, mode: CalendarViewMode) -> Self {
        self.view_mode = mode;
        self
    }

    /// 设置“今天”日期（供 `.item(...)` 中 `state.is_today()` 判断）。
    #[inline(always)]
    pub const fn today(mut self, today: SimpleDate) -> Self {
        self.today = Some(today);
        self
    }

    /// 设置可选日期范围限制（超出范围的日期标记 `state.is_disabled() == true` 且不可点击）。
    #[inline(always)]
    pub const fn date_bounds(mut self, min: Option<SimpleDate>, max: Option<SimpleDate>) -> Self {
        self.min_date = min;
        self.max_date = max;
        self
    }

    /// 绑定点击某一天的回调。
    #[inline(always)]
    pub fn on_select_day(mut self, handler: &'a dyn Fn(SimpleDate, &mut ActionScope)) -> Self {
        self.on_select_day = Some(handler);
        self
    }

    /// 绑定在月视图选中某个月份的回调（`1..=12`）。
    #[inline(always)]
    pub fn on_select_month(mut self, handler: &'a dyn Fn(u8, &mut ActionScope)) -> Self {
        self.on_select_month = Some(handler);
        self
    }

    /// 绑定在年视图选中某个年份的回调。
    #[inline(always)]
    pub fn on_select_year(mut self, handler: &'a dyn Fn(i32, &mut ActionScope)) -> Self {
        self.on_select_year = Some(handler);
        self
    }

    /// 绑定点击头部月份按钮切换月视图的回调。
    #[inline(always)]
    pub fn on_toggle_month_view(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_month_view = Some(handler);
        self
    }

    /// 绑定点击头部年份按钮切换年视图的回调。
    #[inline(always)]
    pub fn on_toggle_year_view(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_year_view = Some(handler);
        self
    }

    /// 绑定切换到上一页（上一月 / 上一年 / 上 12 年）的回调。
    #[inline(always)]
    pub fn on_prev_month(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_prev_month = Some(handler);
        self
    }

    /// 绑定切换到下一页（下一月 / 下一年 / 下 12 年）的回调。
    #[inline(always)]
    pub fn on_next_month(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_next_month = Some(handler);
        self
    }

    /// `gpui-base` 风格插槽渲染回调：接收每个单元格的基础 `Div<'a>` 与 [`CalendarItemState`]，
    /// 由调用方通过 `.size_8()` / `.when(...)` / `.hover(|s| ...)` / `.text_xs()` 自由定制外观（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, CalendarItemState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |item: Div<'a>, state: CalendarItemState, frame_cx: &'a FrameScope| {
                f(item, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格自定义文案格式化回调（覆盖箭头、月份、星期、日期/年份文字，零堆分配）。
    #[inline]
    pub fn label<F, S>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(CalendarItemKind, i32) -> S + 'a,
        S: Into<StrRef<'a>>,
    {
        let cb = cx
            .arena()
            .alloc(move |kind: CalendarItemKind, value: i32| f(kind, value).into());
        self.label_formatter = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Calendar<'a>);

impl<'a> Widget<'a> for Calendar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let text_color = resolve_color(self.text_color, def_fg);
        let muted_color = resolve_color(self.muted_color, def_fg.scale_alpha(140));
        let selected_bg = self.selected_bg;
        let selected_fg = resolve_color(self.selected_fg, def_fg);

        let has_custom_style = !matches!(self.style.width, Length::Auto)
            || self.style.padding != Edges::all(0)
            || self.style.bg != Color::TRANSPARENT
            || self.style.border_px() > 0
            || self.style.rounded_radius() > 0;

        let mut root = div().id(self.id);
        root.style = self.style;
        root.style.set_col();
        if matches!(root.style.width, Length::Auto) && self.item_renderer.is_none() {
            root = root.w(226);
        }
        if root.style.padding == Edges::all(0) && !has_custom_style {
            root = root.p(8);
        }
        if root.style.gap == 0 {
            root = root.gap(4);
        }
        if root.style.bg == Color::TRANSPARENT && self.bg != Color::TRANSPARENT {
            root = root.bg(self.bg);
        }
        if root.style.border_px() == 0 && self.border_color != Color::TRANSPARENT {
            root = root.border(1, self.border_color);
        }
        if root.style.rounded_radius() == 0 && self.rounded > 0 {
            root = root.rounded(self.rounded);
        }
        if self.item_renderer.is_some() {
            root = root.text_color(text_color).text_size(12);
        }
        root = self.style_ext.apply_ext_only(root);

        const WEEKDAY_KEYS: [&str; 7] = [
            "velox.calendar.sun",
            "velox.calendar.mon",
            "velox.calendar.tue",
            "velox.calendar.wed",
            "velox.calendar.thu",
            "velox.calendar.fri",
            "velox.calendar.sat",
        ];
        const SHORT_MONTHS: [&str; 13] = [
            "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];

        let format_label = |kind: CalendarItemKind, value: i32| -> StrRef<'a> {
            if let Some(fmt_fn) = self.label_formatter {
                return fmt_fn(kind, value);
            }
            match kind {
                CalendarItemKind::Previous => StrRef::from("<"),
                CalendarItemKind::Next => StrRef::from(">"),
                CalendarItemKind::MonthToggle | CalendarItemKind::Month => {
                    let idx = (value as usize).clamp(1, 12);
                    StrRef::from(SHORT_MONTHS[idx])
                }
                CalendarItemKind::YearToggle | CalendarItemKind::Year | CalendarItemKind::Day => {
                    fmt!(cx, "{}", value)
                }
                CalendarItemKind::Weekday => {
                    let idx = (value as usize) % 7;
                    crate::i18n::widget_t(cx, WEEKDAY_KEYS[idx])
                }
            }
        };

        let is_date_disabled = |date: SimpleDate| -> bool {
            if let Some(min_d) = self.min_date {
                if date < min_d {
                    return true;
                }
            }
            if let Some(max_d) = self.max_date {
                if date > max_d {
                    return true;
                }
            }
            false
        };

        let cell_rounded = root.style.rounded_radius().saturating_sub(2);
        let item_renderer = self.item_renderer;
        let focus_ring = self.focus_ring;

        // 统一将每个插槽节点通过自定义 `.item(...)` 或默认无样式布局渲染
        let render_slot = |mut base_div: Div<'a>,
                           label: StrRef<'a>,
                           state: CalendarItemState|
         -> Element<'a> {
            if let Some(render_fn) = item_renderer {
                // 使用未显式锁定颜色/字号的 `text(label)`，使 `.item(...)` 中的 `.text_xs().text_color(...)` 自动继承
                base_div = base_div.child(cx, text(label).text_center().nowrap());
                render_fn(base_div, state, cx)
            } else {
                // 经典无样式默认几何排版（完整兼容 `velox-component::Calendar` 与已有测试）
                let styled = match state.kind {
                    CalendarItemKind::Previous | CalendarItemKind::Next => base_div
                        .w(24)
                        .h(24)
                        .rounded(cell_rounded)
                        .items_center()
                        .justify_center()
                        .child(cx, text(label).size(12).color(text_color)),
                    CalendarItemKind::MonthToggle | CalendarItemKind::YearToggle => base_div
                        .px(4)
                        .h(24)
                        .rounded(cell_rounded)
                        .items_center()
                        .justify_center()
                        .child(cx, text(label).size(13).color(text_color)),
                    CalendarItemKind::Weekday => base_div
                        .w(30)
                        .h(22)
                        .items_center()
                        .justify_center()
                        .child(cx, text(label).size(11).color(muted_color)),
                    CalendarItemKind::Day => base_div
                        .w(30)
                        .h(26)
                        .rounded(cell_rounded)
                        .bg(if state.is_active {
                            selected_bg
                        } else {
                            Color::TRANSPARENT
                        })
                        .items_center()
                        .justify_center()
                        .child(
                            cx,
                            text(label).size(12).color(if state.is_active {
                                selected_fg
                            } else if state.is_muted || state.is_disabled {
                                muted_color
                            } else {
                                text_color
                            }),
                        ),
                    CalendarItemKind::Month | CalendarItemKind::Year => base_div
                        .w(66)
                        .h(28)
                        .rounded(cell_rounded)
                        .bg(if state.is_active {
                            selected_bg
                        } else {
                            Color::TRANSPARENT
                        })
                        .items_center()
                        .justify_center()
                        .child(
                            cx,
                            text(label).size(12).color(if state.is_active {
                                selected_fg
                            } else {
                                text_color
                            }),
                        ),
                };
                styled.into_element(cx)
            }
        };

        // 1. 顶部导航栏：Previous | (MonthToggle + YearToggle 或 YYYY-MM) | Next
        let mut prev_div = div()
            .id(crate::scoped_child_id(self.id, 0xA481_0D1F, 1))
            .focusable();
        if !focus_ring {
            prev_div = prev_div.no_focus_border();
        }
        if let Some(cb) = self.on_prev_month {
            prev_div = prev_div.on_click_raw(cb);
        }
        let prev_el = render_slot(
            prev_div,
            format_label(CalendarItemKind::Previous, 0),
            CalendarItemState {
                kind: CalendarItemKind::Previous,
                value: 0,
                date: None,
                is_today: false,
                is_active: false,
                is_in_range: false,
                is_muted: false,
                is_disabled: false,
            },
        );

        let mut next_div = div()
            .id(crate::scoped_child_id(self.id, 0xA481_0D1F, 2))
            .focusable();
        if !focus_ring {
            next_div = next_div.no_focus_border();
        }
        if let Some(cb) = self.on_next_month {
            next_div = next_div.on_click_raw(cb);
        }
        let next_el = render_slot(
            next_div,
            format_label(CalendarItemKind::Next, 0),
            CalendarItemState {
                kind: CalendarItemKind::Next,
                value: 0,
                date: None,
                is_today: false,
                is_active: false,
                is_in_range: false,
                is_muted: false,
                is_disabled: false,
            },
        );

        let center_el: Element<'a> = if item_renderer.is_some()
            || self.label_formatter.is_some()
            || self.on_toggle_month_view.is_some()
            || self.on_toggle_year_view.is_some()
            || self.view_mode != CalendarViewMode::Days
        {
            let mut m_div = div()
                .id(crate::scoped_child_id(self.id, 0xA481_0D1F, 3))
                .focusable();
            if !focus_ring {
                m_div = m_div.no_focus_border();
            }
            if let Some(cb) = self.on_toggle_month_view {
                m_div = m_div.on_click_raw(cb);
            }
            let m_el = render_slot(
                m_div,
                format_label(CalendarItemKind::MonthToggle, self.view_month as i32),
                CalendarItemState {
                    kind: CalendarItemKind::MonthToggle,
                    value: self.view_month as i32,
                    date: None,
                    is_today: false,
                    is_active: self.view_mode == CalendarViewMode::Months,
                    is_in_range: false,
                    is_muted: false,
                    is_disabled: false,
                },
            );

            let mut y_div = div()
                .id(crate::scoped_child_id(self.id, 0xA481_0D1F, 4))
                .focusable();
            if !focus_ring {
                y_div = y_div.no_focus_border();
            }
            if let Some(cb) = self.on_toggle_year_view {
                y_div = y_div.on_click_raw(cb);
            }
            let y_el = render_slot(
                y_div,
                format_label(CalendarItemKind::YearToggle, self.view_year),
                CalendarItemState {
                    kind: CalendarItemKind::YearToggle,
                    value: self.view_year,
                    date: None,
                    is_today: false,
                    is_active: self.view_mode == CalendarViewMode::Years,
                    is_in_range: false,
                    is_muted: false,
                    is_disabled: false,
                },
            );

            div()
                .flex_row()
                .gap(4)
                .items_center()
                .child(cx, m_el)
                .child(cx, y_el)
                .into_element(cx)
        } else {
            let title_str = fmt!(cx, "{:04}-{:02}", self.view_year, self.view_month);
            text(title_str).size(13).color(text_color).into_element(cx)
        };

        let header = div()
            .flex_row()
            .w_full()
            .items_center()
            .justify_between()
            .child(cx, prev_el)
            .child(cx, center_el)
            .child(cx, next_el);
        root = root.child(cx, header);

        // 2. 主体区域：Days（7 列 Grid）/ Months（3 列 Grid）/ Years（3 列 Grid）
        match self.view_mode {
            CalendarViewMode::Days => {
                let mut grid = div().w_full().grid(7);
                for wd_idx in 0..7i32 {
                    let wd_el = render_slot(
                        div(),
                        format_label(CalendarItemKind::Weekday, wd_idx),
                        CalendarItemState {
                            kind: CalendarItemKind::Weekday,
                            value: wd_idx,
                            date: None,
                            is_today: false,
                            is_active: false,
                            is_in_range: false,
                            is_muted: false,
                            is_disabled: false,
                        },
                    );
                    grid = grid.child(cx, wd_el);
                }

                let first_day = SimpleDate::new(self.view_year, self.view_month, 1);
                let first_weekday = first_day.weekday() as i16;
                let total_days = SimpleDate::days_in_month(self.view_year, self.view_month) as i16;
                let prev_m = first_day.prev_month();
                let prev_days = SimpleDate::days_in_month(prev_m.year, prev_m.month) as i16;
                let next_m = first_day.next_month();

                let mut day_counter: i16 = 1 - first_weekday;
                let total_weeks = if item_renderer.is_some() {
                    6
                } else {
                    (((first_weekday + total_days) + 6) / 7).clamp(1, 6)
                };
                for week in 0..total_weeks {
                    for col in 0..7 {
                        let (cur_date, is_muted) = if day_counter < 1 {
                            (
                                SimpleDate::new(
                                    prev_m.year,
                                    prev_m.month,
                                    (prev_days + day_counter) as u8,
                                ),
                                true,
                            )
                        } else if day_counter > total_days {
                            (
                                SimpleDate::new(
                                    next_m.year,
                                    next_m.month,
                                    (day_counter - total_days) as u8,
                                ),
                                true,
                            )
                        } else {
                            (
                                SimpleDate::new(self.view_year, self.view_month, day_counter as u8),
                                false,
                            )
                        };

                        // 经典无样式路径保持非本月空位留白（避免多分配 11 个无用节点），插槽路径完整展示前后月 muted 日期
                        if is_muted && item_renderer.is_none() {
                            grid = grid.child(cx, div().w(30).h(26));
                        } else {
                            let is_active = self.selection.is_endpoint(cur_date);
                            let is_in_range = self.selection.contains(cur_date);
                            let is_today = self.today == Some(cur_date);
                            let is_disabled = is_date_disabled(cur_date);

                            let cell_idx = (week * 7 + col) as u32;
                            let mut cell = div().id(crate::scoped_child_id(
                                self.id,
                                0xA481_0D1F,
                                10 + cell_idx,
                            ));
                            if is_disabled {
                                cell = cell.cursor(CursorShape::NotAllowed);
                            } else {
                                cell = cell.focusable();
                                if !focus_ring {
                                    cell = cell.no_focus_border();
                                }
                                if let Some(cb) = self.on_select_day {
                                    let click_cb =
                                        cx.arena().alloc(move |scope: &mut ActionScope| {
                                            cb(cur_date, scope);
                                        });
                                    cell = cell.on_click_raw(click_cb);
                                }
                            }

                            let cell_el = render_slot(
                                cell,
                                format_label(CalendarItemKind::Day, cur_date.day as i32),
                                CalendarItemState {
                                    kind: CalendarItemKind::Day,
                                    value: cur_date.day as i32,
                                    date: Some(cur_date),
                                    is_today,
                                    is_active,
                                    is_in_range,
                                    is_muted,
                                    is_disabled,
                                },
                            );
                            grid = grid.child(cx, cell_el);
                        }
                        day_counter += 1;
                    }
                }
                root = root.child(cx, grid);
            }
            CalendarViewMode::Months => {
                let mut grid = div().w_full().grid(3).gap(2);
                for m in 1..=12u8 {
                    let mut cell = div()
                        .id(crate::scoped_child_id(self.id, 0xA481_0D1F, 100 + m as u32))
                        .focusable();
                    if !focus_ring {
                        cell = cell.no_focus_border();
                    }
                    if let Some(cb) = self.on_select_month {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(m, scope);
                        });
                        cell = cell.on_click_raw(click_cb);
                    }
                    let el = render_slot(
                        cell,
                        format_label(CalendarItemKind::Month, m as i32),
                        CalendarItemState {
                            kind: CalendarItemKind::Month,
                            value: m as i32,
                            date: None,
                            is_today: self.today.is_some_and(|t| {
                                t.year == self.view_year && t.month == m
                            }),
                            is_active: self.view_month == m,
                            is_in_range: false,
                            is_muted: false,
                            is_disabled: false,
                        },
                    );
                    grid = grid.child(cx, el);
                }
                root = root.child(cx, grid);
            }
            CalendarViewMode::Years => {
                let start_year = self.view_year - (self.view_year.rem_euclid(12));
                let mut grid = div().w_full().grid(3).gap(2);
                for idx in 0..12i32 {
                    let yr = start_year + idx;
                    let mut cell = div()
                        .id(crate::scoped_child_id(
                            self.id,
                            0xA481_0D1F,
                            200 + idx as u32,
                        ))
                        .focusable();
                    if !focus_ring {
                        cell = cell.no_focus_border();
                    }
                    if let Some(cb) = self.on_select_year {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(yr, scope);
                        });
                        cell = cell.on_click_raw(click_cb);
                    }
                    let el = render_slot(
                        cell,
                        format_label(CalendarItemKind::Year, yr),
                        CalendarItemState {
                            kind: CalendarItemKind::Year,
                            value: yr,
                            date: None,
                            is_today: self.today.is_some_and(|t| t.year == yr),
                            is_active: self.view_year == yr,
                            is_in_range: false,
                            is_muted: false,
                            is_disabled: false,
                        },
                    );
                    grid = grid.child(cx, el);
                }
                root = root.child(cx, grid);
            }
        }

        root.into_element(cx)
    }
}
