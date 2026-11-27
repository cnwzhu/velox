//! GPU 离屏渲染目标复用池（`LayerTargetPool`）与 SVG Path `R8Unorm` 遮罩缓存。
//!
//! 按 64 像素桶复用离屏纹理，零帧间显存反复申请/销毁，供隔离图层、Dual Kawase Blur、
//! 实时毛玻璃（`backdrop-filter`）与可编程 W3C 混合模式共享。

use ::wgpu;
use std::collections::HashMap;
use std::vec;
use std::vec::Vec;

use crate::display::{PixelFormat, Rgba8888};
use crate::solver::Region;
use crate::style::Color;
use crate::svg::{rasterize_svg, rasterize_svg_path, SvgRenderOptions};

pub(crate) struct PooledTarget {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) byte_size: usize,
    pub(crate) generation: u64,
    last_used_frame: u64,
    in_use: bool,
}

pub(crate) struct CachedPathMask {
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    byte_size: usize,
    last_used_frame: u64,
}

const MAX_POOLED_TARGETS: usize = 32;
const MAX_CACHED_MASKS: usize = 64;
/// 默认空闲离屏纹理池显存预算（32 MiB：4K 单张约 31.9 MiB，多张特效页切回普通页时立即压回预算内）。
pub(crate) const DEFAULT_IDLE_TARGET_BUDGET_BYTES: usize = 32 * 1024 * 1024;
/// 默认空闲离屏纹理最大滞留帧数（切回普通页约 1 秒 @60fps 后自动全部释放）。
pub(crate) const DEFAULT_IDLE_TARGET_MAX_AGE_FRAMES: u64 = 60;
/// 默认 SVG Path `R8Unorm` 遮罩缓存字节预算（8 MiB）。
pub(crate) const DEFAULT_MASK_BUDGET_BYTES: usize = 8 * 1024 * 1024;

#[inline]
fn format_bytes_per_pixel(format: wgpu::TextureFormat) -> usize {
    match format {
        wgpu::TextureFormat::R8Unorm | wgpu::TextureFormat::R8Snorm => 1,
        wgpu::TextureFormat::Rg8Unorm
        | wgpu::TextureFormat::Rg8Snorm
        | wgpu::TextureFormat::R16Float => 2,
        wgpu::TextureFormat::Rgba16Float => 8,
        wgpu::TextureFormat::Rgba32Float => 16,
        _ => 4,
    }
}

#[inline]
fn target_byte_size(w: u32, h: u32, format: wgpu::TextureFormat) -> usize {
    (w as usize)
        .saturating_mul(h as usize)
        .saturating_mul(format_bytes_per_pixel(format))
}

pub(crate) struct LayerTargetPool {
    format: wgpu::TextureFormat,
    targets: Vec<PooledTarget>,
    next_gen: u64,
    frame_index: u64,
    total_target_bytes: usize,
    idle_target_budget_bytes: usize,
    idle_target_max_age_frames: u64,
    white_mask: CachedPathMask,
    path_masks: HashMap<u64, CachedPathMask>,
    total_mask_bytes: usize,
    mask_budget_bytes: usize,
    masks_evicted: bool,
}

