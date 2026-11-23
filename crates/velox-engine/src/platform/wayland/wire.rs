//! 纯 Wayland 二进制线协议编解码与 UNIX 套接字客户端传输状态机（`WaylandWireState`）。

use std::string::String;
use std::vec::Vec;

use super::super::shm_driver::{decode_linux_keycode, decode_linux_modifier};
use crate::prelude::*;

/// Wayland 客户端表面唯一标识符（对应 `wl_surface` 对象 ID）。
pub type SurfaceId = u32;

/// 由 Wayland 合成器向客户端表面下发的底层协议事件（对应 `configure`、`wl_pointer`、`wl_keyboard`、`close`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WlClientEvent {
    /// 通知客户端调整 `wl_surface` 缓冲区尺寸（对应 `xdg_surface.configure` 或 `zwlr_layer_surface_v1.configure`）。
    XdgConfigure {
        width: i16,
        height: i16,
        maximized: bool,
    },
    /// 转发已转换为表面局部坐标 `(local_x, local_y)` 的输入事件（对应 `wl_pointer` / `wl_keyboard`）。
    SeatInput(InputEvent),
    /// 请求客户端关闭表面（对应 `xdg_toplevel.close`）。
    XdgClose,
}

/// 纯 Wayland 客户端表面角色（决定该 `wl_surface` 在合成器中的底层协议绑定类型）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WaylandSurfaceRole {
    /// `xdg_wm_base` / `xdg_toplevel` 标准应用顶层窗口。
    XdgToplevel { title: String, app_id: String },
    /// `zwlr_layer_shell_v1` / `zwlr_layer_surface_v1` 层表面。
    LayerShell {
        namespace: String,
        exclusive_zone: i16,
    },
    /// `zxdg_toplevel_decoration_v1` / `wl_subsurface` 子表面。
    SubSurfaceDecoration { parent_surface_id: u32 },
}

/// Wayland 客户端协议事件（由合成器通过 Wire 协议下发至客户端）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WaylandWireEvent {
    /// `wl_registry.global` 广播的全局接口对象。
    GlobalAdvertised {
        name: u32,
        interface: String,
        version: u32,
    },
    /// `xdg_wm_base.ping(serial)` 心跳检测（已自动回复 `pong`）。
    Ping { serial: u32 },
    /// `xdg_surface.configure` 或 `zwlr_layer_surface_v1.configure` 窗口尺寸与序列号配置。
    Configure {
        serial: u32,
        width: usize,
        height: usize,
    },
    /// `wl_buffer.release` 通知指定共享内存缓冲槽位已被合成器释放。
    BufferReleased { buffer_id: u32 },
    /// `wl_callback.done` 帧回调完成通知。
    FrameDone { timestamp_ms: u32 },
    /// `wl_pointer` / `wl_keyboard` 输入事件。
    SeatInput(InputEvent),
    /// `xdg_toplevel.close` 请求关闭窗口。
    CloseRequested,
}

/// Wayland 二进制线协议（Wire Protocol）32 位字对齐编解码器与对象状态机。
///
/// 严格遵循 Wayland Wire Format 规范：
/// - 消息头：`[object_id: u32, (size_bytes << 16) | (opcode as u32): u32]`（小端序）；
/// - 字符串：`[byte_len_with_nul: u32, utf8_bytes..., nul, zero_padding_to_4_bytes]`；
/// - 定点数（`wl_fixed_t`）：24.8 有符号定点数（`raw / 256`）。
#[derive(Clone, Debug)]
pub struct WaylandWireState {
    pub display_id: u32,
    pub registry_id: u32,
    pub sync_callback_id: u32,
    pub compositor_id: u32,
    pub shm_id: u32,
    pub xdg_wm_base_id: u32,
    pub layer_shell_id: u32,
    pub seat_id: u32,
    pub shm_pool_id: u32,
    pub buffer_ids: [u32; 2],
    pub surface_id: u32,
    pub xdg_surface_id: u32,
    pub xdg_toplevel_id: u32,
    pub layer_surface_id: u32,
    pub pointer_id: u32,
    pub keyboard_id: u32,
    pub frame_callback_id: u32,
    pub global_compositor_name: Option<(u32, u32)>,
    pub global_shm_name: Option<(u32, u32)>,
    pub global_xdg_wm_base_name: Option<(u32, u32)>,
    pub global_layer_shell_name: Option<(u32, u32)>,
    pub global_seat_name: Option<(u32, u32)>,
    pub pending_configure_size: Option<(usize, usize)>,
    pub configured: bool,
    pub pointer_x: i16,
    pub pointer_y: i16,
    pub modifiers: KeyModifiers,
    pub outbound_tx: Vec<u8>,
    rx_scratch: Vec<u8>,
}

