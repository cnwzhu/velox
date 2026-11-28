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
    has_valid_gpu_frame: bool,
    last_readback_bytes: usize,
    queue: wgpu::Queue,
    device: wgpu::Device,
    _instance: Option<wgpu::Instance>,
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
        let (device, queue, pso_cache) = super::pso_cache::request_device_with_pso_cache(
            &adapter,
            "velox_offscreen_device",
            wgpu::Limits::default(),
        )?;

        let w = width.max(1);
        let h = height.max(1);
        let (target_texture, target_view, readback_buf, padded_bytes_per_row) =
            Self::create_targets(&device, w, h);
        let renderer = WgpuRenderer::new_with_cache(&device, wgpu::TextureFormat::Rgba8Unorm, pso_cache);

        Some(Self {
            renderer,
            target_texture,
            target_view,
            readback_buf,
            padded_bytes_per_row,
            width: w,
            height: h,
            has_valid_gpu_frame: false,
            last_readback_bytes: 0,
            queue,
            device,
            _instance: Some(instance),
        })
    }

    fn from_shared(device: wgpu::Device, queue: wgpu::Queue, width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let (target_texture, target_view, readback_buf, padded_bytes_per_row) =
            Self::create_targets(&device, w, h);
        let renderer = WgpuRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        Self {
            renderer,
            target_texture,
            target_view,
            readback_buf,
            padded_bytes_per_row,
            width: w,
            height: h,
            has_valid_gpu_frame: false,
            last_readback_bytes: 0,
            queue,
            device,
            _instance: None,
        }
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
        self.has_valid_gpu_frame = false;
        self.renderer.on_viewport_resized(w, h);
    }

    fn readback_subrect(&mut self, rx: u32, ry: u32, rw: u32, rh: u32, out_rgba: &mut [u8]) -> bool {
        if rw == 0 || rh == 0 {
            self.last_readback_bytes = 0;
            return true;
        }
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let row_bytes = (rw as usize) * 4;
        let sub_padded = (rw * 4).div_ceil(align) * align;
        let copy_total_bytes = (sub_padded as u64) * (rh as u64);

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("velox_offscreen_copy_subrect"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: rx, y: ry, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(sub_padded),
                    rows_per_image: Some(rh),
                },
            },
            wgpu::Extent3d {
                width: rw,
                height: rh,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);

        let slice = self.readback_buf.slice(0..copy_total_bytes);
        let (tx, rx_ch) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        if rx_ch.recv().ok().and_then(|r| r.ok()).is_none() {
            return false;
        }
        {
            let Ok(mapped) = slice.get_mapped_range() else {
                self.readback_buf.unmap();
                return false;
            };
            let stride = sub_padded as usize;
            let dst_full_stride = (self.width as usize) * 4;
            let dst_x_off = (rx as usize) * 4;
            for y in 0..(rh as usize) {
                let src = &mapped[y * stride..y * stride + row_bytes];
                let dst_start = ((ry as usize) + y) * dst_full_stride + dst_x_off;
                if dst_start + row_bytes <= out_rgba.len() {
                    out_rgba[dst_start..dst_start + row_bytes].copy_from_slice(src);
                }
            }
        }
        self.readback_buf.unmap();
        self.last_readback_bytes = copy_total_bytes as usize;
        true
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
        let ok = self.readback_subrect(0, 0, self.width, self.height, out_rgba);
        if ok {
            self.has_valid_gpu_frame = true;
        }
        ok
    }

    /// 局部脏区 GPU 渲染与局部子矩形纹理回读：仅在已有有效首帧且脏区包围盒小于整屏 70% 时启用，
    /// 避免 1080p 下小范围动画/Hover 仍每帧拷贝整张 7.91 MiB 纹理。
    fn render_rgba_dirty(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        dirty_regions: &[Region],
        out_rgba: &mut [u8],
    ) -> bool {
        let sf = scale_factor.clamp(0.5, 4.0);
        if !self.has_valid_gpu_frame || dirty_regions.is_empty() {
            return self.render_rgba(ops, viewport, sf, out_rgba);
        }

        // 计算脏矩形集合在物理像素上的合并最小包围盒（外扩 1px 抗锯齿余量）
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        for &d in dirty_regions {
            let clipped = d.intersect(viewport);
            if clipped.is_empty() {
                continue;
            }
            let x0 = (((clipped.x.max(0) as f32) * sf).floor() as u32).saturating_sub(1);
            let y0 = (((clipped.y.max(0) as f32) * sf).floor() as u32).saturating_sub(1);
            let x1 = (((clipped.right().max(0) as f32) * sf).ceil() as u32)
                .saturating_add(1)
                .min(self.width);
            let y1 = (((clipped.bottom().max(0) as f32) * sf).ceil() as u32)
                .saturating_add(1)
                .min(self.height);
            if x1 > x0 && y1 > y0 {
                min_x = min_x.min(x0);
                min_y = min_y.min(y0);
                max_x = max_x.max(x1);
                max_y = max_y.max(y1);
            }
        }
        if min_x >= max_x || min_y >= max_y {
            self.last_readback_bytes = 0;
            return true;
        }
        let rw = max_x - min_x;
        let rh = max_y - min_y;
        let full_area = (self.width as u64) * (self.height as u64);
        let sub_area = (rw as u64) * (rh as u64);
        if sub_area * 10 >= full_area * 7 {
            return self.render_rgba(ops, viewport, sf, out_rgba);
        }

        self.renderer.render_to_view_dirty(
            &self.device,
            &self.queue,
            ops,
            viewport,
            sf,
            &self.target_view,
            self.width,
            self.height,
            dirty_regions,
            wgpu::Color::TRANSPARENT,
        );
        self.readback_subrect(min_x, min_y, rw, rh, out_rgba)
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
    ///
    /// 软件回退缓冲 `fallback_pixmap` 按需懒分配，GPU 路径下不会占用多余的整屏 CPU 内存。
    pub fn new(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            scene: WgpuScene::new(),
            rgba_buffer: vec![0u8; w * h * 4],
            width: w,
            height: h,
            fallback_pixmap: Vec::new(),
            gpu: None,
            gpu_probed: false,
        }
    }

    /// 创建零预分配 CPU 像素副本的离屏渲染器（配合 [`Self::render_draw_ops_rgba8888_into`] / [`Self::render_draw_ops_into`] 直接写入调用方目标缓冲）。
    pub fn new_lazy(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            scene: WgpuScene::new(),
            rgba_buffer: Vec::new(),
            width: w,
            height: h,
            fallback_pixmap: Vec::new(),
            gpu: None,
            gpu_probed: false,
        }
    }

    /// 尝试按需初始化硬件 `wgpu` 离屏渲染管线；若宿主存在可用硬件 GPU 返回 `true` 并释放闲置软件回退缓冲。
    pub fn ensure_gpu(&mut self) -> bool {
        if !self.gpu_probed {
            self.gpu_probed = true;
            self.gpu = OffscreenGpuState::new(self.width as u32, self.height as u32);
        }
        if self.gpu.is_some() && !self.fallback_pixmap.is_empty() {
            self.fallback_pixmap = Vec::new();
        }
        self.gpu.is_some()
    }

    /// 复用外部引擎已有的 `wgpu::Device` 与 `wgpu::Queue` 创建离屏渲染器（实现同设备零拷贝外部纹理导入测试与离屏合成）。
    pub fn from_shared_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: usize,
        height: usize,
    ) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            scene: WgpuScene::new(),
            rgba_buffer: vec![0u8; w * h * 4],
            width: w,
            height: h,
            fallback_pixmap: Vec::new(),
            gpu: Some(OffscreenGpuState::from_shared(device, queue, w as u32, h as u32)),
            gpu_probed: true,
        }
    }

    /// 返回底层 `wgpu::Device` 引用（若 GPU 已就绪）。
    #[inline]
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.gpu.as_ref().map(|g| &g.device)
    }

    /// 返回底层 `wgpu::Queue` 引用（若 GPU 已就绪）。
    #[inline]
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.gpu.as_ref().map(|g| &g.queue)
    }

    /// 返回内部 `WgpuRenderer` 的完整 GPU/CPU 子池内存统计快照（若 GPU 已就绪）。
    #[inline]
    pub fn gpu_memory_stats(&self) -> Option<super::WgpuMemoryStats> {
        self.gpu.as_ref().map(|g| g.renderer.memory_stats())
    }

    /// 配置全局 UI `text()` 自动路由至 Slug 2.0 矢量字形的物理字号阈值（默认 `28.0 px`）。
    ///
    /// - 默认 `28.0`：大字号标题（`>= 28px`）走零图集 Slug 曲线渲染，小字号正文走 `GlyphAtlas`；
    /// - 设为 `0.0`：全部常规 UI `text()` 走 Slug 矢量渲染（可用于 A/B 清晰度与性能对比）；
    /// - 设为 `f32::INFINITY`：全部回退至位图图集。
    #[inline]
    pub fn set_slug_text_threshold_px(&mut self, threshold_px: f32) {
        if let Some(g) = &mut self.gpu {
            g.renderer.set_slug_text_threshold_px(threshold_px);
        }
    }

    /// 查询当前全局 UI `text()` 路由至 Slug 2.0 矢量字形的物理字号阈值（像素）。
    #[inline]
    pub fn slug_text_threshold_px(&self) -> f32 {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.slug_text_threshold_px())
            .unwrap_or(super::pipeline::DEFAULT_SLUG_TEXT_THRESHOLD_PX)
    }

    /// 显式启动后台子管线（Effects / Canvas）异步预热线程。
    #[inline]
    pub fn spawn_background_pipeline_warmup(&mut self) {
        if let Some(g) = &mut self.gpu {
            g.renderer.spawn_background_pipeline_warmup(&g.device, &g.queue);
        }
    }

    /// 等待后台子管线预热线程完成并收割已编译子引擎。
    #[inline]
    pub fn wait_background_pipeline_warmup(&mut self) {
        if let Some(g) = &mut self.gpu {
            g.renderer.wait_background_pipeline_warmup();
        }
    }

    /// 返回当前离屏 GPU 渲染器是否成功从磁盘加载了二进制 PSO 缓存。
    #[inline]
    pub fn pso_cache_loaded_from_disk(&self) -> bool {
        self.gpu
            .as_ref()
            .is_some_and(|g| g.renderer.pso_cache_loaded_from_disk())
    }

    /// 估算离屏渲染目标纹理与回读缓冲本身占用的 GPU/映射字节。
    #[inline]
    pub fn offscreen_target_gpu_bytes(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| {
                let target = (g.width as usize) * (g.height as usize) * 4;
                let readback = (g.padded_bytes_per_row as usize) * (g.height as usize);
                target + readback
            })
            .unwrap_or(0)
    }

    /// 向离屏渲染器注册外部零拷贝 GPU 纹理（如海图/视频/3D 视口）。
    #[inline]
    pub fn register_external_texture(
        &mut self,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) -> Option<crate::paint::ExternalTextureId> {
        self.gpu
            .as_mut()
            .map(|g| g.renderer.register_external_texture(view, sampler))
    }

    /// 按显式 ID 注册或替换外部零拷贝 GPU 纹理。
    #[inline]
    pub fn register_external_texture_with_id(
        &mut self,
        id: crate::paint::ExternalTextureId,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) -> bool {
        if let Some(g) = &mut self.gpu {
            g.renderer.register_external_texture_with_id(id, view, sampler);
            true
        } else {
            false
        }
    }

    /// 更新已注册的外部零拷贝 GPU 纹理视图。
    #[inline]
    pub fn update_external_texture(
        &mut self,
        id: crate::paint::ExternalTextureId,
        view: wgpu::TextureView,
    ) -> bool {
        self.gpu
            .as_mut()
            .is_some_and(|g| g.renderer.update_external_texture(id, view))
    }

    /// 注销外部零拷贝 GPU 纹理。
    #[inline]
    pub fn unregister_external_texture(&mut self, id: crate::paint::ExternalTextureId) -> bool {
        self.gpu
            .as_mut()
            .is_some_and(|g| g.renderer.unregister_external_texture(id))
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

    /// 返回底层离屏图层纹理池当前占用的估算 GPU 显存字节数。
    #[inline]
    pub fn layer_pool_bytes(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.layer_pool_bytes())
            .unwrap_or(0)
    }

    /// 返回底层离屏图层纹理池当前持有的纹理数量。
    #[inline]
    pub fn layer_pool_target_count(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.layer_pool_target_count())
            .unwrap_or(0)
    }

    /// 返回底层 SVG Path 遮罩缓存占用的估算 GPU 显存字节数。
    #[inline]
    pub fn layer_mask_cache_bytes(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.layer_mask_cache_bytes())
            .unwrap_or(0)
    }

    /// 配置离屏图层纹理池的空闲字节预算（超出时在帧末按 LRU 立即回收）。
    #[inline]
    pub fn set_layer_pool_idle_budget(&mut self, bytes: usize) {
        if let Some(g) = &mut self.gpu {
            g.renderer.set_layer_pool_idle_budget(bytes);
        }
    }

    /// 配置离屏图层纹理池的最大闲置帧数（切回普通页超过该帧数后自动回收）。
    #[inline]
    pub fn set_layer_pool_idle_max_age_frames(&mut self, frames: u64) {
        if let Some(g) = &mut self.gpu {
            g.renderer.set_layer_pool_idle_max_age_frames(frames);
        }
    }

    /// 配置 SVG Path 遮罩缓存的显存字节预算。
    #[inline]
    pub fn set_layer_mask_cache_budget(&mut self, bytes: usize) {
        if let Some(g) = &mut self.gpu {
            g.renderer.set_layer_mask_cache_budget(bytes);
        }
    }

    /// 主动回收全部空闲 GPU 离屏纹理与 SVG 遮罩缓存。
    #[inline]
    pub fn trim_gpu_memory(&mut self) {
        if let Some(g) = &mut self.gpu {
            g.renderer.trim_gpu_memory();
        }
    }

    /// 释放内部冗余的 CPU 侧像素缓冲（`rgba_buffer` 与 `fallback_pixmap`），并标记下次 GPU 回读需全帧同步。
    #[inline]
    pub fn trim_cpu_buffers(&mut self) {
        self.rgba_buffer = Vec::new();
        self.fallback_pixmap = Vec::new();
        self.invalidate_readback_cache();
    }

    /// 标记底层离屏纹理增量回读状态失效，强制下一帧执行完整整屏回读（如切换/清空外部输出缓冲后调用）。
    #[inline]
    pub fn invalidate_readback_cache(&mut self) {
        if let Some(g) = &mut self.gpu {
            g.has_valid_gpu_frame = false;
        }
    }

    /// 返回最近一次 GPU 离屏回读实际拷贝的字节数（局部脏区回读时远低于整屏 7.91 MiB）。
    #[inline]
    pub fn last_readback_bytes(&self) -> usize {
        self.gpu.as_ref().map(|g| g.last_readback_bytes).unwrap_or(0)
    }

    /// 返回底层 `WgpuRenderer` 最近一帧增量上传的实例缓冲字节数。
    #[inline]
    pub fn last_uploaded_instance_bytes(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.last_uploaded_instance_bytes())
            .unwrap_or(0)
    }

    /// 显式移除并释放指定 `SceneCanvas` 场景的 GPU 显存缓冲。
    #[inline]
    pub fn remove_canvas_scene(&mut self, scene_id: crate::canvas_scene::StaticSceneId) -> bool {
        self.gpu
            .as_mut()
            .is_some_and(|g| g.renderer.remove_canvas_scene(scene_id))
    }

    /// 返回底层 `CanvasGpuEngine` 当前缓存的 GPU 显存总字节数。
    #[inline]
    pub fn canvas_cache_bytes(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.canvas_cache_bytes())
            .unwrap_or(0)
    }

    /// 返回底层 `CanvasGpuEngine` 当前缓存的场景数。
    #[inline]
    pub fn canvas_scene_count(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.canvas_scene_count())
            .unwrap_or(0)
    }

    /// 返回最近一次 `SceneCanvas` 动态状态增量上传字节数。
    #[inline]
    pub fn canvas_last_state_upload_bytes(&self) -> usize {
        self.gpu
            .as_ref()
            .map(|g| g.renderer.canvas_last_state_upload_bytes())
            .unwrap_or(0)
    }

    /// 配置底层 `CanvasGpuEngine` 空闲场景显存预算。
    #[inline]
    pub fn set_canvas_idle_budget(&mut self, bytes: usize) {
        if let Some(g) = &mut self.gpu {
            g.renderer.set_canvas_idle_budget(bytes);
        }
    }

    /// 配置底层 `CanvasGpuEngine` 空闲场景最大存活帧数。
    #[inline]
    pub fn set_canvas_idle_max_age_frames(&mut self, frames: u64) {
        if let Some(g) = &mut self.gpu {
            g.renderer.set_canvas_idle_max_age_frames(frames);
        }
    }

    /// 返回当前离屏渲染器在 CPU 侧持有的像素缓冲总容量（字节）。
    #[inline]
    pub fn cpu_memory_bytes(&self) -> usize {
        self.rgba_buffer.capacity() + self.fallback_pixmap.capacity() * core::mem::size_of::<Rgba8888>()
    }

    fn prepare_dimensions(&mut self, width: usize, height: usize) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width != w || self.height != h {
            self.width = w;
            self.height = h;
            if let Some(gpu) = &mut self.gpu {
                gpu.resize(w as u32, h as u32);
            }
            if !self.fallback_pixmap.is_empty() {
                self.fallback_pixmap.resize(w * h, Rgba8888(0));
            }
        }
    }

    fn prepare_size(&mut self, width: usize, height: usize) {
        self.prepare_dimensions(width, height);
        let needed = self.width * self.height * 4;
        if self.rgba_buffer.len() != needed {
            self.rgba_buffer.resize(needed, 0);
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

    /// 直接渲染并写入调用方提供的 `Rgba8888` 像素切片（GPU 回读与软件回退均零中间整屏 CPU 副本）。
    pub fn render_draw_ops_rgba8888_into(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        dpi_scale: i16,
        dirty_regions: &[Region],
        out_pixels: &mut [Rgba8888],
    ) -> (usize, usize) {
        if dirty_regions.is_empty() {
            return (0, 0);
        }
        let s = dpi_scale.max(1);
        let pw = (viewport.w.max(1) as usize).saturating_mul(s as usize);
        let ph = (viewport.h.max(1) as usize).saturating_mul(s as usize);
        self.prepare_dimensions(pw, ph);
        let _ = self.scene.record_draw_ops(ops, s, dirty_regions);

        let mut total_pixels = 0usize;
        for &r in dirty_regions {
            total_pixels += (r.w.max(0) as usize) * (r.h.max(0) as usize);
        }

        if let Some(gpu) = &mut self.gpu {
            if out_pixels.len() >= pw * ph {
                // SAFETY: `Rgba8888` 是 `#[repr(transparent)]` 封装 `u32` 且字节序即为 `[R, G, B, A]`，
                // 与 `wgpu::TextureFormat::Rgba8Unorm` 逐字节对齐，直接按脏区子矩形回读零中间副本。
                let dst_bytes = unsafe {
                    core::slice::from_raw_parts_mut(out_pixels.as_mut_ptr() as *mut u8, pw * ph * 4)
                };
                if gpu.render_rgba_dirty(ops, viewport, s as f32, dirty_regions, dst_bytes) {
                    return (dirty_regions.len(), total_pixels);
                }
            }
        }

        // 软件回退直接光栅化到调用方 `out_pixels`，彻底免除 `fallback_pixmap` 与 `rgba_buffer` 副本。
        for &reg in dirty_regions {
            if !reg.is_empty() {
                rasterize_region_scaled_parallel(
                    ops,
                    viewport,
                    s as f32,
                    reg,
                    0,
                    0,
                    pw,
                    out_pixels,
                );
            }
        }
        (dirty_regions.len(), total_pixels)
    }

    fn rasterize_software_fallback(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        sf: f32,
        dirty_regions: &[Region],
    ) {
        let needed = self.width * self.height;
        if self.fallback_pixmap.len() != needed {
            self.fallback_pixmap.resize(needed, Rgba8888(0));
        }
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
            if gpu.render_rgba_dirty(ops, viewport, s as f32, dirty_regions, &mut self.rgba_buffer) {
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
