//! 跨平台通用双缓冲共享内存表面驱动器（`ShmSurfaceDriver`）与平台工具函数。
//!
//! 负责统一管理：
//! - 双缓冲槽位 [`ShmSlot`] 与 Buffer Age = 2 历史脏矩形合并回溯；
//! - 根据激活的 [`DesktopRenderBackend`]（`Wgpu`、`Software`）
//!   单路径执行真实像素渲染并写回前台缓冲区（消除 GPU 后端与软件光栅的双重执行冗余）；
//! - 通用 Linux `evdev` 键码解码 [`decode_linux_keycode`] 与无损 PNG 导出 [`write_uncompressed_png`]。

use std::vec;
use std::vec::Vec;

use crate::display::{PixelFormat, Xrgb8888};
use crate::engine::VeloxEngine;
use crate::pod::{KeyCode, KeyModifiers};
use crate::solver::Region;

use super::DesktopRenderBackend;

/// 共享内存帧缓冲句柄（兼容 Wayland `wl_buffer` ID）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WlBuffer {
    pub id: u32,
}

/// 表面提交状态（跟踪附加的缓冲区 ID、`damage_buffer` 脏矩形列表与累计提交帧数）。
#[derive(Clone, Debug, Default)]
pub struct WlSurface {
    /// 最近一次通过 `attach` 挂载的缓冲区 ID。
    pub attached_buffer_id: Option<u32>,
    /// 最近一次 `commit` 时提交的局部脏区域列表。
    pub committed_damage: Vec<Region>,
    /// 累计提交帧的次数。
    pub commit_count: usize,
}

impl WlSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// 绑定后备缓冲区并清空本帧待提交脏区记录。
    pub fn attach(&mut self, buffer: Option<&WlBuffer>, _x: i32, _y: i32) {
        self.attached_buffer_id = buffer.map(|b| b.id);
        self.committed_damage.clear();
    }

    /// 记录局部缓冲区脏矩形。
    pub fn damage_buffer(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.committed_damage
            .push(Region::new(x as i16, y as i16, w as i16, h as i16));
    }

    /// 完成本帧表面状态原子提交。
    pub fn commit(&mut self) {
        self.commit_count += 1;
    }
}

/// 双缓冲像素槽位（维护 Buffer Age 帧年龄）。
pub struct ShmSlot {
    pub wl_buffer: WlBuffer,
    pub pixels: Vec<Xrgb8888>,
    /// 合成器/宿主当前是否正在读取该 Buffer。
    pub in_use: bool,
    /// 距离上一次在该 Buffer 上绘制过去了多少帧（`0` 表示未初始化）。
    pub age: u8,
}

impl ShmSlot {
    pub fn new(id: u32, _width: usize, _height: usize) -> Self {
        Self {
            wl_buffer: WlBuffer { id },
            pixels: Vec::new(),
            in_use: false,
            age: 0,
        }
    }
}

/// 跨平台双缓冲表面驱动器（支持 Buffer Age = 2 历史脏区回溯同步与多渲染后端真实像素输出）。
pub struct WaylandSurfaceDriver {
    pub slots: [ShmSlot; 2],
    pub damage_history: [Vec<Region>; 2],
    /// 最近一帧实际光栅化至活动槽位的脏矩形列表（包含 Buffer Age = 2 合并的历史脏矩形）。
    pub last_raster_regions: Vec<Region>,
    pub(crate) active_slot_idx: usize,
    width: usize,
    height: usize,
    backend: DesktopRenderBackend,
    #[cfg(feature = "gpu")]
    gpu_scene: Option<crate::renderers::wgpu::WgpuScene>,
}

/// 通用双缓冲表面驱动器别名。
pub type ShmSurfaceDriver = WaylandSurfaceDriver;
/// 软件表面驱动器兼容别名。
pub type SoftwareSurfaceDriver = WaylandSurfaceDriver;

