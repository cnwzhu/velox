//! 加载中骨架屏占位组件 `Skeleton`（支持按需回退 `Theme` 样式）。

use velox_core::prelude::*;

/// 骨架屏占位图形状（`SkeletonShape`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SkeletonShape {
    #[default]
    Text,
    Circle,
    Rect,
}

/// 加载中骨架屏占位组件（`Skeleton`）。
pub struct Skeleton {
    pub id: u32,
    pub shape: SkeletonShape,
    pub width: i16,
    pub height: i16,
    pub lines: u8,
    pub color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl Skeleton {
    /// 创建一个指定宽高的骨架屏占位块（未显式指定的颜色与圆角将在渲染时按需回退 `Theme`）。
    #[inline(always)]
    pub const fn new(id: u32, width: i16, height: i16) -> Self {
        Self {
            id,
            shape: SkeletonShape::Rect,
            width,
            height,
            lines: 1,
            color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 创建多行文本段落骨架屏。
    #[inline(always)]
    pub const fn text_lines(id: u32, width: i16, lines: u8) -> Self {
        Self {
            id,
            shape: SkeletonShape::Text,
            width,
            height: 14,
            lines: if lines == 0 { 1 } else { lines },
            color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 创建圆形头像骨架屏。
    #[inline(always)]
    pub const fn circle(id: u32, diameter: i16) -> Self {
        Self {
            id,
            shape: SkeletonShape::Circle,
            width: diameter,
            height: diameter,
            lines: 1,
            color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 自定义占位填充色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 自定义占位圆角半径。
    #[inline(always)]
    pub const fn rounded(mut self, rounded: u8) -> Self {
        self.rounded = rounded;
        self
    }
}

crate::impl_base_styled_box!(Skeleton);

impl<'a> Widget<'a> for Skeleton {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let color = if self.color.a() > 0 {
            self.color
        } else {
            cx.theme().foreground.scale_alpha(32)
        };
        let rounded = self.rounded;
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);

        match self.shape {
            SkeletonShape::Circle => {
                let r = (width.min(height) / 2).clamp(1, 127) as u8;
                let d = div().id(self.id).w(width).h(height).rounded(r).bg(color);
                self.style_ext.apply_to_div(d, self.style).into_element(cx)
            }
            SkeletonShape::Rect => {
                let d = div()
                    .id(self.id)
                    .w(width)
                    .h(height)
                    .rounded(rounded)
                    .bg(color);
                self.style_ext.apply_to_div(d, self.style).into_element(cx)
            }
            SkeletonShape::Text => {
                let mut col = div().id(self.id).flex_col().w(width).gap(6);
                let line_r = rounded.min(4);
                for i in 0..self.lines {
                    let line_w = if i + 1 == self.lines && self.lines > 1 {
                        (width * 2) / 3
                    } else {
                        width
                    };
                    col = col.child(
                        cx,
                        div()
                            .w(line_w.max(24))
                            .h(height)
                            .rounded(line_r)
                            .bg(color),
                    );
                }
                self.style_ext.apply_to_div(col, self.style).into_element(cx)
            }
        }
    }
}
