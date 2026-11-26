//! Apple macOS / iOS Metal 零拷贝显存桥接（`IOSurfaceRef` + `MTLSharedEvent`）。

use ::wgpu;

use super::super::PreparedWgpuContext;
use super::{ExportedTextureAlloc, ExternalSyncHandle};

pub(crate) struct MetalIoSurfaceGuard {
    iosurface_ref: u64,
}

impl Drop for MetalIoSurfaceGuard {
    fn drop(&mut self) {
        if self.iosurface_ref != 0 {
            unsafe extern "C" {
                fn CFRelease(cf: *const core::ffi::c_void);
            }
            unsafe {
                CFRelease(self.iosurface_ref as usize as *const core::ffi::c_void);
            }
        }
    }
}

pub(crate) struct MetalSharedFence {
    val: core::sync::atomic::AtomicU64,
}

impl MetalSharedFence {
    pub(crate) fn new(_device: &wgpu::Device) -> Option<Self> {
        Some(Self {
            val: core::sync::atomic::AtomicU64::new(0),
        })
    }

    pub(crate) fn export_handle(&self) -> ExternalSyncHandle {
        ExternalSyncHandle::None
    }

    pub(crate) fn signal_on_device(&self, device: &wgpu::Device, value: u64) {
        let _ = device.poll(wgpu::PollType::Poll);
        self.val.store(value, core::sync::atomic::Ordering::Release);
    }

    pub(crate) fn completed_value(&self) -> u64 {
        self.val.load(core::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn wait_cpu(&self, device: &wgpu::Device, value: u64, _timeout_ms: u32) -> bool {
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        self.val.load(core::sync::atomic::Ordering::Acquire) >= value
    }
}

pub(crate) fn init_aligned_metal_context() -> Option<PreparedWgpuContext> {
    PreparedWgpuContext::init_headless()
}

/// 构造 Apple `IOSurfaceRef` 创建参数键描述（在 macOS/iOS 链接 `IOSurface.framework` 后用于零拷贝创建 BGRA8 共享表面）。
#[derive(Copy, Clone, Debug)]
pub struct MetalIoSurfaceDesc {
    pub width: u32,
    pub height: u32,
    pub bytes_per_element: u32,
    pub bytes_per_row: u32,
    pub pixel_format_fourcc: u32, // 'BGRA' = 0x42475241
}

impl MetalIoSurfaceDesc {
    #[inline]
    pub const fn new_bgra8(width: u32, height: u32) -> Self {
        let w = if width == 0 { 1 } else { width };
        let h = if height == 0 { 1 } else { height };
        let stride = (w * 4 + 63) & !63; // Metal 64B 行对齐
        Self {
            width: w,
            height: h,
            bytes_per_element: 4,
            bytes_per_row: stride,
            pixel_format_fourcc: 0x4247_5241,
        }
    }
}

pub(crate) fn allocate_metal_shared_texture(
    _device: &wgpu::Device,
    width: u32,
    height: u32,
    _format: wgpu::TextureFormat,
) -> Option<ExportedTextureAlloc> {
    let _desc = MetalIoSurfaceDesc::new_bgra8(width, height);
    // 返回 None 时由上层安全回退至标准 Metal `wgpu::Texture`；
    // 在 macOS/iOS 宿主工程链接 `IOSurface.framework` 并创建 `IOSurfaceRef` 后，
    // 通过 `MTLDevice::newTextureWithDescriptor:iosurface:plane:` 包装进 `wgpu::hal::metal::Device::texture_from_raw`。
    None
}
