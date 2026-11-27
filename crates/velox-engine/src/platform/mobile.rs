//! 移动端（Android `Surface` / `Choreographer` / JNI、iOS `UIKit` / `CADisplayLink` / `CAMetalLayer` 与鸿蒙 HarmonyOS NEXT `XComponent` / `OHNativeWindow` / N-API）跨平台适配层。
//!
//! 设计特点：
//! - **三大移动生态统一内核（Android + iOS + HarmonyOS NEXT）**：通过 [`MobileOs`] 统一抽象平台安全区域预设（Android 状态栏/手势横条、iOS 灵动岛/Home Indicator、鸿蒙 `AvoidArea`）、原生触控事件映射与 GPU Swapchain API（Vulkan / Metal）；
//! - **无全局单例（Multi-Instance View）**：每个 [`MobileEmbedApp`] 实例独立维护状态、帧缓冲与脏矩形队列，
//!   既可作为独立全屏移动应用运行，也可作为原生视图（Android `SurfaceView`、iOS `UIView`、鸿蒙 `XComponent`）在同一个宿主 App 的多个页面中并行嵌入；
//! - **多点触控主手指追踪（Primary Touch Tracking）**：自动跟踪首个按下的手指 `touch_id`，过滤多指干扰，支持左边缘右滑返回手势；
//! - **异形屏安全区域与软键盘避让（`SafeAreaInsets` & `KeyboardInset`）**：实时同步状态栏、灵动岛/刘海屏/挖孔屏、底部导航横条与软键盘高度；
//! - **单路径渲染后端分发**：当挂载了 [`WgpuWindowRenderer`](crate::renderers::wgpu::WgpuWindowRenderer) 硬件 Swapchain 时直接零拷贝呈现至 GPU，否则按激活后端（`Wgpu` / `Software`）单路径渲染至 `Rgba8888` 缓冲区。

use std::vec::Vec;

use crate::arena::SmallVec;
use crate::display::Rgba8888;
use crate::engine::VeloxEngine;
use crate::platform::testing::TestingPlatform;
use crate::pod::InputEvent;
use crate::pod::SafeAreaInsets;
use crate::solver::Region;

/// 移动端操作系统生态枚举（Android、iOS、鸿蒙 HarmonyOS NEXT / OpenHarmony）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum MobileOs {
    /// Google Android（`ANativeWindow` / `Choreographer` / JNI / Vulkan）。
    #[default]
    Android = 0,
    /// Apple iOS / iPadOS（`UIView` / `CADisplayLink` / `CAMetalLayer` / Metal）。
    Ios = 1,
    /// 华为鸿蒙 HarmonyOS NEXT / OpenHarmony（`OH_NativeXComponent` / `OHNativeWindow` / ArkTS N-API / Vulkan）。
    HarmonyOs = 2,
}

impl MobileOs {
    /// 获取当前移动操作系统的人类可读名称。
    #[inline(always)]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Android => "Android",
            Self::Ios => "iOS",
            Self::HarmonyOs => "HarmonyOS NEXT",
        }
    }

    /// 获取当前移动操作系统对应的原生低开销硬件加速图形 API 名称。
    #[inline(always)]
    pub const fn gpu_api_name(self) -> &'static str {
        match self {
            Self::Android | Self::HarmonyOs => "Vulkan",
            Self::Ios => "Metal",
        }
    }

    /// 获取当前移动操作系统在指定屏幕朝向下的标准安全区域预设。
    #[inline(always)]
    pub const fn default_safe_area(self, orientation: DeviceOrientation) -> SafeAreaInsets {
        let is_portrait = matches!(
            orientation,
            DeviceOrientation::Portrait | DeviceOrientation::PortraitUpsideDown
        );
        match (self, is_portrait) {
            (Self::Android, true) => SafeAreaInsets::android_portrait(),
            (Self::Android, false) => SafeAreaInsets::android_landscape(),
            (Self::Ios, true) => SafeAreaInsets::ios_portrait(),
            (Self::Ios, false) => SafeAreaInsets::ios_landscape(),
            (Self::HarmonyOs, true) => SafeAreaInsets::harmony_portrait(),
            (Self::HarmonyOs, false) => SafeAreaInsets::harmony_landscape(),
        }
    }
}

