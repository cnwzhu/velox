//! GPU 1D 渐变查找表图集（`GradientRampAtlas`，`256 × H` `Rgba8Unorm`）。
//!
//! 任意数量色标与 `Srgb` / `LinearRgb` / `Oklab` 色彩空间均在 CPU 端按内容哈希预烘焙为 256 阶 1D LUT，
//! 着色器内只需单次线性纹理采样，零循环分支且不打断单 Draw Call 合批。

use ::wgpu;
use std::collections::HashMap;

use crate::paint::{bake_gradient_lut, gradient_ramp_hash, ColorStop, GradientSpace};

pub(crate) const RAMP_WIDTH: u32 = 256;
const RAMP_INIT_ROWS: u32 = 64;
const RAMP_MAX_ROWS: u32 = 512;

pub(crate) struct GradientRampAtlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) rows: u32,
    next_row: u32,
    entries: HashMap<u64, u32>,
}

impl GradientRampAtlas {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self::with_rows(device, RAMP_INIT_ROWS)
    }

    fn with_rows(device: &wgpu::Device, rows: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("velox_gradient_ramp_atlas"),
            size: wgpu::Extent3d {
                width: RAMP_WIDTH,
                height: rows,
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
            rows,
            next_row: 0,
            entries: HashMap::with_capacity(rows as usize),
        }
    }

    pub(crate) fn reset_or_grow(&mut self, device: &wgpu::Device) -> bool {
        let next = (self.rows * 2).min(RAMP_MAX_ROWS);
        self.entries.clear();
        self.next_row = 0;
        if next > self.rows {
            *self = Self::with_rows(device, next);
            true
        } else {
            false
        }
    }

    /// 查找或烘焙上传一条 256 阶渐变色标行，返回行中心归一化 V 坐标；表满时返回 `None` 触发扩容重试。
    pub(crate) fn get_or_insert(
        &mut self,
        queue: &wgpu::Queue,
        stops: &[ColorStop],
        space: GradientSpace,
        opacity: u8,
    ) -> Option<f32> {
        let key = gradient_ramp_hash(stops, space, opacity);
        if let Some(&row) = self.entries.get(&key) {
            return Some(((row as f32) + 0.5) / (self.rows as f32));
        }
        if self.next_row >= self.rows {
            return None;
        }
        let row = self.next_row;
        self.next_row += 1;

        let mut lut = [[0u8; 4]; RAMP_WIDTH as usize];
        bake_gradient_lut(stops, space, opacity, &mut lut);
        let bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(lut.as_ptr() as *const u8, (RAMP_WIDTH as usize) * 4)
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(RAMP_WIDTH * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: RAMP_WIDTH,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.entries.insert(key, row);
        Some(((row as f32) + 0.5) / (self.rows as f32))
    }
}
