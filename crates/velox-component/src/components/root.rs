//! 主题化应用根壳层组件 `Root`。

use crate::styled::fill_color;
use crate::theme::Theme;
use velox_core::prelude::*;

crate::define_themed_component! {
    name: Root<'a>,
    base: velox_base::Root<'a>,
    new: (id: u32, width: i16, height: i16),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        fill_color(&mut base.bg, theme.background);
        fill_color(&mut base.text_color, theme.foreground);
        fill_color(&mut base.dialog_backdrop_bg, Color::rgba(0, 0, 0, 136));
    },
    forward: [
        theme(theme: Theme),
        theme_in(cx: &FrameScope, theme: Theme),
        content(cx: &'a FrameScope, child: impl IntoElement<'a>),
        dialog(cx: &'a FrameScope, dialog: impl IntoElement<'a>),
        notification(cx: &'a FrameScope, notification: impl IntoElement<'a>),
    ],
}

impl<'a> Root<'a> {
    /// 通过闭包惰性构造主窗口内容区域（在执行闭包前自动向 `cx` 注入当前 `Root` 主题）。
    #[inline(always)]
    pub fn content_with<E: IntoElement<'a>>(
        mut self,
        cx: &'a FrameScope,
        build: impl FnOnce(&'a FrameScope) -> E,
    ) -> Self {
        self.inner = self.inner.content_with(cx, build);
        self
    }
}
