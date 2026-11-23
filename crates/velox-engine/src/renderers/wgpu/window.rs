//! 跨平台 `wgpu` Swapchain 窗口直出渲染器（桌面 `winit` 与移动端原生窗口共用）。

use ::wgpu;
use std::eprintln;
use std::string::String;
use std::sync::Arc;
use std::vec::Vec;

use crate::motion::DrawOp;
use crate::solver::Region;
use crate::style::Color;

use super::pipeline::WgpuRenderer;
use super::{block_on, create_wgpu_instance};

/// 后台线程无头预初始化的 `wgpu` 设备与预编译 2D SDF 渲染管线上下文（可跨线程 `Send` 移交主线程）。
///
/// 用于实现「秒开首屏 + 异步后台 GPU 无缝热升舱」：
/// 后台线程并发执行最耗时的图形驱动加载、`request_adapter`、`request_device` 与着色器管线编译，
/// 就绪后主线程仅需 ~10ms 绑定窗口 `Surface` 即可零闪烁接管至硬件加速渲染。
pub struct PreparedWgpuContext {
    // 析构顺序：先释放管线，再释放队列、设备、适配器与实例
    precompiled_renderer: Option<(wgpu::TextureFormat, WgpuRenderer)>,
    queue: wgpu::Queue,
    device: wgpu::Device,
    adapter: wgpu::Adapter,
    instance: wgpu::Instance,
    adapter_name: String,
    backend_name: &'static str,
}

// SAFETY: `wgpu::Instance`/`Adapter`/`Device`/`Queue` 及内部仅含 `Arc` 与 wgpu 句柄的 `WgpuRenderer`
// 均满足跨线程所有权移交（`Send`）；显式实现可避免 `wgpu 30` 深层 `Hub` 触发 rustc auto-trait 递归上限警告。
unsafe impl Send for PreparedWgpuContext {}

impl PreparedWgpuContext {
    fn try_init_headless_with_backends(backends: wgpu::Backends) -> Option<Self> {
        let trace = std::env::var_os("VELOX_STARTUP_TRACE").is_some();
        let t0 = std::time::Instant::now();
        let instance = create_wgpu_instance(backends);
        let t_inst = t0.elapsed();

        let adapter = if std::env::var_os("WGPU_ADAPTER_NAME").is_some()
            || std::env::var_os("WGPU_POWER_PREFERENCE").is_some()
        {
            block_on(wgpu::util::initialize_adapter_from_env_or_default(
                &instance, None,
            ))
            .ok()?
        } else {
            block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            }))
            .ok()?
        };
        let t_adap = t0.elapsed();

        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return None;
        }

        let required_limits = wgpu::Limits {
            max_inter_stage_shader_variables: 15,
            ..wgpu::Limits::default()
        };

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("velox_wgpu_device"),
            required_features: wgpu::Features::empty(),
            required_limits,
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
        }))
        .ok()?;
        let t_dev = t0.elapsed();

        device.on_uncaptured_error(Arc::new(|err| {
            eprintln!("[velox::gpu] WGPU uncaptured error: {err:?}");
        }));

        // 桌面端（Windows DX12 / macOS Metal / Linux Vulkan）首选 `Bgra8Unorm`，后台预先完成 WGSL 编译与 PSO 构建
        let preferred_fmt = wgpu::TextureFormat::Bgra8Unorm;
        let renderer = WgpuRenderer::new(&device, preferred_fmt);
        let t_ren = t0.elapsed();

        let backend_name = match info.backend {
            wgpu::Backend::Dx12 => "DX12",
            wgpu::Backend::Vulkan => "Vulkan",
            wgpu::Backend::Metal => "Metal",
            wgpu::Backend::Gl => "OpenGL",
            wgpu::Backend::BrowserWebGpu => "WebGPU",
            wgpu::Backend::Noop => "Noop",
        };

        if trace {
            std::println!(
                "[VELOX_STARTUP_TRACE] async bg wgpu warmup ({} / {}): instance={:.1}ms, adapter=+{:.1}ms, device=+{:.1}ms, precompile_pipeline=+{:.1}ms, total={:.1}ms",
                info.name,
                backend_name,
                t_inst.as_secs_f64() * 1000.0,
                (t_adap - t_inst).as_secs_f64() * 1000.0,
                (t_dev - t_adap).as_secs_f64() * 1000.0,
                (t_ren - t_dev).as_secs_f64() * 1000.0,
                t_ren.as_secs_f64() * 1000.0,
            );
        }

        Some(Self {
            precompiled_renderer: Some((preferred_fmt, renderer)),
            queue,
            device,
            adapter,
            instance,
            adapter_name: info.name,
            backend_name,
        })
    }

    /// 在无窗口句柄的独立后台线程中完成 `wgpu` 驱动加载、设备创建与着色器管线预编译。
    pub fn init_headless() -> Option<Self> {
        if let Some(env_backends) = wgpu::Backends::from_env() {
            return Self::try_init_headless_with_backends(env_backends);
        }
        #[cfg(target_os = "windows")]
        {
            if let Some(ctx) = Self::try_init_headless_with_backends(wgpu::Backends::DX12) {
                return Some(ctx);
            }
            Self::try_init_headless_with_backends(wgpu::Backends::VULKAN)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self::try_init_headless_with_backends(wgpu::Backends::PRIMARY)
        }
    }
}

