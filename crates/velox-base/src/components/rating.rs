//! 无样式星级评分行为原语 `Rating`（默认使用中性前景色，不注入主题警告金黄色）。

use velox_engine::prelude::*;

/// 传递给 `Rating::item(...)` 插槽回调的单项评分状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RatingItemState {
    pub index: u8,
    pub filled: bool,
    pub readonly: bool,
}

impl RatingItemState {
    #[inline(always)] pub const fn index(&self) -> u8 { self.index }
    #[inline(always)] pub const fn is_filled(&self) -> bool { self.filled }
    #[inline(always)] pub const fn is_readonly(&self) -> bool { self.readonly }
}

type RatingItemRenderer<'a> = &'a dyn Fn(Div<'a>, RatingItemState, &'a FrameScope) -> Element<'a>;

/// 无样式交互式星级评分原语（`Rating`）。
pub struct Rating<'a> {
    pub id: u32,
    pub value: u8,
    pub max: u8,
    pub readonly: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub active_color: Color,
    pub inactive_color: Color,
    pub focus_ring: bool,
    pub on_change: Option<&'a dyn Fn(u8, &mut ActionScope)>,
    item_renderer: Option<RatingItemRenderer<'a>>,
}

impl<'a> Rating<'a> {
    /// 创建一个无样式星级评分原语（默认满分 `5` 星）。
    #[inline(always)]
    pub const fn new(id: u32, value: u8) -> Self {
        Self {
            id,
            value,
            max: 5,
            readonly: false,
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
            active_color: Color::TRANSPARENT,
            inactive_color: Color::TRANSPARENT,
            focus_ring: false,
            on_change: None,
            item_renderer: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<u8>`]：渲染期自动建立细粒度订阅，点击评分时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<u8>) -> Self {
        let cb = cx.arena().alloc(move |v: u8, _: &mut ActionScope| {
            sig.set_if_changed(v);
        });
        Self::new(id, sig.get()).on_change(cb)
    }

    /// 设置最大星数。
    #[inline(always)]
    pub const fn max(mut self, max: u8) -> Self {
        self.max = if max == 0 { 1 } else { max };
        self
    }

    /// 设置是否为只读展示模式。
    #[inline(always)]
    pub const fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    /// 绑定点击切换星级评分回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(u8, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// `gpui-base` 风格单星插槽定制回调（零堆分配）。
    #[inline]
    pub fn item<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, RatingItemState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |star: Div<'a>, state: RatingItemState, frame_cx: &'a FrameScope| {
                f(star, state).into_element(frame_cx)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Rating<'a>);

impl<'a> Widget<'a> for Rating<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let active_color = if self.active_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.active_color
        };
        let inactive_color = if self.inactive_color == Color::TRANSPARENT {
            def_fg.scale_alpha(120)
        } else {
            self.inactive_color
        };

        let mut row = div().id(self.id).flex_row().gap(4).items_center();
        if !self.readonly {
            row = row.focusable();
            if !self.focus_ring {
                row = row.no_focus_border();
            }
        }
        let on_chg = self.on_change;

        for idx in 1..=self.max {
            let filled = idx <= self.value;
            let glyph = if filled { "★" } else { "☆" };
            let col = if filled {
                active_color
            } else {
                inactive_color
            };
            let mut star = div().id(crate::scoped_child_id(self.id, 0xF289_1050, idx as u32));
            if !self.readonly {
                if let Some(cb) = on_chg {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    star = star.on_click_raw(click_cb);
                }
            }
            if let Some(render_fn) = self.item_renderer {
                let el = render_fn(
                    star.child(cx, text(glyph)),
                    RatingItemState {
                        index: idx,
                        filled,
                        readonly: self.readonly,
                    },
                    cx,
                );
                row = row.child(cx, el);
            } else {
                star = star
                    .w(22)
                    .h(22)
                    .items_center()
                    .justify_center()
                    .child(cx, text(glyph).size(15).color(col));
                row = row.child(cx, star);
            }
        }

        self.style_ext.apply_to_div(row, self.style).into_element(cx)
    }
}
