//! 跨平台窗口、显示表面与事件循环抽象层。
//!
//! 统一内聚并适配多端平台后端，并为外部专有后端提供 [`Platform`]、[`PlatformSurface`]、[`AsyncPlatform`]、[`AsyncPlatformSurface`] 等通用特征抽象：
//! - [`testing`]（`#![no_std]`）：提供确定性无头事件注入、虚拟时钟推进与像素断言平台；
//! - [`wayland`]（`std`）：提供纯 Wayland 客户端平台 [`wayland::WaylandPlatform`]（支持 `xdg_toplevel`、`zwlr_layer_shell_v1`、`zxdg_decoration_v1` 与 `wl_shm` 双缓冲脏区回溯同步，不依赖 `winit`）；
//! - [`winit`]（`std`）：提供跨桌面宿主窗口的事件驱动调试平台 [`winit::WinitPlatform`]；
//! - [`mobile`] / [`android`] / [`ios`] / [`harmony`]：提供移动端触控、安全区、IME 与原生窗口生命周期桥接；
//! - [`web`]：提供 WebAssembly HTML5 Canvas 像素输出与脏区上报平台。

use core::future::Future;
use core::time::Duration;

use crate::engine::VeloxEngine;

pub mod clipboard;
pub mod mobile;
pub mod testing;
pub mod web;

pub use clipboard::{clipboard_has_text, clipboard_read_into, clipboard_write_text};
pub use clipboard::clipboard_read_text;

pub mod a11y_bridge;
pub mod android;
pub mod embed_ffi;
pub mod harmony;
pub mod ios;
pub mod os_integration;
pub mod shm_driver;
pub mod wayland;
pub mod winit;


pub use crate::renderers::RepaintBufferType;

/// 桌面与移动端平台默认及可选渲染后端类型。
///
/// - 桌面端（[`winit::WinitPlatform`] / [`wayland::WaylandPlatform`]）与移动端（[`mobile::MobileEmbedApp`]）默认采用自研轻量级 `wgpu` 2D SDF GPU 渲染器（[`DesktopRenderBackend::Wgpu`]，跨平台统一驱动 Windows D3D12、macOS/iOS Metal、Linux/Android/HarmonyOS Vulkan）；
/// - 轻量化与受限显示环境可使用内置零外部依赖 SDF 扫描线软件渲染器（[`DesktopRenderBackend::Software`]）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesktopRenderBackend {
    /// 自研轻量级 `wgpu` 2D SDF GPU 硬件加速后端（Windows D3D12 / macOS & iOS Metal / Linux & Android Vulkan）。
    Wgpu,
    /// 多线程按水平条带并行的 SDF 扫描线软件渲染后端。
    Software,
}

impl DesktopRenderBackend {
    /// 返回桌面与移动端平台的默认渲染后端：
    /// - 若启用了 `gpu`（默认开启），选择 [`DesktopRenderBackend::Wgpu`]；
    /// - 否则回退至内置 [`DesktopRenderBackend::Software`]。
    #[inline(always)]
    pub const fn default_desktop() -> Self {
        #[cfg(feature = "gpu")]
        {
            Self::Wgpu
        }
        #[cfg(not(feature = "gpu"))]
        {
            Self::Software
        }
    }

    /// 判断当前后端是否为 GPU 硬件加速后端。
    #[inline(always)]
    pub const fn is_gpu(self) -> bool {
        matches!(self, Self::Wgpu)
    }
}

impl Default for DesktopRenderBackend {
    #[inline(always)]
    fn default() -> Self {
        Self::default_desktop()
    }
}

/// 平台事件循环控制流状态。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PlatformControlFlow {
    /// 存在进行中的过渡动画或待处理事件，下一帧继续立即轮询。
    Poll,
    /// 界面处于完全静止状态（`!engine.needs_redraw()`），平台可进入 `WFI` 中断休眠或阻塞等待输入，实现 0% CPU 占用。
    Wait,
    /// 退出平台事件循环。
    Exit,
}

/// 单帧平台表面刷新统计报告。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct FlushReport {
    /// 本帧合并提交的脏矩形数量（`0` 表示静止帧未触发重绘）。
    pub dirty_regions: usize,
    /// 本帧传输的条带数（MCU 模式）或提交的共享内存缓冲区数（Winit/Wayland/KMS/桌面模式）。
    pub strips_or_buffers: usize,
    /// 本帧实际光栅化并刷新的像素总数。
    pub pixels_updated: usize,
    /// 本帧瞬态内存池（`ScratchArena`）触及的峰值占用字节数。
    pub arena_peak_bytes: usize,
    /// 本帧 `ScratchArena` 发生静默字符串截断或临时切片空降级的次数（非 0 说明竞技场容量紧张）。
    pub arena_truncations: u32,
}

