//! 纯 Wayland 客户端平台后端 `velox_core::platform::wayland`（不依赖 `winit` 或任何 X11 库）。
//!
//! 模块职责划分：
//! - `wire`：Wayland 二进制协议对象、事件与 UNIX Socket 线协议客户端状态机；
//! - `mod`：[`WaylandPlatform`] 双缓冲驱动与 [`Platform`] / [`PlatformSurface`] 实现。

#[cfg(unix)]
mod unix_socket;
mod wire;

use std::string::String;
use std::time::{Duration, Instant};
use std::vec::Vec;

pub use super::shm_driver::{
    decode_linux_keycode, decode_linux_modifier, write_uncompressed_png, ShmSlot, ShmSurfaceDriver,
    SoftwareSurfaceDriver, WaylandSurfaceDriver, WlBuffer, WlSurface,
};
pub use wire::{
    SurfaceId, WaylandSurfaceRole, WaylandWireEvent, WaylandWireState, WlClientEvent,
};
use crate::prelude::*;

/// 纯 Wayland 客户端平台（实现 [`Platform`] 与 [`PlatformSurface`]，不依赖 `winit`）。
///
/// 支持两种工作模式：
/// - **内存缓冲/自定义传输模式（默认）**：`WaylandPlatform::new` 仅初始化 `wl_shm` 双缓冲与 `WaylandWireState`，不会隐式连接宿主机的 `$WAYLAND_DISPLAY`；
/// - **外部 Wayland 合成器 UNIX 套接字直连模式**：通过 [`WaylandPlatform::new_connected`]、[`WaylandPlatform::connect_to_compositor`] 或设置环境变量 `VELOX_WAYLAND_AUTO_CONNECT=1` 显式连接真实 Wayland 合成器。
pub struct WaylandPlatform {
    pub role: WaylandSurfaceRole,
    width: usize,
    height: usize,
    start_instant: Instant,
    pub shm_driver: WaylandSurfaceDriver,
    pub wl_surface: WlSurface,
    pub wire_state: WaylandWireState,
    pub pending_events: Vec<WaylandWireEvent>,
    pub close_requested: bool,
    #[cfg(unix)]
    compositor_conn: Option<unix_socket::WaylandCompositorConnection>,
}

impl WaylandPlatform {
    /// 创建指定 `WaylandSurfaceRole` 与初始尺寸的纯 Wayland 客户端平台。
    ///
    /// 默认工作于无阻塞内存缓冲模式；若设置了 `VELOX_WAYLAND_AUTO_CONNECT=1` 或调用 [`Self::connect_to_compositor`]，则连接 `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`。
    pub fn new(role: WaylandSurfaceRole, width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        #[allow(unused_mut)]
        let mut wire_state = WaylandWireState::new();
        #[cfg(unix)]
        let compositor_conn = if std::env::var("VELOX_WAYLAND_AUTO_CONNECT").as_deref() == Ok("1") {
            unix_socket::WaylandCompositorConnection::try_connect(
                &role,
                w,
                h,
                &mut wire_state,
            )
        } else {
            None
        };
        Self {
            role,
            width: w,
            height: h,
            start_instant: Instant::now(),
            shm_driver: WaylandSurfaceDriver::new(w, h),
            wl_surface: WlSurface::new(),
            wire_state,
            pending_events: Vec::new(),
            close_requested: false,
            #[cfg(unix)]
            compositor_conn,
        }
    }

    /// 创建并显式尝试连接系统 `$WAYLAND_DISPLAY` 合成器套接字的客户端平台。
    pub fn new_connected(role: WaylandSurfaceRole, width: usize, height: usize) -> Self {
        let mut plat = Self::new(role, width, height);
        let _ = plat.connect_to_compositor();
        plat
    }

