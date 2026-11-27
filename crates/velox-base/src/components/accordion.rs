//! 手风琴折叠面板基础组件 `Accordion` 与单块可折叠区域 `Collapsible`（默认无样式）。

use velox_engine::prelude::*;

/// 传递给 `Collapsible::trigger(...)` / `Collapsible::panel(...)` 插槽回调的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollapsibleState<'a> {
    pub title: &'a str,
    pub open: bool,
}

impl<'a> CollapsibleState<'a> {
    #[inline(always)] pub const fn title(&self) -> &'a str { self.title }
    #[inline(always)] pub const fn is_open(&self) -> bool { self.open }
}

type CollapsibleSlotRenderer<'a> =
    &'a dyn Fn(Div<'a>, CollapsibleState<'a>, &'a FrameScope) -> Element<'a>;

/// 单个可折叠区域基础组件（`Collapsible`，默认无样式）。
pub struct Collapsible<'a> {
    pub id: u32,
    pub title: &'a str,
    pub open: bool,
    pub width: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub header_bg: Color,
    pub content_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub arrow_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub rounded: u8,
    pub focus_ring: bool,
    pub animated: bool,
    pub content: Option<Element<'a>>,
    pub on_toggle: Option<&'a dyn Fn(bool, &mut ActionScope)>,
    trigger_renderer: Option<CollapsibleSlotRenderer<'a>>,
    panel_renderer: Option<CollapsibleSlotRenderer<'a>>,
}

impl<'a> Collapsible<'a> {
    /// 创建一个默认无样式的可折叠容器。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, open: bool) -> Self {
        Self {
            id,
            title,
            open,
            width: 280,
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
            content_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            arrow_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            rounded: 0,
            focus_ring: false,
            animated: false,
            content: None,
            on_toggle: None,
            trigger_renderer: None,
            panel_renderer: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，展开/折叠时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, title: &'a str, sig: Signal<bool>) -> Self {
        let cb = cx.arena().alloc(move |open: bool, _: &mut ActionScope| {
            sig.set_if_changed(open);
        });
        Self::new(id, title, sig.get()).on_toggle(cb)
    }

    /// 设置折叠面板宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 设置展开时显示的内部子元素。
    #[inline(always)]
    pub fn content(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.content = Some(child.into_element(cx));
        self
    }

    /// 启用或禁用展开内容滑入与标题配色过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定展开/折叠状态切换回调。
    #[inline(always)]
    pub fn on_toggle(mut self, handler: &'a dyn Fn(bool, &mut ActionScope)) -> Self {
        self.on_toggle = Some(handler);
        self
    }

    /// `gpui-base` 风格触发器头部插槽回调（零堆分配）。
    #[inline]
    pub fn trigger<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, CollapsibleState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: CollapsibleState<'a>, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.trigger_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格展开内容面板插槽回调（零堆分配）。
    #[inline]
    pub fn panel<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, CollapsibleState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: CollapsibleState<'a>, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.panel_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Collapsible<'a>);

impl<'a> Widget<'a> for Collapsible<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let eff_w = StyleExt::resolve_width(self.style, self.width);
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let arrow_color = if self.arrow_color.a() > 0 {
            self.arrow_color
        } else {
            fg
        };
        let state = CollapsibleState {
            title: self.title,
            open: self.open,
        };

        let mut header_base = div().id(self.id).focusable();
        if !self.focus_ring {
            header_base = header_base.no_focus_border();
        }
        if let Some(cb) = self.on_toggle {
            let next = !self.open;
            let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                cb(next, scope);
            });
            header_base = header_base.on_click_raw(click_cb);
        }

        let header_el = if let Some(trig_fn) = self.trigger_renderer {
            trig_fn(header_base.child(cx, text(self.title)), state, cx)
        } else {
            let arrow_d = if self.open {
                "M6 9l6 6 6-6"
            } else {
                "M9 18l6-6-6-6"
            };
            let mut header = header_base
                .flex_row()
                .w(eff_w)
                .h(36)
                .px(12)
                .bg(self.header_bg)
                .items_center()
                .justify_between();
            if let Some(h) = self.hover_bg {
                header = header.hover_bg(h);
            }
            if let Some(a) = self.active_bg {
                header = header.active_bg(a);
            }
            if self.animated {
                header = header.animate_color(Duration::from_millis(140));
            }
            header
                .child(cx, text(self.title).size(13).color(text_color))
                .child(
                    cx,
                    svg_icon(14, 14, arrow_d)
                        .tint(arrow_color)
                        .stroke_width(20),
                )
                .into_element(cx)
        };

        let mut wrap = div()
            .id(crate::scoped_child_id(self.id, 0x75C4_E819, 1))
            .flex_col()
            .w(eff_w)
            .rounded(self.rounded)
            .overflow_hidden()
            .child(cx, header_el);
        if self.animated {
            wrap = wrap.spring_bounds(Duration::from_millis(200));
        }
        if self.border_color.a() > 0 {
            wrap = wrap.border(1, self.border_color);
        }

        if self.open {
            if let Some(body) = self.content {
                if self.border_color.a() > 0 && self.panel_renderer.is_none() {
                    wrap = wrap.child(cx, div().w(eff_w).h(1).bg(self.border_color));
                }
                if let Some(panel_fn) = self.panel_renderer {
                    wrap = wrap.child(cx, panel_fn(div().child(cx, body), state, cx));
                } else {
                    let mut body_div = div()
                        .id(crate::scoped_child_id(self.id, 0x75C4_E819, 99))
                        .w(eff_w)
                        .p(12)
                        .bg(self.content_bg)
                        .child(cx, body);
                    if self.animated {
                        body_div = body_div
                            .enter_from(255, 0, -6, Duration::from_millis(180))
                            .ease(Easing::EaseOut)
                            .exit_to(0, 0, -6, Duration::from_millis(150));
                    }
                    wrap = wrap.child(cx, body_div);
                }
            }
        }

        self.style_ext
            .apply_to_div(wrap, self.style)
            .into_element(cx)
    }
}

