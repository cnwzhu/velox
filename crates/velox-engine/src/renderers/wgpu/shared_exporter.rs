//! 无锁三缓冲共享显存导出渲染器（`WgpuSharedExportRenderer`）。
//!
//! 专为将 Velox 2D SDF GPU 渲染结果零拷贝嵌入外部宿主框架（Qt6 `QQuickRhiItem`、GTK4 `GdkDmabufTextureBuilder`、
//! WinUI 3 `SurfaceImageSource`、WPF `D3DImage`、Android `ASurfaceTransaction`、iOS `IOSurface` 与 Wayland `zwp_linux_dmabuf_v1`）设计：
//! - **无锁三缓冲邮箱（Write / Ready / Read）**：原子 CAS 交换最新完成槽位，渲染与宿主合成线程零锁争用；
//! - **3 帧脏区合并防残影**：自动合并最近 3 帧脏矩形并集，并结合 GPU-to-GPU 极速局部拷贝消除三缓冲 3 帧老画面闪烁；
//! - **预乘透明背景（Premultiplied Alpha）**：默认清屏色 `wgpu::Color::TRANSPARENT`，原生支持镂空悬浮 HUD 叠放；
//! - **安全 Resize 代数隔离与延迟回收（`generation_id` + `retired_pools`）**：宿主持有旧句柄期间绝不悬垂释放。

use ::wgpu;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::collections::VecDeque;
use std::vec;
use std::vec::Vec;

use crate::arena::SmallVec;
use crate::engine::VeloxEngine;
use crate::motion::DrawOp;
use crate::paint::ExternalTextureId;
use crate::solver::Region;

use super::interop::{
    allocate_shared_texture, create_shared_fence, ExportedHandle, ExportedTextureAlloc,
    ExternalSyncHandle, PlatformSharedFence,
};
use super::pipeline::WgpuRenderer;
use super::window::PreparedWgpuContext;

/// 单个三缓冲共享显存槽位（包装外部显存句柄纹理与最新就绪栅栏值）。
pub struct SharedSlot {
    alloc: ExportedTextureAlloc,
    ready_fence_val: AtomicU64,
}

impl SharedSlot {
    /// 返回底层共享 `wgpu::Texture` 引用。
    #[inline]
    pub fn texture(&self) -> &wgpu::Texture {
        &self.alloc.texture
    }

    /// 返回底层共享 `wgpu::TextureView` 引用。
    #[inline]
    pub fn view(&self) -> &wgpu::TextureView {
        &self.alloc.view
    }

    /// 返回跨平台共享显存句柄描述符。
    #[inline]
    pub const fn handle(&self) -> ExportedHandle {
        self.alloc.handle
    }

    /// 返回该槽位最近一次写入完成对应的 GPU 栅栏值。
    #[inline]
    pub fn ready_fence_value(&self) -> u64 {
        self.ready_fence_val.load(Ordering::Acquire)
    }
}

/// 宿主调用 [`WgpuSharedExportRenderer::acquire_shared_frame`] 获得的只读共享帧租约。
#[derive(Clone, Debug)]
pub struct SharedFrameLease {
    /// 表面重建代数（窗口 Resize 后递增，宿主据以检测并重新导入共享句柄）。
    pub generation_id: u32,
    /// 当前锁定的三缓冲槽位索引（`0..3`）。
    pub slot_index: u8,
    /// 物理像素宽度。
    pub width: u32,
    /// 物理像素高度。
    pub height: u32,
    /// 像素格式（默认 `Bgra8Unorm` 或 `Rgba8Unorm`，预乘 Alpha）。
    pub format: wgpu::TextureFormat,
    /// 跨 API 显存导出句柄（Win32 NT HANDLE / Linux dma_buf fd / Apple IOSurface / Android AHB）。
    pub handle: ExportedHandle,
    /// 跨 API GPU 同步栅栏/信号量句柄。
    pub sync_handle: ExternalSyncHandle,
    /// 宿主采样前需等待的 GPU 栅栏单调计数值。
    pub fence_value: u64,
    /// 合并最近 3 帧后的有效脏矩形列表（供宿主做局部合成优化）。
    pub dirty_rects: SmallVec<Region, 8>,
    /// 本次获取是否为新提交帧（`false` 表示复用了上次最新帧）。
    pub is_fresh: bool,
}

/// 等待宿主归还或 GPU 栅栏通过后延迟释放的旧代数三缓冲池。
struct RetiredPool {
    generation_id: u32,
    last_fence_val: u64,
    in_use_by_host: bool,
    _slots: [SharedSlot; 3],
}

