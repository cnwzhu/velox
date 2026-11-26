//! Windows DX12 原生零拷贝显存句柄桥接（`D3D12_HEAP_FLAG_SHARED` + Win32 NT `HANDLE` + `ID3D12Fence`）。
//!
//! 直接复用 `wgpu::hal::api::Dx12` 底层 `ID3D12Device` / `ID3D12CommandQueue` / `DxgiAdapter`：
//! 1. 按宿主提供的 8 字节 `AdapterLuid` 精确匹配物理显卡；
//! 2. 创建带 `D3D12_HEAP_FLAG_SHARED` 的离屏渲染目标资源并调用 `CreateSharedHandle` 导出 Win32 NT `HANDLE`；
//! 3. 通过 `wgpu::hal::dx12::Device::texture_from_raw` + `Device::create_texture_from_hal` 零拷贝封装为 `wgpu::Texture`；
//! 4. 创建 `D3D12_FENCE_FLAG_SHARED` 跨 API 栅栏并导出 Fence NT `HANDLE`，每次提交后 `ID3D12CommandQueue::Signal`。

use ::wgpu;
use std::ffi::c_void;
use std::sync::Arc;

use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Fence, ID3D12Resource, D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_FENCE_FLAG_SHARED,
    D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT,
    D3D12_MEMORY_POOL_UNKNOWN, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_STATE_COMMON,
    D3D12_TEXTURE_LAYOUT_UNKNOWN,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_SAMPLE_DESC,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use super::super::{block_on, create_wgpu_instance, PreparedWgpuContext};
use super::{ExportedHandle, ExportedTextureAlloc, ExternalSyncHandle, PlatformTextureGuard};

/// 持有 Win32 NT `HANDLE` 的 RAII 守卫（离开作用域时安全 `CloseHandle`）。
pub(crate) struct Dx12SharedTextureGuard {
    nt_handle: HANDLE,
}

unsafe impl Send for Dx12SharedTextureGuard {}
unsafe impl Sync for Dx12SharedTextureGuard {}

impl Drop for Dx12SharedTextureGuard {
    fn drop(&mut self) {
        if !self.nt_handle.is_invalid() && !self.nt_handle.0.is_null() {
            unsafe {
                let _ = CloseHandle(self.nt_handle);
            }
        }
    }
}

/// 跨 API 共享 `ID3D12Fence`（附带导出的 Win32 NT `HANDLE` 与 CPU 等待事件句柄）。
pub(crate) struct Dx12SharedFence {
    fence: ID3D12Fence,
    nt_handle: HANDLE,
    wait_event: HANDLE,
}

unsafe impl Send for Dx12SharedFence {}
unsafe impl Sync for Dx12SharedFence {}

impl Drop for Dx12SharedFence {
    fn drop(&mut self) {
        unsafe {
            if !self.wait_event.is_invalid() && !self.wait_event.0.is_null() {
                let _ = CloseHandle(self.wait_event);
            }
            if !self.nt_handle.is_invalid() && !self.nt_handle.0.is_null() {
                let _ = CloseHandle(self.nt_handle);
            }
        }
    }
}

impl Dx12SharedFence {
    pub(crate) fn new(device: &wgpu::Device) -> Option<Self> {
        let hal_dev = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }?;
        let raw_dev = hal_dev.raw_device();
        let fence: ID3D12Fence = unsafe { raw_dev.CreateFence(0, D3D12_FENCE_FLAG_SHARED).ok()? };
        let nt_handle = unsafe {
            raw_dev
                .CreateSharedHandle(&fence, None, GENERIC_ALL.0, None)
                .ok()?
        };
        let wait_event = unsafe { CreateEventW(None, false, false, None).ok()? };
        Some(Self {
            fence,
            nt_handle,
            wait_event,
        })
    }

    #[inline]
    pub(crate) fn export_handle(&self) -> ExternalSyncHandle {
        ExternalSyncHandle::Win32Dx12FenceNtHandle(self.nt_handle.0 as usize as u64)
    }

    pub(crate) fn signal_on_device(&self, device: &wgpu::Device, value: u64) {
        if let Some(hal_dev) = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() } {
            let raw_queue = hal_dev.raw_queue();
            unsafe {
                let _ = raw_queue.Signal(&self.fence, value);
            }
        }
    }

    #[inline]
    pub(crate) fn completed_value(&self) -> u64 {
        unsafe { self.fence.GetCompletedValue() }
    }

    pub(crate) fn wait_cpu(&self, value: u64, timeout_ms: u32) -> bool {
        if self.completed_value() >= value {
            return true;
        }
        unsafe {
            if self
                .fence
                .SetEventOnCompletion(value, self.wait_event)
                .is_err()
            {
                return false;
            }
            WaitForSingleObject(self.wait_event, timeout_ms) == WAIT_OBJECT_0
        }
    }
}

#[inline]
fn to_dxgi_format(fmt: wgpu::TextureFormat) -> Option<DXGI_FORMAT> {
    match fmt {
        wgpu::TextureFormat::Bgra8Unorm => Some(DXGI_FORMAT_B8G8R8A8_UNORM),
        wgpu::TextureFormat::Bgra8UnormSrgb => Some(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB),
        wgpu::TextureFormat::Rgba8Unorm => Some(DXGI_FORMAT_R8G8B8A8_UNORM),
        wgpu::TextureFormat::Rgba8UnormSrgb => Some(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB),
        _ => None,
    }
}

