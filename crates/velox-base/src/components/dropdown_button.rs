//! 无样式分割下拉按钮原语 `DropdownButton`（默认零背景、零边框、零圆角、零阴影）。

use velox_core::prelude::*;

type DropdownButtonItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, usize, &'a str, &'a FrameScope) -> Element<'a>;

/// 无样式带下拉菜单的分割按钮原语（`DropdownButton`）。
pub struct DropdownButton<'a> {
    pub id: u32,
    pub label: &'a str,
    pub options: &'a [&'a str],
    pub open: bool,
    pub width: i16,
    pub height: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub arrow_bg: Color,
    pub divider_color: Color,
    pub text_color: Color,
    pub menu_bg: Color,
    pub menu_border_color: Color,
    pub menu_text_color: Color,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub on_click: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_toggle_menu: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<DropdownButtonItemRenderer<'a>>,
}

impl<'a> DropdownButton<'a> {
    /// 创建一个无样式带下拉菜单的分割按钮原语。
    #[inline(always)]
    pub const fn new(
        id: u32,
        label: &'a str,
        options: &'a [&'a str],
        open: bool,
    ) -> Self {
        Self {
            id,
            label,
            options,
            open,
            width: 148,
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
            bg: Color::TRANSPARENT,
            arrow_bg: Color::TRANSPARENT,
            divider_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            menu_bg: Color::TRANSPARENT,
            menu_border_color: Color::TRANSPARENT,
            menu_text_color: Color::TRANSPARENT,
            shadow: None,
            focus_ring: false,
            rounded: 0,
            on_click: None,
            on_toggle_menu: None,
            on_select: None,
            item_renderer: None,
        }
    }

    /// 绑定点击主按钮区域的回调。
    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self
    }

    /// 绑定点击右侧下拉箭头展开/收起菜单的回调。
    #[inline(always)]
    pub fn on_toggle_menu(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_menu = Some(handler);
        self
    }

    /// 绑定点击下拉菜单某一项的回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格下拉菜单项插槽定制回调。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, usize, &'a str) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, idx: usize, lbl: &'a str, frame_cx: &'a FrameScope| {
                f(d, idx, lbl).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(DropdownButton<'a>);

impl<'a> Widget<'a> for DropdownButton<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let menu_text_color = if self.menu_text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.menu_text_color
        };

        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let eff_h = StyleExt::resolve_height(self.style, self.height);
        let arrow_w = 28i16;
        let main_w = (eff_w - arrow_w - 1).max(40);

        let mut main_btn = div()
            .id(self.id)
            .w(main_w)
            .h(eff_h)
            .px(10)
            .bg(self.bg)
            .items_center()
            .justify_center()
            .focusable()
            .child(cx, text(self.label).size(12).color(text_color));
        if !self.focus_ring {
            main_btn = main_btn.no_focus_border();
        }
        if let Some(cb) = self.on_click {
            main_btn = main_btn.on_click_raw(cb);
        }

        let mut arrow_btn = div()
            .id(crate::scoped_child_id(self.id, 0x02F8_D631, 1))
            .w(arrow_w)
            .h(eff_h)
            .bg(self.arrow_bg)
            .items_center()
            .justify_center()
            .focusable()
            .child(
                cx,
                text(if self.open { "^" } else { "v" })
                    .size(11)
                    .color(text_color),
            );
        if !self.focus_ring {
            arrow_btn = arrow_btn.no_focus_border();
        }
        if let Some(cb) = self.on_toggle_menu {
            arrow_btn = arrow_btn.on_click_raw(cb);
        }

        let mut split_bar = div()
            .flex_row()
            .w(eff_w)
            .h(eff_h)
            .rounded(self.rounded)
            .overflow_hidden()
            .child(cx, main_btn)
            .child(cx, div().w(1).h(eff_h).bg(self.divider_color))
            .child(cx, arrow_btn);
        if self.bg == Color::TRANSPARENT && self.divider_color != Color::TRANSPARENT {
            split_bar = split_bar.border(1, self.divider_color);
        }

        let mut root = div().w(eff_w).h(eff_h).child(cx, split_bar);

        if self.open && !self.options.is_empty() {
            let count = self.options.len() as i16;
            let menu_h = count
                .saturating_mul(26)
                .saturating_add((count - 1).max(0).saturating_mul(2))
                .saturating_add(8);

            let mut menu = div()
                .id(crate::scoped_child_id(self.id, 0x02F8_D631, 100))
                .pos(0, eff_h.saturating_add(4))
                .flex_col()
                .w(eff_w)
                .h(menu_h)
                .p(4)
                .gap(2)
                .rounded(self.rounded)
                .bg(self.menu_bg)
                .overlay()
                .occlude_hit();
            if self.menu_border_color != Color::TRANSPARENT {
                menu = menu.border(1, self.menu_border_color);
            }
            if let Some((blur, ox, oy, col)) = self.shadow {
                menu = menu.shadow(blur, ox, oy, col);
            }

            let on_sel = self.on_select;
            let item_r = self.rounded.saturating_sub(2);
            for (idx, &opt) in self.options.iter().enumerate() {
                if let Some(item_fn) = self.item_renderer {
                    let mut slot = div()
                        .id(crate::scoped_child_id(self.id, 0x02F8_D631, 101 + idx as u32))
                        .focusable()
                        .child(cx, text(opt).ellipsis());
                    if !self.focus_ring {
                        slot = slot.no_focus_border();
                    }
                    if let Some(cb) = on_sel {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(idx, scope);
                        });
                        slot = slot.on_click_raw(click_cb);
                    }
                    menu = menu.child(cx, item_fn(slot, idx, opt, cx));
                    continue;
                }
                let mut item = div()
                    .id(crate::scoped_child_id(self.id, 0x02F8_D631, 101 + idx as u32))
                    .w(eff_w - 8)
                    .h(26)
                    .px(8)
                    .rounded(item_r)
                    .items_center()
                    .focusable()
                    .child(
                        cx,
                        text(opt)
                            .size(12)
                            .color(menu_text_color)
                            .ellipsis(),
                    );
                if !self.focus_ring {
                    item = item.no_focus_border();
                }
                if let Some(cb) = on_sel {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    item = item.on_click_raw(click_cb);
                }
                menu = menu.child(cx, item);
            }
            root = root.child(cx, menu);
        }

        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
