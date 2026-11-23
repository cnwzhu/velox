//! Linux/Unix Wayland 合成器 UNIX 套接字连接、memfd_create 共享内存与 SCM_RIGHTS FD 传输。

use super::wire::{WaylandSurfaceRole, WaylandWireEvent, WaylandWireState};
use crate::prelude::*;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::vec::Vec;

unsafe extern "C" {
    fn memfd_create(name: *const u8, flags: u32) -> i32;
    fn ftruncate(fd: i32, length: isize) -> i32;
    fn mmap(
        addr: *mut u8,
        length: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: isize,
    ) -> *mut u8;
    fn munmap(addr: *mut u8, length: usize) -> i32;
    fn close(fd: i32) -> i32;
    fn sendmsg(sockfd: i32, msg: *const Msghdr, flags: i32) -> isize;
}

#[repr(C)]
struct Iovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
struct Msghdr {
    msg_name: *mut u8,
    msg_namelen: u32,
    msg_iov: *mut Iovec,
    #[cfg(all(target_env = "musl", target_pointer_width = "64", target_endian = "big"))]
    __pad1: i32,
    #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
    msg_iovlen: i32,
    #[cfg(all(target_env = "musl", target_pointer_width = "64", target_endian = "little"))]
    __pad1: i32,
    #[cfg(not(all(target_env = "musl", target_pointer_width = "64")))]
    msg_iovlen: usize,
    msg_control: *mut u8,
    #[cfg(all(target_env = "musl", target_pointer_width = "64", target_endian = "big"))]
    __pad2: i32,
    #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
    msg_controllen: u32,
    #[cfg(all(target_env = "musl", target_pointer_width = "64", target_endian = "little"))]
    __pad2: i32,
    #[cfg(not(all(target_env = "musl", target_pointer_width = "64")))]
    msg_controllen: usize,
    msg_flags: i32,
}

#[repr(C)]
struct Cmsghdr {
    #[cfg(all(target_env = "musl", target_pointer_width = "64", target_endian = "big"))]
    __pad1: i32,
    #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
    cmsg_len: u32,
    #[cfg(all(target_env = "musl", target_pointer_width = "64", target_endian = "little"))]
    __pad1: i32,
    #[cfg(not(all(target_env = "musl", target_pointer_width = "64")))]
    cmsg_len: usize,
    cmsg_level: i32,
    cmsg_type: i32,
}

pub struct WaylandCompositorConnection {
    stream: UnixStream,
    shm_fd: RawFd,
    shm_ptr: *mut u8,
    shm_len: usize,
    pool_width: usize,
    pool_height: usize,
}

impl Drop for WaylandCompositorConnection {
    fn drop(&mut self) {
        unsafe {
            if !self.shm_ptr.is_null() && self.shm_len > 0 {
                let _ = munmap(self.shm_ptr, self.shm_len);
            }
            if self.shm_fd >= 0 {
                let _ = close(self.shm_fd);
            }
        }
    }
}

