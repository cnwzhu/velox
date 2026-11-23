//! 硬件帧缓冲像素格式抽象：`PixelFormat`、`Rgb565`、`Xrgb8888` 与 `SpriteFormat`。

use crate::style::Color;

/// 贴图与图标掩码像素格式。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SpriteFormat {
    /// 4-bit Alpha 抗锯齿掩码（每字节 2 像素）。
    A4 = 0,
    /// 8-bit Alpha 抗锯齿掩码（每字节 1 像素）。
    A8 = 1,
    /// 16-bit RGB565 彩色位图（小端序每像素 2 字节）。
    Rgb565 = 2,
    /// 32-bit RGBA8888 彩色带透明通道位图（每像素 4 字节 `[R, G, B, A]`）。
    Rgba8888 = 5,
}


/// 硬件帧缓冲像素格式抽象。
pub trait PixelFormat: Copy + Clone + Default + PartialEq + Eq + Send + Sync + 'static {
    /// 全零/纯黑默认像素常量（供 `const fn` 静态帧缓冲初始化使用）。
    const BLACK: Self;

    /// 从标准 `ARGB8888` 颜色转换。
    fn from_color(c: Color) -> Self;
    /// 转换回不透明 `ARGB8888` 颜色。
    fn to_color(self) -> Color;
    /// 将前景色 `fg` 以 `coverage`（`0..=255`）覆盖率混合到当前背景像素上（纯整数四舍五入运算）。
    #[inline(always)]
    fn blend(self, fg: Color, coverage: u8) -> Self {
        let alpha = (((fg.a() as u32) * (coverage as u32) + 127) / 255) as u16;
        if alpha == 0 {
            return self;
        }
        if alpha >= 255 {
            return Self::from_color(fg);
        }
        let bg = self.to_color();
        let inv = 255 - alpha;
        let r = (((fg.r() as u16) * alpha + (bg.r() as u16) * inv + 127) / 255) as u8;
        let g = (((fg.g() as u16) * alpha + (bg.g() as u16) * inv + 127) / 255) as u8;
        let b = (((fg.b() as u16) * alpha + (bg.b() as u16) * inv + 127) / 255) as u8;
        Self::from_color(Color::rgb(r, g, b))
    }
}

/// 16 位 `RGB565` 像素格式（面向 STM32 SPI/FSMC/DMA2D 显示屏）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Rgb565(pub u16);

impl PixelFormat for Rgb565 {
    const BLACK: Self = Self(0);

    #[inline(always)]
    fn from_color(c: Color) -> Self {
        let r = ((c.r() as u16) >> 3) & 0x1F;
        let g = ((c.g() as u16) >> 2) & 0x3F;
        let b = ((c.b() as u16) >> 3) & 0x1F;
        Self((r << 11) | (g << 5) | b)
    }

    #[inline(always)]
    fn to_color(self) -> Color {
        let r5 = ((self.0 >> 11) & 0x1F) as u8;
        let g6 = ((self.0 >> 5) & 0x3F) as u8;
        let b5 = (self.0 & 0x1F) as u8;
        let r = (r5 << 3) | (r5 >> 2);
        let g = (g6 << 2) | (g6 >> 4);
        let b = (b5 << 3) | (b5 >> 2);
        Color::rgb(r, g, b)
    }
}

/// 32 位 `XRGB8888` 像素格式（面向 Linux Wayland `wl_shm` 与桌面窗口）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Xrgb8888(pub u32);

impl PixelFormat for Xrgb8888 {
    const BLACK: Self = Self(0);

    #[inline(always)]
    fn from_color(c: Color) -> Self {
        Self(0xFF00_0000 | (c.0 & 0x00FF_FFFF))
    }

    #[inline(always)]
    fn to_color(self) -> Color {
        Color(0xFF00_0000 | (self.0 & 0x00FF_FFFF))
    }
}

/// 32 位 `RGBA8888`（内存字节序 `[R, G, B, A]`，小端序 `0xAABBGGRR`）像素格式。
///
/// 原生适配 WebAssembly HTML5 `<canvas>` `ImageData`、Android `ANativeWindow` (`WINDOW_FORMAT_RGBA_8888`)
/// 以及 iOS CoreGraphics `CGBitmapContext` (`kCGImageAlphaPremultipliedLast`)，零通道转换开销。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Rgba8888(pub u32);

impl Rgba8888 {
    /// 获取小端序 `[R, G, B, A]` 4 字节数组。
    #[inline(always)]
    pub const fn to_rgba_bytes(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }
}

impl PixelFormat for Rgba8888 {
    const BLACK: Self = Self(0xFF00_0000);

    #[inline(always)]
    fn from_color(c: Color) -> Self {
        let r = c.r() as u32;
        let g = (c.g() as u32) << 8;
        let b = (c.b() as u32) << 16;
        let a = (c.a() as u32) << 24;
        Self(a | b | g | r)
    }

    #[inline(always)]
    fn to_color(self) -> Color {
        let r = (self.0 & 0xFF) as u8;
        let g = ((self.0 >> 8) & 0xFF) as u8;
        let b = ((self.0 >> 16) & 0xFF) as u8;
        let a = ((self.0 >> 24) & 0xFF) as u8;
        Color::rgba(r, g, b, a)
    }

    #[inline(always)]
    fn blend(self, fg: Color, coverage: u8) -> Self {
        let sa = (((fg.a() as u32) * (coverage as u32) + 127) / 255) as u16;
        if sa == 0 {
            return self;
        }
        if sa >= 255 {
            return Self::from_color(Color::rgba(fg.r(), fg.g(), fg.b(), 255));
        }
        let bg = self.to_color();
        let da = bg.a() as u16;
        let inv = 255 - sa;
        let out_a = sa + ((da * inv + 127) / 255);
        let r = (((fg.r() as u16) * sa + (bg.r() as u16) * inv + 127) / 255) as u8;
        let g = (((fg.g() as u16) * sa + (bg.g() as u16) * inv + 127) / 255) as u8;
        let b = (((fg.b() as u16) * sa + (bg.b() as u16) * inv + 127) / 255) as u8;
        Self::from_color(Color::rgba(r, g, b, out_a.min(255) as u8))
    }
}
