//! Wayland `zwp_linux_dmabuf_v1` 零拷贝显存提交驱动（并行于 `wl_shm` 共享内存驱动）。
//!
//! 将 `WgpuSharedExportRenderer`（Vulkan `VK_EXT_external_memory_dma_buf`）导出的 `dma_buf` 文件描述符
//! 直接编码为标准 Wayland wire 协议消息（`create_params` -> `add(fd, plane, offset, stride, modifier)` ->
//! `create_immed` -> `wl_surface::attach` -> `damage_buffer` -> `commit`），实现 Linux 原生桌面纯血 GPU 零拷贝上屏。

use std::vec::Vec;

use crate::solver::Region;

/// Wayland `zwp_linux_dmabuf_v1` / `zwp_linux_buffer_params_v1` / `wl_surface` 协议操作码常量。
pub const ZWP_LINUX_DMABUF_V1_CREATE_PARAMS: u16 = 1;
pub const ZWP_LINUX_BUFFER_PARAMS_V1_DESTROY: u16 = 0;
pub const ZWP_LINUX_BUFFER_PARAMS_V1_ADD: u16 = 1;
pub const ZWP_LINUX_BUFFER_PARAMS_V1_CREATE_IMMED: u16 = 3;
pub const WL_SURFACE_ATTACH: u16 = 1;
pub const WL_SURFACE_COMMIT: u16 = 6;
pub const WL_SURFACE_DAMAGE_BUFFER: u16 = 9;

/// 单个 `dma_buf` 平面描述（与 `ExportedHandle::LinuxDmaBuf` 1:1 对齐）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DmaBufPlaneDesc {
    pub fd: i32,
    pub plane_idx: u32,
    pub width: u32,
    pub height: u32,
    pub offset: u32,
    pub stride: u32,
    pub drm_format: u32,
    pub modifier: u64,
}

/// 编码后可直接经 `sendmsg` + `SCM_RIGHTS` 发送给 Wayland 合成器的零拷贝帧提交包。
#[derive(Clone, Debug, Default)]
pub struct WaylandDmaBufPacket {
    /// 符合 Wayland 二进制 wire 格式的请求字节流。
    pub wire_bytes: Vec<u8>,
    /// 随消息附带的文件描述符列表（通过 UNIX 域套接字 `SCM_RIGHTS` 传递）。
    pub attached_fds: Vec<i32>,
    /// 本次分配的新 `wl_buffer` 对象 ID。
    pub wl_buffer_id: u32,
}

/// Wayland `zwp_linux_dmabuf_v1` 零拷贝提交状态机。
pub struct WaylandDmaBufDriver {
    dmabuf_global_id: u32,
    wl_surface_id: u32,
    next_object_id: u32,
    submitted_frames: u64,
}

impl WaylandDmaBufDriver {
    /// 基于已绑定的 `zwp_linux_dmabuf_v1` 全局对象 ID 与 `wl_surface` ID 初始化驱动。
    pub const fn new(dmabuf_global_id: u32, wl_surface_id: u32, first_free_object_id: u32) -> Self {
        Self {
            dmabuf_global_id,
            wl_surface_id,
            next_object_id: first_free_object_id,
            submitted_frames: 0,
        }
    }

    #[inline]
    fn alloc_id(&mut self) -> u32 {
        let id = self.next_object_id;
        self.next_object_id = self.next_object_id.wrapping_add(1).max(2);
        id
    }

    #[inline]
    fn push_header(buf: &mut Vec<u8>, obj_id: u32, opcode: u16, payload_bytes: usize) {
        let total = (8 + payload_bytes) as u32;
        buf.extend_from_slice(&obj_id.to_ne_bytes());
        buf.extend_from_slice(&((total << 16) | (opcode as u32)).to_ne_bytes());
    }

