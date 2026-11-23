//! 基于 Guillotine（断头台）二维装箱的动态 `R8Unorm` 字形图集与跨帧驻留 `Rgba8Unorm` 精灵图集。

use ::wgpu;
use std::collections::HashMap;
use std::vec;
use std::vec::Vec;

use crate::display::measure_char_advance;

/// 轻量级 Guillotine 二维矩形装箱器（Best Short Side Fit + 短余轴分裂，参考 WebRender `etagere`）。
struct GuillotinePacker {
    width: u32,
    height: u32,
    free_rects: Vec<[u32; 4]>, // [x, y, w, h]
}

impl GuillotinePacker {
    fn new(width: u32, height: u32) -> Self {
        let mut free_rects = Vec::with_capacity(64);
        if width > 2 && height > 2 {
            free_rects.push([1, 1, width - 2, height - 2]);
        }
        Self {
            width,
            height,
            free_rects,
        }
    }

    fn reset(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.free_rects.clear();
        if width > 2 && height > 2 {
            self.free_rects.push([1, 1, width - 2, height - 2]);
        }
    }

    /// 分配带 1px 保护边（防采样渗色）的矩形区域；失败返回 `None`。
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let need_w = w.saturating_add(1);
        let need_h = h.saturating_add(1);
        if need_w == 0 || need_h == 0 || need_w > self.width || need_h > self.height {
            return None;
        }

        // Best Short Side Fit (BSSF) 启发式寻找碎片最小的空闲矩形
        let mut best_idx: Option<usize> = None;
        let mut best_short = u32::MAX;
        let mut best_long = u32::MAX;

        for (i, &[_, _, fw, fh]) in self.free_rects.iter().enumerate() {
            if fw >= need_w && fh >= need_h {
                let rem_w = fw - need_w;
                let rem_h = fh - need_h;
                let short_side = rem_w.min(rem_h);
                let long_side = rem_w.max(rem_h);
                if short_side < best_short || (short_side == best_short && long_side < best_long) {
                    best_short = short_side;
                    best_long = long_side;
                    best_idx = Some(i);
                    if short_side == 0 && long_side == 0 {
                        break;
                    }
                }
            }
        }

        let idx = best_idx?;
        let [fx, fy, fw, fh] = self.free_rects.swap_remove(idx);
        let rem_w = fw - need_w;
        let rem_h = fh - need_h;

        // Shorter Leftover Axis Split：沿剩余较短轴切分以尽量保留一整块大矩形
        if rem_w < rem_h {
            if rem_w > 0 {
                self.free_rects.push([fx + need_w, fy, rem_w, need_h]);
            }
            if rem_h > 0 {
                self.free_rects.push([fx, fy + need_h, fw, rem_h]);
            }
        } else {
            if rem_h > 0 {
                self.free_rects.push([fx, fy + need_h, need_w, rem_h]);
            }
            if rem_w > 0 {
                self.free_rects.push([fx + need_w, fy, rem_w, fh]);
            }
        }

        Some((fx, fy))
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GlyphAtlasEntry {
    pub(crate) uv: [f32; 4],
    pub(crate) width: i16,
    pub(crate) height: i16,
    pub(crate) xmin: i16,
    pub(crate) ymin: i16,
    #[allow(dead_code)]
    pub(crate) advance: i16,
}

/// 紧凑动态 `R8Unorm` 字形图集（初始 `512×512 = 256 KB`，Guillotine 二维装箱，零 CPU 影子副本，满格倍增至 `2048×2048`）。
pub(crate) struct GlyphAtlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    height: u32,
    packer: GuillotinePacker,
    entries: HashMap<(char, u16), GlyphAtlasEntry>,
}

pub(crate) const GLYPH_ATLAS_INIT_SIZE: u32 = 512;
const GLYPH_ATLAS_MAX_SIZE: u32 = 2048;

