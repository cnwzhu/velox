//! 跨后端渲染器抽象层 `velox_core::renderers`。
//!
//! 将 **平台窗口/事件层（[`crate::platform`]）** 与 **图形光栅化/GPU 渲染层（`renderers`）** 彻底解耦，
//! 原生内聚两大核心渲染后端，并通过 [`Renderer`]、[`PixelBufferRenderer`] 与 [`LoweredPrimitiveSink`]
//! 面向外部自定义渲染后端开放扩展：
//! - [`software`]（始终内置）：多核并行 SDF 扫描线软光栅渲染器，支持前向遮挡剔除（Front-to-Back Culling）与局部脏区重绘；
//! - `wgpu`（`gpu` 特性默认启用）：自研轻量级 `wgpu` 2D SDF GPU 硬件加速渲染器（单 RenderPass / 单 Draw Call 合批 + 动态 `R8Unorm` 字形图集与惰性 `Rgba8Unorm` 精灵图集，零重型路径计算缓冲）。

use crate::display::{PixelFormat, SpriteFormat};
use crate::element::SpriteSource;
use crate::engine::VeloxEngine;
use crate::motion::{DrawOp, DrawPayload};
use crate::solver::Region;
use crate::style::Color;

pub mod software;
#[cfg(feature = "gpu")]
pub mod wgpu;

use self::software::compute_unoccluded_clip;

/// 平台帧缓冲重绘复用类型。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum RepaintBufferType {
    /// 每一帧分配或写入全新缓冲区（需整屏重绘）。
    #[default]
    NewBuffer,
    /// 原地复用同一个帧缓冲区（仅需光栅化当前帧脏区 `curr_damage`）。
    ReusedBuffer,
    /// 前后双缓冲交替交换（Buffer Age = 2，需光栅化 `curr_damage ∪ prev_damage`）。
    SwappedBuffers,
}

/// 跨后端统一降维 2D 矢量图元接收器接口（供 [`lower_draw_ops`] 公共降维管道回调）。
pub trait LoweredPrimitiveSink {
    /// 进入一个脏矩形裁剪区（`scaled_dirty` 已乘以 `dpi_scale`）。
    fn begin_dirty_clip(&mut self, scaled_dirty: Region);

    /// 退出当前脏矩形裁剪区。
    fn end_dirty_clip(&mut self);

    /// 进入单个图元的精确有效裁剪区（含父容器 `clip_children` 与前向遮挡剔除裁剪）。
    fn begin_element_clip(&mut self, _scaled_clip: Region) {}

    /// 退出单个图元的精确有效裁剪区。
    fn end_element_clip(&mut self) {}

    /// 填充直角或圆角矩形（`corner_radius == 0` 时严格保持直角）。
    fn fill_rect(
        &mut self,
        rect: Region,
        corner_radius: u8,
        start_color: Color,
        end_color: Color,
        horizontal_grad: bool,
    );

    /// 绘制高斯/SDF 软阴影（默认降级为半透明圆角矩形填充）。
    fn fill_soft_shadow(
        &mut self,
        rect: Region,
        corner_radius: u8,
        blur_radius: u8,
        color: Color,
    ) {
        let eff_r = corner_radius.saturating_add(blur_radius / 2);
        let soft_col = color.scale_alpha(128);
        if soft_col.a() > 0 {
            self.fill_rect(rect, eff_r, soft_col, soft_col, false);
        }
    }

    /// 描边直角或圆角矩形边框。
    fn stroke_rect(&mut self, rect: Region, corner_radius: u8, stroke_width: u8, color: Color);

    /// 绘制单条文本运行簇。
    fn draw_text(
        &mut self,
        bounds: Region,
        clip: Region,
        text: &str,
        color: Color,
        font_size: u8,
        text_flags: u8,
    );

    /// 绘制位图精灵或自定义 SDF 着色器画布。
    fn draw_sprite(&mut self, bounds: Region, clip: Region, sprite: SpriteSource<'_>) {
        match sprite {
            SpriteSource::Bitmap {
                bytes,
                format,
                tint,
            } => {
                self.draw_bitmap(bounds, clip, format, tint, bytes);
            }
            SpriteSource::Shader { .. }
            | SpriteSource::Svg { .. }
            | SpriteSource::Arc { .. } => {
                let eff_clip = clip.intersect(bounds);
                if bounds.w <= 0 || bounds.h <= 0 || eff_clip.is_empty() {
                    return;
                }
                let w = bounds.w as usize;
                let h = bounds.h as usize;
                let mut pixels = std::vec![crate::display::Rgba8888(0); w * h];
                self::software::rasterize_sprite_source(
                    bounds,
                    eff_clip,
                    sprite,
                    bounds.x,
                    bounds.y,
                    w,
                    &mut pixels,
                );
                let mut bytes = std::vec::Vec::with_capacity(w * h * 4);
                for px in pixels {
                    bytes.extend_from_slice(&px.to_rgba_bytes());
                }
                self.draw_bitmap(
                    bounds,
                    clip,
                    SpriteFormat::Rgba8888,
                    Color::WHITE,
                    &bytes,
                );
            }
        }
    }

    /// 绘制位图精灵纹理。
    fn draw_bitmap(
        &mut self,
        _bounds: Region,
        _clip: Region,
        _format: SpriteFormat,
        _tint: Color,
        _bytes: &[u8],
    ) {
    }
}

