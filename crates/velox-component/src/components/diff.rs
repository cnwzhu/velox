//! 主题化代码差异对比视图组件 `Diff`。

use crate::styled::{fill_color, fill_rounded};

pub use velox_base::components::diff::{DiffLine, DiffLineKind, DiffViewMode};

crate::define_themed_component! {
    name: Diff<'a>,
    base: velox_base::Diff<'a>,
    new: (id: u32, title: &'a str, lines: &'a [DiffLine<'a>]),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.card);
        fill_color(&mut base.header_bg, theme.secondary);
        fill_color(&mut base.border_color, theme.border);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.added_bg, theme.success.lerp(theme.card, 220));
        fill_color(&mut base.added_fg, theme.success);
        fill_color(&mut base.removed_bg, theme.danger.lerp(theme.card, 220));
        fill_color(&mut base.removed_fg, theme.danger);
        fill_color(&mut base.hunk_bg, theme.primary.lerp(theme.card, 210));
        fill_color(&mut base.hunk_fg, theme.primary);
        fill_rounded(&mut base.rounded, 8);
    },
    forward: [
        mode(mode: DiffViewMode),
        width(width: i16),
    ],
}

impl<'a> Diff<'a> {
    /// 使用 Unified Diff 补丁文本创建主题化 `Diff` 视图。
    #[inline(always)]
    pub fn from_patch(id: u32, title: &'a str, patch: &'a str) -> Self {
        Self::from_base(velox_base::Diff::from_patch(id, title, patch))
    }
}