/// 打包无锁三缓冲状态字：`ready (0..2) | read (2..4) | write (4..6) | fresh (6)`。
#[inline(always)]
const fn pack_mailbox(ready: u8, read: u8, write: u8, fresh: bool) -> u8 {
    (ready & 0x3) | ((read & 0x3) << 2) | ((write & 0x3) << 4) | (if fresh { 1 << 6 } else { 0 })
}

#[inline(always)]
const fn unpack_mailbox(state: u8) -> (u8, u8, u8, bool) {
    (
        state & 0x3,
        (state >> 2) & 0x3,
        (state >> 4) & 0x3,
        (state & (1 << 6)) != 0,
    )
}

/// 跨框架零拷贝显存共享导出渲染器（无锁三缓冲邮箱 + 3 帧脏区合并 + 预乘 Alpha HUD + 延迟回收）。
pub struct WgpuSharedExportRenderer {
    ctx: PreparedWgpuContext,
    sdf_renderer: WgpuRenderer,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    clear_color: wgpu::Color,
    /// 持久增量画布纹理（承载本帧渲染，通过 GPU-to-GPU 拷贝同步至三缓冲导出槽，彻底避免三缓冲 3 帧残影）。
    canvas_tex: wgpu::Texture,
    canvas_view: wgpu::TextureView,
    slots: [SharedSlot; 3],
    /// 无锁三缓冲邮箱状态字（初始 `ready=0, read=1, write=2, fresh=false`）。
    mailbox_state: AtomicU8,
    shared_fence: PlatformSharedFence,
    next_fence_val: AtomicU64,
    generation_id: u32,
    /// 最近 2 帧的物理脏矩形历史（用于计算三缓冲 `Union(Dirty[N], Dirty[N-1], Dirty[N-2])`）。
    dirty_history: [SmallVec<Region, 8>; 2],
    last_merged_dirty: SmallVec<Region, 8>,
    force_full_sync: bool,
    retired_pools: VecDeque<RetiredPool>,
}

impl WgpuSharedExportRenderer {
    /// 按宿主物理显卡 LUID / UUID 对齐并初始化三缓冲共享导出渲染器。
    ///
    /// 默认使用 `Bgra8Unorm`（Windows/macOS/Linux 原生合成格式）与 `wgpu::Color::TRANSPARENT` 预乘透明清屏，
    /// 直接支持镂空悬浮 HUD 叠放至外部窗口上方。
    pub fn new(
        width: u32,
        height: u32,
        adapter_luid: Option<[u8; 8]>,
        device_uuid: Option<[u8; 16]>,
    ) -> Option<Self> {
        let ctx = PreparedWgpuContext::from_host_adapter_id(adapter_luid, device_uuid)?;
        Some(Self::from_context(
            ctx,
            width,
            height,
            wgpu::TextureFormat::Bgra8Unorm,
        ))
    }

    /// 从已有的 [`PreparedWgpuContext`]（支持与外部渲染器共享同一 `wgpu::Device`）创建三缓冲共享导出渲染器。
    pub fn from_context(
        mut ctx: PreparedWgpuContext,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let sdf_renderer = ctx.take_or_create_renderer(format);
        let luid = ctx.adapter_luid().unwrap_or([0u8; 8]);
        let (canvas_tex, canvas_view) = Self::create_canvas(ctx.device(), w, h, format);
        let slots = Self::create_slot_triple(ctx.device(), w, h, format, luid);
        let shared_fence = create_shared_fence(ctx.device());

        Self {
            ctx,
            sdf_renderer,
            format,
            width: w,
            height: h,
            clear_color: wgpu::Color::TRANSPARENT,
            canvas_tex,
            canvas_view,
            slots,
            mailbox_state: AtomicU8::new(pack_mailbox(0, 1, 2, false)),
            shared_fence,
            next_fence_val: AtomicU64::new(1),
            generation_id: 1,
            dirty_history: [SmallVec::new(), SmallVec::new()],
            last_merged_dirty: SmallVec::new(),
            force_full_sync: true,
            retired_pools: VecDeque::new(),
        }
    }

    fn create_canvas(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_shared_exporter_canvas"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    fn create_slot_triple(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        luid: [u8; 8],
    ) -> [SharedSlot; 3] {
        [
            SharedSlot {
                alloc: allocate_shared_texture(device, width, height, format, luid),
                ready_fence_val: AtomicU64::new(0),
            },
            SharedSlot {
                alloc: allocate_shared_texture(device, width, height, format, luid),
                ready_fence_val: AtomicU64::new(0),
            },
            SharedSlot {
                alloc: allocate_shared_texture(device, width, height, format, luid),
                ready_fence_val: AtomicU64::new(0),
            },
        ]
    }

