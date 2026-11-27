//! 无样式基础下拉选择菜单原语 `Dropdown`（定义展开态、选项选中行为、`.overlay()` 顶层弹出与命中遮挡；
//! 默认零背景、零边框、零圆角、零阴影与零动画）。

use velox_engine::prelude::*;

/// 传递给 `Dropdown::item(...)` 插槽回调的下拉选项状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropdownItemState<'a> {
    pub index: usize,
    pub label: &'a str,
    pub selected: bool,
}

impl<'a> DropdownItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn label(&self) -> &'a str { self.label }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.selected }
}

type DropdownItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, DropdownItemState<'a>, &'a FrameScope) -> Element<'a>;
type DropdownTriggerRenderer<'a> = &'a dyn Fn(Div<'a>, bool, &'a FrameScope) -> Element<'a>;
type DropdownPopupRenderer<'a> = &'a dyn Fn(Div<'a>) -> Div<'a>;

/// 无样式基础下拉选择框原语（`Dropdown`）。
pub struct Dropdown<'a> {
    pub id: u32,
    pub options: &'a [&'a str],
    pub selected: usize,
    pub expanded: bool,
    pub width: i16,
    pub height: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub header_bg: Color,
    pub border_color: Color,
    pub active_border_color: Color,
    pub popup_bg: Color,
    pub popup_border_color: Color,
    pub selected_bg: Color,
    pub text_color: Color,
    pub selected_text_color: Color,
    pub check_color: Color,
    pub arrow_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub shadow_alpha: u8,
    /// 是否启用弹出列表入场动画（无样式原语默认关闭）。
    pub animated: bool,
    pub on_toggle: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<DropdownItemRenderer<'a>>,
    trigger_renderer: Option<DropdownTriggerRenderer<'a>>,
    popup_renderer: Option<DropdownPopupRenderer<'a>>,
}

impl<'a> Dropdown<'a> {
    /// 创建一个无样式下拉选择原语。
    #[inline(always)]
    pub const fn new(
        id: u32,
        options: &'a [&'a str],
        selected: usize,
        expanded: bool,
    ) -> Self {
        Self {
            id,
            options,
            selected,
            expanded,
            width: 140,
            height: 32,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            header_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            popup_bg: Color::TRANSPARENT,
            popup_border_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            selected_text_color: Color::TRANSPARENT,
            check_color: Color::TRANSPARENT,
            arrow_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            focus_ring: false,
            rounded: 0,
            shadow_alpha: 0,
            animated: false,
            on_toggle: None,
            on_select: None,
            item_renderer: None,
            trigger_renderer: None,
            popup_renderer: None,
        }
    }

    /// 设置选择框宽度与收起态高度。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 启用或禁用弹出列表入场动画（下滑淡入）。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定点击头部展开/收起切换回调。
    #[inline(always)]
    pub fn on_toggle(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle = Some(handler);
        self
    }

    /// 绑定点击下拉列表项的选中回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格下拉选项插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, DropdownItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: DropdownItemState<'a>, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格触发器插槽回调。
    #[inline]
    pub fn trigger<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, bool) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, open: bool, frame_cx: &'a FrameScope| f(d, open).into_element(frame_cx),
        );
        self.trigger_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格弹出列表容器定制回调。
    #[inline]
    pub fn popup<F>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>) -> Div<'a> + 'a,
    {
        self.popup_renderer = Some(cx.arena().alloc(f));
        self
    }
}

