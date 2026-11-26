//! 全平台零拷贝宿主嵌入统一 C ABI（`velox_engine::platform::embed_ffi`）。
//!
//! 为外部 UI 框架（Qt6 `QQuickRhiItem`、GTK4 `GdkDmabufTextureBuilder`、WinUI 3 `SurfaceImageSource`、
//! WPF `D3DImage`、Android `ASurfaceTransaction`、iOS `IOSurface` 与原生合成器）提供一套统一句柄共享协议：
//! - **GPU 零拷贝优先**：启用 `gpu`（及 `gpu-interop`）且物理 GPU 可用时，通过 [`WgpuSharedExportRenderer`] 导出
//!   Win32 DX12 NT `HANDLE` / Linux `dma_buf` fd / Apple `IOSurfaceRef` / Android `AHardwareBuffer*` 与 GPU 同步栅栏；
//! - **软件缓冲自动回退**：在无 GPU 环境或显式关闭 GPU 时无缝回退至 `Rgba8888` CPU 帧缓冲 + [`CDirtyRect`] 局部脏区；
//! - **预乘透明 HUD 支持**：`transparent_bg = 1` 时背景全透明，可直接零拷贝叠加在外部应用画面之上。

use std::boxed::Box;
use std::vec;
use std::vec::Vec;

use crate::display::Rgba8888;
use crate::element::View;
use crate::engine::VeloxEngine;
use crate::platform::mobile::CDirtyRect;
use crate::pod::{InputEvent, KeyCode};
use crate::renderers::software::rasterize_region_scaled_parallel;

#[cfg(feature = "gpu")]
use crate::renderers::wgpu::{ExportedHandle, ExternalSyncHandle, WgpuSharedExportRenderer};

/// C ABI 导出的共享帧句柄类别。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum VeloxHandleKind {
    #[default]
    None = 0,
    Win32Dx12NtHandle = 1,
    LinuxDmaBufFd = 2,
    AppleIoSurface = 3,
    AndroidHardwareBuffer = 4,
    SoftwareRgbaPtr = 5,
}

/// C ABI 导出的 GPU 同步原语类别。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum VeloxSyncKind {
    #[default]
    None = 0,
    Win32Dx12FenceNtHandle = 1,
    LinuxSemaphoreFd = 2,
    AppleMetalSharedEvent = 3,
    AndroidSyncFd = 4,
}

/// 宿主传入的物理显卡对齐与后端初始化配置（`#[repr(C)]`）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct VeloxHostAdapterConfig {
    /// `1` 表示 `adapter_luid` 有效（Windows DX12 同卡对齐）。
    pub has_luid: u8,
    /// Windows DXGI 8 字节 `AdapterLuid`。
    pub adapter_luid: [u8; 8],
    /// `1` 表示 `device_uuid` 有效（Linux/Android Vulkan 同卡对齐）。
    pub has_uuid: u8,
    /// Vulkan 16 字节 `deviceUUID`。
    pub device_uuid: [u8; 16],
    /// `1` 优先启用硬件 GPU 零拷贝导出，`0` 强制使用内置软件光栅器。
    pub prefer_gpu: u8,
    /// `1` 启用预乘 Alpha 全透明背景（用于悬浮镂空 HUD 合成），`0` 为不透明背景。
    pub transparent_bg: u8,
}

impl Default for VeloxHostAdapterConfig {
    fn default() -> Self {
        Self {
            has_luid: 0,
            adapter_luid: [0u8; 8],
            has_uuid: 0,
            device_uuid: [0u8; 16],
            prefer_gpu: 1,
            transparent_bg: 1,
        }
    }
}