/// 移动端触摸事件阶段（对齐 Android `MotionEvent.ACTION_*`、iOS `UITouchPhase` 与鸿蒙 `OH_NativeXComponent_TouchEventType`）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TouchPhase {
    /// 手指接触屏幕（`ACTION_DOWN` / `UITouchPhaseBegan` / `OH_NATIVEXCOMPONENT_DOWN`）。
    Began = 0,
    /// 手指在屏幕上移动（`ACTION_MOVE` / `UITouchPhaseMoved` / `OH_NATIVEXCOMPONENT_MOVE`）。
    Moved = 1,
    /// 手指离开屏幕（`ACTION_UP` / `UITouchPhaseEnded` / `OH_NATIVEXCOMPONENT_UP`）。
    Ended = 2,
    /// 触摸被系统手势或来电中断取消（`ACTION_CANCEL` / `UITouchPhaseCancelled` / `OH_NATIVEXCOMPONENT_CANCEL`）。
    Cancelled = 3,
}

impl TouchPhase {
    /// 将 Android `MotionEvent.getActionMasked()` (`0=DOWN, 1=UP, 2=MOVE, 3=CANCEL`) 映射为 [`TouchPhase`]。
    #[inline(always)]
    pub const fn from_android_action(action: u8) -> Self {
        match action {
            0 => Self::Began,
            1 => Self::Ended,
            2 => Self::Moved,
            _ => Self::Cancelled,
        }
    }

    /// 将 iOS `UITouchPhase` (`0=Began, 1=Moved, 2=Stationary, 3=Ended, 4=Cancelled`) 映射为 [`TouchPhase`]。
    #[inline(always)]
    pub const fn from_ios_phase(phase: u8) -> Self {
        match phase {
            0 => Self::Began,
            1 | 2 => Self::Moved,
            3 => Self::Ended,
            _ => Self::Cancelled,
        }
    }

    /// 将鸿蒙 `OH_NativeXComponent_TouchEventType` (`0=DOWN, 1=UP, 2=MOVE, 3=CANCEL`) 映射为 [`TouchPhase`]。
    #[inline(always)]
    pub const fn from_harmony_touch_type(touch_type: u8) -> Self {
        match touch_type {
            0 => Self::Began,
            1 => Self::Ended,
            2 => Self::Moved,
            _ => Self::Cancelled,
        }
    }
}

/// 移动设备屏幕朝向枚举。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum DeviceOrientation {
    /// 竖屏正立（默认）。
    #[default]
    Portrait = 0,
    /// 横屏（顶部在左）。
    LandscapeLeft = 1,
    /// 横屏（顶部在右）。
    LandscapeRight = 2,
    /// 竖屏倒置。
    PortraitUpsideDown = 3,
}

/// 供跨语言桥接（Android JNI / iOS Swift / 鸿蒙 ArkTS N-API / WebAssembly）零拷贝读取的紧凑脏矩形结构。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct CDirtyRect {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

/// `#![no_std]` 固定分辨率移动端无头测试平台别名（统一复用泛型 [`TestingPlatform`]，消除重复结构体）。
pub type MobilePlatform<const W: usize = 240, const H: usize = 360> =
    TestingPlatform<W, H, Rgba8888>;

/// 面向 Android (`SurfaceView` / `ANativeWindow` Vulkan / JNI)、iOS (`UIView` / `CAMetalLayer` / Swift `xcframework`)
/// 与鸿蒙 HarmonyOS NEXT (`XComponent` / `OHNativeWindow` Vulkan / ArkTS N-API) 嵌入场景的动态分辨率宿主实例。
pub struct MobileEmbedApp {
    pub engine: VeloxEngine,
    os: MobileOs,
    width: i16,
    height: i16,
    dpi_scale: i16,
    orientation: DeviceOrientation,
    backend: super::DesktopRenderBackend,
    framebuffer: Vec<Rgba8888>,
    dirty_rects: Vec<CDirtyRect>,
    active_touch_id: Option<u32>,
    #[cfg(feature = "gpu")]
    gpu_offscreen: Option<crate::renderers::wgpu::WgpuOffscreenRenderer>,
    #[cfg(feature = "gpu")]
    gpu_window: Option<crate::renderers::wgpu::WgpuWindowRenderer>,
}

