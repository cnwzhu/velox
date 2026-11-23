//! 通用列表展示基础组件 `List` 与条目 `ListItem`（默认无样式）。

use velox_core::prelude::*;

/// 列表项描述结构（`ListItem`）。
#[derive(Clone, Copy, Debug)]
pub struct ListItem<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub trailing: &'a str,
}

impl<'a> ListItem<'a> {
    /// 创建一个列表项。
    #[inline(always)]
    pub const fn new(title: &'a str, subtitle: &'a str, trailing: &'a str) -> Self {
        Self {
            title,
            subtitle,
            trailing,
        }
    }
}

/// 传递给 `List::item(...)` 插槽回调的列表行状态。
#[derive(Clone, Copy, Debug)]
pub struct ListItemState<'a> {
    pub index: usize,
    pub item: ListItem<'a>,
    pub selected: bool,
}

impl<'a> ListItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn item(&self) -> ListItem<'a> { self.item }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.selected }
}

type ListItemRenderer<'a> = &'a dyn Fn(Div<'a>, ListItemState<'a>, &'a FrameScope) -> Element<'a>;

/// 通用列表展示基础组件（`List`，默认无样式，支持双行标题/副标题、右侧元数据、行选中与 `gpui-base` 插槽回调）。
pub struct List<'a> {
    pub id: u32,
    pub items: &'a [ListItem<'a>],
    pub selected_index: Option<usize>,
    pub width: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub rounded: u8,
    pub focus_ring: bool,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<ListItemRenderer<'a>>,
}

impl<'a> List<'a> {
    /// 创建一个默认无样式的列表展示组件。
    #[inline(always)]
    pub const fn new(id: u32, items: &'a [ListItem<'a>]) -> Self {
        Self {
            id,
            items,
            selected_index: None,
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
                flex_flags: 1,
            },
            style_ext: StyleExt::DEFAULT,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            rounded: 0,
            focus_ring: false,
            on_select: None,
            item_renderer: None,
        }
    }

    /// 设置当前选中的行索引。
    #[inline(always)]
    pub const fn selected(mut self, selected_index: Option<usize>) -> Self {
        self.selected_index = selected_index;
        self
    }

    /// 设置列表宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定点击某一项的回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格列表行插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, ListItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |row: Div<'a>, state: ListItemState<'a>, frame_cx: &'a FrameScope| {
                f(row, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(List<'a>);

impl<'a> Widget<'a> for List<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
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
        let selected_fg = if self.selected_fg.a() > 0 {
            self.selected_fg
        } else {
            fg
        };
        let eff_w = match self.style.width {
            Length::Px(w) => w,
            _ => self.width,
        };

        let mut container = div().id(self.id);
        container.style = self.style;
        container.style.set_col();
        if matches!(container.style.width, Length::Auto) {
            container = container.w(eff_w);
        }
        if container.style.padding == Edges::all(0) && self.item_renderer.is_none() {
            container = container.p(4);
        }
        if container.style.gap == 0 {
            container = container.gap(2);
        }
        if container.style.rounded_radius() == 0 && self.rounded > 0 {
            container = container.rounded(self.rounded);
        }
        if container.style.bg == Color::TRANSPARENT && self.bg != Color::TRANSPARENT {
            container = container.bg(self.bg);
        }
        if container.style.border_px() == 0 && self.border_color.a() > 0 {
            container = container.border(1, self.border_color);
        }
        if self.item_renderer.is_some() {
            container = container.text_color(text_color).text_size(13);
        }
        container = self.style_ext.apply_ext_only(container);

        let item_rounded = container.style.rounded_radius().saturating_sub(2);
        let on_sel = self.on_select;
        for (idx, &item) in self.items.iter().enumerate() {
            let is_sel = self.selected_index == Some(idx);
            if let Some(render_fn) = self.item_renderer {
                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x0CFB_5881, idx as u32 + 1))
                    .focusable();
                if !self.focus_ring {
                    row = row.no_focus_border();
                }
                if let Some(cb) = on_sel {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    row = row.on_click_raw(click_cb);
                }
                row = row.child(cx, text(item.title));
                let el = render_fn(
                    row,
                    ListItemState {
                        index: idx,
                        item,
                        selected: is_sel,
                    },
                    cx,
                );
                container = container.child(cx, el);
                continue;
            }
            let mut left_col = div().flex_col().gap(2).child(
                cx,
                text(item.title).size(13).color(if is_sel {
                    selected_fg
                } else {
                    text_color
                }),
            );
            if !item.subtitle.is_empty() {
                left_col = left_col.child(
                    cx,
                    text(item.subtitle).size(11).color(if is_sel {
                        selected_fg
                    } else {
                        muted_color
                    }),
                );
            }

            let row_h = if item.subtitle.is_empty() { 30 } else { 44 };
            let mut row = div()
                .id(crate::scoped_child_id(self.id, 0x0CFB_5881, idx as u32 + 1))
                .flex_row()
                .w(eff_w - 8)
                .h(row_h)
                .px(10)
                .rounded(item_rounded)
                .bg(if is_sel {
                    self.selected_bg
                } else {
                    Color::TRANSPARENT
                })
                .items_center()
                .justify_between()
                .focusable();
            if let Some(h) = self.hover_bg {
                row = row.hover_bg(if is_sel { self.selected_bg } else { h });
            }
            if let Some(a) = self.active_bg {
                row = row.active_bg(a);
            }
            if !self.focus_ring {
                row = row.no_focus_border();
            }
            row = row.child(cx, left_col);

            if !item.trailing.is_empty() {
                row = row.child(
                    cx,
                    text(item.trailing).size(11).color(if is_sel {
                        selected_fg
                    } else {
                        muted_color
                    }),
                );
            }

            if let Some(cb) = on_sel {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(idx, scope);
                });
                row = row.on_click_raw(click_cb);
            }
            container = container.child(cx, row);
        }

        container.into_element(cx)
    }
}