/// 跨语言零拷贝共享帧描述符（由宿主通过 `velox_embed_acquire_shared_frame` 获取）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct VeloxSharedFrameDesc {
    /// 表面重建代数（窗口 Resize 后递增，宿主检测到变化须重新打开共享句柄）。
    pub generation_id: u32,
    /// 三缓冲槽位索引（`0..3`）。
    pub slot_index: u8,
    /// `1` 表示本帧为新渲染完成的画面，`0` 表示复用上次最新帧。
    pub is_fresh: u8,
    /// 显存句柄类型（见 [`VeloxHandleKind`]）。
    pub handle_kind: u8,
    /// 同步原语类型（见 [`VeloxSyncKind`]）。
    pub sync_kind: u8,
    /// 物理像素宽度。
    pub width: u32,
    /// 物理像素高度。
    pub height: u32,
    /// 像素格式编码（`0 = Rgba8Unorm`，`1 = Bgra8Unorm`，均为预乘 Alpha）。
    pub pixel_format: u32,
    /// 跨 API 原始显存句柄（Win32 NT `HANDLE` / Linux `dma_buf` fd / `IOSurfaceRef` / `AHB*`）。
    pub raw_handle: u64,
    /// 跨 API GPU 同步句柄（`ID3D12Fence` NT `HANDLE` / Timeline Semaphore fd / `MTLSharedEvent`）。
    pub sync_handle: u64,
    /// 宿主采样前需等待的 GPU 栅栏单调计数值。
    pub fence_value: u64,
    /// 行步长（字节，主要用于 `LinuxDmaBufFd` 与 `SoftwareRgbaPtr`）。
    pub stride: u32,
    /// 平面字节偏移量。
    pub offset: u32,
    /// DRM FourCC（仅 Linux DMA-BUF 有效）。
    pub drm_format: u32,
    /// DRM Format Modifier（仅 Linux DMA-BUF 有效）。
    pub drm_modifier: u64,
    /// 软件光栅回退模式的 `RGBA8888` 像素缓冲指针（GPU 模式下为空指针）。
    pub software_rgba_ptr: *const u8,
    /// 脏矩形数组指针（三缓冲 GPU 模式下已自动做 3 帧并集合并）。
    pub dirty_rects_ptr: *const CDirtyRect,
    /// 脏矩形数量。
    pub dirty_rect_count: u32,
}

impl Default for VeloxSharedFrameDesc {
    fn default() -> Self {
        Self {
            generation_id: 0,
            slot_index: 0,
            is_fresh: 0,
            handle_kind: VeloxHandleKind::None as u8,
            sync_kind: VeloxSyncKind::None as u8,
            width: 0,
            height: 0,
            pixel_format: 0,
            raw_handle: 0,
            sync_handle: 0,
            fence_value: 0,
            stride: 0,
            offset: 0,
            drm_format: 0,
            drm_modifier: 0,
            software_rgba_ptr: core::ptr::null(),
            dirty_rects_ptr: core::ptr::null(),
            dirty_rect_count: 0,
        }
    }
}

/// 单个可多实例并行的宿主嵌入会话（统一承载 `VeloxEngine` + GPU 共享导出器或软件缓冲回退）。
pub struct VeloxEmbedSession {
    pub engine: VeloxEngine,
    scale_factor: f32,
    phys_width: u32,
    phys_height: u32,
    transparent_bg: bool,
    #[cfg(feature = "gpu")]
    gpu_exporter: Option<WgpuSharedExportRenderer>,
    sw_pixmap: Vec<Rgba8888>,
    sw_rgba_bytes: Vec<u8>,
    sw_generation: u32,
    sw_fresh: bool,
    dirty_rects: Vec<CDirtyRect>,
}

impl VeloxEmbedSession {
    /// 创建一个挂载指定根视图的跨框架嵌入会话。
    pub fn new<V: View>(
        root_view: V,
        logical_width: i16,
        logical_height: i16,
        scale_factor: f32,
        config: VeloxHostAdapterConfig,
    ) -> Self {
        let lw = logical_width.max(1);
        let lh = logical_height.max(1);
        let sf = scale_factor.clamp(0.5, 4.0);
        let pw = ((lw as f32) * sf).round().max(1.0) as u32;
        let ph = ((lh as f32) * sf).round().max(1.0) as u32;

        let mut engine = VeloxEngine::new(lw, lh);
        engine.set_scale_factor(sf);
        let _ = engine.mount_root(root_view);

        #[cfg(feature = "gpu")]
        let gpu_exporter = if config.prefer_gpu != 0 {
            let luid = (config.has_luid != 0).then_some(config.adapter_luid);
            let uuid = (config.has_uuid != 0).then_some(config.device_uuid);
            WgpuSharedExportRenderer::new(pw, ph, luid, uuid).map(|mut exp| {
                if config.transparent_bg != 0 {
                    exp.set_clear_color(crate::renderers::wgpu::wgpu::Color::TRANSPARENT);
                } else {
                    exp.set_clear_color(crate::renderers::wgpu::wgpu::Color::BLACK);
                }
                exp
            })
        } else {
            None
        };

        let sw_needed = {
            #[cfg(feature = "gpu")]
            {
                gpu_exporter.is_none()
            }
            #[cfg(not(feature = "gpu"))]
            {
                true
            }
        };
        let (sw_pixmap, sw_rgba_bytes) = if sw_needed {
            let n = (pw as usize) * (ph as usize);
            (vec![Rgba8888(0); n], vec![0u8; n * 4])
        } else {
            (Vec::new(), Vec::new())
        };

        Self {
            engine,
            scale_factor: sf,
            phys_width: pw,
            phys_height: ph,
            transparent_bg: config.transparent_bg != 0,
            #[cfg(feature = "gpu")]
            gpu_exporter,
            sw_pixmap,
            sw_rgba_bytes,
            sw_generation: 1,
            sw_fresh: false,
            dirty_rects: Vec::with_capacity(8),
        }
    }