/// 统一将扁平化 [`DrawOp`] 流与脏矩形列表 `dirty_regions`（含前向遮挡剔除与 HiDPI 缩放）
/// 降维分发至 [`LoweredPrimitiveSink`]，消除各渲染器后端的重复遍历代码。
///
/// 返回 `(dirty_regions.len(), total_logical_pixels)`。
pub fn lower_draw_ops<S: LoweredPrimitiveSink + ?Sized>(
    ops: &[DrawOp<'_>],
    dpi_scale: i16,
    dirty_regions: &[Region],
    sink: &mut S,
) -> (usize, usize) {
    if dirty_regions.is_empty() {
        return (0, 0);
    }

    let s = dpi_scale.max(1);
    let mut total_pixels = 0usize;

    for &dirty in dirty_regions {
        if dirty.is_empty() {
            continue;
        }
        total_pixels += (dirty.w.max(0) as usize) * (dirty.h.max(0) as usize);
        let scaled_dirty = Region::new(dirty.x * s, dirty.y * s, dirty.w * s, dirty.h * s);
        sink.begin_dirty_clip(scaled_dirty);

        for (idx, op) in ops.iter().enumerate() {
            let raw_clip = dirty.intersect(op.clip).intersect(op.bounds);
            if raw_clip.is_empty() {
                continue;
            }
            let eff_clip = compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..]);
            if eff_clip.is_empty() {
                continue;
            }

            let scaled_bounds = Region::new(
                op.bounds.x * s,
                op.bounds.y * s,
                op.bounds.w * s,
                op.bounds.h * s,
            );
            let scaled_clip = Region::new(
                eff_clip.x * s,
                eff_clip.y * s,
                eff_clip.w * s,
                eff_clip.h * s,
            );

            let needs_elem_clip = scaled_clip != scaled_dirty.intersect(scaled_bounds);
            if needs_elem_clip {
                sink.begin_element_clip(scaled_clip);
            }

            match op.payload {
                DrawPayload::Quad {
                    bg,
                    bg_grad,
                    border_color,
                    rounded,
                    border_width,
                } => {
                    let bw = (border_width & 0x7F).saturating_mul(s as u8);
                    let horiz = (border_width & 0x80) != 0;
                    let r = if rounded == 0 {
                        0
                    } else {
                        rounded.saturating_mul(s as u8)
                    };
                    let is_soft_shadow = border_color.a() == 0 && bw > 0;

                    if is_soft_shadow {
                        if bg.a() > 0 {
                            sink.fill_soft_shadow(scaled_bounds, r, bw, bg);
                        }
                    } else {
                        if bg.a() > 0 || bg_grad.a() > 0 {
                            sink.fill_rect(scaled_bounds, r, bg, bg_grad, horiz);
                        }
                        if bw > 0 && border_color.a() > 0 {
                            sink.stroke_rect(scaled_bounds, r, bw, border_color);
                        }
                    }
                }
                DrawPayload::TextRun {
                    text,
                    color,
                    font_size,
                    text_flags,
                } => {
                    sink.draw_text(
                        scaled_bounds,
                        scaled_clip,
                        text.as_str(),
                        color,
                        font_size.saturating_mul(s as u8),
                        text_flags,
                    );
                }
                DrawPayload::Sprite(sprite) => {
                    sink.draw_sprite(scaled_bounds, scaled_clip, sprite);
                }
            }

            if needs_elem_clip {
                sink.end_element_clip();
            }
        }

        sink.end_dirty_clip();
    }

    (dirty_regions.len(), total_pixels)
}

/// 核心渲染器统一抽象接口。
///
/// 任何实现了 [`Renderer`] 的后端（`SoftwareRenderer`、`WgpuOffscreenRenderer` 或自定义外部后端）
/// 均可直接对接 [`VeloxEngine`] 输出的扁平化绘制指令流 `&[DrawOp]` 与脏矩形列表 `&[Region]`。
pub trait Renderer {
    /// 渲染器后端名称标识（如 `"software"`、`"wgpu"`）。
    fn name(&self) -> &'static str;

    /// 获取或配置当前渲染器的帧缓冲重绘复用策略（`NewBuffer` / `ReusedBuffer` / `SwappedBuffers`）。
    fn repaint_buffer_type(&self) -> RepaintBufferType {
        RepaintBufferType::ReusedBuffer
    }

    /// 针对给定的扁平化绘制指令切片 `ops` 与本帧脏矩形列表 `dirty_regions` 执行一帧渲染。
    ///
    /// 返回本帧实际绘制/提交的脏矩形数量与像素面积统计 `(dirty_rect_count, total_pixels)`。
    fn render_draw_ops(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        dpi_scale: i16,
        dirty_regions: &[Region],
    ) -> (usize, usize);

    /// 驱动 [`VeloxEngine`] 步进 `dt_ms` 并使用当前渲染器渲染本帧所有脏矩形。
    fn render_frame_dt(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> (usize, usize) {
        engine.render_with_renderer(self, dt_ms)
    }
}

/// 软光栅像素缓冲区渲染能力扩展接口。
pub trait PixelBufferRenderer: Renderer {
    /// 将 [`VeloxEngine`] 当前帧的指定局部区域 `dirty_region` 光栅化至具有 `(origin_x, origin_y, stride)` 的目标像素切片 `target`。
    fn render_region_to_buffer<P: PixelFormat>(
        &mut self,
        ops: &[DrawOp<'_>],
        dirty_region: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    );
}
