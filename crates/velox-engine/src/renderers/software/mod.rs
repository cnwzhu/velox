//! 纯整数定点数 SDF 扫描线软件渲染器 `velox_engine::renderers::software`。
//!
//! 模块职责划分（与 `renderers::wgpu` 架构对称）：
//! - `sdf`：纯整数 SDF 距离与抗锯齿覆盖率、前向遮挡剔除与圆角/渐变/阴影矩形光栅化；
//! - `text`：多行折行、对齐与点阵/TTF 矢量文字光栅化；
//! - `sprite`：位图/RLE/旋转精灵、过程化 SDF 与纯整数解析圆弧光栅化；
//! - `compositor`：局部脏区光栅化、DMA2D 硬件加速、多线程条带并行合成与 `OffscreenCompositor`。

mod compositor;
mod sdf;
mod sprite;
mod text;

use crate::display::PixelFormat;
use crate::motion::DrawOp;
use crate::renderers::{PixelBufferRenderer, Renderer, RepaintBufferType};
use crate::solver::Region;

pub use compositor::OffscreenCompositor;
pub(crate) use compositor::rasterize_region_scaled_parallel;
pub use compositor::{
    blit_translated_layer, rasterize_parallel_threads, rasterize_region, rasterize_region_parallel,
};
pub use sdf::{
    compute_sdf_coverage, compute_unoccluded_clip, opaque_core_rect, precompute_occlusion_clips,
};
#[allow(unused_imports)]
pub(crate) use sdf::rasterize_quad;
pub(crate) use sprite::rasterize_sprite_source;
pub use text::rasterize_text;
#[cfg(feature = "gpu")]
pub(crate) use text::{align_offset_x, compute_scaled_text_box, split_text_lines};

/// 将逻辑矩形按分数/整数 `sf` 映射并对齐至物理像素边界（与 `wgpu::snap_bounds` 严格同源，锁定 1px 细线恒定物理宽）。
#[inline(always)]
pub(crate) fn snap_bounds_region_scaled(r: Region, sf: f32) -> Region {
    let x0 = ((r.x as f32) * sf).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let y0 = ((r.y as f32) * sf).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let w = if r.w == 1 {
        sf.round().max(1.0) as i16
    } else {
        let x1 = ((r.right() as f32) * sf)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        x1.saturating_sub(x0).max(0)
    };
    let h = if r.h == 1 {
        sf.round().max(1.0) as i16
    } else {
        let y1 = ((r.bottom() as f32) * sf)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        y1.saturating_sub(y0).max(0)
    };
    Region::new(x0, y0, w, h)
}

/// 将逻辑裁剪框按分数/整数 `sf` 映射并对齐至物理像素边界（与 `wgpu::snap_clip` 严格同源）。
#[inline(always)]
pub(crate) fn snap_clip_region_scaled(r: Region, sf: f32) -> Region {
    let x0 = ((r.x as f32) * sf).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let y0 = ((r.y as f32) * sf).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let x1 = ((r.right() as f32) * sf)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let y1 = ((r.bottom() as f32) * sf)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    Region::new(x0, y0, x1.saturating_sub(x0).max(0), y1.saturating_sub(y0).max(0))
}


/// 有状态软件渲染器。
#[derive(Clone, Debug)]
pub struct SoftwareRenderer {
    repaint_buffer_type: RepaintBufferType,
    pub rendered_regions_count: usize,
    pub rendered_pixels_count: usize,
}

impl Default for SoftwareRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl SoftwareRenderer {
    /// 创建默认复用缓冲策略（`RepaintBufferType::ReusedBuffer`）的软件渲染器。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            repaint_buffer_type: RepaintBufferType::ReusedBuffer,
            rendered_regions_count: 0,
            rendered_pixels_count: 0,
        }
    }

    /// 创建指定 [`RepaintBufferType`] 缓冲复用类型的软件渲染器。
    #[inline(always)]
    pub const fn new_with_repaint_buffer_type(repaint_buffer_type: RepaintBufferType) -> Self {
        Self {
            repaint_buffer_type,
            rendered_regions_count: 0,
            rendered_pixels_count: 0,
        }
    }

    /// 动态设置帧缓冲重绘复用类型。
    #[inline(always)]
    pub fn set_repaint_buffer_type(&mut self, repaint_buffer_type: RepaintBufferType) {
        self.repaint_buffer_type = repaint_buffer_type;
    }

    /// 将当前帧脏区渲染至目标像素切片 `buffer`（行跨度为 `pixel_stride`，`std` 下自动启用多核条带并行光栅化）。
    pub fn render<P: PixelFormat>(
        &mut self,
        ops: &[DrawOp<'_>],
        dirty_regions: &[Region],
        buffer: &mut [P],
        pixel_stride: usize,
    ) {
        for &reg in dirty_regions {
            rasterize_region_parallel(ops, reg, 0, 0, pixel_stride, buffer);
            self.rendered_regions_count = self.rendered_regions_count.saturating_add(1);
            self.rendered_pixels_count = self
                .rendered_pixels_count
                .saturating_add((reg.w.max(0) as usize) * (reg.h.max(0) as usize));
        }
    }

}

impl Renderer for SoftwareRenderer {
    #[inline(always)]
    fn name(&self) -> &'static str {
        "software"
    }

    #[inline(always)]
    fn repaint_buffer_type(&self) -> RepaintBufferType {
        self.repaint_buffer_type
    }

    fn render_draw_ops(
        &mut self,
        _ops: &[DrawOp<'_>],
        _viewport: Region,
        _dpi_scale: i16,
        dirty_regions: &[Region],
    ) -> (usize, usize) {
        let mut px = 0usize;
        for r in dirty_regions {
            px += (r.w.max(0) as usize) * (r.h.max(0) as usize);
        }
        self.rendered_regions_count += dirty_regions.len();
        self.rendered_pixels_count += px;
        (dirty_regions.len(), px)
    }
}

impl PixelBufferRenderer for SoftwareRenderer {
    #[inline]
    fn render_region_to_buffer<P: PixelFormat>(
        &mut self,
        ops: &[DrawOp<'_>],
        dirty_region: Region,
        origin_x: i16,
        origin_y: i16,
        stride: usize,
        target: &mut [P],
    ) {
        rasterize_region_parallel(ops, dirty_region, origin_x, origin_y, stride, target);
        self.rendered_regions_count = self.rendered_regions_count.saturating_add(1);
        self.rendered_pixels_count = self
            .rendered_pixels_count
            .saturating_add((dirty_region.w.max(0) as usize) * (dirty_region.h.max(0) as usize));
    }
}

