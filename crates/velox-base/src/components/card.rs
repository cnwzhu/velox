//! 无样式基础卡片容器原语 `Card`（默认零背景、零边框、零圆角、零内边距，支持全套 `Styled` 链式定制）。

use velox_core::prelude::*;

/// 无样式卡片容器原语（`Card`）。
///
/// 默认等价于纯容器 `div()`，不注入任何主题背景、边框、圆角或内边距；
/// 预设卡片视觉样式由 `velox-component::Card` 提供。
pub struct Card<'a> {
    pub inner: Div<'a>,
    pub style_ext: StyleExt,
}

impl<'a> Default for Card<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Card<'a> {
    /// 创建一个无样式基础卡片容器。
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            inner: div(),
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 显式重置为无样式纯容器状态（`velox-base` 默认即无样式）。
    #[inline(always)]
    pub fn unstyled(mut self) -> Self {
        self.inner.style.set_rounded(0);
        self.inner.style.padding = Edges::all(0);
        self.inner.style.bg = Color::TRANSPARENT;
        self.inner.style.set_border_px(0);
        self
    }

    /// 设置跨帧唯一元素 ID。
    #[inline(always)]
    pub fn id(mut self, id: u32) -> Self {
        self.inner = self.inner.id(id);
        self
    }

    /// 链式追加单个子节点。
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

crate::impl_base_styled_box!(Card<'a>, via inner);

impl<'a> Widget<'a> for Card<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        self.style_ext.apply_ext_only(self.inner).into_element(cx)
    }
}
