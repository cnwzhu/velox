//! 无样式单选按钮原语 `Radio` 与单选按钮组 `RadioGroup`（支持方向键与 `Home`/`End` 循环切换；
//! 默认透明底 + 中性边框与选中点，不注入主题蓝填充与悬停渐变）。

use crate::traits::{ComponentSize, Disableable, Sizable};
use velox_core::prelude::*;

/// 传递给 `Radio::indicator(...)` 插槽回调的单选框状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RadioState {
    pub checked: bool,
    pub disabled: bool,
}

impl RadioState {
    #[inline(always)] pub const fn is_checked(&self) -> bool { self.checked }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type RadioIndicatorRenderer<'a> = &'a dyn Fn(Div<'a>, RadioState, &'a FrameScope) -> Element<'a>;

/// 无样式单个单选按钮行为原语（`Radio`）。
pub struct Radio<'a> {
    pub id: u32,
    pub label: &'a str,
    pub checked: bool,
    pub disabled: bool,
    pub size: ComponentSize,
    pub style: Style,
    pub style_ext: StyleExt,
    pub accent_color: Color,
    pub unchecked_bg: Color,
    pub border_color: Color,
    pub dot_color: Color,
    pub text_color: Color,
    pub disabled_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub fill_on_checked: bool,
    pub on_select: Option<&'a dyn Fn(&mut ActionScope)>,
    pub(crate) on_key_nav: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    indicator_renderer: Option<RadioIndicatorRenderer<'a>>,
}

impl<'a> Radio<'a> {
    /// 创建一个无样式单选按钮原语。
    #[inline(always)]
    pub const fn new(id: u32, label: &'a str, checked: bool) -> Self {
        Self {
            id,
            label,
            checked,
            disabled: false,
            size: ComponentSize::Medium,
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
            accent_color: Color::TRANSPARENT,
            unchecked_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            dot_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            disabled_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            fill_on_checked: false,
            on_select: None,
            on_key_nav: None,
            indicator_renderer: None,
        }
    }

    /// 绑定选中回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格单选圆点指示器插槽回调（零堆分配）。
    #[inline]
    pub fn indicator<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, RadioState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: RadioState, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.indicator_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Radio<'a>);
crate::impl_base_sizable!(Radio<'a>);
crate::impl_base_disableable!(Radio<'a>);

impl<'a> Widget<'a> for Radio<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let accent_color = if self.accent_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.accent_color
        };
        let border_color = if self.border_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.border_color
        };
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let disabled_color = if self.disabled_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.disabled_color
        };

        let (outer_sz, dot_sz, font_sz) = match self.size {
            ComponentSize::Small => (14i16, 6i16, 12u8),
            ComponentSize::Medium => (16i16, 6i16, 13u8),
            ComponentSize::Large => (20i16, 8i16, 14u8),
        };

        let (circle_bg, ring_color, dot_color) = if self.disabled {
            (
                if self.checked && self.fill_on_checked {
                    disabled_color
                } else {
                    self.unchecked_bg
                },
                disabled_color,
                if self.dot_color != Color::TRANSPARENT {
                    self.dot_color
                } else {
                    disabled_color
                },
            )
        } else if self.checked {
            let bg = if self.fill_on_checked {
                accent_color
            } else {
                self.unchecked_bg
            };
            let dot = if self.dot_color != Color::TRANSPARENT {
                self.dot_color
            } else {
                accent_color
            };
            (bg, accent_color, dot)
        } else {
            (self.unchecked_bg, border_color, Color::TRANSPARENT)
        };

        let circle_el = if let Some(ind_fn) = self.indicator_renderer {
            ind_fn(
                div(),
                RadioState {
                    checked: self.checked,
                    disabled: self.disabled,
                },
                cx,
            )
        } else {
            let mut circle = div()
                .w(outer_sz)
                .h(outer_sz)
                .rounded((outer_sz / 2) as u8)
                .bg(circle_bg)
                .border(1, ring_color)
                .items_center()
                .justify_center();

            if !self.disabled {
                if let Some(h) = self.hover_bg {
                    circle = circle.hover_bg(h);
                }
                if let Some(a) = self.active_bg {
                    circle = circle.active_bg(a);
                }
            }

            if self.checked {
                circle = circle.child(
                    cx,
                    div()
                        .w(dot_sz)
                        .h(dot_sz)
                        .rounded((dot_sz / 2) as u8)
                        .bg(dot_color),
                );
            }
            circle.into_element(cx)
        };

        let mut row = div()
            .id(self.id)
            .flex_row()
            .items_center()
            .gap(8)
            .child(cx, circle_el);

        if !self.label.is_empty() {
            let lbl_col = if self.disabled {
                disabled_color
            } else {
                self.style_ext.text_color.unwrap_or(text_color)
            };
            let eff_fs = self.style_ext.text_size.unwrap_or(font_sz);
            row = row.child(cx, text(self.label).size(eff_fs).color(lbl_col));
        }

        if self.disabled {
            row = row.opacity(115).cursor(CursorShape::NotAllowed);
        } else {
            row = row.focusable().no_focus_border();
            if let Some(cb) = self.on_select {
                row = row.on_click_raw(cb);
            }
            if let Some(nav_cb) = self.on_key_nav {
                row = row.on_key_nav(nav_cb);
            }
        }

        self.style_ext.apply_to_div(row, self.style).into_element(cx)
    }
}

