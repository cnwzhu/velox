//! 主题化等高虚拟滚动列表组件 `UniformList` / `VirtualList`。

use velox_core::prelude::*;

/// 主题化等高虚拟滚动列表组件（默认应用卡片背景、1px 边框与 6px 圆角，调用 `.unstyled()` 可还原透明容器）。
pub struct UniformList<'a, F> {
    pub inner: velox_base::UniformList<'a, F>,
    pub unstyled: bool,
}

/// `VirtualList` 主题化虚拟滚动列表类型别名。
pub type VirtualList<'a, F> = UniformList<'a, F>;

impl<'a, F> UniformList<'a, F>
where
    F: Fn(usize, &'a FrameScope) -> Element<'a>,
{
    #[inline]
    pub fn new(
        id: u32,
        item_count: usize,
        item_height: i16,
        viewport_height: i16,
        scroll_offset_y: i16,
        render_item: F,
    ) -> Self {
        Self {
            inner: velox_base::UniformList::new(
                id,
                item_count,
                item_height,
                viewport_height,
                scroll_offset_y,
                render_item,
            ),
            unstyled: false,
        }
    }

    #[inline(always)]
    pub const fn from_base(inner: velox_base::UniformList<'a, F>) -> Self {
        Self {
            inner,
            unstyled: false,
        }
    }

    #[inline(always)]
    pub fn into_base(self) -> velox_base::UniformList<'a, F> {
        self.inner
    }

    #[inline(always)]
    pub fn scroll_y_i32(mut self, scroll_offset_y: i32) -> Self {
        self.inner = self.inner.scroll_y_i32(scroll_offset_y);
        self
    }

    #[inline(always)]
    pub fn unstyled(mut self) -> Self {
        self.unstyled = true;
        self.inner = self.inner.unstyled();
        self
    }
}

impl<'a, F> Styled for UniformList<'a, F> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a, F> StyledExt for UniformList<'a, F> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a, F> Widget<'a> for UniformList<'a, F>
where
    F: Fn(usize, &'a FrameScope) -> Element<'a>,
{
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        if !self.unstyled {
            let theme = cx.theme();
            let style = self.inner.style_mut();
            if style.bg == Color::TRANSPARENT {
                style.bg = theme.card;
            }
            if style.border_px() == 0 {
                style.set_border_px(1);
                style.border_color = theme.border;
            }
            if style.rounded_radius() == 0 {
                style.set_rounded(6);
            }
        }
        self.inner.render_once(cx)
    }
}

/// 创建一个主题化等高虚拟滚动列表。
#[inline]
pub fn uniform_list<'a, F>(
    id: u32,
    item_count: usize,
    item_height: i16,
    viewport_height: i16,
    scroll_offset_y: i16,
    render_item: F,
) -> UniformList<'a, F>
where
    F: Fn(usize, &'a FrameScope) -> Element<'a>,
{
    UniformList::new(
        id,
        item_count,
        item_height,
        viewport_height,
        scroll_offset_y,
        render_item,
    )
}

/// 创建一个主题化虚拟滚动列表（与 [`uniform_list`] 等价）。
#[inline]
pub fn virtual_list<'a, F>(
    id: u32,
    item_count: usize,
    item_height: i16,
    viewport_height: i16,
    scroll_offset_y: i16,
    render_item: F,
) -> VirtualList<'a, F>
where
    F: Fn(usize, &'a FrameScope) -> Element<'a>,
{
    uniform_list(
        id,
        item_count,
        item_height,
        viewport_height,
        scroll_offset_y,
        render_item,
    )
}
