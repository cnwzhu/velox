//! 跨平台桌面窗口与事件驱动平台后端 `velox_core::platform::winit`（Windows / macOS / Linux Wayland）。
//!
//! 模块职责划分：
//! - `mod`：[`WinitPlatform`]（及兼容别名 [`DesktopWindowRunner`]）对外构建器与 [`Platform`] / [`PlatformSurface`] 实现；
//! - `handler`：`WinitAppHandler` 原生事件循环、GPU/软光栅桥接、分数 DPI 与 IME 光标跟随；
//! - `nested`：[`run_nested_compositor_window`] 自定义合成器嵌套窗口宿主。

#[cfg(feature = "winit")]
mod handler;
mod nested;

use std::eprintln;
use std::string::String;
use std::time::{Duration, Instant};

#[cfg(feature = "winit")]
use winit::event_loop::EventLoop;

#[cfg(feature = "winit")]
use handler::{WinitAppHandler, WinitUserEvent};
pub use nested::run_nested_compositor_window;
pub use super::shm_driver::{
    write_uncompressed_png, ShmSlot, ShmSurfaceDriver, SoftwareSurfaceDriver, WaylandSurfaceDriver,
    WlBuffer, WlSurface,
};
use crate::prelude::*;

/// 桌面窗口运行器兼容别名。
pub type DesktopWindowRunner = WinitPlatform;

/// 基于 `winit` + `softbuffer` 的跨平台桌面窗口平台（支持 Linux Wayland、Windows、macOS）。
///
/// 实现 [`Platform`] 与 [`PlatformSurface`] 特征。
pub struct WinitPlatform {
    title: String,
    width: usize,
    height: usize,
    dpi_scale: usize,
    dpi_scale_explicit: bool,
    start_instant: Instant,
    driver: SoftwareSurfaceDriver,
    wl_surface: WlSurface,
}

impl WinitPlatform {
    /// 创建指定标题与逻辑尺寸 `(width, height)` 的跨平台 `WinitPlatform` 窗口运行器。
    pub fn new(title: impl Into<String>, width: usize, height: usize) -> Self {
        let dpi_scale = 2;
        Self {
            title: title.into(),
            width,
            height,
            dpi_scale,
            dpi_scale_explicit: false,
            start_instant: Instant::now(),
            driver: SoftwareSurfaceDriver::new(width * dpi_scale, height * dpi_scale),
            wl_surface: WlSurface::new(),
        }
    }

    /// 设置 HiDPI 缩放倍数（`1` 为原始像素，`2` 为 2× 高清矢量光栅化）。
    pub fn with_dpi_scale(mut self, scale: usize) -> Self {
        self.dpi_scale = scale.max(1);
        self.dpi_scale_explicit = true;
        self.driver
            .resize(self.width * self.dpi_scale, self.height * self.dpi_scale);
        self
    }

    /// 配置桌面渲染后端类型（默认采用 [`super::DesktopRenderBackend::default_desktop`]，即优先 `Wgpu` 2D SDF GPU 后端）。
    #[inline]
    pub fn with_backend(mut self, backend: super::DesktopRenderBackend) -> Self {
        self.driver.set_backend(backend);
        self
    }

    /// 动态切换桌面渲染后端类型。
    #[inline]
    pub fn set_backend(&mut self, backend: super::DesktopRenderBackend) {
        self.driver.set_backend(backend);
    }

    /// 返回当前激活的桌面渲染后端类型。
    #[inline]
    pub const fn backend(&self) -> super::DesktopRenderBackend {
        self.driver.backend()
    }

    /// 返回内部表面驱动器只读引用（用于检查单槽原位复用与局部光栅化脏矩形）。
    #[inline]
    pub const fn driver(&self) -> &SoftwareSurfaceDriver {
        &self.driver
    }