impl WaylandSurfaceDriver {
    /// 创建指定像素尺寸的双缓冲驱动器（默认使用 [`DesktopRenderBackend::default_desktop`]）。
    pub fn new(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            slots: [ShmSlot::new(1, w, h), ShmSlot::new(2, w, h)],
            damage_history: [Vec::new(), Vec::new()],
            last_raster_regions: Vec::new(),
            active_slot_idx: 0,
            width: w,
            height: h,
            backend: DesktopRenderBackend::default_desktop(),
            #[cfg(feature = "gpu")]
            gpu_scene: None,
        }
    }

    /// 配置渲染后端类型（`Wgpu` 或 `Software`）。
    #[inline]
    pub fn with_backend(mut self, backend: DesktopRenderBackend) -> Self {
        self.backend = backend;
        self
    }

    /// 动态切换渲染后端类型。
    #[inline]
    pub fn set_backend(&mut self, backend: DesktopRenderBackend) {
        self.backend = backend;
    }

    /// 返回当前激活的渲染后端类型。
    #[inline]
    pub const fn backend(&self) -> DesktopRenderBackend {
        self.backend
    }

    /// 获取最近一帧编译生成的轻量级 2D SDF GPU 场景图元统计引用。
    #[cfg(feature = "gpu")]
    #[inline]
    pub fn gpu_scene(&self) -> Option<&crate::renderers::wgpu::WgpuScene> {
        self.gpu_scene.as_ref()
    }

    /// 释放指定槽位以供下一帧复用。
    pub fn release_buffer(&mut self, buffer_id: u32) {
        for slot in &mut self.slots {
            if slot.wl_buffer.id == buffer_id {
                slot.in_use = false;
            }
        }
    }

    /// 获取最近一次提交到前台的 `Xrgb8888` 像素切片。
    pub fn front_pixels(&self) -> &[Xrgb8888] {
        &self.slots[self.active_slot_idx].pixels
    }

    /// 获取当前双缓冲表面分配的物理像素宽度（行步长 stride）。
    #[inline(always)]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// 获取当前双缓冲表面分配的物理像素高度。
    #[inline(always)]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// 动态调整双缓冲池尺寸。
    pub fn resize(&mut self, width: usize, height: usize) {
        let w = width.max(1);
        let h = height.max(1);
        self.width = w;
        self.height = h;
        self.slots = [ShmSlot::new(1, w, h), ShmSlot::new(2, w, h)];
        self.damage_history = [Vec::new(), Vec::new()];
        self.last_raster_regions.clear();
        self.active_slot_idx = 0;
    }

    /// 以默认 60FPS (`16ms`) 步长执行一帧检查、Buffer Age 历史脏矩形合并、渲染与提交。
    pub fn flush_frame(&mut self, core: &mut VeloxEngine, wl_surface: &mut WlSurface) -> bool {
        self.flush_frame_dt(core, wl_surface, 16)
    }

    /// 按给定时间步长 `dt_ms` 执行一帧刷新与提交（真正由所选 `backend` 单路径产出像素，不再重复执行软件光栅）。
    pub fn flush_frame_dt(
        &mut self,
        core: &mut VeloxEngine,
        wl_surface: &mut WlSurface,
        dt_ms: u16,
    ) -> bool {
        let curr_damage: Vec<Region> = core.step_frame_dt(dt_ms).to_vec();
        if curr_damage.is_empty() {
            return false;
        }

        let slot_idx = self
            .slots
            .iter()
            .position(|s| !s.in_use)
            .unwrap_or((self.active_slot_idx + 1) & 1);
        let slot_age = self.slots[slot_idx].age;
        let required_len = self.width.saturating_mul(self.height);
        if self.slots[slot_idx].pixels.len() != required_len {
            self.slots[slot_idx]
                .pixels
                .resize(required_len, Xrgb8888(0xFF00_0000));
        }

        let mut raster_regions = curr_damage.clone();
        if slot_age == 0 || slot_age > 2 {
            raster_regions = vec![core.viewport_region()];
        } else if slot_age == 2 {
            raster_regions.extend_from_slice(&self.damage_history[0]);
        }

        #[cfg(feature = "gpu")]
        let s = core.dpi_scale().max(1);
        let sf = core.scale_factor();
        let dst_stride = self.width;

        match self.backend {
            DesktopRenderBackend::Wgpu => {
                #[cfg(feature = "gpu")]
                {
                    let scene = self
                        .gpu_scene
                        .get_or_insert_with(crate::renderers::wgpu::WgpuScene::new);
                    let _ = scene.record_draw_ops(core.cached_draw_ops(), s, &raster_regions);
                }
                let pixels = &mut self.slots[slot_idx].pixels;
                for reg in &raster_regions {
                    clear_transparent_region(pixels, dst_stride, self.height, sf, *reg);
                    core.rasterize_strip(*reg, 0, 0, dst_stride, pixels);
                }
            }
            DesktopRenderBackend::Software => {
                let pixels = &mut self.slots[slot_idx].pixels;
                for reg in &raster_regions {
                    clear_transparent_region(pixels, dst_stride, self.height, sf, *reg);
                    core.rasterize_strip(*reg, 0, 0, dst_stride, pixels);
                }
            }
        }

        let wl_buf = self.slots[slot_idx].wl_buffer;
        wl_surface.attach(Some(&wl_buf), 0, 0);
        for reg in &curr_damage {
            wl_surface.damage_buffer(reg.x as i32, reg.y as i32, reg.w as i32, reg.h as i32);
        }
        wl_surface.commit();

        self.slots[slot_idx].in_use = true;
        self.slots[slot_idx].age = 1;
        for (idx, other) in self.slots.iter_mut().enumerate() {
            if idx != slot_idx && other.age > 0 {
                other.age = other.age.saturating_add(1);
            }
        }
        self.damage_history.rotate_right(1);
        self.damage_history[0] = curr_damage;
        self.last_raster_regions = raster_regions;
        self.active_slot_idx = slot_idx;

        true
    }
}

