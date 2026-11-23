//! 搜索组合下拉框 `Combobox`（默认回退 `cx.theme()`）。

use velox_engine::prelude::*;

pub use crate::components::text_view::{MarkdownView, SelectableText};

const AUTO_COLOR: Color = Color(0x0000_0001);

#[inline(always)]
fn resolve_color(c: Color, fallback: Color) -> Color {
    if c == AUTO_COLOR {
        fallback
    } else {
        c
    }
}

/// 传递给 `Combobox::item(...)` 插槽回调的候选项状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboboxItemState<'a> {
    pub index: usize,
    pub label: &'a str,
    pub selected: bool,
}

impl<'a> ComboboxItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn label(&self) -> &'a str { self.label }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.selected }
}

type ComboboxItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, ComboboxItemState<'a>, &'a FrameScope) -> Element<'a>;
type ComboboxTriggerRenderer<'a> = &'a dyn Fn(Div<'a>, bool, &'a FrameScope) -> Element<'a>;
type ComboboxPopupRenderer<'a> = &'a dyn Fn(Div<'a>) -> Div<'a>;

/// 带搜索过滤能力的下拉组合框组件（`Combobox`）。
pub struct Combobox<'a> {
    pub id: u32,
    pub query: &'a str,
    pub options: &'a [&'a str],
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
    pub text_color: Color,
    pub muted_color: Color,
    pub hover_bg: Option<Color>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub on_toggle_open: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    item_renderer: Option<ComboboxItemRenderer<'a>>,
    trigger_renderer: Option<ComboboxTriggerRenderer<'a>>,
    popup_renderer: Option<ComboboxPopupRenderer<'a>>,
}

impl<'a> Combobox<'a> {
    /// 创建一个无样式可过滤组合选择框原语。
    #[inline(always)]
    pub const fn new(
        id: u32,
        query: &'a str,
        options: &'a [&'a str],
        selected_index: Option<usize>,
        open: bool,
    ) -> Self {
        Self {
            id,
            query,
            options,
            selected_index,
            open,
            placeholder: "Search or select...",
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
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            popup_bg: Color::TRANSPARENT,
            popup_border_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: AUTO_COLOR,
            text_color: AUTO_COLOR,
            muted_color: AUTO_COLOR,
            hover_bg: None,
            focus_ring: false,
            rounded: 0,
            on_toggle_open: None,
            on_select: None,
            on_key_input: None,
            item_renderer: None,
            trigger_renderer: None,
            popup_renderer: None,
        }
    }

    /// 判断候选条目 `option` 是否匹配当前过滤关键词（ASCII 大小写不敏感子串匹配）。
    pub fn matches_query(option: &str, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let ob = option.as_bytes();
        let qb = query.as_bytes();
        if qb.len() > ob.len() {
            return false;
        }
        for i in 0..=(ob.len() - qb.len()) {
            if ob[i..i + qb.len()].eq_ignore_ascii_case(qb) {
                return true;
            }
        }
        false
    }

    /// 绑定展开/收起回调。
    #[inline(always)]
    pub fn on_toggle_open(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_open = Some(handler);
        self
    }

    /// 绑定选中某项回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// 绑定搜索词键盘输入回调。
    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// `gpui-base` 风格候选项插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, ComboboxItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: ComboboxItemState<'a>, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格触发器插槽回调（零堆分配）。
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

    /// `gpui-base` 风格弹出面板容器样式定制回调。
    #[inline]
    pub fn popup<F>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>) -> Div<'a> + 'a,
    {
        self.popup_renderer = Some(cx.arena().alloc(f));
        self
    }
}

