//! 多行文本编辑框组件（[`Textarea`]，兼容 `#![no_std]`）。
//!
//! 同时支持：
//! - **一等 `cosmic-text` 排版模式**（[`Textarea::from_edit`] / [`Textarea::from_buffer`]）：
//!   自动软折行、字形级选区矩形、0 抖动绝对定位叠层光标与亚像素点击/拖拽选区回调；
//! - **经典兼容模式**（[`Textarea::new`]）：
//!   100% 兼容原有 `value: &str` + `CursorSelection` 用法，同步升级为 0 抖动叠层光标与精准选区高亮。

use crate::traits::Disableable;
use velox_engine::display::measure_line_width_f32;
use velox_engine::prelude::*;

use super::cursor::CursorSelection;
use super::editor::{bake_buffer_snapshot, BakedBufferSnapshot};
use super::text_engine::{Cursor, Edit, TextDocBuffer};

const AUTO_COLOR: Color = Color(0x0000_0001);

#[inline(always)]
fn round_nonneg_i16(v: f32) -> i16 {
    if v <= 0.0 {
        0
    } else {
        let n = (v + 0.5) as i32;
        n.min(i16::MAX as i32) as i16
    }
}

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

/// 多行文本编辑区组件（`Textarea`）。
///
/// 支持多行文本展示、0 抖动叠层活动光标、字形级跨行选区背景高亮、占位符提示、垂直滚动与键盘/指针编辑回调。
pub struct Textarea<'a> {
    pub id: u32,
    pub value: &'a str,
    pub placeholder: &'a str,
    pub focused: bool,
    pub disabled: bool,
    /// 聚焦时是否启用标准 530ms 周期光标闪烁（默认 `true`）。
    pub cursor_blink: bool,
    /// 最近一次键入或点击时间戳（毫秒）。
    pub blink_epoch_ms: u32,
    pub selection: CursorSelection,
    pub width: i16,
    pub height: i16,
    pub line_height: i16,
    pub font_size: u8,
    pub rounded: u8,
    pub focus_ring: bool,
    pub bg: Color,
    pub border_color: Color,
    pub focus_border_color: Color,
    pub text_color: Color,
    pub placeholder_color: Color,
    pub selection_bg: Color,
    pub cursor_color: Color,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_focus: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    /// 亚像素内容区指针交互回调：`(content_x, content_y, is_drag, now_ms, shift)`。
    pub on_pointer_action: Option<&'a dyn Fn((i32, i32, bool, u32, bool), &mut ActionScope)>,
    baked: Option<BakedBufferSnapshot<'a>>,
}

impl<'a> Textarea<'a> {
    /// 创建一个无样式多行文本编辑框原语。
    #[inline(always)]
    pub const fn new(id: u32, value: &'a str, focused: bool) -> Self {
        let len = value.len();
        Self {
            id,
            value,
            placeholder: "",
            focused,
            disabled: false,
            cursor_blink: true,
            blink_epoch_ms: 0,
            selection: CursorSelection::new(0, len, len),
            width: 280,
            height: 120,
            line_height: 20,
            font_size: 13,
            rounded: 0,
            focus_ring: false,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            focus_border_color: Color::TRANSPARENT,
            text_color: AUTO_COLOR,
            placeholder_color: AUTO_COLOR,
            selection_bg: AUTO_COLOR,
            cursor_color: AUTO_COLOR,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_focus: None,
            on_key_input: None,
            on_pointer_action: None,
            baked: None,
        }
    }

    /// 直接基于参照 `cosmic-text` 构建的 [`TextDocBuffer`] 烘焙帧级快照并创建多行文本编辑框。
    pub fn from_buffer(
        cx: &'a FrameScope,
        id: u32,
        buffer: &TextDocBuffer,
        cursor: Cursor,
        selection_bounds: Option<(Cursor, Cursor)>,
        focused: bool,
    ) -> Self {
        let snap = bake_buffer_snapshot(cx, buffer, cursor, selection_bounds);
        let mut ta = Self::new(id, "", focused);
        ta.font_size = snap.font_size;
        ta.line_height = snap.line_height;
        ta.baked = Some(snap);
        ta
    }

    /// 直接从任意 [`Edit`] 状态机创建多行文本编辑框。
    #[inline]
    pub fn from_edit<E: Edit>(cx: &'a FrameScope, id: u32, edit: &E, focused: bool) -> Self {
        Self::from_buffer(
            cx,
            id,
            edit.buffer_ref(),
            edit.cursor(),
            edit.selection_bounds(),
            focused,
        )
    }

    #[inline(always)]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    #[inline(always)]
    pub const fn selection(mut self, selection: CursorSelection) -> Self {
        self.selection = selection;
        self
    }