impl GlyphAtlas {
    pub(crate) fn new(device: &wgpu::Device, size: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_glyph_atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width: size,
            height: size,
            packer: GuillotinePacker::new(size, size),
            entries: HashMap::with_capacity(256),
        }
    }

    pub(crate) fn reset_or_grow(&mut self, device: &wgpu::Device) -> bool {
        let next_size = (self.width * 2).min(GLYPH_ATLAS_MAX_SIZE);
        self.entries.clear();
        if next_size > self.width {
            *self = Self::new(device, next_size);
            true
        } else {
            self.packer.reset(self.width, self.height);
            false
        }
    }

    /// 查找或光栅化并上传字形（按 0.25px 量化字号 `font_size_q4` 精确光栅化）；若图集已满返回 `None` 触发扩容并重编本帧。
    pub(crate) fn get_or_insert(
        &mut self,
        queue: &wgpu::Queue,
        ch: char,
        font_size_q4: u16,
        use_ttf: bool,
    ) -> Option<GlyphAtlasEntry> {
        let q4 = font_size_q4.clamp(32, 1020);
        if let Some(&entry) = self.entries.get(&(ch, q4)) {
            return Some(entry);
        }

        let font_size_u8 = ((q4 + 2) >> 2).clamp(8, 255) as u8;
        if use_ttf {
            let Some(g) = crate::display::ttf_atlas::get_cached_glyph_q4(ch, q4) else {
                let empty = GlyphAtlasEntry {
                    uv: [0.0; 4],
                    width: 0,
                    height: 0,
                    xmin: 0,
                    ymin: 0,
                    advance: measure_char_advance(ch, font_size_u8),
                };
                self.entries.insert((ch, q4), empty);
                return Some(empty);
            };
            if g.width == 0 || g.height == 0 {
                let empty = GlyphAtlasEntry {
                    uv: [0.0; 4],
                    width: 0,
                    height: 0,
                    xmin: g.xmin as i16,
                    ymin: g.ymin as i16,
                    advance: g.advance_width,
                };
                self.entries.insert((ch, q4), empty);
                return Some(empty);
            }
            let gw = g.width as u32;
            let gh = g.height as u32;
            let (ax, ay) = self.packer.alloc(gw, gh)?;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &g.bitmap,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(gw),
                    rows_per_image: Some(gh),
                },
                wgpu::Extent3d {
                    width: gw,
                    height: gh,
                    depth_or_array_layers: 1,
                },
            );
            let inv_w = 1.0 / (self.width as f32);
            let inv_h = 1.0 / (self.height as f32);
            let entry = GlyphAtlasEntry {
                uv: [
                    (ax as f32) * inv_w,
                    (ay as f32) * inv_h,
                    ((ax + gw) as f32) * inv_w,
                    ((ay + gh) as f32) * inv_h,
                ],
                width: g.width as i16,
                height: g.height as i16,
                xmin: g.xmin as i16,
                ymin: g.ymin as i16,
                advance: g.advance_width,
            };
            self.entries.insert((ch, q4), entry);
            Some(entry)
        } else {
            // 无系统 TTF 字体时回退到 Velox 内建 SDF/点阵字形覆盖率采样
            let adv = measure_char_advance(ch, font_size_u8).max(1);
            let char_h = (font_size_u8 as i16).max(8);
            let gw = adv as u32;
            let gh = char_h as u32;
            let mut bitmap = vec![0u8; (gw * gh) as usize];
            let mut non_empty = false;
            for r in 0..char_h {
                for c in 0..adv {
                    let cov = crate::display::sample_glyph_coverage(ch, c, r, adv, char_h);
                    if cov > 0 {
                        bitmap[(r as usize) * (gw as usize) + (c as usize)] = cov;
                        non_empty = true;
                    }
                }
            }
            if !non_empty {
                let empty = GlyphAtlasEntry {
                    uv: [0.0; 4],
                    width: 0,
                    height: 0,
                    xmin: 0,
                    ymin: 0,
                    advance: adv,
                };
                self.entries.insert((ch, q4), empty);
                return Some(empty);
            }
            let (ax, ay) = self.packer.alloc(gw, gh)?;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &bitmap,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(gw),
                    rows_per_image: Some(gh),
                },
                wgpu::Extent3d {
                    width: gw,
                    height: gh,
                    depth_or_array_layers: 1,
                },
            );
            let inv_w = 1.0 / (self.width as f32);
            let inv_h = 1.0 / (self.height as f32);
            let entry = GlyphAtlasEntry {
                uv: [
                    (ax as f32) * inv_w,
                    (ay as f32) * inv_h,
                    ((ax + gw) as f32) * inv_w,
                    ((ay + gh) as f32) * inv_h,
                ],
                width: adv,
                height: char_h,
                xmin: 0,
                ymin: 0,
                advance: adv,
            };
            self.entries.insert((ch, q4), entry);
            Some(entry)
        }
    }
}

