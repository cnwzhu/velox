//! 多面板停靠工作区基础组件 `Dock`（默认无样式）。

pub use crate::components::resizable::DockWorkspaceState;
use velox_engine::prelude::*;

/// 停靠面板区域描述（`DockPanel`）。
#[derive(Clone, Copy)]
pub struct DockPanel<'a> {
    pub tabs: &'a [&'a str],
    pub active_tab: usize,
    pub body_text: &'a str,
}

impl<'a> DockPanel<'a> {
    /// 创建一个带标签页栏的停靠面板。
    #[inline(always)]
    pub const fn new(tabs: &'a [&'a str], active_tab: usize, body_text: &'a str) -> Self {
        Self {
            tabs,
            active_tab,
            body_text,
        }
    }
}

/// 面向 IDE / 专业工具的多区域停靠工作区布局基础组件（`Dock`，默认无样式）。
pub struct Dock<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub state: DockWorkspaceState,
    pub left_panel: Option<DockPanel<'a>>,
    pub right_panel: Option<DockPanel<'a>>,
    pub bottom_panel: Option<DockPanel<'a>>,
    pub center: Option<Element<'a>>,
    pub bg: Color,
    pub panel_bg: Color,
    pub tab_bar_bg: Color,
    pub active_tab_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Dock<'a> {
    /// 创建一个默认无样式的停靠工作区布局组件。
    #[inline(always)]
    pub const fn new(id: u32, width: i16, height: i16, state: DockWorkspaceState) -> Self {
        Self {
            id,
            width,
            height,
            state,
            left_panel: None,
            right_panel: None,
            bottom_panel: None,
            center: None,
            bg: Color::TRANSPARENT,
            panel_bg: Color::TRANSPARENT,
            tab_bar_bg: Color::TRANSPARENT,
            active_tab_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置左侧停靠面板。
    #[inline(always)]
    pub const fn left_panel(mut self, panel: DockPanel<'a>) -> Self {
        self.left_panel = Some(panel);
        self
    }

    /// 设置右侧停靠面板。
    #[inline(always)]
    pub const fn right_panel(mut self, panel: DockPanel<'a>) -> Self {
        self.right_panel = Some(panel);
        self
    }

    /// 设置底部停靠面板。
    #[inline(always)]
    pub const fn bottom_panel(mut self, panel: DockPanel<'a>) -> Self {
        self.bottom_panel = Some(panel);
        self
    }

    /// 设置中央主工作区内容。
    #[inline(always)]
    pub fn center(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.center = Some(child.into_element(cx));
        self
    }

    fn render_panel(
        &self,
        cx: &'a FrameScope,
        base_id: u32,
        w: i16,
        h: i16,
        panel: DockPanel<'a>,
        text_color: Color,
        muted_color: Color,
    ) -> Element<'a> {
        let mut tab_bar = div()
            .flex_row()
            .w(w)
            .h(26)
            .px(4)
            .gap(2)
            .bg(self.tab_bar_bg)
            .items_center();

        let tab_rounded = self.rounded.saturating_sub(2);
        for (idx, &tab) in panel.tabs.iter().enumerate() {
            let is_active = idx == panel.active_tab;
            let mut tab_btn = div()
                .id(crate::scoped_child_id(base_id, 0x40EB_5A9A, idx as u32 + 1))
                .h(22)
                .px(8)
                .rounded(tab_rounded)
                .bg(if is_active {
                    self.active_tab_bg
                } else {
                    Color::TRANSPARENT
                })
                .items_center()
                .focusable();
            if !self.focus_ring {
                tab_btn = tab_btn.no_focus_border();
            }
            tab_bar = tab_bar.child(
                cx,
                tab_btn.child(
                    cx,
                    text(tab).size(11).color(if is_active {
                        text_color
                    } else {
                        muted_color
                    }),
                ),
            );
        }

        let mut p = div()
            .id(base_id)
            .flex_col()
            .w(w)
            .h(h)
            .bg(self.panel_bg)
            .child(cx, tab_bar)
            .child(
                cx,
                div()
                    .w(w)
                    .h((h - 26).max(0))
                    .p(8)
                    .child(cx, text(panel.body_text).size(12).color(text_color)),
            );
        if self.border_color.a() > 0 {
            p = p.border(1, self.border_color);
        }
        p.into_element(cx)
    }
}

crate::impl_base_styled_box!(Dock<'a>);

impl<'a> Widget<'a> for Dock<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        self.width = StyleExt::resolve_width(self.style, self.width);
        self.height = StyleExt::resolve_height(self.style, self.height);
        let fg = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let muted_color = if self.muted_color.a() > 0 {
            self.muted_color
        } else {
            fg.scale_alpha(150)
        };

        let center_el = self.center.take();
        let bottom_h = if self.state.bottom_visible && self.bottom_panel.is_some() {
            self.state.bottom_height.clamp(40, (self.height - 60).max(40))
        } else {
            0
        };
        let top_h = (self.height - bottom_h).max(40);

        let left_w = if self.state.left_visible && self.left_panel.is_some() {
            self.state.left_width.clamp(60, (self.width / 2).max(60))
        } else {
            0
        };
        let right_w = if self.state.right_visible && self.right_panel.is_some() {
            self.state.right_width.clamp(60, (self.width / 2).max(60))
        } else {
            0
        };
        let center_w = (self.width - left_w - right_w).max(40);

        let mut top_row = div().flex_row().w(self.width).h(top_h);
        if left_w > 0 {
            if let Some(lp) = self.left_panel {
                top_row = top_row.child(
                    cx,
                    self.render_panel(cx, self.id ^ 0x100, left_w, top_h, lp, text_color, muted_color),
                );
            }
        }

        let mut center_box = div().w(center_w).h(top_h).bg(self.bg);
        if self.border_color.a() > 0 {
            center_box = center_box.border(1, self.border_color);
        }
        if let Some(c) = center_el {
            center_box = center_box.child(cx, c);
        }
        top_row = top_row.child(cx, center_box);

        if right_w > 0 {
            if let Some(rp) = self.right_panel {
                top_row = top_row.child(
                    cx,
                    self.render_panel(cx, self.id ^ 0x200, right_w, top_h, rp, text_color, muted_color),
                );
            }
        }

        let mut root = div()
            .id(self.id)
            .flex_col()
            .w(self.width)
            .h(self.height)
            .rounded(self.rounded)
            .child(cx, top_row);

        if bottom_h > 0 {
            if let Some(bp) = self.bottom_panel {
                root = root.child(
                    cx,
                    self.render_panel(
                        cx,
                        self.id ^ 0x300,
                        self.width,
                        bottom_h,
                        bp,
                        text_color,
                        muted_color,
                    ),
                );
            }
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