    /// 返回当前会话是否激活了 GPU 硬件共享导出后端。
    #[inline]
    pub fn backend_is_gpu(&self) -> bool {
        #[cfg(feature = "gpu")]
        {
            self.gpu_exporter.is_some()
        }
        #[cfg(not(feature = "gpu"))]
        {
            false
        }
    }

    /// 返回底层 GPU 共享导出器可变引用（供 Rust 宿主直接注册外部零拷贝纹理或检查句柄）。
    #[cfg(feature = "gpu")]
    #[inline]
    pub fn gpu_exporter_mut(&mut self) -> Option<&mut WgpuSharedExportRenderer> {
        self.gpu_exporter.as_mut()
    }

    /// 调整嵌入视口的逻辑尺寸与缩放因子。
    pub fn resize(&mut self, logical_width: i16, logical_height: i16, scale_factor: f32) {
        let lw = logical_width.max(1);
        let lh = logical_height.max(1);
        let sf = scale_factor.clamp(0.5, 4.0);
        let pw = ((lw as f32) * sf).round().max(1.0) as u32;
        let ph = ((lh as f32) * sf).round().max(1.0) as u32;
        self.scale_factor = sf;
        self.engine.set_scale_factor(sf);
        self.engine.resize(lw, lh);
        if pw != self.phys_width || ph != self.phys_height {
            self.phys_width = pw;
            self.phys_height = ph;
            #[cfg(feature = "gpu")]
            if let Some(exp) = &mut self.gpu_exporter {
                exp.resize(pw, ph);
            }
            if !self.backend_is_gpu() {
                let n = (pw as usize) * (ph as usize);
                self.sw_pixmap.resize(n, Rgba8888(0));
                self.sw_rgba_bytes.resize(n * 4, 0);
                self.sw_generation = self.sw_generation.wrapping_add(1).max(1);
            }
        }
    }

    /// 推进一帧动画与布局求解，并将有变化的画面提交至 GPU 共享槽或软件回退缓冲；返回是否有新帧产出。
    pub fn step_frame(&mut self, dt_ms: u16) -> bool {
        #[cfg(feature = "gpu")]
        if let Some(exp) = &mut self.gpu_exporter {
            return exp.step_and_render(&mut self.engine, dt_ms).is_some();
        }

        let dirty_slice = self.engine.step_frame_dt(dt_ms);
        if dirty_slice.is_empty() {
            return false;
        }
        let mut dirty_copy = Vec::with_capacity(dirty_slice.len());
        self.dirty_rects.clear();
        for &r in dirty_slice {
            dirty_copy.push(r);
            self.dirty_rects.push(CDirtyRect {
                x: r.x,
                y: r.y,
                w: r.w,
                h: r.h,
            });
        }

        let vp = self.engine.viewport_region();
        let ops = self.engine.cached_draw_ops();
        let w = self.phys_width as usize;
        if self.transparent_bg {
            for px in &mut self.sw_pixmap {
                *px = Rgba8888(0);
            }
        }
        for reg in &dirty_copy {
            if !reg.is_empty() {
                rasterize_region_scaled_parallel(
                    ops,
                    vp,
                    self.scale_factor,
                    *reg,
                    0,
                    0,
                    w,
                    &mut self.sw_pixmap,
                );
            }
        }
        for (dst, px) in self
            .sw_rgba_bytes
            .chunks_exact_mut(4)
            .zip(self.sw_pixmap.iter())
        {
            dst.copy_from_slice(&px.to_rgba_bytes());
        }
        self.sw_fresh = true;
        true
    }

