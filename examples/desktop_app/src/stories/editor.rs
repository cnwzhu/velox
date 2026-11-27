//! `Code Editor` 故事页：完全基于 `velox-component` 的 `CodeEditor` 与 `Button`，
//! 展示参照 `pop-os/cosmic-text` 重构的 Velox 文本编辑引擎。

use super::story_header;
use crate::{ContextMenuTarget, GalleryApp};
use velox_kit::component::{BufferPoint, Button, CodeEditor, Edit, Wrap};
use velox_kit::prelude::*;

impl GalleryApp {
    pub(crate) fn render_story_editor<'a>(
        &mut self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        // 每帧进入前同步暗/亮主题语法配色、软折行容器宽度并按需惰性重排
        self.editor_buf.sync_theme(&theme);
        let content_wrap_w = if self.sidebar_collapsed { 1080.0 } else { 910.0 };
        self.editor_buf
            .buffer_mut()
            .set_size(Some(content_wrap_w), None);
        self.editor_buf.shape_as_needed(false);

        let raw_focus_ed = cx.action(|this, acx| {
            this.editor_focused = true;
            this.input_ctx_menu = None;
            this.editor_buf.reset_blink(acx.now_ms());
        });
        let on_ed_key = cx.listener(|this, key: KeyCode, acx| {
            let _ = this.editor_buf.handle_key_with_mods(key, acx.modifiers());
            this.editor_buf.reset_blink(acx.now_ms());
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ed_ptr = cx.listener(
            |this, (x, y, is_drag, now_ms, shift): (i32, i32, bool, u32, bool), _acx| {
                this.editor_focused = true;
                this.input_ctx_menu = None;
                if is_drag {
                    this.editor_buf.handle_pointer_drag(x, y);
                    this.editor_buf.reset_blink(now_ms);
                } else {
                    this.editor_buf.handle_pointer_press(x, y, now_ms, shift);
                }
            },
        );
        let on_ed_sec = cx.listener(|this, (_pt, wx, wy): (BufferPoint, i16, i16), acx| {
            this.editor_focused = true;
            this.editor_buf.reset_blink(acx.now_ms());
            this.input_ctx_menu = Some((ContextMenuTarget::CodeEditor, wx, wy));
        });
        let on_undo = cx.listener(|this, _: ClickEvent, _acx| {
            let _ = this.editor_buf.undo();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_redo = cx.listener(|this, _: ClickEvent, _acx| {
            let _ = this.editor_buf.redo();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_wrap = cx.listener(|this, _: ClickEvent, _acx| {
            let next = match this.editor_buf.buffer_ref().wrap() {
                Wrap::None => Wrap::WordOrGlyph,
                _ => Wrap::None,
            };
            this.editor_buf.buffer_mut().set_wrap(next);
            this.editor_buf.shape_as_needed(false);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_select_all = cx.listener(|this, _: ClickEvent, _acx| {
            this.editor_focused = true;
            this.editor_buf.select_all();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_insert_fn = cx.listener(|this, _: ClickEvent, _acx| {
            this.editor_buf
                .insert_string("\nfn on_tick(dt_ms: u32) -> bool {\n    dt_ms > 0\n}", None);
            this.event_count = this.event_count.saturating_add(1);
        });

        let cursor = self.editor_buf.cursor();
        let col_chars = self
            .editor_buf
            .buffer_ref()
            .lines
            .as_slice()
            .get(cursor.line)
            .map(|l| {
                let mut i = cursor.index.min(l.text().len());
                while i > 0 && !l.text().is_char_boundary(i) {
                    i -= 1;
                }
                l.text()[..i].chars().count()
            })
            .unwrap_or(0);
        let sel_bytes = self.editor_buf.selected_byte_len();
        let wrap_btn_label = match self.editor_buf.buffer_ref().wrap() {
            Wrap::None => "Wrap: Off",
            _ => "Wrap: Auto",
        };
        let summary_text = if sel_bytes > 0 {
            fmt!(
                cx,
                "Ln {}, Col {}  •  {}B selected  •  {}B total",
                cursor.line + 1,
                col_chars + 1,
                sel_bytes,
                self.editor_buf.len()
            )
        } else {
            fmt!(
                cx,
                "Ln {}, Col {}  •  {}B  •  2× Word / 3× Line / Shift+Arrows",
                cursor.line + 1,
                col_chars + 1,
                self.editor_buf.len()
            )
        };

        div()
            .flex_col()
            .size_full()
            .gap(12)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    cx.t("ed.title"),
                    "cosmic-text inspired BufferLine / LayoutRun shaping, incremental Rust highlighting, soft-wrap, subpixel hit-testing & Change::reverse() undo/redo.",
                    "velox_component::CodeEditor",
                ),
            )
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .h(32)
                    .gap(12)
                    .items_center()
                    .justify_between()
                    .child(
                        cx,
                        text(summary_text)
                            .size(12)
                            .color(theme.muted_foreground)
                            .ellipsis(),
                    )
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(8)
                            .items_center()
                            .child(
                                cx,
                                Button::new(313, wrap_btn_label)
                                    .outline()
                                    .small()
                                    .w(88)
                                    .on_click(on_toggle_wrap),
                            )
                            .child(
                                cx,
                                Button::new(314, "Select All")
                                    .outline()
                                    .small()
                                    .w(82)
                                    .on_click(on_select_all),
                            )
                            .child(
                                cx,
                                Button::new(310, cx.t("ed.undo"))
                                    .outline()
                                    .small()
                                    .w(66)
                                    .on_click(on_undo),
                            )
                            .child(
                                cx,
                                Button::new(311, cx.t("ed.redo"))
                                    .outline()
                                    .small()
                                    .w(66)
                                    .on_click(on_redo),
                            )
                            .child(
                                cx,
                                Button::new(312, cx.t("ed.insert_fn"))
                                    .primary()
                                    .small()
                                    .w(92)
                                    .on_click(on_insert_fn),
                            ),
                    ),
            )
            .child(
                cx,
                CodeEditor::from_edit(cx, 320, &self.editor_buf, self.editor_focused)
                    .size(0, 540)
                    .blink_epoch(self.editor_buf.blink_epoch_ms())
                    .on_focus(raw_focus_ed)
                    .on_key_input(on_ed_key)
                    .on_pointer_action(on_ed_ptr)
                    .on_secondary_click(on_ed_sec),
            )
    }
}
