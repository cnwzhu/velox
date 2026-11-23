//! 底部状态栏基础组件 `StatusBar`（默认无样式）。

use velox_engine::prelude::*;

/// 底部状态栏单元条目（`StatusBarItem`）。
#[derive(Clone, Copy, Debug)]
pub struct StatusBarItem<'a> {
    pub label: &'a str,
    pub accent: Option<Color>,
}

impl<'a> StatusBarItem<'a> {
    /// 创建一个状态栏条目。
    #[inline(always)]
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            accent: None,
        }
    }

    /// 设置条目强调前景色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.accent = Some(color);
        self
    }
}

/// 底部状态栏基础组件（`StatusBar`，默认无样式，支持左、中、右三个分区的状态指示项排布）。
pub struct StatusBar<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub left: &'a [StatusBarItem<'a>],
    pub center: &'a [StatusBarItem<'a>],
    pub right: &'a [StatusBarItem<'a>],
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> StatusBar<'a> {
    /// 创建一个默认无样式的底部状态栏组件。
    #[inline(always)]
    pub const fn new(id: u32, width: i16) -> Self {
        Self {
            id,
            width,
            height: 24,
            left: &[],
            center: &[],
            right: &[],
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置左侧区域状态项。
    #[inline(always)]
    pub const fn left(mut self, items: &'a [StatusBarItem<'a>]) -> Self {
        self.left = items;
        self
    }

    /// 设置中间区域状态项。
    #[inline(always)]
    pub const fn center(mut self, items: &'a [StatusBarItem<'a>]) -> Self {
        self.center = items;
        self
    }

    /// 设置右侧区域状态项。
    #[inline(always)]
    pub const fn right(mut self, items: &'a [StatusBarItem<'a>]) -> Self {
        self.right = items;
        self
    }
}

crate::impl_base_styled_box!(StatusBar<'a>);

impl<'a> Widget<'a> for StatusBar<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let default_fg = if self.text_color.a() > 0 {
            self.text_color
        } else {
            cx.theme().foreground
        };

        let render_region = |cx: &'a FrameScope, items: &'a [StatusBarItem<'a>]| {
            let mut r = div().flex_row().gap(10).items_center();
            for item in items {
                let col = item.accent.unwrap_or(default_fg);
                r = r.child(cx, text(item.label).size(11).color(col));
            }
            r
        };

        let mut bar = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .h(height)
            .px(10)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .justify_between()
            .child(cx, render_region(cx, self.left))
            .child(cx, render_region(cx, self.center))
            .child(cx, render_region(cx, self.right));
        if self.border_color.a() > 0 {
            bar = bar.border(1, self.border_color);
        }
        bar = self.style_ext.apply_to_div(bar, self.style);
        bar.into_element(cx)
    }
}