impl Default for WaylandWireState {
    fn default() -> Self {
        Self::new()
    }
}

impl WaylandWireState {
    pub fn new() -> Self {
        Self {
            display_id: 1,
            registry_id: 2,
            sync_callback_id: 3,
            compositor_id: 4,
            shm_id: 5,
            xdg_wm_base_id: 6,
            layer_shell_id: 7,
            seat_id: 8,
            shm_pool_id: 9,
            buffer_ids: [10, 11],
            surface_id: 12,
            xdg_surface_id: 13,
            xdg_toplevel_id: 14,
            layer_surface_id: 15,
            pointer_id: 16,
            keyboard_id: 17,
            frame_callback_id: 18,
            global_compositor_name: None,
            global_shm_name: None,
            global_xdg_wm_base_name: None,
            global_layer_shell_name: None,
            global_seat_name: None,
            pending_configure_size: None,
            configured: false,
            pointer_x: 0,
            pointer_y: 0,
            modifiers: KeyModifiers::NONE,
            outbound_tx: Vec::new(),
            rx_scratch: Vec::new(),
        }
    }

    #[inline]
    pub fn write_header(buf: &mut Vec<u8>, object_id: u32, opcode: u16, size: u16) {
        buf.extend_from_slice(&object_id.to_ne_bytes());
        let word2 = ((size as u32) << 16) | (opcode as u32);
        buf.extend_from_slice(&word2.to_ne_bytes());
    }

    #[inline]
    pub fn write_u32(buf: &mut Vec<u8>, val: u32) {
        buf.extend_from_slice(&val.to_ne_bytes());
    }

    #[inline]
    pub fn write_i32(buf: &mut Vec<u8>, val: i32) {
        buf.extend_from_slice(&val.to_ne_bytes());
    }

    pub fn write_string(buf: &mut Vec<u8>, s: &str) {
        let bytes = s.as_bytes();
        let len_with_nul = (bytes.len() + 1) as u32;
        Self::write_u32(buf, len_with_nul);
        buf.extend_from_slice(bytes);
        buf.push(0);
        let pad = (4 - ((bytes.len() + 1) % 4)) % 4;
        for _ in 0..pad {
            buf.push(0);
        }
    }

    #[inline]
    pub fn padded_string_size(s: &str) -> u16 {
        let raw = s.len() + 1;
        let padded = (raw + 3) & !3;
        (4 + padded) as u16
    }

    /// 编码 `wl_display.get_registry(registry_id)` 与 `wl_display.sync(sync_callback_id)` 握手请求。
    pub fn encode_init_handshake(&mut self) {
        // wl_display.get_registry (opcode 1)
        Self::write_header(&mut self.outbound_tx, self.display_id, 1, 12);
        Self::write_u32(&mut self.outbound_tx, self.registry_id);
        // wl_display.sync (opcode 0)
        Self::write_header(&mut self.outbound_tx, self.display_id, 0, 12);
        Self::write_u32(&mut self.outbound_tx, self.sync_callback_id);
    }

    /// 编码 `wl_registry.bind(name, interface, version, new_id)`。
    pub fn encode_registry_bind(
        &mut self,
        name: u32,
        interface: &str,
        version: u32,
        new_id: u32,
    ) {
        let size = 8 + 4 + Self::padded_string_size(interface) + 4 + 4;
        Self::write_header(&mut self.outbound_tx, self.registry_id, 0, size);
        Self::write_u32(&mut self.outbound_tx, name);
        Self::write_string(&mut self.outbound_tx, interface);
        Self::write_u32(&mut self.outbound_tx, version);
        Self::write_u32(&mut self.outbound_tx, new_id);
    }

