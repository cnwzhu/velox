//! 可滚动容器基础组件 `Scrollable` 与滚动条 `Scrollbar`（默认无样式）。

use velox_core::prelude::*;

/// 滚动条方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScrollbarAxis {
    #[default]
    Vertical,
    Horizontal,
}

/// 比例滚动条指示器基础组件（`Scrollbar`，默认无样式）。
pub struct Scrollbar {
    pub id: u32,
    pub axis: ScrollbarAxis,
    pub track_length: i16,
    pub content_size: i32,
    pub viewport_size: i32,
    pub scroll_offset: i32,
    pub thickness: i16,
    pub track_color: Color,
    pub thumb_color: Color,
    /// `u8::MAX` 表示自动圆角 `thickness / 2`，默认 `0`（直角）。
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl Scrollbar {
    /// 创建一个默认无样式的垂直或水平滚动条组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        axis: ScrollbarAxis,
        track_length: i16,
        content_size: i32,
        viewport_size: i32,
        scroll_offset: i32,
    ) -> Self {
        Self {
            id,
            axis,
            track_length,
            content_size,
            viewport_size,
            scroll_offset,
            thickness: 6,
            track_color: Color::TRANSPARENT,
            thumb_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 计算滑块在轨道内的 `(thumb_offset_px, thumb_length_px)`。
    pub fn thumb_metrics(&self) -> (i16, i16) {
        let track = self.track_length.max(16) as i32;
        if self.content_size <= self.viewport_size || self.content_size <= 0 {
            return (0, track as i16);
        }
        let thumb_len = ((track * self.viewport_size) / self.content_size).clamp(16, track);
        let max_scroll = (self.content_size - self.viewport_size).max(1);
        let clamped_scroll = self.scroll_offset.clamp(0, max_scroll);
        let avail_track = (track - thumb_len).max(0);
        let offset = (clamped_scroll * avail_track) / max_scroll;
        (offset as i16, thumb_len as i16)
    }
}

crate::impl_base_styled_box!(Scrollbar);

impl<'a> Widget<'a> for Scrollbar {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let thumb_color = if self.thumb_color.a() > 0 {
            self.thumb_color
        } else {
            cx.theme().foreground.scale_alpha(80)
        };
        let rounded = if self.rounded == u8::MAX {
            (self.thickness / 2).max(0) as u8
        } else {
            self.rounded
        };

        let (offset, thumb_len) = self.thumb_metrics();
        let root = match self.axis {
            ScrollbarAxis::Vertical => div()
                .id(self.id)
                .flex_col()
                .w(self.thickness)
                .h(self.track_length)
                .rounded(rounded)
                .bg(self.track_color)
                .child(cx, div().w(self.thickness).h(offset))
                .child(
                    cx,
                    div()
                        .w(self.thickness)
                        .h(thumb_len)
                        .rounded(rounded)
                        .bg(thumb_color),
                ),
            ScrollbarAxis::Horizontal => div()
                .id(self.id)
                .flex_row()
                .w(self.track_length)
                .h(self.thickness)
                .rounded(rounded)
                .bg(self.track_color)
                .child(cx, div().w(offset).h(self.thickness))
                .child(
                    cx,
                    div()
                        .w(thumb_len)
                        .h(self.thickness)
                        .rounded(rounded)
                        .bg(thumb_color),
                ),
        };
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}

/// 可滚动视口容器基础组件（`Scrollable`，默认无样式，内置滚轮/触控拖拽滚动与比例滚动条指示器）。
pub struct Scrollable<'a> {
    pub id: u32,
    pub axis: ScrollbarAxis,
    pub width: i16,
    pub height: i16,
    pub content_size: i32,
    pub scroll_offset: i32,
    pub show_scrollbar: bool,
    pub track_color: Color,
    pub thumb_color: Color,
    pub scrollbar_rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub child: Option<Element<'a>>,
}

impl<'a> Scrollable<'a> {
    /// 创建一个默认无样式的可滚动容器组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        width: i16,
        height: i16,
        content_size: i32,
        scroll_offset: i32,
    ) -> Self {
        Self {
            id,
            axis: ScrollbarAxis::Vertical,
            width,
            height,
            content_size,
            scroll_offset,
            show_scrollbar: true,
            track_color: Color::TRANSPARENT,
            thumb_color: Color::TRANSPARENT,
            scrollbar_rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            child: None,
        }
    }

    /// 设置滚动方向（默认垂直滚动）。
    #[inline(always)]
    pub const fn axis(mut self, axis: ScrollbarAxis) -> Self {
        self.axis = axis;
        self
    }

    /// 设置是否显示滚动条指示器。
    #[inline(always)]
    pub const fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// 设置容器内部可滚动子元素。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.child = Some(child.into_element(cx));
        self
    }
}

crate::impl_base_styled_box!(Scrollable<'a>);

impl<'a> Widget<'a> for Scrollable<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let bar_thick = if self.show_scrollbar { 6i16 } else { 0 };

        let wrap = match self.axis {
            ScrollbarAxis::Vertical => {
                let inner_w = (width - bar_thick).max(1);
                let max_scroll = (self.content_size - (height as i32))
                    .clamp(0, (i16::MAX - 1) as i32) as i16;
                let mut viewport = div()
                    .id(self.id ^ 0x5343_5650)
                    .flex_col()
                    .w(inner_w)
                    .h(height)
                    .scrollable(max_scroll);
                if let Some(c) = self.child {
                    viewport = viewport.child(cx, c);
                }
                let mut wrap = div()
                    .id(self.id)
                    .flex_row()
                    .w(width)
                    .h(height)
                    .overflow_hidden()
                    .child(cx, viewport);
                if self.show_scrollbar {
                    let mut sb = Scrollbar::new(
                        self.id ^ 0x5343_524C,
                        ScrollbarAxis::Vertical,
                        height,
                        self.content_size,
                        height as i32,
                        self.scroll_offset,
                    );
                    sb.track_color = self.track_color;
                    sb.thumb_color = self.thumb_color;
                    sb.rounded = self.scrollbar_rounded;
                    wrap = wrap.child(cx, sb);
                }
                wrap
            }
            ScrollbarAxis::Horizontal => {
                let inner_h = (height - bar_thick).max(1);
                let max_scroll = (self.content_size - (width as i32))
                    .clamp(0, (i16::MAX - 1) as i32) as i16;
                let mut viewport = div()
                    .id(self.id ^ 0x5343_5650)
                    .flex_row()
                    .w(width)
                    .h(inner_h)
                    .scrollable(max_scroll);
                if let Some(c) = self.child {
                    viewport = viewport.child(cx, c);
                }
                let mut wrap = div()
                    .id(self.id)
                    .flex_col()
                    .w(width)
                    .h(height)
                    .overflow_hidden()
                    .child(cx, viewport);
                if self.show_scrollbar {
                    let mut sb = Scrollbar::new(
                        self.id ^ 0x5343_524C,
                        ScrollbarAxis::Horizontal,
                        width,
                        self.content_size,
                        width as i32,
                        self.scroll_offset,
                    );
                    sb.track_color = self.track_color;
                    sb.thumb_color = self.thumb_color;
                    sb.rounded = self.scrollbar_rounded;
                    wrap = wrap.child(cx, sb);
                }
                wrap
            }
        };
        self.style_ext.apply_to_div(wrap, self.style).into_element(cx)
    }
}
