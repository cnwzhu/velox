//! 无样式双态切换按钮原语 `Toggle` 与切换按钮组 `ToggleGroup`（默认零背景、零边框、零圆角、零悬停渐变，
//! 专注切换状态、焦点与禁用交互，完整支持 `Styled` 链式样式与外部主题层注入）。

use crate::traits::Disableable;
use velox_engine::prelude::*;

/// 无样式双态切换按钮原语（`Toggle`）。
pub struct Toggle<'a> {
    pub id: u32,
    pub label: &'a str,
    pub checked: bool,
    pub disabled: bool,
    pub font_size: u8,
    pub active_bg: Color,
    pub inactive_bg: Color,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub border_color: Color,
    pub border_width: u8,
    pub hover_bg: Option<Color>,
    pub active_press_bg: Option<Color>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub custom_child: Option<Element<'a>>,
    pub on_change: Option<&'a dyn Fn(bool, &mut ActionScope)>,
}

impl<'a> Toggle<'a> {
    /// 创建一个无样式双态切换按钮原语。
    #[inline(always)]
    pub fn new(id: u32, label: &'a str, checked: bool) -> Self {
        let style = crate::base_style! { row, items_center, justify_center };
        Self {
            id,
            label,
            checked,
            disabled: false,
            font_size: 0,
            active_bg: Color::TRANSPARENT,
            inactive_bg: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            inactive_fg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            border_width: 0,
            hover_bg: None,
            active_press_bg: None,
            focus_ring: false,
            rounded: 0,
            style,
            style_ext: StyleExt::DEFAULT,
            custom_child: None,
            on_change: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，切换时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, label: &'a str, sig: Signal<bool>) -> Self {
        let cb = cx.arena().alloc(move |v: bool, _: &mut ActionScope| {
            sig.set_if_changed(v);
        });
        Self::new(id, label, sig.get()).on_change(cb)
    }

    /// 自定义按钮内部子元素内容（对齐 `gpui-kit` `.child(...)` 写法）。
    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.custom_child = Some(child.into_element(cx));
        self
    }

    /// 设置字体大小（默认 `13`）。
    #[inline(always)]
    pub const fn font_size(mut self, size: u8) -> Self {
        self.font_size = size;
        self
    }

    /// 设置按下/未按下背景色与前景色。
    #[inline(always)]
    pub const fn colors(
        mut self,
        active_bg: Color,
        inactive_bg: Color,
        active_fg: Color,
        inactive_fg: Color,
    ) -> Self {
        self.active_bg = active_bg;
        self.inactive_bg = inactive_bg;
        self.active_fg = active_fg;
        self.inactive_fg = inactive_fg;
        self
    }

    /// 设置悬停态背景色。
    #[inline(always)]
    pub const fn hover_bg(mut self, color: Color) -> Self {
        self.hover_bg = Some(color);
        self
    }

    /// 设置按压态背景色。
    #[inline(always)]
    pub const fn active_press_bg(mut self, color: Color) -> Self {
        self.active_press_bg = Some(color);
        self
    }