    /// 显式尝试连接 `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` 合成器 UNIX 套接字。
    pub fn connect_to_compositor(&mut self) -> bool {
        #[cfg(unix)]
        {
            if self.compositor_conn.is_none() {
                self.compositor_conn = unix_socket::WaylandCompositorConnection::try_connect(
                    &self.role,
                    self.width,
                    self.height,
                    &mut self.wire_state,
                );
            }
            self.compositor_conn.is_some()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// 是否已连接至真实的系统 Wayland 合成器 UNIX 套接字（`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`）。
    #[inline]
    pub fn is_connected_to_compositor(&self) -> bool {
        #[cfg(unix)]
        {
            self.compositor_conn.is_some()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// 创建 `zwlr_layer_shell_v1` Wayland 客户端平台。
    pub fn new_layer_shell(
        namespace: impl Into<String>,
        width: usize,
        height: usize,
        exclusive_zone: i16,
    ) -> Self {
        Self::new(
            WaylandSurfaceRole::LayerShell {
                namespace: namespace.into(),
                exclusive_zone,
            },
            width,
            height,
        )
    }

    /// 创建 `zxdg_toplevel_decoration_v1` / `wl_subsurface` Wayland 客户端平台。
    pub fn new_subsurface_decoration(parent_surface_id: u32, width: usize, height: usize) -> Self {
        Self::new(
            WaylandSurfaceRole::SubSurfaceDecoration { parent_surface_id },
            width,
            height,
        )
    }

    /// 创建标准 `xdg_toplevel` 应用窗口的 Wayland 客户端平台。
    pub fn new_xdg_toplevel(
        title: impl Into<String>,
        app_id: impl Into<String>,
        width: usize,
        height: usize,
    ) -> Self {
        Self::new(
            WaylandSurfaceRole::XdgToplevel {
                title: title.into(),
                app_id: app_id.into(),
            },
            width,
            height,
        )
    }

    /// 向客户端注入原始 Wayland Wire 协议字节流并处理事件（可用于合成器直连或协议单元测试）。
    pub fn feed_wire_bytes(&mut self, engine: &mut VeloxEngine, bytes: &[u8]) {
        let evs = self.wire_state.feed_incoming_bytes(bytes);
        self.process_wire_events(engine, evs);
    }

    fn process_wire_events(&mut self, engine: &mut VeloxEngine, events: Vec<WaylandWireEvent>) {
        for ev in events {
            match &ev {
                WaylandWireEvent::Configure { width, height, .. } => {
                    if *width > 0 && *height > 0 && (*width != self.width || *height != self.height)
                    {
                        self.handle_configure(engine, *width as i16, *height as i16);
                    }
                }
                WaylandWireEvent::BufferReleased { buffer_id } => {
                    self.shm_driver.release_buffer(*buffer_id);
                }
                WaylandWireEvent::SeatInput(input_ev) => {
                    if matches!(input_ev, InputEvent::PointerDown { .. }) {
                        engine.set_pointer_kind(PointerKind::Mouse);
                    }
                    engine.dispatch_event(*input_ev);
                }
                WaylandWireEvent::CloseRequested => {
                    self.close_requested = true;
                }
                _ => {}
            }
            self.pending_events.push(ev);
        }
    }

    /// 响应合成器下发的 `configure(width, height)` 事件，调整 `VeloxEngine` 视口与 `wl_shm` 缓冲池。
    pub fn handle_configure(&mut self, engine: &mut VeloxEngine, width: i16, height: i16) {
        let w = width.max(16) as usize;
        let h = height.max(16) as usize;
        self.width = w;
        self.height = h;
        engine.resize(w as i16, h as i16);
        self.shm_driver.resize(w, h);
        self.wl_surface = WlSurface::new();
        #[cfg(unix)]
        if let Some(conn) = &mut self.compositor_conn {
            conn.resize_pool(w, h, &mut self.wire_state);
        } else {
            self.wire_state.encode_destroy_double_buffers();
            self.wire_state.encode_create_double_buffers(w, h);
        }
        #[cfg(not(unix))]
        {
            self.wire_state.encode_destroy_double_buffers();
            self.wire_state.encode_create_double_buffers(w, h);
        }
    }

    /// 获取当前前台已提交的 `wl_shm` 像素切片。
    #[inline]
    pub fn front_pixels(&self) -> &[Xrgb8888] {
        self.shm_driver.front_pixels()
    }

    /// 配置桌面渲染后端类型（`Wgpu` 或 `Software`）。
    #[inline]
    pub fn with_backend(mut self, backend: super::DesktopRenderBackend) -> Self {
        self.shm_driver.set_backend(backend);
        self
    }

    /// 动态切换桌面渲染后端类型。
    #[inline]
    pub fn set_backend(&mut self, backend: super::DesktopRenderBackend) {
        self.shm_driver.set_backend(backend);
    }

    /// 返回当前激活的桌面渲染后端类型。
    #[inline]
    pub const fn backend(&self) -> super::DesktopRenderBackend {
        self.shm_driver.backend()
    }

    /// 获取最近一次 `wl_surface.commit` 提交的局部脏矩形列表。
    #[inline]
    pub fn committed_damage(&self) -> &[Region] {
        &self.wl_surface.committed_damage
    }
}

impl PlatformSurface for WaylandPlatform {
    #[inline(always)]
    fn surface_size(&self) -> (i16, i16) {
        (self.width as i16, self.height as i16)
    }

    fn flush_frame_dt(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> FlushReport {
        if self.is_connected_to_compositor() && self.shm_driver.slots.iter().all(|s| s.in_use) {
            return FlushReport::default();
        }
        let prev_buf = self.wl_surface.attached_buffer_id;
        if self
            .shm_driver
            .flush_frame_dt(engine, &mut self.wl_surface, dt_ms)
        {
            let active_slot = self.shm_driver.active_slot_idx;
            let dmg = self.wl_surface.committed_damage.clone();

            #[cfg(unix)]
            if let Some(conn) = &mut self.compositor_conn {
                conn.commit_slot_damage(
                    active_slot,
                    &self.shm_driver.slots[active_slot].pixels,
                    &self.shm_driver.last_raster_regions,
                    &dmg,
                    &mut self.wire_state,
                );
            } else {
                if let Some(old_id) = prev_buf {
                    self.shm_driver.release_buffer(old_id);
                }
                if self.wire_state.outbound_tx.len() > 4096 {
                    self.wire_state.outbound_tx.clear();
                }
                self.wire_state
                    .encode_surface_commit_damage(active_slot, &dmg);
            }

            #[cfg(not(unix))]
            {
                if let Some(old_id) = prev_buf {
                    self.shm_driver.release_buffer(old_id);
                }
                if self.wire_state.outbound_tx.len() > 4096 {
                    self.wire_state.outbound_tx.clear();
                }
                self.wire_state
                    .encode_surface_commit_damage(active_slot, &dmg);
            }

            let px: usize = dmg
                .iter()
                .map(|r| (r.w.max(0) as usize) * (r.h.max(0) as usize))
                .sum();
            FlushReport {
                dirty_regions: dmg.len(),
                strips_or_buffers: 1,
                pixels_updated: px,
            }
        } else {
            FlushReport::default()
        }
    }
}

impl Platform for WaylandPlatform {
    #[inline]
    fn duration_since_start(&self) -> Duration {
        self.start_instant.elapsed()
    }

    fn step_platform(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> PlatformControlFlow {
        #[cfg(unix)]
        if let Some(conn) = &mut self.compositor_conn {
            let evs = conn.poll_wire(&mut self.wire_state);
            self.process_wire_events(engine, evs);
        }
        if self.close_requested {
            return PlatformControlFlow::Exit;
        }
        let report = self.flush_frame_dt(engine, dt_ms);
        if report.has_updates() || engine.needs_redraw() {
            PlatformControlFlow::Poll
        } else {
            PlatformControlFlow::Wait
        }
    }
}
