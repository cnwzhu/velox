//! 菜单基础组件 `Menu`、菜单项 `MenuItem` 与右键上下文菜单 `ContextMenu`（默认无样式）。

use velox_engine::prelude::*;

/// 菜单条目定义（`MenuItem`）。
#[derive(Clone, Copy, Debug)]
pub struct MenuItem<'a> {
    pub label: &'a str,
    pub shortcut: &'a str,
    pub disabled: bool,
    pub destructive: bool,
    pub is_separator: bool,
}

impl<'a> MenuItem<'a> {
    /// 创建一个标准菜单项。
    #[inline(always)]
    pub const fn new(label: &'a str, shortcut: &'a str) -> Self {
        Self {
            label,
            shortcut,
            disabled: false,
            destructive: false,
            is_separator: false,
        }
    }

    /// 创建一个菜单水平分割线。
    #[inline(always)]
    pub const fn separator() -> Self {
        Self {
            label: "",
            shortcut: "",
            disabled: true,
            destructive: false,
            is_separator: true,
        }
    }

    /// 设置为危险操作标记。
    #[inline(always)]
    pub const fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// 设置是否禁用。
    #[inline(always)]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 面向 [`Menu::item`] / [`ContextMenu::item`] 插槽回调的菜单项状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemState {
    pub index: usize,
    pub is_separator: bool,
    pub is_disabled: bool,
    pub is_destructive: bool,
}

impl MenuItemState {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn is_separator(&self) -> bool { self.is_separator }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.is_disabled }
    #[inline(always)] pub const fn is_destructive(&self) -> bool { self.is_destructive }
}

/// 菜单与下拉菜单基础组件（`Menu`，默认无样式）。
pub struct Menu<'a> {
    pub id: u32,
    pub items: &'a [MenuItem<'a>],
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub separator_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub disabled_color: Color,
    pub destructive_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub rounded: u8,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    /// 点击菜单项时是否接管键盘焦点（独立菜单栏默认 `true`，右键 `ContextMenu` 默认 `false` 以保留底层输入框焦点）。
    pub steal_focus: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub item_renderer: Option<&'a dyn Fn(Div<'a>, MenuItemState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Menu<'a> {
    /// 创建一个默认无样式的菜单组件。
    #[inline(always)]
    pub const fn new(id: u32, items: &'a [MenuItem<'a>]) -> Self {
        Self {
            id,
            items,
            width: 190,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            separator_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            disabled_color: Color::TRANSPARENT,
            destructive_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            rounded: 0,
            shadow: None,
            focus_ring: false,
            steal_focus: true,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_select: None,
            item_renderer: None,
        }
    }

    /// 设置菜单宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定点击某个可用菜单项的回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格菜单项插槽回调：自定义每一项菜单条目（或分隔线）外观。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, MenuItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: MenuItemState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Menu<'a>);

impl<'a> Widget<'a> for Menu<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg_base = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg_base
        };
        let muted_color = if self.muted_color.a() > 0 {
            self.muted_color
        } else {
            fg_base.scale_alpha(150)
        };
        let disabled_color = if self.disabled_color.a() > 0 {
            self.disabled_color
        } else {
            fg_base.scale_alpha(150)
        };
        let destructive_color = if self.destructive_color.a() > 0 {
            self.destructive_color
        } else {
            fg_base
        };
        let sep_color = if self.separator_color.a() > 0 {
            self.separator_color
        } else if self.border_color.a() > 0 {
            self.border_color
        } else {
            fg_base.scale_alpha(48)
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut box_col = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .p(4)
            .gap(2)
            .rounded(self.rounded)
            .bg(self.bg);
        if self.border_color.a() > 0 {
            box_col = box_col.border(1, self.border_color);
        }
        if let Some((blur, ox, oy, col)) = self.shadow {
            box_col = box_col.shadow(blur, ox, oy, col);
        }

        let item_rounded = self.rounded.saturating_sub(2);
        let on_sel = self.on_select;
        for (idx, item) in self.items.iter().enumerate() {
            if let Some(renderer) = self.item_renderer {
                let state = MenuItemState {
                    index: idx,
                    is_separator: item.is_separator,
                    is_disabled: item.disabled,
                    is_destructive: item.destructive,
                };
                if item.is_separator {
                    box_col = box_col.child(cx, renderer(div().w_full().h(1), state, cx));
                    continue;
                }
                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x99E4_DD3A, idx as u32 + 1))
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(cx, text(item.label).nowrap());
                if !item.shortcut.is_empty() {
                    row = row.child(cx, text(item.shortcut).nowrap());
                }
                if !item.disabled {
                    if let Some(cb) = on_sel {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(idx, scope);
                        });
                        row = row.on_click_raw(click_cb);
                    }
                    if self.steal_focus {
                        row = row.focusable().no_focus_border();
                    } else {
                        row = row.unfocusable();
                    }
                }
                box_col = box_col.child(cx, renderer(row, state, cx));
                continue;
            }

            if item.is_separator {
                box_col = box_col.child(cx, div().w(width - 8).h(1).bg(sep_color));
                continue;
            }

            let fg = if item.disabled {
                disabled_color
            } else if item.destructive {
                destructive_color
            } else {
                text_color
            };

            let mut row = div()
                .id(crate::scoped_child_id(self.id, 0x99E4_DD3A, idx as u32 + 1))
                .flex_row()
                .w(width - 8)
                .h(26)
                .px(8)
                .rounded(item_rounded)
                .items_center()
                .justify_between()
                .child(cx, text(item.label).size(12).color(fg).nowrap());

            if !item.shortcut.is_empty() {
                row = row.child(
                    cx,
                    text(item.shortcut).size(11).color(muted_color).nowrap(),
                );
            }

            if !item.disabled {
                if let Some(h) = self.hover_bg {
                    row = row.hover_bg(h);
                }
                if let Some(a) = self.active_bg {
                    row = row.active_bg(a);
                }
                row = row.cursor(CursorShape::Pointer);
                if let Some(cb) = on_sel {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    row = row.on_click_raw(click_cb);
                }
                if self.steal_focus {
                    row = row.focusable();
                    if !self.focus_ring {
                        row = row.no_focus_border();
                    }
                } else {
                    row = row.unfocusable();
                }
            } else {
                row = row.opacity(125).cursor(CursorShape::NotAllowed);
            }
            box_col = box_col.child(cx, row);
        }

        box_col = self.style_ext.apply_to_div(box_col, self.style);
        box_col.into_element(cx)
    }
}