    #[inline(always)]
    pub const fn cursor(mut self, byte_offset: usize) -> Self {
        self.selection = CursorSelection::new(0, byte_offset, byte_offset);
        self
    }

    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    #[inline(always)]
    pub const fn rows(mut self, rows: u8) -> Self {
        self.height = (rows as i16) * self.line_height + 16;
        self
    }

    #[inline(always)]
    pub const fn font_size(mut self, font_size: u8) -> Self {
        self.font_size = font_size;
        self
    }

    #[inline(always)]
    pub const fn cursor_blink(mut self, enabled: bool) -> Self {
        self.cursor_blink = enabled;
        self
    }

    #[inline(always)]
    pub const fn blink_epoch(mut self, epoch_ms: u32) -> Self {
        self.blink_epoch_ms = epoch_ms;
        self
    }

    #[inline(always)]
    pub fn on_focus(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_focus = Some(handler);
        self
    }

    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// 绑定一等 `cosmic-text` 内容区亚像素指针交互回调。
    #[inline(always)]
    pub fn on_pointer_action(
        mut self,
        handler: &'a dyn Fn((i32, i32, bool, u32, bool), &mut ActionScope),
    ) -> Self {
        self.on_pointer_action = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Textarea<'a>);

impl<'a> Disableable for Textarea<'a> {
    #[inline(always)]
    fn disabled(mut self, is_disabled: bool) -> Self {
        self.disabled = is_disabled;
        self
    }
}

impl<'a> Widget<'a> for Textarea<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let focus_border_color = if self.focus_border_color != Color::TRANSPARENT {
            self.focus_border_color
        } else {
            border_color
        };
        let text_color = resolve_color(self.text_color, def_fg);
        let placeholder_color = resolve_color(self.placeholder_color, def_fg.scale_alpha(128));
        let selection_bg = resolve_color(self.selection_bg, def_fg.scale_alpha(48));
        let cursor_color = resolve_color(self.cursor_color, def_fg);

        let border_col = if self.focused && !self.disabled {
            focus_border_color
        } else {
            border_color
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let mut container = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(height)
            .p(8)
            .rounded(self.rounded)
            .bg(bg)
            .scrollable_auto();
        if border_col != Color::TRANSPARENT {
            container = container.border(1, border_col);
        }

        if self.disabled {
            container = container.opacity(120).cursor(CursorShape::NotAllowed);
        } else {
            container = container.focusable().cursor(CursorShape::Text);
            if self.focus_border_color != Color::TRANSPARENT {
                container = container.focus_border(1, self.focus_border_color);
            } else if !self.focus_ring {
                container = container.no_focus_border();
            }
            if let Some(cb) = self.on_focus {
                container = container.on_click_raw(cb);
            }
            if let Some(kcb) = self.on_key_input {
                container = container.on_key_input(kcb);
            }
        }
        container = self.style_ext.apply_to_div(container, self.style);

        let cursor_visible = if self.focused && !self.disabled {
            if self.cursor_blink {
                cx.request_frame();
                let elapsed = cx.now_ms().wrapping_sub(self.blink_epoch_ms);
                (elapsed / crate::components::text_input::CURSOR_BLINK_HALF_PERIOD_MS) % 2 == 0
            } else {
                true
            }
        } else {
            false
        };

        if let Some(snap) = self.baked {
            if snap.total_bytes == 0 && !self.focused && !self.placeholder.is_empty() {
                return container
                    .child(
                        cx,
                        text(self.placeholder)
                            .size(self.font_size)
                            .color(placeholder_color),
                    )
                    .into_element(cx);
            }

            let lh = snap.line_height.max(12);
            let font_size = snap.font_size;

            let mut inner = div()
                .id(self.id ^ 0x5441_494E)
                .flex_col()
                .w_full()
                .cursor(if self.disabled {
                    CursorShape::NotAllowed
                } else {
                    CursorShape::Text
                });

            if !self.disabled && (self.on_pointer_action.is_some() || self.on_focus.is_some()) {
                let on_focus = self.on_focus;
                let on_act = self.on_pointer_action;
                let ptr_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let ev = acx.drag_event();
                    let is_drag = ev.delta_x != 0 || ev.delta_y != 0;
                    if !is_drag {
                        if let Some(fcb) = on_focus {
                            fcb(acx);
                        }
                    }
                    if let Some(acb) = on_act {
                        acb(
                            (
                                ev.local_x as i32,
                                ev.local_y as i32,
                                is_drag,
                                acx.now_ms(),
                                acx.modifiers().shift(),
                            ),
                            acx,
                        );
                    }
                });
                inner = inner.on_drag_raw(ptr_cb);
            }

            for &run in snap.runs {
                let row_seed = ((run.line_i as u32) << 8) | (run.layout_i as u32 & 0xFF);
                let mut row_div = div()
                    .id(crate::scoped_child_id(
                        self.id,
                        0xD6AA_B9FB,
                        0x1000 + row_seed,
                    ))
                    .flex_row()
                    .w_full()
                    .h(lh)
                    .items_center()
                    .overflow_hidden();

                if let Some((x0, x1)) = run.sel_rect {
                    row_div = row_div.child(
                        cx,
                        div()
                            .pos(x0, 1)
                            .w((x1 - x0).max(3))
                            .h((lh - 2).max(10))
                            .rounded(2)
                            .bg(selection_bg),
                    );
                }

                for &span in run.spans {
                    let col = span.color_opt.unwrap_or(text_color);
                    row_div = row_div.child(
                        cx,
                        text(span.text).size(font_size).color(col).nowrap(),
                    );
                }

                if self.focused {
                    if let Some(cx_px) = run.cursor_x {
                        let caret_bg = if cursor_visible {
                            cursor_color
                        } else {
                            Color::TRANSPARENT
                        };
                        row_div = row_div.child(
                            cx,
                            div()
                                .id(self.id ^ 0x5441_4352)
                                .pos(cx_px, 2)
                                .w(1)
                                .h((lh - 4).max(10))
                                .bg(caret_bg)
                                .ime_caret(),
                        );
                    }
                }

                inner = inner.child(cx, row_div);
            }

            return container.child(cx, inner).into_element(cx);
        }