    /// 根据已收集的 `wl_registry.global` 广播绑定 `wl_compositor`、`wl_shm`、`xdg_wm_base` / `zwlr_layer_shell_v1`、`wl_seat`。
    pub fn encode_bind_discovered_globals(&mut self) {
        if let Some((name, ver)) = self.global_compositor_name {
            let id = self.compositor_id;
            self.encode_registry_bind(name, "wl_compositor", ver.min(4), id);
        }
        if let Some((name, ver)) = self.global_shm_name {
            let id = self.shm_id;
            self.encode_registry_bind(name, "wl_shm", ver.min(1), id);
        }
        if let Some((name, ver)) = self.global_xdg_wm_base_name {
            let id = self.xdg_wm_base_id;
            self.encode_registry_bind(name, "xdg_wm_base", ver.min(2), id);
        }
        if let Some((name, ver)) = self.global_layer_shell_name {
            let id = self.layer_shell_id;
            self.encode_registry_bind(name, "zwlr_layer_shell_v1", ver.min(1), id);
        }
        if let Some((name, ver)) = self.global_seat_name {
            let id = self.seat_id;
            self.encode_registry_bind(name, "wl_seat", ver.min(5), id);
        }
    }

    /// 编码 `wl_shm.create_pool(shm_pool_id, total_bytes)`（需在 `sendmsg` 时附带 `SCM_RIGHTS` 共享内存 fd）。
    pub fn encode_create_shm_pool_msg(&self, total_bytes: i32) -> Vec<u8> {
        let mut msg = Vec::with_capacity(16);
        Self::write_header(&mut msg, self.shm_id, 0, 16);
        Self::write_u32(&mut msg, self.shm_pool_id);
        Self::write_i32(&mut msg, total_bytes);
        msg
    }

    /// 编码销毁当前双缓冲 `wl_buffer` 对象（`wl_buffer.destroy`, opcode 0），供窗口尺寸重配重建缓冲前调用。
    pub fn encode_destroy_double_buffers(&mut self) {
        for &buf_id in &self.buffer_ids {
            Self::write_header(&mut self.outbound_tx, buf_id, 0, 8);
        }
    }

    /// 编码创建双缓冲 `wl_shm_pool.create_buffer`（槽位 0 与槽位 1，像素格式 `WL_SHM_FORMAT_XRGB8888 = 1`）。
    pub fn encode_create_double_buffers(&mut self, width: usize, height: usize) {
        let stride = (width * 4) as i32;
        let slot_bytes = stride * (height as i32);
        for (slot, &buf_id) in self.buffer_ids.iter().enumerate() {
            // wl_shm_pool.create_buffer (opcode 0): id, offset, width, height, stride, format(1=XRGB8888)
            Self::write_header(&mut self.outbound_tx, self.shm_pool_id, 0, 32);
            Self::write_u32(&mut self.outbound_tx, buf_id);
            Self::write_i32(&mut self.outbound_tx, (slot as i32) * slot_bytes);
            Self::write_i32(&mut self.outbound_tx, width as i32);
            Self::write_i32(&mut self.outbound_tx, height as i32);
            Self::write_i32(&mut self.outbound_tx, stride);
            Self::write_u32(&mut self.outbound_tx, 1); // WL_SHM_FORMAT_XRGB8888
        }
    }