/// 顶层定位右键上下文菜单组件（`ContextMenu`）。
pub struct ContextMenu<'a> {
    pub id: u32,
    pub x: i16,
    pub y: i16,
    pub open: bool,
    pub animated: bool,
    pub menu: Menu<'a>,
    pub on_dismiss: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> ContextMenu<'a> {
    /// 创建一个位于坐标 `(x, y)` 的上下文菜单（默认不抢夺底层输入框键盘焦点）。
    #[inline(always)]
    pub const fn new(id: u32, x: i16, y: i16, open: bool, items: &'a [MenuItem<'a>]) -> Self {
        let mut menu = Menu::new(id, items);
        menu.steal_focus = false;
        Self {
            id,
            x,
            y,
            open,
            animated: false,
            menu,
            on_dismiss: None,
        }
    }

    /// 启用或禁用上下文菜单开关过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 设置上下文菜单宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.menu.width = width;
        self
    }

    /// 绑定点击某个可用菜单项回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.menu.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格菜单项插槽回调：转发至内部 [`Menu::item`]。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, MenuItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        self.menu = self.menu.item(cx, f);
        self
    }

    /// 绑定点击菜单外部空白处、右击外部或按 `Escape` 键关闭菜单回调。
    #[inline(always)]
    pub fn on_dismiss(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_dismiss = Some(handler);
        self
    }
}

impl<'a> Styled for ContextMenu<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.menu.style_mut()
    }
}

impl<'a> StyledExt for ContextMenu<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.menu.style_ext_mut()
    }
}

impl<'a> Widget<'a> for ContextMenu<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        if !self.open {
            return div().id(self.id).w(0).h(0).into_element(cx);
        }
        if let Some(dismiss) = self.on_dismiss {
            let mut menu_box = div()
                .id(crate::scoped_child_id(self.id, 0xC7E0_BD01, 2))
                .pos(self.x.max(4), self.y.max(4))
                .occlude_hit();
            if self.animated {
                menu_box = menu_box
                    .slide_in(0, -6, Duration::from_millis(130))
                    .slide_out(0, -6, Duration::from_millis(110));
            }
            menu_box = menu_box.child(cx, self.menu);
            div()
                .id(crate::scoped_child_id(self.id, 0xC7E0_BD01, 1))
                .pos(0, 0)
                .size_full()
                .overlay()
                .occlude_hit()
                .cursor(CursorShape::Default)
                .on_click_raw(dismiss)
                .unfocusable()
                .on_secondary_click_raw(dismiss)
                .on_navigate_back(dismiss)
                .child(cx, menu_box)
                .into_element(cx)
        } else {
            let mut menu_box = div()
                .id(crate::scoped_child_id(self.id, 0xC7E0_BD01, 2))
                .pos(self.x.max(4), self.y.max(4))
                .overlay()
                .occlude_hit();
            if self.animated {
                menu_box = menu_box
                    .slide_in(0, -6, Duration::from_millis(130))
                    .slide_out(0, -6, Duration::from_millis(110));
            }
            menu_box.child(cx, self.menu).into_element(cx)
        }
    }
}
