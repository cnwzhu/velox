//! 主题化多行文本输入框组件 `Textarea`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::CursorSelection;
use velox_engine::prelude::*;

crate::define_themed_component! {
    name: Textarea<'a>,
    base: velox_base::Textarea<'a>,
    new: (id: u32, value: &'a str, focused: bool),
    styled: false,
    variants: false,
    sizable: false,
    disableable: true,
    init: |base| {
        base.focus_ring = true;
    },
    apply_tw: |base, theme| {
        let p = theme.primary;
        fill_color(&mut base.bg, theme.input);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.focus_border_color, theme.ring);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.placeholder_color, theme.muted_foreground);
        fill_color(&mut base.selection_bg, Color::rgba(p.r(), p.g(), p.b(), 102));
        fill_color(&mut base.cursor_color, theme.primary);
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        placeholder(placeholder: &'a str),
        selection(selection: CursorSelection),
        cursor(byte_offset: usize),
        size(width: i16, height: i16),
        rows(rows: u8),
        font_size(font_size: u8),
        cursor_blink(enabled: bool),
        blink_epoch(epoch_ms: u32),
        on_focus(handler: &'a dyn Fn(&mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
        on_pointer_action(handler: &'a dyn Fn((i32, i32, bool, u32, bool), &mut ActionScope)),
    ],
}

impl<'a> Textarea<'a> {
    /// 直接从任意 [`velox_base::Edit`] 状态机构造主题化多行编辑框。
    #[inline]
    pub fn from_edit<E: velox_base::Edit>(
        cx: &'a FrameScope,
        id: u32,
        edit: &E,
        focused: bool,
    ) -> Self {
        Self::from_base(velox_base::Textarea::from_edit(cx, id, edit, focused))
    }
}
