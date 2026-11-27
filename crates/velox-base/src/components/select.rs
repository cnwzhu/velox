//! 无样式下拉选择器原语 `Select` 与候选条目 `SelectOption`（默认零背景、零边框、零圆角、零阴影与零悬停渐变）。

use velox_engine::prelude::*;

const AUTO_COLOR: Color = Color(0x0000_0001);

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

/// 选择器候选条目定义（`SelectOption`）。
#[derive(Clone, Copy, Debug)]
pub struct SelectOption<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub disabled: bool,
}

impl<'a> SelectOption<'a> {
    /// 创建一个可用候选选项。
    #[inline(always)]
    pub const fn new(label: &'a str, value: &'a str) -> Self {
        Self {
            label,
            value,
            disabled: false,
        }
    }

    /// 设置该选项是否禁用。
    #[inline(always)]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 传递给 `Select::item(...)` 插槽渲染回调的候选条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectItemState<'a> {
    pub index: usize,
    pub label: &'a str,
    pub value: &'a str,
    pub selected: bool,
    pub disabled: bool,
}

impl<'a> SelectItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn label(&self) -> &'a str { self.label }
    #[inline(always)] pub const fn value(&self) -> &'a str { self.value }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.selected }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type SelectItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, SelectItemState<'a>, &'a FrameScope) -> Element<'a>;
type SelectTriggerRenderer<'a> = &'a dyn Fn(Div<'a>, bool, &'a FrameScope) -> Element<'a>;
type SelectPopupRenderer<'a> = &'a dyn Fn(Div<'a>) -> Div<'a>;
type SelectLabelFormatter<'a> = &'a dyn Fn(SelectOption<'a>) -> StrRef<'a>;

/// 无样式表单下拉选择器原语（`Select`，支持占位符、禁用选项、勾选标记、浮动菜单、方向键跳过禁用项、Escape 关闭与 `gpui-base` 插槽回调）。
pub struct Select<'a> {
    pub id: u32,
    pub options: &'a [SelectOption<'a>],
    pub selected_index: Option<usize>,
    pub open: bool,
    pub placeholder: &'a str,
    pub width: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub border_color: Color,
    pub active_border_color: Color,
    pub popup_bg: Color,
    pub popup_border_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub check_color: Color,
    pub text_color: Color,
    pub placeholder_color: Color,
    pub disabled_color: Color,
    pub arrow_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub shadow: Option<(u8, i8, i8, Color)>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub animated: bool,
    pub on_toggle: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<SelectItemRenderer<'a>>,
    trigger_renderer: Option<SelectTriggerRenderer<'a>>,
    popup_renderer: Option<SelectPopupRenderer<'a>>,
    label_formatter: Option<SelectLabelFormatter<'a>>,
}