    /// 宿主获取最新共享帧描述（GPU 模式返回零拷贝句柄，软件模式返回 CPU RGBA 缓冲指针）。
    pub fn acquire_shared_frame(&mut self) -> VeloxSharedFrameDesc {
        #[cfg(feature = "gpu")]
        if let Some(exp) = &self.gpu_exporter {
            let lease = exp.acquire_shared_frame();
            self.dirty_rects.clear();
            for &r in lease.dirty_rects.as_slice() {
                self.dirty_rects.push(CDirtyRect {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: r.h,
                });
            }
            let (handle_kind, stride, offset, drm_format, drm_modifier) = match lease.handle {
                ExportedHandle::Win32Dx12NtHandle { .. } => {
                    (VeloxHandleKind::Win32Dx12NtHandle, lease.width * 4, 0, 0, 0)
                }
                ExportedHandle::LinuxDmaBuf {
                    stride,
                    offset,
                    drm_format,
                    modifier,
                    ..
                } => (
                    VeloxHandleKind::LinuxDmaBufFd,
                    stride,
                    offset,
                    drm_format,
                    modifier,
                ),
                ExportedHandle::AppleIoSurface { .. } => {
                    (VeloxHandleKind::AppleIoSurface, lease.width * 4, 0, 0, 0)
                }
                ExportedHandle::AndroidHardwareBuffer { .. } => (
                    VeloxHandleKind::AndroidHardwareBuffer,
                    lease.width * 4,
                    0,
                    0,
                    0,
                ),
                ExportedHandle::InternalOnly => (VeloxHandleKind::None, lease.width * 4, 0, 0, 0),
            };
            let sync_kind = match lease.sync_handle {
                ExternalSyncHandle::Win32Dx12FenceNtHandle(_) => {
                    VeloxSyncKind::Win32Dx12FenceNtHandle
                }
                ExternalSyncHandle::LinuxSemaphoreFd(_) => VeloxSyncKind::LinuxSemaphoreFd,
                ExternalSyncHandle::AppleMetalSharedEvent(_) => {
                    VeloxSyncKind::AppleMetalSharedEvent
                }
                ExternalSyncHandle::AndroidSyncFd(_) => VeloxSyncKind::AndroidSyncFd,
                ExternalSyncHandle::None => VeloxSyncKind::None,
            };
            let pixel_format = if lease.format == crate::renderers::wgpu::wgpu::TextureFormat::Bgra8Unorm {
                1
            } else {
                0
            };
            return VeloxSharedFrameDesc {
                generation_id: lease.generation_id,
                slot_index: lease.slot_index,
                is_fresh: if lease.is_fresh { 1 } else { 0 },
                handle_kind: handle_kind as u8,
                sync_kind: sync_kind as u8,
                width: lease.width,
                height: lease.height,
                pixel_format,
                raw_handle: lease.handle.raw_u64(),
                sync_handle: lease.sync_handle.raw_u64(),
                fence_value: lease.fence_value,
                stride,
                offset,
                drm_format,
                drm_modifier,
                software_rgba_ptr: core::ptr::null(),
                dirty_rects_ptr: self.dirty_rects.as_ptr(),
                dirty_rect_count: self.dirty_rects.len() as u32,
            };
        }

        let is_fresh = self.sw_fresh;
        self.sw_fresh = false;
        VeloxSharedFrameDesc {
            generation_id: self.sw_generation,
            slot_index: 0,
            is_fresh: if is_fresh { 1 } else { 0 },
            handle_kind: VeloxHandleKind::SoftwareRgbaPtr as u8,
            sync_kind: VeloxSyncKind::None as u8,
            width: self.phys_width,
            height: self.phys_height,
            pixel_format: 0,
            raw_handle: 0,
            sync_handle: 0,
            fence_value: 0,
            stride: self.phys_width * 4,
            offset: 0,
            drm_format: 0,
            drm_modifier: 0,
            software_rgba_ptr: self.sw_rgba_bytes.as_ptr(),
            dirty_rects_ptr: self.dirty_rects.as_ptr(),
            dirty_rect_count: self.dirty_rects.len() as u32,
        }
    }

    /// 宿主归还已完成采样的共享帧租约（驱动旧 Resize 显存池安全回收）。
    pub fn release_shared_frame(&mut self, generation_id: u32, slot_index: u8) {
        #[cfg(feature = "gpu")]
        if let Some(exp) = &mut self.gpu_exporter {
            exp.release_shared_frame(generation_id, slot_index);
        }
        let _ = (generation_id, slot_index);
    }

