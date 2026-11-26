//! 跨 API / 跨框架零拷贝 GPU 显存纹理句柄桥接层（`velox_engine::renderers::wgpu::interop`）。
//!
//! 统一封装四大原生桌面/移动平台的显存导出句柄与 GPU 同步原语：
//! - **Windows (DX12)**：`ID3D12Resource` 共享堆 + Win32 NT `HANDLE` + 共享 `ID3D12Fence` NT `HANDLE`；
//! - **Linux (Vulkan)**：`VK_EXT_external_memory_dma_buf` 导出 `dma_buf` fd + DRM FourCC/Modifier + Timeline Semaphore FD；
//! - **macOS / iOS (Metal)**：`IOSurfaceRef` + `MTLSharedEvent`；
//! - **Android (Vulkan)**：`AHardwareBuffer*` + Sync FD。
//!
//! 当未启用 `gpu-interop` feature 或平台驱动不支持外部句柄导出时，自动回退为标准 `wgpu::Texture`（`ExportedHandle::InternalOnly`），
//! 确保所有上层 [`WgpuSharedExportRenderer`](super::WgpuSharedExportRenderer) 与单元测试在任意平台均可安全运行。

use ::wgpu;

#[cfg(all(feature = "gpu-interop", target_os = "windows"))]
pub(crate) mod dx12;
#[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
pub(crate) mod metal_iosurface;
#[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
pub(crate) mod vk_dmabuf;

/// 跨平台导出的共享显存句柄描述符（供外部 Qt6 / GTK4 / WinUI 3 / WPF / 原生合成器零拷贝采样）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExportedHandle {
    /// Windows DX12 跨进程/跨 API NT `HANDLE`（由 `ID3D12Device::CreateSharedHandle` 创建，宿主通过 `OpenSharedHandle` / `ID3D11Device1::OpenSharedResource1` 打开）。
    Win32Dx12NtHandle {
        /// Win32 `HANDLE` 指针整数表示（`0` 表示无效）。
        nt_handle: u64,
        /// DXGI 像素格式数值（如 `DXGI_FORMAT_B8G8R8A8_UNORM = 87`、`DXGI_FORMAT_R8G8B8A8_UNORM = 28`）。
        dxgi_format: u32,
        /// 物理适配器 LUID（供宿主校验同卡）。
        adapter_luid: [u8; 8],
    },
    /// Linux Vulkan / Wayland `dma_buf` 文件描述符（对接 `zwp_linux_dmabuf_v1` / `GdkDmabufTextureBuilder` / Qt RHI）。
    LinuxDmaBuf {
        fd: i32,
        stride: u32,
        offset: u32,
        /// DRM FourCC（如 `DRM_FORMAT_ARGB8888 = 0x34325241` / `DRM_FORMAT_ABGR8888 = 0x34324241`）。
        drm_format: u32,
        /// DRM Format Modifier（`0` = Linear，`0x00ff_ffff_ffff_ffff` = Invalid/Implicit）。
        modifier: u64,
    },
    /// Apple Metal `IOSurfaceRef` 指针与全局 `IOSurfaceID`（macOS / iOS 零拷贝共享显存）。
    AppleIoSurface {
        iosurface_ref: u64,
        iosurface_id: u32,
    },
    /// Android NDK `AHardwareBuffer*` 原生句柄（对接 `ASurfaceTransaction_setBuffer` / Vulkan 外部内存）。
    AndroidHardwareBuffer {
        ahb_ptr: u64,
    },
    /// 内部标准 `wgpu::Texture`（未启用 `gpu-interop` 或驱动不支持外部显存导出时的安全回退模式）。
    InternalOnly,
}

impl ExportedHandle {
    /// 判断该句柄是否为真实的跨 API / 跨框架外部共享显存句柄。
    #[inline(always)]
    pub const fn is_external_shared(&self) -> bool {
        !matches!(self, Self::InternalOnly)
    }

    /// 返回适合 C-FFI 传递的原始数值句柄（`Win32 NT HANDLE` / `dma_buf fd` / `IOSurfaceRef` / `AHB*`）。
    #[inline(always)]
    pub const fn raw_u64(&self) -> u64 {
        match *self {
            Self::Win32Dx12NtHandle { nt_handle, .. } => nt_handle,
            Self::LinuxDmaBuf { fd, .. } => fd as u64,
            Self::AppleIoSurface { iosurface_ref, .. } => iosurface_ref,
            Self::AndroidHardwareBuffer { ahb_ptr } => ahb_ptr,
            Self::InternalOnly => 0,
        }
    }
}

