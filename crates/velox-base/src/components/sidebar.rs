//! 无样式侧边导航栏行为原语 `Sidebar`（支持分组导航、徽标、折叠紧凑模式与选中回调；
//! 默认零背景、零边框、零圆角与零悬停渐变）。

use crate::components::icon::IconName;
use velox_core::prelude::*;

/// 侧边栏导航项描述（`SidebarItem`）。
#[derive(Clone, Copy, Debug)]
pub struct SidebarItem<'a> {
    pub id: u32,
    pub label: &'a str,
    pub icon: &'a str,
    pub icon_name: Option<IconName<'a>>,
    pub badge: Option<&'a str>,
}

impl<'a> SidebarItem<'a> {
    /// 创建一个侧边栏导航条目。
    #[inline(always)]
    pub const fn new(id: u32, label: &'a str, icon: &'a str) -> Self {
        Self {
            id,
            label,
            icon,
            icon_name: None,
            badge: None,
        }
    }

    /// 设置原生矢量 SVG 图标（渲染时将优先于纯文本 `icon` 显示）。
    #[inline(always)]
    pub const fn icon_name(mut self, name: IconName<'a>) -> Self {
        self.icon_name = Some(name);
        self
    }

    /// 设置条目右侧徽标文本（如未读数 `"3"`）。
    #[inline(always)]
    pub const fn badge(mut self, badge: &'a str) -> Self {
        self.badge = Some(badge);
        self
    }
}

/// 侧边栏导航分组描述（`SidebarSection`）。
#[derive(Clone, Copy, Debug)]
pub struct SidebarSection<'a> {
    pub title: &'a str,
    pub items: &'a [SidebarItem<'a>],
}

impl<'a> SidebarSection<'a> {
    /// 创建一个侧边栏导航分组。
    #[inline(always)]
    pub const fn new(title: &'a str, items: &'a [SidebarItem<'a>]) -> Self {
        Self { title, items }
    }
}

/// 面向 [`Sidebar::item`] 插槽回调的侧边栏导航项状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SidebarItemState {
    pub id: u32,
    pub section_index: usize,
    pub item_index: usize,
    pub is_active: bool,
    pub is_collapsed: bool,
}

impl SidebarItemState {
    #[inline(always)] pub const fn id(&self) -> u32 { self.id }
    #[inline(always)] pub const fn section_index(&self) -> usize { self.section_index }
    #[inline(always)] pub const fn item_index(&self) -> usize { self.item_index }
    #[inline(always)] pub const fn is_active(&self) -> bool { self.is_active }
    #[inline(always)] pub const fn is_collapsed(&self) -> bool { self.is_collapsed }
}

