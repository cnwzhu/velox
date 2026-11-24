//! 扁平绘制指令原语（`DrawOp`、`DrawPayload` 与帧间差异指纹 `DrawOpSnapshot`）。

use crate::arena::StrRef;
use crate::element::SpriteSource;
use crate::paint::{LayerSpec, StyledQuad};
use crate::solver::Region;
use crate::style::Color;

/// 扁平化一维渲染指令。
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct DrawOp<'a> {
    /// 关联的元素 ID（用于 $O(1)$ 匹配上一帧指令触发过渡动画）。
    pub id: Option<u32>,
    /// 屏幕绝对坐标矩形。
    pub bounds: Region,
    /// 父级裁剪区（用于滚动视图与圆角容器裁剪）。
    pub clip: Region,
    /// 父级裁剪圆角半径（逻辑像素；`0` 表示轴对齐直角裁剪，非零时启用抗锯齿圆角 SDF 裁切）。
    pub clip_radius: u8,
    /// 绘制负载数据。
    pub payload: DrawPayload<'a>,
}

/// 闭合绘制负载枚举（3 种零开销高频基元 `Quad`/`TextRun`/`Sprite` + 3 种零体积膨胀冷路径高级原语）。
#[derive(Copy, Clone)]
pub enum DrawPayload<'a> {
    /// 圆角矩形、定点数线性渐变与边框（由 SDF 扫描线算法一次性合成，`border_width` 最高位 `0x80` 标识水平渐变）。
    Quad {
        bg: Color,
        bg_grad: Color,
        border_color: Color,
        rounded: u8,
        border_width: u8,
    },
    /// 文本切片（`text_flags` 包含水平对齐、`ellipsis` 省略号与 `nowrap` 标志位）。
    TextRun {
        text: StrRef<'a>,
        color: Color,
        font_size: u8,
        text_flags: u8,
    },
    /// A4/A8 抗锯齿 Alpha 图标掩码、RGB565 贴图或过程化矢量精灵。
    Sprite(SpriteSource<'a>),
    /// 高级矩形图元：多色标渐变（Linear/Radial/Conic + OKLab）、四角独立圆角、2D 仿射变换与单图元混合模式（8B Arena 引用，不膨胀 `DrawOp`）。
    StyledQuad(&'a StyledQuad<'a>),
    /// 推入隔离合成图层：支持组级不透明度、27 种混合模式、高斯模糊、色彩矩阵、任意形状投影、实时毛玻璃与 SVG Path 裁剪（8B Arena 引用）。
    PushLayer(&'a LayerSpec<'a>),
    /// 弹出当前隔离合成图层并执行滤镜后处理与合成回父画布（0B）。
    PopLayer,
}

impl<'a> PartialEq for DrawPayload<'a> {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (
                Self::Quad {
                    bg: b1,
                    bg_grad: bg1,
                    border_color: bc1,
                    rounded: r1,
                    border_width: bw1,
                },
                Self::Quad {
                    bg: b2,
                    bg_grad: bg2,
                    border_color: bc2,
                    rounded: r2,
                    border_width: bw2,
                },
            ) => b1 == b2 && bg1 == bg2 && bc1 == bc2 && r1 == r2 && bw1 == bw2,
            (
                Self::TextRun {
                    text: t1,
                    color: c1,
                    font_size: f1,
                    text_flags: tf1,
                },
                Self::TextRun {
                    text: t2,
                    color: c2,
                    font_size: f2,
                    text_flags: tf2,
                },
            ) => t1 == t2 && c1 == c2 && f1 == f2 && tf1 == tf2,
            (
                Self::Sprite(SpriteSource::Bitmap {
                    bytes: b1,
                    format: f1,
                    tint: t1,
                }),
                Self::Sprite(SpriteSource::Bitmap {
                    bytes: b2,
                    format: f2,
                    tint: t2,
                }),
            ) => {
                core::ptr::eq(b1.as_ptr(), b2.as_ptr())
                    && b1.len() == b2.len()
                    && f1 == f2
                    && t1 == t2
            }
            (
                Self::Sprite(SpriteSource::Shader { state_hash: h1, .. }),
                Self::Sprite(SpriteSource::Shader { state_hash: h2, .. }),
            ) => h1 == h2,
            (
                Self::Sprite(s1 @ SpriteSource::Svg { .. }),
                Self::Sprite(s2 @ SpriteSource::Svg { .. }),
            ) => s1 == s2,
            (Self::Sprite(s1 @ SpriteSource::Arc { .. }), Self::Sprite(s2 @ SpriteSource::Arc { .. })) => {
                s1 == s2
            }
            (Self::StyledQuad(a), Self::StyledQuad(b)) => a == b,
            (Self::PushLayer(a), Self::PushLayer(b)) => a == b,
            (Self::PopLayer, Self::PopLayer) => true,
            _ => false,
        }
    }
}

impl<'a> Eq for DrawPayload<'a> {}

