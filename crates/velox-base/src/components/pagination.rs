//! 分页导航基础组件 `Pagination`（默认无样式）。

use velox_engine::prelude::*;

/// 分页插槽按钮类型。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PaginationItemKind {
    Previous,
    Page,
    Next,
}

/// 传递给 `Pagination::item(...)` 插槽回调的按钮状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PaginationItemState {
    pub kind: PaginationItemKind,
    pub page: usize,
    pub active: bool,
    pub disabled: bool,
}

impl PaginationItemState {
    #[inline(always)] pub const fn kind(&self) -> PaginationItemKind { self.kind }
    #[inline(always)] pub const fn page(&self) -> usize { self.page }
    #[inline(always)] pub const fn is_active(&self) -> bool { self.active }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type PaginationItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, PaginationItemState, &'a FrameScope) -> Element<'a>;
type PaginationLabelFormatter<'a> = &'a dyn Fn(PaginationItemKind, usize) -> StrRef<'a>;

/// 分页导航条基础组件（`Pagination`，默认无样式，页码从 `1` 开始，支持 `gpui-base` 插槽回调与外层 `Styled`）。
pub struct Pagination<'a> {
    pub id: u32,
    pub current_page: usize,
    pub total_pages: usize,
    pub visible_pages: usize,
    pub style: Style,
    pub style_ext: StyleExt,
    pub item_bg: Color,
    pub active_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub active_text_color: Color,
    pub disabled_text_color: Color,
    pub hover_bg: Option<Color>,
    pub press_bg: Option<Color>,
    pub active_hover_bg: Option<Color>,
    pub active_press_bg: Option<Color>,
    pub active_focus_border: Option<(u8, Color)>,
    pub rounded: u8,
    pub focus_ring: bool,
    pub on_change: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<PaginationItemRenderer<'a>>,
    label_formatter: Option<PaginationLabelFormatter<'a>>,
}