/// 跨平台外部 GPU 同步栅栏/信号量导出句柄（宿主在采样共享纹理前须等待该同步对象达到 `fence_value`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ExternalSyncHandle {
    /// Windows DX12 共享 `ID3D12Fence` NT `HANDLE`。
    Win32Dx12FenceNtHandle(u64),
    /// Linux Vulkan Timeline Semaphore OPAQUE_FD / SYNC_FD。
    LinuxSemaphoreFd(i32),
    /// Apple Metal `MTLSharedEvent` 指针句柄。
    AppleMetalSharedEvent(u64),
    /// Android Vulkan Sync FD。
    AndroidSyncFd(i32),
    /// 仅内部队列同步（无跨进程同步句柄）。
    #[default]
    None,
}

impl ExternalSyncHandle {
    #[inline(always)]
    pub const fn raw_u64(&self) -> u64 {
        match *self {
            Self::Win32Dx12FenceNtHandle(h) => h,
            Self::LinuxSemaphoreFd(fd) => fd as u64,
            Self::AppleMetalSharedEvent(p) => p,
            Self::AndroidSyncFd(fd) => fd as u64,
            Self::None => 0,
        }
    }
}

/// 平台原生显存资源句柄 RAII 守卫（与 `wgpu::Texture` 同步存活，销毁时自动释放 OS 句柄防泄漏）。
#[allow(dead_code)]
pub(crate) enum PlatformTextureGuard {
    None,
    #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
    Dx12(dx12::Dx12SharedTextureGuard),
    #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
    VkDmaBuf(vk_dmabuf::VkSharedTextureGuard),
    #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
    MetalIoSurface(metal_iosurface::MetalIoSurfaceGuard),
}

/// 单张已分配并包装完毕的共享导出纹理槽位资源。
pub(crate) struct ExportedTextureAlloc {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) handle: ExportedHandle,
    pub(crate) _guard: PlatformTextureGuard,
}

/// 跨平台 GPU 共享栅栏包装（渲染提交后在 GPU 队列上 `signal`，宿主侧异步等待或本机 CPU 等待）。
pub(crate) enum PlatformSharedFence {
    Fallback(core::sync::atomic::AtomicU64),
    #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
    Dx12(dx12::Dx12SharedFence),
    #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
    Vk(vk_dmabuf::VkSharedFence),
    #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
    Metal(metal_iosurface::MetalSharedFence),
}

impl PlatformSharedFence {
    /// 返回可供外部宿主导入的跨 API 同步句柄。
    pub(crate) fn export_handle(&self) -> ExternalSyncHandle {
        match self {
            Self::Fallback(_) => ExternalSyncHandle::None,
            #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
            Self::Dx12(f) => f.export_handle(),
            #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
            Self::Vk(f) => f.export_handle(),
            #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
            Self::Metal(f) => f.export_handle(),
        }
    }

    /// 在 `wgpu::Queue` 提交完渲染命令后向共享栅栏写入单调递增的 `value`。
    pub(crate) fn signal_on_queue(&self, device: &wgpu::Device, value: u64) {
        match self {
            Self::Fallback(v) => {
                let _ = device.poll(wgpu::PollType::Poll);
                v.store(value, core::sync::atomic::Ordering::Release);
            }
            #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
            Self::Dx12(f) => f.signal_on_device(device, value),
            #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
            Self::Vk(f) => f.signal_on_device(device, value),
            #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
            Self::Metal(f) => f.signal_on_device(device, value),
        }
    }

    /// 查询 GPU 当前已完成的最高栅栏值。
    pub(crate) fn completed_value(&self) -> u64 {
        match self {
            Self::Fallback(v) => v.load(core::sync::atomic::Ordering::Acquire),
            #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
            Self::Dx12(f) => f.completed_value(),
            #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
            Self::Vk(f) => f.completed_value(),
            #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
            Self::Metal(f) => f.completed_value(),
        }
    }

