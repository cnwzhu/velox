//! 主题化卡片表面组件 `Card`。

use crate::styled::apply_tw_card_surface;
use velox_engine::prelude::*;

/// 建立在 [`velox_base::Card`] 之上的主题化卡片表面组件（默认 `rounded(8) p(8) bg(card) border(1, border)`）。
pub struct Card<'a> {
    pub inner: velox_base::components::card::Card<'a>,
    pub unstyled: bool,
}

impl<'a> Default for Card<'a> {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Card<'a> {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            inner: velox_base::components::card::Card::new(),
            unstyled: false,
        }
    }

    #[inline(always)]
    pub const fn from_base(inner: velox_base::components::card::Card<'a>) -> Self {
        Self {
            inner,
            unstyled: false,
        }
    }

    #[inline(always)]
    pub fn into_base(self) -> velox_base::components::card::Card<'a> {
        self.inner
    }

    #[inline(always)]
    pub fn unstyled(mut self) -> Self {
        self.unstyled = true;
        self.inner = self.inner.unstyled();
        self
    }

    #[inline(always)]
    pub fn id(mut self, id: u32) -> Self {
        self.inner = self.inner.id(id);
        self
    }

    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.inner = self.inner.child(cx, child);
        self
    }

    /// 批量追加子节点序列。
    #[inline(always)]
    pub fn children<I, E>(mut self, cx: &'a FrameScope, iter: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: IntoElement<'a>,
    {
        self.inner = self.inner.children(cx, iter);
        self
    }
}

impl<'a> core::ops::Deref for Card<'a> {
    type Target = velox_base::components::card::Card<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> core::ops::DerefMut for Card<'a> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a> From<velox_base::components::card::Card<'a>> for Card<'a> {
    #[inline(always)]
    fn from(inner: velox_base::components::card::Card<'a>) -> Self {
        Self::from_base(inner)
    }
}

impl<'a> Styled for Card<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a> StyledExt for Card<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a> Widget<'a> for Card<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        if !self.unstyled {
            let style = self.inner.style_mut();
            apply_tw_card_surface(style, cx.theme());
            if style.padding == Edges::all(0) {
                style.padding = Edges::all(8);
            }
        }
        self.inner.render_once(cx)
    }
}