/// 手风琴条目描述（`AccordionItem`）。
#[derive(Clone, Copy)]
pub struct AccordionItem<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

impl<'a> AccordionItem<'a> {
    /// 创建一条手风琴标题与正文条目。
    #[inline(always)]
    pub const fn new(title: &'a str, body: &'a str) -> Self {
        Self { title, body }
    }
}

/// 传递给 `Accordion::item(...)` 插槽回调的手风琴条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccordionItemState<'a> {
    pub index: usize,
    pub title: &'a str,
    pub body: &'a str,
    pub open: bool,
}

impl<'a> AccordionItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn title(&self) -> &'a str { self.title }
    #[inline(always)] pub const fn body(&self) -> &'a str { self.body }
    #[inline(always)] pub const fn is_open(&self) -> bool { self.open }
}

type AccordionItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, AccordionItemState<'a>, &'a FrameScope) -> Element<'a>;

/// 多项手风琴折叠面板基础组件（`Accordion`，默认无样式，支持 `gpui-base` 插槽回调与外层 `Styled`）。
pub struct Accordion<'a> {
    pub id: u32,
    pub items: &'a [AccordionItem<'a>],
    /// 位掩码：第 `i` 位为 `1` 表示第 `i` 个面板处于展开状态。
    pub open_mask: u32,
    pub width: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub header_bg: Color,
    pub content_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub body_text_color: Color,
    pub arrow_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub rounded: u8,
    pub focus_ring: bool,
    pub animated: bool,
    pub on_toggle_item: Option<&'a dyn Fn(usize, bool, &mut ActionScope)>,
    item_renderer: Option<AccordionItemRenderer<'a>>,
}

impl<'a> Accordion<'a> {
    /// 创建一个默认无样式的手风琴组件。
    #[inline(always)]
    pub const fn new(id: u32, items: &'a [AccordionItem<'a>], open_mask: u32) -> Self {
        Self {
            id,
            items,
            open_mask,
            width: 300,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 4,
                rounded: 0,
                border_width: 0,
                flex_flags: 1,
            },
            style_ext: StyleExt::DEFAULT,
            header_bg: Color::TRANSPARENT,
            content_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            body_text_color: Color::TRANSPARENT,
            arrow_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            rounded: 0,
            focus_ring: false,
            animated: false,
            on_toggle_item: None,
            item_renderer: None,
        }
    }

    /// 设置宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 启用或禁用手风琴条目展开过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定点击切换某一项展开状态的回调。
    #[inline(always)]
    pub fn on_toggle_item(mut self, handler: &'a dyn Fn(usize, bool, &mut ActionScope)) -> Self {
        self.on_toggle_item = Some(handler);
        self
    }

    /// `gpui-base` 风格条目插槽回调：传入绑定了点击切换回调的标题 `Div<'a>` 与 [`AccordionItemState`]。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, AccordionItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |header: Div<'a>, state: AccordionItemState<'a>, frame_cx: &'a FrameScope| {
                f(header, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Accordion<'a>);

impl<'a> Widget<'a> for Accordion<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let body_text_color = if self.body_text_color.a() > 0 {
            self.body_text_color
        } else {
            fg
        };
        let eff_w = match self.style.width {
            Length::Px(w) => w,
            _ => self.width,
        };

        let mut root = div().id(self.id);
        root.style = self.style;
        root.style.set_col();
        if matches!(root.style.width, Length::Auto) {
            root = root.w(eff_w);
        }
        if self.item_renderer.is_some() {
            root = root.text_color(fg).text_size(13);
        }
        root = self.style_ext.apply_ext_only(root);
        let on_toggle = self.on_toggle_item;

        for (idx, item) in self.items.iter().enumerate() {
            let is_open = (self.open_mask & (1u32 << (idx as u32))) != 0;
            if let Some(render_fn) = self.item_renderer {
                let mut hdr = div()
                    .id(crate::scoped_child_id(self.id, 0x75C4_E819, idx as u32 + 1))
                    .focusable();
                if !self.focus_ring {
                    hdr = hdr.no_focus_border();
                }
                if let Some(cb) = on_toggle {
                    let next = !is_open;
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, next, scope);
                    });
                    hdr = hdr.on_click_raw(click_cb);
                }
                hdr = hdr.child(cx, text(item.title));
                let el = render_fn(
                    hdr,
                    AccordionItemState {
                        index: idx,
                        title: item.title,
                        body: item.body,
                        open: is_open,
                    },
                    cx,
                );
                root = root.child(cx, el);
                continue;
            }
            let mut col = Collapsible::new(
                crate::scoped_child_id(self.id, 0x75C4_E819, idx as u32 + 1),
                item.title,
                is_open,
            )
            .width(eff_w)
            .content(cx, text(item.body).size(12).color(body_text_color));
            col.header_bg = self.header_bg;
            col.content_bg = self.content_bg;
            col.border_color = self.border_color;
            col.text_color = self.text_color;
            col.arrow_color = self.arrow_color;
            col.hover_bg = self.hover_bg;
            col.active_bg = self.active_bg;
            col.rounded = self.rounded;
            col.focus_ring = self.focus_ring;
            col.animated = self.animated;
            if let Some(cb) = on_toggle {
                let item_cb = cx.arena().alloc(move |next: bool, scope: &mut ActionScope| {
                    cb(idx, next, scope);
                });
                col = col.on_toggle(item_cb);
            }
            root = root.child(cx, col);
        }

        root.into_element(cx)
    }
}
