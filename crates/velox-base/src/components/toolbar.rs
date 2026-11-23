//! 工具栏基础组件 `Toolbar`（默认无样式）。

use crate::traits::{ComponentSize, Sizable};
use velox_core::prelude::*;

/// 工具栏动作按钮分组描述（`ToolbarGroup`）。
#[derive(Clone, Copy)]
pub struct ToolbarGroup<'a> {
    pub labels: &'a [&'a str],
}

impl<'a> ToolbarGroup<'a> {
    /// 创建一个工具栏按钮组。
    #[inline(always)]
    pub const fn new(labels: &'a [&'a str]) -> Self {
        Self { labels }
    }
}

/// 顶部或内嵌工具栏基础组件（`Toolbar`，默认无样式，支持左/右区域与尺寸档位）。
pub struct Toolbar<'a> {
    pub id: u32,
    pub groups: &'a [ToolbarGroup<'a>],
    pub right_groups: &'a [ToolbarGroup<'a>],
    pub active_index: Option<(usize, usize)>,
    pub width: Option<i16>,
    pub height: i16,
    pub size: ComponentSize,
    pub bg: Color,
    pub border_color: Color,
    pub item_bg: Color,
    pub active_bg: Color,
    pub hover_bg: Option<Color>,
    pub press_bg: Option<Color>,
    pub text_color: Color,
    pub active_fg: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_action: Option<&'a dyn Fn(usize, usize, &mut ActionScope)>,
}

impl<'a> Toolbar<'a> {
    /// 创建一个默认无样式的工具栏。
    #[inline(always)]
    pub const fn new(id: u32, groups: &'a [ToolbarGroup<'a>]) -> Self {
        Self {
            id,
            groups,
            right_groups: &[],
            active_index: None,
            width: None,
            height: 34,
            size: ComponentSize::Medium,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            item_bg: Color::TRANSPARENT,
            active_bg: Color::TRANSPARENT,
            hover_bg: None,
            press_bg: None,
            text_color: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_action: None,
        }
    }

    /// 设置右侧区域工具栏按钮组。
    #[inline(always)]
    pub const fn right_groups(mut self, groups: &'a [ToolbarGroup<'a>]) -> Self {
        self.right_groups = groups;
        self
    }

    /// 设置工具栏总宽度（设置后左/右区域自动两端对齐）。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = Some(width);
        self
    }

    /// 设置当前激活的 `(group_idx, item_idx)`。
    #[inline(always)]
    pub const fn active(mut self, group_idx: usize, item_idx: usize) -> Self {
        self.active_index = Some((group_idx, item_idx));
        self
    }

    /// 绑定点击工具栏按钮回调 `(group_idx, item_idx, &mut ActionScope)`。
    #[inline(always)]
    pub fn on_action(mut self, handler: &'a dyn Fn(usize, usize, &mut ActionScope)) -> Self {
        self.on_action = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Toolbar<'a>);

impl<'a> Sizable for Toolbar<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self.height = match size {
            ComponentSize::Small => 28,
            ComponentSize::Medium => 34,
            ComponentSize::Large => 42,
        };
        self
    }
}

impl<'a> Widget<'a> for Toolbar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let active_fg = if self.active_fg.a() > 0 {
            self.active_fg
        } else {
            fg
        };

        let mut bar = div()
            .id(self.id)
            .flex_row()
            .h(self.height)
            .px(6)
            .gap(6)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center();
        if self.border_color.a() > 0 {
            bar = bar.border(1, self.border_color);
        }

        if let Some(w) = self.width {
            bar = bar.w(w).justify_between();
        }

        let on_act = self.on_action;
        let item_rounded = self.rounded.saturating_sub(2);
        let mut btn_seq = 1u32;

        let mut left_row = div().flex_row().gap(6).items_center();
        for (g_idx, grp) in self.groups.iter().enumerate() {
            if g_idx > 0 && self.border_color.a() > 0 {
                left_row = left_row.child(cx, div().w(1).h(self.height - 12).bg(self.border_color));
            }
            for (i_idx, &lbl) in grp.labels.iter().enumerate() {
                let is_active = self.active_index == Some((g_idx, i_idx));
                let mut btn = div()
                    .id(crate::scoped_child_id(self.id, 0xFD12_C898, btn_seq))
                    .h(self.height - 8)
                    .px(8)
                    .rounded(item_rounded)
                    .bg(if is_active {
                        self.active_bg
                    } else {
                        self.item_bg
                    })
                    .items_center()
                    .justify_center()
                    .focusable();
                if let Some(h) = self.hover_bg {
                    btn = btn.hover_bg(if is_active { self.active_bg } else { h });
                }
                if let Some(p) = self.press_bg {
                    btn = btn.active_bg(p);
                }
                if !self.focus_ring {
                    btn = btn.no_focus_border();
                }
                btn = btn.child(
                    cx,
                    text(lbl)
                        .size(12)
                        .color(if is_active { active_fg } else { text_color })
                        .text_center()
                        .nowrap(),
                );
                btn_seq += 1;

                if let Some(cb) = on_act {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(g_idx, i_idx, scope);
                    });
                    btn = btn.on_click_raw(click_cb);
                }
                left_row = left_row.child(cx, btn);
            }
        }
        bar = bar.child(cx, left_row);

        if !self.right_groups.is_empty() {
            let mut right_row = div().flex_row().gap(6).items_center();
            let base_g = self.groups.len();
            for (rg_idx, grp) in self.right_groups.iter().enumerate() {
                let g_idx = base_g + rg_idx;
                if rg_idx > 0 && self.border_color.a() > 0 {
                    right_row =
                        right_row.child(cx, div().w(1).h(self.height - 12).bg(self.border_color));
                }
                for (i_idx, &lbl) in grp.labels.iter().enumerate() {
                    let is_active = self.active_index == Some((g_idx, i_idx));
                    let mut btn = div()
                        .id(crate::scoped_child_id(self.id, 0xFD12_C898, btn_seq))
                        .h(self.height - 8)
                        .px(8)
                        .rounded(item_rounded)
                        .bg(if is_active {
                            self.active_bg
                        } else {
                            self.item_bg
                        })
                        .items_center()
                        .justify_center()
                        .focusable();
                    if let Some(h) = self.hover_bg {
                        btn = btn.hover_bg(if is_active { self.active_bg } else { h });
                    }
                    if let Some(p) = self.press_bg {
                        btn = btn.active_bg(p);
                    }
                    if !self.focus_ring {
                        btn = btn.no_focus_border();
                    }
                    btn = btn.child(
                        cx,
                        text(lbl)
                            .size(12)
                            .color(if is_active { active_fg } else { text_color })
                            .text_center()
                            .nowrap(),
                    );
                    btn_seq += 1;
                    if let Some(cb) = on_act {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(g_idx, i_idx, scope);
                        });
                        btn = btn.on_click_raw(click_cb);
                    }
                    right_row = right_row.child(cx, btn);
                }
            }
            bar = bar.child(cx, right_row);
        }

        bar = self.style_ext.apply_to_div(bar, self.style);
        bar.into_element(cx)
    }
}