    /// 注入指针事件（`phase`: `0=Down`, `1=Move`, `2=Up`, `3=Cancel`）。
    pub fn inject_pointer(&mut self, phase: u8, x: i16, y: i16) -> bool {
        match phase {
            0 => self.engine.dispatch_pointer_down(x, y),
            1 => self.engine.dispatch_pointer_move(x, y),
            2 => self.engine.dispatch_pointer_up(x, y),
            _ => {
                self.engine.dispatch_pointer_up(x, y);
                true
            }
        }
    }

    /// 注入滚轮事件（正值向下、负值向上）。
    pub fn inject_wheel(&mut self, x: i16, y: i16, delta_y: i16) -> bool {
        self.engine
            .dispatch_event(InputEvent::Scroll { x, y, delta_y })
    }

    /// 注入键盘事件（`is_char == 1` 时按 Unicode 码点派发字符键；否则按标准键码派发导航键）。
    pub fn inject_key(&mut self, code: u32, is_char: bool) -> bool {
        if is_char {
            if let Some(ch) = char::from_u32(code) {
                return self.engine.dispatch_key(KeyCode::Char(ch));
            }
            return false;
        }
        let key = match code {
            9 => KeyCode::Tab,
            13 => KeyCode::Enter,
            27 => KeyCode::Escape,
            8 => KeyCode::Backspace,
            46 => KeyCode::Delete,
            37 => KeyCode::Left,
            38 => KeyCode::Up,
            39 => KeyCode::Right,
            40 => KeyCode::Down,
            36 => KeyCode::Home,
            35 => KeyCode::End,
            _ => return false,
        };
        self.engine.dispatch_key(key)
    }
}

// ============================================================================
// C ABI 导出符号（供 Qt6 / GTK4 / WinUI 3 / WPF / C++ / C# / Swift / Kotlin 调用）
// ============================================================================

/// 查询嵌入会话是否处于 GPU 零拷贝共享导出模式（`1 = GPU`，`0 = Software`）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_backend_is_gpu(session: *const VeloxEmbedSession) -> u8 {
    let Some(s) = (unsafe { session.as_ref() }) else {
        return 0;
    };
    if s.backend_is_gpu() { 1 } else { 0 }
}

/// 调整嵌入会话逻辑尺寸与 HiDPI 缩放比例。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_resize(
    session: *mut VeloxEmbedSession,
    logical_width: i16,
    logical_height: i16,
    scale_factor: f32,
) {
    if let Some(s) = unsafe { session.as_mut() } {
        s.resize(logical_width, logical_height, scale_factor);
    }
}

/// 推进一帧嵌入引擎并提交渲染（返回 `1` 表示有新画面产出，`0` 表示静止零重绘）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_step_frame(
    session: *mut VeloxEmbedSession,
    dt_ms: u16,
) -> u8 {
    let Some(s) = (unsafe { session.as_mut() }) else {
        return 0;
    };
    if s.step_frame(dt_ms) { 1 } else { 0 }
}

/// 宿主获取当前最新共享显存帧句柄（GPU 模式零拷贝，无 GPU 时返回 CPU RGBA 回退指针）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_acquire_shared_frame(
    session: *mut VeloxEmbedSession,
    out_frame: *mut VeloxSharedFrameDesc,
) -> u8 {
    let (Some(s), Some(out)) = (unsafe { session.as_mut() }, unsafe { out_frame.as_mut() }) else {
        return 0;
    };
    *out = s.acquire_shared_frame();
    1
}

/// 宿主归还已完成采样的共享帧租约。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_release_shared_frame(
    session: *mut VeloxEmbedSession,
    generation_id: u32,
    slot_index: u8,
) {
    if let Some(s) = unsafe { session.as_mut() } {
        s.release_shared_frame(generation_id, slot_index);
    }
}

/// 向嵌入会话注入指针输入事件。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_inject_pointer(
    session: *mut VeloxEmbedSession,
    phase: u8,
    x: i16,
    y: i16,
) -> u8 {
    let Some(s) = (unsafe { session.as_mut() }) else {
        return 0;
    };
    if s.inject_pointer(phase, x, y) { 1 } else { 0 }
}