impl MobileEmbedApp {
    /// 创建一个可嵌入原生移动视图层级的独立引擎实例（默认采用 `Wgpu` 2D SDF 硬件加速后端）。
    pub fn new(width: i16, height: i16, safe_area: SafeAreaInsets) -> Self {
        Self::new_for_os(MobileOs::Android, width, height, safe_area)
    }

    /// 按指定移动端操作系统生态（[`MobileOs`]）与安全区域创建独立引擎实例。
    pub fn new_for_os(os: MobileOs, width: i16, height: i16, safe_area: SafeAreaInsets) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let mut engine = VeloxEngine::new(w, h);
        engine.set_safe_area(safe_area);
        Self {
            engine,
            os,
            width: w,
            height: h,
            dpi_scale: 1,
            orientation: DeviceOrientation::Portrait,
            backend: super::DesktopRenderBackend::default_desktop(),
            // 按需懒分配：走原生 GPU Swapchain 直出时保持 0 字节，避免无条件常驻整屏 CPU 帧缓冲。
            framebuffer: Vec::new(),
            dirty_rects: Vec::with_capacity(8),
            active_touch_id: None,
            #[cfg(feature = "gpu")]
            gpu_offscreen: None,
            #[cfg(feature = "gpu")]
            gpu_window: None,
        }
    }

    /// 创建搭载 Android 默认竖屏安全区（状态栏 `48px`、手势导航条 `34px`）的实例。
    #[inline]
    pub fn new_android(width: i16, height: i16) -> Self {
        Self::new_for_os(
            MobileOs::Android,
            width,
            height,
            SafeAreaInsets::android_portrait(),
        )
    }

    /// 创建搭载 iOS 默认竖屏安全区（灵动岛 `59pt`、Home Indicator `34pt`）的实例。
    #[inline]
    pub fn new_ios(width: i16, height: i16) -> Self {
        Self::new_for_os(MobileOs::Ios, width, height, SafeAreaInsets::ios_portrait())
    }

    /// 创建搭载鸿蒙 HarmonyOS NEXT 默认竖屏避让区（状态栏 `38vp`、导航指示条 `28vp`）的实例。
    #[inline]
    pub fn new_harmony(width: i16, height: i16) -> Self {
        Self::new_for_os(
            MobileOs::HarmonyOs,
            width,
            height,
            SafeAreaInsets::harmony_portrait(),
        )
    }

    /// 配置当前实例所属的移动操作系统生态类型。
    #[inline]
    pub const fn with_os(mut self, os: MobileOs) -> Self {
        self.os = os;
        self
    }

    /// 获取当前实例所属的移动操作系统生态类型。
    #[inline(always)]
    pub const fn os(&self) -> MobileOs {
        self.os
    }

    /// 配置移动端渲染后端类型（`Wgpu` 或 `Software`）。
    #[inline]
    pub fn with_backend(mut self, backend: super::DesktopRenderBackend) -> Self {
        self.backend = backend;
        self
    }

    /// 动态切换移动端渲染后端类型。
    #[inline]
    pub fn set_backend(&mut self, backend: super::DesktopRenderBackend) {
        self.backend = backend;
    }

    /// 获取当前激活的移动端渲染后端类型。
    #[inline(always)]
    pub const fn backend(&self) -> super::DesktopRenderBackend {
        self.backend
    }

    /// 获取当前设备屏幕朝向。
    #[inline(always)]
    pub const fn orientation(&self) -> DeviceOrientation {
        self.orientation
    }

    /// 切换设备屏幕朝向并自动应用当前移动操作系统（Android / iOS / HarmonyOS）对应朝向的默认安全区域。
    pub fn set_orientation(&mut self, orientation: DeviceOrientation) {
        self.orientation = orientation;
        let insets = self.os.default_safe_area(orientation);
        self.engine.set_safe_area(insets);
    }

    /// 获取最近一帧离屏渲染编译生成的轻量级 2D SDF GPU 场景图元统计引用。
    #[cfg(feature = "gpu")]
    #[inline]
    pub fn gpu_scene(&self) -> Option<&crate::renderers::wgpu::WgpuScene> {
        self.gpu_offscreen.as_ref().map(|r| &r.scene)
    }

    /// 绑定原生移动端窗口句柄（Android `ANativeWindow` / iOS `UIView` `CAMetalLayer` / HarmonyOS `OHNativeWindow`），
    /// 开启轻量级 `wgpu` (Vulkan / Metal) 2D SDF 120Hz 零拷贝 Swapchain 直通硬件加速渲染。
    ///
    /// 绑定成功后会自动释放不再需要的 CPU `framebuffer` 与离屏渲染器内存。
    #[cfg(feature = "gpu")]
    pub fn attach_gpu_window<T>(&mut self, window_handle: T) -> bool
    where
        T: Into<crate::renderers::wgpu::wgpu::SurfaceTarget<'static>> + Clone,
    {
        let pw = self.physical_width().max(1) as u32;
        let ph = self.physical_height().max(1) as u32;
        self.gpu_window = crate::renderers::wgpu::WgpuWindowRenderer::new(window_handle, pw, ph);
        if self.gpu_window.is_some() {
            self.framebuffer = Vec::new();
            self.gpu_offscreen = None;
            true
        } else {
            false
        }
    }

    /// 解绑/挂起移动端 GPU Swapchain 表面。
    #[cfg(feature = "gpu")]
    pub fn detach_gpu_window(&mut self) {
        self.gpu_window = None;
    }

    /// 后台化或收到系统内存压力回调（`onTrimMemory` / `didReceiveMemoryWarning`）时释放闲置 CPU 帧缓冲与 GPU 离屏显存。
    pub fn trim_memory(&mut self) {
        self.framebuffer = Vec::new();
        self.engine.resize(self.width, self.height);
        #[cfg(feature = "gpu")]
        {
            if let Some(off) = &mut self.gpu_offscreen {
                off.trim_cpu_buffers();
                off.trim_gpu_memory();
            }
            if let Some(win) = &mut self.gpu_window {
                win.trim_gpu_memory();
            }
        }
    }

    /// 返回当前移动端实例 CPU 侧 `framebuffer` 占用的字节容量（原生 GPU Swapchain 路径下为 `0`）。
    #[inline]
    pub fn cpu_framebuffer_bytes(&self) -> usize {
        self.framebuffer.capacity() * core::mem::size_of::<Rgba8888>()
    }

    /// 返回当前移动端实例在 CPU 侧持有的像素缓冲总字节容量（包含 `framebuffer` 与离屏渲染器内部缓冲）。
    #[inline]
    pub fn cpu_memory_bytes(&self) -> usize {
        #[allow(unused_mut)]
        let mut total = self.cpu_framebuffer_bytes();
        #[cfg(feature = "gpu")]
        if let Some(off) = &self.gpu_offscreen {
            total = total.saturating_add(off.cpu_memory_bytes());
        }
        total
    }

    fn ensure_cpu_framebuffer(&mut self) {
        let pw = self.physical_width().max(1) as usize;
        let ph = self.physical_height().max(1) as usize;
        let needed = pw.saturating_mul(ph);
        if self.framebuffer.len() != needed {
            self.framebuffer.resize(needed, Rgba8888(0));
        }
    }

    /// 检查当前是否已挂载并激活硬件 GPU Swapchain 表面。
    #[cfg(feature = "gpu")]
    #[inline]
    pub fn has_active_gpu_window(&self) -> bool {
        self.gpu_window.is_some()
    }

    /// 检查当前是否已挂载并激活硬件 GPU Swapchain 表面。
    #[cfg(not(feature = "gpu"))]
    #[inline]
    pub const fn has_active_gpu_window(&self) -> bool {
        false
    }

    /// 设置移动端物理像素 HiDPI 缩放倍率。
    pub fn set_dpi_scale(&mut self, scale: i16) {
        let s = scale.max(1);
        if s != self.dpi_scale {
            self.dpi_scale = s;
            self.engine.set_dpi_scale(s);
            let pw = (self.width as usize) * (s as usize);
            let ph = (self.height as usize) * (s as usize);
            if !self.framebuffer.is_empty() && !self.has_active_gpu_window() {
                self.framebuffer.resize(pw * ph, Rgba8888(0));
            }
            #[cfg(feature = "gpu")]
            if let Some(gpu_win) = &mut self.gpu_window {
                gpu_win.resize(pw.max(1) as u32, ph.max(1) as u32);
            }
            self.engine.resize(self.width, self.height);
        }
    }

    /// 获取当前 HiDPI 缩放倍率。
    #[inline(always)]
    pub const fn dpi_scale(&self) -> i16 {
        self.dpi_scale
    }

    /// 获取物理像素帧缓冲宽度（`logical_width * dpi_scale`）。
    #[inline(always)]
    pub const fn physical_width(&self) -> i16 {
        self.width.saturating_mul(self.dpi_scale)
    }

    /// 获取物理像素帧缓冲高度（`logical_height * dpi_scale`）。
    #[inline(always)]
    pub const fn physical_height(&self) -> i16 {
        self.height.saturating_mul(self.dpi_scale)
    }

    /// 调整视图逻辑宽高（如屏幕旋转或分屏窗口大小改变），保留全部组件状态。
    pub fn resize(&mut self, width: i16, height: i16) {
        let w = width.max(1);
        let h = height.max(1);
        if w != self.width || h != self.height {
            self.width = w;
            self.height = h;
            let pw = (w as usize) * (self.dpi_scale as usize);
            let ph = (h as usize) * (self.dpi_scale as usize);
            if !self.framebuffer.is_empty() && !self.has_active_gpu_window() {
                self.framebuffer.resize(pw * ph, Rgba8888(0));
            }
            #[cfg(feature = "gpu")]
            if let Some(gpu_win) = &mut self.gpu_window {
                gpu_win.resize(pw.max(1) as u32, ph.max(1) as u32);
            }
            self.engine.resize(w, h);
        }
    }

    /// 注入触摸事件（自动锁定并跟踪主触控手指 `touch_id`）。
    pub fn push_touch(&mut self, touch_id: u32, phase: TouchPhase, x: i16, y: i16) {
        match phase {
            TouchPhase::Began => {
                if self.active_touch_id.is_none() {
                    self.active_touch_id = Some(touch_id);
                    self.engine.dispatch_touch_down(x, y);
                }
            }
            TouchPhase::Moved => {
                if self.active_touch_id == Some(touch_id) {
                    self.engine.dispatch_event(InputEvent::PointerMove { x, y });
                }
            }
            TouchPhase::Ended => {
                if self.active_touch_id == Some(touch_id) {
                    self.active_touch_id = None;
                    self.engine.dispatch_event(InputEvent::PointerUp { x, y });
                }
            }
            TouchPhase::Cancelled => {
                if self.active_touch_id == Some(touch_id) {
                    self.active_touch_id = None;
                    self.engine.dispatch_event(InputEvent::PointerCancel);
                }
            }
        }
    }

    /// 处理系统返回键/返回手势，返回是否被 Velox 视图消费。
    pub fn handle_back(&mut self) -> bool {
        self.engine.dispatch_navigate_back()
    }

    /// 动态更新异形屏安全区域内边距（状态栏、灵动岛/刘海、底部 Home Indicator）。
    pub fn set_safe_area(&mut self, safe_area: SafeAreaInsets) {
        self.engine.set_safe_area(safe_area);
    }

    /// 动态更新软键盘弹出高度（像素）。
    pub fn set_keyboard_inset(&mut self, height_px: i16) {
        self.engine.set_keyboard_inset(height_px);
    }

    /// 注入原生移动端事件循环唤醒器（例如 Android `ALooper_wake` / iOS `CFRunLoopWakeUp` / 鸿蒙 `OH_NativeVSync` 请求帧），
    /// 实现后台异步任务完成与定时器到期时的零轮询精准唤醒。
    pub fn set_async_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.engine.set_async_waker(waker);
    }

    /// 在移动端 UI 主线程排空所有已就绪的前景异步协程与 `defer` 回调。
    #[inline]
    pub fn poll_async_tasks(&mut self) -> bool {
        self.engine.poll_async_tasks()
    }

    /// 检查当前引擎是否存在待重绘脏视图、动画或已就绪异步任务（用于决定 `Choreographer` / `CADisplayLink` 是否休眠）。
    #[inline]
    pub fn needs_redraw(&self) -> bool {
        self.engine.needs_redraw()
    }

    /// 推进一帧，按当前激活的 `backend` 单路径执行渲染（消除三重重复渲染），返回本帧重绘的脏矩形数量。
    pub fn step_frame(&mut self, dt_ms: u16) -> usize {
        self.dirty_rects.clear();
        let mut regions: SmallVec<Region, 8> = SmallVec::new();
        for &r in self.engine.step_frame_dt(dt_ms) {
            regions.push(r);
        }
        if regions.is_empty() {
            return 0;
        }
        let s = self.dpi_scale.max(1);
        for &reg in regions.as_slice() {
            self.dirty_rects.push(CDirtyRect {
                x: reg.x.saturating_mul(s),
                y: reg.y.saturating_mul(s),
                w: reg.w.saturating_mul(s),
                h: reg.h.saturating_mul(s),
            });
        }

        // 1. 若已绑定原生 GPU 窗口 Swapchain，直接通过 WgpuWindowRenderer 上屏并返回，不再执行离屏/软件光栅
        #[cfg(feature = "gpu")]
        if self.backend == super::DesktopRenderBackend::Wgpu {
            if let Some(gpu_win) = &mut self.gpu_window {
                let viewport = self.engine.viewport_region();
                let ops = self.engine.cached_draw_ops();
                if gpu_win.render(ops, viewport, s) {
                    return self.dirty_rects.len();
                }
            }
        }

        let _pw = self.physical_width().max(1) as usize;
        let _ph = self.physical_height().max(1) as usize;
        self.ensure_cpu_framebuffer();

        // 2. 离屏 / ANativeWindow 缓冲区模式：按选定后端单路径直写 `self.framebuffer`，消除多余整屏 CPU 副本
        match self.backend {
            super::DesktopRenderBackend::Wgpu => {
                #[cfg(feature = "gpu")]
                {
                    use crate::renderers::wgpu::WgpuOffscreenRenderer;
                    let offscreen = self
                        .gpu_offscreen
                        .get_or_insert_with(|| WgpuOffscreenRenderer::new_lazy(_pw, _ph));
                    let _ = offscreen.render_draw_ops_rgba8888_into(
                        self.engine.cached_draw_ops(),
                        self.engine.viewport_region(),
                        s,
                        regions.as_slice(),
                        self.framebuffer.as_mut_slice(),
                    );
                }
                #[cfg(not(feature = "gpu"))]
                {
                    for &reg in regions.as_slice() {
                        self.engine
                            .rasterize_region(reg, self.framebuffer.as_mut_slice());
                    }
                }
            }
            super::DesktopRenderBackend::Software => {
                for &reg in regions.as_slice() {
                    self.engine
                        .rasterize_region(reg, self.framebuffer.as_mut_slice());
                }
            }
        }

        self.dirty_rects.len()
    }

    /// 获取 `RGBA8888` 像素缓冲区的只读字节切片。
    #[inline(always)]
    pub fn rgba_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.framebuffer.as_ptr() as *const u8,
                self.framebuffer.len() * 4,
            )
        }
    }

    /// 获取本帧脏矩形切片。
    #[inline(always)]
    pub fn dirty_rects(&self) -> &[CDirtyRect] {
        self.dirty_rects.as_slice()
    }
}