fn read_adapter_luid(adapter: &wgpu::Adapter) -> Option<[u8; 8]> {
    let hal_adapter = unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }?;
    let desc = unsafe { hal_adapter.raw_adapter().GetDesc2().ok()? };
    let mut bytes = [0u8; 8];
    bytes[0..4].copy_from_slice(&desc.AdapterLuid.LowPart.to_le_bytes());
    bytes[4..8].copy_from_slice(&desc.AdapterLuid.HighPart.to_le_bytes());
    Some(bytes)
}

/// 按可选 8 字节 Windows DXGI `LUID` 精确对齐物理显卡并创建 `PreparedWgpuContext`。
pub(crate) fn init_aligned_dx12_context(
    target_luid: Option<[u8; 8]>,
) -> Option<PreparedWgpuContext> {
    let instance = create_wgpu_instance(wgpu::Backends::DX12);
    let adapters = block_on(instance.enumerate_adapters(wgpu::Backends::DX12));

    let mut chosen: Option<(wgpu::Adapter, [u8; 8])> = None;
    for adapter in adapters {
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            continue;
        }
        let luid = read_adapter_luid(&adapter).unwrap_or([0u8; 8]);
        if let Some(want) = target_luid {
            if luid == want {
                chosen = Some((adapter, luid));
                break;
            }
        } else if chosen.is_none() {
            chosen = Some((adapter, luid));
        }
    }

    let (adapter, luid) = chosen?;
    let info = adapter.get_info();
    let required_limits = wgpu::Limits {
        max_inter_stage_shader_variables: 15,
        ..wgpu::Limits::default()
    };
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("velox_dx12_interop_device"),
        required_features: wgpu::Features::empty(),
        required_limits,
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
    }))
    .ok()?;

    device.on_uncaptured_error(Arc::new(|err| {
        std::eprintln!("[velox::gpu::dx12_interop] WGPU uncaptured error: {err:?}");
    }));

    Some(PreparedWgpuContext::from_interop_parts(
        instance,
        adapter,
        device,
        queue,
        wgpu::TextureFormat::Bgra8Unorm,
        info.name,
        "DX12",
        Some(luid),
        None,
    ))
}

/// 创建带 `D3D12_HEAP_FLAG_SHARED` 的原生 DX12 纹理，导出 NT `HANDLE`，并零拷贝封装为 `wgpu::Texture`。
pub(crate) fn allocate_dx12_shared_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    adapter_luid: [u8; 8],
) -> Option<ExportedTextureAlloc> {
    let dxgi_fmt = to_dxgi_format(format)?;
    let hal_dev = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }?;
    let raw_dev = hal_dev.raw_device();

    let heap_props = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
    };
    let res_desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: width as u64,
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: dxgi_fmt,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    };

    let mut resource: Option<ID3D12Resource> = None;
    unsafe {
        raw_dev
            .CreateCommittedResource(
                &heap_props,
                D3D12_HEAP_FLAG_SHARED,
                &res_desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut resource,
            )
            .ok()?;
    }
    let resource = resource?;

    let nt_handle = unsafe {
        raw_dev
            .CreateSharedHandle(&resource, None, GENERIC_ALL.0, None)
            .ok()?
    };

    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let hal_tex = unsafe {
        wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            format,
            wgpu::TextureDimension::D2,
            extent,
            1,
            1,
        )
    };
    drop(hal_dev);

    let desc = wgpu::TextureDescriptor {
        label: Some("velox_dx12_shared_export_slot"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    let texture = unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
            hal_tex,
            &desc,
            wgpu::TextureUses::COPY_DST,
        )
    };
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    Some(ExportedTextureAlloc {
        texture,
        view,
        handle: ExportedHandle::Win32Dx12NtHandle {
            nt_handle: nt_handle.0 as usize as u64,
            dxgi_format: dxgi_fmt.0 as u32,
            adapter_luid,
        },
        _guard: PlatformTextureGuard::Dx12(Dx12SharedTextureGuard { nt_handle }),
    })
}

/// 验证导出的 DX12 共享资源 NT `HANDLE` 与栅栏 NT `HANDLE` 均可通过 `ID3D12Device::OpenSharedHandle` 成功打开，
/// 返回 `(opened_width, opened_height, fence_completed_value)`。
pub(crate) fn verify_dx12_exported_handles(
    device: &wgpu::Device,
    tex_nt_handle: u64,
    fence_nt_handle: u64,
) -> Option<(u64, u32, u64)> {
    if tex_nt_handle == 0 || fence_nt_handle == 0 {
        return None;
    }
    let hal_dev = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }?;
    let raw_dev = hal_dev.raw_device();

    let mut opened_res: Option<ID3D12Resource> = None;
    let mut opened_fence: Option<ID3D12Fence> = None;
    unsafe {
        raw_dev
            .OpenSharedHandle(HANDLE(tex_nt_handle as usize as *mut c_void), &mut opened_res)
            .ok()?;
        raw_dev
            .OpenSharedHandle(
                HANDLE(fence_nt_handle as usize as *mut c_void),
                &mut opened_fence,
            )
            .ok()?;
    }
    let res = opened_res?;
    let fence = opened_fence?;
    let desc = unsafe { res.GetDesc() };
    let val = unsafe { fence.GetCompletedValue() };
    Some((desc.Width, desc.Height, val))
}