    /// 编码创建 `wl_surface` 及对应的 `WaylandSurfaceRole`（`xdg_toplevel` 或 `zwlr_layer_surface_v1`）。
    pub fn encode_create_surface_and_role(
        &mut self,
        role: &WaylandSurfaceRole,
        width: usize,
        height: usize,
    ) {
        // wl_compositor.create_surface (opcode 0)
        Self::write_header(&mut self.outbound_tx, self.compositor_id, 0, 12);
        Self::write_u32(&mut self.outbound_tx, self.surface_id);

        match role {
            WaylandSurfaceRole::XdgToplevel { title, app_id } => {
                if self.global_xdg_wm_base_name.is_some() {
                    // xdg_wm_base.get_xdg_surface (opcode 2)
                    Self::write_header(&mut self.outbound_tx, self.xdg_wm_base_id, 2, 16);
                    Self::write_u32(&mut self.outbound_tx, self.xdg_surface_id);
                    Self::write_u32(&mut self.outbound_tx, self.surface_id);

                    // xdg_surface.get_toplevel (opcode 1)
                    Self::write_header(&mut self.outbound_tx, self.xdg_surface_id, 1, 12);
                    Self::write_u32(&mut self.outbound_tx, self.xdg_toplevel_id);

                    // xdg_toplevel.set_title (opcode 2)
                    let title_sz = 8 + Self::padded_string_size(title);
                    Self::write_header(&mut self.outbound_tx, self.xdg_toplevel_id, 2, title_sz);
                    Self::write_string(&mut self.outbound_tx, title);

                    // xdg_toplevel.set_app_id (opcode 3)
                    let app_sz = 8 + Self::padded_string_size(app_id);
                    Self::write_header(&mut self.outbound_tx, self.xdg_toplevel_id, 3, app_sz);
                    Self::write_string(&mut self.outbound_tx, app_id);
                }
            }
            WaylandSurfaceRole::LayerShell {
                namespace,
                exclusive_zone,
            } => {
                if self.global_layer_shell_name.is_some() {
                    // zwlr_layer_shell_v1.get_layer_surface (opcode 0): id, surface, output(0), layer(2=TOP), namespace
                    let ns_sz = 8 + 16 + Self::padded_string_size(namespace);
                    Self::write_header(&mut self.outbound_tx, self.layer_shell_id, 0, ns_sz);
                    Self::write_u32(&mut self.outbound_tx, self.layer_surface_id);
                    Self::write_u32(&mut self.outbound_tx, self.surface_id);
                    Self::write_u32(&mut self.outbound_tx, 0); // output = NULL
                    Self::write_u32(&mut self.outbound_tx, 2); // ZWLR_LAYER_SHELL_V1_LAYER_TOP
                    Self::write_string(&mut self.outbound_tx, namespace);

                    // zwlr_layer_surface_v1.set_size (opcode 0)
                    Self::write_header(&mut self.outbound_tx, self.layer_surface_id, 0, 16);
                    Self::write_u32(&mut self.outbound_tx, width as u32);
                    Self::write_u32(&mut self.outbound_tx, height as u32);

                    // zwlr_layer_surface_v1.set_exclusive_zone (opcode 2)
                    Self::write_header(&mut self.outbound_tx, self.layer_surface_id, 2, 12);
                    Self::write_i32(&mut self.outbound_tx, *exclusive_zone as i32);
                } else if self.global_xdg_wm_base_name.is_some() {
                    // 回退至 xdg_toplevel
                    Self::write_header(&mut self.outbound_tx, self.xdg_wm_base_id, 2, 16);
                    Self::write_u32(&mut self.outbound_tx, self.xdg_surface_id);
                    Self::write_u32(&mut self.outbound_tx, self.surface_id);
                    Self::write_header(&mut self.outbound_tx, self.xdg_surface_id, 1, 12);
                    Self::write_u32(&mut self.outbound_tx, self.xdg_toplevel_id);
                }
            }
            WaylandSurfaceRole::SubSurfaceDecoration { .. } => {}
        }

        // 初次空 commit 触发合成器下发 configure
        Self::write_header(&mut self.outbound_tx, self.surface_id, 6, 8);
    }

    /// 编码一帧 `wl_surface.attach` + `wl_surface.damage_buffer` + `wl_surface.commit` 消息序列。
    pub fn encode_surface_commit_damage(&mut self, slot_idx: usize, damage: &[Region]) {
        let buf_id = self.buffer_ids[slot_idx & 1];
        // wl_surface.attach (opcode 1): buffer_id, x=0, y=0
        Self::write_header(&mut self.outbound_tx, self.surface_id, 1, 20);
        Self::write_u32(&mut self.outbound_tx, buf_id);
        Self::write_i32(&mut self.outbound_tx, 0);
        Self::write_i32(&mut self.outbound_tx, 0);

        for reg in damage {
            // wl_surface.damage_buffer (opcode 9) / wl_surface.damage (opcode 2)
            Self::write_header(&mut self.outbound_tx, self.surface_id, 2, 24);
            Self::write_i32(&mut self.outbound_tx, reg.x as i32);
            Self::write_i32(&mut self.outbound_tx, reg.y as i32);
            Self::write_i32(&mut self.outbound_tx, reg.w.max(0) as i32);
            Self::write_i32(&mut self.outbound_tx, reg.h.max(0) as i32);
        }

        // wl_surface.commit (opcode 6)
        Self::write_header(&mut self.outbound_tx, self.surface_id, 6, 8);
    }