    /// 编码一帧完整的 `dma_buf` 零拷贝上屏请求序列（`create_params` + `add` + `create_immed` + `attach` + `damage_buffer` + `commit`）。
    pub fn encode_present_packet(
        &mut self,
        plane: DmaBufPlaneDesc,
        dirty_rects: &[Region],
    ) -> WaylandDmaBufPacket {
        let params_id = self.alloc_id();
        let buffer_id = self.alloc_id();
        let mut wire = Vec::with_capacity(128);

        // 1. zwp_linux_dmabuf_v1::create_params(new_id params_id)
        Self::push_header(&mut wire, self.dmabuf_global_id, ZWP_LINUX_DMABUF_V1_CREATE_PARAMS, 4);
        wire.extend_from_slice(&params_id.to_ne_bytes());

        // 2. zwp_linux_buffer_params_v1::add(fd [via SCM_RIGHTS], plane_idx, offset, stride, mod_hi, mod_lo)
        Self::push_header(&mut wire, params_id, ZWP_LINUX_BUFFER_PARAMS_V1_ADD, 20);
        wire.extend_from_slice(&plane.plane_idx.to_ne_bytes());
        wire.extend_from_slice(&plane.offset.to_ne_bytes());
        wire.extend_from_slice(&plane.stride.to_ne_bytes());
        wire.extend_from_slice(&((plane.modifier >> 32) as u32).to_ne_bytes());
        wire.extend_from_slice(&(plane.modifier as u32).to_ne_bytes());

        // 3. zwp_linux_buffer_params_v1::create_immed(new_id buffer_id, width, height, format, flags=0)
        Self::push_header(&mut wire, params_id, ZWP_LINUX_BUFFER_PARAMS_V1_CREATE_IMMED, 20);
        wire.extend_from_slice(&buffer_id.to_ne_bytes());
        wire.extend_from_slice(&(plane.width as i32).to_ne_bytes());
        wire.extend_from_slice(&(plane.height as i32).to_ne_bytes());
        wire.extend_from_slice(&plane.drm_format.to_ne_bytes());
        wire.extend_from_slice(&0u32.to_ne_bytes());

        // 4. zwp_linux_buffer_params_v1::destroy()
        Self::push_header(&mut wire, params_id, ZWP_LINUX_BUFFER_PARAMS_V1_DESTROY, 0);

        // 5. wl_surface::attach(buffer_id, x=0, y=0)
        Self::push_header(&mut wire, self.wl_surface_id, WL_SURFACE_ATTACH, 12);
        wire.extend_from_slice(&buffer_id.to_ne_bytes());
        wire.extend_from_slice(&0i32.to_ne_bytes());
        wire.extend_from_slice(&0i32.to_ne_bytes());

        // 6. wl_surface::damage_buffer(...)
        if dirty_rects.is_empty() {
            Self::push_header(&mut wire, self.wl_surface_id, WL_SURFACE_DAMAGE_BUFFER, 16);
            wire.extend_from_slice(&0i32.to_ne_bytes());
            wire.extend_from_slice(&0i32.to_ne_bytes());
            wire.extend_from_slice(&(plane.width as i32).to_ne_bytes());
            wire.extend_from_slice(&(plane.height as i32).to_ne_bytes());
        } else {
            for &r in dirty_rects {
                if r.is_empty() {
                    continue;
                }
                Self::push_header(&mut wire, self.wl_surface_id, WL_SURFACE_DAMAGE_BUFFER, 16);
                wire.extend_from_slice(&(r.x as i32).to_ne_bytes());
                wire.extend_from_slice(&(r.y as i32).to_ne_bytes());
                wire.extend_from_slice(&(r.w as i32).to_ne_bytes());
                wire.extend_from_slice(&(r.h as i32).to_ne_bytes());
            }
        }

        // 7. wl_surface::commit()
        Self::push_header(&mut wire, self.wl_surface_id, WL_SURFACE_COMMIT, 0);

        self.submitted_frames = self.submitted_frames.saturating_add(1);
        WaylandDmaBufPacket {
            wire_bytes: wire,
            attached_fds: std::vec![plane.fd],
            wl_buffer_id: buffer_id,
        }
    }

    /// 返回累计编码提交的零拷贝帧数。
    #[inline]
    pub const fn submitted_frames(&self) -> u64 {
        self.submitted_frames
    }
}
