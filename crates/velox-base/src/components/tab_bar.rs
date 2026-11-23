//! 移动端底部标签栏基础组件 `TabBar`（默认无样式，支持多页签切换与底部 Home Indicator 安全区域自动避让）。

use core::time::Duration;
use velox_core::prelude::*;

/// 面向 [`TabBar::item`] 插槽回调的底部标签项状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabBarItemState {
    pub index: usize,
    pub is_active: bool,
}

impl TabBarItemState {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn is_active(&self) -> bool { self.is_active }
}

/// 移动端底部标签栏基础组件（`TabBar`，默认无样式）。
pub struct TabBar<'a> {
    pub id: u32,
    pub labels: &'a [&'a str],
    pub active: usize,
    pub content_height: i16,
    pub safe_bottom: bool,
    pub bg: Color,
    pub border_color: Color,
    pub active_pill_bg: Color,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub item_rounded: u8,
    pub animated: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub item_renderer:
        Option<&'a dyn Fn(Div<'a>, TabBarItemState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> TabBar<'a> {
    /// 创建一个默认无样式的底部标签栏（默认开启底部 `safe_area.bottom` 自动避让）。
    #[inline(always)]
    pub const fn new(id: u32, labels: &'a [&'a str], active: usize) -> Self {
        Self {
            id,
            labels,
            active,
            content_height: 48,
            safe_bottom: true,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_pill_bg: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            inactive_fg: Color::TRANSPARENT,
            item_rounded: 0,
            animated: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_select: None,
            item_renderer: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，切换底部标签时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, labels: &'a [&'a str], sig: Signal<usize>) -> Self {
        let cb = cx.arena().alloc(move |idx: usize, _: &mut ActionScope| {
            sig.set_if_changed(idx);
        });
        Self::new(id, labels, sig.get()).on_select(cb)
    }

    /// 设置是否自动叠加移动端底部 `safe_area.bottom` 安全边距（默认 `true`）。
    #[inline(always)]
    pub const fn safe_bottom(mut self, enabled: bool) -> Self {
        self.safe_bottom = enabled;
        self
    }

    /// 设置标签栏内容区高度（不含底部安全区，默认 `48px`）。
    #[inline(always)]
    pub const fn h(mut self, height: i16) -> Self {
        self.content_height = if height < 32 { 32 } else { height };
        self
    }

    /// 绑定底部标签切换回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// `gpui-base` 风格底部标签项插槽回调：自定义每一枚底部 Tab 样式。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TabBarItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: TabBarItemState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(TabBar<'a>);

impl<'a> Widget<'a> for TabBar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let active_fg = if self.active_fg.a() > 0 {
            self.active_fg
        } else {
            fg
        };
        let inactive_fg = if self.inactive_fg.a() > 0 {
            self.inactive_fg
        } else {
            fg.scale_alpha(150)
        };

        let bottom_pad = if self.safe_bottom {
            cx.safe_area().bottom.max(0)
        } else {
            0
        };
        let total_h = self.content_height.saturating_add(bottom_pad);

        let mut bar = div()
            .id(self.id)
            .flex_row()
            .w_full()
            .h(total_h)
            .pb(bottom_pad.clamp(0, 127) as i8)
            .px(8)
            .gap(6)
            .items_center()
            .bg(self.bg);
        if self.border_color.a() > 0 {
            bar = bar.border(1, self.border_color);
        }

        for (idx, &label) in self.labels.iter().enumerate() {
            let is_active = idx == self.active;
            if let Some(renderer) = self.item_renderer {
                let mut item = div()
                    .id(crate::scoped_child_id(self.id, 0x2AFD_A046, (idx as u32) + 1))
                    .flex_col()
                    .flex_grow(1)
                    .items_center()
                    .justify_center()
                    .child(cx, text(label));
                if let Some(cb) = self.on_select {
                    let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                        cb(idx, acx);
                    });
                    item = item.on_click_raw(raw_cb);
                }
                let state = TabBarItemState {
                    index: idx,
                    is_active,
                };
                bar = bar.child(cx, renderer(item, state, cx));
                continue;
            }
            let item_bg = if is_active {
                self.active_pill_bg
            } else {
                Color::TRANSPARENT
            };
            let item_fg = if is_active { active_fg } else { inactive_fg };

            let mut item = div()
                .id(crate::scoped_child_id(self.id, 0x2AFD_A046, (idx as u32) + 1))
                .flex_col()
                .flex_grow(1)
                .h((self.content_height - 8).max(24))
                .items_center()
                .justify_center()
                .rounded(self.item_rounded)
                .bg(item_bg);

            if self.animated {
                item = item.animate_color(Duration::from_millis(160));
            }

            if let Some(cb) = self.on_select {
                let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    cb(idx, acx);
                });
                item = item.on_click_raw(raw_cb);
            }

            bar = bar.child(cx, item.child(cx, text(label).size(13).color(item_fg)));
        }

        bar = self.style_ext.apply_to_div(bar, self.style);
        bar.into_element(cx)
    }
}
