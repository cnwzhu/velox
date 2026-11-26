//! 主题化单行文本输入框组件 `TextInput` / `Input`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::text_input::EditableText;

crate::define_themed_component! {
    name: TextInput<'a>,
    base: velox_base::components::text_input::TextInput<'a>,
    new: (id: u32, value: &'a str, focused: bool),
    styled: false,
    variants: false,
    sizable: true,
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
        fill_color(&mut base.cursor_color, theme.primary);
        fill_color(&mut base.selection_bg, Color::rgba(p.r(), p.g(), p.b(), 102));
        fill_rounded(&mut base.selection_rounded, 2);
        fill_color(&mut base.clear_icon_color, theme.muted_foreground);
        if base.clear_hover_bg.is_none() {
            base.clear_hover_bg = Some(theme.accent);
        }
        if base.clear_active_bg.is_none() {
            base.clear_active_bg = Some(theme.list_active);
        }
        fill_rounded(&mut base.rounded, 6);
    },
    forward: [
        cursor(pos: usize),
        selection(range: Option<(usize, usize)>),
        cursor_blink(enabled: bool),
        blink_epoch(epoch_ms: u32),
        masked(masked: bool),
        password_toggle(enabled: bool),
        password(enabled: bool),
        clearable(clearable: bool),
        size(width: i16, height: i16),
        surface(bg: Color, border_color: Color, focus_border_color: Color),
        rounded(radius: u8),
        placeholder(text: &'a str),
        on_focus(handler: &'a dyn Fn(&mut ActionScope)),
        on_clear(handler: &'a dyn Fn(&mut ActionScope)),
        on_toggle_mask(handler: &'a dyn Fn(&mut ActionScope)),
        on_key_input(handler: &'a dyn Fn(KeyCode, &mut ActionScope)),
        on_pointer_action(handler: &'a dyn Fn((i16, bool, u32, bool), &mut ActionScope)),
        on_secondary_click(handler: &'a dyn Fn((i16, i16), &mut ActionScope)),
    ],
}

impl<'a> TextInput<'a> {
    /// 设置前置图标/装饰插槽。
    #[inline(always)]
    pub fn prefix(mut self, cx: &'a FrameScope, element: impl IntoElement<'a>) -> Self {
        self.inner = self.inner.prefix(cx, element);
        self
    }

    /// 设置后置图标/装饰插槽。
    #[inline(always)]
    pub fn suffix(mut self, cx: &'a FrameScope, element: impl IntoElement<'a>) -> Self {
        self.inner = self.inner.suffix(cx, element);
        self
    }
}

/// `TextInput` 主题化简短别名。

pub type Input<'a> = TextInput<'a>;