    /// 取出当前待发送给合成器的所有二进制协议字节流。
    pub fn take_outbound_bytes(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.outbound_tx)
    }

    /// 解析从合成器读取到的原始字节流，自动生成 `pong` / `ack_configure` 响应并提取 [`WaylandWireEvent`]。
    pub fn feed_incoming_bytes(&mut self, bytes: &[u8]) -> Vec<WaylandWireEvent> {
        self.rx_scratch.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut cursor = 0usize;

        while cursor + 8 <= self.rx_scratch.len() {
            let obj_id = u32::from_ne_bytes([
                self.rx_scratch[cursor],
                self.rx_scratch[cursor + 1],
                self.rx_scratch[cursor + 2],
                self.rx_scratch[cursor + 3],
            ]);
            let word2 = u32::from_ne_bytes([
                self.rx_scratch[cursor + 4],
                self.rx_scratch[cursor + 5],
                self.rx_scratch[cursor + 6],
                self.rx_scratch[cursor + 7],
            ]);
            let msg_size = ((word2 >> 16) as usize).max(8);
            let opcode = (word2 & 0xFFFF) as u16;

            if cursor + msg_size > self.rx_scratch.len() {
                break;
            }

            let payload = self.rx_scratch[(cursor + 8)..(cursor + msg_size)].to_vec();
            self.handle_single_message(obj_id, opcode, &payload, &mut events);
            cursor += msg_size;
        }

        if cursor > 0 {
            self.rx_scratch.drain(0..cursor);
        }
        events
    }

    fn read_u32_at(payload: &[u8], offset: usize) -> Option<u32> {
        let slice = payload.get(offset..offset + 4)?;
        Some(u32::from_ne_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_i32_at(payload: &[u8], offset: usize) -> Option<i32> {
        let slice = payload.get(offset..offset + 4)?;
        Some(i32::from_ne_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_string_at(payload: &[u8], offset: usize) -> Option<(String, usize)> {
        let byte_len = Self::read_u32_at(payload, offset)? as usize;
        if byte_len == 0 {
            return Some((String::new(), offset + 4));
        }
        let str_end = offset + 4 + byte_len.saturating_sub(1);
        let raw_bytes = payload.get((offset + 4)..str_end)?;
        let s = core::str::from_utf8(raw_bytes).ok()?;
        let padded = (byte_len + 3) & !3;
        Some((String::from(s), offset + 4 + padded))
    }

    fn handle_single_message(
        &mut self,
        obj_id: u32,
        opcode: u16,
        payload: &[u8],
        events: &mut Vec<WaylandWireEvent>,
    ) {
        if obj_id == self.registry_id && opcode == 0 {
            // wl_registry.global: name(u32), interface(string), version(u32)
            if let Some(name) = Self::read_u32_at(payload, 0) {
                if let Some((iface, next_off)) = Self::read_string_at(payload, 4) {
                    if let Some(version) = Self::read_u32_at(payload, next_off) {
                        match iface.as_str() {
                            "wl_compositor" => self.global_compositor_name = Some((name, version)),
                            "wl_shm" => self.global_shm_name = Some((name, version)),
                            "xdg_wm_base" => self.global_xdg_wm_base_name = Some((name, version)),
                            "zwlr_layer_shell_v1" => {
                                self.global_layer_shell_name = Some((name, version))
                            }
                            "wl_seat" => self.global_seat_name = Some((name, version)),
                            _ => {}
                        }
                        events.push(WaylandWireEvent::GlobalAdvertised {
                            name,
                            interface: iface,
                            version,
                        });
                    }
                }
            }
        } else if obj_id == self.xdg_wm_base_id && opcode == 0 {
            // xdg_wm_base.ping(serial) -> 自动回复 xdg_wm_base.pong(serial) (opcode 3)
            if let Some(serial) = Self::read_u32_at(payload, 0) {
                Self::write_header(&mut self.outbound_tx, self.xdg_wm_base_id, 3, 12);
                Self::write_u32(&mut self.outbound_tx, serial);
                events.push(WaylandWireEvent::Ping { serial });
            }
        } else if obj_id == self.xdg_toplevel_id && opcode == 0 {
            // xdg_toplevel.configure(width: i32, height: i32, states: array)
            if let (Some(w), Some(h)) = (Self::read_i32_at(payload, 0), Self::read_i32_at(payload, 4))
            {
                if w > 0 && h > 0 {
                    self.pending_configure_size = Some((w as usize, h as usize));
                }
            }
        } else if obj_id == self.xdg_toplevel_id && opcode == 1 {
            // xdg_toplevel.close
            events.push(WaylandWireEvent::CloseRequested);
        } else if obj_id == self.xdg_surface_id && opcode == 0 {
            // xdg_surface.configure(serial) -> 自动回复 xdg_surface.ack_configure(serial) (opcode 4)
            if let Some(serial) = Self::read_u32_at(payload, 0) {
                Self::write_header(&mut self.outbound_tx, self.xdg_surface_id, 4, 12);
                Self::write_u32(&mut self.outbound_tx, serial);
                self.configured = true;
                let (w, h) = self.pending_configure_size.take().unwrap_or((0, 0));
                events.push(WaylandWireEvent::Configure {
                    serial,
                    width: w,
                    height: h,
                });
            }
        } else if obj_id == self.layer_surface_id && opcode == 0 {
            // zwlr_layer_surface_v1.configure(serial, width, height) -> ack_configure (opcode 6)
            if let (Some(serial), Some(w), Some(h)) = (
                Self::read_u32_at(payload, 0),
                Self::read_u32_at(payload, 4),
                Self::read_u32_at(payload, 8),
            ) {
                Self::write_header(&mut self.outbound_tx, self.layer_surface_id, 6, 12);
                Self::write_u32(&mut self.outbound_tx, serial);
                self.configured = true;
                events.push(WaylandWireEvent::Configure {
                    serial,
                    width: w as usize,
                    height: h as usize,
                });
            }
        } else if (obj_id == self.buffer_ids[0] || obj_id == self.buffer_ids[1]) && opcode == 0 {
            // wl_buffer.release
            let mapped_id = if obj_id == self.buffer_ids[0] { 1 } else { 2 };
            events.push(WaylandWireEvent::BufferReleased {
                buffer_id: mapped_id,
            });
        } else if (obj_id == self.frame_callback_id || obj_id == self.sync_callback_id)
            && opcode == 0
        {
            // wl_callback.done(callback_data)
            if let Some(ts) = Self::read_u32_at(payload, 0) {
                events.push(WaylandWireEvent::FrameDone { timestamp_ms: ts });
            }
        } else if obj_id == self.seat_id && opcode == 0 {
            // wl_seat.capabilities(caps: u32): 1 = pointer, 2 = keyboard
            if let Some(caps) = Self::read_u32_at(payload, 0) {
                if (caps & 1) != 0 {
                    Self::write_header(&mut self.outbound_tx, self.seat_id, 0, 12);
                    Self::write_u32(&mut self.outbound_tx, self.pointer_id);
                }
                if (caps & 2) != 0 {
                    Self::write_header(&mut self.outbound_tx, self.seat_id, 1, 12);
                    Self::write_u32(&mut self.outbound_tx, self.keyboard_id);
                }
            }
        } else if obj_id == self.pointer_id {
            match opcode {
                0 => {
                    // wl_pointer.enter(serial, surface, sx_fixed, sy_fixed)
                    if let (Some(sx), Some(sy)) =
                        (Self::read_i32_at(payload, 8), Self::read_i32_at(payload, 12))
                    {
                        self.pointer_x = (sx >> 8) as i16;
                        self.pointer_y = (sy >> 8) as i16;
                        events.push(WaylandWireEvent::SeatInput(InputEvent::PointerMove {
                            x: self.pointer_x,
                            y: self.pointer_y,
                        }));
                    }
                }
                1 => {
                    // wl_pointer.leave(serial, surface)
                    events.push(WaylandWireEvent::SeatInput(InputEvent::PointerLeave));
                }
                2 => {
                    // wl_pointer.motion(time, sx_fixed, sy_fixed)
                    if let (Some(sx), Some(sy)) =
                        (Self::read_i32_at(payload, 4), Self::read_i32_at(payload, 8))
                    {
                        self.pointer_x = (sx >> 8) as i16;
                        self.pointer_y = (sy >> 8) as i16;
                        events.push(WaylandWireEvent::SeatInput(InputEvent::PointerMove {
                            x: self.pointer_x,
                            y: self.pointer_y,
                        }));
                    }
                }
                3 => {
                    // wl_pointer.button(serial, time, button, state)
                    let button = Self::read_u32_at(payload, 8).unwrap_or(0x110);
                    if let Some(state) = Self::read_u32_at(payload, 12) {
                        let ev = if button == 0x111 {
                            if state == 1 {
                                InputEvent::PointerSecondaryDown {
                                    x: self.pointer_x,
                                    y: self.pointer_y,
                                }
                            } else {
                                InputEvent::PointerSecondaryUp {
                                    x: self.pointer_x,
                                    y: self.pointer_y,
                                }
                            }
                        } else if state == 1 {
                            InputEvent::PointerDown {
                                x: self.pointer_x,
                                y: self.pointer_y,
                            }
                        } else {
                            InputEvent::PointerUp {
                                x: self.pointer_x,
                                y: self.pointer_y,
                            }
                        };
                        events.push(WaylandWireEvent::SeatInput(ev));
                    }
                }
                4 => {
                    // wl_pointer.axis(time, axis, value_fixed)
                    if let (Some(axis), Some(val)) =
                        (Self::read_u32_at(payload, 4), Self::read_i32_at(payload, 8))
                    {
                        let delta = (val >> 8) as i16;
                        if delta != 0 {
                            if axis == 0 {
                                events.push(WaylandWireEvent::SeatInput(InputEvent::Scroll {
                                    x: self.pointer_x,
                                    y: self.pointer_y,
                                    delta_y: delta,
                                }));
                            } else if axis == 1 {
                                events.push(WaylandWireEvent::SeatInput(InputEvent::ScrollX {
                                    x: self.pointer_x,
                                    y: self.pointer_y,
                                    delta_x: delta,
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        } else if obj_id == self.keyboard_id && opcode == 3 {
            // wl_keyboard.key(serial, time, key, state)
            if let (Some(key), Some(state)) =
                (Self::read_u32_at(payload, 8), Self::read_u32_at(payload, 12))
            {
                let code_u16 = key as u16;
                if let Some(mod_flag) = decode_linux_modifier(code_u16) {
                    let next_mods = if state != 0 {
                        self.modifiers.with(mod_flag)
                    } else {
                        self.modifiers.without(mod_flag)
                    };
                    if next_mods != self.modifiers {
                        self.modifiers = next_mods;
                        events.push(WaylandWireEvent::SeatInput(InputEvent::ModifiersChanged(
                            self.modifiers,
                        )));
                    }
                }
                if let Some(kc) = decode_linux_keycode(code_u16) {
                    if state == 1 {
                        events.push(WaylandWireEvent::SeatInput(InputEvent::KeyDown(kc)));
                    } else if state == 0 {
                        events.push(WaylandWireEvent::SeatInput(InputEvent::KeyUp(kc)));
                    }
                }
            }
        } else if obj_id == self.keyboard_id && opcode == 4 {
            // wl_keyboard.modifiers(serial, mods_depressed, mods_latched, mods_locked, group)
            if let Some(depressed) = Self::read_u32_at(payload, 4) {
                let mut mods = KeyModifiers::NONE;
                if (depressed & (1 << 0)) != 0 {
                    mods = mods.with(KeyModifiers::SHIFT);
                }
                if (depressed & (1 << 2)) != 0 {
                    mods = mods.with(KeyModifiers::CTRL);
                }
                if (depressed & (1 << 3)) != 0 {
                    mods = mods.with(KeyModifiers::ALT);
                }
                if (depressed & (1 << 6)) != 0 {
                    mods = mods.with(KeyModifiers::LOGO);
                }
                if mods != self.modifiers {
                    self.modifiers = mods;
                    events.push(WaylandWireEvent::SeatInput(InputEvent::ModifiersChanged(
                        self.modifiers,
                    )));
                }
            }
        }
    }
}

