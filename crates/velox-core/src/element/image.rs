//! 位图、RLE 压缩掩码、旋转精灵、过程化 SDF、原生矢量 SVG 与 GPU 原生解析圆弧图像节点 `Img<'a>`。

use crate::arena::StrRef;
use crate::display::SpriteFormat;
use crate::pod::FrameScope;
use crate::sdf::SurfacePainter;
use crate::style::Color;

use super::{Element, IntoElement};

impl<'a> IntoElement<'a> for Img<'a> {
    #[inline(always)]
    fn into_element(self, _cx: &'a FrameScope) -> Element<'a> {
        Element::Img(self)
    }
}


/// `Sprite` 贴图数据源（支持静态/帧池预烘焙位图掩码，或过程化 SDF 精灵贴图）。
#[derive(Copy, Clone)]
pub enum SpriteSource<'a> {
    /// 预烘焙 `A4`/`A8` Alpha 掩码或 `Rgb565` 位图切片。
    Bitmap {
        bytes: &'a [u8],
        format: SpriteFormat,
        tint: Color,
    },
    /// 过程化矢量精灵（分配于 `ScratchArena`，如旋转表针或动态仪表盘层）。
    Shader {
        state_hash: u32,
        painter: &'a dyn Fn(&mut dyn SurfacePainter),
    },
    /// 原生矢量 SVG 文档或单路径 `d` 字符串（纯 POD，支持软件多核条带并行与 GPU `SpriteAtlas` HiDPI 跨帧缓存）。
    Svg {
        /// SVG XML 文本或单条 `d` 路径数据（静态字面量或帧级 `ScratchArena` 字符串）。
        source: StrRef<'a>,
        /// `true` 表示 `source` 为 24×24 标准 SVG Path `d` 字符串；`false` 表示完整 `<svg>` XML 文档。
        is_path_only: bool,
        /// 前景着色颜色（对应 XML 中的 `currentColor` 及整体 Alpha 透明度；纯路径模式下作为绘制颜色）。
        tint: Color,
        /// 0.1px 精度描边宽度（纯路径模式下 `0` 表示实心填充，`>0` 如 `20` 表示 `2.0px` 圆头描边线条图标；XML 模式下 `0` 遵循内置 `stroke-width`）。
        stroke_width_x10: u8,
    },
    /// 原生解析圆弧/圆环 SDF 图元（纯 POD，GPU 端单 Draw Call 片元直算零上传，软件端支持多线程并行光栅）。
    Arc {
        /// 圆弧线宽（逻辑像素）。
        thickness: u8,
        /// 起始顺时针角度（单位 `0.1°`，`0` 为 12 点钟方向）。
        start_deg10: u16,
        /// 背景轨道总跨度（单位 `0.1°`）。
        sweep_deg10: u16,
        /// 前景填充进度跨度（单位 `0.1°`）。
        fill_sweep_deg10: u16,
        /// 背景轨道颜色。
        track_color: Color,
        /// 前景填充颜色。
        fill_color: Color,
    },
}

impl<'a> PartialEq for SpriteSource<'a> {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (
                Self::Bitmap {
                    bytes: b1,
                    format: f1,
                    tint: t1,
                },
                Self::Bitmap {
                    bytes: b2,
                    format: f2,
                    tint: t2,
                },
            ) => f1 == f2 && t1 == t2 && b1 == b2,
            (
                Self::Shader {
                    state_hash: h1, ..
                },
                Self::Shader {
                    state_hash: h2, ..
                },
            ) => h1 == h2,
            (
                Self::Svg {
                    source: s1,
                    is_path_only: p1,
                    tint: t1,
                    stroke_width_x10: sw1,
                },
                Self::Svg {
                    source: s2,
                    is_path_only: p2,
                    tint: t2,
                    stroke_width_x10: sw2,
                },
            ) => p1 == p2 && t1 == t2 && sw1 == sw2 && s1 == s2,
            (
                Self::Arc {
                    thickness: t1,
                    start_deg10: s1,
                    sweep_deg10: sw1,
                    fill_sweep_deg10: f1,
                    track_color: tc1,
                    fill_color: fc1,
                },
                Self::Arc {
                    thickness: t2,
                    start_deg10: s2,
                    sweep_deg10: sw2,
                    fill_sweep_deg10: f2,
                    track_color: tc2,
                    fill_color: fc2,
                },
            ) => t1 == t2 && s1 == s2 && sw1 == sw2 && f1 == f2 && tc1 == tc2 && fc1 == fc2,
            _ => false,
        }
    }
}

impl<'a> Eq for SpriteSource<'a> {}

