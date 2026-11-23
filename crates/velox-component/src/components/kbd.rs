//! 主题化键盘快捷键徽章组件 `Kbd`。

use crate::styled::{fill_color, fill_rounded};

crate::define_themed_component! {
    name: Kbd<'a>,
    base: velox_base::Kbd<'a>,
    new: (id: u32, shortcut: &'a str),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        if base.key_height == 0 {
            base.key_height = 20;
        }
        if base.key_px == 0 {
            base.key_px = 6;
        }
        fill_color(&mut base.key_bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.separator_color, theme.muted_foreground);
        fill_rounded(&mut base.rounded, 4);
    },
}

impl<'a> Kbd<'a> {
    /// 使用显式按键切片创建主题化 `Kbd`。
    #[inline(always)]
    pub fn from_keys(id: u32, keys: &'a [&'a str]) -> Self {
        Self::from_base(velox_base::Kbd::from_keys(id, keys))
    }
}