/// 精灵缓存键（结合尺寸与内容哈希，实现跨帧驻留免重复光栅与上传）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct SpriteCacheKey {
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) hash: u64,
}

/// 惰性动态 `Rgba8Unorm` 精灵/Shader 画布图集（无精灵时仅 `1×1 = 4 B` 占位；
/// 使用 Guillotine 装箱 + 跨帧驻留缓存，静态图标与过程化画布命中后零上传）。
pub(crate) struct SpriteAtlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    height: u32,
    packer: GuillotinePacker,
    entries: HashMap<SpriteCacheKey, [f32; 4]>,
    pub(crate) cache_hits: u64,
}

const SPRITE_ATLAS_MIN_ACTIVE_SIZE: u32 = 512;
const SPRITE_ATLAS_MAX_SIZE: u32 = 2048;

impl SpriteAtlas {
    pub(crate) fn new(device: &wgpu::Device, size: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_sprite_atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width: size,
            height: size,
            packer: GuillotinePacker::new(size, size),
            entries: HashMap::with_capacity(64),
            cache_hits: 0,
        }
    }

    #[inline]
    pub(crate) fn lookup(&mut self, key: SpriteCacheKey) -> Option<[f32; 4]> {
        let uv = self.entries.get(&key).copied()?;
        self.cache_hits = self.cache_hits.wrapping_add(1);
        Some(uv)
    }

    /// 扩容或清空已满的精灵图集；返回 `(texture_recreated, can_retry)`。
    pub(crate) fn reset_or_grow(
        &mut self,
        device: &wgpu::Device,
        need_w: u32,
        need_h: u32,
    ) -> (bool, bool) {
        let min_need = need_w.max(need_h).saturating_add(4);
        if min_need > SPRITE_ATLAS_MAX_SIZE {
            return (false, false);
        }
        let had_entries = !self.entries.is_empty();
        self.entries.clear();
        let mut next = self.width.max(SPRITE_ATLAS_MIN_ACTIVE_SIZE);
        if self.width >= SPRITE_ATLAS_MIN_ACTIVE_SIZE {
            next = (self.width * 2).max(min_need.next_power_of_two());
        } else {
            next = next.max(min_need.next_power_of_two());
        }
        next = next.min(SPRITE_ATLAS_MAX_SIZE);
        if next > self.width {
            let hits = self.cache_hits;
            *self = Self::new(device, next);
            self.cache_hits = hits;
            (true, true)
        } else if had_entries {
            // 图集已达最大尺寸，清空旧驻留条目以便本帧重试打包
            self.packer.reset(self.width, self.height);
            (false, true)
        } else {
            (false, false)
        }
    }

    pub(crate) fn insert_and_upload(
        &mut self,
        queue: &wgpu::Queue,
        key: SpriteCacheKey,
        rgba_bytes: &[u8],
    ) -> Option<[f32; 4]> {
        if self.width < 4 || key.w + 2 > self.width || key.h + 2 > self.height {
            return None;
        }
        let (ax, ay) = self.packer.alloc(key.w, key.h)?;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(key.w * 4),
                rows_per_image: Some(key.h),
            },
            wgpu::Extent3d {
                width: key.w,
                height: key.h,
                depth_or_array_layers: 1,
            },
        );
        let inv_w = 1.0 / (self.width as f32);
        let inv_h = 1.0 / (self.height as f32);
        let uv = [
            (ax as f32) * inv_w,
            (ay as f32) * inv_h,
            ((ax + key.w) as f32) * inv_w,
            ((ay + key.h) as f32) * inv_h,
        ];
        self.entries.insert(key, uv);
        Some(uv)
    }
}