impl<'a> Select<'a> {
    /// 创建一个无样式下拉选择器原语。
    #[inline(always)]
    pub const fn new(
        id: u32,
        options: &'a [SelectOption<'a>],
        selected_index: Option<usize>,
        open: bool,
    ) -> Self {
        Self {
            id,
            options,
            selected_index,
            open,
            placeholder: "Select an option...",
            width: 180,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 1,
            },
            style_ext: StyleExt::DEFAULT,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            popup_bg: Color::TRANSPARENT,
            popup_border_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: AUTO_COLOR,
            check_color: AUTO_COLOR,
            text_color: AUTO_COLOR,
            placeholder_color: AUTO_COLOR,
            disabled_color: AUTO_COLOR,
            arrow_color: AUTO_COLOR,
            hover_bg: None,
            active_bg: None,
            shadow: None,
            focus_ring: false,
            rounded: 0,
            animated: false,
            on_toggle: None,
            on_select: None,
            item_renderer: None,
            trigger_renderer: None,
            popup_renderer: None,
            label_formatter: None,
        }
    }

    /// 设置未选中时的占位提示文案。
    #[inline(always)]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// 设置宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 启用或禁用下拉菜单开关过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定展开/收起回调。
    #[inline(always)]
    pub fn on_toggle(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle = Some(handler);
        self
    }

    /// 绑定选中某个可用选项的回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格下拉选项插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SelectItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |item: Div<'a>, state: SelectItemState<'a>, frame_cx: &'a FrameScope| {
                f(item, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格触发按钮插槽回调 `(trigger_div, is_open)`。
    #[inline]
    pub fn trigger<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, bool) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |trigger: Div<'a>, open: bool, frame_cx: &'a FrameScope| {
                f(trigger, open).into_element(frame_cx)
            },
        );
        self.trigger_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格弹层菜单容器修饰回调。
    #[inline]
    pub fn popup<F>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>) -> Div<'a> + 'a,
    {
        self.popup_renderer = Some(cx.arena().alloc(f));
        self
    }

    /// 自定义选项文本格式化回调。
    #[inline]
    pub fn label<F, S>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(SelectOption<'a>) -> S + 'a,
        S: Into<StrRef<'a>>,
    {
        let cb = cx.arena().alloc(move |opt: SelectOption<'a>| f(opt).into());
        self.label_formatter = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Select<'a>);

impl<'a> Widget<'a> for Select<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let active_border_color = if self.active_border_color != Color::TRANSPARENT {
            self.active_border_color
        } else {
            border_color
        };
        let popup_bg = self.popup_bg;
        let popup_border_color = self.popup_border_color;
        let selected_bg = self.selected_bg;
        let selected_fg = resolve_color(self.selected_fg, def_fg);
        let check_col = resolve_color(self.check_color, selected_fg);
        let text_color = resolve_color(self.text_color, def_fg);
        let placeholder_color = resolve_color(self.placeholder_color, def_fg.scale_alpha(140));
        let disabled_color = resolve_color(self.disabled_color, def_fg.scale_alpha(120));
        let arrow_color = resolve_color(self.arrow_color, def_fg);

        let eff_width = match self.style.width {
            Length::Px(w) => w,
            _ => self.width,
        };
        let eff_bg = if self.style.bg != Color::TRANSPARENT {
            self.style.bg
        } else {
            bg
        };
        let eff_border = if self.style.border_color != Color::TRANSPARENT {
            self.style.border_color
        } else {
            border_color
        };
        let eff_rounded = if self.style.rounded_radius() > 0 {
            self.style.rounded_radius()
        } else {
            self.rounded
        };
        let (display_txt, is_placeholder) = match self.selected_index.and_then(|i| self.options.get(i))
        {
            Some(&opt) => match self.label_formatter {
                Some(f) => (f(opt), false),
                None => (StrRef::from(opt.label), false),
            },
            None => (StrRef::from(self.placeholder), true),
        };

        let arrow_d = if self.open {
            "M18 15l-6-6-6 6"
        } else {
            "M6 9l6 6 6-6"
        };

        let trig_border = if self.open && active_border_color != Color::TRANSPARENT {
            active_border_color
        } else {
            eff_border
        };

        let mut trigger = div()
            .id(self.id)
            .flex_row()
            .w(eff_width)
            .h(32)
            .px(10)
            .gap(8)
            .rounded(eff_rounded)
            .bg(eff_bg)
            .items_center()
            .justify_between()
            .focusable()
            .child(
                cx,
                text(display_txt)
                    .size(13)
                    .color(if is_placeholder {
                        placeholder_color
                    } else {
                        text_color
                    })
                    .ellipsis(),
            )
            .child(
                cx,
                svg_icon(14, 14, arrow_d)
                    .tint(arrow_color)
                    .stroke_width(20),
            );

        let bw = if self.style.border_px() > 0 {
            self.style.border_px()
        } else if trig_border != Color::TRANSPARENT {
            1
        } else {
            0
        };
        if bw > 0 && trig_border != Color::TRANSPARENT {
            trigger = trigger.border(bw, trig_border);
        }
        if !self.focus_ring {
            trigger = trigger.no_focus_border();
        }
        if let Some(h) = self.hover_bg {
            trigger = trigger.hover_bg(h);
        }
        if let Some(a) = self.active_bg {
            trigger = trigger.active_bg(a);
        }
        if let Some(cb) = self.on_toggle {
            trigger = trigger.on_click_raw(cb);
        }

        let trigger_el = match self.trigger_renderer {
            Some(f) => f(trigger, self.open, cx),
            None => trigger.into_element(cx),
        };

        let mut root = div().flex_col().w(eff_width).child(cx, trigger_el);

        if self.open && !self.options.is_empty() {
            let mut list = div()
                .id(crate::scoped_child_id(self.id, 0x11C2_662D, 100))
                .pos(0, 36)
                .overlay()
                .occlude_hit()
                .flex_col()
                .w(eff_width)
                .p(4)
                .gap(2)
                .rounded(eff_rounded)
                .bg(popup_bg);
            if self.item_renderer.is_some() {
                list = list.text_color(text_color).text_size(13);
            }

            if popup_border_color != Color::TRANSPARENT {
                list = list.border(1, popup_border_color);
            }
            if let Some((blur, ox, oy, col)) = self.shadow {
                list = list.shadow(blur, ox, oy, col);
            }
            if self.animated {
                list = list
                    .slide_in(0, -6, Duration::from_millis(140))
                    .slide_out(0, -6, Duration::from_millis(120));
            }
            if let Some(toggle_cb) = self.on_toggle {
                list = list.on_navigate_back(toggle_cb);
            }
            if let Some(pop_fn) = self.popup_renderer {
                list = pop_fn(list);
            }

            if self.on_select.is_some() || self.on_toggle.is_some() {
                let options = self.options;
                let cur_sel = self.selected_index;
                let on_sel = self.on_select;
                let on_tog = self.on_toggle;
                let nav = cx.arena().alloc(move |key: KeyCode, scope: &mut ActionScope| {
                    let len = options.len();
                    if len == 0 {
                        return;
                    }
                    match key {
                        KeyCode::Down => {
                            if let Some(cb) = on_sel {
                                let start = cur_sel.map(|i| (i + 1) % len).unwrap_or(0);
                                for step in 0..len {
                                    let cand = (start + step) % len;
                                    if !options[cand].disabled {
                                        cb(cand, scope);
                                        break;
                                    }
                                }
                            }
                        }
                        KeyCode::Up => {
                            if let Some(cb) = on_sel {
                                let start = cur_sel.map(|i| (i + len - 1) % len).unwrap_or(len - 1);
                                for step in 0..len {
                                    let cand = (start + len - step) % len;
                                    if !options[cand].disabled {
                                        cb(cand, scope);
                                        break;
                                    }
                                }
                            }
                        }
                        KeyCode::Home => {
                            if let Some(cb) = on_sel {
                                if let Some(idx) = options.iter().position(|o| !o.disabled) {
                                    cb(idx, scope);
                                }
                            }
                        }
                        KeyCode::End => {
                            if let Some(cb) = on_sel {
                                if let Some(idx) = options.iter().rposition(|o| !o.disabled) {
                                    cb(idx, scope);
                                }
                            }
                        }
                        KeyCode::Enter | KeyCode::Space => {
                            if let Some(cb) = on_tog {
                                cb(scope);
                            }
                        }
                        _ => {}
                    }
                });
                list = list.on_key_nav(nav);
            }

            let on_sel = self.on_select;
            let row_r = eff_rounded.saturating_sub(2);
            for (idx, opt) in self.options.iter().enumerate() {
                let is_sel = self.selected_index == Some(idx);
                let disp_opt = match self.label_formatter {
                    Some(f) => f(*opt),
                    None => StrRef::from(opt.label),
                };

                if let Some(render_fn) = self.item_renderer {
                    let mut row = div()
                        .id(crate::scoped_child_id(self.id, 0x11C2_662D, 101 + idx as u32))
                        .child(cx, text(disp_opt).ellipsis());
                    if !opt.disabled {
                        if let Some(cb) = on_sel {
                            let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                                cb(idx, scope);
                            });
                            row = row.on_click_raw(click_cb);
                        }
                    } else {
                        row = row.cursor(CursorShape::NotAllowed);
                    }
                    let el = render_fn(
                        row,
                        SelectItemState {
                            index: idx,
                            label: opt.label,
                            value: opt.value,
                            selected: is_sel,
                            disabled: opt.disabled,
                        },
                        cx,
                    );
                    list = list.child(cx, el);
                    continue;
                }

                let fg = if opt.disabled {
                    disabled_color
                } else if is_sel {
                    selected_fg
                } else {
                    text_color
                };

                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x11C2_662D, 101 + idx as u32))
                    .flex_row()
                    .w(eff_width - 8)
                    .h(28)
                    .px(8)
                    .gap(8)
                    .rounded(row_r)
                    .bg(if is_sel {
                        selected_bg
                    } else {
                        Color::TRANSPARENT
                    })
                    .items_center()
                    .justify_between()
                    .child(cx, text(disp_opt).size(13).color(fg).ellipsis());

                if is_sel {
                    row = row.child(
                        cx,
                        svg_icon(13, 13, "M20 6 9 17l-5-5")
                            .tint(check_col)
                            .stroke_width(24),
                    );
                }

                if !opt.disabled {
                    if let Some(h) = self.hover_bg {
                        row = row.hover_bg(if is_sel && selected_bg != Color::TRANSPARENT {
                            selected_bg
                        } else {
                            h
                        });
                    }
                    if let Some(a) = self.active_bg {
                        row = row.active_bg(a);
                    }
                    if let Some(cb) = on_sel {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(idx, scope);
                        });
                        row = row.on_click_raw(click_cb);
                    }
                } else {
                    row = row.opacity(125).cursor(CursorShape::NotAllowed);
                }
                list = list.child(cx, row);
            }
            root = root.child(cx, list);
        }

        self.style_ext.apply_ext_only(root).into_element(cx)
    }
}
