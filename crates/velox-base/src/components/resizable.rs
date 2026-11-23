//! 可调整分栏布局基础组件 `ResizableSplit` 与停靠工作区状态 `DockWorkspaceState`（默认无样式）。

use velox_core::prelude::*;

/// 分栏切割方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SplitAxis {
    #[default]
    Horizontal,
    Vertical,
}

/// 面向 IDE / 桌面端多面板工作区的停靠布局状态（`DockWorkspaceState`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockWorkspaceState {
    pub left_width: i16,
    pub right_width: i16,
    pub bottom_height: i16,
    pub left_visible: bool,
    pub right_visible: bool,
    pub bottom_visible: bool,
}

impl Default for DockWorkspaceState {
    fn default() -> Self {
        Self::new(220, 240, 160)
    }
}

impl DockWorkspaceState {
    /// 创建具有左、右、底三侧停靠面板尺寸的工作区状态。
    #[inline]
    pub const fn new(left_width: i16, right_width: i16, bottom_height: i16) -> Self {
        Self {
            left_width,
            right_width,
            bottom_height,
            left_visible: true,
            right_visible: true,
            bottom_visible: true,
        }
    }

    /// 调整左侧停靠栏宽度并限制在 `[min_w, max_w]` 内。
    #[inline]
    pub fn resize_left(&mut self, delta: i16, min_w: i16, max_w: i16) {
        self.left_width = self.left_width.saturating_add(delta).clamp(min_w, max_w);
    }

    /// 调整底部停靠栏高度并限制在 `[min_h, max_h]` 内。
    #[inline]
    pub fn resize_bottom(&mut self, delta: i16, min_h: i16, max_h: i16) {
        self.bottom_height = self.bottom_height.saturating_add(delta).clamp(min_h, max_h);
    }
}

/// `Resizable` 可调整分栏布局组件类型别名（与 [`ResizableSplit`] 等价）。
pub type Resizable<'a> = ResizableSplit<'a>;

/// 双面板可拖拽分隔容器基础组件（`ResizableSplit`，默认无样式）。
pub struct ResizableSplit<'a> {
    pub id: u32,
    pub axis: SplitAxis,
    pub width: i16,
    pub height: i16,
    /// 第一个面板的尺寸（水平分栏时为左面板宽度，垂直分栏时为上面板高度）。
    pub primary_size: i16,
    pub handle_thickness: i16,
    pub handle_color: Color,
    pub handle_hover_color: Option<Color>,
    pub handle_active_color: Option<Color>,
    pub style: Style,
    pub style_ext: StyleExt,
    pub first: Option<Element<'a>>,
    pub second: Option<Element<'a>>,
    pub on_resize: Option<&'a dyn Fn(i16, &mut ActionScope)>,
}

impl<'a> ResizableSplit<'a> {
    /// 创建一个默认无样式的可调节分栏布局组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        axis: SplitAxis,
        width: i16,
        height: i16,
        primary_size: i16,
    ) -> Self {
        Self {
            id,
            axis,
            width,
            height,
            primary_size,
            handle_thickness: 4,
            handle_color: Color::TRANSPARENT,
            handle_hover_color: None,
            handle_active_color: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            first: None,
            second: None,
            on_resize: None,
        }
    }

    /// 设置主面板（左/上）内容。
    #[inline(always)]
    pub fn first(mut self, cx: &'a FrameScope, el: impl IntoElement<'a>) -> Self {
        self.first = Some(el.into_element(cx));
        self
    }

    /// 设置次面板（右/下）内容。
    #[inline(always)]
    pub fn second(mut self, cx: &'a FrameScope, el: impl IntoElement<'a>) -> Self {
        self.second = Some(el.into_element(cx));
        self
    }

    /// 绑定拖拽分隔条调整主面板尺寸的回调（参数为更新后的 `primary_size` 像素值）。
    #[inline(always)]
    pub fn on_resize(mut self, handler: &'a dyn Fn(i16, &mut ActionScope)) -> Self {
        self.on_resize = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(ResizableSplit<'a>);

impl<'a> Widget<'a> for ResizableSplit<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let handle_color = if self.handle_color.a() > 0 {
            self.handle_color
        } else {
            cx.theme().foreground.scale_alpha(48)
        };

        match self.axis {
            SplitAxis::Horizontal => {
                let max_first = (width - self.handle_thickness - 20).max(20);
                let first_w = self.primary_size.clamp(20, max_first);
                let second_w = (width - first_w - self.handle_thickness).max(0);

                let mut p1 = div().w(first_w).h(height).overflow_hidden();
                if let Some(f) = self.first {
                    p1 = p1.child(cx, f);
                }
                let mut handle = div()
                    .id(self.id ^ 0x5350_4C54)
                    .w(self.handle_thickness)
                    .h(height)
                    .bg(handle_color)
                    .cursor(CursorShape::ResizeEW)
                    .focusable()
                    .no_focus_border();
                if let Some(h) = self.handle_hover_color {
                    handle = handle.hover_bg(h);
                }
                if let Some(a) = self.handle_active_color {
                    handle = handle.active_bg(a);
                }
                if let Some(cb) = self.on_resize {
                    let raw_drag = cx.arena().alloc(move |acx: &mut ActionScope| {
                        let ev = acx.drag_event();
                        let next_w = ev.local_x.saturating_add(first_w).clamp(20, max_first);
                        cb(next_w, acx);
                    });
                    handle = handle.on_drag_raw(raw_drag);
                }
                let mut p2 = div().w(second_w).h(height).overflow_hidden();
                if let Some(s) = self.second {
                    p2 = p2.child(cx, s);
                }

                let root = div()
                    .id(self.id)
                    .flex_row()
                    .w(width)
                    .h(height)
                    .child(cx, p1)
                    .child(cx, handle)
                    .child(cx, p2);
                self.style_ext.apply_to_div(root, self.style).into_element(cx)
            }
            SplitAxis::Vertical => {
                let max_first = (height - self.handle_thickness - 20).max(20);
                let first_h = self.primary_size.clamp(20, max_first);
                let second_h = (height - first_h - self.handle_thickness).max(0);

                let mut p1 = div().w(width).h(first_h).overflow_hidden();
                if let Some(f) = self.first {
                    p1 = p1.child(cx, f);
                }
                let mut handle = div()
                    .id(self.id ^ 0x5350_4C54)
                    .w(width)
                    .h(self.handle_thickness)
                    .bg(handle_color)
                    .cursor(CursorShape::ResizeNS)
                    .focusable()
                    .no_focus_border();
                if let Some(h) = self.handle_hover_color {
                    handle = handle.hover_bg(h);
                }
                if let Some(a) = self.handle_active_color {
                    handle = handle.active_bg(a);
                }
                if let Some(cb) = self.on_resize {
                    let raw_drag = cx.arena().alloc(move |acx: &mut ActionScope| {
                        let ev = acx.drag_event();
                        let next_h = ev.local_y.saturating_add(first_h).clamp(20, max_first);
                        cb(next_h, acx);
                    });
                    handle = handle.on_drag_raw(raw_drag);
                }
                let mut p2 = div().w(width).h(second_h).overflow_hidden();
                if let Some(s) = self.second {
                    p2 = p2.child(cx, s);
                }

                let root = div()
                    .id(self.id)
                    .flex_col()
                    .w(width)
                    .h(height)
                    .child(cx, p1)
                    .child(cx, handle)
                    .child(cx, p2);
                self.style_ext.apply_to_div(root, self.style).into_element(cx)
            }
        }
    }
}