impl<'a> core::fmt::Debug for SpriteSource<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Bitmap {
                bytes,
                format,
                tint,
            } => f
                .debug_struct("Bitmap")
                .field("byte_len", &bytes.len())
                .field("format", &format)
                .field("tint", &tint)
                .finish(),
            Self::Shader { state_hash, .. } => f
                .debug_struct("Shader")
                .field("state_hash", &state_hash)
                .finish(),
            Self::Svg {
                source,
                is_path_only,
                tint,
                stroke_width_x10,
            } => f
                .debug_struct("Svg")
                .field("len", &source.as_str().len())
                .field("is_path_only", &is_path_only)
                .field("tint", &tint)
                .field("stroke_width_x10", &stroke_width_x10)
                .finish(),
            Self::Arc {
                thickness,
                start_deg10,
                sweep_deg10,
                fill_sweep_deg10,
                track_color,
                fill_color,
            } => f
                .debug_struct("Arc")
                .field("thickness", &thickness)
                .field("start_deg10", &start_deg10)
                .field("sweep_deg10", &sweep_deg10)
                .field("fill_sweep_deg10", &fill_sweep_deg10)
                .field("track_color", &track_color)
                .field("fill_color", &fill_color)
                .finish(),
        }
    }
}

/// 贴图与图标掩码原语节点（最终编译为 `DrawPayload::Sprite`）。
#[derive(Copy, Clone)]
pub struct Img<'a> {
    pub width: i16,
    pub height: i16,
    pub source: SpriteSource<'a>,
}

/// 构造一个位图贴图节点（支持 `&'static [u8]` 或 `ScratchArena` 切片 `&'a [u8]`）。
#[inline(always)]
pub const fn img<'a>(
    width: i16,
    height: i16,
    bytes: &'a [u8],
    format: SpriteFormat,
) -> Img<'a> {
    Img {
        width,
        height,
        source: SpriteSource::Bitmap {
            bytes,
            format,
            tint: Color::WHITE,
        },
    }
}

/// 构造一个 A8 Alpha 掩码图标节点（等价于编译期预烘焙的 SVG 图标）。
#[inline(always)]
pub const fn svg<'a>(width: i16, height: i16, alpha_mask: &'a [u8]) -> Img<'a> {
    img(width, height, alpha_mask, SpriteFormat::A8)
}

/// 构造一个 A4 (4-bit Alpha) 紧凑掩码图标节点（体积比 A8 减半）。
#[inline(always)]
pub const fn svg_a4<'a>(width: i16, height: i16, alpha_mask_a4: &'a [u8]) -> Img<'a> {
    img(width, height, alpha_mask_a4, SpriteFormat::A4)
}

/// 构造一个完整原生矢量 SVG XML 图像节点（支持 `<svg viewBox>`、`<g>` 嵌套、`transform`、多色填充/描边与 `currentColor`）。
#[inline(always)]
pub fn svg_str<'a>(width: i16, height: i16, xml: impl Into<StrRef<'a>>) -> Img<'a> {
    Img {
        width,
        height,
        source: SpriteSource::Svg {
            source: xml.into(),
            is_path_only: false,
            tint: Color::WHITE,
            stroke_width_x10: 0,
        },
    }
}

/// 构造一个实心填充的 24×24 标准 SVG Path `d` 矢量图形节点（配合 `.tint(color)` 着色）。
#[inline(always)]
pub fn svg_path<'a>(width: i16, height: i16, d: impl Into<StrRef<'a>>) -> Img<'a> {
    Img {
        width,
        height,
        source: SpriteSource::Svg {
            source: d.into(),
            is_path_only: true,
            tint: Color::WHITE,
            stroke_width_x10: 0,
        },
    }
}

/// 构造一个 Lucide 风格 24×24 圆头圆角描边（默认 `stroke-width = 2.0`）矢量线条图标节点（配合 `.tint(color)` 与 `.stroke_width(...)` 定制）。
#[inline(always)]
pub fn svg_icon<'a>(width: i16, height: i16, d: impl Into<StrRef<'a>>) -> Img<'a> {
    Img {
        width,
        height,
        source: SpriteSource::Svg {
            source: d.into(),
            is_path_only: true,
            tint: Color::WHITE,
            stroke_width_x10: 20,
        },
    }
}