        // 经典 `&str` 兼容路径（同样升级为 0 抖动绝对定位叠层光标 + 字符级选区高亮）
        if self.value.is_empty() && !self.focused {
            return container
                .child(
                    cx,
                    text(self.placeholder)
                        .size(self.font_size)
                        .color(placeholder_color),
                )
                .into_element(cx);
        }

        let cursor_off = self.selection.cursor_offset().min(self.value.len());
        let sel_start = self.selection.start.min(self.value.len());
        let sel_end = self.selection.end.min(self.value.len());
        let has_sel = sel_start < sel_end;

        let mut line_start = 0usize;
        for (line_idx, raw_line) in self.value.split('\n').enumerate() {
            let line_str = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let line_end = line_start + line_str.len();
            let line_has_sel =
                has_sel && sel_start < line_end.max(line_start + 1) && sel_end > line_start;
            let cursor_in_line =
                cursor_visible && cursor_off >= line_start && cursor_off <= line_end;

            let mut row_div = div()
                .id(crate::scoped_child_id(
                    self.id,
                    0xD6AA_B9FB,
                    0x1000 + line_idx as u32,
                ))
                .flex_row()
                .w_full()
                .h(self.line_height)
                .items_center()
                .overflow_hidden();

            if line_has_sel {
                let mut s_rel = sel_start.saturating_sub(line_start).min(line_str.len());
                let mut e_rel = sel_end.saturating_sub(line_start).min(line_str.len());
                while s_rel > 0 && !line_str.is_char_boundary(s_rel) {
                    s_rel -= 1;
                }
                while e_rel < line_str.len() && !line_str.is_char_boundary(e_rel) {
                    e_rel += 1;
                }
                let x0 = round_nonneg_i16(measure_line_width_f32(&line_str[..s_rel], self.font_size));
                let mut x1 =
                    round_nonneg_i16(measure_line_width_f32(&line_str[..e_rel], self.font_size));
                if sel_end > line_end {
                    x1 = x1.saturating_add(7);
                }
                row_div = row_div.child(
                    cx,
                    div()
                        .pos(x0, 1)
                        .w((x1 - x0).max(3))
                        .h((self.line_height - 2).max(10))
                        .rounded(2)
                        .bg(selection_bg),
                );
            }

            if !line_str.is_empty() {
                row_div = row_div.child(
                    cx,
                    text(line_str)
                        .size(self.font_size)
                        .color(text_color)
                        .nowrap(),
                );
            }

            if cursor_in_line {
                let mut rel_cursor = (cursor_off - line_start).min(line_str.len());
                while rel_cursor > 0 && !line_str.is_char_boundary(rel_cursor) {
                    rel_cursor -= 1;
                }
                let raw_x = measure_line_width_f32(&line_str[..rel_cursor], self.font_size);
                let cx_px = if rel_cursor == 0 {
                    0
                } else if rel_cursor < line_str.len() {
                    round_nonneg_i16((raw_x - 0.85).max(0.0))
                } else {
                    round_nonneg_i16(raw_x)
                };
                row_div = row_div.child(
                    cx,
                    div()
                        .id(self.id ^ 0x5441_4352)
                        .pos(cx_px, 2)
                        .w(1)
                        .h((self.line_height - 4).max(10))
                        .bg(cursor_color)
                        .ime_caret(),
                );
            }

            container = container.child(cx, row_div);
            line_start += raw_line.len() + 1;
        }

        container.into_element(cx)
    }
}
