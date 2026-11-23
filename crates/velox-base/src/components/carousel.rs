//! 轮播图与幻灯片浏览基础组件 `Carousel`（默认无样式）。

use velox_engine::prelude::*;

/// 轮播图与幻灯片浏览基础组件（`Carousel`，默认无样式，支持上一项/下一项切换按钮与底部指示器）。
pub struct Carousel<'a> {
    pub id: u32,
    pub slides: &'a [&'a str],
    pub active_index: usize,
    pub width: i16,
    pub height: i16,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub btn_bg: Color,
    pub dot_bg: Color,
    pub active_dot_bg: Color,
    pub rounded: u8,
    pub btn_rounded: u8,
    pub dot_rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_change: Option<&'a dyn Fn(usize, &mut ActionScope)>,
}

impl<'a> Carousel<'a> {
    /// 创建一个默认无样式的轮播组件。
    #[inline(always)]
    pub const fn new(id: u32, slides: &'a [&'a str], active_index: usize) -> Self {
        Self {
            id,
            slides,
            active_index,
            width: 320,
            height: 140,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            btn_bg: Color::TRANSPARENT,
            dot_bg: Color::TRANSPARENT,
            active_dot_bg: Color::TRANSPARENT,
            rounded: 0,
            btn_rounded: 0,
            dot_rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_change: None,
        }
    }

    /// 设置宽高。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 绑定切换幻灯片索引回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Carousel<'a>);

impl<'a> Widget<'a> for Carousel<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let dot_bg = if self.dot_bg.a() > 0 {
            self.dot_bg
        } else {
            fg.scale_alpha(64)
        };
        let active_dot_bg = if self.active_dot_bg.a() > 0 {
            self.active_dot_bg
        } else {
            fg
        };

        let count = self.slides.len();
        let cur = if count > 0 {
            self.active_index % count
        } else {
            0
        };
        let slide_txt = self.slides.get(cur).copied().unwrap_or("Empty Slide");
        let on_chg = self.on_change;

        let mut prev_btn = div()
            .id(crate::scoped_child_id(self.id, 0x4F34_8A33, 1))
            .w(28)
            .h(28)
            .rounded(self.btn_rounded)
            .bg(self.btn_bg)
            .items_center()
            .justify_center()
            .focusable()
            .child(cx, text("<").size(12).color(text_color));
        if self.border_color.a() > 0 {
            prev_btn = prev_btn.border(1, self.border_color);
        }
        if !self.focus_ring {
            prev_btn = prev_btn.no_focus_border();
        }

        let mut next_btn = div()
            .id(crate::scoped_child_id(self.id, 0x4F34_8A33, 2))
            .w(28)
            .h(28)
            .rounded(self.btn_rounded)
            .bg(self.btn_bg)
            .items_center()
            .justify_center()
            .focusable()
            .child(cx, text(">").size(12).color(text_color));
        if self.border_color.a() > 0 {
            next_btn = next_btn.border(1, self.border_color);
        }
        if !self.focus_ring {
            next_btn = next_btn.no_focus_border();
        }

        if let (Some(cb), true) = (on_chg, count > 0) {
            let prev_idx = if cur == 0 { count - 1 } else { cur - 1 };
            let next_idx = (cur + 1) % count;
            let pcb = cx.arena().alloc(move |scope: &mut ActionScope| {
                cb(prev_idx, scope);
            });
            let ncb = cx.arena().alloc(move |scope: &mut ActionScope| {
                cb(next_idx, scope);
            });
            prev_btn = prev_btn.on_click_raw(pcb);
            next_btn = next_btn.on_click_raw(ncb);
        }

        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let stage = div()
            .flex_row()
            .w(width - 16)
            .h(height - 36)
            .items_center()
            .justify_between()
            .child(cx, prev_btn)
            .child(
                cx,
                div()
                    .w((width - 88).max(60))
                    .items_center()
                    .justify_center()
                    .child(cx, text(slide_txt).size(14).color(text_color)),
            )
            .child(cx, next_btn);

        let mut dots = div().flex_row().gap(6).items_center().justify_center();
        for i in 0..count {
            let active = i == cur;
            dots = dots.child(
                cx,
                div()
                    .w(if active { 16 } else { 6 })
                    .h(6)
                    .rounded(self.dot_rounded)
                    .bg(if active { active_dot_bg } else { dot_bg }),
            );
        }

        let mut root = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(height)
            .p(8)
            .gap(6)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .justify_between()
            .focusable()
            .child(cx, stage)
            .child(cx, dots);
        if self.border_color.a() > 0 {
            root = root.border(1, self.border_color);
        }
        if !self.focus_ring {
            root = root.no_focus_border();
        }
        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