crate::impl_base_styled_box!(Combobox<'a>);

impl<'a> Widget<'a> for Combobox<'a> {
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
        let text_color = resolve_color(self.text_color, def_fg);
        let muted_color = resolve_color(self.muted_color, def_fg.scale_alpha(140));

        let (display_text, is_placeholder) = if !self.query.is_empty() {
            (self.query, false)
        } else if let Some(opt) = self.selected_index.and_then(|idx| self.options.get(idx).copied()) {
            (opt, false)
        } else {
            (self.placeholder, true)
        };

        let border_col = if self.open {
            active_border_color
        } else {
            border_color
        };

        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let mut trigger = div()
            .id(self.id)
            .flex_row()
            .w(eff_w)
            .h(32)
            .px(10)
            .rounded(self.rounded)
            .bg(bg)
            .items_center()
            .justify_between()
            .focusable()
            .child(
                cx,
                text(display_text)
                    .size(13)
                    .color(if is_placeholder { muted_color } else { text_color })
                    .ellipsis(),
            )
            .child(cx, text("v").size(11).color(muted_color));
        if border_col != Color::TRANSPARENT {
            trigger = trigger.border(1, border_col);
        }
        if !self.focus_ring {
            trigger = trigger.no_focus_border();
        }

        if let Some(cb) = self.on_toggle_open {
            trigger = trigger.on_click_raw(cb);
        }
        if let Some(kcb) = self.on_key_input {
            trigger = trigger.on_key_input(kcb);
        }

        let trigger_el = match self.trigger_renderer {
            Some(f) => f(trigger, self.open, cx),
            None => trigger.into_element(cx),
        };
        let mut root = div().flex_col().child(cx, trigger_el);

        if self.open {
            let mut list_box = div()
                .id(crate::scoped_child_id(self.id, 0xAFB6_5F8E, 100))
                .pos(0, 36)
                .overlay()
                .occlude_hit()
                .flex_col()
                .w(eff_w)
                .p(4)
                .gap(2)
                .rounded(self.rounded)
                .bg(popup_bg);
            if popup_border_color != Color::TRANSPARENT {
                list_box = list_box.border(1, popup_border_color);
            }
            if let Some(pop_fn) = self.popup_renderer {
                list_box = pop_fn(list_box);
            }

            let on_sel = self.on_select;
            let item_r = self.rounded.saturating_sub(2);
            let mut matched_any = false;
            for (idx, &opt) in self.options.iter().enumerate() {
                if !Self::matches_query(opt, self.query) {
                    continue;
                }
                matched_any = true;
                let is_sel = self.selected_index == Some(idx);
                if let Some(item_fn) = self.item_renderer {
                    let mut row = div()
                        .id(crate::scoped_child_id(self.id, 0xAFB6_5F8E, idx as u32 + 1))
                        .child(cx, text(opt).ellipsis());
                    if let Some(cb) = on_sel {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(idx, scope);
                        });
                        row = row.on_click_raw(click_cb);
                    }
                    list_box = list_box.child(
                        cx,
                        item_fn(
                            row,
                            ComboboxItemState {
                                index: idx,
                                label: opt,
                                selected: is_sel,
                            },
                            cx,
                        ),
                    );
                    continue;
                }
                let mut item = div()
                    .id(crate::scoped_child_id(self.id, 0xAFB6_5F8E, idx as u32 + 1))
                    .w(eff_w - 8)
                    .h(26)
                    .px(8)
                    .rounded(item_r)
                    .bg(if is_sel {
                        selected_bg
                    } else {
                        Color::TRANSPARENT
                    })
                    .items_center()
                    .child(
                        cx,
                        text(opt)
                            .size(12)
                            .color(if is_sel { selected_fg } else { text_color })
                            .ellipsis(),
                    );
                if let Some(h) = self.hover_bg {
                    item = item.hover_bg(if is_sel && selected_bg != Color::TRANSPARENT {
                        selected_bg
                    } else {
                        h
                    });
                }
                if let Some(cb) = on_sel {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    item = item.on_click_raw(click_cb);
                }
                list_box = list_box.child(cx, item);
            }

            if !matched_any {
                list_box = list_box.child(
                    cx,
                    div()
                        .h(26)
                        .px(8)
                        .items_center()
                        .child(
                            cx,
                            text(crate::i18n::widget_t(cx, "velox.common.no_results"))
                                .size(12)
                                .color(muted_color),
                        ),
                );
            }

            root = root.child(cx, list_box);
        }

        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