/// 供 Android JNI、iOS Swift `xcframework` 与鸿蒙 ArkTS N-API 共享复用的底层 C ABI 辅助函数模块。
pub mod ffi {
    use std::boxed::Box;

    use super::{CDirtyRect, MobileEmbedApp, TouchPhase};
    use crate::pod::SafeAreaInsets;

    /// 销毁通过 FFI 创建的 [`MobileEmbedApp`] 实例。
    #[inline]
    pub unsafe fn destroy(app: *mut MobileEmbedApp) {
        if !app.is_null() {
            unsafe {
                drop(Box::from_raw(app));
            }
        }
    }

    /// 查询当前移动端实例是否启用了基于轻量 `wgpu` 2D SDF 的硬件 GPU 渲染后端。
    #[inline]
    pub unsafe fn backend_is_gpu(app: *const MobileEmbedApp) -> bool {
        if let Some(app_ref) = unsafe { app.as_ref() } {
            app_ref.backend().is_gpu()
        } else {
            false
        }
    }

    /// 调整屏幕旋转或窗口分屏后的逻辑尺寸。
    #[inline]
    pub unsafe fn resize(app: *mut MobileEmbedApp, width: i16, height: i16) {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.resize(width, height);
        }
    }

    /// 设置屏幕整数倍像素缩放因子（`@1x` / `@2x` / `@3x`）。
    #[inline]
    pub unsafe fn set_dpi_scale(app: *mut MobileEmbedApp, scale: i16) {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.set_dpi_scale(scale);
        }
    }

    /// 更新移动端异形屏安全区域内边距。
    #[inline]
    pub unsafe fn set_safe_area(
        app: *mut MobileEmbedApp,
        top: i16,
        right: i16,
        bottom: i16,
        left: i16,
    ) {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.set_safe_area(SafeAreaInsets::new(top, right, bottom, left));
        }
    }

    /// 更新移动端软键盘底部遮挡高度。
    #[inline]
    pub unsafe fn set_keyboard_inset(app: *mut MobileEmbedApp, height_px: i16) {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.set_keyboard_inset(height_px);
        }
    }

    /// 注入已归一化的 [`TouchPhase`] 触控事件。
    #[inline]
    pub unsafe fn push_touch(
        app: *mut MobileEmbedApp,
        touch_id: u32,
        phase: TouchPhase,
        x: i16,
        y: i16,
    ) {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.push_touch(touch_id, phase, x, y);
        }
    }

    /// 注入原始数值编码（`0=Began, 1=Moved, 2=Ended, 3=Cancelled`）的触控事件。
    #[inline]
    pub unsafe fn push_touch_raw(
        app: *mut MobileEmbedApp,
        touch_id: u32,
        phase: u8,
        x: i16,
        y: i16,
    ) {
        let p = match phase {
            0 => TouchPhase::Began,
            1 => TouchPhase::Moved,
            2 => TouchPhase::Ended,
            _ => TouchPhase::Cancelled,
        };
        unsafe {
            push_touch(app, touch_id, p, x, y);
        }
    }

    /// 触发系统返回手势或返回按键，返回 `true` 表示已被应用内部消费。
    #[inline]
    pub unsafe fn handle_back(app: *mut MobileEmbedApp) -> bool {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.handle_back()
        } else {
            false
        }
    }

    /// 在移动端 UI 主线程排空所有已就绪的前景异步协程与 `defer` 回调。
    #[inline]
    pub unsafe fn poll_async_tasks(app: *mut MobileEmbedApp) -> bool {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.poll_async_tasks()
        } else {
            false
        }
    }

    /// 检查当前实例是否存在待重绘脏区、活跃动画或已就绪异步任务。
    #[inline]
    pub unsafe fn needs_redraw(app: *const MobileEmbedApp) -> bool {
        if let Some(app_ref) = unsafe { app.as_ref() } {
            app_ref.needs_redraw()
        } else {
            false
        }
    }

    /// 推进一帧（`dt_ms` 毫秒），返回本帧脏矩形数量。
    #[inline]
    pub unsafe fn step_frame(app: *mut MobileEmbedApp, dt_ms: u16) -> usize {
        if let Some(app_ref) = unsafe { app.as_mut() } {
            app_ref.step_frame(dt_ms)
        } else {
            0
        }
    }

    /// 获取 `RGBA8888` 像素缓冲区的只读首地址。
    #[inline]
    pub unsafe fn framebuffer_ptr(app: *const MobileEmbedApp) -> *const u8 {
        if let Some(app_ref) = unsafe { app.as_ref() } {
            app_ref.rgba_bytes().as_ptr()
        } else {
            core::ptr::null()
        }
    }

    /// 获取本帧脏矩形数组的首地址。
    #[inline]
    pub unsafe fn dirty_rects_ptr(app: *const MobileEmbedApp) -> *const CDirtyRect {
        if let Some(app_ref) = unsafe { app.as_ref() } {
            app_ref.dirty_rects().as_ptr()
        } else {
            core::ptr::null()
        }
    }

    /// 获取本帧脏矩形数组的元素个数。
    #[inline]
    pub unsafe fn dirty_rects_count(app: *const MobileEmbedApp) -> usize {
        if let Some(app_ref) = unsafe { app.as_ref() } {
            app_ref.dirty_rects().len()
        } else {
            0
        }
    }
}