/// 传递给 `RadioGroup::item(...)` 插槽回调的单选项状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RadioItemState<'a> {
    pub index: usize,
    pub label: &'a str,
    pub selected: bool,
    pub disabled: bool,
}

impl<'a> RadioItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn label(&self) -> &'a str { self.label }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.selected }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type RadioItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, RadioItemState<'a>, &'a FrameScope) -> Element<'a>;
type RadioLabelFormatter<'a> = &'a dyn Fn(usize, &'a str) -> StrRef<'a>;

/// 无样式单选按钮组原语（`RadioGroup`，支持键盘方向键与 `Home`/`End` 循环切换及 `gpui-base` 插槽回调）。
pub struct RadioGroup<'a> {
    pub id: u32,
    pub options: &'a [&'a str],
    pub selected_index: Option<usize>,
    pub horizontal: bool,
    pub disabled: bool,
    pub size: ComponentSize,
    pub style: Style,
    pub style_ext: StyleExt,
    pub accent_color: Color,
    pub unchecked_bg: Color,
    pub border_color: Color,
    pub dot_color: Color,
    pub text_color: Color,
    pub disabled_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub checked_hover_bg: Option<Color>,
    pub checked_active_bg: Option<Color>,
    pub fill_on_checked: bool,
    pub on_change: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<RadioItemRenderer<'a>>,
    label_formatter: Option<RadioLabelFormatter<'a>>,
}

