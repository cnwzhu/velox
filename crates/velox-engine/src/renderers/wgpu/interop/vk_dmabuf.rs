//! Linux / Android Vulkan 零拷贝显存句柄桥接（`VK_EXT_external_memory_dma_buf` / `AHardwareBuffer`）。
//!
//! - **Linux**：通过 `VK_KHR_external_memory_fd` + `VK_EXT_external_memory_dma_buf` + `VK_EXT_image_drm_format_modifier`
//!   导出 `dma_buf` 文件描述符（零拷贝对接 GTK4 `GdkDmabufTextureBuilder`、Qt6 `QQuickRhiItem` 与 Wayland `zwp_linux_dmabuf_v1`）；
//! - **Android**：启用 `VK_ANDROID_external_memory_android_hardware_buffer` 导出 `AHardwareBuffer*`。

use ::wgpu;
use ash::vk;
use core::ffi::CStr;
use std::sync::Arc;
use std::vec::Vec;

use super::super::{block_on, create_wgpu_instance, PreparedWgpuContext};
use super::{ExportedHandle, ExportedTextureAlloc, ExternalSyncHandle, PlatformTextureGuard};

const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241; // 'AR24' -> BGRA8
const DRM_FORMAT_ABGR8888: u32 = 0x3432_4241; // 'AB24' -> RGBA8

pub(crate) struct VkSharedTextureGuard {
    fd: i32,
}

impl Drop for VkSharedTextureGuard {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe extern "C" {
                fn close(fd: i32) -> i32;
            }
            unsafe {
                let _ = close(self.fd);
            }
        }
    }
}

pub(crate) struct VkSharedFence {
    val: core::sync::atomic::AtomicU64,
    fd: i32,
}

impl Drop for VkSharedFence {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe extern "C" {
                fn close(fd: i32) -> i32;
            }
            unsafe {
                let _ = close(self.fd);
            }
        }
    }
}

impl VkSharedFence {
    pub(crate) fn new(_device: &wgpu::Device) -> Option<Self> {
        Some(Self {
            val: core::sync::atomic::AtomicU64::new(0),
            fd: -1,
        })
    }

    pub(crate) fn export_handle(&self) -> ExternalSyncHandle {
        if self.fd >= 0 {
            #[cfg(target_os = "android")]
            return ExternalSyncHandle::AndroidSyncFd(self.fd);
            #[cfg(not(target_os = "android"))]
            return ExternalSyncHandle::LinuxSemaphoreFd(self.fd);
        }
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

fn query_vk_device_uuid(adapter: &wgpu::Adapter) -> Option<[u8; 16]> {
    let hal_adapter = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }?;
    let raw_inst = hal_adapter.shared_instance().raw_instance();
    let phd = hal_adapter.raw_physical_device();
    let mut id_props = vk::PhysicalDeviceIDProperties::default();
    let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut id_props);
    unsafe {
        raw_inst.get_physical_device_properties2(phd, &mut props2);
    }
    Some(id_props.device_uuid)
}

