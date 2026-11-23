//! `wgpu` 离屏渲染与像素回读渲染器（无硬件 GPU 时自动无缝回退 `software` 软光栅）。

use ::wgpu;
use std::vec;
use std::vec::Vec;

use crate::display::Rgba8888;
use crate::engine::VeloxEngine;
use crate::motion::DrawOp;
use crate::renderers::software::rasterize_region_scaled_parallel;
use crate::renderers::{Renderer, RepaintBufferType};
use crate::solver::Region;

use super::pipeline::WgpuRenderer;
use super::{block_on, create_wgpu_instance, WgpuScene};

struct OffscreenGpuState {
    renderer: WgpuRenderer,
    target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,
    readback_buf: wgpu::Buffer,
    padded_bytes_per_row: u32,
    width: u32,
    height: u32,
    queue: wgpu::Queue,
    device: wgpu::Device,
    _instance: wgpu::Instance,
}

impl OffscreenGpuState {
    fn create_targets(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::Buffer, u32) {
        let w = width.max(1);
        let h = height.max(1);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_offscreen_target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let unpadded = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_offscreen_readback"),
            size: (padded as u64) * (h as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        (tex, view, readback, padded)
    }

    fn try_new_with_backends(width: u32, height: u32, backends: wgpu::Backends) -> Option<Self> {
        let instance = create_wgpu_instance(backends);
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .ok()?;
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return None;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("velox_offscreen_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
        }))
        .ok()?;

        let w = width.max(1);
        let h = height.max(1);
        let (target_texture, target_view, readback_buf, padded_bytes_per_row) =
            Self::create_targets(&device, w, h);
        let renderer = WgpuRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);

        Some(Self {
            renderer,
            target_texture,
            target_view,
            readback_buf,
            padded_bytes_per_row,
            width: w,
            height: h,
            queue,
            device,
            _instance: instance,
        })
    }

    fn new(width: u32, height: u32) -> Option<Self> {
        if let Some(env_backends) = wgpu::Backends::from_env() {
            return Self::try_new_with_backends(width, height, env_backends);
        }
        #[cfg(target_os = "windows")]
        {
            if let Some(state) = Self::try_new_with_backends(width, height, wgpu::Backends::DX12) {
                return Some(state);
            }
            Self::try_new_with_backends(width, height, wgpu::Backends::VULKAN)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self::try_new_with_backends(width, height, wgpu::Backends::PRIMARY)
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return;
        }
        let (tex, view, rb, padded) = Self::create_targets(&self.device, w, h);
        self.target_texture = tex;
        self.target_view = view;
        self.readback_buf = rb;
        self.padded_bytes_per_row = padded;
        self.width = w;
        self.height = h;
    }

    fn render_rgba(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        out_rgba: &mut [u8],
    ) -> bool {
        self.renderer.render_to_view(
            &self.device,
            &self.queue,
            ops,
            viewport,
            scale_factor.clamp(0.5, 4.0),
            &self.target_view,
            self.width,
            self.height,
            wgpu::Color::TRANSPARENT,
        );
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("velox_offscreen_copy"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);

        let slice = self.readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        if rx.recv().ok().and_then(|r| r.ok()).is_none() {
            return false;
        }
        {
            let Ok(mapped) = slice.get_mapped_range() else {
                self.readback_buf.unmap();
                return false;
            };
            let row_bytes = (self.width as usize) * 4;
            let stride = self.padded_bytes_per_row as usize;
            for y in 0..(self.height as usize) {
                let src = &mapped[y * stride..y * stride + row_bytes];
                let dst_start = y * row_bytes;
                if dst_start + row_bytes <= out_rgba.len() {
                    out_rgba[dst_start..dst_start + row_bytes].copy_from_slice(src);
                }
            }
        }
        self.readback_buf.unmap();
        true
    }
}

/// 面向移动端离屏缓冲与无头集成测试的轻量级 `wgpu` 离屏渲染器。
///
/// 默认惰性按需探测硬件 GPU（可显式调用 [`Self::ensure_gpu`]），在无 GPU 或 CI 无头环境中
/// 自动无缝回退至 Velox 内置 [`crate::renderers::software`] 光栅器产出 `rgba_buffer`。
pub struct WgpuOffscreenRenderer {
    pub scene: WgpuScene,
    pub rgba_buffer: Vec<u8>,
    pub width: usize,
    pub height: usize,
    fallback_pixmap: Vec<Rgba8888>,
    gpu: Option<OffscreenGpuState>,
    gpu_probed: bool,
}

