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
    /// 该像素格式在渐变渲染时是否需要 4×4 Bayer 有序抖动（仅 `Rgb565` 为 `true`，32-bit 格式编译期零开销消除）。
    const NEEDS_DITHER: bool = false;

    /// 从标准 `ARGB8888` 颜色转换。
    fn from_color(c: Color) -> Self;
    /// 带像素坐标 `(x, y)` 的有序抖动颜色转换：16-bit `Rgb565` 使用 4×4 Bayer 矩阵消除渐变色带，
    /// 32-bit 像素格式默认编译期零开销直通 [`Self::from_color`]。
    #[inline(always)]
    fn from_color_dithered(c: Color, _x: i16, _y: i16) -> Self {
        Self::from_color(c)
    }
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

/// 4×4 Bayer 有序抖动阈值表（值域 `0..=15`，专供 `Rgb565` 渐变消除 5-6-5 量化色带）。
const BAYER_4X4: [u8; 16] = [
    0, 8, 2, 10,
    12, 4, 14, 6,
    3, 11, 1, 9,
    15, 7, 13, 5,
];

impl PixelFormat for Rgb565 {
    const BLACK: Self = Self(0);
    const NEEDS_DITHER: bool = true;

    #[inline(always)]
    fn from_color(c: Color) -> Self {
        let r = ((c.r() as u16) >> 3) & 0x1F;
        let g = ((c.g() as u16) >> 2) & 0x3F;
        let b = ((c.b() as u16) >> 3) & 0x1F;
        Self((r << 11) | (g << 5) | b)
    }

    #[inline(always)]
    fn from_color_dithered(c: Color, x: i16, y: i16) -> Self {
        // 无偏 4×4 Bayer 有序抖动：精确按 31/255（R/B）与 63/255（G）比例分配残余误差，
        // 避免简单 `>> 3` 截断带来的中性灰偏亮，同时保证纯黑（0）与纯白（255）100% 精确无噪点。
        let t_off = ((BAYER_4X4[(((y as usize) & 3) << 2) | ((x as usize) & 3)] as u16) << 4) + 8;
        let nr = (c.r() as u16) * 31 + t_off;
        let ng = (c.g() as u16) * 63 + t_off;
        let nb = (c.b() as u16) * 31 + t_off;
        let r = ((nr + (nr >> 8)) >> 8).min(31);
        let g = ((ng + (ng >> 8)) >> 8).min(63);
        let b = ((nb + (nb >> 8)) >> 8).min(31);
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

    /// 面向嵌入式 MCU 榨干级特化：32-bit 隔槽 SWAR 单乘法 RGB565 Alpha 混合（**0 次除法**、2 次 32-bit 乘法）。
    ///
    /// 将 16 位 RGB565 展开到 `0x07E0_F81F`（`00000_GGGGGG_00000_RRRRR_000000_BBBBB`），
    /// 三个通道间留有 ≥5 bit 隔离防进位间隙，量化 `a5 ∈ 0..=32` 后一次并行完成 R/G/B 三通道混合。
    #[inline(always)]
    fn blend(self, fg: Color, coverage: u8) -> Self {
        let fg_a = fg.a() as u32;
        if fg_a == 0 || coverage == 0 {
            return self;
        }
        let alpha = if fg_a == 255 {
            coverage as u32
        } else {
            (((fg_a * (coverage as u32)) + 128) * 257) >> 16
        };
        if alpha == 0 {
            return self;
        }
        let fg565 = Self::from_color(fg).0 as u32;
        let a5 = (alpha + 4) >> 3; // 0..=32
        if a5 >= 32 {
            return Self(fg565 as u16);
        }
        if a5 == 0 {
            return self;
        }
        let bg = self.0 as u32;
        let bg_sp = (bg | (bg << 16)) & 0x07E0_F81F;
        let fg_sp = (fg565 | (fg565 << 16)) & 0x07E0_F81F;
        let inv = 32 - a5;
        let res = ((bg_sp * inv + fg_sp * a5) >> 5) & 0x07E0_F81F;
        Self((res | (res >> 16)) as u16)
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