/// 向嵌入会话注入滚轮滚动事件。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_inject_wheel(
    session: *mut VeloxEmbedSession,
    x: i16,
    y: i16,
    delta_y: i16,
) -> u8 {
    let Some(s) = (unsafe { session.as_mut() }) else {
        return 0;
    };
    if s.inject_wheel(x, y, delta_y) { 1 } else { 0 }
}

/// 向嵌入会话注入键盘按键事件。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_inject_key(
    session: *mut VeloxEmbedSession,
    code: u32,
    is_char: u8,
) -> u8 {
    let Some(s) = (unsafe { session.as_mut() }) else {
        return 0;
    };
    if s.inject_key(code, is_char != 0) { 1 } else { 0 }
}

/// 销毁由 `Box::into_raw(Box::new(VeloxEmbedSession::new(...)))` 创建的嵌入会话。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn velox_embed_destroy(session: *mut VeloxEmbedSession) {
    if !session.is_null() {
        unsafe {
            drop(Box::from_raw(session));
        }
    }
}

/// 供 Qt6 / GTK4 / WinUI 3 / WPF / C++ / Swift 宿主直接引用的标准 C 头文件模板。
pub const VELOX_EMBED_HEADER_H: &str = r#"#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct VeloxEmbedSession VeloxEmbedSession;

typedef struct VeloxDirtyRect {
    int16_t x;
    int16_t y;
    int16_t w;
    int16_t h;
} VeloxDirtyRect;

typedef enum VeloxHandleKind {
    VELOX_HANDLE_NONE = 0,
    VELOX_HANDLE_WIN32_DX12_NT = 1,
    VELOX_HANDLE_LINUX_DMABUF_FD = 2,
    VELOX_HANDLE_APPLE_IOSURFACE = 3,
    VELOX_HANDLE_ANDROID_AHB = 4,
    VELOX_HANDLE_SOFTWARE_RGBA_PTR = 5,
} VeloxHandleKind;

typedef enum VeloxSyncKind {
    VELOX_SYNC_NONE = 0,
    VELOX_SYNC_WIN32_DX12_FENCE_NT = 1,
    VELOX_SYNC_LINUX_SEMAPHORE_FD = 2,
    VELOX_SYNC_APPLE_METAL_SHARED_EVENT = 3,
    VELOX_SYNC_ANDROID_SYNC_FD = 4,
} VeloxSyncKind;

typedef struct VeloxHostAdapterConfig {
    uint8_t has_luid;
    uint8_t adapter_luid[8];
    uint8_t has_uuid;
    uint8_t device_uuid[16];
    uint8_t prefer_gpu;
    uint8_t transparent_bg;
} VeloxHostAdapterConfig;

typedef struct VeloxSharedFrameDesc {
    uint32_t generation_id;
    uint8_t slot_index;
    uint8_t is_fresh;
    uint8_t handle_kind;
    uint8_t sync_kind;
    uint32_t width;
    uint32_t height;
    uint32_t pixel_format;
    uint64_t raw_handle;
    uint64_t sync_handle;
    uint64_t fence_value;
    uint32_t stride;
    uint32_t offset;
    uint32_t drm_format;
    uint64_t drm_modifier;
    const uint8_t* software_rgba_ptr;
    const VeloxDirtyRect* dirty_rects_ptr;
    uint32_t dirty_rect_count;
} VeloxSharedFrameDesc;

uint8_t velox_embed_backend_is_gpu(const VeloxEmbedSession* session);
void    velox_embed_resize(VeloxEmbedSession* session, int16_t logical_width, int16_t logical_height, float scale_factor);
uint8_t velox_embed_step_frame(VeloxEmbedSession* session, uint16_t dt_ms);
uint8_t velox_embed_acquire_shared_frame(VeloxEmbedSession* session, VeloxSharedFrameDesc* out_frame);
void    velox_embed_release_shared_frame(VeloxEmbedSession* session, uint32_t generation_id, uint8_t slot_index);
uint8_t velox_embed_inject_pointer(VeloxEmbedSession* session, uint8_t phase, int16_t x, int16_t y);
uint8_t velox_embed_inject_wheel(VeloxEmbedSession* session, int16_t x, int16_t y, int16_t delta_y);
uint8_t velox_embed_inject_key(VeloxEmbedSession* session, uint32_t code, uint8_t is_char);
void    velox_embed_destroy(VeloxEmbedSession* session);

#ifdef __cplusplus
}
#endif
"#;