    /// 绑定切换状态回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(bool, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Toggle<'a>);
crate::impl_base_disableable!(Toggle<'a>);

impl<'a> Widget<'a> for Toggle<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = if self.checked {
            if self.active_bg != Color::TRANSPARENT {
                self.active_bg
            } else {
                self.style.bg
            }
        } else if self.inactive_bg != Color::TRANSPARENT {
            self.inactive_bg
        } else {
            self.style.bg
        };
        let fg = if self.checked {
            if self.active_fg != Color::TRANSPARENT {
                self.active_fg
            } else {
                def_fg
            }
        } else if self.inactive_fg != Color::TRANSPARENT {
            self.inactive_fg
        } else {
            def_fg
        };
        let font_sz = if self.font_size == 0 {
            13
        } else {
            self.font_size
        };

        let mut btn = div().id(self.id);
        btn.style = self.style;
        btn.style.bg = bg;
        if self.rounded > 0 && btn.style.rounded_radius() == 0 {
            btn.style.set_rounded(self.rounded);
        }
        if btn.style.border_px() == 0 && (self.border_width > 0 || self.border_color != Color::TRANSPARENT) {
            let bw = if self.border_width > 0 { self.border_width } else { 1 };
            btn.style.set_border_px(bw);
            btn.style.border_color = self.border_color;
        }

        let eff_fg = self.style_ext.text_color.unwrap_or(fg);
        let eff_fs = self.style_ext.text_size.unwrap_or(font_sz);
        if let Some(custom) = self.custom_child {
            btn = btn.text_color(eff_fg).text_size(eff_fs).child(cx, custom);
        } else {
            btn = btn.child(
                cx,
                text(self.label)
                    .size(eff_fs)
                    .color(eff_fg)
                    .text_center()
                    .nowrap(),
            );
        }

        if !self.disabled {
            btn = btn.focusable();
            if !self.focus_ring {
                btn = btn.no_focus_border();
            }
            if let Some(h) = self.hover_bg {
                btn = btn.hover_bg(h);
            }
            if let Some(a) = self.active_press_bg {
                btn = btn.active_bg(a);
            }
            btn = self.style_ext.apply_ext_only(btn);
            if let Some(cb) = self.on_change {
                let next_state = !self.checked;
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(next_state, scope);
                });
                btn = btn.on_click_raw(click_cb);
            }
        } else {
            let mut safe_ext = self.style_ext;
            safe_ext.hover_style = None;
            safe_ext.active_style = None;
            safe_ext.focus_style = None;
            btn = safe_ext.apply_ext_only(btn.opacity(115).cursor(CursorShape::NotAllowed));
        }

        btn.into_element(cx)
    }
}

/// 传递给 `ToggleGroup::item(...)` 插槽回调的按钮状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ToggleItemState<'a> {
    pub index: usize,
    pub label: &'a str,
    pub active: bool,
    pub disabled: bool,
}

impl<'a> ToggleItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn label(&self) -> &'a str { self.label }
    #[inline(always)] pub const fn is_active(&self) -> bool { self.active }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type ToggleItemRenderer<'a> =
    &'a dyn Fn(Div<'a>, ToggleItemState<'a>, &'a FrameScope) -> Element<'a>;
type ToggleLabelFormatter<'a> = &'a dyn Fn(usize, &'a str) -> StrRef<'a>;

/// 无样式多项切换按钮组原语（`ToggleGroup`，支持 `gpui-base` 插槽回调与外层 `Styled`）。
pub struct ToggleGroup<'a> {
    pub id: u32,
    pub labels: &'a [&'a str],
    pub checked_mask: u32,
    pub multiple: bool,
    pub disabled: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub item_height: i16,
    pub item_px: i8,
    pub font_size: u8,
    pub active_bg: Color,
    pub inactive_bg: Color,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub border_color: Color,
    pub border_width: u8,
    pub hover_bg: Option<Color>,
    pub active_press_bg: Option<Color>,
    pub focus_ring: bool,
    pub rounded: u8,
    pub gap: i8,
    pub on_toggle: Option<&'a dyn Fn(usize, bool, &mut ActionScope)>,
    item_renderer: Option<ToggleItemRenderer<'a>>,
    label_formatter: Option<ToggleLabelFormatter<'a>>,
}

