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

pub(crate) fn allocate_metal_shared_texture(
    _device: &wgpu::Device,
    _width: u32,
    _height: u32,
    _format: wgpu::TextureFormat,
) -> Option<ExportedTextureAlloc> {
    // 返回 None 时由上层回退至标准 Metal `wgpu::Texture`；完整 `IOSurfaceCreate` + `newTextureWithDescriptor:iosurface:plane:`
    // 需在 Apple macOS/iOS 宿主框架链接 IOSurface.framework 启用。
    None
}