/// 无样式侧边导航栏原语（`Sidebar`）。
pub struct Sidebar<'a> {
    pub id: u32,
    pub header_title: &'a str,
    pub sections: &'a [SidebarSection<'a>],
    pub active_id: Option<u32>,
    pub collapsed: bool,
    pub width: i16,
    pub height: i16,
    pub bg: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub section_title_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub active_bg: Color,
    pub active_fg: Color,
    pub active_icon_color: Color,
    pub hover_bg: Option<Color>,
    pub press_bg: Option<Color>,
    pub badge_bg: Color,
    pub badge_fg: Color,
    pub collapse_btn_bg: Color,
    pub focus_ring: bool,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_select: Option<&'a dyn Fn(u32, &mut ActionScope)>,
    pub on_toggle_collapse: Option<&'a dyn Fn(&mut ActionScope)>,
    pub item_renderer:
        Option<&'a dyn Fn(Div<'a>, SidebarItemState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Sidebar<'a> {
    /// 创建一个无样式侧边导航栏原语。
    #[inline(always)]
    pub const fn new(id: u32, header_title: &'a str, sections: &'a [SidebarSection<'a>]) -> Self {
        Self {
            id,
            header_title,
            sections,
            active_id: None,
            collapsed: false,
            width: 210,
            height: 360,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            section_title_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            active_bg: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            active_icon_color: Color::TRANSPARENT,
            hover_bg: None,
            press_bg: None,
            badge_bg: Color::TRANSPARENT,
            badge_fg: Color::TRANSPARENT,
            collapse_btn_bg: Color::TRANSPARENT,
            focus_ring: false,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_select: None,
            on_toggle_collapse: None,
            item_renderer: None,
        }
    }

    /// 设置当前激活的导航项 ID。
    #[inline(always)]
    pub const fn active(mut self, active_id: Option<u32>) -> Self {
        self.active_id = active_id;
        self
    }

    /// 设置是否为折叠图标模式。
    #[inline(always)]
    pub const fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// 设置侧边栏宽高。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 绑定点击导航条目回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(u32, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// 绑定点击折叠/展开按钮回调。
    #[inline(always)]
    pub fn on_toggle_collapse(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_collapse = Some(handler);
        self
    }

    /// `gpui-base` 风格侧边导航项插槽回调：自定义每一项侧边栏条目外观。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SidebarItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: SidebarItemState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Sidebar<'a>);

impl<'a> Widget<'a> for Sidebar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let title_color = if self.title_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.title_color
        };
        let section_title_color = if self.section_title_color == Color::TRANSPARENT {
            def_fg.scale_alpha(140)
        } else {
            self.section_title_color
        };
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let muted_color = if self.muted_color == Color::TRANSPARENT {
            def_fg.scale_alpha(150)
        } else {
            self.muted_color
        };
        let active_bg = self.active_bg;
        let active_fg = if self.active_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.active_fg
        };
        let active_icon_col = if self.active_icon_color != Color::TRANSPARENT {
            self.active_icon_color
        } else {
            active_fg
        };
        let badge_bg = self.badge_bg;
        let badge_fg = if self.badge_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.badge_fg
        };
        let collapse_btn_bg = self.collapse_btn_bg;
        let rounded = self.rounded;

        let eff_w = if self.collapsed {
            52
        } else {
            StyleExt::resolve_width(self.style, self.width)
        };
        let eff_h = StyleExt::resolve_height(self.style, self.height);
        let btn_rounded = if rounded > 0 {
            rounded.saturating_sub(2).max(4)
        } else {
            0
        };
        let collapse_icon_d = if self.collapsed {
            "M9 18l6-6-6-6"
        } else {
            "M15 18l-6-6 6-6"
        };

        let mut collapse_btn = div()
            .id(self.id ^ 0x5342_0001)
            .w(24)
            .h(24)
            .rounded(btn_rounded)
            .bg(collapse_btn_bg)
            .items_center()
            .justify_center()
            .focusable()
            .child(
                cx,
                svg_icon(14, 14, collapse_icon_d)
                    .tint(muted_color)
                    .stroke_width(20),
            );
        if !self.focus_ring {
            collapse_btn = collapse_btn.no_focus_border();
        }
        if let Some(h) = self.hover_bg {
            collapse_btn = collapse_btn.hover_bg(h);
        }
        if let Some(a) = self.press_bg {
            collapse_btn = collapse_btn.active_bg(a);
        }
        if let Some(cb) = self.on_toggle_collapse {
            collapse_btn = collapse_btn.on_click_raw(cb);
        }

        let mut header = div()
            .flex_row()
            .w(eff_w - 16)
            .h(32)
            .items_center()
            .justify_between();
        if !self.collapsed {
            header = header.child(
                cx,
                text(self.header_title)
                    .size(13)
                    .color(title_color)
                    .ellipsis(),
            );
        }
        header = header.child(cx, collapse_btn);

        let mut bar = div()
            .id(self.id)
            .flex_col()
            .w(eff_w)
            .p(8)
            .gap(6)
            .bg(bg)
            .child(cx, header);
        if border_color != Color::TRANSPARENT {
            bar = bar.border(1, border_color);
        }
        bar = if eff_h <= 0 {
            bar.h_full()
        } else {
            bar.h(eff_h)
        };

        let on_sel = self.on_select;
        for (sec_idx, sec) in self.sections.iter().enumerate() {
            let mut sec_col = div().flex_col().w(eff_w - 16).gap(2);
            if !self.collapsed && !sec.title.is_empty() {
                let mut title_box = div().px(8).py(2);
                if sec_idx > 0 {
                    title_box = title_box.mt(4);
                }
                sec_col = sec_col.child(
                    cx,
                    title_box.child(cx, text(sec.title).size(10).color(section_title_color)),
                );
            }
            for (item_idx, item) in sec.items.iter().enumerate() {
                let is_active = self.active_id == Some(item.id);
                let icon_col = if is_active {
                    active_icon_col
                } else {
                    muted_color
                };
                let mut left_group = div().flex_row().gap(8).items_center().overflow_hidden();
                if let Some(svg_d) = item.icon_name.and_then(|n| n.svg_path()) {
                    left_group = left_group.child(
                        cx,
                        svg_icon(14, 14, svg_d).tint(icon_col).stroke_width(18),
                    );
                } else {
                    left_group = left_group.child(cx, text(item.icon).size(12).color(icon_col));
                }
                if !self.collapsed {
                    left_group = left_group.child(
                        cx,
                        text(item.label)
                            .size(12)
                            .color(if is_active { active_fg } else { text_color })
                            .ellipsis(),
                    );
                }

                if let Some(renderer) = self.item_renderer {
                    let mut row = div()
                        .id(crate::scoped_child_id(self.id, 0xC87A_CFC7, item.id))
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .focusable()
                        .no_focus_border()
                        .child(cx, left_group);
                    if let Some(cb) = on_sel {
                        let id = item.id;
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(id, scope);
                        });
                        row = row.on_click_raw(click_cb);
                    }
                    let state = SidebarItemState {
                        id: item.id,
                        section_index: sec_idx,
                        item_index: item_idx,
                        is_active,
                        is_collapsed: self.collapsed,
                    };
                    sec_col = sec_col.child(cx, renderer(row, state, cx));
                    continue;
                }

                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0xC87A_CFC7, item.id))
                    .flex_row()
                    .w(eff_w - 16)
                    .h(28)
                    .px(8)
                    .rounded(rounded)
                    .bg(if is_active {
                        active_bg
                    } else {
                        Color::TRANSPARENT
                    })
                    .items_center()
                    .justify_between()
                    .focusable()
                    .child(cx, left_group);
                if !self.focus_ring {
                    row = row.no_focus_border();
                }
                if let Some(h) = self.hover_bg {
                    row = row.hover_bg(if is_active && active_bg != Color::TRANSPARENT {
                        active_bg
                    } else {
                        h
                    });
                }
                if let Some(a) = self.press_bg {
                    row = row.active_bg(a);
                }

                if !self.collapsed {
                    if let Some(b) = item.badge {
                        row = row.child(
                            cx,
                            div()
                                .px(6)
                                .py(1)
                                .rounded(if rounded > 0 { 8 } else { 0 })
                                .bg(badge_bg)
                                .child(cx, text(b).size(10).color(badge_fg)),
                        );
                    }
                }

                if let Some(cb) = on_sel {
                    let id = item.id;
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(id, scope);
                    });
                    row = row.on_click_raw(click_cb);
                }
                sec_col = sec_col.child(cx, row);
            }
            bar = bar.child(cx, sec_col);
        }

        bar = self.style_ext.apply_to_div(bar, self.style);
        bar.into_element(cx)
    }
}
