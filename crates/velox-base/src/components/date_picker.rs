//! 日历弹出日期选择器组件 `DatePicker`（默认回退 `cx.theme()`）。

use velox_core::prelude::*;

pub use crate::components::calendar::{Calendar, DateSelection, SimpleDate};

const AUTO_COLOR: Color = Color(0x0000_0001);

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

type DatePickerTriggerRenderer<'a> = &'a dyn Fn(Div<'a>, bool, &'a FrameScope) -> Element<'a>;

/// 日期选择器触发器与弹出日历组件（`DatePicker`）。
pub struct DatePicker<'a> {
    pub id: u32,
    pub selected: Option<SimpleDate>,
    pub view_year: i32,
    pub view_month: u8,
    pub open: bool,
    pub placeholder: &'a str,
    pub width: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub border_color: Color,
    pub active_border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub popup_bg: Color,
    pub popup_border_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub focus_ring: bool,
    pub rounded: u8,
    pub on_toggle_open: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_select_day: Option<&'a dyn Fn(SimpleDate, &mut ActionScope)>,
    trigger_renderer: Option<DatePickerTriggerRenderer<'a>>,
}

impl<'a> DatePicker<'a> {
    /// 创建一个无样式日期选择器原语。
    #[inline(always)]
    pub const fn new(id: u32, selected: Option<SimpleDate>, open: bool) -> Self {
        let (y, m) = match selected {
            Some(d) => (d.year, d.month),
            None => (2026, 1),
        };
        Self {
            id,
            selected,
            view_year: y,
            view_month: m,
            open,
            placeholder: "Select date...",
            width: 160,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            text_color: AUTO_COLOR,
            muted_color: AUTO_COLOR,
            popup_bg: Color::TRANSPARENT,
            popup_border_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: AUTO_COLOR,
            focus_ring: false,
            rounded: 0,
            on_toggle_open: None,
            on_select_day: None,
            trigger_renderer: None,
        }
    }

    /// 绑定点击展开/收起回调。
    #[inline(always)]
    pub fn on_toggle_open(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_open = Some(handler);
        self
    }

    /// 绑定选择日期回调。
    #[inline(always)]
    pub fn on_select_day(mut self, handler: &'a dyn Fn(SimpleDate, &mut ActionScope)) -> Self {
        self.on_select_day = Some(handler);
        self
    }

    /// `gpui-base` 风格自定义触发器插槽回调（零堆分配）。
    #[inline]
    pub fn trigger<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, bool) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, open: bool, frame_cx: &'a FrameScope| f(d, open).into_element(frame_cx),
        );
        self.trigger_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(DatePicker<'a>);

impl<'a> Widget<'a> for DatePicker<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let active_border_color = if self.active_border_color != Color::TRANSPARENT {
            self.active_border_color
        } else {
            border_color
        };
        let text_color = resolve_color(self.text_color, def_fg);
        let muted_color = resolve_color(self.muted_color, def_fg.scale_alpha(140));
        let popup_bg = self.popup_bg;
        let popup_border_color = self.popup_border_color;
        let selected_bg = self.selected_bg;
        let selected_fg = resolve_color(self.selected_fg, def_fg);

        let (label, is_placeholder): (StrRef<'a>, bool) = match self.selected {
            Some(d) => (fmt!(cx, "{:04}-{:02}-{:02}", d.year, d.month, d.day), false),
            None => (StrRef::from(self.placeholder), true),
        };

        let trig_border = if self.open {
            active_border_color
        } else {
            border_color
        };

        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let mut trigger = div()
            .id(self.id)
            .flex_row()
            .w(eff_w)
            .h(32)
            .px(10)
            .rounded(self.rounded)
            .bg(bg)
            .items_center()
            .justify_between()
            .focusable()
            .child(
                cx,
                text(label)
                    .size(13)
                    .color(if is_placeholder { muted_color } else { text_color }),
            )
            .child(cx, text("v").size(11).color(muted_color));
        if trig_border != Color::TRANSPARENT {
            trigger = trigger.border(1, trig_border);
        }
        if !self.focus_ring {
            trigger = trigger.no_focus_border();
        }

        if let Some(cb) = self.on_toggle_open {
            trigger = trigger.on_click_raw(cb);
        }

        let trigger_el = match self.trigger_renderer {
            Some(f) => f(trigger, self.open, cx),
            None => trigger.into_element(cx),
        };
        let mut wrap = div().flex_col().child(cx, trigger_el);
        if self.open {
            let sel = match self.selected {
                Some(d) => DateSelection::Single(d),
                None => DateSelection::None,
            };
            let mut cal =
                Calendar::new(crate::scoped_child_id(self.id, 0x2F00_0300, 100), self.view_year, self.view_month)
                    .selection(sel);
            cal.bg = popup_bg;
            cal.border_color = popup_border_color;
            cal.text_color = text_color;
            cal.muted_color = muted_color;
            cal.selected_bg = selected_bg;
            cal.selected_fg = selected_fg;
            cal.focus_ring = self.focus_ring;
            cal.rounded = if self.rounded > 0 {
                self.rounded.max(8)
            } else {
                0
            };
            if let Some(cb) = self.on_select_day {
                cal = cal.on_select_day(cb);
            }
            let popup = div()
                .id(self.id ^ 0x4450_5055)
                .pos(0, 36)
                .overlay()
                .occlude_hit()
                .child(cx, cal);
            wrap = wrap.child(cx, popup);
        }

        self.style_ext.apply_to_div(wrap, self.style).into_element(cx)
    }
}
