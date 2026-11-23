//! 无样式复选框行为原语 `Checkbox`（支持选中/未选中/半选三态、原生矢量 SVG 对勾与横杠、
//! 键盘 `Enter`/`Space` 切换与尺寸控制；默认零圆角、零主题蓝填充、零悬停渐变与零动画）。

use crate::traits::{ComponentSize, Sizable};
use core::time::Duration;
use velox_engine::prelude::*;

const CHECKBOX_FLAG_COLORS: u8 = 1 << 0;
const CHECKBOX_FLAG_ROUNDED: u8 = 1 << 1;

const CHECK_SVG_PATH: &str = "M20 6 9 17l-5-5";
const MINUS_SVG_PATH: &str = "M5 12h14";

/// 传递给 `Checkbox::indicator(...)` / `Checkbox::label_slot(...)` 插槽回调的状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CheckboxState {
    pub checked: bool,
    pub indeterminate: bool,
    pub disabled: bool,
}

impl CheckboxState {
    #[inline(always)] pub const fn is_checked(&self) -> bool { self.checked }
    #[inline(always)] pub const fn is_indeterminate(&self) -> bool { self.indeterminate }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type CheckboxIndicatorRenderer<'a> =
    &'a dyn Fn(Div<'a>, CheckboxState, &'a FrameScope) -> Element<'a>;
type CheckboxLabelRenderer<'a> =
    &'a dyn Fn(StrRef<'a>, CheckboxState, &'a FrameScope) -> Element<'a>;

/// 无样式复选框行为原语（`Checkbox`）。
pub struct Checkbox<'a> {
    pub id: u32,
    pub checked: bool,
    pub indeterminate: bool,
    pub disabled: bool,
    pub focus_ring: bool,
    pub label: Option<StrRef<'a>>,
    pub size: ComponentSize,
    pub style: Style,
    pub style_ext: StyleExt,
    pub box_size: i16,
    pub box_rounded: u8,
    pub box_bg: Color,
    pub border_color: Color,
    pub check_color: Color,
    pub label_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub font_size: u8,
    pub animated: bool,
    pub on_change: Option<&'a dyn Fn(bool, &mut ActionScope)>,
    indicator_renderer: Option<CheckboxIndicatorRenderer<'a>>,
    label_renderer: Option<CheckboxLabelRenderer<'a>>,
    custom_flags: u8,
}

impl<'a> Checkbox<'a> {
    /// 创建一个无样式复选框原语（默认直角 `16×16` 透明框 + 中性前景色边框与对勾，不注入主题蓝与圆角）。
    #[inline(always)]
    pub const fn new(id: u32, checked: bool) -> Self {
        Self {
            id,
            checked,
            indeterminate: false,
            disabled: false,
            focus_ring: false,
            label: None,
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
            box_size: 16,
            box_rounded: 0,
            box_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            check_color: Color::TRANSPARENT,
            label_color: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            font_size: 13,
            animated: false,
            on_change: None,
            indicator_renderer: None,
            label_renderer: None,
            custom_flags: 0,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，点击时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<bool>) -> Self {
        let cb = cx.arena().alloc(move |v: bool, _: &mut ActionScope| {
            sig.set_if_changed(v);
        });
        Self::new(id, sig.get()).on_change(cb)
    }

    /// 设置半选态（`indeterminate`，渲染原生矢量横杠；点击后将转为选中态 `true`）。
    #[inline(always)]
    pub const fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        self
    }

    /// 启用或禁用键盘聚焦时的外围焦点环（无样式原语默认关闭）。
    #[inline(always)]
    pub const fn focus_ring(mut self, enabled: bool) -> Self {
        self.focus_ring = enabled;
        self
    }

    /// 设置右侧描述标签文本。
    #[inline(always)]
    pub fn label(mut self, text: impl Into<StrRef<'a>>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// 设置勾选框背景色、边框色、内部标记色与右侧标签色。
    #[inline(always)]
    pub const fn colors(
        mut self,
        box_bg: Color,
        border_color: Color,
        check_color: Color,
        label_color: Color,
    ) -> Self {
        self.box_bg = box_bg;
        self.border_color = border_color;
        self.check_color = check_color;
        self.label_color = label_color;
        self.custom_flags |= CHECKBOX_FLAG_COLORS;
        self
    }

    /// 设置勾选框圆角半径。
    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.box_rounded = radius;
        self.custom_flags |= CHECKBOX_FLAG_ROUNDED;
        self
    }

