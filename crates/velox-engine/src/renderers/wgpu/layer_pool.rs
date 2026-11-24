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
    in_use: bool,
}

pub(crate) struct CachedPathMask {
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

const MAX_POOLED_TARGETS: usize = 32;
const MAX_CACHED_MASKS: usize = 64;

pub(crate) struct LayerTargetPool {
    format: wgpu::TextureFormat,
    targets: Vec<PooledTarget>,
    white_mask: CachedPathMask,
    path_masks: HashMap<u64, CachedPathMask>,
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
            white_mask: CachedPathMask {
                _texture: white_tex,
                view: white_view,
            },
            path_masks: HashMap::with_capacity(16),
        }
    }

    /// 帧开始时归还所有已借出的离屏目标（不销毁底层 GPU 显存）。
    #[inline]
    pub(crate) fn begin_frame(&mut self) {
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
            self.targets[idx].in_use = true;
            return idx;
        }

        // 若池已达上限，回收一个未使用的槽位重建
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
        let entry = PooledTarget {
            texture: tex,
            view,
            width: bw,
            height: bh,
            in_use: true,
        };

        if self.targets.len() >= MAX_POOLED_TARGETS {
            if let Some(evict) = self.targets.iter().position(|t| !t.in_use) {
                self.targets[evict] = entry;
                return evict;
            }
        }
        let idx = self.targets.len();
        self.targets.push(entry);
        idx
    }

    #[inline]
    pub(crate) fn release(&mut self, idx: usize) {
        if let Some(t) = self.targets.get_mut(idx) {
            t.in_use = false;
        }
    }

    #[inline]
    pub(crate) fn get(&self, idx: usize) -> &PooledTarget {
        &self.targets[idx]
    }

    #[inline]
    pub(crate) fn white_mask_view(&self) -> &wgpu::TextureView {
        &self.white_mask.view
    }

    /// 确保指定 SVG Path 抗锯齿 `R8Unorm` 裁剪遮罩已上传缓存，返回其缓存键（按内容与尺寸哈希跨帧驻留）。
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

        if self.path_masks.contains_key(&key) {
            return key;
        }
        if self.path_masks.len() >= MAX_CACHED_MASKS {
            self.path_masks.clear();
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
        self.path_masks.insert(
            key,
            CachedPathMask {
                _texture: tex,
                view,
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