impl WgpuOffscreenRenderer {
    /// 创建指定像素尺寸的离屏渲染器。
    pub fn new(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            scene: WgpuScene::new(),
            rgba_buffer: vec![0u8; w * h * 4],
            width: w,
            height: h,
            fallback_pixmap: vec![Rgba8888(0); w * h],
            gpu: None,
            gpu_probed: false,
        }
    }

    /// 尝试按需初始化硬件 `wgpu` 离屏渲染管线；若宿主存在可用硬件 GPU 返回 `true`。
    pub fn ensure_gpu(&mut self) -> bool {
        if !self.gpu_probed {
            self.gpu_probed = true;
            self.gpu = OffscreenGpuState::new(self.width as u32, self.height as u32);
        }
        self.gpu.is_some()
    }

    /// 返回当前帧编译出的图元总数。
    #[inline]
    pub fn primitive_count(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.last_primitive_count())
            .unwrap_or_else(|| {
                self.scene.primitives.len() + self.scene.text_run_count + self.scene.sprite_count
            })
    }

    /// 返回底层 `WgpuRenderer` 跨帧文本行排版缓存（`TextLayoutCache`）累计命中次数。
    #[inline]
    pub fn text_layout_cache_hits(&self) -> u64 {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.text_layout_cache_hits())
            .unwrap_or(0)
    }

    /// 返回底层 `WgpuRenderer` 静态精灵跨帧驻留缓存累计命中次数。
    #[inline]
    pub fn sprite_cache_hits(&self) -> u64 {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.sprite_cache_hits())
            .unwrap_or(0)
    }

    fn prepare_size(&mut self, width: usize, height: usize) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width != w || self.height != h {
            self.width = w;
            self.height = h;
            self.rgba_buffer.resize(w * h * 4, 0);
            self.fallback_pixmap.resize(w * h, Rgba8888(0));
            if let Some(gpu) = &mut self.gpu {
                gpu.resize(w as u32, h as u32);
            }
        }
    }

    /// 以任意浮点分数 DPI 缩放因子 `scale_factor`（如 `1.25`、`1.5`、`2.0`）执行硬件 `wgpu` 离屏渲染，
    /// 成功完成硬件 GPU 渲染与像素回读时返回 `true`。
    pub fn render_draw_ops_scaled(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
    ) -> bool {
        let sf = scale_factor.clamp(0.5, 4.0);
        let pw = ((viewport.w.max(1) as f32) * sf).round().max(1.0) as usize;
        let ph = ((viewport.h.max(1) as f32) * sf).round().max(1.0) as usize;
        self.prepare_size(pw, ph);
        if let Some(gpu) = &mut self.gpu {
            return gpu.render_rgba(ops, viewport, sf, &mut self.rgba_buffer);
        }
        false
    }

    fn rasterize_software_fallback(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        sf: f32,
        dirty_regions: &[Region],
    ) {
        for &reg in dirty_regions {
            if !reg.is_empty() {
                rasterize_region_scaled_parallel(
                    ops,
                    viewport,
                    sf,
                    reg,
                    0,
                    0,
                    self.width,
                    &mut self.fallback_pixmap,
                );
            }
        }
    }
}

impl Renderer for WgpuOffscreenRenderer {
    #[inline]
    fn name(&self) -> &'static str {
        "wgpu"
    }

    #[inline]
    fn repaint_buffer_type(&self) -> RepaintBufferType {
        RepaintBufferType::ReusedBuffer
    }

    fn render_draw_ops(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        dpi_scale: i16,
        dirty_regions: &[Region],
    ) -> (usize, usize) {
        if dirty_regions.is_empty() {
            return (0, 0);
        }
        let s = dpi_scale.max(1);
        let pw = (viewport.w.max(1) as usize).saturating_mul(s as usize);
        let ph = (viewport.h.max(1) as usize).saturating_mul(s as usize);
        self.prepare_size(pw, ph);
        let _ = self.scene.record_draw_ops(ops, s, dirty_regions);

        let mut total_pixels = 0usize;
        for &r in dirty_regions {
            total_pixels += (r.w.max(0) as usize) * (r.h.max(0) as usize);
        }

        if let Some(gpu) = &mut self.gpu {
            if gpu.render_rgba(ops, viewport, s as f32, &mut self.rgba_buffer) {
                return (dirty_regions.len(), total_pixels);
            }
        }

        self.rasterize_software_fallback(ops, viewport, s as f32, dirty_regions);
        for (dst, px) in self
            .rgba_buffer
            .chunks_exact_mut(4)
            .zip(self.fallback_pixmap.iter())
        {
            dst.copy_from_slice(&px.to_rgba_bytes());
        }
        (dirty_regions.len(), total_pixels)
    }

    fn render_frame_dt(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> (usize, usize) {
        let mut dirty: crate::arena::SmallVec<Region, 8> = crate::arena::SmallVec::new();
        for &r in engine.step_frame_dt(dt_ms) {
            dirty.push(r);
        }
        if dirty.is_empty() {
            return (0, 0);
        }
        let viewport = engine.viewport_region();
        let dpi_scale = engine.dpi_scale();
        let ops = engine.cached_draw_ops();
        self.render_draw_ops(ops, viewport, dpi_scale, dirty.as_slice())
    }
}