crate::impl_base_styled_box!(Dropdown<'a>);

impl<'a> Widget<'a> for Dropdown<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let selected_text_color = if self.selected_text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.selected_text_color
        };
        let check_col = if self.check_color == Color::TRANSPARENT {
            selected_text_color
        } else {
            self.check_color
        };
        let arrow_color = if self.arrow_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.arrow_color
        };

        let current_label = if !self.options.is_empty() {
            self.options[self.selected.min(self.options.len() - 1)]
        } else {
            "--"
        };

        let border_col = if self.expanded && self.active_border_color != Color::TRANSPARENT {
            self.active_border_color
        } else {
            self.border_color
        };

        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let eff_h = StyleExt::resolve_height(self.style, self.height);
        let mut header = div()
            .id(self.id)
            .flex_row()
            .w(eff_w)
            .h(eff_h)
            .px(10)
            .gap(8)
            .items_center()
            .justify_between()
            .rounded(self.rounded)
            .bg(self.header_bg)
            .focusable();
        if border_col != Color::TRANSPARENT {
            header = header.border(1, border_col);
        }
        if !self.focus_ring {
            header = header.no_focus_border();
        }
        if let Some(h) = self.hover_bg {
            header = header.hover_bg(h);
        }
        if let Some(a) = self.active_bg {
            header = header.active_bg(a);
        }
        if let Some(toggle_cb) = self.on_toggle {
            header = header.on_click_raw(toggle_cb);
        }

        let arrow_d = if self.expanded {
            "M18 15l-6-6-6 6"
        } else {
            "M6 9l6 6 6-6"
        };
        header = header
            .child(cx, text(current_label).size(13).color(text_color).ellipsis())
            .child(
                cx,
                svg_icon(14, 14, arrow_d)
                    .tint(arrow_color)
                    .stroke_width(20),
            );

        let header_el = match self.trigger_renderer {
            Some(f) => f(header, self.expanded, cx),
            None => header.into_element(cx),
        };
        let mut root = div().flex_col().w(eff_w).child(cx, header_el);

        if !self.expanded || self.options.is_empty() {
            return self.style_ext.apply_to_div(root, self.style).into_element(cx);
        }

        let item_h = 28i16;
        let menu_h = (self.options.len() as i16)
            .saturating_mul(item_h)
            .saturating_add(8);
        let mut popup = div()
            .id(crate::scoped_child_id(self.id, 0x929D_72D8, 100))
            .pos(0, eff_h.saturating_add(4))
            .w(eff_w)
            .h(menu_h)
            .p(4)
            .flex_col()
            .rounded(self.rounded)
            .bg(self.popup_bg)
            .overlay()
            .occlude_hit();

        if self.popup_border_color != Color::TRANSPARENT {
            popup = popup.border(1, self.popup_border_color);
        }
        if let Some(pop_fn) = self.popup_renderer {
            popup = pop_fn(popup);
        }
        if let Some(toggle_cb) = self.on_toggle {
            popup = popup.on_navigate_back(toggle_cb);
        }
        if self.on_select.is_some() || self.on_toggle.is_some() {
            let len = self.options.len();
            let cur = self.selected.min(len.saturating_sub(1));
            let on_sel = self.on_select;
            let on_tog = self.on_toggle;
            let nav = cx.arena().alloc(move |key: KeyCode, acx: &mut ActionScope| {
                if len == 0 {
                    return;
                }
                match key {
                    KeyCode::Down => {
                        if let Some(cb) = on_sel {
                            cb((cur + 1) % len, acx);
                        }
                    }
                    KeyCode::Up => {
                        if let Some(cb) = on_sel {
                            cb((cur + len - 1) % len, acx);
                        }
                    }
                    KeyCode::Home => {
                        if let Some(cb) = on_sel {
                            cb(0, acx);
                        }
                    }
                    KeyCode::End => {
                        if let Some(cb) = on_sel {
                            cb(len - 1, acx);
                        }
                    }
                    KeyCode::Enter | KeyCode::Space => {
                        if let Some(cb) = on_tog {
                            cb(acx);
                        }
                    }
                    _ => {}
                }
            });
            popup = popup.on_key_nav(nav);
        }

        if self.shadow_alpha > 0 {
            popup = popup.shadow(12, 0, 4, Color::rgba(0, 0, 0, self.shadow_alpha.min(100)));
        }
        if self.animated {
            popup = popup
                .enter_from(0, 0, -6, core::time::Duration::from_millis(140))
                .exit_to(0, 0, -6, core::time::Duration::from_millis(130));
        }

        for (idx, &opt) in self.options.iter().enumerate() {
            let is_sel = idx == self.selected;
            if let Some(item_fn) = self.item_renderer {
                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x929D_72D8, 101 + idx as u32))
                    .child(cx, text(opt).ellipsis());
                if let Some(sel_cb) = self.on_select {
                    let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                        sel_cb(idx, acx);
                    });
                    row = row.on_click_raw(raw_cb);
                }
                popup = popup.child(
                    cx,
                    item_fn(
                        row,
                        DropdownItemState {
                            index: idx,
                            label: opt,
                            selected: is_sel,
                        },
                        cx,
                    ),
                );
                continue;
            }
            let row_bg = if is_sel {
                self.selected_bg
            } else {
                Color::TRANSPARENT
            };
            let row_fg = if is_sel {
                selected_text_color
            } else {
                text_color
            };

            let mut row = div()
                .id(crate::scoped_child_id(self.id, 0x929D_72D8, 101 + idx as u32))
                .flex_row()
                .w_full()
                .h(item_h)
                .px(8)
                .gap(8)
                .items_center()
                .justify_between()
                .rounded(self.rounded.saturating_sub(2))
                .bg(row_bg);
            if let Some(h) = self.hover_bg {
                row = row.hover_bg(if is_sel && self.selected_bg != Color::TRANSPARENT {
                    self.selected_bg
                } else {
                    h
                });
            }
            if let Some(a) = self.active_bg {
                row = row.active_bg(a);
            }

            if let Some(sel_cb) = self.on_select {
                let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    sel_cb(idx, acx);
                });
                row = row.on_click_raw(raw_cb);
            }

            row = row.child(cx, text(opt).size(13).color(row_fg).ellipsis());
            if is_sel {
                row = row.child(
                    cx,
                    svg_icon(13, 13, "M20 6 9 17l-5-5")
                        .tint(check_col)
                        .stroke_width(24),
                );
            }

            popup = popup.child(cx, row);
        }

        root = root.child(cx, popup);
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
