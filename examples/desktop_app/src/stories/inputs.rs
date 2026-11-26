//! `Inputs & Select` 故事页：完全基于 `velox-component` 的 `TextInput`、`NumberInput`、`Select`、`Dropdown`、
//! `OtpInput`、`ColorPicker` 与 `Calendar` 插槽定制示例。

use super::{state_pill, story_header, story_section};
use crate::{ContextMenuTarget, GalleryApp};
use velox_kit::component::{
    Calendar, CalendarItemKind, ColorPicker, DateSelection, Dropdown, NumberInput, OtpInput,
    Select, SelectOption, SimpleDate, TextInput,
};
use velox_kit::prelude::*;

const SELECT_FRAMEWORKS: &[SelectOption<'static>] = &[
    SelectOption::new("Velox Engine (2D SDF GPU)", "velox"),
    SelectOption::new("Velox Component (75+ Widgets)", "velox-component"),
    SelectOption::new("Legacy Immediate Mode (Disabled)", "legacy").disabled(true),
    SelectOption::new("WASM Canvas2D Runtime", "wasm"),
];

const DROPDOWN_BACKENDS: &[&str] = &[
    "wgpu 2D SDF (GPU)",
    "Built-in Software SDF",
    "Pure Wayland SHM",
];

impl GalleryApp {
    pub(crate) fn render_story_inputs<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let on_focus_name = cx.action(|this, acx| {
            this.active_input_field = 0;
            this.input_ctx_menu = None;
            this.text_input_name.reset_blink(acx.now_ms());
        });
        let on_focus_pass = cx.action(|this, acx| {
            this.active_input_field = 1;
            this.input_ctx_menu = None;
            this.text_input_pass.reset_blink(acx.now_ms());
        });
        let on_key_name = cx.listener(|this, key: KeyCode, acx| {
            let _ = this.text_input_name.handle_key_with_mods(
                key,
                acx.modifiers(),
                false,
                acx.now_ms(),
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ptr_name = cx.listener(
            |this, (x, is_drag, now_ms, shift): (i16, bool, u32, bool), _acx| {
                this.active_input_field = 0;
                this.input_ctx_menu = None;
                this.text_input_name
                    .handle_pointer_action(x, is_drag, now_ms, shift, false, 13);
            },
        );
        let on_sec_name = cx.listener(|this, (wx, wy): (i16, i16), acx| {
            this.active_input_field = 0;
            this.text_input_name.reset_blink(acx.now_ms());
            this.input_ctx_menu = Some((ContextMenuTarget::NameInput, wx, wy));
        });
        let on_clear_name = cx.action(|this, acx| {
            this.active_input_field = 0;
            this.text_input_name.clear();
            this.text_input_name.reset_blink(acx.now_ms());
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_key_pass = cx.listener(|this, key: KeyCode, acx| {
            let is_masked = !this.password_visible;
            let _ = this.text_input_pass.handle_key_with_mods(
                key,
                acx.modifiers(),
                is_masked,
                acx.now_ms(),
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ptr_pass = cx.listener(
            |this, (x, is_drag, now_ms, shift): (i16, bool, u32, bool), _acx| {
                this.active_input_field = 1;
                this.input_ctx_menu = None;
                let is_masked = !this.password_visible;
                this.text_input_pass
                    .handle_pointer_action(x, is_drag, now_ms, shift, is_masked, 13);
            },
        );
        let on_toggle_pass_mask = cx.action(|this, acx| {
            this.password_visible = !this.password_visible;
            this.active_input_field = 1;
            this.text_input_pass.reset_blink(acx.now_ms());
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sec_pass = cx.listener(|this, (wx, wy): (i16, i16), acx| {
            this.active_input_field = 1;
            this.text_input_pass.reset_blink(acx.now_ms());
            this.input_ctx_menu = Some((ContextMenuTarget::PasswordInput, wx, wy));
        });
        let on_num_change = cx.listener(|this, val: i64, _acx| {
            this.number_val = val;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_select = cx.action(|this, _acx| {
            this.select_open = !this.select_open;
            this.dropdown_open = false;
            this.input_ctx_menu = None;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_select_opt = cx.listener(|this, idx: usize, _acx| {
            this.select_idx = Some(idx);
            this.select_open = false;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_dd = cx.action(|this, _acx| {
            this.dropdown_open = !this.dropdown_open;
            this.select_open = false;
            this.input_ctx_menu = None;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_select_dd = cx.listener(|this, idx: usize, _acx| {
            this.dropdown_idx = idx;
            this.dropdown_open = false;
            this.event_count = this.event_count.saturating_add(1);
        });

        let name_str = cx.arena().alloc_str(self.text_input_name.as_str()).as_str();
        let pass_str = cx.arena().alloc_str(self.text_input_pass.as_str()).as_str();

        let text_section = story_section(
            cx,
            theme,
            "TextInput & NumberInput",
            "Drag to select • 2× Word / 3× All • Right-click or Ctrl+C/X/V for clipboard",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(14)
                .items_end()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(
                            cx,
                            div().h(16).flex_row().items_center().child(
                                cx,
                                text("Clearable Input")
                                    .size(12)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            ),
                        )
                        .child(
                            cx,
                            TextInput::new(1401, name_str, self.active_input_field == 0)
                                .size(220, 32)
                                .placeholder("Type project name...")
                                .cursor(self.text_input_name.cursor())
                                .selection(self.text_input_name.selected_range())
                                .blink_epoch(self.text_input_name.blink_epoch_ms())
                                .clearable(true)
                                .on_focus(on_focus_name)
                                .on_clear(on_clear_name)
                                .on_key_input(on_key_name)
                                .on_pointer_action(on_ptr_name)
                                .on_secondary_click(on_sec_name),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(
                            cx,
                            div().h(16).flex_row().items_center().child(
                                cx,
                                text("Password Masked")
                                    .size(12)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            ),
                        )
                        .child(
                            cx,
                            TextInput::new(1402, pass_str, self.active_input_field == 1)
                                .size(200, 32)
                                .masked(!self.password_visible)
                                .password_toggle(true)
                                .cursor(self.text_input_pass.cursor())
                                .selection(self.text_input_pass.selected_range())
                                .blink_epoch(self.text_input_pass.blink_epoch_ms())
                                .placeholder("Secret token...")
                                .on_focus(on_focus_pass)
                                .on_toggle_mask(on_toggle_pass_mask)
                                .on_key_input(on_key_pass)
                                .on_pointer_action(on_ptr_pass)
                                .on_secondary_click(on_sec_pass),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(
                            cx,
                            div().h(16).flex_row().items_center().child(
                                cx,
                                text("NumberInput (Up/Down)")
                                    .size(12)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            ),
                        )
                        .child(
                            cx,
                            NumberInput::new(1403, self.number_val)
                                .range(0, 240)
                                .step(12)
                                .suffix(" px")
                                .width(152)
                                .on_change(on_num_change),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(
                            cx,
                            div().h(16).flex_row().items_center().child(
                                cx,
                                text("Disabled").size(12).color(theme.muted_foreground).nowrap(),
                            ),
                        )
                        .child(
                            cx,
                            TextInput::new(1404, "Read-only field", false)
                                .size(168, 32)
                                .disabled(true),
                        ),
                ),
        );

        let sel_label = self
            .select_idx
            .and_then(|i| SELECT_FRAMEWORKS.get(i))
            .map(|o| o.label)
            .unwrap_or("None");

        let select_section = story_section(
            cx,
            theme,
            "Select & Dropdown (velox-component SVG Chevrons & Esc / Arrow Keys)",
            "Open popup, press Up/Down to skip disabled items, press Escape to dismiss",
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(24)
                .items_start()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(
                            cx,
                            div().h(16).flex_row().items_center().child(
                                cx,
                                text("Select with Disabled Item")
                                    .size(12)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            ),
                        )
                        .child(
                            cx,
                            Select::new(1410, SELECT_FRAMEWORKS, self.select_idx, self.select_open)
                                .width(260)
                                .placeholder("Choose target engine...")
                                .on_toggle(on_toggle_select)
                                .on_select(on_select_opt),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(
                            cx,
                            div().h(16).flex_row().items_center().child(
                                cx,
                                text("Dropdown Menu")
                                    .size(12)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            ),
                        )
                        .child(
                            cx,
                            Dropdown::new(
                                1411,
                                DROPDOWN_BACKENDS,
                                self.dropdown_idx,
                                self.dropdown_open,
                            )
                            .size(220, 32)
                            .on_toggle(on_toggle_dd)
                            .on_select(on_select_dd),
                        ),
                ),
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(10)
                .child(cx, state_pill(cx, theme, fmt!(cx, "Select: {}", sel_label)))
                .child(
                    cx,
                    state_pill(
                        cx,
                        theme,
                        fmt!(
                            cx,
                            "Dropdown: {}",
                            DROPDOWN_BACKENDS[self.dropdown_idx.min(DROPDOWN_BACKENDS.len() - 1)]
                        ),
                    ),
                ),
        );

        // 3. gpui-base 风格插槽式 Calendar（完整展示 .w(px(250.)) / .p_3() / .border_1() / .item() / .label() / .when() / .hover(|s| ...) / .transition_colors()）
        let on_cal_prev = cx.action(|this, _acx| {
            this.calendar_state.prev();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cal_next = cx.action(|this, _acx| {
            this.calendar_state.next();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cal_month_toggle = cx.action(|this, _acx| {
            this.calendar_state.toggle_months();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cal_year_toggle = cx.action(|this, _acx| {
            this.calendar_state.toggle_years();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cal_day = cx.listener(|this, date: SimpleDate, _acx| {
            this.calendar_state.select_day(date);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cal_month = cx.listener(|this, month: u8, _acx| {
            this.calendar_state.select_month(month);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_cal_year = cx.listener(|this, year: i32, _acx| {
            this.calendar_state.select_year(year);
            this.event_count = this.event_count.saturating_add(1);
        });

        let cal_hover = theme.list_hover;
        let cal_border = theme.border;
        let cal_muted = theme.muted_foreground;
        let cal_active_bg = theme.primary;
        let cal_active_fg = theme.primary_foreground;
        let cal_fg = theme.foreground;

        let gpui_style_calendar = Calendar::from_state(1420, &self.calendar_state)
            .w(px(250.))
            .p_3()
            .bg(theme.background)
            .rounded(8)
            .border_1()
            .border_color(cal_border)
            .overflow_hidden()
            .on_prev_month(on_cal_prev)
            .on_next_month(on_cal_next)
            .on_toggle_month_view(on_cal_month_toggle)
            .on_toggle_year_view(on_cal_year_toggle)
            .on_select_day(on_cal_day)
            .on_select_month(on_cal_month)
            .on_select_year(on_cal_year)
            .item(cx, move |item, state| match state.kind() {
                CalendarItemKind::Previous | CalendarItemKind::Next => item
                    .size_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cal_fg)
                    .transition_colors()
                    .hover(|s| s.bg(cal_hover)),
                CalendarItemKind::MonthToggle | CalendarItemKind::YearToggle => item
                    .px_1()
                    .h_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(cal_fg)
                    .transition_colors()
                    .hover(|s| s.bg(cal_hover)),
                CalendarItemKind::Weekday => item
                    .size_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(cal_muted),
                CalendarItemKind::Day => item
                    .size_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(cal_fg)
                    .when(state.is_muted(), |s| s.text_color(cal_muted.scale_alpha(120)))
                    .when(state.is_today() && !state.is_active(), |s| {
                        s.border_1().border_color(cal_border)
                    })
                    .when(state.is_active(), |s| {
                        s.bg(cal_active_bg).text_color(cal_active_fg)
                    })
                    .when(!state.is_disabled() && !state.is_active(), |s| {
                        s.transition_colors().hover(|h| h.bg(cal_hover))
                    }),
                CalendarItemKind::Month | CalendarItemKind::Year => item
                    .w(px(74.))
                    .h_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(cal_fg)
                    .when(state.is_active(), |s| {
                        s.bg(cal_active_bg).text_color(cal_active_fg)
                    })
                    .when(!state.is_active(), |s| {
                        s.transition_colors().hover(|h| h.bg(cal_hover))
                    }),
            })
            .label(cx, move |kind, value| match kind {
                CalendarItemKind::Previous => StrRef::from("‹"),
                CalendarItemKind::Next => StrRef::from("›"),
                CalendarItemKind::MonthToggle | CalendarItemKind::Month => StrRef::from(
                    [
                        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
                        "Nov", "Dec",
                    ][(value as usize).clamp(1, 12)],
                ),
                CalendarItemKind::Weekday => {
                    StrRef::from(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][(value as usize) % 7])
                }
                _ => fmt!(cx, "{}", value),
            });

        let selected_date_str = match self.calendar_state.selection {
            DateSelection::Single(d) => fmt!(cx, "{:04}-{:02}-{:02}", d.year, d.month, d.day),
            DateSelection::Range { start, end } => fmt!(
                cx,
                "{:04}-{:02}-{:02}..{:04}-{:02}-{:02}",
                start.year,
                start.month,
                start.day,
                end.year,
                end.month,
                end.day
            ),
            DateSelection::None => StrRef::from("None"),
        };

        let calendar_section = story_section(
            cx,
            theme,
            "Headless Calendar (gpui-base Slot API)",
            "Click Nov / 2026 header to switch Days, Months & Years views",
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(20)
                .items_start()
                .child(cx, gpui_style_calendar)
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(8)
                        .child(
                            cx,
                            state_pill(cx, theme, fmt!(cx, "Selected: {}", selected_date_str.as_str())),
                        )
                        .child(
                            cx,
                            state_pill(
                                cx,
                                theme,
                                fmt!(
                                    cx,
                                    "View: {:04}-{:02} ({:?})",
                                    self.calendar_state.view_year,
                                    self.calendar_state.view_month,
                                    self.calendar_state.view_mode
                                ),
                            ),
                        )
                        .child(
                            cx,
                            text("Uses .w(px(250.)).p_3().border_1().border_color(...)")
                                .size(11)
                                .color(theme.muted_foreground),
                        )
                        .child(
                            cx,
                            text("+ .item(cx, |item, state| ... .when(...).hover(|s| ...))")
                                .size(11)
                                .color(theme.muted_foreground),
                        ),
                ),
        );

        let on_otp_focus = cx.action(|this, _acx| {
            this.active_input_field = 2;
            this.input_ctx_menu = None;
        });
        let on_otp_key = cx.listener(|this, key: KeyCode, _acx| {
            this.active_input_field = 2;
            let _ = this.input_otp.handle_key(key);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_color = cx.action(|this, _acx| {
            this.input_color_picker.open = !this.input_color_picker.open;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_pick_color = cx.listener(|this, col: Color, _acx| {
            this.input_color_picker.r = col.r();
            this.input_color_picker.g = col.g();
            this.input_color_picker.b = col.b();
            this.input_color_picker.open = false;
            this.event_count = this.event_count.saturating_add(1);
        });

        let extended_inputs_section = story_section(
            cx,
            theme,
            "OtpInput & ColorPicker (velox-component)",
            "6-digit grouped OTP verification input & themed ColorPicker swatch popover",
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(24)
                .items_center()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(cx, text("OTP Verification Code").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            OtpInput::new(
                                1450,
                                cx.arena().alloc_str(self.input_otp.as_str()).as_str(),
                                6,
                            )
                            .focused(self.active_input_field == 2)
                            .groups(3)
                            .on_focus(on_otp_focus)
                            .on_key_input(on_otp_key),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(6)
                        .child(cx, text("Accent Color Picker").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            ColorPicker::new(1451, self.input_color_picker)
                                .on_toggle_open(on_toggle_color)
                                .on_select_color(on_pick_color),
                        ),
                )
                .child(
                    cx,
                    state_pill(
                        cx,
                        theme,
                        fmt!(
                            cx,
                            "OTP: {} | Color: #{:02X}{:02X}{:02X}",
                            self.input_otp.as_str(),
                            self.input_color_picker.r,
                            self.input_color_picker.g,
                            self.input_color_picker.b
                        ),
                    ),
                ),
        );

        div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Inputs, Select, OTP, ColorPicker & Calendar",
                    "Editable text fields, stepper inputs, popups, OTP, ColorPicker & slot-customized Calendar.",
                    "velox_component::{TextInput, Select, OtpInput, ColorPicker, Calendar}",
                ),
            )
            .child(cx, text_section)
            .child(cx, select_section)
            .child(cx, extended_inputs_section)
            .child(cx, calendar_section)
    }
}
