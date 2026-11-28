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
    pso_cache: Option<Arc<super::pso_cache::DiskPipelineCache>>,
    queue: wgpu::Queue,
    device: wgpu::Device,
    adapter: Option<wgpu::Adapter>,
    instance: Option<wgpu::Instance>,
    adapter_name: String,
    backend_name: &'static str,
    adapter_luid: Option<[u8; 8]>,
    device_uuid: Option<[u8; 16]>,
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

        let (device, queue, pso_cache) = super::pso_cache::request_device_with_pso_cache(
            &adapter,
            "velox_wgpu_device",
            required_limits,
        )?;
        let t_dev = t0.elapsed();

        device.on_uncaptured_error(Arc::new(|err| {
            eprintln!("[velox::gpu] WGPU uncaptured error: {err:?}");
        }));

        // 桌面端（Windows DX12 / macOS Metal / Linux Vulkan）首选 `Bgra8Unorm`，后台预先完成 WGSL 编译与 PSO 构建
        let preferred_fmt = wgpu::TextureFormat::Bgra8Unorm;
        let mut renderer = WgpuRenderer::new_with_cache(&device, preferred_fmt, pso_cache.clone());
        // 首屏主 SDF 管线就绪后立即触发 Effects / Canvas 管线的独立后台预热，消除首次切页同步编译停顿
        renderer.spawn_background_pipeline_warmup(&device, &queue);
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
            pso_cache,
            queue,
            device,
            adapter: Some(adapter),
            instance: Some(instance),
            adapter_name: info.name,
            backend_name,
            adapter_luid: None,
            device_uuid: None,
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

    /// 直接复用外部引擎（如海图/3D/视频渲染器）已创建的 `wgpu::Device` 与 `wgpu::Queue` 构建共享上下文。
    ///
    /// 外部引擎与 Velox 共享同一物理 GPU 与命令队列后，可将外部 `wgpu::TextureView` 通过
    /// [`WgpuRenderer::register_external_texture`] 零拷贝嵌入 Velox UI，无需任何跨 API 句柄转换或 CPU 回读。
    pub fn from_shared_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter: Option<wgpu::Adapter>,
        instance: Option<wgpu::Instance>,
        preferred_format: wgpu::TextureFormat,
        adapter_name: impl Into<String>,
        backend_name: &'static str,
    ) -> Self {
        let renderer = WgpuRenderer::new(&device, preferred_format);
        Self {
            precompiled_renderer: Some((preferred_format, renderer)),
            pso_cache: None,
            queue,
            device,
            adapter,
            instance,
            adapter_name: adapter_name.into(),
            backend_name,
            adapter_luid: None,
            device_uuid: None,
        }
    }

    /// 按宿主物理显卡标识（Windows DX12 `LUID` 或 Linux/Android Vulkan `deviceUUID`）对齐并创建上下文。
    ///
    /// - 启用 `gpu-interop` feature 时，通过底层 `wgpu-hal` 精确匹配与宿主 Qt/GTK4/WPF/WinUI 相同的物理 GPU
    ///   并激活跨进程/跨 API 外部显存与信号量扩展；
    /// - 未启用 `gpu-interop` 或参数均为 `None` 时自动退化为默认硬件无头初始化。
    pub fn from_host_adapter_id(
        adapter_luid: Option<[u8; 8]>,
        device_uuid: Option<[u8; 16]>,
    ) -> Option<Self> {
        #[cfg(feature = "gpu-interop")]
        {
            if let Some(ctx) = super::interop::init_aligned_wgpu_context(adapter_luid, device_uuid) {
                return Some(ctx);
            }
        }
        let mut ctx = Self::init_headless()?;
        ctx.adapter_luid = adapter_luid;
        ctx.device_uuid = device_uuid;
        Some(ctx)
    }

    /// 供 `gpu-interop` 内部组装带精确硬件 LUID/UUID 标识的上下文。
    #[cfg(feature = "gpu-interop")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_interop_parts(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        preferred_format: wgpu::TextureFormat,
        adapter_name: String,
        backend_name: &'static str,
        adapter_luid: Option<[u8; 8]>,
        device_uuid: Option<[u8; 16]>,
    ) -> Self {
        let renderer = WgpuRenderer::new(&device, preferred_format);
        Self {
            precompiled_renderer: Some((preferred_format, renderer)),
            pso_cache: None,
            queue,
            device,
            adapter: Some(adapter),
            instance: Some(instance),
            adapter_name,
            backend_name,
            adapter_luid,
            device_uuid,
        }
    }

    /// 返回底层 `wgpu::Device` 句柄引用（供外部渲染器同设备创建共享纹理）。
    #[inline]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// 返回底层 `wgpu::Queue` 句柄引用。
    #[inline]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 返回可选 `wgpu::Adapter` 引用。
    #[inline]
    pub fn adapter(&self) -> Option<&wgpu::Adapter> {
        self.adapter.as_ref()
    }

    /// 返回可选 `wgpu::Instance` 引用。
    #[inline]
    pub fn instance(&self) -> Option<&wgpu::Instance> {
        self.instance.as_ref()
    }

    /// 返回适配器名称。
    #[inline]
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// 返回后端图形 API 名称。
    #[inline]
    pub const fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    /// 返回匹配到的 Windows DX12 适配器 LUID（若可用）。
    #[inline]
    pub const fn adapter_luid(&self) -> Option<[u8; 8]> {
        self.adapter_luid
    }

    /// 返回匹配到的 Vulkan 物理设备 UUID（若可用）。
    #[inline]
    pub const fn device_uuid(&self) -> Option<[u8; 16]> {
        self.device_uuid
    }

    /// 取出预编译的 `WgpuRenderer`（若格式一致）或按指定像素格式即时构建（保留磁盘 PSO 缓存）。
    pub fn take_or_create_renderer(&mut self, format: wgpu::TextureFormat) -> WgpuRenderer {
        match self.precompiled_renderer.take() {
            Some((fmt, r)) if fmt == format => r,
            _ => WgpuRenderer::new_with_cache(&self.device, format, self.pso_cache.clone()),
        }
    }
}