/// 独立于帧生命周期的 `DrawOp` 微快照（12 字节紧凑表示，预求交裁剪框并内联哈希，用于原地流式比对）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct DrawOpSnapshot {
    /// 预先与父级 `clip` 求交后的有效屏幕矩形（8 字节）。
    pub effective_bounds: Region,
    /// 包含 ID、裁剪区与绘制负载的 32 位 FNV-1a 哈希（4 字节）。
    pub payload_hash: u32,
}

impl<'a> DrawOp<'a> {
    /// 生成与 `'a` 生命周期无关的 12 字节紧凑绘制快照。
    pub fn to_snapshot(&self) -> DrawOpSnapshot {
        let mut h: u32 = 0x811c_9dc5;
        let mix = |h: &mut u32, byte: u8| {
            *h ^= byte as u32;
            *h = h.wrapping_mul(0x0100_0193);
        };
        let mix_i16 = |h: &mut u32, val: i16| {
            for b in val.to_le_bytes() {
                mix(h, b);
            }
        };
        let mix_u32 = |h: &mut u32, val: u32| {
            for b in val.to_le_bytes() {
                mix(h, b);
            }
        };
        if let Some(id) = self.id {
            mix(&mut h, 0xFF);
            mix_u32(&mut h, id);
        }
        // 将原始 bounds 与 clip 混入哈希，保证任何几何或裁剪变化均能被检出
        mix_i16(&mut h, self.bounds.x);
        mix_i16(&mut h, self.bounds.y);
        mix_i16(&mut h, self.bounds.w);
        mix_i16(&mut h, self.bounds.h);
        mix_i16(&mut h, self.clip.x);
        mix_i16(&mut h, self.clip.y);
        mix_i16(&mut h, self.clip.w);
        mix_i16(&mut h, self.clip.h);
        mix(&mut h, self.clip_radius);

        match self.payload {
            DrawPayload::Quad {
                bg,
                bg_grad,
                border_color,
                rounded,
                border_width,
            } => {
                mix(&mut h, 1);
                mix_u32(&mut h, bg.0);
                mix_u32(&mut h, bg_grad.0);
                mix_u32(&mut h, border_color.0);
                mix(&mut h, rounded);
                mix(&mut h, border_width);
            }
            DrawPayload::TextRun {
                text,
                color,
                font_size,
                text_flags,
            } => {
                mix(&mut h, 2);
                mix_u32(&mut h, color.0);
                mix(&mut h, font_size);
                mix(&mut h, text_flags);
                for &b in text.as_str().as_bytes() {
                    mix(&mut h, b);
                }
            }
            DrawPayload::Sprite(SpriteSource::Bitmap {
                bytes,
                format,
                tint,
            }) => {
                mix(&mut h, 3);
                mix(&mut h, format as u8);
                mix_u32(&mut h, tint.0);
                mix_u32(&mut h, bytes.as_ptr() as usize as u32);
                mix_u32(&mut h, bytes.len() as u32);
            }
            DrawPayload::Sprite(SpriteSource::Shader { state_hash, .. }) => {
                mix(&mut h, 4);
                mix_u32(&mut h, state_hash);
            }
            DrawPayload::Sprite(SpriteSource::Svg {
                source,
                is_path_only,
                tint,
                stroke_width_x10,
            }) => {
                mix(&mut h, 6);
                mix(&mut h, if is_path_only { 1 } else { 0 });
                mix(&mut h, stroke_width_x10);
                mix_u32(&mut h, tint.0);
                for &b in source.as_str().as_bytes() {
                    mix(&mut h, b);
                }
            }
            DrawPayload::Sprite(SpriteSource::Arc {
                thickness,
                start_deg10,
                sweep_deg10,
                fill_sweep_deg10,
                track_color,
                fill_color,
            }) => {
                mix(&mut h, 5);
                mix(&mut h, thickness);
                mix_i16(&mut h, start_deg10 as i16);
                mix_i16(&mut h, sweep_deg10 as i16);
                mix_i16(&mut h, fill_sweep_deg10 as i16);
                mix_u32(&mut h, track_color.0);
                mix_u32(&mut h, fill_color.0);
            }
            DrawPayload::StyledQuad(sq) => {
                mix(&mut h, 7);
                sq.mix_hash(&mut h);
            }
            DrawPayload::PushLayer(layer) => {
                mix(&mut h, 8);
                layer.mix_hash(&mut h);
            }
            DrawPayload::PopLayer => {
                mix(&mut h, 9);
            }
        }
        let eff_bounds = match self.payload {
            DrawPayload::PushLayer(layer) => layer.expanded_bounds().intersect(self.clip),
            DrawPayload::StyledQuad(sq) => {
                if let Some(t) = sq.transform {
                    let cx = self.bounds.x as f32 + self.bounds.w as f32 * 0.5;
                    let cy = self.bounds.y as f32 + self.bounds.h as f32 * 0.5;
                    t.centered_at(cx, cy)
                        .transform_region(self.bounds)
                        .intersect(self.clip)
                } else {
                    self.bounds.intersect(self.clip)
                }
            }
            _ => self.bounds.intersect(self.clip),
        };
        DrawOpSnapshot {
            effective_bounds: eff_bounds,
            payload_hash: h,
        }
    }
}