    /// 启用或禁用颜色过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 设置禁用态。
    #[inline(always)]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 绑定勾选状态切换回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(bool, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// `gpui-base` 风格勾选框指示器插槽回调（零堆分配）。
    #[inline]
    pub fn indicator<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, CheckboxState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |box_div: Div<'a>, state: CheckboxState, frame_cx: &'a FrameScope| {
                f(box_div, state).into_element(frame_cx)
            },
        );
        self.indicator_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格自定义标签插槽回调（零堆分配）。
    #[inline]
    pub fn label_slot<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(StrRef<'a>, CheckboxState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |lbl: StrRef<'a>, state: CheckboxState, frame_cx: &'a FrameScope| {
                f(lbl, state).into_element(frame_cx)
            },
        );
        self.label_renderer = Some(cb);
        self
    }

    /// 是否已由调用方显式指定过颜色组。
    #[inline(always)]
    pub const fn has_custom_colors(&self) -> bool {
        (self.custom_flags & CHECKBOX_FLAG_COLORS) != 0
    }

    /// 是否已由调用方显式指定过圆角。
    #[inline(always)]
    pub const fn has_custom_rounded(&self) -> bool {
        (self.custom_flags & CHECKBOX_FLAG_ROUNDED) != 0
    }
}

crate::impl_base_styled_box!(Checkbox<'a>);
crate::impl_base_disableable!(Checkbox<'a>);

impl<'a> Sizable for Checkbox<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        let (box_sz, font_sz) = match size {
            ComponentSize::Small => (14i16, 12u8),
            ComponentSize::Medium => (16i16, 13u8),
            ComponentSize::Large => (20i16, 14u8),
        };
        self.box_size = box_sz;
        self.font_size = font_sz;
        self
    }
}

impl<'a> Widget<'a> for Checkbox<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let has_colors = (self.custom_flags & CHECKBOX_FLAG_COLORS) != 0;

        let box_bg = self.box_bg;
        let border_color = if has_colors || self.border_color != Color::TRANSPARENT {
            self.border_color
        } else {
            def_fg
        };
        let check_color = if has_colors || self.check_color != Color::TRANSPARENT {
            self.check_color
        } else {
            def_fg
        };
        let label_color = if has_colors || self.label_color != Color::TRANSPARENT {
            self.label_color
        } else {
            def_fg
        };
        let box_rounded = self.box_rounded;

        let mut row = div().id(self.id).flex_row().items_center().gap(8);

        if self.disabled {
            row = row.opacity(115).cursor(CursorShape::NotAllowed);
        } else {
            row = row.focusable();
            if self.focus_ring && self.label.is_none() {
                row = row.rounded(box_rounded);
            } else {
                row = row.no_focus_border();
            }
            if let Some(cb) = self.on_change {
                let next_state = if self.indeterminate {
                    true
                } else {
                    !self.checked
                };
                let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    cb(next_state, acx);
                });
                row = row.on_click_raw(raw_cb);
            }
        }

        let state = CheckboxState {
            checked: self.checked,
            indeterminate: self.indeterminate,
            disabled: self.disabled,
        };

        let box_el = if let Some(ind_fn) = self.indicator_renderer {
            let slot = div().id(self.id ^ 0x4000_0000);
            ind_fn(slot, state, cx)
        } else {
            let mut box_div = div()
                .id(self.id ^ 0x4000_0000)
                .w(self.box_size)
                .h(self.box_size)
                .rounded(box_rounded)
                .items_center()
                .justify_center()
                .bg(box_bg);
            if border_color != Color::TRANSPARENT {
                box_div = box_div.border(1, border_color);
            }

            if !self.disabled {
                if let Some(h) = self.hover_bg {
                    box_div = box_div.hover_bg(h);
                }
                if let Some(a) = self.active_bg {
                    box_div = box_div.active_bg(a);
                }
            }

            if self.animated {
                box_div = box_div.animate_color(Duration::from_millis(140));
            }

            if self.indeterminate {
                let mark_sz = (self.box_size - 4).max(8);
                box_div = box_div.child(
                    cx,
                    svg_icon(mark_sz, mark_sz, MINUS_SVG_PATH)
                        .tint(check_color)
                        .stroke_width(28),
                );
            } else if self.checked {
                let mark_sz = (self.box_size - 4).max(8);
                box_div = box_div.child(
                    cx,
                    svg_icon(mark_sz, mark_sz, CHECK_SVG_PATH)
                        .tint(check_color)
                        .stroke_width(28),
                );
            }
            box_div.into_element(cx)
        };

        row = row.child(cx, box_el);
        if let Some(lbl) = self.label {
            if let Some(lbl_fn) = self.label_renderer {
                row = row.child(cx, lbl_fn(lbl, state, cx));
            } else {
                let eff_fs = self.style_ext.text_size.unwrap_or(self.font_size);
                let eff_col = self.style_ext.text_color.unwrap_or(label_color);
                row = row.child(cx, text(lbl).size(eff_fs).color(eff_col));
            }
        }

        self.style_ext.apply_to_div(row, self.style).into_element(cx)
    }
}