impl LayerTargetPool {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let white_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_white_mask_1x1"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &white_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let white_view = white_tex.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            format,
            targets: Vec::with_capacity(8),
            next_gen: 1,
            frame_index: 0,
            total_target_bytes: 0,
            idle_target_budget_bytes: DEFAULT_IDLE_TARGET_BUDGET_BYTES,
            idle_target_max_age_frames: DEFAULT_IDLE_TARGET_MAX_AGE_FRAMES,
            white_mask: CachedPathMask {
                _texture: white_tex,
                view: white_view,
                byte_size: 1,
                last_used_frame: 0,
            },
            path_masks: HashMap::with_capacity(16),
            total_mask_bytes: 0,
            mask_budget_bytes: DEFAULT_MASK_BUDGET_BYTES,
            masks_evicted: false,
        }
    }

    /// 帧开始时推进帧计数并归还所有已借出的离屏目标（保留热点纹理跨帧复用）。
    #[inline]
    pub(crate) fn begin_frame(&mut self) {
        self.frame_index = self.frame_index.saturating_add(1);
        for t in &mut self.targets {
            t.in_use = false;
        }
    }

    /// 借用一张尺寸不小于 `(req_w, req_h)` 的离屏目标纹理，返回其在池中的稳定索引。
    pub(crate) fn acquire(&mut self, device: &wgpu::Device, req_w: u32, req_h: u32) -> usize {
        let bw = req_w.max(1).div_ceil(64) * 64;
        let bh = req_h.max(1).div_ceil(64) * 64;

        // 寻找最紧凑的空闲槽位（面积不大于需求 4 倍以避免小图层误占全屏大纹理）
        let mut best: Option<(usize, u64)> = None;
        let req_area = (bw as u64) * (bh as u64);
        for (idx, t) in self.targets.iter().enumerate() {
            if !t.in_use && t.width >= bw && t.height >= bh {
                let area = (t.width as u64) * (t.height as u64);
                if area <= req_area.saturating_mul(4).max(65_536) {
                    if best.is_none_or(|(_, best_a)| area < best_a) {
                        best = Some((idx, area));
                    }
                }
            }
        }
        if let Some((idx, _)) = best {
            let t = &mut self.targets[idx];
            t.in_use = true;
            t.last_used_frame = self.frame_index;
            return idx;
        }

        let byte_size = target_byte_size(bw, bh, self.format);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_pooled_layer_target"),
            size: wgpu::Extent3d {
                width: bw,
                height: bh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let generation = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1).max(1);
        let entry = PooledTarget {
            texture: tex,
            view,
            width: bw,
            height: bh,
            byte_size,
            generation,
            last_used_frame: self.frame_index,
            in_use: true,
        };

        // 若池已达数量上限，按最久未用优先回收一个空闲槽位重建
        if self.targets.len() >= MAX_POOLED_TARGETS {
            if let Some((evict, _)) = self
                .targets
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.in_use)
                .min_by_key(|(_, t)| (t.last_used_frame, core::cmp::Reverse(t.byte_size)))
            {
                let old_bytes = self.targets[evict].byte_size;
                self.total_target_bytes = self
                    .total_target_bytes
                    .saturating_sub(old_bytes)
                    .saturating_add(byte_size);
                self.targets[evict] = entry;
                return evict;
            }
        }
        let idx = self.targets.len();
        self.total_target_bytes = self.total_target_bytes.saturating_add(byte_size);
        self.targets.push(entry);
        idx
    }

    #[inline]
    pub(crate) fn release(&mut self, idx: usize) {
        if let Some(t) = self.targets.get_mut(idx) {
            t.in_use = false;
            t.last_used_frame = self.frame_index;
        }
    }

    /// 多 Pass 帧提交完成后调用：归还本帧全部槽位并按字节预算与闲置年龄回收多余空闲纹理。
    /// 返回是否销毁了任意纹理或遮罩（调用方据此清理失效 `BindGroup`）。
    pub(crate) fn end_frame_reclaim(&mut self) -> bool {
        for t in &mut self.targets {
            t.in_use = false;
        }
        self.reclaim_idle()
    }

    /// 普通单 Pass 帧（无特效/图层）推进帧计数并按年龄与字节预算回收闲置离屏纹理。
    pub(crate) fn tick_idle_frame(&mut self) -> bool {
        self.frame_index = self.frame_index.saturating_add(1);
        for t in &mut self.targets {
            t.in_use = false;
        }
        self.reclaim_idle()
    }

    /// 按闲置年龄与字节预算回收空闲离屏纹理与 SVG 遮罩缓存。
    fn reclaim_idle(&mut self) -> bool {
        let mut evicted = false;

        // 1. 回收超过最大闲置帧数的陈旧空闲纹理
        let max_age = self.idle_target_max_age_frames;
        let cur_frame = self.frame_index;
        if max_age > 0 {
            let mut i = 0;
            while i < self.targets.len() {
                let t = &self.targets[i];
                if !t.in_use && cur_frame.saturating_sub(t.last_used_frame) >= max_age {
                    let removed = self.targets.swap_remove(i);
                    self.total_target_bytes = self.total_target_bytes.saturating_sub(removed.byte_size);
                    evicted = true;
                } else {
                    i += 1;
                }
            }
        }

        // 2. 若空闲纹理总字节仍超过预算，按「最久未用优先、同帧下最大纹理优先」逐个回收
        let mut idle_bytes: usize = self
            .targets
            .iter()
            .filter(|t| !t.in_use)
            .map(|t| t.byte_size)
            .sum();
        while idle_bytes > self.idle_target_budget_bytes {
            let Some((victim, _)) = self
                .targets
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.in_use)
                .min_by_key(|(_, t)| (t.last_used_frame, core::cmp::Reverse(t.byte_size)))
            else {
                break;
            };
            let removed = self.targets.swap_remove(victim);
            idle_bytes = idle_bytes.saturating_sub(removed.byte_size);
            self.total_target_bytes = self.total_target_bytes.saturating_sub(removed.byte_size);
            evicted = true;
        }

        // 3. 遮罩缓存超出字节预算或数量上限时按 LRU 淘汰
        if self.evict_masks_to_budget(0) {
            evicted = true;
        }
        if self.masks_evicted {
            self.masks_evicted = false;
            evicted = true;
        }

        evicted
    }

    /// 视口尺寸变化（尤其是窗口缩小）时，立即回收明显大于新视口的空闲大纹理。
    pub(crate) fn on_viewport_resized(&mut self, new_w: u32, new_h: u32) -> bool {
        let nbw = new_w.max(1).div_ceil(64) * 64;
        let nbh = new_h.max(1).div_ceil(64) * 64;
        let mut evicted = false;
        let mut i = 0;
        while i < self.targets.len() {
            let t = &self.targets[i];
            if !t.in_use && (t.width > nbw || t.height > nbh) {
                let removed = self.targets.swap_remove(i);
                self.total_target_bytes = self.total_target_bytes.saturating_sub(removed.byte_size);
                evicted = true;
            } else {
                i += 1;
            }
        }
        if self.reclaim_idle() {
            evicted = true;
        }
        evicted
    }

    /// 后台化或内存警告时立即释放全部空闲离屏纹理与 SVG 遮罩缓存。
    pub(crate) fn trim_all_idle(&mut self) -> bool {
        let mut evicted = false;
        let mut i = 0;
        while i < self.targets.len() {
            if !self.targets[i].in_use {
                let removed = self.targets.swap_remove(i);
                self.total_target_bytes = self.total_target_bytes.saturating_sub(removed.byte_size);
                evicted = true;
            } else {
                i += 1;
            }
        }
        if !self.path_masks.is_empty() {
            self.path_masks.clear();
            self.total_mask_bytes = 0;
            self.masks_evicted = false;
            evicted = true;
        }
        evicted
    }

    fn evict_masks_to_budget(&mut self, incoming_bytes: usize) -> bool {
        let mut evicted = false;
        let cap = MAX_CACHED_MASKS.saturating_sub(usize::from(incoming_bytes > 0));
        while !self.path_masks.is_empty()
            && (self.total_mask_bytes.saturating_add(incoming_bytes) > self.mask_budget_bytes
                || self.path_masks.len() > cap)
        {
            let Some((&victim_key, _)) = self
                .path_masks
                .iter()
                .min_by_key(|(_, m)| (m.last_used_frame, core::cmp::Reverse(m.byte_size)))
            else {
                break;
            };
            if let Some(removed) = self.path_masks.remove(&victim_key) {
                self.total_mask_bytes = self.total_mask_bytes.saturating_sub(removed.byte_size);
                evicted = true;
            }
        }
        evicted
    }

    #[inline]
    pub(crate) fn take_masks_evicted(&mut self) -> bool {
        let v = self.masks_evicted;
        self.masks_evicted = false;
        v
    }

    #[inline]
    pub(crate) fn has_live_generation(&self, generation: u64) -> bool {
        self.targets.iter().any(|t| t.generation == generation)
    }

    #[inline]
    pub(crate) fn has_live_mask(&self, mask_key: u64) -> bool {
        mask_key == 0 || self.path_masks.contains_key(&mask_key)
    }

    #[inline]
    pub(crate) fn target_bytes(&self) -> usize {
        self.total_target_bytes
    }

    #[inline]
    pub(crate) fn mask_bytes(&self) -> usize {
        self.total_mask_bytes
    }

    #[inline]
    pub(crate) fn target_count(&self) -> usize {
        self.targets.len()
    }

    #[inline]
    pub(crate) fn mask_count(&self) -> usize {
        self.path_masks.len()
    }

    #[inline]
    pub(crate) fn set_idle_target_budget_bytes(&mut self, bytes: usize) -> bool {
        self.idle_target_budget_bytes = bytes;
        self.reclaim_idle()
    }

    #[inline]
    pub(crate) fn set_idle_target_max_age_frames(&mut self, frames: u64) -> bool {
        self.idle_target_max_age_frames = frames;
        self.reclaim_idle()
    }

    #[inline]
    pub(crate) fn set_mask_budget_bytes(&mut self, bytes: usize) -> bool {
        self.mask_budget_bytes = bytes;
        self.reclaim_idle()
    }

    #[inline]
    pub(crate) fn get(&self, idx: usize) -> &PooledTarget {
        &self.targets[idx]
    }

    #[inline]
    pub(crate) fn white_mask_view(&self) -> &wgpu::TextureView {
        &self.white_mask.view
    }

    /// 确保指定 SVG Path 抗锯齿 `R8Unorm` 裁剪遮罩已上传缓存，返回其缓存键（按字节预算与 LRU 淘汰）。
    pub(crate) fn ensure_path_mask(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path_d: &str,
        w: u32,
        h: u32,
    ) -> u64 {
        let w = w.max(1);
        let h = h.max(1);
        let mut key: u64 = 0xcbf2_9ce4_8422_2325;
        let mix = |k: &mut u64, v: u64| {
            *k ^= v;
            *k = k.wrapping_mul(0x0100_0000_01b3);
        };
        mix(&mut key, w as u64);
        mix(&mut key, h as u64);
        for &b in path_d.as_bytes() {
            mix(&mut key, b as u64);
        }

        if let Some(entry) = self.path_masks.get_mut(&key) {
            entry.last_used_frame = self.frame_index;
            return key;
        }
        let incoming_bytes = (w as usize).saturating_mul(h as usize);
        if self.evict_masks_to_budget(incoming_bytes) {
            self.masks_evicted = true;
        }

        let mut rgba = vec![Rgba8888(0); (w as usize) * (h as usize)];
        let reg = Region::new(0, 0, w as i16, h as i16);
        if path_d.trim_start().starts_with('<') {
            rasterize_svg(
                path_d,
                reg,
                reg,
                SvgRenderOptions::default(),
                0,
                0,
                w as usize,
                &mut rgba,
            );
        } else {
            rasterize_svg_path(
                path_d,
                reg,
                reg,
                Color::WHITE,
                0,
                0,
                0,
                w as usize,
                &mut rgba,
            );
        }
        let mask_bytes: Vec<u8> = rgba.into_iter().map(|p| p.to_color().a()).collect();

        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_svg_clip_mask"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &mask_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.total_mask_bytes = self.total_mask_bytes.saturating_add(incoming_bytes);
        self.path_masks.insert(
            key,
            CachedPathMask {
                _texture: tex,
                view,
                byte_size: incoming_bytes,
                last_used_frame: self.frame_index,
            },
        );
        key
    }

    /// 按缓存键只读获取已上传的 SVG Path 遮罩视图（缺省回退至 1×1 全白遮罩）。
    #[inline]
    pub(crate) fn path_mask_view(&self, key: Option<u64>) -> &wgpu::TextureView {
        key.and_then(|k| self.path_masks.get(&k).map(|m| &m.view))
            .unwrap_or(&self.white_mask.view)
    }
}
