//! 尾部自动跟随虚拟化消息列表组件 `MessageScroller`。

use crate::components::Message;
use velox_core::prelude::*;

/// 尾部跟随虚拟消息滚动列表状态（`MessageScrollerState`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageScrollerState {
    pub scroll_offset_y: i32,
    pub follow_tail: bool,
    pub unread_count: u16,
}

impl Default for MessageScrollerState {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageScrollerState {
    /// 创建一个默认开启尾部自动跟随（`follow_tail = true`）的滚动状态。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            scroll_offset_y: 0,
            follow_tail: true,
            unread_count: 0,
        }
    }

    /// 当用户手动向上滚动时更新偏移量并自动解除尾部锁定（支持百万像素级 `i32` 大列表偏移）。
    pub fn scroll_to(&mut self, offset_y: i32, max_scroll_y: i32) {
        let max_s = max_scroll_y.max(0);
        self.scroll_offset_y = offset_y.clamp(0, max_s);
        self.follow_tail = self.scroll_offset_y >= (max_s - 8).max(0);
        if self.follow_tail {
            self.unread_count = 0;
        }
    }

    /// 立即跳回消息流底部并恢复尾部跟随。
    #[inline]
    pub fn scroll_to_bottom(&mut self, max_scroll_y: i32) {
        self.scroll_offset_y = max_scroll_y.max(0);
        self.follow_tail = true;
        self.unread_count = 0;
    }

    /// 当有新消息到达时同步更新滚动状态。
    #[inline]
    pub fn on_new_message(&mut self, new_max_scroll_y: i32) {
        if self.follow_tail {
            self.scroll_offset_y = new_max_scroll_y.max(0);
        } else {
            self.unread_count = self.unread_count.saturating_add(1);
        }
    }
}

/// 尾部跟随虚拟化消息列表基础组件（`MessageScroller`，默认无样式）。
pub struct MessageScroller<'a> {
    pub id: u32,
    pub messages: &'a [Message<'a>],
    pub state: MessageScrollerState,
    pub width: i16,
    pub height: i16,
    pub estimated_row_height: i16,
    pub bg: Color,
    pub border_color: Color,
    pub jump_bg: Color,
    pub jump_text_color: Color,
    pub rounded: u8,
    pub jump_rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_jump_bottom: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> MessageScroller<'a> {
    /// 创建一个默认无样式的尾部跟随虚拟化消息滚动列表。
    #[inline(always)]
    pub const fn new(
        id: u32,
        messages: &'a [Message<'a>],
        state: MessageScrollerState,
        width: i16,
        height: i16,
    ) -> Self {
        Self {
            id,
            messages,
            state,
            width,
            height,
            estimated_row_height: 64,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            jump_bg: Color::TRANSPARENT,
            jump_text_color: Color::TRANSPARENT,
            rounded: 0,
            jump_rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_jump_bottom: None,
        }
    }

    /// 设置圆角半径。
    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.rounded = radius;
        self
    }

    /// 绑定点击“回到底部”浮动按钮的回调。
    #[inline(always)]
    pub fn on_jump_bottom(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_jump_bottom = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(MessageScroller<'a>);

impl<'a> Widget<'a> for MessageScroller<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let jump_fg = if self.jump_text_color.a() > 0 {
            self.jump_text_color
        } else {
            cx.theme().foreground
        };

        let row_h = (self.estimated_row_height.max(32)) as i32;
        let total_h = ((self.messages.len() as i64) * (row_h as i64)).min(i32::MAX as i64) as i32;
        let footer_h = if self.state.follow_tail { 0i16 } else { 28i16 };
        let inner_view_h = (height - 16 - footer_h).max(24);
        let max_scroll = (total_h - (inner_view_h as i32)).max(0);
        let eff_scroll = if self.state.follow_tail {
            max_scroll
        } else {
            self.state.scroll_offset_y.clamp(0, max_scroll)
        };

        let first_idx = ((eff_scroll / row_h).max(0) as usize).min(self.messages.len());
        let visible_cnt = ((inner_view_h as i32 + row_h - 1) / row_h) as usize + 1;
        let end_idx = first_idx.saturating_add(visible_cnt).min(self.messages.len());

        let mut outer = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(height)
            .p(8)
            .gap(4)
            .rounded(self.rounded)
            .bg(self.bg)
            .overflow_hidden();
        if self.border_color.a() > 0 {
            outer = outer.border(1, self.border_color);
        }

        let mut list_col = div()
            .flex_col()
            .w_full()
            .h(inner_view_h)
            .gap(8)
            .overflow_hidden();

        for idx in first_idx..end_idx {
            let mut msg = self.messages[idx];
            msg.width = (width - 16).max(120);
            list_col = list_col.child(cx, msg);
        }

        outer = outer.child(cx, list_col);

        if !self.state.follow_tail {
            let mut jump_btn = div()
                .id(crate::scoped_child_id(self.id, 0xF6F6_E8E7, 999))
                .px(10)
                .py(4)
                .rounded(self.jump_rounded)
                .bg(self.jump_bg)
                .items_center()
                .justify_center()
                .focusable();
            if !self.focus_ring {
                jump_btn = jump_btn.no_focus_border();
            }
            jump_btn = jump_btn.child(
                cx,
                text(if self.state.unread_count > 0 {
                    velox_core::fmt!(cx, "↓ {} new · Jump to latest", self.state.unread_count)
                } else {
                    StrRef::Static("↓ Jump to latest")
                })
                .size(11)
                .color(jump_fg),
            );
            if let Some(cb) = self.on_jump_bottom {
                jump_btn = jump_btn.on_click_raw(cb);
            }
            outer = outer.child(
                cx,
                div()
                    .w_full()
                    .h(24)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .child(cx, jump_btn),
            );
        }

        outer = self.style_ext.apply_to_div(outer, self.style);
        outer.into_element(cx)
    }
}