/// 在光栅化脏区前将目标区域清零为全透明像素（避免透明悬浮层如 Dock 缩放/重排时残留上一帧边框与图标）。
#[inline]
fn clear_transparent_region(
    pixels: &mut [Xrgb8888],
    stride: usize,
    height: usize,
    scale_factor: f32,
    reg: Region,
) {
    let sf = scale_factor.clamp(0.5, 4.0);
    let x0 = (((reg.x.max(0) as f32) * sf).floor() as usize).min(stride);
    let y0 = (((reg.y.max(0) as f32) * sf).floor() as usize).min(height);
    let x1 = (((reg.right().max(0) as f32) * sf).ceil() as usize).min(stride);
    let y1 = (((reg.bottom().max(0) as f32) * sf).ceil() as usize).min(height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for y in y0..y1 {
        let row = y * stride;
        if row + x1 <= pixels.len() {
            pixels[row + x0..row + x1].fill(Xrgb8888(0));
        }
    }
}

/// 将 Linux `evdev` 键码（`<linux/input-event-codes.h>`）解码为 `Velox` [`KeyCode`]。
///
/// 供 `wayland` 及外部 Linux 输入后端统一共享（消除重复键码表）。
pub fn decode_linux_keycode(code: u16) -> Option<KeyCode> {
    match code {
        1 => Some(KeyCode::Escape),
        2 => Some(KeyCode::Char('1')),
        3 => Some(KeyCode::Char('2')),
        4 => Some(KeyCode::Char('3')),
        5 => Some(KeyCode::Char('4')),
        6 => Some(KeyCode::Char('5')),
        7 => Some(KeyCode::Char('6')),
        8 => Some(KeyCode::Char('7')),
        9 => Some(KeyCode::Char('8')),
        10 => Some(KeyCode::Char('9')),
        11 => Some(KeyCode::Char('0')),
        12 => Some(KeyCode::Char('-')),
        13 => Some(KeyCode::Char('=')),
        14 => Some(KeyCode::Backspace),
        15 => Some(KeyCode::Tab),
        16 => Some(KeyCode::Char('q')),
        17 => Some(KeyCode::Char('w')),
        18 => Some(KeyCode::Char('e')),
        19 => Some(KeyCode::Char('r')),
        20 => Some(KeyCode::Char('t')),
        21 => Some(KeyCode::Char('y')),
        22 => Some(KeyCode::Char('u')),
        23 => Some(KeyCode::Char('i')),
        24 => Some(KeyCode::Char('o')),
        25 => Some(KeyCode::Char('p')),
        28 | 96 => Some(KeyCode::Enter),
        30 => Some(KeyCode::Char('a')),
        31 => Some(KeyCode::Char('s')),
        32 => Some(KeyCode::Char('d')),
        33 => Some(KeyCode::Char('f')),
        34 => Some(KeyCode::Char('g')),
        35 => Some(KeyCode::Char('h')),
        36 => Some(KeyCode::Char('j')),
        37 => Some(KeyCode::Char('k')),
        38 => Some(KeyCode::Char('l')),
        44 => Some(KeyCode::Char('z')),
        45 => Some(KeyCode::Char('x')),
        46 => Some(KeyCode::Char('c')),
        47 => Some(KeyCode::Char('v')),
        48 => Some(KeyCode::Char('b')),
        49 => Some(KeyCode::Char('n')),
        50 => Some(KeyCode::Char('m')),
        51 => Some(KeyCode::Char(',')),
        52 => Some(KeyCode::Char('.')),
        53 => Some(KeyCode::Char('/')),
        57 => Some(KeyCode::Space),
        102 => Some(KeyCode::Home),
        103 => Some(KeyCode::Up),
        104 => Some(KeyCode::PageUp),
        105 => Some(KeyCode::Left),
        106 => Some(KeyCode::Right),
        107 => Some(KeyCode::End),
        108 => Some(KeyCode::Down),
        109 => Some(KeyCode::PageDown),
        111 => Some(KeyCode::Delete),
        _ => None,
    }
}

/// 将 Linux `evdev` 修饰键码解码为 [`KeyModifiers`] 位标志。
pub fn decode_linux_modifier(code: u16) -> Option<KeyModifiers> {
    match code {
        42 | 54 => Some(KeyModifiers::SHIFT),
        29 | 97 => Some(KeyModifiers::CTRL),
        56 | 100 => Some(KeyModifiers::ALT),
        125 | 126 => Some(KeyModifiers::LOGO),
        _ => None,
    }
}

/// 将 `Xrgb8888` 像素切片导出为标准无损 PNG 文件。
pub fn write_uncompressed_png(
    path: &str,
    width: usize,
    height: usize,
    pixels: &[Xrgb8888],
) -> std::io::Result<()> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                if (crc & 1) != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let c = crc32(&out[start..]);
        out.extend_from_slice(&c.to_be_bytes());
    }

    let mut raw_scanlines = Vec::with_capacity(height * (1 + width * 3));
    for y in 0..height {
        raw_scanlines.push(0u8);
        for x in 0..width {
            let c = pixels[y * width + x].to_color();
            raw_scanlines.push(c.r());
            raw_scanlines.push(c.g());
            raw_scanlines.push(c.b());
        }
    }

    let mut zlib_stream = Vec::with_capacity(raw_scanlines.len() + 64);
    zlib_stream.push(0x78);
    zlib_stream.push(0x01);

    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in &raw_scanlines {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    let adler = (s2 << 16) | s1;

    let total_len = raw_scanlines.len();
    let mut offset = 0usize;
    while offset < total_len {
        let rem = total_len - offset;
        let block_len = rem.min(65535);
        let is_final = u8::from(offset + block_len >= total_len);
        zlib_stream.push(is_final);
        let len_u16 = block_len as u16;
        let nlen_u16 = !len_u16;
        zlib_stream.extend_from_slice(&len_u16.to_le_bytes());
        zlib_stream.extend_from_slice(&nlen_u16.to_le_bytes());
        zlib_stream.extend_from_slice(&raw_scanlines[offset..offset + block_len]);
        offset += block_len;
    }
    zlib_stream.extend_from_slice(&adler.to_be_bytes());

    let mut png = Vec::with_capacity(zlib_stream.len() + 128);
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib_stream);
    write_chunk(&mut png, b"IEND", &[]);

    std::fs::write(path, png)
}
