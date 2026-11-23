//! 无样式键盘快捷键展示原语 `Kbd`（默认零背景、零边框、零圆角，仅按 `+` 拆分渲染按键组合）。

use velox_core::prelude::*;

/// 无样式键盘快捷键提示原语（`Kbd`，例如 `Ctrl + K`）。
pub struct Kbd<'a> {
    pub id: u32,
    pub shortcut: &'a str,
    pub keys: &'a [&'a str],
    pub key_height: i16,
    pub key_px: i8,
    pub key_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub separator_color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Kbd<'a> {
    /// 使用快捷键字符串（如 `"Ctrl+K"`，自动按 `'+'` 拆分为按键项）创建无样式 `Kbd`。
    #[inline(always)]
    pub const fn new(id: u32, shortcut: &'a str) -> Self {
        Self {
            id,
            shortcut,
            keys: &[],
            key_height: 0,
            key_px: 0,
            key_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            separator_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 使用显式按键切片（如 `&["Cmd", "Shift", "P"]`）创建无样式 `Kbd`。
    #[inline(always)]
    pub const fn from_keys(id: u32, keys: &'a [&'a str]) -> Self {
        Self {
            id,
            shortcut: "",
            keys,
            key_height: 0,
            key_px: 0,
            key_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            separator_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }
}

crate::impl_base_styled_box!(Kbd<'a>);

impl<'a> Widget<'a> for Kbd<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let text_col = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let sep_col = if self.separator_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.separator_color
        };
        let key_bg = self.key_bg;
        let border_col = self.border_color;
        let rounded = self.rounded;
        let key_h = self.key_height;
        let key_px = self.key_px;

        let mut row = div().id(self.id).flex_row().gap(3).items_center();
        let render_key = move |cx: &'a FrameScope, k: &'a str| {
            let mut cap = div()
                .rounded(rounded)
                .bg(key_bg)
                .items_center()
                .justify_center()
                .child(cx, text(k).size(11).color(text_col).text_center().nowrap());
            if key_h > 0 {
                cap = cap.h(key_h);
            }
            if key_px > 0 {
                cap = cap.px(key_px);
            }
            if border_col != Color::TRANSPARENT {
                cap = cap.border(1, border_col);
            }
            cap
        };

        if !self.keys.is_empty() {
            for (i, &k) in self.keys.iter().enumerate() {
                if i > 0 {
                    row = row.child(cx, text("+").size(10).color(sep_col));
                }
                row = row.child(cx, render_key(cx, k));
            }
        } else {
            for (i, part) in self.shortcut.split('+').enumerate() {
                if i > 0 {
                    row = row.child(cx, text("+").size(10).color(sep_col));
                }
                row = row.child(cx, render_key(cx, part));
            }
        }

        row = self.style_ext.apply_to_div(row, self.style);
        row.into_element(cx)
    }
}