    /// 设置清屏背景色（默认 `wgpu::Color::TRANSPARENT`，供镂空悬浮 HUD 预乘 Alpha 合成）。
    #[inline]
    pub fn set_clear_color(&mut self, color: wgpu::Color) {
        self.clear_color = color;
    }

    /// 返回当前共享表面重建代数 `generation_id`。
    #[inline]
    pub const fn generation_id(&self) -> u32 {
        self.generation_id
    }

    /// 返回当前共享目标物理像素尺寸 `(width, height)`。
    #[inline]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 返回当前像素格式。
    #[inline]
    pub const fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// 返回底层 `wgpu::Device` 引用（可用于同设备创建外部纹理并注册零拷贝导入）。
    #[inline]
    pub fn device(&self) -> &wgpu::Device {
        self.ctx.device()
    }

    /// 返回底层 `wgpu::Queue` 引用。
    #[inline]
    pub fn queue(&self) -> &wgpu::Queue {
        self.ctx.queue()
    }

    /// 返回物理适配器名称与图形 API 名称。
    #[inline]
    pub fn adapter_info(&self) -> (&str, &'static str) {
        (self.ctx.adapter_name(), self.ctx.backend_name())
    }

    /// 返回当前匹配到的 Windows DX12 物理显卡 LUID（若可用）。
    #[inline]
    pub const fn adapter_luid(&self) -> Option<[u8; 8]> {
        self.ctx.adapter_luid()
    }

    /// 返回当前匹配到的 Vulkan 物理显卡 UUID（若可用）。
    #[inline]
    pub const fn device_uuid(&self) -> Option<[u8; 16]> {
        self.ctx.device_uuid()
    }

    /// 返回跨平台共享 GPU 同步栅栏句柄。
    #[inline]
    pub fn sync_handle(&self) -> ExternalSyncHandle {
        self.shared_fence.export_handle()
    }

    /// 返回内部 `WgpuRenderer` 可变引用。
    #[inline]
    pub fn renderer_mut(&mut self) -> &mut WgpuRenderer {
        &mut self.sdf_renderer
    }

    /// 注册外部零拷贝 GPU 纹理（支持“导入外部海图 -> Velox 叠加毛玻璃 HUD -> 导出共享句柄”全链路闭环）。
    #[inline]
    pub fn register_external_texture(
        &mut self,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) -> ExternalTextureId {
        self.sdf_renderer.register_external_texture(view, sampler)
    }

    /// 原地更新已注册的外部零拷贝 GPU 纹理视图。
    #[inline]
    pub fn update_external_texture(
        &mut self,
        id: ExternalTextureId,
        view: wgpu::TextureView,
    ) -> bool {
        self.sdf_renderer.update_external_texture(id, view)
    }

    /// 注销外部零拷贝纹理。
    #[inline]
    pub fn unregister_external_texture(&mut self, id: ExternalTextureId) -> bool {
        self.sdf_renderer.unregister_external_texture(id)
    }

    /// 返回待延迟销毁的旧代数池数量（用于验证 Resize 后的延迟回收机制）。
    #[inline]
    pub fn retired_pool_count(&self) -> usize {
        self.retired_pools.len()
    }

