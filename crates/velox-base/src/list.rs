//! `uniform_list` 与 `VirtualList` 虚拟滚动列表原语。
//!
//! 仅在 `ScratchArena` 中展开当前视口可见范围内的列表项，
//! 无论总数据量是 10 条还是 10,000 条，单帧 `ScratchArena` 内存消耗始终保持恒定。

use velox_engine::prelude::*;

/// `VirtualList` 虚拟滚动列表组件类型别名（与 [`UniformList`] 等价）。
pub type VirtualList<'a, F> = UniformList<'a, F>;

/// 创建一个虚拟滚动列表原语（与 [`uniform_list`] 等价）。
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

/// 等高虚拟滚动列表配置与构建器（默认无样式纯容器，支持百万像素级 `i32` 大列表偏移）。
pub struct UniformList<'a, F> {
    id: u32,
    item_count: usize,
    item_height: i16,
    viewport_height: i16,
    scroll_offset_y: i32,
    style: Style,
    style_ext: StyleExt,
    render_item: F,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a, F> UniformList<'a, F>
where
    F: Fn(usize, &'a FrameScope) -> Element<'a>,
{
    /// 创建一个等高虚拟滚动列表组件。
    #[inline]
    pub fn new(
        id: u32,
        item_count: usize,
        item_height: i16,
        viewport_height: i16,
        scroll_offset_y: i16,
        render_item: F,
    ) -> Self {
        uniform_list(
            id,
            item_count,
            item_height,
            viewport_height,
            scroll_offset_y,
            render_item,
        )
    }

    /// 设置大范围 32 位垂直滚动偏移（支持超过 `32,767px` 的超长数据列表）。
    #[inline(always)]
    pub const fn scroll_y_i32(mut self, scroll_offset_y: i32) -> Self {
        self.scroll_offset_y = if scroll_offset_y > 0 { scroll_offset_y } else { 0 };
        self
    }

    /// 清除外框背景、边框与圆角，保持完全透明容器。
    #[inline(always)]
    pub fn unstyled(mut self) -> Self {
        self.style.bg = Color::TRANSPARENT;
        self.style.set_border_px(0);
        self.style.border_color = Color::TRANSPARENT;
        self.style.set_rounded(0);
        self
    }
}

/// 创建一个默认无样式的等高虚拟滚动列表原语。
///
/// - `id`：列表容器跨帧唯一 ID；
/// - `item_count`：总数据条数（例如 10000 条）；
/// - `item_height`：单行固定高度（像素）；
/// - `viewport_height`：可视窗口高度（像素）；
/// - `scroll_offset_y`：当前垂直滚动像素偏移；
/// - `render_item`：仅对可见索引 `idx` 调用的单行渲染闭包 `Fn(usize, &'a FrameScope) -> Element<'a>`。
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
    let mut style = Style::default();
    style.set_col();
    style.width = Length::Full;
    style.height = Length::Px(viewport_height);
    UniformList {
        id,
        item_count,
        item_height: item_height.max(1),
        viewport_height: viewport_height.max(1),
        scroll_offset_y: (scroll_offset_y as i32).max(0),
        style,
        style_ext: StyleExt::DEFAULT,
        render_item,
        _marker: core::marker::PhantomData,
    }
}

crate::impl_base_styled_box!(UniformList<'a, F>);

impl<'a, F> Widget<'a> for UniformList<'a, F>
where
    F: Fn(usize, &'a FrameScope) -> Element<'a>,
{
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let item_h = (self.item_height.max(1)) as i32;
        let view_h = (self.viewport_height.max(1)) as i32;
        let total_h = ((self.item_count as i64) * (item_h as i64)).min(i32::MAX as i64) as i32;
        let max_scroll = (total_h - view_h).max(0);
        let clamped_scroll = self.scroll_offset_y.clamp(0, max_scroll);

        let first_idx = ((clamped_scroll / item_h).max(0) as usize).min(self.item_count);
        let visible_rows = ((view_h + item_h - 1) / item_h) as usize;
        let end_idx = first_idx.saturating_add(visible_rows).min(self.item_count);

        let mut container = div().id(self.id).overflow_hidden();
        container.style = self.style;
        container = self.style_ext.apply_ext_only(container);

        for idx in first_idx..end_idx {
            let item_el = (self.render_item)(idx, cx);
            let row_wrapper = div()
                .w_full()
                .h(self.item_height)
                .flex_row()
                .items_center()
                .child(cx, item_el);
            container = container.child(cx, row_wrapper);
        }

        container.into_element(cx)
    }
}