impl FlushReport {
    /// 编译期空刷新报告常量（供 `const fn` 平台构造器使用）。
    #[inline(always)]
    pub const fn const_empty() -> Self {
        Self {
            dirty_regions: 0,
            strips_or_buffers: 0,
            pixels_updated: 0,
            arena_peak_bytes: 0,
            arena_truncations: 0,
        }
    }

    /// 构造并自动附加来自 [`VeloxEngine`] 的 `ScratchArena` 峰值水位与静默截断遥测。
    #[inline(always)]
    pub fn with_engine(
        dirty_regions: usize,
        strips_or_buffers: usize,
        pixels_updated: usize,
        engine: &VeloxEngine,
    ) -> Self {
        Self {
            dirty_regions,
            strips_or_buffers,
            pixels_updated,
            arena_peak_bytes: engine.last_arena_peak_bytes(),
            arena_truncations: engine.last_arena_truncations(),
        }
    }

    /// 本帧是否发生了实际像素重绘。
    #[inline(always)]
    pub const fn has_updates(&self) -> bool {
        self.dirty_regions > 0
    }
}

/// 平台显示表面抽象。
///
/// 负责将 [`VeloxEngine`] 求解出的脏矩形区域桥接至底层显示设备
/// （如 Winit/Wayland 双缓冲表面、移动端图形表面、Web Canvas 或外部自定义显示驱动）。
pub trait PlatformSurface {
    /// 获取当前显示表面的逻辑宽高 `(width, height)`。
    fn surface_size(&self) -> (i16, i16);

    /// 按给定时间步长 `dt_ms`（毫秒）推进一帧 [`VeloxEngine`] 并刷新所有脏区到显示设备。
    fn flush_frame_dt(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> FlushReport;

    /// 以默认 60FPS（`16ms`）步长执行一帧刷新。
    #[inline]
    fn flush_frame(&mut self, engine: &mut VeloxEngine) -> FlushReport {
        self.flush_frame_dt(engine, 16)
    }
}

/// 核心平台抽象接口。
///
/// 负责提供单调时钟源、平台输入事件收集、显示表面刷屏以及主事件循环调度。
pub trait Platform {
    /// 返回自平台启动以来的单调递增时长（桌面端默认由 `Instant` 提供）。
    fn duration_since_start(&self) -> Duration {
        Duration::ZERO
    }

    /// 单步执行一次平台事件采集、状态更新与按需脏区刷屏，返回下一次循环的 [`PlatformControlFlow`] 调度策略。
    fn step_platform(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> PlatformControlFlow;

    /// 启动平台主事件循环，接管 [`VeloxEngine`] 直至收到 [`PlatformControlFlow::Exit`]。
    fn run_event_loop(mut self, mut engine: VeloxEngine)
    where
        Self: Sized,
    {
        let mut last_tick = std::time::Instant::now();
        loop {
            let dt_ms = {
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_tick).as_millis();
                last_tick = now;
                elapsed.clamp(1, 100) as u16
            };

            match self.step_platform(&mut engine, dt_ms) {
                PlatformControlFlow::Poll => {
                    {
                        let frame_budget = Duration::from_millis(16);
                        let spent = last_tick.elapsed();
                        if spent < frame_budget {
                            std::thread::sleep(frame_budget - spent);
                        }
                    }
                }
                PlatformControlFlow::Wait => {
                    std::thread::sleep(Duration::from_millis(16));
                }
                PlatformControlFlow::Exit => break,
            }
        }
    }
}

/// 面向异步运行时的平台显示表面抽象（非阻塞异步帧刷新）。
pub trait AsyncPlatformSurface {
    /// 获取当前显示表面的逻辑宽高 `(width, height)`。
    fn surface_size(&self) -> (i16, i16);

    /// 按给定时间步长 `dt_ms`（毫秒）异步推进一帧 [`VeloxEngine`] 并刷新所有脏区。
    fn flush_frame_dt_async(
        &mut self,
        engine: &mut VeloxEngine,
        dt_ms: u16,
    ) -> impl Future<Output = FlushReport>;

    /// 以默认 60FPS（`16ms`）步长异步执行一帧刷新。
    fn flush_frame_async(&mut self, engine: &mut VeloxEngine) -> impl Future<Output = FlushReport> {
        self.flush_frame_dt_async(engine, 16)
    }
}

/// 面向异步执行器的核心平台抽象接口（零忙等阻塞，帧刷新与空闲等待全程 `.await` 让出执行权）。
pub trait AsyncPlatform {
    /// 返回自平台启动以来的单调递增时长。
    fn duration_since_start(&self) -> Duration {
        Duration::ZERO
    }

    /// 异步单步执行一次平台事件采集、状态更新与非阻塞 DMA 脏区刷屏，返回下一次循环的 [`PlatformControlFlow`]。
    fn step_platform_async(
        &mut self,
        engine: &mut VeloxEngine,
        dt_ms: u16,
    ) -> impl Future<Output = PlatformControlFlow>;
}