    /// 调整导出表面物理尺寸：重建三缓冲并递增 `generation_id`，将旧槽位移入延迟回收队列防悬垂。
    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return;
        }
        let luid = self.ctx.adapter_luid().unwrap_or([0u8; 8]);
        let new_slots = Self::create_slot_triple(self.ctx.device(), w, h, self.format, luid);
        let old_slots = core::mem::replace(&mut self.slots, new_slots);
        let last_fence = self.next_fence_val.load(Ordering::Acquire).saturating_sub(1);
        self.retired_pools.push_back(RetiredPool {
            generation_id: self.generation_id,
            last_fence_val: last_fence,
            in_use_by_host: true,
            _slots: old_slots,
        });

        let (ct, cv) = Self::create_canvas(self.ctx.device(), w, h, self.format);
        self.canvas_tex = ct;
        self.canvas_view = cv;
        self.width = w;
        self.height = h;
        self.generation_id = self.generation_id.wrapping_add(1).max(1);
        self.mailbox_state
            .store(pack_mailbox(0, 1, 2, false), Ordering::Release);
        self.dirty_history[0].clear();
        self.dirty_history[1].clear();
        self.last_merged_dirty.clear();
        self.force_full_sync = true;
        self.gc_retired_pools();
    }

    /// 回收宿主已释放且 GPU 栅栏已完成的旧代数共享池。
    pub fn gc_retired_pools(&mut self) {
        let completed = self.shared_fence.completed_value();
        while let Some(front) = self.retired_pools.front() {
            if !front.in_use_by_host && completed >= front.last_fence_val {
                self.retired_pools.pop_front();
            } else {
                break;
            }
        }
    }

    /// 计算三缓冲安全的 3 帧脏矩形并集 `Union(Dirty[N], Dirty[N-1], Dirty[N-2])`。
    fn merge_3_frame_dirty(&mut self, current_dirty: &[Region], sf: f32) -> SmallVec<Region, 8> {
        let full = Region::new(0, 0, self.width as i16, self.height as i16);
        let mut cur_phys: SmallVec<Region, 8> = SmallVec::new();
        for &r in current_dirty {
            if r.is_empty() {
                continue;
            }
            let x0 = ((r.x as f32) * sf).floor().max(0.0) as i16;
            let y0 = ((r.y as f32) * sf).floor().max(0.0) as i16;
            let x1 = ((r.right() as f32) * sf).ceil().min(self.width as f32) as i16;
            let y1 = ((r.bottom() as f32) * sf).ceil().min(self.height as f32) as i16;
            if x1 > x0 && y1 > y0 {
                let _ = cur_phys.push(Region::new(x0, y0, x1 - x0, y1 - y0));
            }
        }

        let mut merged: SmallVec<Region, 8> = SmallVec::new();
        if self.force_full_sync || cur_phys.is_empty() {
            let _ = merged.push(full);
            self.force_full_sync = false;
        } else {
            let mut push_or_union = |r: Region| {
                if !merged.push(r) {
                    // 超过 8 块时合并进首块包围盒，保证零堆且绝对不丢脏区
                    let mut u = merged[0];
                    let x0 = u.x.min(r.x);
                    let y0 = u.y.min(r.y);
                    let x1 = u.right().max(r.right());
                    let y1 = u.bottom().max(r.bottom());
                    u = Region::new(x0, y0, x1 - x0, y1 - y0);
                    merged[0] = u;
                }
            };
            for &r in cur_phys.as_slice() {
                push_or_union(r);
            }
            for &r in self.dirty_history[0].as_slice() {
                push_or_union(r);
            }
            for &r in self.dirty_history[1].as_slice() {
                push_or_union(r);
            }
        }

        self.dirty_history[1] = self.dirty_history[0].clone();
        self.dirty_history[0] = cur_phys;
        self.last_merged_dirty = merged.clone();
        merged
    }

    /// 将当前帧 `DrawOp` 渲染并零拷贝发布到三缓冲邮箱的 `Ready` 槽位，返回该帧对应的 GPU 栅栏值。
    pub fn render_draw_ops(
        &mut self,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        dirty_regions: &[Region],
    ) -> u64 {
        let sf = scale_factor.clamp(0.5, 4.0);
        let pw = ((viewport.w.max(1) as f32) * sf).round().max(1.0) as u32;
        let ph = ((viewport.h.max(1) as f32) * sf).round().max(1.0) as u32;
        if pw != self.width || ph != self.height {
            self.resize(pw, ph);
        }

        let merged_dirty = self.merge_3_frame_dirty(dirty_regions, sf);

        // 1. 100% 复用现有 WgpuRenderer 渲染到持久画布纹理（零破坏现有 SDF + 渐变 + 毛玻璃 + 外部导入管线）
        self.sdf_renderer.render_to_view(
            self.ctx.device(),
            self.ctx.queue(),
            ops,
            viewport,
            sf,
            &self.canvas_view,
            self.width,
            self.height,
            self.clear_color,
        );

        // 2. 确定当前渲染线程独占的 `write_idx` 槽位，执行 GPU-to-GPU 极速脏区拷贝
        let (_, _, write_idx, _) = unpack_mailbox(self.mailbox_state.load(Ordering::Acquire));
        let dst_tex = &self.slots[write_idx as usize].alloc.texture;
        let mut copy_enc =
            self.ctx
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("velox_shared_exporter_copy"),
                });
        for &r in merged_dirty.as_slice() {
            let x = (r.x.max(0) as u32).min(self.width);
            let y = (r.y.max(0) as u32).min(self.height);
            let cw = (r.w.max(0) as u32).min(self.width.saturating_sub(x));
            let ch = (r.h.max(0) as u32).min(self.height.saturating_sub(y));
            if cw == 0 || ch == 0 {
                continue;
            }
            copy_enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.canvas_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: dst_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: cw,
                    height: ch,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.ctx.queue().submit([copy_enc.finish()]);

        // 3. 在 GPU 命令队列末尾写入单调递增的共享栅栏值
        let fence_val = self.next_fence_val.fetch_add(1, Ordering::AcqRel);
        self.shared_fence
            .signal_on_queue(self.ctx.device(), fence_val);
        self.slots[write_idx as usize]
            .ready_fence_val
            .store(fence_val, Ordering::Release);

        // 4. 无锁 CAS 交换 `write_idx <-> ready_idx` 并置 `fresh = true`
        loop {
            let cur = self.mailbox_state.load(Ordering::Acquire);
            let (ready, read, write, _fresh) = unpack_mailbox(cur);
            let next = pack_mailbox(write, read, ready, true);
            if self
                .mailbox_state
                .compare_exchange_weak(cur, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }

        self.gc_retired_pools();
        fence_val
    }

    /// 推进一帧 [`VeloxEngine`] 并在有脏区（或首帧）时渲染发布至共享三缓冲邮箱。
    pub fn step_and_render(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> Option<u64> {
        let mut dirty: SmallVec<Region, 8> = SmallVec::new();
        for &r in engine.step_frame_dt(dt_ms) {
            let _ = dirty.push(r);
        }
        if dirty.is_empty() && !self.force_full_sync {
            return None;
        }
        let vp = engine.viewport_region();
        let sf = engine.scale_factor();
        let ops = engine.cached_draw_ops();
        Some(self.render_draw_ops(ops, vp, sf, dirty.as_slice()))
    }

    /// 宿主合成线程无锁获取最新就绪的共享显存帧句柄（若无新帧则返回当前 `read` 槽位且 `is_fresh = false`）。
    pub fn acquire_shared_frame(&self) -> SharedFrameLease {
        let (slot_idx, is_fresh) = loop {
            let cur = self.mailbox_state.load(Ordering::Acquire);
            let (ready, read, write, fresh) = unpack_mailbox(cur);
            if !fresh {
                break (read, false);
            }
            let next = pack_mailbox(read, ready, write, false);
            if self
                .mailbox_state
                .compare_exchange_weak(cur, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break (ready, true);
            }
        };

        let slot = &self.slots[slot_idx as usize];
        SharedFrameLease {
            generation_id: self.generation_id,
            slot_index: slot_idx,
            width: self.width,
            height: self.height,
            format: self.format,
            handle: slot.handle(),
            sync_handle: self.shared_fence.export_handle(),
            fence_value: slot.ready_fence_value(),
            dirty_rects: self.last_merged_dirty.clone(),
            is_fresh,
        }
    }

    /// 宿主完成采样后归还帧租约；若为旧代数租约则标记为可安全回收并触发延迟垃圾回收。
    pub fn release_shared_frame(&mut self, generation_id: u32, _slot_index: u8) {
        if generation_id != self.generation_id {
            for pool in &mut self.retired_pools {
                if pool.generation_id == generation_id {
                    pool.in_use_by_host = false;
                }
            }
            self.gc_retired_pools();
        }
    }

    /// 阻塞等待指定栅栏值在 GPU 上执行完毕（主要供同步测试或同步宿主使用）。
    pub fn wait_fence_cpu(&self, fence_value: u64, timeout_ms: u32) -> bool {
        self.shared_fence
            .wait_cpu(self.ctx.device(), fence_value, timeout_ms)
    }

    /// 测试与诊断辅助：将指定共享槽位的显存内容回读为直通 RGBA/BGRA 字节流（验证真实渲染像素与透明通道）。
    pub fn readback_slot_bytes(&self, slot_index: u8) -> Option<Vec<u8>> {
        let slot = self.slots.get(slot_index as usize)?;
        let fence_val = slot.ready_fence_value();
        if fence_val > 0 {
            let _ = self.wait_fence_cpu(fence_val, 3000);
        }
        let unpadded = self.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let rb = self.ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_shared_slot_readback"),
            size: (padded as u64) * (self.height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc =
            self.ctx
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("velox_shared_slot_readback_enc"),
                });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: slot.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &rb,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.ctx.queue().submit([enc.finish()]);

        let slice = rb.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        let _ = self.ctx.device().poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let mut out = vec![0u8; (self.width as usize) * (self.height as usize) * 4];
        let row_bytes = (self.width as usize) * 4;
        let stride = padded as usize;
        for y in 0..(self.height as usize) {
            let src = &mapped[y * stride..y * stride + row_bytes];
            out[y * row_bytes..(y + 1) * row_bytes].copy_from_slice(src);
        }
        drop(mapped);
        rb.unmap();
        Some(out)
    }
}