/// 构造一个支持任意角度旋转（`angle_deg10` 单位为 `0.1°`）与定点缩放（`scale_q8` 基准为 `256 = 1.0x`）的 `Rgb565` 仿射变换精灵节点。
pub fn img_rotated_rgb565<'a>(
    cx: &'a FrameScope,
    width: i16,
    height: i16,
    src_w: i16,
    src_h: i16,
    rgb565_le_bytes: &'a [u8],
    angle_deg10: u16,
    scale_q8: u16,
) -> Img<'a> {
    let state_hash = (rgb565_le_bytes.as_ptr() as usize as u32)
        ^ ((angle_deg10 as u32) << 16)
        ^ (scale_q8 as u32);
    img_shader(cx, width, height, state_hash, move |surface| {
        let b = surface.bounds();
        let c = surface.clip();
        if c.is_empty() || src_w <= 0 || src_h <= 0 || scale_q8 == 0 {
            return;
        }
        let (sin_q10, cos_q10) = crate::sdf::sin_cos_deg10((angle_deg10 % 3600) as i32);
        let dst_cx = b.x as i32 + (b.w as i32) / 2;
        let dst_cy = b.y as i32 + (b.h as i32) / 2;
        let src_cx_q8 = (src_w as i32) * 128;
        let src_cy_q8 = (src_h as i32) * 128;
        let inv_scale_q8 = (65536 / (scale_q8 as i32)).max(1);

        for py in c.y..c.bottom() {
            let dy = (py as i32) - dst_cy;
            for px in c.x..c.right() {
                let dx = (px as i32) - dst_cx;
                // 反向旋转映射回源贴图坐标 (Q8，使用 i64 杜绝小 scale_q8 时乘法溢出)
                let rx_q10 = (dx * cos_q10 + dy * sin_q10) as i64;
                let ry_q10 = (-dx * sin_q10 + dy * cos_q10) as i64;
                let sx = ((((rx_q10 * inv_scale_q8 as i64) >> 10) + src_cx_q8 as i64) >> 8) as i32;
                let sy = ((((ry_q10 * inv_scale_q8 as i64) >> 10) + src_cy_q8 as i64) >> 8) as i32;
                if sx >= 0 && sx < src_w as i32 && sy >= 0 && sy < src_h as i32 {
                    let idx = ((sy as usize) * (src_w as usize) + (sx as usize)) * 2;
                    if let Some(pair) = rgb565_le_bytes.get(idx..idx + 2) {
                        let raw = u16::from_le_bytes([pair[0], pair[1]]);
                        if raw != 0 {
                            let col = <crate::display::Rgb565 as crate::display::PixelFormat>::to_color(
                                crate::display::Rgb565(raw),
                            );
                            surface.blend_pixel(px, py, col, 255);
                        }
                    }
                }
            }
        }
    })
}

/// 构造一个原生解析圆弧/圆环 SDF 图元节点（GPU 端单 Draw Call 直算零纹理上传，软件端多线程并行光栅）。
#[inline(always)]
pub const fn img_arc(
    width: i16,
    height: i16,
    thickness: u8,
    start_deg10: u16,
    sweep_deg10: u16,
    fill_sweep_deg10: u16,
    track_color: Color,
    fill_color: Color,
) -> Img<'static> {
    Img {
        width,
        height,
        source: SpriteSource::Arc {
            thickness,
            start_deg10,
            sweep_deg10,
            fill_sweep_deg10,
            track_color,
            fill_color,
        },
    }
}

/// 构造一个过程化 SDF 矢量精灵节点（作为 `DrawPayload::Sprite` 渲染，闭包分配在 `ScratchArena` 中）。
#[inline(always)]
pub fn img_shader<'a, F>(
    cx: &'a FrameScope,
    width: i16,
    height: i16,
    state_hash: u32,
    painter: F,
) -> Img<'a>
where
    F: Fn(&mut dyn SurfacePainter) + 'a,
{
    Img {
        width,
        height,
        source: SpriteSource::Shader {
            state_hash,
            painter: cx.arena().alloc(painter),
        },
    }
}

impl<'a> Img<'a> {
    /// 设置位图掩码或矢量 SVG 的着色颜色（对完整 XML SVG 映射为 `currentColor` 及整体不透明度）。
    #[inline(always)]
    pub const fn tint(mut self, color: Color) -> Self {
        match self.source {
            SpriteSource::Bitmap {
                bytes, format, ..
            } => {
                self.source = SpriteSource::Bitmap {
                    bytes,
                    format,
                    tint: color,
                };
            }
            SpriteSource::Svg {
                source,
                is_path_only,
                stroke_width_x10,
                ..
            } => {
                self.source = SpriteSource::Svg {
                    source,
                    is_path_only,
                    tint: color,
                    stroke_width_x10,
                };
            }
            _ => {}
        }
        self
    }

    /// 设置矢量 SVG 的描边宽度（单位 `0.1px`，如 `20` 表示 `2.0px`；对 `svg_path` 设为 `0` 表示实心填充）。
    #[inline(always)]
    pub const fn stroke_width(mut self, width_x10: u8) -> Self {
        if let SpriteSource::Svg {
            source,
            is_path_only,
            tint,
            ..
        } = self.source
        {
            self.source = SpriteSource::Svg {
                source,
                is_path_only,
                tint,
                stroke_width_x10: width_x10,
            };
        }
        self
    }
}
