//! 无样式页签导航栏行为原语 `Tabs`（支持点击与方向键/`Home`/`End` 键盘漫游切换；
//! 默认零背景槽、零边框、零圆角、零阴影与零动画，三种主题变体外观由 `velox-component::Tabs` 注入）。

use crate::traits::{ComponentSize, Sizable};
use core::time::Duration;
use velox_engine::prelude::*;

const FLAG_CUSTOM_HEIGHT: u8 = 1 << 0;
const FLAG_CUSTOM_APPEARANCE: u8 = 1 << 1;

/// 页签导航栏视觉变体标记（在 `velox-component::Tabs` 中映射为对应主题外观）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum TabsVariant {
    #[default]
    Segmented,
    Underline,
    Pill,
}

/// 传递给 `Tabs::item(...)` 插槽渲染回调的页签状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TabItemState<'a> {
    pub index: usize,
    pub label: &'a str,
    pub active: bool,
}

impl<'a> TabItemState<'a> {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn label(&self) -> &'a str { self.label }
    #[inline(always)] pub const fn is_active(&self) -> bool { self.active }
}

type TabItemRenderer<'a> = &'a dyn Fn(Div<'a>, TabItemState<'a>, &'a FrameScope) -> Element<'a>;
type TabLabelFormatter<'a> = &'a dyn Fn(usize, &'a str) -> StrRef<'a>;

/// 无样式页签导航行为原语（`Tabs`）。
pub struct Tabs<'a> {
    pub id: u32,
    pub labels: &'a [&'a str],
    pub active: usize,
    pub variant: TabsVariant,
    pub size: ComponentSize,
    pub height: i16,
    pub style: Style,
    pub style_ext: StyleExt,
    pub bar_bg: Color,
    pub bar_border_color: Color,
    pub bar_pad: i8,
    pub active_bg: Color,
    pub inactive_bg: Color,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub hover_bg: Option<Color>,
    pub active_press_bg: Option<Color>,
    pub active_shadow: Option<(u8, i8, i8, Color)>,
    pub underline_active_color: Color,
    pub underline_inactive_color: Color,
    pub bar_rounded: u8,
    pub item_rounded: u8,
    pub focus_ring: bool,
    pub animated: bool,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    item_renderer: Option<TabItemRenderer<'a>>,
    label_formatter: Option<TabLabelFormatter<'a>>,
    custom_flags: u8,
}