impl WaylandCompositorConnection {
    pub fn try_connect(
        role: &WaylandSurfaceRole,
        width: usize,
        height: usize,
        wire: &mut WaylandWireState,
    ) -> Option<Self> {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let sock_path = if display.starts_with('/') {
            std::path::PathBuf::from(display)
        } else {
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
            std::path::PathBuf::from(runtime_dir).join(display)
        };

        let mut stream = UnixStream::connect(sock_path).ok()?;
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(80)));

        // 1. 发送 get_registry + sync 握手
        wire.encode_init_handshake();
        let init_bytes = wire.take_outbound_bytes();
        stream.write_all(&init_bytes).ok()?;
        let _ = stream.flush();

        // 2. 读取 wl_registry.global 广播直至收到 sync callback done
        let mut rx_buf = [0u8; 4096];
        for _ in 0..8 {
            match stream.read(&mut rx_buf) {
                Ok(n) if n > 0 => {
                    let evs = wire.feed_incoming_bytes(&rx_buf[..n]);
                    if evs
                        .iter()
                        .any(|e| matches!(e, WaylandWireEvent::FrameDone { .. }))
                    {
                        break;
                    }
                }
                _ => break,
            }
        }

        if wire.global_compositor_name.is_none() || wire.global_shm_name.is_none() {
            return None;
        }

        // 3. 绑定全局对象
        wire.encode_bind_discovered_globals();
        let bind_bytes = wire.take_outbound_bytes();
        stream.write_all(&bind_bytes).ok()?;

        // 4. 创建 memfd 共享内存池并通过 SCM_RIGHTS 发送给 wl_shm.create_pool
        let shm_len = width.max(1) * height.max(1) * 4 * 2;
        let shm_fd = unsafe { memfd_create(c"velox-wl-shm".as_ptr() as *const u8, 1) };
        if shm_fd < 0 {
            return None;
        }
        if unsafe { ftruncate(shm_fd, shm_len as isize) } < 0 {
            unsafe {
                let _ = close(shm_fd);
            }
            return None;
        }
        let shm_ptr = unsafe {
            mmap(
                core::ptr::null_mut(),
                shm_len,
                3, // PROT_READ | PROT_WRITE
                1, // MAP_SHARED
                shm_fd,
                0,
            )
        };
        if shm_ptr.is_null() || (shm_ptr as isize) == -1 {
            unsafe {
                let _ = close(shm_fd);
            }
            return None;
        }

        let mut create_pool_msg = wire.encode_create_shm_pool_msg(shm_len as i32);
        if !Self::send_msg_with_fd(stream.as_raw_fd(), &mut create_pool_msg, shm_fd) {
            unsafe {
                let _ = munmap(shm_ptr, shm_len);
                let _ = close(shm_fd);
            }
            return None;
        }

        // 5. 创建双缓冲 wl_buffer 与 wl_surface + 角色对象
        wire.encode_create_double_buffers(width, height);
        wire.encode_create_surface_and_role(role, width, height);
        let setup_bytes = wire.take_outbound_bytes();
        stream.write_all(&setup_bytes).ok()?;
        let _ = stream.flush();

        // 6. 读取初始 configure 并回复 ack_configure / pong
        if let Ok(n) = stream.read(&mut rx_buf) {
            if n > 0 {
                let _ = wire.feed_incoming_bytes(&rx_buf[..n]);
                let replies = wire.take_outbound_bytes();
                if !replies.is_empty() {
                    let _ = stream.write_all(&replies);
                    let _ = stream.flush();
                }
            }
        }

        let _ = stream.set_nonblocking(true);
        Some(Self {
            stream,
            shm_fd,
            shm_ptr,
            shm_len,
            pool_width: width,
            pool_height: height,
        })
    }

    fn send_msg_with_fd(sock_fd: RawFd, data: &mut [u8], fd_to_pass: RawFd) -> bool {
        #[repr(C)]
        struct CmsgFdBuf {
            hdr: Cmsghdr,
            fd: RawFd,
            pad: [u8; 4],
        }

        let cmsg_len_bytes = core::mem::size_of::<Cmsghdr>() + core::mem::size_of::<RawFd>();
        let cmsg_space = (cmsg_len_bytes + 7) & !7usize;
        let cmsg_controllen = cmsg_space.min(core::mem::size_of::<CmsgFdBuf>());
        let mut cmsg_buf = CmsgFdBuf {
            hdr: Cmsghdr {
                #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
                __pad1: 0,
                #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
                cmsg_len: cmsg_len_bytes as u32,
                #[cfg(not(all(target_env = "musl", target_pointer_width = "64")))]
                cmsg_len: cmsg_len_bytes,
                cmsg_level: 1, // SOL_SOCKET
                cmsg_type: 1,  // SCM_RIGHTS
            },
            fd: fd_to_pass,
            pad: [0; 4],
        };
        let mut iov = Iovec {
            iov_base: data.as_mut_ptr(),
            iov_len: data.len(),
        };
        let msg = Msghdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: &mut iov,
            #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
            __pad1: 0,
            #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
            msg_iovlen: 1,
            #[cfg(not(all(target_env = "musl", target_pointer_width = "64")))]
            msg_iovlen: 1,
            msg_control: &mut cmsg_buf as *mut _ as *mut u8,
            #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
            __pad2: 0,
            #[cfg(all(target_env = "musl", target_pointer_width = "64"))]
            msg_controllen: cmsg_controllen as u32,
            #[cfg(not(all(target_env = "musl", target_pointer_width = "64")))]
            msg_controllen: cmsg_controllen,
            msg_flags: 0,
        };
        let sent = unsafe { sendmsg(sock_fd, &msg, 0) };
        sent == (data.len() as isize)
    }

    pub fn poll_wire(&mut self, wire: &mut WaylandWireState) -> Vec<WaylandWireEvent> {
        let mut events = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(n) if n > 0 => {
                    events.extend(wire.feed_incoming_bytes(&buf[..n]));
                }
                _ => break,
            }
        }
        let replies = wire.take_outbound_bytes();
        if !replies.is_empty() {
            let _ = self.stream.write_all(&replies);
            let _ = self.stream.flush();
        }
        events
    }

    pub fn resize_pool(&mut self, width: usize, height: usize, wire: &mut WaylandWireState) {
        let w = width.max(1);
        let h = height.max(1);
        if w == self.pool_width && h == self.pool_height {
            return;
        }
        let needed_len = w * h * 4 * 2;
        if needed_len > self.shm_len && self.shm_fd >= 0 {
            unsafe {
                if ftruncate(self.shm_fd, needed_len as isize) == 0 {
                    if !self.shm_ptr.is_null() && self.shm_len > 0 {
                        let _ = munmap(self.shm_ptr, self.shm_len);
                    }
                    let new_ptr = mmap(
                        core::ptr::null_mut(),
                        needed_len,
                        3, // PROT_READ | PROT_WRITE
                        1, // MAP_SHARED
                        self.shm_fd,
                        0,
                    );
                    if !new_ptr.is_null() && (new_ptr as isize) != -1 {
                        self.shm_ptr = new_ptr;
                        self.shm_len = needed_len;
                        // wl_shm_pool.resize (opcode 2): size(i32)
                        WaylandWireState::write_header(
                            &mut wire.outbound_tx,
                            wire.shm_pool_id,
                            2,
                            12,
                        );
                        WaylandWireState::write_i32(&mut wire.outbound_tx, needed_len as i32);
                    }
                }
            }
        }
        self.pool_width = w;
        self.pool_height = h;
        wire.encode_destroy_double_buffers();
        wire.encode_create_double_buffers(w, h);
        let tx = wire.take_outbound_bytes();
        if !tx.is_empty() {
            let _ = self.stream.write_all(&tx);
            let _ = self.stream.flush();
        }
    }

    pub fn commit_slot_damage(
        &mut self,
        slot_idx: usize,
        pixels: &[Xrgb8888],
        copy_regions: &[Region],
        commit_damage: &[Region],
        wire: &mut WaylandWireState,
    ) {
        let w = self.pool_width;
        let h = self.pool_height;
        let slot_bytes = w * h * 4;
        let base_off = (slot_idx & 1) * slot_bytes;
        if base_off + slot_bytes <= self.shm_len && pixels.len() >= w * h {
            unsafe {
                for reg in copy_regions {
                    let x0 = (reg.x.max(0) as usize).min(w);
                    let y0 = (reg.y.max(0) as usize).min(h);
                    let x1 = (reg.right().max(0) as usize).min(w);
                    let y1 = (reg.bottom().max(0) as usize).min(h);
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    let row_bytes = (x1 - x0) * 4;
                    for y in y0..y1 {
                        let src_ptr = pixels.as_ptr().add(y * w + x0) as *const u8;
                        let dst_ptr = self.shm_ptr.add(base_off + (y * w + x0) * 4);
                        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, row_bytes);
                    }
                }
            }
        }
        wire.encode_surface_commit_damage(slot_idx, commit_damage);
        let tx = wire.take_outbound_bytes();
        if !tx.is_empty() {
            let _ = self.stream.write_all(&tx);
            let _ = self.stream.flush();
        }
    }
}