/// 窗口 Swapchain 提交结果状态（用于区分正常呈现、可瞬态重试错误与持续遮挡以杜绝后台空转）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SurfacePresentOutcome {
    /// 本帧已成功提交至 Swapchain。
    Presented,
    /// 瞬态获取失败（如 `Timeout` / `Outdated` / `Lost`），已失效持久纹理，可有限次重试。
    RetryableFailure,
    /// 窗口处于遮挡或最小化状态（`Occluded`），已保留脏区并失效持久纹理，不应立即空转重试。
    Occluded,
}

/// 面向桌面（`winit`）与移动端（`ANativeWindow` / `CAMetalLayer` / `OHNativeWindow`）的统一轻量级
/// `wgpu` Swapchain 窗口渲染器。
///
/// 支持 Slint 风格增量重绘策略：
/// - 稳态局部更新（脏区面积 ≤ 60%、不透明底且无多 Pass 图层）时，惰性分配持久主纹理（`ReusedBuffer` 语义），
///   仅对 `dirty_regions` 执行 `LoadOp::Load + set_scissor_rect` 局部重绘并在同一 `CommandEncoder` 内拷贝上屏；
/// - 整屏刷新、尺寸变化、表面重建或透明/毛玻璃场景下自动退化为直出 Swapchain（`NewBuffer` 语义，0 额外拷屏成本），
///   `trim_gpu_memory()` 时立即释放持久纹理显存。
pub struct WgpuWindowRenderer {
    // 析构顺序：先释放管线/纹理/表面，再释放队列、设备与实例
    renderer: WgpuRenderer,
    persistent_target: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    persistent_valid: bool,
    persistent_target_enabled: bool,
    surface_supports_copy_dst: bool,
    last_frame_used_partial_repaint: bool,
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

        let (device, queue, pso_cache) = super::pso_cache::request_device_with_pso_cache(
            &adapter,
            "velox_wgpu_device",
            required_limits,
        )?;
        let t_dev = t0.elapsed();

        device.on_uncaptured_error(Arc::new(|err| {
            eprintln!("[velox::gpu] WGPU uncaptured error: {err:?}");
        }));

        let w = width.max(1);
        let h = height.max(1);
        let surface_supports_copy_dst = caps.usages.contains(wgpu::TextureUsages::COPY_DST);
        let surface_usage = if surface_supports_copy_dst {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let surface_config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
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

        let mut renderer = WgpuRenderer::new_with_cache(&device, format, pso_cache);
        renderer.spawn_background_pipeline_warmup(&device, &queue);
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
            persistent_target: None,
            persistent_valid: false,
            persistent_target_enabled: true,
            surface_supports_copy_dst,
            last_frame_used_partial_repaint: false,
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
            pso_cache,
            queue,
            device,
            adapter,
            instance,
            adapter_name,
            backend_name,
            ..
        } = prepared;

