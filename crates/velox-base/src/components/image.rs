//! 支持回退占位符（Fallback）的图像展示组件 `Image`。

use velox_engine::prelude::*;

/// 图像缩放适配模式（`ImageFit`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageFit {
    #[default]
    Cover,
    Contain,
    Fill,
}

/// 支持回退占位符（Fallback）的图像展示组件（`Image`）。
pub struct Image<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub src: &'a str,
    pub fallback: &'a str,
    pub fit: ImageFit,
    pub rounded: u8,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub style: Style,
    pub style_ext: StyleExt,
    pub pixels: Option<(&'a [u32], u16, u16)>,
    has_custom_rounded: bool,
}

impl<'a> Image<'a> {
    /// 创建一个带宽高与资源标识/回退文案的图像组件。
    #[inline(always)]
    pub const fn new(id: u32, width: i16, height: i16, src: &'a str) -> Self {
        Self {
            id,
            width: if width < 16 { 16 } else { width },
            height: if height < 16 { 16 } else { height },
            src,
            fallback: "IMG",
            fit: ImageFit::Cover,
            rounded: 0,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            pixels: None,
            has_custom_rounded: false,
        }
    }

    /// 设置当图像未加载或无像素缓冲时显示的回退文本。
    #[inline(always)]
    pub const fn fallback(mut self, fallback: &'a str) -> Self {
        self.fallback = fallback;
        self
    }

    /// 设置图像缩放适配方式。
    #[inline(always)]
    pub const fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// 设置圆角半径。
    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.rounded = radius;
        self.has_custom_rounded = true;
        self
    }

    /// 绑定内存 ARGB8888 像素缓冲区 `(pixels, src_w, src_h)`。
    #[inline(always)]
    pub const fn pixels(mut self, pixels: &'a [u32], src_w: u16, src_h: u16) -> Self {
        self.pixels = Some((pixels, src_w, src_h));
        self
    }
}

crate::impl_base_styled_box!(Image<'a>);

impl<'a> Widget<'a> for Image<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        if let Some((buf, sw, sh)) = self.pixels {
            if !buf.is_empty() && sw > 0 && sh > 0 {
                let w = width;
                let h = height;
                let hash = self.id ^ ((sw as u32) << 16) ^ (sh as u32) ^ buf[0];
                let elem = img_shader(cx, w, h, hash, move |painter: &mut dyn SurfacePainter| {
                    let b = painter.bounds();
                    let c = painter.clip();
                    if b.w <= 0 || b.h <= 0 {
                        return;
                    }
                    for py in c.y..c.bottom() {
                        let sy = (((py - b.y) as u32 * (sh as u32)) / (b.h as u32))
                            .min((sh - 1) as u32) as usize;
                        for px in c.x..c.right() {
                            let sx = (((px - b.x) as u32 * (sw as u32)) / (b.w as u32))
                                .min((sw - 1) as u32) as usize;
                            let idx = sy * (sw as usize) + sx;
                            if let Some(&argb) = buf.get(idx) {
                                let a = ((argb >> 24) & 0xFF) as u8;
                                let r = ((argb >> 16) & 0xFF) as u8;
                                let g = ((argb >> 8) & 0xFF) as u8;
                                let bl = (argb & 0xFF) as u8;
                                painter.blend_pixel(px, py, Color::rgb(r, g, bl), a);
                            }
                        }
                    }
                })
                .into_element(cx);
                return self.style_ext.wrap_element(cx, elem, self.style);
            }
        }

        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            cx.theme().foreground
        };

        let label = if !self.fallback.is_empty() {
            self.fallback
        } else if !self.src.is_empty() {
            self.src
        } else {
            "IMG"
        };

        let mut box_el = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(height)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .justify_center()
            .overflow_hidden()
            .child(cx, text(label).size(12).color(text_color).ellipsis());
        if self.border_color.a() > 0 {
            box_el = box_el.border(1, self.border_color);
        }
        box_el = self.style_ext.apply_to_div(box_el, self.style);
        box_el.into_element(cx)
    }
}