    /// CPU 阻塞等待直到 GPU 完成指定 `value`（或在 `timeout_ms` 超时返回 `false`）。
    #[allow(unused_variables)]
    pub(crate) fn wait_cpu(&self, device: &wgpu::Device, value: u64, timeout_ms: u32) -> bool {
        match self {
            Self::Fallback(v) => {
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                v.load(core::sync::atomic::Ordering::Acquire) >= value
            }
            #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
            Self::Dx12(f) => f.wait_cpu(value, timeout_ms),
            #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
            Self::Vk(f) => f.wait_cpu(device, value, timeout_ms),
            #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
            Self::Metal(f) => f.wait_cpu(device, value, timeout_ms),
        }
    }
}

/// 按宿主物理显卡 LUID / UUID 精确初始化对齐的 [`PreparedWgpuContext`](super::PreparedWgpuContext)。
#[cfg(feature = "gpu-interop")]
pub(crate) fn init_aligned_wgpu_context(
    adapter_luid: Option<[u8; 8]>,
    device_uuid: Option<[u8; 16]>,
) -> Option<super::PreparedWgpuContext> {
    #[cfg(target_os = "windows")]
    {
        let _ = device_uuid;
        if let Some(ctx) = dx12::init_aligned_dx12_context(adapter_luid) {
            return Some(ctx);
        }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let _ = adapter_luid;
        if let Some(ctx) = vk_dmabuf::init_aligned_vk_context(device_uuid) {
            return Some(ctx);
        }
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let _ = (adapter_luid, device_uuid);
        if let Some(ctx) = metal_iosurface::init_aligned_metal_context() {
            return Some(ctx);
        }
    }
    let _ = (adapter_luid, device_uuid);
    None
}

/// 为共享导出渲染器分配单张带平台外部句柄的 `wgpu::Texture`（不支持导出时自动回退为标准离屏 `wgpu::Texture`）。
pub(crate) fn allocate_shared_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    adapter_luid: [u8; 8],
) -> ExportedTextureAlloc {
    let w = width.max(1);
    let h = height.max(1);

    #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
    {
        if let Some(alloc) = dx12::allocate_dx12_shared_texture(device, w, h, format, adapter_luid) {
            return alloc;
        }
    }
    #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
    {
        let _ = adapter_luid;
        if let Some(alloc) = vk_dmabuf::allocate_vk_shared_texture(device, w, h, format) {
            return alloc;
        }
    }
    #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
    {
        let _ = adapter_luid;
        if let Some(alloc) = metal_iosurface::allocate_metal_shared_texture(device, w, h, format) {
            return alloc;
        }
    }

    let _ = adapter_luid;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("velox_shared_slot_fallback"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    ExportedTextureAlloc {
        texture,
        view,
        handle: ExportedHandle::InternalOnly,
        _guard: PlatformTextureGuard::None,
    }
}

/// 创建跨平台 GPU 共享同步栅栏（Windows DX12 `ID3D12Fence` / Vulkan Timeline Semaphore / Metal SharedEvent）。
pub(crate) fn create_shared_fence(device: &wgpu::Device) -> PlatformSharedFence {
    #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
    {
        if let Some(f) = dx12::Dx12SharedFence::new(device) {
            return PlatformSharedFence::Dx12(f);
        }
    }
    #[cfg(all(feature = "gpu-interop", any(target_os = "linux", target_os = "android")))]
    {
        if let Some(f) = vk_dmabuf::VkSharedFence::new(device) {
            return PlatformSharedFence::Vk(f);
        }
    }
    #[cfg(all(feature = "gpu-interop", any(target_os = "macos", target_os = "ios")))]
    {
        if let Some(f) = metal_iosurface::MetalSharedFence::new(device) {
            return PlatformSharedFence::Metal(f);
        }
    }
    let _ = device;
    PlatformSharedFence::Fallback(core::sync::atomic::AtomicU64::new(0))
}

/// Windows DX12 诊断/测试辅助：验证导出的纹理 NT `HANDLE` 与栅栏 NT `HANDLE` 可被原生 `ID3D12Device::OpenSharedHandle` 零拷贝打开。
#[cfg(all(feature = "gpu-interop", target_os = "windows"))]
pub fn verify_dx12_exported_handles(
    device: &wgpu::Device,
    tex_nt_handle: u64,
    fence_nt_handle: u64,
) -> Option<(u64, u32, u64)> {
    dx12::verify_dx12_exported_handles(device, tex_nt_handle, fence_nt_handle)
}