/// 面向桌面（`winit`）与移动端（`ANativeWindow` / `CAMetalLayer` / `OHNativeWindow`）的统一轻量级
/// `wgpu` Swapchain 窗口渲染器。
///
/// 直接渲染至 Swapchain 纹理，无中间全屏纹理与 Blit 开销。
pub struct WgpuWindowRenderer {
    // 析构顺序：先释放管线/纹理/表面，再释放队列、设备与实例
    renderer: WgpuRenderer,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    device: wgpu::Device,
    _instance: wgpu::Instance,
    adapter_name: String,
    backend_name: &'static str,
}

impl WgpuWindowRenderer {
    fn try_init_with_backends<T>(
        target: T,
        width: u32,
        height: u32,
        backends: wgpu::Backends,
    ) -> Option<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let trace = std::env::var_os("VELOX_STARTUP_TRACE").is_some();
        let t0 = std::time::Instant::now();
        let instance = create_wgpu_instance(backends);
        let t_inst = t0.elapsed();
        let surface = instance.create_surface(target).ok()?;
        let t_surf = t0.elapsed();

        let adapter = if std::env::var_os("WGPU_ADAPTER_NAME").is_some()
            || std::env::var_os("WGPU_POWER_PREFERENCE").is_some()
        {
            block_on(wgpu::util::initialize_adapter_from_env_or_default(
                &instance,
                Some(&surface),
            ))
            .ok()?
        } else {
            // 桌面 2D SDF UI 优先选择低功耗常驻核显，避免双显卡设备唤醒休眠独显导致 0.8~1.8s 冷启动停顿；
            // 需要指定独显时可通过 `WGPU_POWER_PREFERENCE=high` 覆盖。
            block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            }))
            .ok()?
        };
        let t_adap = t0.elapsed();

        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return None;
        }

        let caps = surface.get_capabilities(&adapter);
        let format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|f| caps.formats.contains(f))
        .or_else(|| caps.formats.first().copied())?;

        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied()?
        };

        let required_limits = wgpu::Limits {
            max_inter_stage_shader_variables: 15,
            ..wgpu::Limits::default()
        };

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("velox_wgpu_device"),
            required_features: wgpu::Features::empty(),
            required_limits,
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
        }))
        .ok()?;
        let t_dev = t0.elapsed();

        device.on_uncaptured_error(Arc::new(|err| {
            eprintln!("[velox::gpu] WGPU uncaptured error: {err:?}");
        }));

        let w = width.max(1);
        let h = height.max(1);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode,
            view_formats: Vec::new(),
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &surface_config);
        let t_cfg = t0.elapsed();

        let renderer = WgpuRenderer::new(&device, format);
        let t_ren = t0.elapsed();
        if trace {
            std::println!(
                "[VELOX_STARTUP_TRACE] wgpu init: instance={:.1}ms, surface=+{:.1}ms, adapter=+{:.1}ms, device=+{:.1}ms, configure=+{:.1}ms, pipeline=+{:.1}ms, total={:.1}ms",
                t_inst.as_secs_f64() * 1000.0,
                (t_surf - t_inst).as_secs_f64() * 1000.0,
                (t_adap - t_surf).as_secs_f64() * 1000.0,
                (t_dev - t_adap).as_secs_f64() * 1000.0,
                (t_cfg - t_dev).as_secs_f64() * 1000.0,
                (t_ren - t_cfg).as_secs_f64() * 1000.0,
                t_ren.as_secs_f64() * 1000.0,
            );
        }
        let backend_name = match info.backend {
            wgpu::Backend::Dx12 => "DX12",
            wgpu::Backend::Vulkan => "Vulkan",
            wgpu::Backend::Metal => "Metal",
            wgpu::Backend::Gl => "OpenGL",
            wgpu::Backend::BrowserWebGpu => "WebGPU",
            wgpu::Backend::Noop => "Noop",
        };

        Some(Self {
            renderer,
            surface,
            surface_config,
            queue,
            device,
            _instance: instance,
            adapter_name: info.name,
            backend_name,
        })
    }

    /// 将后台线程预热完毕的 [`PreparedWgpuContext`] 在主线程快速绑定至目标窗口 `target`（仅耗时 ~10ms）。
    pub fn from_prepared<T>(
        prepared: PreparedWgpuContext,
        target: T,
        width: u32,
        height: u32,
    ) -> Option<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let trace = std::env::var_os("VELOX_STARTUP_TRACE").is_some();
        let t0 = std::time::Instant::now();
        let PreparedWgpuContext {
            precompiled_renderer,
            queue,
            device,
            adapter,
            instance,
            adapter_name,
            backend_name,
        } = prepared;

        let surface = instance.create_surface(target).ok()?;
        let caps = surface.get_capabilities(&adapter);
        if caps.formats.is_empty() || caps.alpha_modes.is_empty() {
            return None;
        }
        let format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|f| caps.formats.contains(f))
        .or_else(|| caps.formats.first().copied())?;

        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied()?
        };

        let w = width.max(1);
        let h = height.max(1);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode,
            view_formats: Vec::new(),
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &surface_config);

        let renderer = match precompiled_renderer {
            Some((fmt, r)) if fmt == format => r,
            _ => WgpuRenderer::new(&device, format),
        };
        if trace {
            std::println!(
                "[VELOX_STARTUP_TRACE] main-thread GPU surface attach & promote: {:.1}ms",
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }

        Some(Self {
            renderer,
            surface,
            surface_config,
            queue,
            device,
            _instance: instance,
            adapter_name,
            backend_name,
        })
    }

    /// 初始化窗口 GPU 渲染表面；若宿主无可用硬件 GPU 或创建失败则返回 `None` 以便优雅回退至软件光栅。
    pub fn new<T>(target: T, width: u32, height: u32) -> Option<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>> + Clone,
    {
        if let Some(env_backends) = wgpu::Backends::from_env() {
            return Self::try_init_with_backends(target, width, height, env_backends);
        }

        #[cfg(target_os = "windows")]
        {
            // Windows 优先采用原生 DX12，避开部分核显 Vulkan WSI 上屏黑/白屏；若无 DX12 硬件再回退 Vulkan
            if let Some(surf) =
                Self::try_init_with_backends(target.clone(), width, height, wgpu::Backends::DX12)
            {
                return Some(surf);
            }
            Self::try_init_with_backends(target, width, height, wgpu::Backends::VULKAN)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self::try_init_with_backends(target, width, height, wgpu::Backends::PRIMARY)
        }
    }

    /// 返回当前选中的显卡适配器名称。
    #[inline]
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// 返回当前激活的图形 API 后端名称（`"DX12"` / `"Vulkan"` / `"Metal"` 等）。
    #[inline]
    pub const fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    /// 返回最近一帧合批渲染的图元总数。
    #[inline]
    pub fn last_primitive_count(&self) -> usize {
        self.renderer.last_primitive_count()
    }

    /// 返回当前字形图集与精灵图集的物理像素边长 `(glyph_size, sprite_size)`。
    #[inline]
    pub const fn atlas_sizes(&self) -> (u32, u32) {
        self.renderer.atlas_sizes()
    }

    /// 调整 Swapchain 表面物理像素尺寸。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn acquire_surface_texture(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => Some(tex),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(tex)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => Some(tex),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// 直接在 Swapchain 上执行单 Pass 2D SDF 渲染并提交显示（整数 DPI 兼容入口）。
    pub fn render(&mut self, ops: &[DrawOp<'_>], viewport: Region, dpi_scale: i16) -> bool {
        self.render_scaled(ops, viewport, dpi_scale.max(1) as f32)
    }

    /// 直接在 Swapchain 上执行单 Pass 2D SDF 渲染并提交显示（原生支持分数 DPI `scale_factor` 与 Pixel Snapping）。
    pub fn render_scaled(&mut self, ops: &[DrawOp<'_>], viewport: Region, scale_factor: f32) -> bool {
        self.render_scaled_with_clear(ops, viewport, scale_factor, Color::rgb(0, 0, 0))
    }

    /// 直接在 Swapchain 上以指定背景清屏色 `clear` 执行单 Pass 2D SDF 渲染并提交显示。
    pub fn render_scaled_with_clear(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        clear: Color,
    ) -> bool {
        let Some(frame) = self.acquire_surface_texture() else {
            return false;
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let clear_color = if self.surface_config.format.is_srgb() {
            let srgb_to_lin = |c: u8| -> f64 {
                let s = (c as f64) / 255.0;
                if s <= 0.04045 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            wgpu::Color {
                r: srgb_to_lin(clear.r()),
                g: srgb_to_lin(clear.g()),
                b: srgb_to_lin(clear.b()),
                a: 1.0,
            }
        } else {
            wgpu::Color {
                r: (clear.r() as f64) / 255.0,
                g: (clear.g() as f64) / 255.0,
                b: (clear.b() as f64) / 255.0,
                a: 1.0,
            }
        };
        self.renderer.render_to_view(
            &self.device,
            &self.queue,
            ops,
            viewport,
            scale_factor,
            &frame_view,
            self.surface_config.width,
            self.surface_config.height,
            clear_color,
        );
        self.queue.present(frame);
        true
    }

    /// 等待已提交至 GPU 队列的渲染指令完成（用于首帧隐身提交后、显窗前的零白屏同步栅栏）。
    #[inline]
    pub fn flush_and_wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