impl<'a> Pagination<'a> {
    /// 创建一个默认无样式的分页导航器。
    #[inline(always)]
    pub const fn new(id: u32, current_page: usize, total_pages: usize) -> Self {
        Self {
            id,
            current_page: if current_page == 0 { 1 } else { current_page },
            total_pages: if total_pages == 0 { 1 } else { total_pages },
            visible_pages: 5,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 4,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            item_bg: Color::TRANSPARENT,
            active_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            active_text_color: Color::TRANSPARENT,
            disabled_text_color: Color::TRANSPARENT,
            hover_bg: None,
            press_bg: None,
            active_hover_bg: None,
            active_press_bg: None,
            active_focus_border: None,
            rounded: 0,
            focus_ring: false,
            on_change: None,
            item_renderer: None,
            label_formatter: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，翻页时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, total_pages: usize, sig: Signal<usize>) -> Self {
        let cb = cx.arena().alloc(move |page: usize, _: &mut ActionScope| {
            sig.set_if_changed(page);
        });
        Self::new(id, sig.get(), total_pages).on_change(cb)
    }

    /// 设置最多同时展示的页码按钮数量。
    #[inline(always)]
    pub const fn visible_pages(mut self, count: usize) -> Self {
        self.visible_pages = if count < 3 { 3 } else { count };
        self
    }

    /// 绑定页码切换回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// `gpui-base` 风格页码/翻页按钮插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, PaginationItemState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |btn: Div<'a>, state: PaginationItemState, frame_cx: &'a FrameScope| {
                f(btn, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// 自定义分页按钮文案格式化回调。
    #[inline]
    pub fn label<F, S>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(PaginationItemKind, usize) -> S + 'a,
        S: Into<StrRef<'a>>,
    {
        let cb = cx
            .arena()
            .alloc(move |kind: PaginationItemKind, page: usize| f(kind, page).into());
        self.label_formatter = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Pagination<'a>);

impl<'a> Widget<'a> for Pagination<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg_base = cx.theme().foreground;
        let cur = self.current_page.clamp(1, self.total_pages);
        let on_change = self.on_change;
        let item_bg = self.item_bg;
        let active_bg = self.active_bg;
        let border_col = self.border_color;
        let text_col = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg_base
        };
        let active_text_col = if self.active_text_color.a() > 0 {
            self.active_text_color
        } else {
            fg_base
        };
        let disabled_text_col = if self.disabled_text_color.a() > 0 {
            self.disabled_text_color
        } else {
            fg_base.scale_alpha(115)
        };
        let rounded = self.rounded;
        let hover_bg = self.hover_bg;
        let press_bg = self.press_bg;
        let active_hover_bg = self.active_hover_bg;
        let active_press_bg = self.active_press_bg;
        let active_focus_border = self.active_focus_border;
        let focus_ring = self.focus_ring;
        let item_renderer = self.item_renderer;
        let label_formatter = self.label_formatter;

        let make_btn = |btn_id: u32,
                        kind: PaginationItemKind,
                        icon_d: Option<&'static str>,
                        target_page: usize,
                        active: bool,
                        enabled: bool|
         -> Element<'a> {
            let formatted_label: Option<StrRef<'a>> = match label_formatter {
                Some(f) => Some(f(kind, target_page)),
                None => match kind {
                    PaginationItemKind::Page => Some(fmt!(cx, "{}", target_page)),
                    PaginationItemKind::Previous if item_renderer.is_some() => Some(StrRef::from("‹")),
                    PaginationItemKind::Next if item_renderer.is_some() => Some(StrRef::from("›")),
                    _ => None,
                },
            };

            if let Some(render_fn) = item_renderer {
                let mut d = div().id(btn_id);
                if let Some(lbl) = formatted_label {
                    d = d.child(cx, text(lbl).text_center().nowrap());
                }
                if enabled {
                    d = d.focusable();
                    if !focus_ring {
                        d = d.no_focus_border();
                    }
                    if let Some(cb) = on_change {
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(target_page, scope);
                        });
                        d = d.on_click_raw(click_cb);
                    }
                } else {
                    d = d.cursor(CursorShape::NotAllowed);
                }
                return render_fn(
                    d,
                    PaginationItemState {
                        kind,
                        page: target_page,
                        active,
                        disabled: !enabled,
                    },
                    cx,
                );
            }

            let bg = if active { active_bg } else { item_bg };
            let fg = if !enabled {
                disabled_text_col
            } else if active {
                active_text_col
            } else {
                text_col
            };

            let mut d = div()
                .id(btn_id)
                .w(28)
                .h(28)
                .rounded(rounded)
                .bg(bg)
                .items_center()
                .justify_center();

            let bcol = if active && active_bg.a() > 0 {
                active_bg
            } else {
                border_col
            };
            if bcol.a() > 0 {
                d = d.border(1, bcol);
            }

            if let Some(lbl) = formatted_label {
                d = d.child(cx, text(lbl).size(12).color(fg).text_center().nowrap());
            } else if let Some(path) = icon_d {
                d = d.child(cx, svg_icon(13, 13, path).tint(fg).stroke_width(20));
            }

            if enabled {
                if active {
                    if let Some(h) = active_hover_bg {
                        d = d.hover_bg(h);
                    }
                    if let Some(p) = active_press_bg {
                        d = d.active_bg(p);
                    }
                } else {
                    if let Some(h) = hover_bg {
                        d = d.hover_bg(h);
                    }
                    if let Some(p) = press_bg {
                        d = d.active_bg(p);
                    }
                }
                d = d.focusable();
                if active {
                    if let Some((w, col)) = active_focus_border {
                        d = d.focus_border(w, col);
                    } else if !focus_ring {
                        d = d.no_focus_border();
                    }
                } else if !focus_ring {
                    d = d.no_focus_border();
                }
                if let Some(cb) = on_change {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(target_page, scope);
                    });
                    d = d.on_click_raw(click_cb);
                }
            } else {
                d = d.opacity(115).cursor(CursorShape::NotAllowed);
            }
            d.into_element(cx)
        };

        let mut row = div().id(self.id);
        row.style = self.style;
        row = row.items_center();
        if item_renderer.is_some() {
            row = row.text_color(text_col).text_size(12);
        }
        row = self.style_ext.apply_ext_only(row);
        row = row.child(
            cx,
            make_btn(
                crate::scoped_child_id(self.id, 0x46C0_13C9, 1),
                PaginationItemKind::Previous,
                Some("M15 18l-6-6 6-6"),
                cur.saturating_sub(1).max(1),
                false,
                cur > 1,
            ),
        );

        let half = self.visible_pages / 2;
        let start_page = cur.saturating_sub(half).max(1);
        let end_page = (start_page + self.visible_pages - 1).min(self.total_pages);
        let start_page = end_page.saturating_sub(self.visible_pages - 1).max(1);

        for p in start_page..=end_page {
            row = row.child(
                cx,
                make_btn(
                    crate::scoped_child_id(self.id, 0x46C0_13C9, 10 + p as u32),
                    PaginationItemKind::Page,
                    None,
                    p,
                    p == cur,
                    true,
                ),
            );
        }

        row = row.child(
            cx,
            make_btn(
                crate::scoped_child_id(self.id, 0x46C0_13C9, 2),
                PaginationItemKind::Next,
                Some("M9 18l6-6-6-6"),
                (cur + 1).min(self.total_pages),
                false,
                cur < self.total_pages,
            ),
        );

        row.into_element(cx)
    }
}