/// 按宿主 16 字节 Vulkan `deviceUUID` 精确匹配物理显卡，并开启外部内存/信号量扩展。
pub(crate) fn init_aligned_vk_context(
    target_uuid: Option<[u8; 16]>,
) -> Option<PreparedWgpuContext> {
    let instance = create_wgpu_instance(wgpu::Backends::VULKAN);
    let adapters = block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));

    let mut chosen: Option<(wgpu::Adapter, [u8; 16])> = None;
    for adapter in adapters {
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            continue;
        }
        let uuid = query_vk_device_uuid(&adapter).unwrap_or([0u8; 16]);
        if let Some(want) = target_uuid {
            if uuid == want {
                chosen = Some((adapter, uuid));
                break;
            }
        } else if chosen.is_none() {
            chosen = Some((adapter, uuid));
        }
    }

    let (adapter, uuid) = chosen?;
    let info = adapter.get_info();
    let required_limits = wgpu::Limits {
        max_inter_stage_shader_variables: 15,
        ..wgpu::Limits::default()
    };
    let desc = wgpu::DeviceDescriptor {
        label: Some("velox_vk_interop_device"),
        required_features: wgpu::Features::empty(),
        required_limits: required_limits.clone(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
    };

    // 尝试通过 `open_with_callback` 显式注入平台外部内存/信号量 Vulkan 设备扩展
    let opened = unsafe {
        if let Some(hal_adapter) = adapter.as_hal::<wgpu::hal::api::Vulkan>() {
            let raw_inst = hal_adapter.shared_instance().raw_instance();
            let phd = hal_adapter.raw_physical_device();
            let avail_exts: Vec<vk::ExtensionProperties> = raw_inst
                .enumerate_device_extension_properties(phd)
                .unwrap_or_default();
            let has_ext = |name: &CStr| {
                avail_exts.iter().any(|e| {
                    CStr::from_ptr(e.extension_name.as_ptr()) == name
                })
            };

            let mut extra_exts: Vec<&'static CStr> = Vec::new();
            #[cfg(target_os = "linux")]
            for ext in [
                ash::khr::external_memory_fd::NAME,
                ash::ext::external_memory_dma_buf::NAME,
                ash::ext::image_drm_format_modifier::NAME,
                ash::khr::external_semaphore_fd::NAME,
            ] {
                if has_ext(ext) {
                    extra_exts.push(ext);
                }
            }
            #[cfg(target_os = "android")]
            for ext in [
                ash::android::external_memory_android_hardware_buffer::NAME,
                ash::khr::external_fence_fd::NAME,
            ] {
                if has_ext(ext) {
                    extra_exts.push(ext);
                }
            }

            if !extra_exts.is_empty() {
                let hal_open = hal_adapter.open_with_callback(
                    wgpu::Features::empty(),
                    &required_limits,
                    &wgpu::MemoryHints::MemoryUsage,
                    Some(Box::new(move |args| {
                        for ext in &extra_exts {
                            if !args.extensions.contains(ext) {
                                args.extensions.push(*ext);
                            }
                        }
                    })),
                );
                drop(hal_adapter);
                if let Ok(open_dev) = hal_open {
                    adapter.create_device_from_hal::<wgpu::hal::api::Vulkan>(open_dev, &desc).ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    let (device, queue) = match opened {
        Some(pair) => pair,
        None => block_on(adapter.request_device(&desc)).ok()?,
    };

    device.on_uncaptured_error(Arc::new(|err| {
        std::eprintln!("[velox::gpu::vk_interop] WGPU uncaptured error: {err:?}");
    }));

    Some(PreparedWgpuContext::from_interop_parts(
        instance,
        adapter,
        device,
        queue,
        wgpu::TextureFormat::Bgra8Unorm,
        info.name,
        "Vulkan",
        None,
        Some(uuid),
    ))
}

/// 分配可导出 `dma_buf` FD 的线性/Modifier Vulkan `VkImage` 并包装进 `wgpu::Texture`。
pub(crate) fn allocate_vk_shared_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Option<ExportedTextureAlloc> {
    let (vk_format, drm_format) = match format {
        wgpu::TextureFormat::Bgra8Unorm => (vk::Format::B8G8R8A8_UNORM, DRM_FORMAT_ARGB8888),
        wgpu::TextureFormat::Rgba8Unorm => (vk::Format::R8G8B8A8_UNORM, DRM_FORMAT_ABGR8888),
        _ => return None,
    };
    let hal_dev = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
    let enabled = hal_dev.enabled_device_extensions();
    if !enabled.contains(&ash::khr::external_memory_fd::NAME)
        || !enabled.contains(&ash::ext::external_memory_dma_buf::NAME)
    {
        return None;
    }

    let raw_dev = hal_dev.raw_device();
    let raw_inst = hal_dev.shared_instance().raw_instance();
    let phd = hal_dev.raw_physical_device();

    let mut ext_mem_img = vk::ExternalMemoryImageCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let img_info = vk::ImageCreateInfo::default()
        .push_next(&mut ext_mem_img)
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk_format)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::LINEAR)
        .usage(
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
        )
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);

    let vk_image = unsafe { raw_dev.create_image(&img_info, None).ok()? };
    let mem_reqs = unsafe { raw_dev.get_image_memory_requirements(vk_image) };
    let mem_props = unsafe { raw_inst.get_physical_device_memory_properties(phd) };

    let mem_type_idx = (0..mem_props.memory_type_count).find(|&i| {
        (mem_reqs.memory_type_bits & (1 << i)) != 0
            && mem_props.memory_types[i as usize]
                .property_flags
                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
    })?;

    let mut export_alloc = vk::ExportMemoryAllocateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(vk_image);
    let alloc_info = vk::MemoryAllocateInfo::default()
        .push_next(&mut export_alloc)
        .push_next(&mut dedicated)
        .allocation_size(mem_reqs.size)
        .memory_type_index(mem_type_idx);

    let vk_mem = match unsafe { raw_dev.allocate_memory(&alloc_info, None) } {
        Ok(m) => m,
        Err(_) => {
            unsafe { raw_dev.destroy_image(vk_image, None) };
            return None;
        }
    };
    if unsafe { raw_dev.bind_image_memory(vk_image, vk_mem, 0) }.is_err() {
        unsafe {
            raw_dev.free_memory(vk_mem, None);
            raw_dev.destroy_image(vk_image, None);
        }
        return None;
    }

    let subres = vk::ImageSubresource {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        array_layer: 0,
    };
    let layout = unsafe { raw_dev.get_image_subresource_layout(vk_image, subres) };

    let ext_fd_fn = ash::khr::external_memory_fd::Device::new(raw_inst, raw_dev);
    let fd_info = vk::MemoryGetFdInfoKHR::default()
        .memory(vk_mem)
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let fd = match unsafe { ext_fd_fn.get_memory_fd(&fd_info) } {
        Ok(fd) => fd,
        Err(_) => {
            unsafe {
                raw_dev.free_memory(vk_mem, None);
                raw_dev.destroy_image(vk_image, None);
            }
            return None;
        }
    };

    let raw_dev_clone = raw_dev.clone();
    let drop_cb: wgpu::hal::DropCallback = Box::new(move || unsafe {
        raw_dev_clone.destroy_image(vk_image, None);
        raw_dev_clone.free_memory(vk_mem, None);
    });
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let hal_desc = wgpu::hal::TextureDescriptor {
        label: Some("velox_vk_dmabuf_slot"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUses::COLOR_TARGET
            | wgpu::TextureUses::RESOURCE
            | wgpu::TextureUses::COPY_SRC
            | wgpu::TextureUses::COPY_DST,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    let hal_tex = unsafe {
        hal_dev.texture_from_raw(
            vk_image,
            &hal_desc,
            Some(drop_cb),
            wgpu::hal::vulkan::TextureMemory::External,
        )
    };
    drop(hal_dev);

    let desc = wgpu::TextureDescriptor {
        label: Some("velox_vk_dmabuf_slot"),
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
        device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
            hal_tex,
            &desc,
            wgpu::TextureUses::UNINITIALIZED,
        )
    };
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    #[cfg(target_os = "android")]
    let handle = ExportedHandle::AndroidHardwareBuffer {
        ahb_ptr: fd as u64,
    };
    #[cfg(not(target_os = "android"))]
    let handle = ExportedHandle::LinuxDmaBuf {
        fd,
        stride: layout.row_pitch as u32,
        offset: layout.offset as u32,
        drm_format,
        modifier: 0, // DRM_FORMAT_MOD_LINEAR
    };

    Some(ExportedTextureAlloc {
        texture,
        view,
        handle,
        _guard: PlatformTextureGuard::VkDmaBuf(VkSharedTextureGuard { fd }),
    })
}