impl<'a> Tabs<'a> {
    /// 创建一个无样式页签导航原语（默认透明底、零圆角、零阴影与零动画）。
    #[inline(always)]
    pub const fn new(id: u32, labels: &'a [&'a str], active: usize) -> Self {
        Self {
            id,
            labels,
            active,
            variant: TabsVariant::Segmented,
            size: ComponentSize::Medium,
            height: 32,
            style: Style {
                width: Length::Full,
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
            bar_bg: Color::TRANSPARENT,
            bar_border_color: Color::TRANSPARENT,
            bar_pad: 0,
            active_bg: Color::TRANSPARENT,
            inactive_bg: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            inactive_fg: Color::TRANSPARENT,
            hover_bg: None,
            active_press_bg: None,
            active_shadow: None,
            underline_active_color: Color::TRANSPARENT,
            underline_inactive_color: Color::TRANSPARENT,
            bar_rounded: 0,
            item_rounded: 0,
            focus_ring: false,
            animated: false,
            on_select: None,
            item_renderer: None,
            label_formatter: None,
            custom_flags: 0,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，切换页签时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, labels: &'a [&'a str], sig: Signal<usize>) -> Self {
        let cb = cx.arena().alloc(move |idx: usize, _: &mut ActionScope| {
            sig.set_if_changed(idx);
        });
        Self::new(id, labels, sig.get()).on_select(cb)
    }

    /// 设置页签栏视觉变体标记。
    #[inline(always)]
    pub const fn variant(mut self, variant: TabsVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 快捷切换为下划线页签风格标记。
    #[inline(always)]
    pub const fn underline(mut self) -> Self {
        self.variant = TabsVariant::Underline;
        self
    }

    /// 快捷切换为胶囊页签风格标记。
    #[inline(always)]
    pub const fn pill(mut self) -> Self {
        self.variant = TabsVariant::Pill;
        self
    }

    /// 快捷切换为分段槽页签风格标记。
    #[inline(always)]
    pub const fn segmented(mut self) -> Self {
        self.variant = TabsVariant::Segmented;
        self
    }

    /// 设置导航栏高度（像素）。
    #[inline(always)]
    pub const fn h(mut self, height: i16) -> Self {
        self.height = height;
        self.custom_flags |= FLAG_CUSTOM_HEIGHT;
        self
    }

    /// 显式设置页签栏与页签项配色及圆角。
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub const fn appearance(
        mut self,
        bar_bg: Color,
        bar_border_color: Color,
        active_bg: Color,
        inactive_bg: Color,
        active_fg: Color,
        inactive_fg: Color,
        bar_rounded: u8,
        item_rounded: u8,
        animated: bool,
    ) -> Self {
        self.bar_bg = bar_bg;
        self.bar_border_color = bar_border_color;
        self.active_bg = active_bg;
        self.inactive_bg = inactive_bg;
        self.active_fg = active_fg;
        self.inactive_fg = inactive_fg;
        self.bar_rounded = bar_rounded;
        self.item_rounded = item_rounded;
        self.animated = animated;
        self.custom_flags |= FLAG_CUSTOM_APPEARANCE;
        self
    }

    /// 是否已显式调用过 `.appearance(...)`。
    #[inline(always)]
    pub const fn has_custom_appearance(&self) -> bool {
        (self.custom_flags & FLAG_CUSTOM_APPEARANCE) != 0
    }

    /// 是否已显式设置过高度。
    #[inline(always)]
    pub const fn has_custom_height(&self) -> bool {
        (self.custom_flags & FLAG_CUSTOM_HEIGHT) != 0
    }

    /// 绑定页签切换回调（传入选中的 `usize` 索引）。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格页签插槽渲染回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TabItemState<'a>) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |item: Div<'a>, state: TabItemState<'a>, frame_cx: &'a FrameScope| {
                f(item, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格页签文本格式化回调。
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

crate::impl_base_styled_box!(Tabs<'a>);

impl<'a> Sizable for Tabs<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl<'a> Widget<'a> for Tabs<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let font_sz = match self.size {
            ComponentSize::Small => 12u8,
            ComponentSize::Medium => 13u8,
            ComponentSize::Large => 14u8,
        };
        let height = if (self.custom_flags & FLAG_CUSTOM_HEIGHT) != 0 || self.height != 32 {
            self.height
        } else {
            match self.size {
                ComponentSize::Small => 28,
                ComponentSize::Medium => 32,
                ComponentSize::Large => 38,
            }
        };

        let active_fg = if self.active_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.active_fg
        };
        let inactive_fg = if self.inactive_fg == Color::TRANSPARENT {
            def_fg.scale_alpha(150)
        } else {
            self.inactive_fg
        };

        let len = self.labels.len();
        let cur_active = self.active;
        let nav_cb: Option<&'a dyn Fn(KeyCode, &mut ActionScope)> = if len > 0 {
            self.on_select.map(|cb| {
                let h = cx.arena().alloc(move |key: KeyCode, acx: &mut ActionScope| {
                    let next = match key {
                        KeyCode::Left | KeyCode::Up => (cur_active + len - 1) % len,
                        KeyCode::Right | KeyCode::Down => (cur_active + 1) % len,
                        KeyCode::Home => 0,
                        KeyCode::End => len - 1,
                        _ => return,
                    };
                    if next != cur_active {
                        cb(next, acx);
                    }
                });
                h as &'a dyn Fn(KeyCode, &mut ActionScope)
            })
        } else {
            None
        };

        let border_w = if self.bar_border_color.a() > 0 { 1 } else { 0 };
        let bar_pad = if self.bar_pad > 0 {
            self.bar_pad
        } else if (self.custom_flags & FLAG_CUSTOM_APPEARANCE) != 0 {
            3
        } else {
            0
        };

        let mut bar = div().id(self.id);
        bar.style = self.style;
        if matches!(bar.style.height, Length::Auto) {
            bar = bar.h(height);
        }
        if bar.style.padding == Edges::all(0) && bar_pad > 0 {
            bar = bar.p(bar_pad);
        }
        if bar.style.rounded_radius() == 0 && self.bar_rounded > 0 {
            bar = bar.rounded(self.bar_rounded);
        }
        if bar.style.bg == Color::TRANSPARENT && self.bar_bg != Color::TRANSPARENT {
            bar = bar.bg(self.bar_bg);
        }
        if bar.style.border_px() == 0 && border_w > 0 {
            bar = bar.border(border_w, self.bar_border_color);
        }
        if self.item_renderer.is_some() {
            bar = bar.text_color(active_fg).text_size(font_sz);
        }
        bar = self.style_ext.apply_ext_only(bar);

        for (idx, &raw_label) in self.labels.iter().enumerate() {
            let is_active = idx == self.active;
            let disp_label = match self.label_formatter {
                Some(f) => f(idx, raw_label),
                None => StrRef::from(raw_label),
            };

            let mut item = div()
                .id(crate::scoped_child_id(
                    self.id,
                    0x0C1D_CD2D,
                    (idx as u32) + 1,
                ))
                .focusable();
            if !self.focus_ring {
                item = item.no_focus_border();
            }
            if let Some(cb) = self.on_select {
                let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    cb(idx, acx);
                });
                item = item.on_click_raw(raw_cb);
            }
            if let Some(nav) = nav_cb {
                item = item.on_key_nav(nav);
            }

            if let Some(render_fn) = self.item_renderer {
                item = item.child(cx, text(disp_label).text_center().nowrap());
                let el = render_fn(
                    item,
                    TabItemState {
                        index: idx,
                        label: raw_label,
                        active: is_active,
                    },
                    cx,
                );
                bar = bar.child(cx, el);
                continue;
            }

            let tab_bg = if is_active {
                self.active_bg
            } else {
                self.inactive_bg
            };
            let tab_fg = if is_active { active_fg } else { inactive_fg };

            item = item
                .flex_col()
                .flex_grow(1)
                .h_full()
                .items_center()
                .justify_center()
                .rounded(self.item_rounded)
                .bg(tab_bg);

            if !is_active {
                if let Some(h) = self.hover_bg {
                    item = item.hover_bg(h);
                }
                if let Some(a) = self.active_press_bg {
                    item = item.active_bg(a);
                }
            } else if let Some((blur, ox, oy, col)) = self.active_shadow {
                item = item.shadow(blur, ox, oy, col);
            }

            if self.animated {
                {
                    item = item
                        .animate_color(Duration::from_millis(180))
                        .ease(Easing::Spring);
                }
            }

            item = item.child(
                cx,
                text(disp_label).size(font_sz).color(tab_fg).text_center().nowrap(),
            );

            let ind_col = if is_active {
                self.underline_active_color
            } else {
                self.underline_inactive_color
            };
            if ind_col != Color::TRANSPARENT {
                let indicator_h = if is_active { 2 } else { 1 };
                item = item.child(
                    cx,
                    div()
                        .pos(0, height - indicator_h)
                        .w_full()
                        .h(indicator_h)
                        .bg(ind_col),
                );
            }

            bar = bar.child(cx, item);
        }

        bar.into_element(cx)
    }
}