        let instance = instance?;
        let adapter = adapter?;
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
        let surface_supports_copy_dst = caps.usages.contains(wgpu::TextureUsages::COPY_DST);
        let surface_usage = if surface_supports_copy_dst {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let surface_config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
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
            _ => {
                let mut r = WgpuRenderer::new_with_cache(&device, format, pso_cache);
                r.spawn_background_pipeline_warmup(&device, &queue);
                r
            }
        };
        if trace {
            std::println!(
                "[VELOX_STARTUP_TRACE] main-thread GPU surface attach & promote: {:.1}ms",
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }

        Some(Self {
            renderer,
            persistent_target: None,
            persistent_valid: false,
            persistent_target_enabled: true,
            surface_supports_copy_dst,
            last_frame_used_partial_repaint: false,
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

    /// 返回底层 `wgpu::Device` 句柄引用（供外部引擎同设备零拷贝创建共享纹理）。
    #[inline]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// 返回底层 `wgpu::Queue` 句柄引用。
    #[inline]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 返回内部 `WgpuRenderer` 只读引用。
    #[inline]
    pub fn renderer(&self) -> &WgpuRenderer {
        &self.renderer
    }

    /// 返回内部 `WgpuRenderer` 可变引用。
    #[inline]
    pub fn renderer_mut(&mut self) -> &mut WgpuRenderer {
        &mut self.renderer
    }

    /// 向窗口渲染器注册外部零拷贝 GPU 纹理（如海图/视频/3D 视口），返回句柄 ID。
    #[inline]
    pub fn register_external_texture(
        &mut self,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) -> crate::paint::ExternalTextureId {
        self.renderer.register_external_texture(view, sampler)
    }

    /// 原地更新已注册外部零拷贝纹理的 `TextureView`（如外部引擎 Swap/Resize 后切换目标）。
    #[inline]
    pub fn update_external_texture(
        &mut self,
        id: crate::paint::ExternalTextureId,
        view: wgpu::TextureView,
    ) -> bool {
        self.renderer.update_external_texture(id, view)
    }

    /// 注销外部零拷贝纹理槽位。
    #[inline]
    pub fn unregister_external_texture(&mut self, id: crate::paint::ExternalTextureId) -> bool {
        self.renderer.unregister_external_texture(id)
    }

    /// 调整 Swapchain 表面物理像素尺寸（窗口缩小时自动回收大于新视口的空闲离屏纹理，并按 Slint `NewBuffer` 规则失效持久主纹理）。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.persistent_target = None;
        self.persistent_valid = false;
        self.last_frame_used_partial_repaint = false;
        self.surface.configure(&self.device, &self.surface_config);
        self.renderer.on_viewport_resized(width, height);
    }

    /// 后台化或内存压力时主动释放全部空闲离屏纹理、持久渲染目标与 SVG 遮罩显存。
    #[inline]
    pub fn trim_gpu_memory(&mut self) {
        self.persistent_target = None;
        self.persistent_valid = false;
        self.renderer.trim_gpu_memory();
    }

    /// 启用或禁用窗口级持久渲染目标局部重绘（默认开启；关闭后立刻释放持久纹理显存）。
    #[inline]
    pub fn set_persistent_target_enabled(&mut self, enabled: bool) {
        self.persistent_target_enabled = enabled;
        if !enabled {
            self.persistent_target = None;
            self.persistent_valid = false;
        }
    }

    /// 返回当前是否启用了窗口级持久渲染目标局部重绘。
    #[inline]
    pub const fn persistent_target_enabled(&self) -> bool {
        self.persistent_target_enabled && self.surface_supports_copy_dst
    }

    /// 返回最近一帧是否实际命中了持久渲染目标的局部 Scissor 增量重绘（`ReusedBuffer` 语义）。
    #[inline]
    pub const fn last_frame_used_partial_repaint(&self) -> bool {
        self.last_frame_used_partial_repaint
    }

    /// 返回当前常驻持久主纹理占用的估算显存字节数（惰性分配，未创建或已 `trim_gpu_memory` 时为 `0`）。
    #[inline]
    pub fn persistent_target_bytes(&self) -> usize {
        self.persistent_target
            .as_ref()
            .map(|(_, _, w, h)| (*w as usize).saturating_mul(*h as usize).saturating_mul(4))
            .unwrap_or(0)
    }

    /// 主动使持久主纹理缓存失效（下一帧强制按 `NewBuffer` 语义整屏重建）。
    #[inline]
    pub fn invalidate_persistent_target(&mut self) {
        self.persistent_valid = false;
    }

    /// 返回当前离屏图层纹理池占用的估算显存字节数。
    #[inline]
    pub fn layer_pool_bytes(&self) -> usize {
        self.renderer.layer_pool_bytes()
    }

    /// 返回最近一帧增量上传至 GPU 实例缓冲的实际字节数。
    #[inline]
    pub fn last_uploaded_instance_bytes(&self) -> usize {
        self.renderer.last_uploaded_instance_bytes()
    }

    /// 显式移除并释放指定 `SceneCanvas` 场景的常驻 GPU 显存缓冲。
    #[inline]
    pub fn remove_canvas_scene(&mut self, scene_id: crate::canvas_scene::StaticSceneId) -> bool {
        self.renderer.remove_canvas_scene(scene_id)
    }

    /// 返回 `CanvasGpuEngine` 当前缓存的场景显存总字节数。
    #[inline]
    pub fn canvas_cache_bytes(&self) -> usize {
        self.renderer.canvas_cache_bytes()
    }

    fn ensure_persistent_target(&mut self) -> (&wgpu::Texture, &wgpu::TextureView) {
        let w = self.surface_config.width.max(1);
        let h = self.surface_config.height.max(1);
        let needs_alloc = match &self.persistent_target {
            Some((_, _, pw, ph)) => *pw != w || *ph != h,
            None => true,
        };
        if needs_alloc {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("velox_window_persistent_rt"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.persistent_target = Some((tex, view, w, h));
            self.persistent_valid = false;
        }
        let (tex, view, _, _) = self.persistent_target.as_ref().unwrap();
        (tex, view)
    }

    fn acquire_surface_texture_status(&mut self) -> Result<wgpu::SurfaceTexture, SurfacePresentOutcome> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => Ok(tex),
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.persistent_valid = false;
                self.last_frame_used_partial_repaint = false;
                Err(SurfacePresentOutcome::Occluded)
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // Swapchain 重建后按 Slint `NewBuffer` 规则强制失效持久缓存
                self.persistent_valid = false;
                self.last_frame_used_partial_repaint = false;
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(tex)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => Ok(tex),
                    wgpu::CurrentSurfaceTexture::Occluded => {
                        self.persistent_valid = false;
                        self.last_frame_used_partial_repaint = false;
                        Err(SurfacePresentOutcome::Occluded)
                    }
                    _ => {
                        self.persistent_valid = false;
                        self.last_frame_used_partial_repaint = false;
                        Err(SurfacePresentOutcome::RetryableFailure)
                    }
                }
            }
            _ => {
                // Timeout / Validation 等瞬态获取失败：失效持久底帧并回报可有限重试状态
                self.persistent_valid = false;
                self.last_frame_used_partial_repaint = false;
                Err(SurfacePresentOutcome::RetryableFailure)
            }
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

    /// 直接在 Swapchain 上以指定背景清屏色 `clear` 执行 2D SDF 渲染并提交显示。
    pub fn render_scaled_with_clear(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        clear: Color,
    ) -> bool {
        self.render_scaled_with_clear_dirty(ops, viewport, scale_factor, clear, &[])
    }

    /// 面向窗口 Swapchain 的统一提交入口，融合 Slint `NewBuffer` / `ReusedBuffer` 增量重绘规则。
    pub fn render_scaled_with_clear_dirty(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        clear: Color,
        dirty_regions: &[Region],
    ) -> bool {
        self.render_scaled_with_clear_dirty_status(ops, viewport, scale_factor, clear, dirty_regions)
            == SurfacePresentOutcome::Presented
    }

    /// 面向窗口 Swapchain 的带精确状态回传提交入口（区分 `Presented` / `Occluded` / `RetryableFailure` 防止窗口被遮挡时后台空转）：
    /// - 当本帧为局部脏更新（面积占比 ≤ 60%、不透明全屏底、无多 Pass 毛玻璃或 `SceneCanvas`）且 Surface 支持 `COPY_DST` 时，
    ///   惰性启用持久主纹理执行 `LoadOp::Load + set_scissor_rect` 局部重绘并在同一 `CommandEncoder` 内拷贝上屏；
    /// - 当脏区 > 60%（整屏滚动/切换）或存在多 Pass 图层/透明底时，直接整屏直出 Swapchain（零多余拷屏开销）并自动失效持久底帧。
    pub fn render_scaled_with_clear_dirty_status(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        clear: Color,
        dirty_regions: &[Region],
    ) -> SurfacePresentOutcome {
        let frame = match self.acquire_surface_texture_status() {
            Ok(tex) => tex,
            Err(status) => return status,
        };
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

        let w = self.surface_config.width;
        let h = self.surface_config.height;

        // 判定是否满足持久渲染目标局部增量重绘条件（Slint ReusedBuffer 边界 + 60% 面积拐点护栏）
        let vp_area = (viewport.w.max(0) as u64).saturating_mul(viewport.h.max(0) as u64);
        let dirty_area: u64 = dirty_regions
            .iter()
            .map(|r| {
                let c = r.intersect(viewport);
                (c.w.max(0) as u64).saturating_mul(c.h.max(0) as u64)
            })
            .sum();
        let has_opaque_bg = ops.first().is_some_and(|op| {
            matches!(
                op.payload,
                crate::motion::DrawPayload::Quad {
                    rounded: 0,
                    border_width: 0,
                    ..
                }
            ) && op.clip_radius == 0
                && crate::renderers::software::opaque_core_rect(op).intersect(viewport) == viewport
        });
        let can_use_persistent = self.persistent_target_enabled
            && self.surface_supports_copy_dst
            && vp_area > 0
            && !dirty_regions.is_empty()
            && dirty_area > 0
            && dirty_area.saturating_mul(10) <= vp_area.saturating_mul(6)
            && has_opaque_bg
            && !crate::renderers::software::ops_have_layers_or_blend(ops)
            && !ops
                .iter()
                .any(|op| matches!(op.payload, crate::motion::DrawPayload::SceneCanvas(_)));

        if can_use_persistent {
            let _ = self.ensure_persistent_target();
            let (p_tex, p_view, _, _) = self.persistent_target.as_ref().unwrap();
            if self.persistent_valid {
                // 稳态 ReusedBuffer：在持久主纹理上局部 Scissor 重绘，并在同一 CommandEncoder 末尾拷入 Swapchain
                let used_partial = self.renderer.render_to_view_dirty_with_copy(
                    &self.device,
                    &self.queue,
                    ops,
                    viewport,
                    scale_factor,
                    p_view,
                    w,
                    h,
                    dirty_regions,
                    clear_color,
                    Some((p_tex, &frame.texture)),
                );
                self.persistent_valid = true;
                self.last_frame_used_partial_repaint = used_partial;
            } else {
                // 首次建立持久主纹理（NewBuffer -> ReusedBuffer）：整屏写入持久纹理并拷入 Swapchain
                let _ = self.renderer.render_to_view_dirty_with_copy(
                    &self.device,
                    &self.queue,
                    ops,
                    viewport,
                    scale_factor,
                    p_view,
                    w,
                    h,
                    &[],
                    clear_color,
                    Some((p_tex, &frame.texture)),
                );
                self.persistent_valid = true;
                self.last_frame_used_partial_repaint = false;
            }
            self.queue.present(frame);
            return SurfacePresentOutcome::Presented;
        }

        // 大面积脏区 / 整屏首帧 / 多 Pass 毛玻璃 / SceneCanvas：直接渲染至 Swapchain，零拷屏开销
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let hints = if dirty_regions.is_empty() {
            None
        } else {
            Some(dirty_regions)
        };
        self.renderer.render_to_view_with_damage_hints(
            &self.device,
            &self.queue,
            ops,
            viewport,
            scale_factor,
            &frame_view,
            w,
            h,
            clear_color,
            hints,
        );
        self.persistent_valid = false;
        self.last_frame_used_partial_repaint = false;
        self.queue.present(frame);
        SurfacePresentOutcome::Presented
    }

    /// 等待已提交至 GPU 队列的渲染指令完成（用于首帧隐身提交后、显窗前的零白屏同步栅栏）。
    #[inline]
    pub fn flush_and_wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