impl<'a> ToggleGroup<'a> {
    /// 创建一个无样式切换按钮组原语（`checked_mask` 的第 `i` 位代表第 `i` 项是否按下）。
    #[inline(always)]
    pub const fn new(id: u32, labels: &'a [&'a str], checked_mask: u32) -> Self {
        Self {
            id,
            labels,
            checked_mask,
            multiple: false,
            disabled: false,
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
            item_height: 0,
            item_px: 0,
            font_size: 0,
            active_bg: Color::TRANSPARENT,
            inactive_bg: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            inactive_fg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            border_width: 0,
            hover_bg: None,
            active_press_bg: None,
            focus_ring: false,
            rounded: 0,
            gap: 4,
            on_toggle: None,
            item_renderer: None,
            label_formatter: None,
        }
    }

    /// 允许同时选中多项。
    #[inline(always)]
    pub const fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    /// 绑定某一项切换回调 `(index, new_checked, &mut ActionScope)`。
    #[inline(always)]
    pub fn on_toggle(mut self, handler: &'a dyn Fn(usize, bool, &mut ActionScope)) -> Self {
        self.on_toggle = Some(handler);
        self
    }

    /// `gpui-base` 风格切换项插槽回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, ToggleItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |item: Div<'a>, state: ToggleItemState<'a>, frame_cx: &'a FrameScope| {
                f(item, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// 自定义按钮文案格式化回调。
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

crate::impl_base_styled_box!(ToggleGroup<'a>);
crate::impl_base_disableable!(ToggleGroup<'a>);

impl<'a> Widget<'a> for ToggleGroup<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let mut row = div().id(self.id);
        row.style = self.style;
        if row.style.gap == 0 {
            row = row.gap(self.gap);
        }
        row = row.items_center();
        if self.item_renderer.is_some() {
            row = row
                .text_color(cx.theme().foreground)
                .text_size(if self.font_size == 0 { 13 } else { self.font_size });
        }
        row = self.style_ext.apply_ext_only(row);
        let on_toggle = self.on_toggle;

        for (idx, &raw_label) in self.labels.iter().enumerate() {
            let checked = (self.checked_mask & (1u32 << (idx as u32))) != 0;
            let disp_label = match self.label_formatter {
                Some(f) => f(idx, raw_label),
                None => StrRef::from(raw_label),
            };
            if let Some(render_fn) = self.item_renderer {
                let mut slot = div()
                    .id(crate::scoped_child_id(self.id, 0x4029_6205, idx as u32 + 1))
                    .child(cx, text(disp_label).text_center().nowrap());
                if self.disabled {
                    slot = slot.cursor(CursorShape::NotAllowed);
                } else {
                    slot = slot.focusable();
                    if !self.focus_ring {
                        slot = slot.no_focus_border();
                    }
                    if let Some(cb) = on_toggle {
                        let next_checked = !checked;
                        let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                            cb(idx, next_checked, scope);
                        });
                        slot = slot.on_click_raw(click_cb);
                    }
                }
                let el = render_fn(
                    slot,
                    ToggleItemState {
                        index: idx,
                        label: raw_label,
                        active: checked,
                        disabled: self.disabled,
                    },
                    cx,
                );
                row = row.child(cx, el);
                continue;
            }
            let mut item = Toggle::new(
                crate::scoped_child_id(self.id, 0x4029_6205, idx as u32 + 1),
                disp_label.as_str(),
                checked,
            )
            .disabled(self.disabled);
            if self.item_height > 0 {
                item = item.h(self.item_height);
            }
            if self.item_px > 0 {
                item = item.px(self.item_px);
            }
            item.font_size = self.font_size;
            item.active_bg = self.active_bg;
            item.inactive_bg = self.inactive_bg;
            item.active_fg = self.active_fg;
            item.inactive_fg = self.inactive_fg;
            item.border_color = self.border_color;
            item.border_width = self.border_width;
            item.hover_bg = self.hover_bg;
            item.active_press_bg = self.active_press_bg;
            item.focus_ring = self.focus_ring;
            item.rounded = self.rounded;
            if !self.disabled {
                if let Some(cb) = on_toggle {
                    let item_cb = cx.arena().alloc(move |next: bool, scope: &mut ActionScope| {
                        cb(idx, next, scope);
                    });
                    item = item.on_change(item_cb);
                }
            }
            row = row.child(cx, item);
        }

        row.into_element(cx)
    }
}