impl<'a> RadioGroup<'a> {
    /// 创建一个无样式单选按钮组原语。
    #[inline(always)]
    pub const fn new(id: u32, options: &'a [&'a str], selected_index: Option<usize>) -> Self {
        Self {
            id,
            options,
            selected_index,
            horizontal: false,
            disabled: false,
            size: ComponentSize::Medium,
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
            accent_color: Color::TRANSPARENT,
            unchecked_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            dot_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            disabled_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            checked_hover_bg: None,
            checked_active_bg: None,
            fill_on_checked: false,
            on_change: None,
            item_renderer: None,
            label_formatter: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，切换选项时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, options: &'a [&'a str], sig: Signal<usize>) -> Self {
        let cb = cx.arena().alloc(move |idx: usize, _: &mut ActionScope| {
            sig.set_if_changed(idx);
        });
        Self::new(id, options, Some(sig.get())).on_change(cb)
    }

    /// 设置为水平排列。
    #[inline(always)]
    pub const fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    /// 设置为垂直排列。
    #[inline(always)]
    pub const fn vertical(mut self) -> Self {
        self.horizontal = false;
        self
    }

    /// 绑定选中项索引变更回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// `gpui-base` 风格单选项插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, RadioItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |item: Div<'a>, state: RadioItemState<'a>, frame_cx: &'a FrameScope| {
                f(item, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格标签文案格式化回调。
    #[inline]
    pub fn label<F, S>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(usize, &'a str) -> S + 'a,
        S: Into<StrRef<'a>>,
    {
        let cb = cx
            .arena()
            .alloc(move |idx: usize, lbl: &'a str| f(idx, lbl).into());
        self.label_formatter = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(RadioGroup<'a>);
crate::impl_base_sizable!(RadioGroup<'a>);
crate::impl_base_disableable!(RadioGroup<'a>);

impl<'a> Widget<'a> for RadioGroup<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let mut container = div().id(self.id);
        container.style = self.style;
        if container.style.gap == 0 {
            container = container.gap(if self.horizontal { 14 } else { 8 });
        }
        container = if self.horizontal {
            container.flex_row().items_center()
        } else {
            container.flex_col()
        };
        if self.item_renderer.is_some() {
            container = container.text_color(cx.theme().foreground).text_size(13);
        }
        container = self.style_ext.apply_ext_only(container);

        let len = self.options.len();
        let cur_sel = self.selected_index.unwrap_or(0);
        let on_change = self.on_change;
        let group_nav: Option<&'a dyn Fn(KeyCode, &mut ActionScope)> =
            if !self.disabled && len > 0 {
                on_change.map(|cb| {
                    let h = cx.arena().alloc(move |key: KeyCode, scope: &mut ActionScope| {
                        let next = match key {
                            KeyCode::Left | KeyCode::Up => (cur_sel + len - 1) % len,
                            KeyCode::Right | KeyCode::Down => (cur_sel + 1) % len,
                            KeyCode::Home => 0,
                            KeyCode::End => len - 1,
                            KeyCode::Enter | KeyCode::Space => cur_sel,
                            _ => return,
                        };
                        cb(next, scope);
                    });
                    h as &'a dyn Fn(KeyCode, &mut ActionScope)
                })
            } else {
                None
            };

        for (idx, &raw_label) in self.options.iter().enumerate() {
            let checked = self.selected_index == Some(idx);
            let disp_label = match self.label_formatter {
                Some(f) => f(idx, raw_label),
                None => StrRef::from(raw_label),
            };
            if let Some(render_fn) = self.item_renderer {
                let mut slot = div()
                    .id(crate::scoped_child_id(self.id, 0xAE40_1EB4, idx as u32 + 1))
                    .child(cx, text(disp_label));
                if self.disabled {
                    slot = slot.cursor(CursorShape::NotAllowed);
                } else {
                    slot = slot.focusable().no_focus_border();
                    if let Some(nav_cb) = group_nav {
                        slot = slot.on_key_nav(nav_cb);
                    }
                    if let Some(cb) = on_change {
                        let item_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(idx, scope);
                        });
                        slot = slot.on_click_raw(item_cb);
                    }
                }
                let el = render_fn(
                    slot,
                    RadioItemState {
                        index: idx,
                        label: raw_label,
                        selected: checked,
                        disabled: self.disabled,
                    },
                    cx,
                );
                container = container.child(cx, el);
                continue;
            }
            let mut item = Radio::new(
                crate::scoped_child_id(self.id, 0xAE40_1EB4, idx as u32 + 1),
                disp_label.as_str(),
                checked,
            )
            .with_size(self.size)
            .disabled(self.disabled);
            item.accent_color = self.accent_color;
            item.unchecked_bg = self.unchecked_bg;
            item.border_color = self.border_color;
            item.dot_color = self.dot_color;
            item.text_color = self.text_color;
            item.disabled_color = self.disabled_color;
            item.fill_on_checked = self.fill_on_checked;
            item.hover_bg = if checked {
                self.checked_hover_bg.or(self.hover_bg)
            } else {
                self.hover_bg
            };
            item.active_bg = if checked {
                self.checked_active_bg.or(self.active_bg)
            } else {
                self.active_bg
            };
            item.on_key_nav = group_nav;

            if !self.disabled {
                if let Some(cb) = on_change {
                    let item_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    item = item.on_select(item_cb);
                }
            }
            container = container.child(cx, item);
        }

        container.into_element(cx)
    }
}
