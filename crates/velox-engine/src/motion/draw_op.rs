//! 扁平绘制指令原语（`DrawOp`、`DrawPayload` 与帧间差异指纹 `DrawOpSnapshot`）。

use crate::arena::StrRef;
use crate::element::SpriteSource;
use crate::paint::{ExternalTextureSpec, LayerSpec, StyledQuad};
use crate::solver::Region;
use crate::style::Color;

/// 将有符号 4-bit 亚像素偏移 `(dx_q4, dy_q4)`（范围 `-8..=7`，单位 `1/16 px`）紧凑打包为 1 字节。
#[inline(always)]
pub const fn pack_subpx_q4(dx_q4: i8, dy_q4: i8) -> u8 {
    let dx = if dx_q4 < -8 {
        -8
    } else if dx_q4 > 7 {
        7
    } else {
        dx_q4
    };
    let dy = if dy_q4 < -8 {
        -8
    } else if dy_q4 > 7 {
        7
    } else {
        dy_q4
    };
    (((dx as u8) & 0x0F) << 4) | ((dy as u8) & 0x0F)
}

/// 将 [`pack_subpx_q4`] 打包的 1 字节解码为有符号 `(dx_q4, dy_q4)`（范围 `-8..=7`，单位 `1/16 px`）。
#[inline(always)]
pub const fn unpack_subpx_q4(packed: u8) -> (i8, i8) {
    let dx = (packed as i8) >> 4;
    let dy = ((packed << 4) as i8) >> 4;
    (dx, dy)
}

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
    /// 亚像素动画平移残差（利用结构体原有对齐空洞，0 字节膨胀；高 4 位 `dx_q4`、低 4 位 `dy_q4`，单位 `1/16 px`）。
    pub subpx_q4: u8,
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
    /// 外部零拷贝 GPU 纹理（如海图/视频/3D 视口共享的 `wgpu::TextureView`，8B Arena 引用，零膨胀 `DrawOp`）。
    ExternalTexture(&'a ExternalTextureSpec),
    /// 行内多字族/字重/BiDi/彩色 Emoji 富文本段落（8B Arena 引用，零膨胀 `DrawOp`）。
    RichText(&'a crate::text_rich::RichTextSpec<'a>),
    /// 支持 `ObjectFit` / `NineSlice` / LRU 纹理缓存的通用解码图像（8B Arena 引用）。
    DecodedImage(&'a crate::image_pipeline::DecodedImageSpec),
    /// 原生抗锯齿 2D 矢量画布（8B Arena 引用）。
    Vector(&'a crate::vector_canvas::VectorDrawSpec<'a>),
    /// 高密度 2D 交互式场景画布（8B Arena 引用，零膨胀 `DrawOp`）。
    SceneCanvas(&'a crate::canvas_scene::SceneCanvasSpec<'a>),
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
            (Self::ExternalTexture(a), Self::ExternalTexture(b)) => a == b,
            (Self::RichText(a), Self::RichText(b)) => a == b,
            (Self::DecodedImage(a), Self::DecodedImage(b)) => a == b,
            (Self::Vector(a), Self::Vector(b)) => a == b,
            (Self::SceneCanvas(a), Self::SceneCanvas(b)) => a == b,
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
        mix(&mut h, self.subpx_q4);

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
            DrawPayload::ExternalTexture(ext) => {
                mix(&mut h, 10);
                ext.mix_hash(&mut h);
            }
            DrawPayload::RichText(rt) => {
                mix(&mut h, 11);
                rt.mix_hash(&mut h);
            }
            DrawPayload::DecodedImage(img) => {
                mix(&mut h, 12);
                img.mix_hash(&mut h);
            }
            DrawPayload::Vector(vec_spec) => {
                mix(&mut h, 13);
                vec_spec.mix_hash(&mut h);
            }
            DrawPayload::SceneCanvas(spec) => {
                mix(&mut h, 14);
                mix_u32(&mut h, spec.content_hash());
            }
        }
        let eff_bounds = self.effective_damage_bounds();
        DrawOpSnapshot {
            effective_bounds: eff_bounds,
            payload_hash: h,
        }
    }

    /// 计算本条绘制指令实际可能触及的逻辑像素包围盒（供 `DamageTracker` 与 `DrawOpSnapshot` 共用）。
    ///
    /// 对单行 / `nowrap` `TextRun` 同步覆盖与 `compute_scaled_text_box` 同源的横向/纵向外扩范围（受 `self.clip` 裁剪），
    /// 确保溢出布局框的长文本在变更或删除时均生成完整脏区，杜绝持久渲染目标局部重绘残影。
    #[inline]
    pub fn effective_damage_bounds(&self) -> Region {
        match self.payload {
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
            DrawPayload::TextRun {
                text,
                color,
                font_size,
                text_flags,
            } => {
                if text.is_empty() || color.a() == 0 {
                    return Region::EMPTY;
                }
                let logical_fs = font_size.max(8);
                let char_h = logical_fs as i16;
                let line_step = (char_h.saturating_mul(5) / 4).max(char_h);
                let is_ellipsis = (text_flags & 0x04) != 0;
                let is_nowrap = (text_flags & 0x08) != 0;
                let mut b = self.bounds;

                let measure_w = |s: &str| -> i16 {
                    {
                        crate::display::measure_line_width_f32(s, logical_fs).ceil() as i16
                    }
                };

                // 统一覆盖单行/nowrap 与多行折行（含圆角父容器中溢出布局框的多行文本），零堆分配
                let (line_count, max_line_w) = if is_nowrap || is_ellipsis || self.bounds.w <= 0 {
                    (1i16, measure_w(text.as_str()))
                } else {
                    let mut count = 0i16;
                    let mut max_w = 0i16;
                    let mut rem = text.as_str();
                    while !rem.is_empty() && count < 32 {
                        let (line_slice, next_rem) =
                            crate::display::next_wrapped_line(rem, logical_fs, self.bounds.w);
                        max_w = max_w.max(measure_w(line_slice));
                        count += 1;
                        if next_rem.len() == rem.len() {
                            break;
                        }
                        rem = next_rem;
                    }
                    (count.max(1), max_w)
                };

                if !is_ellipsis {
                    // 横向：覆盖单行 HiDPI 扩展或长词超出布局框的真实绘制宽度（+~3% 或至少 4px 余量）
                    let extra_w = (max_line_w / 32).max(4);
                    let target_w = max_line_w.saturating_add(extra_w);
                    if target_w > b.w {
                        let diff_w = target_w - b.w;
                        match text_flags & 0x03 {
                            1 => b.x = b.x.saturating_sub(diff_w / 2),
                            2 => b.x = b.x.saturating_sub(diff_w),
                            _ => {}
                        }
                        b.w = target_w;
                        if b.right() > self.clip.right() {
                            let over = b.right().saturating_sub(self.clip.right());
                            b.x = b.x.saturating_sub(over).max(self.clip.x);
                        }
                    }
                    // 纵向：覆盖单行字高扩宽与多行总块高 `char_h + line_step * (lines - 1)` + ascent/descent 余量
                    let block_h = char_h
                        .saturating_add(line_step.saturating_mul(line_count.saturating_sub(1)))
                        .saturating_add((char_h / 2).max(4));
                    if block_h > b.h {
                        if line_count <= 1 {
                            let diff_h = block_h - b.h;
                            b.y = b.y.saturating_sub(diff_h / 2);
                        } else {
                            // 多行文本从 top_y = bounds.y 开始向下延伸，同时顶部预留 ascent 微量溢出余量
                            let top_pad = (char_h / 4).max(2);
                            b.y = b.y.saturating_sub(top_pad);
                        }
                        b.h = block_h.saturating_add((char_h / 4).max(2));
                    }
                } else if max_line_w <= self.bounds.w {
                    b.w = b.w.max(max_line_w.saturating_add(4));
                }

                let mut res = b.union(self.bounds);
                if self.subpx_q4 != 0 {
                    res = Region::new(
                        res.x.saturating_sub(1),
                        res.y.saturating_sub(1),
                        res.w.saturating_add(2),
                        res.h.saturating_add(2),
                    );
                }
                res.intersect(self.clip)
            }
            _ => {
                let mut res = self.bounds;
                if self.subpx_q4 != 0 {
                    res = Region::new(
                        res.x.saturating_sub(1),
                        res.y.saturating_sub(1),
                        res.w.saturating_add(2),
                        res.h.saturating_add(2),
                    );
                }
                res.intersect(self.clip)
            }
        }
    }
}