    /// 启动基于 `winit::event_loop::EventLoop` + `softbuffer` 的原生跨平台窗口事件循环。
    pub fn run(self, mut core: VeloxEngine, scroll_list_id: Option<u32>) {
        #[cfg(feature = "winit")]
        {
            // 启动阶段后台线程并行预热系统主字体（CJK 语言下同步预热回退字库），与 EventLoop 及 GPU 管线初始化并发重叠
            #[cfg(not(target_arch = "wasm32"))]
            {
                let loc = core.locale();
                let lang = loc.language();
                let preload_cjk = lang == "zh" || lang == "ja" || lang == "ko";
                crate::display::ttf_atlas::spawn_font_preload(preload_cjk);
            }

            let event_loop = match EventLoop::<WinitUserEvent>::with_user_event().build() {
                Ok(el) => el,
                Err(e) => {
                    eprintln!("[velox::platform::winit] 无法初始化显示服务器 EventLoop ({e})，回退至无头验证模式。");
                    self.run_headless(&mut core, 10);
                    return;
                }
            };
            event_loop.listen_device_events(winit::event_loop::DeviceEvents::Never);

            let Self {
                title,
                width,
                height,
                dpi_scale,
                dpi_scale_explicit,
                mut driver,
                wl_surface,
                ..
            } = self;

            let scale_factor = (dpi_scale.max(1)) as f32;
            core.set_scale_factor(scale_factor);
            core.set_pointer_kind(PointerKind::Mouse);
            let async_proxy = event_loop.create_proxy();
            core.set_async_waker(move || {
                let _ = async_proxy.send_event(WinitUserEvent::WakeAsync);
            });
            let phys_w = ((width as f32) * scale_factor).round().max(1.0) as usize;
            let phys_h = ((height as f32) * scale_factor).round().max(1.0) as usize;

            // Instant First-Paint + Async GPU Promotion：
            // 当选用 GPU 后端时，立刻在独立后台线程并发初始化 wgpu 驱动/适配器/设备/着色器管线，
            // 主线程零阻塞以内置软件光栅器 ~60ms 秒开首屏，后台 GPU 就绪后自动热升舱接管并释放软件双缓冲。
            #[cfg(feature = "gpu")]
            let gpu_warmup_in_flight = if driver.backend().is_gpu() {
                let proxy = event_loop.create_proxy();
                std::thread::Builder::new()
                    .name(String::from("velox-gpu-warmup"))
                    .stack_size(512 * 1024)
                    .spawn(move || {
                        let prepared = crate::renderers::wgpu::PreparedWgpuContext::init_headless();
                        let _ = proxy.send_event(WinitUserEvent::GpuReady(prepared));
                    })
                    .is_ok()
            } else {
                false
            };

            // 首帧立即由软件渲染器承接，按真实物理尺寸在 `resumed()` 中分配；GPU 热升舱完成后立即收缩回 1x1 释放内存
            driver.resize(1, 1);

            let mut app = WinitAppHandler {
                title,
                logical_w: width,
                logical_h: height,
                phys_w,
                phys_h,
                scale_factor,
                dpi_scale_explicit,
                core,
                scroll_list_id,
                driver,
                wl_surface,
                #[cfg(feature = "gpu")]
                gpu_surface: None,
                #[cfg(feature = "gpu")]
                pending_gpu: None,
                #[cfg(feature = "gpu")]
                gpu_warmup_in_flight,
                surface: None,
                window: None,
                window_shown: false,
                force_present_once: false,
                last_window_theme_dark: None,
                cursor_pos: (0, 0),
                mouse_down: false,
                last_tick: Instant::now() - Duration::from_secs(1),
                last_ime_area: None,
                last_cursor: CursorShape::Default,
            };

            let _ = event_loop.run_app(&mut app);
        }
        #[cfg(not(feature = "winit"))]
        {
            let _ = scroll_list_id;
            let _ = &self.title;
            eprintln!("[velox::platform::winit] 未启用 winit 特性，以无头模式步进 10 帧。");
            self.run_headless(&mut core, 10);
        }
    }

    /// 无头模式运行指定帧数（用于 CI 或无显示服务器环境验证）。
    pub fn run_headless(&self, core: &mut VeloxEngine, frames: usize) {
        let scale = self.dpi_scale.max(1);
        core.set_dpi_scale(scale as i16);
        let mut driver = SoftwareSurfaceDriver::new(self.width * scale, self.height * scale)
            .with_backend(self.driver.backend());
        let mut wl_surface = WlSurface::new();
        for _ in 0..frames {
            if let Some(id) = wl_surface.attached_buffer_id {
                driver.release_buffer(id);
            }
            let _ = driver.flush_frame_dt(core, &mut wl_surface, 16);
        }
    }

    /// 渲染一帧并将前台 `Xrgb8888` 像素缓冲保存为无损 PNG 图像文件。
    pub fn render_to_png(&self, core: &mut VeloxEngine, path: &str) -> std::io::Result<()> {
        let scale = self.dpi_scale.max(1);
        core.set_dpi_scale(scale as i16);
        let phys_w = self.width * scale;
        let phys_h = self.height * scale;
        let mut driver =
            SoftwareSurfaceDriver::new(phys_w, phys_h).with_backend(self.driver.backend());
        let mut wl_surface = WlSurface::new();
        driver.flush_frame_dt(core, &mut wl_surface, 16);
        write_uncompressed_png(path, phys_w, phys_h, driver.front_pixels())
    }
}


impl PlatformSurface for WinitPlatform {
    #[inline(always)]
    fn surface_size(&self) -> (i16, i16) {
        (self.width as i16, self.height as i16)
    }

    fn flush_frame_dt(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> FlushReport {
        engine.set_dpi_scale(self.dpi_scale.max(1) as i16);
        if let Some(old_id) = self.wl_surface.attached_buffer_id {
            self.driver.release_buffer(old_id);
        }
        if self
            .driver
            .flush_frame_dt(engine, &mut self.wl_surface, dt_ms)
        {
            let rects = self.wl_surface.committed_damage.len();
            let px: usize = self
                .wl_surface
                .committed_damage
                .iter()
                .map(|r| (r.w.max(0) as usize) * (r.h.max(0) as usize))
                .sum();
            FlushReport {
                dirty_regions: rects,
                strips_or_buffers: 1,
                pixels_updated: px,
            }
        } else {
            FlushReport::default()
        }
    }
}

impl Platform for WinitPlatform {
    #[inline]
    fn duration_since_start(&self) -> Duration {
        self.start_instant.elapsed()
    }

    fn step_platform(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> PlatformControlFlow {
        let report = self.flush_frame_dt(engine, dt_ms);
        if report.has_updates() || engine.needs_redraw() {
            PlatformControlFlow::Poll
        } else {
            PlatformControlFlow::Wait
        }
    }

    fn run_event_loop(self, engine: VeloxEngine) {
        self.run(engine, None);
    }
}

