//! 顶部导航栏基础组件 `NavBar`（默认无样式，支持居中标题、左侧返回按钮、右侧操作按钮与移动端顶部 `SafeAreaInsets` 自动避让）。

use velox_core::prelude::*;

/// 顶部导航栏基础组件（`NavBar`，默认无样式）。
pub struct NavBar<'a> {
    pub id: u32,
    pub title: StrRef<'a>,
    pub back_label: Option<StrRef<'a>>,
    pub action_label: Option<StrRef<'a>>,
    pub content_height: i16,
    pub safe_top: bool,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub action_color: Color,
    pub action_hover_bg: Option<Color>,
    pub action_active_bg: Option<Color>,
    pub action_rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_back: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_action: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> NavBar<'a> {
    /// 创建一个默认无样式的顶部导航栏（默认开启顶部 `safe_area` 自动内边距）。
    #[inline(always)]
    pub fn new(id: u32, title: impl Into<StrRef<'a>>) -> Self {
        Self {
            id,
            title: title.into(),
            back_label: None,
            action_label: None,
            content_height: 44,
            safe_top: true,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            action_color: Color::TRANSPARENT,
            action_hover_bg: None,
            action_active_bg: None,
            action_rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_back: None,
            on_action: None,
        }
    }

    /// 设置左侧返回按钮标签与点击回调。
    #[inline(always)]
    pub fn back(
        mut self,
        label: impl Into<StrRef<'a>>,
        handler: &'a dyn Fn(&mut ActionScope),
    ) -> Self {
        self.back_label = Some(label.into());
        self.on_back = Some(handler);
        self
    }

    /// 设置右侧操作按钮标签与点击回调。
    #[inline(always)]
    pub fn action(
        mut self,
        label: impl Into<StrRef<'a>>,
        handler: &'a dyn Fn(&mut ActionScope),
    ) -> Self {
        self.action_label = Some(label.into());
        self.on_action = Some(handler);
        self
    }

    /// 设置是否自动叠加移动端顶部 `safe_area.top` 安全边距（默认 `true`）。
    #[inline(always)]
    pub const fn safe_top(mut self, enabled: bool) -> Self {
        self.safe_top = enabled;
        self
    }

    /// 设置导航栏内容区高度（不含顶部安全区，默认 `44px`）。
    #[inline(always)]
    pub const fn h(mut self, height: i16) -> Self {
        self.content_height = if height < 28 { 28 } else { height };
        self
    }
}

crate::impl_base_styled_box!(NavBar<'a>);

impl<'a> Widget<'a> for NavBar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };
        let action_color = if self.action_color.a() > 0 {
            self.action_color
        } else {
            fg
        };

        let top_pad = if self.safe_top {
            cx.safe_area().top.max(0)
        } else {
            0
        };
        let total_h = self.content_height.saturating_add(top_pad);

        let has_side_slots = self.back_label.is_some() || self.action_label.is_some();
        let mut bar = div()
            .id(self.id)
            .flex_row()
            .w_full()
            .h(total_h)
            .pt(top_pad.clamp(0, 127) as i8)
            .px(12)
            .items_center()
            .bg(self.bg);
        if self.border_color.a() > 0 {
            bar = bar.border(1, self.border_color);
        }

        if !has_side_slots {
            bar = bar
                .justify_center()
                .child(cx, text(self.title).size(16).color(title_color));
            return self.style_ext.apply_to_div(bar, self.style).into_element(cx);
        }

        bar = bar.justify_between();

        let mut left_slot = div()
            .id(crate::scoped_child_id(self.id, 0x5323_D6E8, 1))
            .flex_row()
            .w(80)
            .h(self.content_height)
            .px(6)
            .rounded(self.action_rounded)
            .items_center();
        if let Some(back_txt) = self.back_label {
            if let Some(cb) = self.on_back {
                if let Some(h) = self.action_hover_bg {
                    left_slot = left_slot.hover_bg(h);
                }
                if let Some(a) = self.action_active_bg {
                    left_slot = left_slot.active_bg(a);
                }
                left_slot = left_slot.on_click_raw(cb);
            }
            left_slot = left_slot.child(cx, text(back_txt).size(14).nowrap().color(action_color));
        }

        let mut right_slot = div()
            .id(crate::scoped_child_id(self.id, 0x5323_D6E8, 2))
            .flex_row()
            .w(80)
            .h(self.content_height)
            .px(6)
            .rounded(self.action_rounded)
            .items_center()
            .justify_end();
        if let Some(act_txt) = self.action_label {
            if let Some(cb) = self.on_action {
                if let Some(h) = self.action_hover_bg {
                    right_slot = right_slot.hover_bg(h);
                }
                if let Some(a) = self.action_active_bg {
                    right_slot = right_slot.active_bg(a);
                }
                right_slot = right_slot.on_click_raw(cb);
            }
            right_slot = right_slot.child(cx, text(act_txt).size(14).nowrap().color(action_color));
        }

        bar = bar
            .child(cx, left_slot)
            .child(cx, text(self.title).size(16).nowrap().color(title_color))
            .child(cx, right_slot);
        bar = self.style_ext.apply_to_div(bar, self.style);
        bar.into_element(cx)
    }
}
