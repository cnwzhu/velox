//! `WgpuRenderer` 核心 2D SDF 渲染管线（单 RenderPass / 单 Draw Call 合批编码、分数 DPI Pixel Snapping 与提交）。

use ::wgpu;
use std::collections::HashMap;
use std::string::String;
use std::sync::Arc;
use std::vec::Vec;

use crate::display::{measure_char_advance, measure_char_kerning, Rgba8888};
use crate::element::SpriteSource;
use crate::motion::{DrawOp, DrawPayload};
use crate::renderers::software::{
    align_offset_x, compute_scaled_text_box, compute_unoccluded_clip, rasterize_sprite_source,
    split_text_lines,
};
use crate::solver::Region;
use crate::style::Color;

use super::atlas::{GlyphAtlas, SpriteAtlas, SpriteCacheKey, GLYPH_ATLAS_INIT_SIZE};
use super::effects_pass::{
    clamp_rect_u32, EffectUniform, GpuEffectsEngine, GpuPassStep, EFFECT_KIND_BACKDROP,
    EFFECT_KIND_BLIT, EFFECT_KIND_COMPOSITE, EFFECT_KIND_EXTERNAL_TEX, EFFECT_KIND_SHADOW_EXTRACT,
};
use super::ramp::GradientRampAtlas;
use super::{
    color_to_f32, make_arc_primitive, make_quad_primitive, make_shadow_primitive,
    make_styled_quad_primitive, pod_as_bytes, snap_bounds, snap_clip, GpuPrimitive,
    ViewportUniform, IDENTITY_XFORM, KIND_GLYPH, KIND_SPRITE,
};
use crate::paint::{BlendMode, ExternalTextureId, LayerSpec, Paint, StyledQuad};
use crate::renderers::software::{ops_have_layers_or_blend, snap_bounds_region_scaled, snap_clip_region_scaled};

const SDF_WGSL: &str = include_str!("shader.wgsl");
const INITIAL_INSTANCE_CAPACITY: usize = 512;
const MAX_TEXT_LAYOUT_CACHE_ENTRIES: usize = 512;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct TextLayoutKey {
    text_hash: u64,
    text_len: u32,
    logical_font_size: u8,
    font_size_q4: u16,
    max_width: i16,
    text_flags: u8,
    use_ttf: bool,
}

#[derive(Copy, Clone)]
struct ShapedGlyphPos {
    ch: char,
    rel_x: i16,
    rel_y: i16,
}

struct CachedTextLayout {
    text: String,
    total_h: i16,
    glyphs: Vec<ShapedGlyphPos>,
}

#[inline]
fn hash_layout_text(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in text.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[inline]
fn mix_u64(h: &mut u64, v: u64) {
    *h ^= v;
    *h = h.wrapping_mul(0x0100_0000_01b3);
}

#[inline]
fn mix_region(h: &mut u64, r: Region) {
    let packed = ((r.x as u16 as u64) << 48)
        | ((r.y as u16 as u64) << 32)
        | ((r.w as u16 as u64) << 16)
        | (r.h as u16 as u64);
    mix_u64(h, packed);
}

/// 判断逻辑脏矩形切片映射到物理像素并外扩 1px 保护边后，是否与毛玻璃外扩采样区 `[sx, sy, sw, sh]` 相交。
fn dirty_intersects_phys_rect(
    dirty_regions: &[Region],
    sx: u32,
    sy: u32,
    sw: u32,
    sh: u32,
    sf: f32,
) -> bool {
    let bx0 = sx as i64;
    let by0 = sy as i64;
    let bx1 = bx0 + sw as i64;
    let by1 = by0 + sh as i64;
    for &d in dirty_regions {
        if d.is_empty() {
            continue;
        }
        let dx0 = ((d.x as f32) * sf).floor() as i64 - 1;
        let dy0 = ((d.y as f32) * sf).floor() as i64 - 1;
        let dx1 = ((d.right() as f32) * sf).ceil() as i64 + 1;
        let dy1 = ((d.bottom() as f32) * sf).ceil() as i64 + 1;
        if dx0 < bx1 && dx1 > bx0 && dy0 < by1 && dy1 > by0 {
            return true;
        }
    }
    false
}

/// 计算当前毛玻璃之前、落在外扩采样框 `sample_logical` 内的所有背景图元内容特征哈希。
///
/// 若下方存在外部实时视频/雷达纹理（内容可能脱离 `DrawOp` 变更），返回 `None` 禁止跨帧复用以保证实时性。
fn compute_backdrop_underlay_hash(
    ops_before: &[DrawOp<'_>],
    sample_logical: Region,
    clear_color: wgpu::Color,
) -> Option<u64> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    mix_u64(&mut h, clear_color.r.to_bits());
    mix_u64(&mut h, clear_color.g.to_bits());
    mix_u64(&mut h, clear_color.b.to_bits());
    mix_u64(&mut h, clear_color.a.to_bits());

    for op in ops_before {
        let always_affect = matches!(
            op.payload,
            DrawPayload::PushLayer(_)
                | DrawPayload::PopLayer
                | DrawPayload::ExternalTexture(_)
                | DrawPayload::StyledQuad(_)
        );
        if !always_affect && op.bounds.intersect(sample_logical).is_empty() {
            continue;
        }
        if matches!(op.payload, DrawPayload::ExternalTexture(_))
            && !op.bounds.intersect(sample_logical).is_empty()
        {
            return None;
        }
        if let DrawPayload::StyledQuad(sq) = op.payload {
            if sq.transform.is_none() && op.bounds.intersect(sample_logical).is_empty() {
                continue;
            }
        }
        let snap = op.to_snapshot();
        mix_region(&mut h, snap.effective_bounds);
        mix_region(&mut h, op.clip);
        mix_u64(&mut h, snap.payload_hash as u64);
        match op.payload {
            DrawPayload::SceneCanvas(sc) => {
                mix_u64(&mut h, sc.content_hash() as u64);
            }
            DrawPayload::Vector(vs) => {
                let mut vh = 0x5645_4354u32;
                vs.mix_hash(&mut vh);
                mix_u64(&mut h, vh as u64);
            }
            DrawPayload::DecodedImage(img) => {
                mix_u64(&mut h, img.id.0 as u64);
                mix_u64(&mut h, crate::image_pipeline::image_revision(img.id).unwrap_or(0));
            }
            _ => {}
        }
    }
    Some(h)
}

/// 256 字节对齐的动态 `ViewportUniform` 槽位（供多 Pass 离屏图层共享单个 `viewport_buf`）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct AlignedViewportSlot {
    uniform: ViewportUniform,
    _pad: [u8; 256 - core::mem::size_of::<ViewportUniform>()],
}

impl Default for AlignedViewportSlot {
    fn default() -> Self {
        Self {
            uniform: ViewportUniform::default(),
            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
        }
    }
}

const _: () = assert!(core::mem::size_of::<AlignedViewportSlot>() == 256);
const INIT_VIEWPORT_SLOTS: usize = 8;

/// 编码重试原因（任一图集满格触发扩容重编）。
struct AtlasGrowNeed {
    glyph_full: bool,
    sprite_need: Option<(u32, u32)>,
    ramp_full: bool,
}

/// 外部零拷贝 GPU 纹理注册条目（由外部渲染引擎共享 `wgpu::TextureView` 与可选采样器）。
pub(crate) struct ExternalTextureEntry {
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: Option<wgpu::Sampler>,
    pub(crate) generation: u64,
}

/// `WgpuRenderer` GPU 与伴随 CPU 缓存字节统计快照（用于四层内存账本剖析）。
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct WgpuMemoryStats {
    pub glyph_atlas_size: u32,
    pub glyph_atlas_bytes: usize,
    pub glyph_atlas_entries: usize,
    pub glyph_atlas_occupancy: f32,
    pub sprite_atlas_size: u32,
    pub sprite_atlas_bytes: usize,
    pub sprite_atlas_entries: usize,
    pub sprite_atlas_occupancy: f32,
    pub ramp_atlas_bytes: usize,
    pub instance_buf_bytes: usize,
    pub uploaded_primitives_cpu_bytes: usize,
    pub layer_pool_targets: usize,
    pub layer_pool_target_bytes: usize,
    pub layer_pool_masks: usize,
    pub layer_pool_mask_bytes: usize,
    pub canvas_scenes: usize,
    pub canvas_gpu_bytes: usize,
    pub slug_cached_glyphs: usize,
    pub slug_gpu_bytes: usize,
    pub slug_last_drawn_glyphs: usize,
    pub text_layout_cache_entries: usize,
    pub backdrop_blur_cache_hits: u64,
    pub backdrop_blur_cache_misses: u64,
}

impl WgpuMemoryStats {
    /// 估算该渲染器持有的全部 GPU 纹理与缓冲总字节。
    #[inline]
    pub const fn total_gpu_bytes(&self) -> usize {
        self.glyph_atlas_bytes
            + self.sprite_atlas_bytes
            + self.ramp_atlas_bytes
            + self.instance_buf_bytes
            + self.layer_pool_target_bytes
            + self.layer_pool_mask_bytes
            + self.canvas_gpu_bytes
            + self.slug_gpu_bytes
    }
}

/// 默认全局 UI `text()` 自动切换至 Slug 2.0 GPU 曲线字形的物理像素字号阈值。
///
/// 小于该阈值的常规小字号 UI 正文继续走 `512×512` `GlyphAtlas`（兼顾片元吞吐与像素网格锐度）；
/// 大于等于该阈值的大字号标题与缩放文本自动走零图集 Slug 2.0 曲线管线，避免撑爆字形图集与触发重光栅化。
pub const DEFAULT_SLUG_TEXT_THRESHOLD_PX: f32 = 28.0;

/// 自研轻量级 `wgpu` 2D SDF 渲染管线核心（绑定任意目标 `TextureView` 即可渲染）。
pub struct WgpuRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    viewport_buf: wgpu::Buffer,
    viewport_capacity: usize,
    viewport_slots: Vec<AlignedViewportSlot>,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    glyph_atlas: GlyphAtlas,
    sprite_atlas: SpriteAtlas,
    ramp_atlas: GradientRampAtlas,
    effects: Option<GpuEffectsEngine>,
    canvas_engine: Option<super::canvas_pipeline::CanvasGpuEngine>,
    /// 面向全局 UI `text()` 的懒初始化 Slug 2.0 GPU 曲线字形子引擎（默认物理字号 `>= 28px` 启用）。
    ui_slug_engine: Option<super::slug_glyph::SlugTextEngine>,
    /// 全局 UI `text()` 路由至 Slug 2.0 的物理字号阈值（像素）。
    slug_text_threshold_px: f32,
    /// 单 Pass 路径的 Z-order 交错分段：`(prim_start, prim_end, slug_start, slug_end)`（无 Slug 文本时为空，零开销）。
    ui_slug_segments: Vec<(u32, u32, u32, u32)>,
    pipeline_cache: Option<Arc<super::pso_cache::DiskPipelineCache>>,
    bg_effects_rx: Option<std::sync::mpsc::Receiver<GpuEffectsEngine>>,
    bg_canvas_rx: Option<std::sync::mpsc::Receiver<super::canvas_pipeline::CanvasGpuEngine>>,
    external_textures: HashMap<ExternalTextureId, ExternalTextureEntry>,
    next_external_id: u32,
    next_external_gen: u64,
    target_format: wgpu::TextureFormat,
    primitives: Vec<GpuPrimitive>,
    /// 上一帧已上传到 GPU `instance_buf` 的实例镜像，用于增量局部区间 `queue.write_buffer`。
    uploaded_primitives: Vec<GpuPrimitive>,
    instance_buf_dirty_all: bool,
    last_uploaded_instance_bytes: usize,
    sprite_scratch: Vec<Rgba8888>,
    #[cfg(not(target_endian = "little"))]
    sprite_bytes_scratch: Vec<u8>,
    text_layout_cache: HashMap<TextLayoutKey, Arc<CachedTextLayout>>,
    text_layout_hits: u64,
    target_is_srgb: bool,
}

impl WgpuRenderer {
    /// 基于已有 `wgpu::Device` 与目标表面像素格式创建 2D SDF 渲染管线。
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        Self::new_with_cache(device, target_format, None)
    }

    /// 基于已有 `wgpu::Device`、目标表面像素格式与可选持久化磁盘 `PipelineCache` 创建 2D SDF 渲染管线。
    pub(crate) fn new_with_cache(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        pipeline_cache: Option<Arc<super::pso_cache::DiskPipelineCache>>,
    ) -> Self {
        let trace = std::env::var_os("VELOX_STARTUP_TRACE").is_some();
        let t0 = std::time::Instant::now();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("velox_wgpu_2d_shader"),
            source: wgpu::ShaderSource::Wgsl(SDF_WGSL.into()),
        });
        let t_shader = t0.elapsed();

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velox_wgpu_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            core::mem::size_of::<ViewportUniform>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("velox_wgpu_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velox_wgpu_2d_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipeline_cache.as_deref().map(|c| c.wgpu_cache()),
        });
        let t_pso = t0.elapsed();
        if let Some(c) = &pipeline_cache {
            c.persist();
        }
        if trace {
            std::println!(
                "[VELOX_STARTUP_TRACE]   pipeline detail: create_shader_module={:.1}ms, create_render_pipeline=+{:.1}ms, pso_cache_hit={}",
                t_shader.as_secs_f64() * 1000.0,
                (t_pso - t_shader).as_secs_f64() * 1000.0,
                pipeline_cache.as_ref().is_some_and(|c| c.loaded_from_disk()),
            );
        }

        let viewport_buf = Self::create_viewport_buffer(device, INIT_VIEWPORT_SLOTS);
        let instance_buf = Self::create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("velox_atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("velox_linear_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let glyph_atlas = GlyphAtlas::new(device, GLYPH_ATLAS_INIT_SIZE);
        let sprite_atlas = SpriteAtlas::new(device, 1);
        let ramp_atlas = GradientRampAtlas::new(device);
        let bind_group = Self::create_bind_group(
            device,
            &bind_group_layout,
            &viewport_buf,
            &instance_buf,
            &glyph_atlas.view,
            &sprite_atlas.view,
            &sampler,
            &ramp_atlas.view,
            &linear_sampler,
        );

        Self {
            pipeline,
            bind_group_layout,
            bind_group,
            viewport_buf,
            viewport_capacity: INIT_VIEWPORT_SLOTS,
            viewport_slots: Vec::with_capacity(INIT_VIEWPORT_SLOTS),
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            sampler,
            linear_sampler,
            glyph_atlas,
            sprite_atlas,
            ramp_atlas,
            effects: None,
            canvas_engine: None,
            ui_slug_engine: None,
            slug_text_threshold_px: DEFAULT_SLUG_TEXT_THRESHOLD_PX,
            ui_slug_segments: Vec::new(),
            pipeline_cache,
            bg_effects_rx: None,
            bg_canvas_rx: None,
            external_textures: HashMap::new(),
            next_external_id: 1,
            next_external_gen: 1,
            target_format,
            primitives: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            uploaded_primitives: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            instance_buf_dirty_all: true,
            last_uploaded_instance_bytes: 0,
            sprite_scratch: Vec::new(),
            #[cfg(not(target_endian = "little"))]
            sprite_bytes_scratch: Vec::new(),
            text_layout_cache: HashMap::with_capacity(64),
            text_layout_hits: 0,
            target_is_srgb: target_format.is_srgb(),
        }
    }

    fn create_viewport_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_wgpu_viewport"),
            size: (capacity.max(1) * core::mem::size_of::<AlignedViewportSlot>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_wgpu_instances"),
            size: (capacity.max(1) * core::mem::size_of::<GpuPrimitive>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        viewport_buf: &wgpu::Buffer,
        instance_buf: &wgpu::Buffer,
        glyph_view: &wgpu::TextureView,
        sprite_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        ramp_view: &wgpu::TextureView,
        linear_sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("velox_wgpu_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: viewport_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(core::mem::size_of::<ViewportUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instance_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(glyph_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(sprite_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(ramp_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(linear_sampler),
                },
            ],
        })
    }

    /// 返回最近一帧合批提交的 [`GpuPrimitive`] 实例数量。
    #[inline]
    pub fn last_primitive_count(&self) -> usize {
        self.primitives.len()
    }

    /// 返回当前字形图集与精灵图集的物理像素边长 `(glyph_size, sprite_size)`。
    #[inline]
    pub const fn atlas_sizes(&self) -> (u32, u32) {
        (self.glyph_atlas.width, self.sprite_atlas.width)
    }

    /// 返回静态精灵跨帧驻留缓存累计命中次数。
    #[inline]
    pub const fn sprite_cache_hits(&self) -> u64 {
        self.sprite_atlas.cache_hits
    }

    /// 返回跨帧文本行排版缓存（`TextLayoutCache`）累计命中次数。
    #[inline]
    pub const fn text_layout_cache_hits(&self) -> u64 {
        self.text_layout_hits
    }

    /// 返回跨帧文本行排版缓存当前驻留条目数。
    #[inline]
    pub fn text_layout_cache_len(&self) -> usize {
        self.text_layout_cache.len()
    }

    /// 注册一张外部零拷贝 GPU 纹理（如海图、雷达、视频解码器或 3D 视口产出的 `wgpu::TextureView`），
    /// 返回可供 `div().external_texture(id)` 或 `SurfaceCanvas` 引用的句柄 ID。
    pub fn register_external_texture(
        &mut self,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) -> ExternalTextureId {
        let mut id = self.next_external_id;
        while id == 0 || self.external_textures.contains_key(&ExternalTextureId(id)) {
            id = id.wrapping_add(1);
        }
        self.next_external_id = id.wrapping_add(1).max(1);
        let generation = self.next_external_gen;
        self.next_external_gen = self.next_external_gen.wrapping_add(1).max(1);
        let tex_id = ExternalTextureId(id);
        self.external_textures.insert(
            tex_id,
            ExternalTextureEntry {
                view,
                sampler,
                generation,
            },
        );
        tex_id
    }

    /// 按显式 ID 注册或替换外部零拷贝 GPU 纹理（便于固定槽位号与跨语言 FFI 绑定）。
    pub fn register_external_texture_with_id(
        &mut self,
        id: ExternalTextureId,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) {
        let generation = self.next_external_gen;
        self.next_external_gen = self.next_external_gen.wrapping_add(1).max(1);
        self.external_textures.insert(
            id,
            ExternalTextureEntry {
                view,
                sampler,
                generation,
            },
        );
    }

    /// 原地更新已注册外部纹理的 `wgpu::TextureView`（如外部引擎 Resize 或双缓冲翻页时调用）。
    pub fn update_external_texture(&mut self, id: ExternalTextureId, view: wgpu::TextureView) -> bool {
        if let Some(entry) = self.external_textures.get_mut(&id) {
            entry.view = view;
            let generation = self.next_external_gen;
            self.next_external_gen = self.next_external_gen.wrapping_add(1).max(1);
            entry.generation = generation;
            true
        } else {
            false
        }
    }

    /// 注销外部零拷贝纹理槽位并释放 `TextureView` 引用。
    pub fn unregister_external_texture(&mut self, id: ExternalTextureId) -> bool {
        self.external_textures.remove(&id).is_some()
    }

    /// 返回当前已注册的外部零拷贝 GPU 纹理数量。
    #[inline]
    pub fn external_texture_count(&self) -> usize {
        self.external_textures.len()
    }

    /// 返回当前离屏图层纹理池占用的估算 GPU 显存字节数（无特效子引擎时为 `0`）。
    #[inline]
    pub fn layer_pool_bytes(&self) -> usize {
        self.effects.as_ref().map(|fx| fx.pool.target_bytes()).unwrap_or(0)
    }

    /// 返回当前离屏图层纹理池中驻留的纹理数量。
    #[inline]
    pub fn layer_pool_target_count(&self) -> usize {
        self.effects.as_ref().map(|fx| fx.pool.target_count()).unwrap_or(0)
    }

    /// 返回当前 SVG Path 遮罩缓存占用的估算 GPU 显存字节数。
    #[inline]
    pub fn layer_mask_cache_bytes(&self) -> usize {
        self.effects.as_ref().map(|fx| fx.pool.mask_bytes()).unwrap_or(0)
    }

    /// 返回当前 SVG Path 遮罩缓存驻留的条目数量。
    #[inline]
    pub fn layer_mask_cache_count(&self) -> usize {
        self.effects.as_ref().map(|fx| fx.pool.mask_count()).unwrap_or(0)
    }

    /// 返回内部已加载的持久化磁盘 PSO 缓存是否命中（仅在支持 `Features::PIPELINE_CACHE` 的后端上为 `true`）。
    #[inline]
    pub fn pso_cache_loaded_from_disk(&self) -> bool {
        self.pipeline_cache
            .as_ref()
            .is_some_and(|c| c.loaded_from_disk())
    }

    /// 在低优先级后台线程异步预编译 `GpuEffectsEngine`（Dual Kawase / 混合模式）与 `CanvasGpuEngine` PSO，
    /// 首屏不阻塞且消除首次切换至特效或交互画布页时的主线程同步编译卡顿。
    pub fn spawn_background_pipeline_warmup(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let need_effects = self.effects.is_none() && self.bg_effects_rx.is_none();
        let need_canvas = self.canvas_engine.is_none() && self.bg_canvas_rx.is_none();
        if !need_effects && !need_canvas {
            return;
        }
        struct WarmupPayload {
            device: wgpu::Device,
            queue: wgpu::Queue,
            fmt: wgpu::TextureFormat,
            cache: Option<Arc<super::pso_cache::DiskPipelineCache>>,
            fx_tx: Option<std::sync::mpsc::Sender<GpuEffectsEngine>>,
            cv_tx: Option<std::sync::mpsc::Sender<super::canvas_pipeline::CanvasGpuEngine>>,
        }
        // SAFETY: `wgpu::Device`/`Queue` 底层均为 `Arc` 跨线程安全句柄，显式实现避免 `wgpu 30` 深层 `Hub` 递归校验警告。
        unsafe impl Send for WarmupPayload {}

        let payload = WarmupPayload {
            device: device.clone(),
            queue: queue.clone(),
            fmt: self.target_format,
            cache: self.pipeline_cache.clone(),
            fx_tx: if need_effects {
                let (tx, rx) = std::sync::mpsc::channel();
                self.bg_effects_rx = Some(rx);
                Some(tx)
            } else {
                None
            },
            cv_tx: if need_canvas {
                let (tx, rx) = std::sync::mpsc::channel();
                self.bg_canvas_rx = Some(rx);
                Some(tx)
            } else {
                None
            },
        };

        let _ = std::thread::Builder::new()
            .name(String::from("velox-pso-warmup"))
            .stack_size(512 * 1024)
            .spawn(move || {
                let payload = payload;
                let trace = std::env::var_os("VELOX_STARTUP_TRACE").is_some();
                let t0 = std::time::Instant::now();
                let raw_cache = payload.cache.as_deref().map(|c| c.wgpu_cache());
                if let Some(tx) = payload.fx_tx {
                    let fx = GpuEffectsEngine::new(&payload.device, &payload.queue, payload.fmt, raw_cache);
                    if let Some(c) = &payload.cache {
                        c.persist();
                    }
                    let _ = tx.send(fx);
                }
                let t_fx = t0.elapsed();
                if let Some(tx) = payload.cv_tx {
                    let cv = super::canvas_pipeline::CanvasGpuEngine::new(
                        &payload.device,
                        payload.fmt,
                        raw_cache,
                    );
                    if let Some(c) = &payload.cache {
                        c.persist();
                    }
                    let _ = tx.send(cv);
                }
                if trace {
                    let t_all = t0.elapsed();
                    std::println!(
                        "[VELOX_STARTUP_TRACE] async bg sub-pipeline warmup: effects={:.1}ms, canvas=+{:.1}ms, total={:.1}ms",
                        t_fx.as_secs_f64() * 1000.0,
                        (t_all - t_fx).as_secs_f64() * 1000.0,
                        t_all.as_secs_f64() * 1000.0,
                    );
                }
            });
    }

    /// 非阻塞收割已在后台线程预热完毕的特效与画布子引擎（0.0ms）。
    pub(crate) fn poll_warmed_subengines(&mut self) {
        if self.effects.is_none() {
            if let Some(rx) = &self.bg_effects_rx {
                match rx.try_recv() {
                    Ok(fx) => {
                        self.effects = Some(fx);
                        self.bg_effects_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.bg_effects_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }
        if self.canvas_engine.is_none() {
            if let Some(rx) = &self.bg_canvas_rx {
                match rx.try_recv() {
                    Ok(cv) => {
                        self.canvas_engine = Some(cv);
                        self.bg_canvas_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.bg_canvas_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }
    }

    /// 等待后台子管线预热线程完成并收割结果（主要用于测试基准验证或应用显式预热屏障）。
    pub fn wait_background_pipeline_warmup(&mut self) {
        if let Some(rx) = self.bg_effects_rx.take() {
            if let Ok(fx) = rx.recv() {
                if self.effects.is_none() {
                    self.effects = Some(fx);
                }
            }
        }
        if let Some(rx) = self.bg_canvas_rx.take() {
            if let Ok(cv) = rx.recv() {
                if self.canvas_engine.is_none() {
                    self.canvas_engine = Some(cv);
                }
            }
        }
    }

    /// 返回跨帧毛玻璃（`backdrop-filter`）模糊小纹理缓存命中与未命中统计 `(hits, misses)`。
    #[inline]
    pub fn backdrop_blur_cache_stats(&self) -> (u64, u64) {
        self.effects
            .as_ref()
            .map(|fx| fx.pool.backdrop_cache_stats())
            .unwrap_or((0, 0))
    }

    /// 配置全局 UI `text()` 自动路由至 Slug 2.0 矢量字形的物理像素字号阈值（默认 `28.0 px`）。
    ///
    /// - 默认 `28.0`：大字号标题（`>= 28px`）走零图集 Slug 2.0 曲线管线，小字号正文保留 `GlyphAtlas`；
    /// - 设为 `0.0`：全部常规 UI `text()` 走 Slug 2.0 渲染（便于全量矢量 A/B 对比）；
    /// - 设为 `f32::INFINITY`：全部回退至位图图集。
    #[inline]
    pub fn set_slug_text_threshold_px(&mut self, threshold_px: f32) {
        self.slug_text_threshold_px = threshold_px.max(0.0);
    }

    /// 查询当前全局 UI `text()` 路由至 Slug 2.0 矢量字形的物理字号阈值（像素）。
    #[inline]
    pub const fn slug_text_threshold_px(&self) -> f32 {
        self.slug_text_threshold_px
    }

    /// 返回当前 `WgpuRenderer` 各子池（字形/精灵/渐变图集、实例缓冲、离屏图层池、Canvas 场景缓冲与共享 Slug 曲线池）完整内存快照。
    pub fn memory_stats(&self) -> WgpuMemoryStats {
        let (canvas_scenes, canvas_gpu_bytes) = self
            .canvas_engine
            .as_ref()
            .map(|c| (c.scene_count(), c.total_gpu_bytes()))
            .unwrap_or((0, 0));
        let (slug_cached_glyphs, slug_gpu_bytes, slug_last_drawn_glyphs) = self
            .ui_slug_engine
            .as_ref()
            .map(|s| (s.cached_glyph_count(), s.gpu_bytes(), s.last_drawn_glyphs()))
            .unwrap_or((0, 0, 0));
        let (backdrop_blur_cache_hits, backdrop_blur_cache_misses) = self.backdrop_blur_cache_stats();
        WgpuMemoryStats {
            glyph_atlas_size: self.glyph_atlas.width,
            glyph_atlas_bytes: self.glyph_atlas.gpu_bytes(),
            glyph_atlas_entries: self.glyph_atlas.entry_count(),
            glyph_atlas_occupancy: self.glyph_atlas.occupancy_ratio(),
            sprite_atlas_size: self.sprite_atlas.width,
            sprite_atlas_bytes: self.sprite_atlas.gpu_bytes(),
            sprite_atlas_entries: self.sprite_atlas.entry_count(),
            sprite_atlas_occupancy: self.sprite_atlas.occupancy_ratio(),
            ramp_atlas_bytes: (super::ramp::RAMP_WIDTH as usize)
                .saturating_mul(self.ramp_atlas.rows as usize)
                .saturating_mul(4),
            instance_buf_bytes: self
                .instance_capacity
                .saturating_mul(core::mem::size_of::<GpuPrimitive>()),
            uploaded_primitives_cpu_bytes: self
                .uploaded_primitives
                .capacity()
                .saturating_add(self.primitives.capacity())
                .saturating_mul(core::mem::size_of::<GpuPrimitive>()),
            layer_pool_targets: self.layer_pool_target_count(),
            layer_pool_target_bytes: self.layer_pool_bytes(),
            layer_pool_masks: self.layer_mask_cache_count(),
            layer_pool_mask_bytes: self.layer_mask_cache_bytes(),
            canvas_scenes,
            canvas_gpu_bytes,
            slug_cached_glyphs,
            slug_gpu_bytes,
            slug_last_drawn_glyphs,
            text_layout_cache_entries: self.text_layout_cache.len(),
            backdrop_blur_cache_hits,
            backdrop_blur_cache_misses,
        }
    }

    /// 配置离屏图层纹理池的空闲字节预算（超出时在帧末按 LRU 立即回收）。
    pub fn set_layer_pool_idle_budget(&mut self, bytes: usize) {
        if let Some(fx) = &mut self.effects {
            if fx.pool.set_idle_target_budget_bytes(bytes) {
                fx.prune_stale_bind_groups();
            }
        }
    }

    /// 配置离屏图层纹理池的最大闲置帧数（切回普通页超过该帧数后自动回收）。
    pub fn set_layer_pool_idle_max_age_frames(&mut self, frames: u64) {
        if let Some(fx) = &mut self.effects {
            if fx.pool.set_idle_target_max_age_frames(frames) {
                fx.prune_stale_bind_groups();
            }
        }
    }

    /// 配置 SVG Path 遮罩缓存的显存字节预算。
    pub fn set_layer_mask_cache_budget(&mut self, bytes: usize) {
        if let Some(fx) = &mut self.effects {
            if fx.pool.set_mask_budget_bytes(bytes) {
                fx.prune_stale_bind_groups();
            }
        }
    }

    /// 视口尺寸缩小或变更时调用，立即回收远大于新视口的空闲离屏大纹理。
    pub fn on_viewport_resized(&mut self, phys_width: u32, phys_height: u32) {
        if let Some(fx) = &mut self.effects {
            fx.on_viewport_resized(phys_width, phys_height);
        }
    }

    /// 后台化或收到内存警告时主动回收全部空闲 GPU 离屏纹理、SVG 遮罩缓存与 Canvas 场景缓冲。
    pub fn trim_gpu_memory(&mut self) {
        if let Some(fx) = &mut self.effects {
            fx.trim_idle_all();
        }
        if let Some(canvas) = &mut self.canvas_engine {
            canvas.trim_idle_scenes();
        }
    }

    /// 返回最近一帧增量上传至 GPU `instance_buf` 的实际字节数（用于观测增量上传收益）。
    #[inline]
    pub fn last_uploaded_instance_bytes(&self) -> usize {
        self.last_uploaded_instance_bytes
    }

    /// 显式移除并释放指定 `SceneCanvas` 场景的 GPU 显存缓冲。
    #[inline]
    pub fn remove_canvas_scene(&mut self, scene_id: crate::canvas_scene::StaticSceneId) -> bool {
        self.canvas_engine
            .as_mut()
            .is_some_and(|c| c.remove_scene(scene_id))
    }

    /// 返回 `CanvasGpuEngine` 当前缓存的所有场景 GPU 显存总字节数。
    #[inline]
    pub fn canvas_cache_bytes(&self) -> usize {
        self.canvas_engine
            .as_ref()
            .map(|c| c.total_gpu_bytes())
            .unwrap_or(0)
    }

    /// 返回 `CanvasGpuEngine` 当前缓存的场景数量。
    #[inline]
    pub fn canvas_scene_count(&self) -> usize {
        self.canvas_engine
            .as_ref()
            .map(|c| c.scene_count())
            .unwrap_or(0)
    }

    /// 返回最近一次 `SceneCanvas` 动态状态增量上传的实际字节数。
    #[inline]
    pub fn canvas_last_state_upload_bytes(&self) -> usize {
        self.canvas_engine
            .as_ref()
            .map(|c| c.last_state_upload_bytes())
            .unwrap_or(0)
    }

    /// 配置 `CanvasGpuEngine` 空闲场景显存预算（超出时按 LRU 淘汰切走的旧场景）。
    #[inline]
    pub fn set_canvas_idle_budget(&mut self, bytes: usize) {
        if let Some(c) = &mut self.canvas_engine {
            c.set_idle_budget_bytes(bytes);
        }
    }

    /// 配置 `CanvasGpuEngine` 空闲场景最大存活帧数。
    #[inline]
    pub fn set_canvas_idle_max_age_frames(&mut self, frames: u64) {
        if let Some(c) = &mut self.canvas_engine {
            c.set_idle_max_age_frames(frames);
        }
    }

    /// 将当前帧 `self.primitives` 增量同步至 GPU `instance_buf`：
    /// - 新建或扩容缓冲时整块上传；
    /// - 稳态帧按 192B 实例逐项位级比对，合并相邻变更区间（间隙 <= 16 项），仅对实际改动区间局部 `queue.write_buffer`。
    fn sync_instance_buffer(&mut self, queue: &wgpu::Queue) {
        const INSTANCE_STRIDE: usize = core::mem::size_of::<GpuPrimitive>();
        const INSTANCE_MERGE_GAP: usize = 16;

        let new_len = self.primitives.len();
        if new_len == 0 {
            self.uploaded_primitives.clear();
            self.last_uploaded_instance_bytes = 0;
            return;
        }

        if self.instance_buf_dirty_all || self.uploaded_primitives.is_empty() {
            let bytes = pod_as_bytes(&self.primitives);
            queue.write_buffer(&self.instance_buf, 0, bytes);
            self.uploaded_primitives.clear();
            self.uploaded_primitives.extend_from_slice(&self.primitives);
            self.instance_buf_dirty_all = false;
            self.last_uploaded_instance_bytes = bytes.len();
            return;
        }

        let old_len = self.uploaded_primitives.len();
        let common = old_len.min(new_len);
        let new_bytes = pod_as_bytes(&self.primitives);
        let old_bytes = pod_as_bytes(&self.uploaded_primitives);

        let mut uploaded_bytes = 0usize;
        let mut run_start: Option<usize> = None;
        let mut run_end: usize = 0;

        for i in 0..new_len {
            let changed = if i < common {
                let off = i * INSTANCE_STRIDE;
                new_bytes[off..off + INSTANCE_STRIDE] != old_bytes[off..off + INSTANCE_STRIDE]
            } else {
                true
            };
            if changed {
                match run_start {
                    None => {
                        run_start = Some(i);
                        run_end = i + 1;
                    }
                    Some(rs) => {
                        if i.saturating_sub(run_end) <= INSTANCE_MERGE_GAP {
                            run_end = i + 1;
                        } else {
                            let b_start = rs * INSTANCE_STRIDE;
                            let b_end = run_end * INSTANCE_STRIDE;
                            queue.write_buffer(
                                &self.instance_buf,
                                b_start as u64,
                                &new_bytes[b_start..b_end],
                            );
                            uploaded_bytes += b_end - b_start;
                            run_start = Some(i);
                            run_end = i + 1;
                        }
                    }
                }
            }
        }
        if let Some(rs) = run_start {
            let b_start = rs * INSTANCE_STRIDE;
            let b_end = run_end * INSTANCE_STRIDE;
            queue.write_buffer(
                &self.instance_buf,
                b_start as u64,
                &new_bytes[b_start..b_end],
            );
            uploaded_bytes += b_end - b_start;
        }

        if uploaded_bytes > 0 || old_len != new_len {
            self.uploaded_primitives.clear();
            self.uploaded_primitives.extend_from_slice(&self.primitives);
        }
        self.last_uploaded_instance_bytes = uploaded_bytes;
    }

    fn rebuild_bind_group(&mut self, device: &wgpu::Device) {
        self.bind_group = Self::create_bind_group(
            device,
            &self.bind_group_layout,
            &self.viewport_buf,
            &self.instance_buf,
            &self.glyph_atlas.view,
            &self.sprite_atlas.view,
            &self.sampler,
            &self.ramp_atlas.view,
            &self.linear_sampler,
        );
    }

    #[inline]
    fn sprite_cache_hash(sprite: SpriteSource<'_>, rel_x: i16, rel_y: i16, sb_w: i16, sb_h: i16) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mix = |h: &mut u64, v: u64| {
            *h ^= v;
            *h = h.wrapping_mul(0x0100_0000_01b3);
        };
        mix(&mut h, ((rel_x as u16 as u64) << 16) | (rel_y as u16 as u64));
        mix(&mut h, ((sb_w as u16 as u64) << 16) | (sb_h as u16 as u64));
        match sprite {
            SpriteSource::Bitmap {
                bytes,
                format,
                tint,
            } => {
                mix(&mut h, 1);
                mix(&mut h, bytes.as_ptr() as usize as u64);
                mix(&mut h, bytes.len() as u64);
                mix(&mut h, format as u64);
                mix(&mut h, tint.0 as u64);
            }
            SpriteSource::Shader { state_hash, .. } => {
                mix(&mut h, 2);
                mix(&mut h, state_hash as u64);
            }
            SpriteSource::Svg {
                source,
                is_path_only,
                tint,
                stroke_width_x10,
            } => {
                mix(&mut h, 3);
                mix(&mut h, if is_path_only { 1 } else { 0 });
                mix(&mut h, stroke_width_x10 as u64);
                mix(&mut h, tint.0 as u64);
                let s = source.as_str().as_bytes();
                mix(&mut h, s.len() as u64);
                for &b in s {
                    mix(&mut h, b as u64);
                }
            }
            SpriteSource::Arc { .. } => {}
        }
        h
    }

    /// 将单条非图层 `DrawOp` 编码追加到 `self.primitives`（或大字号文本自动路由至 `ui_slug_engine`），
    /// 并将所有屏幕坐标减去 `(origin_x, origin_y)`。
    #[allow(clippy::too_many_arguments)]
    fn encode_single_op(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        op: &DrawOp<'_>,
        eff_clip: Region,
        viewport: Region,
        sf: f32,
        use_ttf: bool,
        origin_x: f32,
        origin_y: f32,
        target_w: u32,
        target_h: u32,
    ) -> Result<(), AtlasGrowNeed> {
        let mut sb_f = snap_bounds(op.bounds, sf);
        if op.subpx_q4 != 0 {
            let (dx_q4, dy_q4) = crate::motion::unpack_subpx_q4(op.subpx_q4);
            sb_f[0] += (dx_q4 as f32) * (sf * 0.0625);
            sb_f[1] += (dy_q4 as f32) * (sf * 0.0625);
        }
        let clip_region = if op.clip_radius > 0 { op.clip } else { eff_clip };
        let mut sc_f = snap_clip(clip_region, sf);
        if (op.bounds.w == 1 && eff_clip.w == 1) || (op.bounds.h == 1 && eff_clip.h == 1) {
            let vp_clip_f = snap_clip(viewport, sf);
            if op.bounds.w == 1 && eff_clip.w == 1 {
                sc_f[2] = sc_f[2].max(sb_f[0] + sb_f[2]).min(vp_clip_f[2]);
            }
            if op.bounds.h == 1 && eff_clip.h == 1 {
                sc_f[3] = sc_f[3].max(sb_f[1] + sb_f[3]).min(vp_clip_f[3]);
            }
        }
        let clip_radius_px = (op.clip_radius as f32) * sf;
        let shifted_sb = [sb_f[0] - origin_x, sb_f[1] - origin_y, sb_f[2], sb_f[3]];
        let shifted_sc = [
            sc_f[0] - origin_x,
            sc_f[1] - origin_y,
            sc_f[2] - origin_x,
            sc_f[3] - origin_y,
        ];

        match op.payload {
            DrawPayload::Quad {
                bg,
                bg_grad,
                border_color,
                rounded,
                border_width,
            } => {
                let raw_bw = border_width & 0x7F;
                let bw = if raw_bw == 0 {
                    0.0
                } else {
                    ((raw_bw as f32) * sf).round().max(1.0)
                };
                let horiz = (border_width & 0x80) != 0;
                let r = (rounded as f32) * sf;
                let is_shadow = border_color.a() == 0 && raw_bw > 0;
                if is_shadow {
                    if bg.a() > 0 {
                        self.primitives.push(make_shadow_primitive(
                            shifted_sb,
                            shifted_sc,
                            clip_radius_px,
                            r,
                            (raw_bw as f32) * sf,
                            bg,
                        ));
                    }
                } else if bg.a() > 0 || bg_grad.a() > 0 || (bw > 0.0 && border_color.a() > 0) {
                    self.primitives.push(make_quad_primitive(
                        shifted_sb,
                        shifted_sc,
                        clip_radius_px,
                        r,
                        bw,
                        horiz,
                        bg,
                        bg_grad,
                        border_color,
                    ));
                }
            }
            DrawPayload::StyledQuad(sq) => {
                let ramp_v = match sq.paint {
                    Paint::Solid(_) => None,
                    Paint::LinearGradient { stops, space, .. }
                    | Paint::RadialGradient { stops, space, .. }
                    | Paint::ConicGradient { stops, space, .. } => {
                        let Some(v) = self.ramp_atlas.get_or_insert(queue, stops, space, sq.opacity)
                        else {
                            return Err(AtlasGrowNeed {
                                glyph_full: false,
                                sprite_need: None,
                                ramp_full: true,
                            });
                        };
                        Some(v)
                    }
                };
                if let Some(prim) = make_styled_quad_primitive(
                    shifted_sb,
                    shifted_sc,
                    clip_radius_px,
                    sq,
                    sf,
                    ramp_v,
                ) {
                    self.primitives.push(prim);
                }
            }
            DrawPayload::TextRun {
                text,
                color,
                font_size,
                text_flags,
            } => {
                if text.is_empty() || color.a() == 0 {
                    return Ok(());
                }
                let sb_x0 = sb_f[0].floor();
                let sb_y0 = sb_f[1].floor();
                let subpx_frac_x = sb_f[0] - sb_x0;
                let subpx_frac_y = sb_f[1] - sb_y0;
                let sb = Region::new(
                    sb_x0 as i16,
                    sb_y0 as i16,
                    sb_f[2] as i16,
                    sb_f[3] as i16,
                );
                let sc = Region::new(
                    sc_f[0].floor() as i16,
                    sc_f[1].floor() as i16,
                    (sc_f[2] - sc_f[0]).max(0.0).ceil() as i16,
                    (sc_f[3] - sc_f[1]).max(0.0).ceil() as i16,
                );
                let (adj_bounds, adj_sc, adj_flags, logical_fs, fs_q4) = compute_scaled_text_box(
                    op.bounds,
                    op.clip,
                    viewport,
                    sb,
                    sc,
                    text.as_str(),
                    font_size,
                    text_flags,
                    sf,
                );
                let adj_sc_f = [
                    adj_sc.x as f32 - origin_x,
                    adj_sc.y as f32 - origin_y,
                    adj_sc.right() as f32 - origin_x,
                    adj_sc.bottom() as f32 - origin_y,
                ];
                let prim_before = self.primitives.len();
                if !self.encode_text_run(
                    device,
                    queue,
                    adj_bounds,
                    adj_sc,
                    adj_sc_f,
                    clip_radius_px,
                    text.as_str(),
                    color,
                    logical_fs,
                    fs_q4,
                    adj_flags,
                    use_ttf,
                    subpx_frac_x,
                    subpx_frac_y,
                    origin_x,
                    origin_y,
                    target_w,
                    target_h,
                ) {
                    return Err(AtlasGrowNeed {
                        glyph_full: true,
                        sprite_need: None,
                        ramp_full: false,
                    });
                }
                if origin_x != 0.0 || origin_y != 0.0 {
                    for p in &mut self.primitives[prim_before..] {
                        p.bounds[0] -= origin_x;
                        p.bounds[1] -= origin_y;
                    }
                }
            }
            DrawPayload::Sprite(SpriteSource::Arc {
                thickness,
                start_deg10,
                sweep_deg10,
                fill_sweep_deg10,
                track_color,
                fill_color,
            }) => {
                self.primitives.push(make_arc_primitive(
                    shifted_sb,
                    shifted_sc,
                    clip_radius_px,
                    ((thickness.max(1) as f32) * sf).max(1.5),
                    start_deg10,
                    sweep_deg10,
                    fill_sweep_deg10,
                    track_color,
                    fill_color,
                ));
            }
            DrawPayload::Sprite(sprite) => {
                sb_f[0] = sb_f[0].round();
                sb_f[1] = sb_f[1].round();
                let sb = Region::new(
                    sb_f[0] as i16,
                    sb_f[1] as i16,
                    sb_f[2] as i16,
                    sb_f[3] as i16,
                );
                let sc = Region::new(
                    sc_f[0] as i16,
                    sc_f[1] as i16,
                    (sc_f[2] - sc_f[0]).max(0.0) as i16,
                    (sc_f[3] - sc_f[1]).max(0.0) as i16,
                );
                let eff = sc.intersect(sb);
                if eff.is_empty() {
                    return Ok(());
                }
                let w = eff.w.max(1) as usize;
                let h = eff.h.max(1) as usize;
                let cache_key = SpriteCacheKey {
                    w: w as u32,
                    h: h as u32,
                    hash: Self::sprite_cache_hash(
                        sprite,
                        eff.x.saturating_sub(sb.x),
                        eff.y.saturating_sub(sb.y),
                        sb.w,
                        sb.h,
                    ),
                };

                let uv = if let Some(cached_uv) = self.sprite_atlas.lookup(cache_key) {
                    cached_uv
                } else {
                    let len = w * h;
                    if self.sprite_scratch.len() < len {
                        self.sprite_scratch.resize(len, Rgba8888(0));
                    }
                    let slice = &mut self.sprite_scratch[..len];
                    slice.fill(Rgba8888(0));
                    rasterize_sprite_source(sb, eff, sprite, eff.x, eff.y, w, slice);

                    #[cfg(target_endian = "little")]
                    let rgba_bytes: &[u8] = pod_as_bytes(slice);
                    #[cfg(not(target_endian = "little"))]
                    let rgba_bytes: &[u8] = {
                        self.sprite_bytes_scratch.clear();
                        self.sprite_bytes_scratch.reserve(len * 4);
                        for px in slice.iter() {
                            self.sprite_bytes_scratch
                                .extend_from_slice(&px.to_rgba_bytes());
                        }
                        &self.sprite_bytes_scratch
                    };

                    let Some(uv) = self
                        .sprite_atlas
                        .insert_and_upload(queue, cache_key, rgba_bytes)
                    else {
                        return Err(AtlasGrowNeed {
                            glyph_full: false,
                            sprite_need: Some((w as u32, h as u32)),
                            ramp_full: false,
                        });
                    };
                    uv
                };

                self.primitives.push(GpuPrimitive {
                    bounds: [
                        eff.x as f32 - origin_x,
                        eff.y as f32 - origin_y,
                        w as f32,
                        h as f32,
                    ],
                    clip: shifted_sc,
                    uv,
                    color0: [1.0; 4],
                    color1: [0.0; 4],
                    border_color: [0.0; 4],
                    params: [0.0, 0.0, KIND_SPRITE, 0.0],
                    extra: [clip_radius_px, 0.0, 0.0, 0.0],
                    radii: [0.0; 4],
                    grad_a: [0.0; 4],
                    grad_b: [0.0; 4],
                    xform: IDENTITY_XFORM,
                });
            }
            DrawPayload::RichText(rt) => {
                // 启用 `swash` 特性时，对携带可变字体轴或大字号的单行简单富文本直通 Swash -> 共享 Slug 2.0 曲线管线，
                // 支持弹簧驱动连续可变字重（wght）与字宽（wdth）过渡，免除 CPU 反复光栅化与 SpriteAtlas 抖动。
                #[cfg(feature = "swash")]
                if use_ttf && clip_radius_px <= 0.0 && !rt.rtl_paragraph && !rt.spans.is_empty() {
                    let has_var = rt.spans.iter().any(|sp| sp.has_variable_axes());
                    let max_fs_px = rt
                        .spans
                        .iter()
                        .map(|sp| sp.font_size.unwrap_or(rt.default_font_size).max(8) as f32 * sf)
                        .fold(0.0f32, f32::max);
                    let simple_single_line = (has_var
                        || (self.slug_text_threshold_px.is_finite()
                            && max_fs_px >= self.slug_text_threshold_px))
                        && rt.spans.iter().all(|sp| {
                            sp.bg_color.is_none()
                                && sp.decoration.0 == 0
                                && sp.letter_spacing_q4 == 0
                                && !sp.text.as_str().contains('\n')
                                && !sp.text.as_str().chars().any(crate::text_rich::is_emoji_char)
                        });
                    if simple_single_line {
                        let mut shaped_runs: Vec<(
                            std::sync::Arc<crate::swash_shaper::ShapedGlyphRun>,
                            f32,
                            [f32; 4],
                            [Option<crate::swash_shaper::FontVariationAxis>; 2],
                        )> = Vec::with_capacity(rt.spans.len());
                        let mut total_adv = 0.0f32;
                        let mut all_ok = true;
                        for sp in rt.spans {
                            let s = sp.text.as_str();
                            if s.is_empty() {
                                continue;
                            }
                            let span_fs =
                                (sp.font_size.unwrap_or(rt.default_font_size).max(8) as f32) * sf;
                            let mut vars: [Option<crate::swash_shaper::FontVariationAxis>; 2] =
                                [None, None];
                            let mut var_buf: [crate::swash_shaper::FontVariationAxis; 2] = [
                                crate::swash_shaper::FontVariationAxis::weight(400.0),
                                crate::swash_shaper::FontVariationAxis::width(100.0),
                            ];
                            let mut var_len = 0usize;
                            if sp.var_weight_x10 != 0 {
                                let ax = crate::swash_shaper::FontVariationAxis::weight(
                                    (sp.var_weight_x10 as f32) * 0.1,
                                );
                                vars[var_len] = Some(ax);
                                var_buf[var_len] = ax;
                                var_len += 1;
                            }
                            if sp.var_width_x10 != 0 {
                                let ax = crate::swash_shaper::FontVariationAxis::width(
                                    (sp.var_width_x10 as f32) * 0.1,
                                );
                                vars[var_len] = Some(ax);
                                var_buf[var_len] = ax;
                                var_len += 1;
                            }
                            let Some(run) = crate::swash_shaper::shape_text_run_swash(
                                s,
                                sp.font,
                                span_fs,
                                sp.ligatures,
                                &var_buf[..var_len],
                            ) else {
                                all_ok = false;
                                break;
                            };
                            total_adv += run.total_advance;
                            let col = color_to_f32(sp.color.unwrap_or(rt.default_color));
                            shaped_runs.push((run, span_fs, col, vars));
                        }
                        let tol = (max_fs_px * 0.25).max(4.0);
                        if all_ok
                            && !shaped_runs.is_empty()
                            && (rt.nowrap || total_adv <= sb_f[2] + tol)
                        {
                            if self.ui_slug_engine.is_none() {
                                let raw_cache =
                                    self.pipeline_cache.as_deref().map(|c| c.wgpu_cache());
                                self.ui_slug_engine =
                                    Some(super::slug_glyph::SlugTextEngine::new(
                                        device,
                                        self.target_format,
                                        raw_cache,
                                    ));
                                if let Some(c) = &self.pipeline_cache {
                                    c.persist();
                                }
                            }
                            let is_srgb = self.target_is_srgb;
                            let mut slug = self.ui_slug_engine.take().unwrap();
                            let align_x = match rt.align {
                                crate::style::TextAlign::Center => {
                                    ((sb_f[2] - total_adv).max(0.0)) * 0.5
                                }
                                crate::style::TextAlign::Right => (sb_f[2] - total_adv).max(0.0),
                                crate::style::TextAlign::Left => 0.0,
                            };
                            let ascent = crate::display::ttf_atlas::get_font_ascent_f32(max_fs_px)
                                as f32;
                            let baseline_y = shifted_sb[1] + ascent;
                            let mut pen_x = shifted_sb[0] + align_x;
                            let mut total_pushed = 0usize;
                            for (run, span_fs, col, vars) in &shaped_runs {
                                let mut var_buf: [crate::swash_shaper::FontVariationAxis; 2] = [
                                    crate::swash_shaper::FontVariationAxis::weight(400.0),
                                    crate::swash_shaper::FontVariationAxis::width(100.0),
                                ];
                                let mut var_len = 0usize;
                                for v in vars.iter().flatten() {
                                    var_buf[var_len] = *v;
                                    var_len += 1;
                                }
                                total_pushed += slug.push_swash_run(
                                    run,
                                    pen_x,
                                    baseline_y,
                                    *span_fs,
                                    &var_buf[..var_len],
                                    shifted_sc,
                                    *col,
                                    target_w,
                                    target_h,
                                    is_srgb,
                                );
                                pen_x += run.total_advance;
                            }
                            self.ui_slug_engine = Some(slug);
                            if total_pushed > 0 {
                                return Ok(());
                            }
                        }
                    }
                }
                let sb = snap_bounds_region_scaled(op.bounds, sf);
                let sc = Region::new(
                    sc_f[0] as i16,
                    sc_f[1] as i16,
                    (sc_f[2] - sc_f[0]).max(0.0) as i16,
                    (sc_f[3] - sc_f[1]).max(0.0) as i16,
                );
                let eff = sc.intersect(sb);
                if eff.is_empty() {
                    return Ok(());
                }
                let w = eff.w.max(1) as usize;
                let h = eff.h.max(1) as usize;
                let mut kh: u32 = 0x5254_4558;
                rt.mix_hash(&mut kh);
                let hash = ((kh as u64) << 32)
                    | (((eff.x.saturating_sub(sb.x) as u16 as u64) << 16)
                        | (eff.y.saturating_sub(sb.y) as u16 as u64))
                    ^ ((sf * 16.0).round() as u64);
                let cache_key = SpriteCacheKey {
                    w: w as u32,
                    h: h as u32,
                    hash,
                };
                let uv = if let Some(cached) = self.sprite_atlas.lookup(cache_key) {
                    cached
                } else {
                    let len = w * h;
                    if self.sprite_scratch.len() < len {
                        self.sprite_scratch.resize(len, Rgba8888(0));
                    }
                    let slice = &mut self.sprite_scratch[..len];
                    slice.fill(Rgba8888(0));
                    crate::renderers::software::rasterize_rich_text(
                        rt, sb, eff, sf, eff.x, eff.y, w, slice,
                    );
                    let rgba_bytes = pod_as_bytes(slice);
                    let Some(u) = self.sprite_atlas.insert_and_upload(queue, cache_key, rgba_bytes)
                    else {
                        return Err(AtlasGrowNeed {
                            glyph_full: false,
                            sprite_need: Some((w as u32, h as u32)),
                            ramp_full: false,
                        });
                    };
                    u
                };
                self.primitives.push(GpuPrimitive {
                    bounds: [
                        eff.x as f32 - origin_x,
                        eff.y as f32 - origin_y,
                        w as f32,
                        h as f32,
                    ],
                    clip: shifted_sc,
                    uv,
                    color0: [1.0; 4],
                    color1: [0.0; 4],
                    border_color: [0.0; 4],
                    params: [0.0, 0.0, KIND_SPRITE, 0.0],
                    extra: [clip_radius_px, 0.0, 0.0, 0.0],
                    radii: [0.0; 4],
                    grad_a: [0.0; 4],
                    grad_b: [0.0; 4],
                    xform: IDENTITY_XFORM,
                });
            }
            DrawPayload::DecodedImage(img) => {
                let sb = snap_bounds_region_scaled(op.bounds, sf);
                let sc = Region::new(
                    sc_f[0] as i16,
                    sc_f[1] as i16,
                    (sc_f[2] - sc_f[0]).max(0.0) as i16,
                    (sc_f[3] - sc_f[1]).max(0.0) as i16,
                );
                let eff = sc.intersect(sb);
                if eff.is_empty() {
                    return Ok(());
                }
                let w = eff.w.max(1) as usize;
                let h = eff.h.max(1) as usize;
                // 从轻量资产元数据读取版本号，不触发 CPU 像素缓存查询，CPU 像素淘汰后也不会退化为 0。
                let cache_rev = crate::image_pipeline::image_revision(img.id).unwrap_or(0);
                let mut kh: u32 = 0x4449_4D47 ^ (cache_rev as u32);
                img.mix_hash(&mut kh);
                let hash = ((kh as u64) << 32)
                    | (((eff.x.saturating_sub(sb.x) as u16 as u64) << 16)
                        | (eff.y.saturating_sub(sb.y) as u16 as u64))
                    ^ ((sf * 16.0).round() as u64);
                let cache_key = SpriteCacheKey {
                    w: w as u32,
                    h: h as u32,
                    hash,
                };
                let uv = if let Some(cached) = self.sprite_atlas.lookup(cache_key) {
                    cached
                } else {
                    let len = w * h;
                    if self.sprite_scratch.len() < len {
                        self.sprite_scratch.resize(len, Rgba8888(0));
                    }
                    let slice = &mut self.sprite_scratch[..len];
                    slice.fill(Rgba8888(0));
                    crate::renderers::software::rasterize_decoded_image(
                        img, sb, eff, sf, eff.x, eff.y, w, slice,
                    );
                    let rgba_bytes = pod_as_bytes(slice);
                    let Some(u) = self.sprite_atlas.insert_and_upload(queue, cache_key, rgba_bytes)
                    else {
                        return Err(AtlasGrowNeed {
                            glyph_full: false,
                            sprite_need: Some((w as u32, h as u32)),
                            ramp_full: false,
                        });
                    };
                    u
                };
                self.primitives.push(GpuPrimitive {
                    bounds: [
                        eff.x as f32 - origin_x,
                        eff.y as f32 - origin_y,
                        w as f32,
                        h as f32,
                    ],
                    clip: shifted_sc,
                    uv,
                    color0: [1.0; 4],
                    color1: [0.0; 4],
                    border_color: [0.0; 4],
                    params: [0.0, 0.0, KIND_SPRITE, 0.0],
                    extra: [clip_radius_px, 0.0, 0.0, 0.0],
                    radii: [0.0; 4],
                    grad_a: [0.0; 4],
                    grad_b: [0.0; 4],
                    xform: IDENTITY_XFORM,
                });
            }
            DrawPayload::Vector(vec_spec) => {
                let sb = snap_bounds_region_scaled(op.bounds, sf);
                let sc = Region::new(
                    sc_f[0] as i16,
                    sc_f[1] as i16,
                    (sc_f[2] - sc_f[0]).max(0.0) as i16,
                    (sc_f[3] - sc_f[1]).max(0.0) as i16,
                );
                let eff = sc.intersect(sb);
                if eff.is_empty() {
                    return Ok(());
                }
                let w = eff.w.max(1) as usize;
                let h = eff.h.max(1) as usize;
                let mut kh: u32 = 0x5645_4354;
                vec_spec.mix_hash(&mut kh);
                let hash = ((kh as u64) << 32)
                    | (((eff.x.saturating_sub(sb.x) as u16 as u64) << 16)
                        | (eff.y.saturating_sub(sb.y) as u16 as u64))
                    ^ ((sf * 16.0).round() as u64);
                let cache_key = SpriteCacheKey {
                    w: w as u32,
                    h: h as u32,
                    hash,
                };
                let uv = if let Some(cached) = self.sprite_atlas.lookup(cache_key) {
                    cached
                } else {
                    let len = w * h;
                    if self.sprite_scratch.len() < len {
                        self.sprite_scratch.resize(len, Rgba8888(0));
                    }
                    let slice = &mut self.sprite_scratch[..len];
                    slice.fill(Rgba8888(0));
                    crate::vector_canvas::rasterize_vector_spec(
                        vec_spec, sb, eff, sf, eff.x, eff.y, w, slice,
                    );
                    let rgba_bytes = pod_as_bytes(slice);
                    let Some(u) = self.sprite_atlas.insert_and_upload(queue, cache_key, rgba_bytes)
                    else {
                        return Err(AtlasGrowNeed {
                            glyph_full: false,
                            sprite_need: Some((w as u32, h as u32)),
                            ramp_full: false,
                        });
                    };
                    u
                };
                self.primitives.push(GpuPrimitive {
                    bounds: [
                        eff.x as f32 - origin_x,
                        eff.y as f32 - origin_y,
                        w as f32,
                        h as f32,
                    ],
                    clip: shifted_sc,
                    uv,
                    color0: [1.0; 4],
                    color1: [0.0; 4],
                    border_color: [0.0; 4],
                    params: [0.0, 0.0, KIND_SPRITE, 0.0],
                    extra: [clip_radius_px, 0.0, 0.0, 0.0],
                    radii: [0.0; 4],
                    grad_a: [0.0; 4],
                    grad_b: [0.0; 4],
                    xform: IDENTITY_XFORM,
                });
            }
            DrawPayload::SceneCanvas(spec) => {
                // 隔离图层/多 Pass 兜底路径：通过软件光栅同步到精灵图集以保持与复杂混合模式兼容
                let sb = snap_bounds_region_scaled(op.bounds, sf);
                let sc = Region::new(
                    sc_f[0] as i16,
                    sc_f[1] as i16,
                    (sc_f[2] - sc_f[0]).max(0.0) as i16,
                    (sc_f[3] - sc_f[1]).max(0.0) as i16,
                );
                let eff = sc.intersect(sb);
                if eff.is_empty() {
                    return Ok(());
                }
                let w = eff.w.max(1) as usize;
                let h = eff.h.max(1) as usize;
                let hash = ((spec.content_hash() as u64) << 32)
                    | (((eff.x.saturating_sub(sb.x) as u16 as u64) << 16)
                        | (eff.y.saturating_sub(sb.y) as u16 as u64))
                    ^ ((sf * 16.0).round() as u64);
                let cache_key = SpriteCacheKey {
                    w: w as u32,
                    h: h as u32,
                    hash,
                };
                let uv = if let Some(cached) = self.sprite_atlas.lookup(cache_key) {
                    cached
                } else {
                    let len = w * h;
                    if self.sprite_scratch.len() < len {
                        self.sprite_scratch.resize(len, Rgba8888(0));
                    }
                    let slice = &mut self.sprite_scratch[..len];
                    slice.fill(Rgba8888(0));
                    crate::canvas_scene::rasterize_scene_canvas(
                        sb, eff, spec, sf, eff.x, eff.y, w, slice,
                    );
                    let rgba_bytes = pod_as_bytes(slice);
                    let Some(u) = self.sprite_atlas.insert_and_upload(queue, cache_key, rgba_bytes)
                    else {
                        return Err(AtlasGrowNeed {
                            glyph_full: false,
                            sprite_need: Some((w as u32, h as u32)),
                            ramp_full: false,
                        });
                    };
                    u
                };
                self.primitives.push(GpuPrimitive {
                    bounds: [
                        eff.x as f32 - origin_x,
                        eff.y as f32 - origin_y,
                        w as f32,
                        h as f32,
                    ],
                    clip: shifted_sc,
                    uv,
                    color0: [1.0; 4],
                    color1: [0.0; 4],
                    border_color: [0.0; 4],
                    params: [0.0, 0.0, KIND_SPRITE, 0.0],
                    extra: [clip_radius_px, 0.0, 0.0, 0.0],
                    radii: [0.0; 4],
                    grad_a: [0.0; 4],
                    grad_b: [0.0; 4],
                    xform: IDENTITY_XFORM,
                });
            }
            DrawPayload::PushLayer(_)
            | DrawPayload::PopLayer
            | DrawPayload::ExternalTexture(_) => {}
        }
        Ok(())
    }

    /// 单 Pass 无图层快路径：将 `DrawOp` 编码为 `self.primitives`（大字号文本自动路由至 Slug 2.0 并记录 Z-order 分段），
    /// 并按需上传字形、精灵与渐变图集。
    fn try_encode_ops(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        phys_w: u32,
        phys_h: u32,
    ) -> Result<(), AtlasGrowNeed> {
        self.primitives.clear();
        self.ui_slug_segments.clear();
        if let Some(slug) = &mut self.ui_slug_engine {
            slug.begin_frame();
        }
        if viewport.is_empty() {
            return Ok(());
        }

        let sf = scale_factor.clamp(0.5, 4.0);
        let use_ttf = crate::display::ttf_atlas::has_system_font();
        let mut batch_prim_start = 0u32;

        for (idx, op) in ops.iter().enumerate() {
            let raw_clip = match op.payload {
                DrawPayload::StyledQuad(sq) if sq.transform.is_some() => viewport.intersect(op.clip),
                _ => viewport.intersect(op.clip).intersect(op.bounds),
            };
            if raw_clip.is_empty() {
                continue;
            }
            let eff_clip = compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..]);
            if eff_clip.is_empty() {
                continue;
            }
            let prim_before = self.primitives.len() as u32;
            let slug_before = self
                .ui_slug_engine
                .as_ref()
                .map(|s| s.instance_count())
                .unwrap_or(0);
            self.encode_single_op(
                device, queue, op, eff_clip, viewport, sf, use_ttf, 0.0, 0.0, phys_w, phys_h,
            )?;
            let slug_after = self
                .ui_slug_engine
                .as_ref()
                .map(|s| s.instance_count())
                .unwrap_or(0);
            if slug_after > slug_before {
                self.ui_slug_segments.push((
                    batch_prim_start,
                    prim_before,
                    slug_before,
                    slug_after,
                ));
                batch_prim_start = prim_before;
            }
        }
        if !self.ui_slug_segments.is_empty() {
            let final_prim = self.primitives.len() as u32;
            if final_prim > batch_prim_start {
                self.ui_slug_segments.push((batch_prim_start, final_prim, 0, 0));
            }
        }
        Ok(())
    }

    fn get_or_build_text_layout(
        &mut self,
        text: &str,
        logical_font_size: u8,
        font_size_q4: u16,
        max_width: i16,
        text_flags: u8,
        use_ttf: bool,
    ) -> Arc<CachedTextLayout> {
        let logical_fs = logical_font_size.max(8);
        let q4 = font_size_q4.clamp(32, 1020);
        let key = TextLayoutKey {
            text_hash: hash_layout_text(text),
            text_len: text.len() as u32,
            logical_font_size: logical_fs,
            font_size_q4: q4,
            max_width,
            text_flags,
            use_ttf,
        };
        if let Some(hit) = self.text_layout_cache.get(&key) {
            if hit.text.as_str() == text {
                self.text_layout_hits = self.text_layout_hits.wrapping_add(1);
                return Arc::clone(hit);
            }
        }

        let render_fs = (q4 as f32) * 0.25;
        let fs_u8 = ((q4 + 2) >> 2).clamp(8, 255) as u8;
        let scale_ratio = render_fs / (logical_fs as f32);
        let is_ellipsis = (text_flags & 0x04) != 0;
        let is_nowrap = (text_flags & 0x08) != 0;
        let lines = if use_ttf {
            let logical_max_w = ((max_width as f32) / scale_ratio)
                .round()
                .clamp(1.0, i16::MAX as f32) as i16;
            split_text_lines(text, logical_fs, logical_max_w, is_nowrap || is_ellipsis)
        } else {
            split_text_lines(text, fs_u8, max_width, is_nowrap || is_ellipsis)
        };
        let line_count = lines.len();
        let char_h = if use_ttf {
            (render_fs.round() as i16).max(8)
        } else {
            (fs_u8 as i16).max(8)
        };
        let line_step = if use_ttf {
            ((render_fs * 1.25).round() as i16).max(char_h)
        } else {
            ((fs_u8 as i16) * 5 / 4).max(char_h)
        };
        let ascent = if use_ttf {
            crate::display::ttf_atlas::get_font_ascent_f32(render_fs) as i16
        } else {
            char_h
        };
        let dot_w = measure_char_advance('.', fs_u8);
        let dot_adv_f = if use_ttf {
            crate::display::ttf_atlas::measure_ttf_char_advance_f32('.', logical_fs)
                .map(|a| a * scale_ratio)
                .unwrap_or(dot_w as f32)
        } else {
            dot_w as f32
        };
        let total_h = if line_count == 0 {
            0
        } else {
            char_h.saturating_add(line_step.saturating_mul((line_count as i16) - 1))
        };

        let mut glyphs = Vec::with_capacity(text.len().min(128));
        for (line_idx, &(b_start, b_end, raw_line_w_orig)) in lines.as_slice().iter().enumerate() {
            let line_str = &text[b_start..b_end];
            let raw_line_w = if use_ttf {
                (crate::display::measure_line_width_f32(line_str, logical_fs) * scale_ratio).ceil()
                    as i16
            } else {
                raw_line_w_orig
            };
            let need_ellipsis = is_ellipsis && raw_line_w > max_width && max_width > dot_w * 3;
            let eff_w = if need_ellipsis {
                max_width
            } else {
                raw_line_w.min(max_width)
            };
            let start_rel_x = align_offset_x(max_width, eff_w, text_flags);
            let rel_y = if use_ttf {
                ascent.saturating_add((line_idx as i16).saturating_mul(line_step))
            } else {
                (line_idx as i16).saturating_mul(line_step)
            };

            if use_ttf {
                let mut pen_x = start_rel_x as f32;
                let max_x_before_dots_f = (max_width as f32) - dot_adv_f * 3.0;
                let mut prev_ch: Option<char> = None;
                for ch in line_str.chars() {
                    if let Some(p) = prev_ch {
                        pen_x += crate::display::ttf_atlas::measure_ttf_kerning_f32(
                            p, ch, logical_fs,
                        ) * scale_ratio;
                    }
                    let adv_f =
                        crate::display::ttf_atlas::measure_ttf_char_advance_f32(ch, logical_fs)
                            .map(|a| a * scale_ratio)
                            .unwrap_or_else(|| {
                                (measure_char_advance(ch, logical_fs) as f32) * scale_ratio
                            });
                    if need_ellipsis && pen_x + adv_f > max_x_before_dots_f + 0.01 {
                        for _ in 0..3 {
                            glyphs.push(ShapedGlyphPos {
                                ch: '.',
                                rel_x: pen_x.round() as i16,
                                rel_y,
                            });
                            pen_x += dot_adv_f;
                        }
                        break;
                    }
                    if ch != ' ' {
                        glyphs.push(ShapedGlyphPos {
                            ch,
                            rel_x: pen_x.round() as i16,
                            rel_y,
                        });
                    }
                    pen_x += adv_f;
                    prev_ch = Some(ch);
                }
            } else {
                let mut rel_x = start_rel_x;
                let max_x_before_dots = max_width.saturating_sub(dot_w * 3);
                let mut prev_ch: Option<char> = None;
                for ch in line_str.chars() {
                    if let Some(p) = prev_ch {
                        rel_x = rel_x.saturating_add(measure_char_kerning(p, ch, fs_u8));
                    }
                    let adv = measure_char_advance(ch, fs_u8);
                    if need_ellipsis && rel_x.saturating_add(adv) > max_x_before_dots {
                        for _ in 0..3 {
                            glyphs.push(ShapedGlyphPos {
                                ch: '.',
                                rel_x,
                                rel_y,
                            });
                            rel_x = rel_x.saturating_add(dot_w);
                        }
                        break;
                    }
                    if ch != ' ' {
                        glyphs.push(ShapedGlyphPos { ch, rel_x, rel_y });
                    }
                    rel_x = rel_x.saturating_add(adv);
                    prev_ch = Some(ch);
                }
            }
        }

        let built = Arc::new(CachedTextLayout {
            text: String::from(text),
            total_h,
            glyphs,
        });
        if self.text_layout_cache.len() >= MAX_TEXT_LAYOUT_CACHE_ENTRIES {
            self.text_layout_cache.clear();
        }
        self.text_layout_cache.insert(key, Arc::clone(&built));
        built
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_text_run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: Region,
        clip: Region,
        clip_f32: [f32; 4],
        clip_radius_px: f32,
        text: &str,
        color: Color,
        logical_font_size: u8,
        font_size_q4: u16,
        text_flags: u8,
        use_ttf: bool,
        subpx_frac_x: f32,
        subpx_frac_y: f32,
        origin_x: f32,
        origin_y: f32,
        target_w: u32,
        target_h: u32,
    ) -> bool {
        let col_f32 = color_to_f32(color);
        let render_fs = (font_size_q4.clamp(32, 1020) as f32) * 0.25;
        let slug_offset_x = subpx_frac_x - origin_x;
        let slug_offset_y = subpx_frac_y - origin_y;
        let has_subpx = if subpx_frac_x != 0.0 || subpx_frac_y != 0.0 {
            1.0
        } else {
            0.0
        };
        let route_to_slug = use_ttf
            && clip_radius_px <= 0.0
            && self.slug_text_threshold_px.is_finite()
            && render_fs >= self.slug_text_threshold_px;

        // 启用 `swash` 且命中单行 Slug 快路径时，直接绕过旧版 `get_or_build_text_layout` 避免重复排版
        #[cfg(feature = "swash")]
        if route_to_slug {
            let is_ellipsis = (text_flags & 0x04) != 0;
            let is_nowrap = (text_flags & 0x08) != 0;
            if !is_ellipsis
                && !text.contains('\n')
                && !text.chars().any(crate::text_rich::is_emoji_char)
            {
                if let Some(run) = crate::swash_shaper::shape_text_run_swash(
                    text,
                    crate::text_rich::FontSpec::SANS,
                    render_fs,
                    true,
                    &[],
                ) {
                    let tol = (render_fs * 0.25).max(4.0);
                    if is_nowrap || run.total_advance <= (bounds.w as f32) + tol {
                        if self.ui_slug_engine.is_none() {
                            let raw_cache = self.pipeline_cache.as_deref().map(|c| c.wgpu_cache());
                            self.ui_slug_engine = Some(super::slug_glyph::SlugTextEngine::new(
                                device,
                                self.target_format,
                                raw_cache,
                            ));
                            if let Some(c) = &self.pipeline_cache {
                                c.persist();
                            }
                        }
                        let is_srgb = self.target_is_srgb;
                        let mut slug = self.ui_slug_engine.take().unwrap();
                        let line_h = (render_fs.round() as i16).max(8);
                        let top_y = bounds
                            .y
                            .saturating_add(bounds.h.saturating_sub(line_h).max(0) >> 1);
                        let eff_w = (run.total_advance.ceil() as i16).clamp(0, bounds.w);
                        let align_x = align_offset_x(bounds.w, eff_w, text_flags) as f32;
                        let ascent =
                            crate::display::ttf_atlas::get_font_ascent_f32(render_fs) as i16;
                        let baseline_y =
                            (top_y.saturating_add(ascent) as f32) + slug_offset_y;
                        let pen_x0 = (bounds.x as f32) + align_x + slug_offset_x;
                        let pushed = slug.push_swash_run(
                            &run,
                            pen_x0,
                            baseline_y,
                            render_fs,
                            &[],
                            clip_f32,
                            col_f32,
                            target_w,
                            target_h,
                            is_srgb,
                        );
                        self.ui_slug_engine = Some(slug);
                        if pushed > 0 {
                            return true;
                        }
                    }
                }
            }
        }

        let layout = self.get_or_build_text_layout(
            text,
            logical_font_size,
            font_size_q4,
            bounds.w,
            text_flags,
            use_ttf,
        );
        if layout.glyphs.is_empty() {
            return true;
        }
        let top_y = bounds
            .y
            .saturating_add(bounds.h.saturating_sub(layout.total_h).max(0) >> 1);

        if route_to_slug {
            if self.ui_slug_engine.is_none() {
                let raw_cache = self.pipeline_cache.as_deref().map(|c| c.wgpu_cache());
                self.ui_slug_engine = Some(super::slug_glyph::SlugTextEngine::new(
                    device,
                    self.target_format,
                    raw_cache,
                ));
                if let Some(c) = &self.pipeline_cache {
                    c.persist();
                }
            }
            let is_srgb = self.target_is_srgb;
            let mut slug = self.ui_slug_engine.take().unwrap();
            let mut ok = true;
            for g in &layout.glyphs {
                let pen_x = (bounds.x.saturating_add(g.rel_x) as f32) + slug_offset_x;
                let baseline_y = (top_y.saturating_add(g.rel_y) as f32) + slug_offset_y;
                if !slug.push_ui_glyph(
                    g.ch,
                    pen_x,
                    baseline_y,
                    render_fs,
                    clip_f32,
                    col_f32,
                    target_w,
                    target_h,
                    is_srgb,
                ) {
                    // 无 TTF 矢量轮廓字符（例如彩色 Emoji）自动回退单字到位图字形图集，绝不丢字
                    let Some(entry) = self
                        .glyph_atlas
                        .get_or_insert(queue, g.ch, font_size_q4, use_ttf)
                    else {
                        ok = false;
                        break;
                    };
                    if entry.width > 0 && entry.height > 0 {
                        let gx = bounds
                            .x
                            .saturating_add(g.rel_x)
                            .saturating_add(entry.xmin);
                        let gy = top_y
                            .saturating_add(g.rel_y)
                            .saturating_sub(entry.height)
                            .saturating_sub(entry.ymin);
                        let greg = Region::new(gx, gy, entry.width, entry.height);
                        if !clip.intersect(greg).is_empty() {
                            self.primitives.push(GpuPrimitive {
                                bounds: [
                                    (gx as f32) + subpx_frac_x,
                                    (gy as f32) + subpx_frac_y,
                                    entry.width as f32,
                                    entry.height as f32,
                                ],
                                clip: clip_f32,
                                uv: entry.uv,
                                color0: col_f32,
                                color1: [0.0; 4],
                                border_color: [0.0; 4],
                                params: [0.0, 0.0, KIND_GLYPH, 0.0],
                                extra: [clip_radius_px, has_subpx, 0.0, 0.0],
                                radii: [0.0; 4],
                                grad_a: [0.0; 4],
                                grad_b: [0.0; 4],
                                xform: IDENTITY_XFORM,
                            });
                        }
                    }
                }
            }
            self.ui_slug_engine = Some(slug);
            return ok;
        }

        for g in &layout.glyphs {
            let Some(entry) = self
                .glyph_atlas
                .get_or_insert(queue, g.ch, font_size_q4, use_ttf)
            else {
                return false;
            };
            if entry.width > 0 && entry.height > 0 {
                let gx = bounds
                    .x
                    .saturating_add(g.rel_x)
                    .saturating_add(entry.xmin);
                let gy = if use_ttf {
                    top_y
                        .saturating_add(g.rel_y)
                        .saturating_sub(entry.height)
                        .saturating_sub(entry.ymin)
                } else {
                    top_y.saturating_add(g.rel_y)
                };
                let greg = Region::new(gx, gy, entry.width, entry.height);
                if !clip.intersect(greg).is_empty() {
                    // 透传 Q4 亚像素残差（运动帧启用防渗色双线性亚像素采样同步外框无“框动字跳”，静止帧自动为 0.0 走 Nearest 保整像素锐度）
                    self.primitives.push(GpuPrimitive {
                        bounds: [
                            (gx as f32) + subpx_frac_x,
                            (gy as f32) + subpx_frac_y,
                            entry.width as f32,
                            entry.height as f32,
                        ],
                        clip: clip_f32,
                        uv: entry.uv,
                        color0: col_f32,
                        color1: [0.0; 4],
                        border_color: [0.0; 4],
                        params: [0.0, 0.0, KIND_GLYPH, 0.0],
                        extra: [clip_radius_px, has_subpx, 0.0, 0.0],
                        radii: [0.0; 4],
                        grad_a: [0.0; 4],
                        grad_b: [0.0; 4],
                        xform: IDENTITY_XFORM,
                    });
                }
            }
        }
        true
    }

    fn handle_atlas_grow(&mut self, device: &wgpu::Device, need: AtlasGrowNeed) -> (bool, bool) {
        let mut dirty = false;
        let mut can_retry = true;
        if need.glyph_full && self.glyph_atlas.reset_or_grow(device) {
            dirty = true;
        }
        if need.ramp_full && self.ramp_atlas.reset_or_grow(device) {
            dirty = true;
        }
        if let Some((sw, sh)) = need.sprite_need {
            let (recreated, retry) = self.sprite_atlas.reset_or_grow(device, sw, sh);
            if recreated {
                dirty = true;
            }
            if !retry {
                can_retry = false;
            }
        }
        (dirty, can_retry)
    }

    /// 将当前帧 `ops` 直接渲染至目标 `target_view`：
    /// - 无图层/混合模式时走单 RenderPass / 单 Draw Call 直出快路径（0 离屏拷贝）；
    /// - 含 `PushLayer` / 混合 / 毛玻璃 / 滤镜时自动通过 `GpuEffectsEngine` 在单 `CommandEncoder` 内完成多 Pass 离屏合成。
    #[allow(clippy::too_many_arguments)]
    pub fn render_to_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        target_view: &wgpu::TextureView,
        phys_width: u32,
        phys_height: u32,
        clear_color: wgpu::Color,
    ) {
        self.render_to_view_with_damage_hints(
            device,
            queue,
            ops,
            viewport,
            scale_factor,
            target_view,
            phys_width,
            phys_height,
            clear_color,
            None,
        );
    }

    /// 面向 Swapchain 整帧清屏呈现的入口，同时接受可选的 `damage_hints` 脏区切片供多 Pass 毛玻璃
    /// 执行外扩采样框交集剪枝与模糊小纹理跨帧复用（绝不跳过 Swapchain 整屏清屏）。
    #[allow(clippy::too_many_arguments)]
    pub fn render_to_view_with_damage_hints(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        target_view: &wgpu::TextureView,
        phys_width: u32,
        phys_height: u32,
        clear_color: wgpu::Color,
        damage_hints: Option<&[Region]>,
    ) {
        if ops_have_layers_or_blend(ops) && !viewport.is_empty() {
            self.render_to_view_multi_pass(
                device,
                queue,
                ops,
                viewport,
                scale_factor,
                target_view,
                phys_width,
                phys_height,
                clear_color,
                damage_hints,
            );
            return;
        }

        if ops
            .iter()
            .any(|op| matches!(op.payload, DrawPayload::SceneCanvas(_)))
            && !viewport.is_empty()
        {
            self.render_to_view_with_scene_canvas(
                device,
                queue,
                ops,
                viewport,
                scale_factor,
                target_view,
                phys_width,
                phys_height,
                clear_color,
            );
            return;
        }

        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            match self.try_encode_ops(
                device,
                queue,
                ops,
                viewport,
                scale_factor,
                phys_width.max(1),
                phys_height.max(1),
            ) {
                Ok(()) => break,
                Err(need) => {
                    let (dirty, can_retry) = self.handle_atlas_grow(device, need);
                    if dirty {
                        bind_group_dirty = true;
                    }
                    if !can_retry {
                        break;
                    }
                }
            }
        }
        if let Some(slug) = &mut self.ui_slug_engine {
            slug.sync_and_finalize(device, queue);
        }

        if self.primitives.len() > self.instance_capacity {
            self.instance_capacity = self.primitives.len().next_power_of_two();
            self.instance_buf = Self::create_instance_buffer(device, self.instance_capacity);
            self.instance_buf_dirty_all = true;
            bind_group_dirty = true;
        }
        if bind_group_dirty {
            self.rebuild_bind_group(device);
        }

        let slot0 = AlignedViewportSlot {
            uniform: ViewportUniform {
                size: [phys_width.max(1) as f32, phys_height.max(1) as f32],
                is_srgb: if self.target_is_srgb { 1.0 } else { 0.0 },
                _pad: 0.0,
            },
            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
        };
        queue.write_buffer(&self.viewport_buf, 0, pod_as_bytes(&[slot0]));
        self.sync_instance_buffer(queue);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("velox_wgpu_render_encoder"),
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("velox_wgpu_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            if self.ui_slug_segments.is_empty() {
                if !self.primitives.is_empty() {
                    rpass.set_pipeline(&self.pipeline);
                    rpass.set_bind_group(0, &self.bind_group, &[0]);
                    rpass.draw(0..6, 0..(self.primitives.len() as u32));
                }
            } else {
                for &(p_start, p_end, s_start, s_end) in &self.ui_slug_segments {
                    if p_end > p_start {
                        rpass.set_pipeline(&self.pipeline);
                        rpass.set_bind_group(0, &self.bind_group, &[0]);
                        rpass.draw(0..6, p_start..p_end);
                    }
                    if s_end > s_start {
                        if let Some(slug) = &self.ui_slug_engine {
                            slug.draw_range(&mut rpass, s_start, s_end);
                        }
                    }
                }
            }
        }
        queue.submit([encoder.finish()]);
        if let Some(fx) = &mut self.effects {
            fx.tick_idle_frame();
        }
        if let Some(canvas) = &mut self.canvas_engine {
            canvas.tick_frame_reclaim();
        }
    }

    /// 面向持久离屏画布的 GPU 硬件脏区 Scissor 局部增量渲染（仅重绘 `dirty_regions` 覆盖的物理像素，保留未脏区域以压低 4K 功耗）。
    ///
    /// 若本帧包含复杂离屏图层/毛玻璃或 `dirty_regions` 为空，自动无缝回退至标准整帧 `render_to_view`。
    #[allow(clippy::too_many_arguments)]
    pub fn render_to_view_dirty(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        target_view: &wgpu::TextureView,
        phys_width: u32,
        phys_height: u32,
        dirty_regions: &[Region],
        clear_color: wgpu::Color,
    ) {
        if dirty_regions.is_empty() {
            self.render_to_view(
                device,
                queue,
                ops,
                viewport,
                scale_factor,
                target_view,
                phys_width,
                phys_height,
                clear_color,
            );
            return;
        }
        if crate::renderers::software::ops_have_layers_or_blend(ops) && !viewport.is_empty() {
            // 透传精确脏区列表供 `backdrop-filter` 执行外扩采样框交集跳过与模糊小纹理复用
            self.render_to_view_multi_pass(
                device,
                queue,
                ops,
                viewport,
                scale_factor,
                target_view,
                phys_width,
                phys_height,
                clear_color,
                Some(dirty_regions),
            );
            return;
        }
        if ops
            .iter()
            .any(|op| matches!(op.payload, DrawPayload::SceneCanvas(_)))
        {
            self.render_to_view(
                device,
                queue,
                ops,
                viewport,
                scale_factor,
                target_view,
                phys_width,
                phys_height,
                clear_color,
            );
            return;
        }
        let sf = scale_factor.clamp(0.5, 4.0);
        let pw = phys_width.max(1);
        let ph = phys_height.max(1);

        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            match self.try_encode_ops(device, queue, ops, viewport, sf, pw, ph) {
                Ok(()) => break,
                Err(need) => {
                    let (dirty, can_retry) = self.handle_atlas_grow(device, need);
                    if dirty {
                        bind_group_dirty = true;
                    }
                    if !can_retry {
                        break;
                    }
                }
            }
        }
        if let Some(slug) = &mut self.ui_slug_engine {
            slug.sync_and_finalize(device, queue);
        }
        if self.primitives.len() > self.instance_capacity {
            self.instance_capacity = self.primitives.len().next_power_of_two();
            self.instance_buf = Self::create_instance_buffer(device, self.instance_capacity);
            self.instance_buf_dirty_all = true;
            bind_group_dirty = true;
        }
        if bind_group_dirty {
            self.rebuild_bind_group(device);
        }

        let slot0 = AlignedViewportSlot {
            uniform: ViewportUniform {
                size: [pw as f32, ph as f32],
                is_srgb: if self.target_is_srgb { 1.0 } else { 0.0 },
                _pad: 0.0,
            },
            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
        };
        queue.write_buffer(&self.viewport_buf, 0, pod_as_bytes(&[slot0]));
        self.sync_instance_buffer(queue);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("velox_wgpu_dirty_scissor_encoder"),
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("velox_wgpu_dirty_scissor_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            if !self.primitives.is_empty() || !self.ui_slug_segments.is_empty() {
                for &d in dirty_regions {
                    let pd = snap_clip_region_scaled(d.intersect(viewport), sf);
                    let sx = (pd.x.max(0) as u32).min(pw);
                    let sy = (pd.y.max(0) as u32).min(ph);
                    let sw = (pd.w.max(0) as u32).min(pw.saturating_sub(sx));
                    let sh = (pd.h.max(0) as u32).min(ph.saturating_sub(sy));
                    if sw > 0 && sh > 0 {
                        rpass.set_scissor_rect(sx, sy, sw, sh);
                        if self.ui_slug_segments.is_empty() {
                            rpass.set_pipeline(&self.pipeline);
                            rpass.set_bind_group(0, &self.bind_group, &[0]);
                            rpass.draw(0..6, 0..(self.primitives.len() as u32));
                        } else {
                            for &(p_start, p_end, s_start, s_end) in &self.ui_slug_segments {
                                if p_end > p_start {
                                    rpass.set_pipeline(&self.pipeline);
                                    rpass.set_bind_group(0, &self.bind_group, &[0]);
                                    rpass.draw(0..6, p_start..p_end);
                                }
                                if s_end > s_start {
                                    if let Some(slug) = &self.ui_slug_engine {
                                        slug.draw_range(&mut rpass, s_start, s_end);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        queue.submit([encoder.finish()]);
        if let Some(fx) = &mut self.effects {
            fx.tick_idle_frame();
        }
        if let Some(canvas) = &mut self.canvas_engine {
            canvas.tick_frame_reclaim();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_to_view_multi_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        target_view: &wgpu::TextureView,
        phys_width: u32,
        phys_height: u32,
        clear_color: wgpu::Color,
        dirty_regions: Option<&[Region]>,
    ) {
        let pw = phys_width.max(1);
        let ph = phys_height.max(1);
        let sf = scale_factor.clamp(0.5, 4.0);
        let use_ttf = crate::display::ttf_atlas::has_system_font();
        let is_srgb = self.target_is_srgb;

        self.poll_warmed_subengines();
        if self.effects.is_none() {
            let raw_cache = self.pipeline_cache.as_deref().map(|c| c.wgpu_cache());
            self.effects = Some(GpuEffectsEngine::new(
                device,
                queue,
                self.target_format,
                raw_cache,
            ));
            if let Some(c) = &self.pipeline_cache {
                c.persist();
            }
        }

        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            match self.try_plan_multi_pass(
                device,
                queue,
                ops,
                viewport,
                sf,
                use_ttf,
                pw,
                ph,
                clear_color,
                is_srgb,
                dirty_regions,
            ) {
                Ok(()) => break,
                Err(need) => {
                    let (dirty, can_retry) = self.handle_atlas_grow(device, need);
                    if dirty {
                        bind_group_dirty = true;
                    }
                    if !can_retry {
                        break;
                    }
                }
            }
        }

        if let Some(slug) = &mut self.ui_slug_engine {
            slug.sync_and_finalize(device, queue);
        }

        if self.primitives.len() > self.instance_capacity {
            self.instance_capacity = self.primitives.len().next_power_of_two();
            self.instance_buf = Self::create_instance_buffer(device, self.instance_capacity);
            self.instance_buf_dirty_all = true;
            bind_group_dirty = true;
        }
        if self.viewport_slots.len() > self.viewport_capacity {
            self.viewport_capacity = self.viewport_slots.len().next_power_of_two();
            self.viewport_buf = Self::create_viewport_buffer(device, self.viewport_capacity);
            bind_group_dirty = true;
        }
        if bind_group_dirty {
            self.rebuild_bind_group(device);
        }

        if !self.viewport_slots.is_empty() {
            queue.write_buffer(&self.viewport_buf, 0, pod_as_bytes(&self.viewport_slots));
        }
        self.sync_instance_buffer(queue);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("velox_wgpu_multi_pass_encoder"),
        });
        let raw_cache = self.pipeline_cache.as_deref().map(|c| c.wgpu_cache());
        let had_canvas = self.canvas_engine.is_some();
        if let Some(ref mut fx) = self.effects {
            fx.execute_steps(
                device,
                queue,
                &mut encoder,
                &self.pipeline,
                &self.bind_group,
                &self.linear_sampler,
                &self.external_textures,
                ops,
                &mut self.canvas_engine,
                &mut self.ui_slug_engine,
                raw_cache,
                self.target_format,
                target_view,
            );
        }
        if !had_canvas && self.canvas_engine.is_some() {
            if let Some(c) = &self.pipeline_cache {
                c.persist();
            }
        }
        queue.submit([encoder.finish()]);
        if let Some(fx) = &mut self.effects {
            fx.end_frame_reclaim();
        }
        if let Some(canvas) = &mut self.canvas_engine {
            canvas.tick_frame_reclaim();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn try_plan_multi_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        sf: f32,
        use_ttf: bool,
        pw: u32,
        ph: u32,
        clear_color: wgpu::Color,
        is_srgb: bool,
        dirty_regions: Option<&[Region]>,
    ) -> Result<(), AtlasGrowNeed> {
        struct LayerFrame<'a> {
            tex_idx: usize,
            active_w: u32,
            active_h: u32,
            origin_x: f32,
            origin_y: f32,
            viewport_slot: u32,
            cleared: bool,
            clear_col: wgpu::Color,
            batch_start: u32,
            spec: Option<&'a LayerSpec<'a>>,
            body_phys: Region,
            buf_rect: Region,
            parent_clip_phys: Region,
        }

        self.primitives.clear();
        self.viewport_slots.clear();
        if let Some(slug) = &mut self.ui_slug_engine {
            slug.begin_frame();
        }
        let mut fx = self.effects.take().expect("effects engine initialized");
        fx.begin_frame();

        let root_idx = fx.pool.acquire(device, pw, ph);
        let root_tex_sz = (fx.pool.get(root_idx).width, fx.pool.get(root_idx).height);
        let root_vp_slot = 0u32;
        self.viewport_slots.push(AlignedViewportSlot {
            uniform: ViewportUniform {
                size: [root_tex_sz.0 as f32, root_tex_sz.1 as f32],
                is_srgb: if is_srgb { 1.0 } else { 0.0 },
                _pad: 0.0,
            },
            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
        });

        let mut stack: Vec<LayerFrame<'_>> = Vec::with_capacity(8);
        stack.push(LayerFrame {
            tex_idx: root_idx,
            active_w: pw,
            active_h: ph,
            origin_x: 0.0,
            origin_y: 0.0,
            viewport_slot: root_vp_slot,
            cleared: false,
            clear_col: clear_color,
            batch_start: 0,
            spec: None,
            body_phys: Region::new(0, 0, pw as i16, ph as i16),
            buf_rect: Region::new(0, 0, pw as i16, ph as i16),
            parent_clip_phys: Region::new(0, 0, pw as i16, ph as i16),
        });

        let flush_top = |stack: &mut [LayerFrame<'_>], fx: &mut GpuEffectsEngine, prim_len: u32| {
            let Some(top) = stack.last_mut() else {
                return;
            };
            if !top.cleared || prim_len > top.batch_start {
                let clear = if !top.cleared {
                    top.cleared = true;
                    Some(top.clear_col)
                } else {
                    None
                };
                fx.steps.push(GpuPassStep::DrawBatch {
                    target_idx: top.tex_idx,
                    clear,
                    viewport_slot: top.viewport_slot,
                    prim_start: top.batch_start,
                    prim_end: prim_len,
                });
                top.batch_start = prim_len;
            }
        };

        let result: Result<(), AtlasGrowNeed> = (|| {
            for (op_idx, op) in ops.iter().enumerate() {
                match op.payload {
                    DrawPayload::PushLayer(spec) => {
                        flush_top(&mut stack, &mut fx, self.primitives.len() as u32);
                        let parent = stack.last().unwrap();
                        let parent_idx = parent.tex_idx;
                        let parent_aw = parent.active_w;
                        let parent_ah = parent.active_h;
                        let parent_ox = parent.origin_x;
                        let parent_oy = parent.origin_y;
                        let parent_tex_sz = (fx.pool.get(parent_idx).width, fx.pool.get(parent_idx).height);

                        let body_phys = snap_bounds_region_scaled(spec.bounds, sf);
                        let parent_clip_phys = snap_clip_region_scaled(op.clip, sf);

                        // 1. 实时毛玻璃 `backdrop-filter`
                        if !spec.backdrop_filter.is_empty() && !body_phys.is_empty() {
                            let bd = spec.backdrop_filter;
                            let blur_px = (bd.blur_radius as f32) * sf;
                            let pad = (blur_px * 2.0).ceil() as i16;
                            let local_body = Region::new(
                                body_phys.x.saturating_sub(parent_ox as i16),
                                body_phys.y.saturating_sub(parent_oy as i16),
                                body_phys.w,
                                body_phys.h,
                            );
                            let raw_sample = Region::new(
                                local_body.x.saturating_sub(pad),
                                local_body.y.saturating_sub(pad),
                                local_body.w.saturating_add(pad.saturating_mul(2)),
                                local_body.h.saturating_add(pad.saturating_mul(2)),
                            );
                            let (sx, sy, sw, sh) = clamp_rect_u32(raw_sample, parent_aw, parent_ah);
                            if sw > 0 && sh > 0 {
                                // 脏矩形锥体剪枝（Dirty-Tile Blur Culling）：
                                // 仅当位于根图层、外扩采样区 `[sx, sy, sw, sh]` 与本帧所有脏区无交集，
                                // 且下方覆盖该采样区的背景图元特征哈希严格未变时，直接复用上一帧已模糊小纹理，跳过全部 Kawase Pass。
                                let bd_cache_key = super::layer_pool::BackdropCacheKey {
                                    sx,
                                    sy,
                                    sw,
                                    sh,
                                    blur_px_q4: (blur_px * 4.0).round().clamp(0.0, u16::MAX as f32) as u16,
                                    is_srgb,
                                };
                                let underlay_hash_opt = if stack.len() == 1 && blur_px >= 0.5 {
                                    let sample_logical = Region::new(
                                        ((sx as f32) / sf).floor().max(0.0) as i16,
                                        ((sy as f32) / sf).floor().max(0.0) as i16,
                                        ((sw as f32) / sf).ceil().max(1.0) as i16,
                                        ((sh as f32) / sf).ceil().max(1.0) as i16,
                                    );
                                    compute_backdrop_underlay_hash(&ops[..op_idx], sample_logical, clear_color)
                                } else {
                                    None
                                };
                                let can_reuse_backdrop = underlay_hash_opt.is_some()
                                    && dirty_regions.is_some_and(|dirty| {
                                        !dirty.is_empty()
                                            && !dirty_intersects_phys_rect(dirty, sx, sy, sw, sh, sf)
                                    });
                                let (bd_blurred, reused_cached_blur) = if can_reuse_backdrop {
                                    let uh = underlay_hash_opt.unwrap();
                                    if let Some(hit_idx) = fx.pool.lookup_backdrop(bd_cache_key, uh) {
                                        (hit_idx, true)
                                    } else {
                                        let bd_raw = fx.pool.acquire(device, sw, sh);
                                        fx.steps.push(GpuPassStep::CopyRect {
                                            src_idx: parent_idx,
                                            src_origin: (sx, sy),
                                            dst_idx: bd_raw,
                                            dst_origin: (0, 0),
                                            size: (sw, sh),
                                        });
                                        let b = fx.plan_kawase_blur(device, bd_raw, sw, sh, blur_px, is_srgb);
                                        if b != bd_raw {
                                            fx.pool.release(bd_raw);
                                        }
                                        (b, false)
                                    }
                                } else {
                                    let bd_raw = fx.pool.acquire(device, sw, sh);
                                    fx.steps.push(GpuPassStep::CopyRect {
                                        src_idx: parent_idx,
                                        src_origin: (sx, sy),
                                        dst_idx: bd_raw,
                                        dst_origin: (0, 0),
                                        size: (sw, sh),
                                    });
                                    let b = if blur_px >= 0.5 {
                                        let blurred = fx.plan_kawase_blur(device, bd_raw, sw, sh, blur_px, is_srgb);
                                        if blurred != bd_raw {
                                            fx.pool.release(bd_raw);
                                        }
                                        blurred
                                    } else {
                                        bd_raw
                                    };
                                    (b, false)
                                };

                                let dst_rect_bd = [
                                    local_body.x as f32,
                                    local_body.y as f32,
                                    local_body.w as f32,
                                    local_body.h as f32,
                                ];
                                let clip_rect_bd = [
                                    parent_clip_phys.x as f32 - parent_ox,
                                    parent_clip_phys.y as f32 - parent_oy,
                                    parent_clip_phys.right() as f32 - parent_ox,
                                    parent_clip_phys.bottom() as f32 - parent_oy,
                                ];
                                let (snap_idx, snap_ox, snap_oy, snap_tw, snap_th) = fx
                                    .acquire_subrect_snapshot(
                                        device,
                                        parent_idx,
                                        parent_aw,
                                        parent_ah,
                                        dst_rect_bd,
                                        clip_rect_bd,
                                    );

                                let bd_tex_sz = (fx.pool.get(bd_blurred).width, fx.pool.get(bd_blurred).height);
                                let mut u = EffectUniform::default();
                                u.tex_sizes = [
                                    parent_tex_sz.0 as f32,
                                    parent_tex_sz.1 as f32,
                                    bd_tex_sz.0 as f32,
                                    bd_tex_sz.1 as f32,
                                ];
                                u.dst_rect = dst_rect_bd;
                                u.src_rect = [sx as f32, sy as f32, sw as f32, sh as f32];
                                u.clip_rect = clip_rect_bd;
                                u.body_rect = u.dst_rect;
                                u.radii = [
                                    (spec.clip_radii.tl as f32) * sf,
                                    (spec.clip_radii.tr as f32) * sf,
                                    (spec.clip_radii.br as f32) * sf,
                                    (spec.clip_radii.bl as f32) * sf,
                                ];
                                u.params = [
                                    (spec.opacity as f32) * (1.0 / 255.0),
                                    0.0,
                                    EFFECT_KIND_BACKDROP,
                                    0.0,
                                ];
                                u.set_color_matrix(bd.color_matrix);
                                u.flags[1] = if spec.clip_path.is_some() { 1.0 } else { 0.0 };
                                u.flags[2] = (bd.noise_amount as f32) * (1.0 / 255.0) * 0.12;
                                u.flags[3] = if is_srgb { 1.0 } else { 0.0 };
                                u.tint = color_to_f32(bd.tint);
                                u.extra = [snap_ox, snap_oy, snap_tw, snap_th];

                                let slot = fx.push_slot(u);
                                let mask_path = spec.clip_path.map(|p| {
                                    (
                                        std::string::String::from(p.as_str()),
                                        local_body.w.max(1) as u32,
                                        local_body.h.max(1) as u32,
                                    )
                                });
                                fx.steps.push(GpuPassStep::Effect {
                                    dst_idx: parent_idx,
                                    clear_dst: false,
                                    dont_care_scissor: None,
                                    src_idx: bd_blurred,
                                    dst_snap_idx: snap_idx,
                                    mask_path,
                                    effect_slot: slot,
                                    use_hw_blend: false,
                                });
                                if !reused_cached_blur {
                                    if let Some(uh) = underlay_hash_opt {
                                        fx.pool.pin_backdrop(bd_blurred, bd_cache_key, uh);
                                    } else {
                                        fx.pool.release(bd_blurred);
                                    }
                                } else {
                                    fx.pool.release(bd_blurred);
                                }
                                fx.pool.release(snap_idx);
                            }
                        }

                        // 2. 为新隔离图层分配离屏目标并压栈
                        let (fl, ft, fr, fb) = spec.filter.outset();
                        let pl = ((fl as f32) * sf).ceil() as i16;
                        let pt = ((ft as f32) * sf).ceil() as i16;
                        let pr = ((fr as f32) * sf).ceil() as i16;
                        let pb = ((fb as f32) * sf).ceil() as i16;
                        let buf_rect = Region::new(
                            body_phys.x.saturating_sub(pl),
                            body_phys.y.saturating_sub(pt),
                            body_phys.w.saturating_add(pl).saturating_add(pr).max(1),
                            body_phys.h.saturating_add(pt).saturating_add(pb).max(1),
                        );
                        let lw = buf_rect.w.max(1) as u32;
                        let lh = buf_rect.h.max(1) as u32;
                        let layer_tex = fx.pool.acquire(device, lw, lh);
                        let ltex_sz = (fx.pool.get(layer_tex).width, fx.pool.get(layer_tex).height);
                        let vp_slot = self.viewport_slots.len() as u32;
                        self.viewport_slots.push(AlignedViewportSlot {
                            uniform: ViewportUniform {
                                size: [ltex_sz.0 as f32, ltex_sz.1 as f32],
                                is_srgb: if is_srgb { 1.0 } else { 0.0 },
                                _pad: 0.0,
                            },
                            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
                        });

                        stack.push(LayerFrame {
                            tex_idx: layer_tex,
                            active_w: lw,
                            active_h: lh,
                            origin_x: buf_rect.x as f32,
                            origin_y: buf_rect.y as f32,
                            viewport_slot: vp_slot,
                            cleared: false,
                            clear_col: wgpu::Color::TRANSPARENT,
                            batch_start: self.primitives.len() as u32,
                            spec: Some(spec),
                            body_phys,
                            buf_rect,
                            parent_clip_phys,
                        });
                    }

                    DrawPayload::PopLayer => {
                        if stack.len() <= 1 {
                            continue;
                        }
                        flush_top(&mut stack, &mut fx, self.primitives.len() as u32);
                        let layer = stack.pop().unwrap();
                        if let Some(parent_mut) = stack.last_mut() {
                            parent_mut.batch_start = self.primitives.len() as u32;
                        }
                        let Some(spec) = layer.spec else {
                            continue;
                        };
                        let parent = stack.last().unwrap();
                        let parent_idx = parent.tex_idx;
                        let parent_aw = parent.active_w;
                        let parent_ah = parent.active_h;
                        let parent_ox = parent.origin_x;
                        let parent_oy = parent.origin_y;
                        let parent_tex_sz = (fx.pool.get(parent_idx).width, fx.pool.get(parent_idx).height);

                        let local_buf = [
                            layer.buf_rect.x as f32 - parent_ox,
                            layer.buf_rect.y as f32 - parent_oy,
                            layer.buf_rect.w as f32,
                            layer.buf_rect.h as f32,
                        ];
                        let local_body = [
                            layer.body_phys.x as f32 - parent_ox,
                            layer.body_phys.y as f32 - parent_oy,
                            layer.body_phys.w as f32,
                            layer.body_phys.h as f32,
                        ];
                        let clip_f = [
                            layer.parent_clip_phys.x as f32 - parent_ox,
                            layer.parent_clip_phys.y as f32 - parent_oy,
                            layer.parent_clip_phys.right() as f32 - parent_ox,
                            layer.parent_clip_phys.bottom() as f32 - parent_oy,
                        ];

                        // 计算经 2D 仿射变换后的目标绘制矩形
                        let dst_rect = if let Some(mut t) = spec.transform {
                            if !t.is_identity() {
                                t.e *= sf;
                                t.f *= sf;
                                let cx = local_body[0] + local_body[2] * 0.5;
                                let cy = local_body[1] + local_body[3] * 0.5;
                                let xreg = t.centered_at(cx, cy).transform_region(Region::new(
                                    local_buf[0] as i16,
                                    local_buf[1] as i16,
                                    local_buf[2] as i16,
                                    local_buf[3] as i16,
                                ));
                                [xreg.x as f32, xreg.y as f32, xreg.w as f32, xreg.h as f32]
                            } else {
                                local_buf
                            }
                        } else {
                            local_buf
                        };

                        // 1. 若启用任意子树 `drop_shadow`：提取 Alpha（含 SVG Path 遮罩）-> Kawase Blur -> 变换偏移合成到父层
                        if let Some(ds) = spec.filter.drop_shadow {
                            if ds.is_active() {
                                let sh_raw = fx.pool.acquire(device, layer.active_w, layer.active_h);
                                let lsz = (fx.pool.get(layer.tex_idx).width, fx.pool.get(layer.tex_idx).height);
                                let sh_sz = (fx.pool.get(sh_raw).width, fx.pool.get(sh_raw).height);
                                let mut eu = EffectUniform::default();
                                eu.tex_sizes = [sh_sz.0 as f32, sh_sz.1 as f32, lsz.0 as f32, lsz.1 as f32];
                                eu.dst_rect = [0.0, 0.0, layer.active_w as f32, layer.active_h as f32];
                                eu.src_rect = eu.dst_rect;
                                eu.body_rect = [
                                    (layer.body_phys.x - layer.buf_rect.x) as f32,
                                    (layer.body_phys.y - layer.buf_rect.y) as f32,
                                    layer.body_phys.w as f32,
                                    layer.body_phys.h as f32,
                                ];
                                eu.params = [1.0, 0.0, EFFECT_KIND_SHADOW_EXTRACT, 0.0];
                                eu.flags[1] = if spec.clip_path.is_some() { 1.0 } else { 0.0 };
                                eu.flags[3] = if is_srgb { 1.0 } else { 0.0 };
                                eu.tint = color_to_f32(ds.color);
                                let eslot = fx.push_slot(eu);
                                let sh_mask = spec.clip_path.map(|p| {
                                    (
                                        std::string::String::from(p.as_str()),
                                        layer.body_phys.w.max(1) as u32,
                                        layer.body_phys.h.max(1) as u32,
                                    )
                                });
                                fx.steps.push(GpuPassStep::Effect {
                                    dst_idx: sh_raw,
                                    clear_dst: true,
                                    dont_care_scissor: None,
                                    src_idx: layer.tex_idx,
                                    dst_snap_idx: layer.tex_idx,
                                    mask_path: sh_mask,
                                    effect_slot: eslot,
                                    use_hw_blend: false,
                                });

                                let ds_blur = (ds.blur as f32) * sf;
                                let sh_final = if ds_blur >= 0.5 {
                                    let b = fx.plan_kawase_blur(
                                        device,
                                        sh_raw,
                                        layer.active_w,
                                        layer.active_h,
                                        ds_blur,
                                        is_srgb,
                                    );
                                    if b != sh_raw {
                                        fx.pool.release(sh_raw);
                                    }
                                    b
                                } else {
                                    sh_raw
                                };

                                let sh_tex_sz = (fx.pool.get(sh_final).width, fx.pool.get(sh_final).height);
                                let ox = (ds.offset_x as f32) * sf;
                                let oy = (ds.offset_y as f32) * sf;
                                let sh_dst_rect = [
                                    dst_rect[0] + ox,
                                    dst_rect[1] + oy,
                                    dst_rect[2],
                                    dst_rect[3],
                                ];
                                let sh_hw_blend = spec.blend_mode.is_normal();
                                let (sh_snap_opt, snap_ox, snap_oy, snap_tw, snap_th) = if sh_hw_blend {
                                    (None, 0.0, 0.0, 1.0, 1.0)
                                } else {
                                    let (sidx, sx0, sy0, stw, sth) = fx.acquire_subrect_snapshot(
                                        device,
                                        parent_idx,
                                        parent_aw,
                                        parent_ah,
                                        sh_dst_rect,
                                        clip_f,
                                    );
                                    (Some(sidx), sx0, sy0, stw, sth)
                                };

                                let mut cu = EffectUniform::default();
                                cu.tex_sizes = [
                                    parent_tex_sz.0 as f32,
                                    parent_tex_sz.1 as f32,
                                    sh_tex_sz.0 as f32,
                                    sh_tex_sz.1 as f32,
                                ];
                                cu.dst_rect = sh_dst_rect;
                                cu.src_rect = local_buf;
                                cu.clip_rect = clip_f;
                                cu.body_rect = local_body;
                                cu.params = [
                                    (spec.opacity as f32) * (1.0 / 255.0),
                                    if sh_hw_blend {
                                        -1.0
                                    } else {
                                        spec.blend_mode as u8 as f32
                                    },
                                    EFFECT_KIND_COMPOSITE,
                                    0.0,
                                ];
                                cu.set_color_matrix(None);
                                cu.set_transform(spec.transform, sf);
                                cu.tint = [snap_ox, snap_oy, 0.0, 0.0];
                                cu.extra[0] += ox;
                                cu.extra[1] += oy;
                                cu.extra[2] = snap_tw;
                                cu.extra[3] = snap_th;
                                let cslot = fx.push_slot(cu);
                                fx.steps.push(GpuPassStep::Effect {
                                    dst_idx: parent_idx,
                                    clear_dst: false,
                                    dont_care_scissor: None,
                                    src_idx: sh_final,
                                    dst_snap_idx: sh_snap_opt.unwrap_or(sh_final),
                                    mask_path: None,
                                    effect_slot: cslot,
                                    use_hw_blend: sh_hw_blend,
                                });
                                fx.pool.release(sh_final);
                                if let Some(sidx) = sh_snap_opt {
                                    fx.pool.release(sidx);
                                }
                            }
                        }

                        // 2. 图层本体模糊（Dual Kawase Blur）
                        let blur_px = (spec.filter.blur_radius as f32) * sf;
                        let src_final = if blur_px >= 0.5 {
                            let b = fx.plan_kawase_blur(
                                device,
                                layer.tex_idx,
                                layer.active_w,
                                layer.active_h,
                                blur_px,
                                is_srgb,
                            );
                            if b != layer.tex_idx {
                                fx.pool.release(layer.tex_idx);
                            }
                            b
                        } else {
                            layer.tex_idx
                        };

                        // 3. 合成回父层（SrcOver 走零拷贝硬件混合直出；其余 26 种混合模式走局部包围盒快照）
                        let use_hw_blend = spec.blend_mode.is_normal();
                        let (snap_opt, snap_ox, snap_oy, snap_tw, snap_th) = if use_hw_blend {
                            (None, 0.0, 0.0, 1.0, 1.0)
                        } else {
                            let (sidx, sx0, sy0, stw, sth) = fx.acquire_subrect_snapshot(
                                device,
                                parent_idx,
                                parent_aw,
                                parent_ah,
                                dst_rect,
                                clip_f,
                            );
                            (Some(sidx), sx0, sy0, stw, sth)
                        };
                        let src_tex_sz = (fx.pool.get(src_final).width, fx.pool.get(src_final).height);

                        let mut cu = EffectUniform::default();
                        cu.tex_sizes = [
                            parent_tex_sz.0 as f32,
                            parent_tex_sz.1 as f32,
                            src_tex_sz.0 as f32,
                            src_tex_sz.1 as f32,
                        ];
                        cu.dst_rect = dst_rect;
                        cu.src_rect = local_buf;
                        cu.clip_rect = clip_f;
                        cu.body_rect = local_body;
                        cu.radii = [
                            (spec.clip_radii.tl as f32) * sf,
                            (spec.clip_radii.tr as f32) * sf,
                            (spec.clip_radii.br as f32) * sf,
                            (spec.clip_radii.bl as f32) * sf,
                        ];
                        cu.params = [
                            (spec.opacity as f32) * (1.0 / 255.0),
                            if use_hw_blend {
                                -1.0
                            } else {
                                spec.blend_mode as u8 as f32
                            },
                            EFFECT_KIND_COMPOSITE,
                            0.0,
                        ];
                        cu.set_color_matrix(spec.filter.color_matrix);
                        cu.flags[1] = if spec.clip_path.is_some() { 1.0 } else { 0.0 };
                        cu.flags[3] = if is_srgb { 1.0 } else { 0.0 };
                        cu.set_transform(spec.transform, sf);
                        cu.tint = [snap_ox, snap_oy, 0.0, 0.0];
                        cu.extra[2] = snap_tw;
                        cu.extra[3] = snap_th;
                        let cslot = fx.push_slot(cu);

                        let mask_path = spec.clip_path.map(|p| {
                            (
                                std::string::String::from(p.as_str()),
                                layer.body_phys.w.max(1) as u32,
                                layer.body_phys.h.max(1) as u32,
                            )
                        });
                        fx.steps.push(GpuPassStep::Effect {
                            dst_idx: parent_idx,
                            clear_dst: false,
                            dont_care_scissor: None,
                            src_idx: src_final,
                            dst_snap_idx: snap_opt.unwrap_or(src_final),
                            mask_path,
                            effect_slot: cslot,
                            use_hw_blend,
                        });
                        fx.pool.release(src_final);
                        if let Some(sidx) = snap_opt {
                            fx.pool.release(sidx);
                        }
                    }

                    // 单图元非 `SrcOver` 混合：自动借用微型临时层执行精确 27 种 W3C 混合模式
                    DrawPayload::StyledQuad(sq) if !sq.blend_mode.is_normal() => {
                        flush_top(&mut stack, &mut fx, self.primitives.len() as u32);
                        let parent = stack.last().unwrap();
                        let parent_idx = parent.tex_idx;
                        let parent_aw = parent.active_w;
                        let parent_ah = parent.active_h;
                        let parent_ox = parent.origin_x;
                        let parent_oy = parent.origin_y;
                        let parent_tex_sz = (fx.pool.get(parent_idx).width, fx.pool.get(parent_idx).height);

                        let body_phys = snap_bounds_region_scaled(op.bounds, sf);
                        let buf_rect = if let Some(mut t) = sq.transform {
                            if !t.is_identity() {
                                t.e *= sf;
                                t.f *= sf;
                                let cx = body_phys.x as f32 + body_phys.w as f32 * 0.5;
                                let cy = body_phys.y as f32 + body_phys.h as f32 * 0.5;
                                t.centered_at(cx, cy).transform_region(body_phys)
                            } else {
                                body_phys
                            }
                        } else {
                            body_phys
                        };
                        let lw = buf_rect.w.max(1) as u32;
                        let lh = buf_rect.h.max(1) as u32;
                        let tmp_tex = fx.pool.acquire(device, lw, lh);
                        let tmp_sz = (fx.pool.get(tmp_tex).width, fx.pool.get(tmp_tex).height);
                        let vp_slot = self.viewport_slots.len() as u32;
                        self.viewport_slots.push(AlignedViewportSlot {
                            uniform: ViewportUniform {
                                size: [tmp_sz.0 as f32, tmp_sz.1 as f32],
                                is_srgb: if is_srgb { 1.0 } else { 0.0 },
                                _pad: 0.0,
                            },
                            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
                        });

                        let normal_sq = StyledQuad {
                            blend_mode: BlendMode::SrcOver,
                            ..*sq
                        };
                        let tmp_op = DrawOp {
                            payload: DrawPayload::StyledQuad(&normal_sq),
                            ..*op
                        };
                        let pstart = self.primitives.len() as u32;
                        let slug_before = self
                            .ui_slug_engine
                            .as_ref()
                            .map(|s| s.instance_count())
                            .unwrap_or(0);
                        self.encode_single_op(
                            device,
                            queue,
                            &tmp_op,
                            op.clip,
                            viewport,
                            sf,
                            use_ttf,
                            buf_rect.x as f32,
                            buf_rect.y as f32,
                            tmp_sz.0,
                            tmp_sz.1,
                        )?;
                        let pend = self.primitives.len() as u32;
                        let slug_after = self
                            .ui_slug_engine
                            .as_ref()
                            .map(|s| s.instance_count())
                            .unwrap_or(0);
                        fx.steps.push(GpuPassStep::DrawBatch {
                            target_idx: tmp_tex,
                            clear: Some(wgpu::Color::TRANSPARENT),
                            viewport_slot: vp_slot,
                            prim_start: pstart,
                            prim_end: pend,
                        });
                        if slug_after > slug_before {
                            fx.steps.push(GpuPassStep::DrawSlugBatch {
                                target_idx: tmp_tex,
                                clear: None,
                                slug_start: slug_before,
                                slug_end: slug_after,
                            });
                        }
                        if let Some(top) = stack.last_mut() {
                            top.batch_start = pend;
                        }

                        let clip_phys = snap_clip_region_scaled(op.clip, sf);
                        let local_buf = [
                            buf_rect.x as f32 - parent_ox,
                            buf_rect.y as f32 - parent_oy,
                            buf_rect.w as f32,
                            buf_rect.h as f32,
                        ];
                        let clip_f = [
                            clip_phys.x as f32 - parent_ox,
                            clip_phys.y as f32 - parent_oy,
                            clip_phys.right() as f32 - parent_ox,
                            clip_phys.bottom() as f32 - parent_oy,
                        ];
                        let (snap_idx, snap_ox, snap_oy, snap_tw, snap_th) = fx
                            .acquire_subrect_snapshot(
                                device,
                                parent_idx,
                                parent_aw,
                                parent_ah,
                                local_buf,
                                clip_f,
                            );
                        let mut cu = EffectUniform::default();
                        cu.tex_sizes = [
                            parent_tex_sz.0 as f32,
                            parent_tex_sz.1 as f32,
                            tmp_sz.0 as f32,
                            tmp_sz.1 as f32,
                        ];
                        cu.dst_rect = local_buf;
                        cu.src_rect = local_buf;
                        cu.clip_rect = clip_f;
                        cu.body_rect = local_buf;
                        cu.params = [1.0, sq.blend_mode as u8 as f32, EFFECT_KIND_COMPOSITE, 0.0];
                        cu.set_color_matrix(None);
                        cu.tint = [snap_ox, snap_oy, 0.0, 0.0];
                        cu.inv_xform = [1.0, 0.0, 0.0, 1.0];
                        cu.extra = [0.0, 0.0, snap_tw, snap_th];
                        let cslot = fx.push_slot(cu);
                        fx.steps.push(GpuPassStep::Effect {
                            dst_idx: parent_idx,
                            clear_dst: false,
                            dont_care_scissor: None,
                            src_idx: tmp_tex,
                            dst_snap_idx: snap_idx,
                            mask_path: None,
                            effect_slot: cslot,
                            use_hw_blend: false,
                        });
                        fx.pool.release(tmp_tex);
                        fx.pool.release(snap_idx);
                    }

                    // 外部零拷贝 GPU 纹理采样（直接消费外部引擎共享的 `wgpu::TextureView`）
                    DrawPayload::ExternalTexture(ext) => {
                        let raw_clip = viewport.intersect(op.clip).intersect(op.bounds);
                        if raw_clip.is_empty() {
                            continue;
                        }
                        if self.external_textures.contains_key(&ext.id) {
                            flush_top(&mut stack, &mut fx, self.primitives.len() as u32);
                            let parent = stack.last().unwrap();
                            let parent_idx = parent.tex_idx;
                            let parent_aw = parent.active_w;
                            let parent_ah = parent.active_h;
                            let parent_ox = parent.origin_x;
                            let parent_oy = parent.origin_y;
                            let parent_tex_sz =
                                (fx.pool.get(parent_idx).width, fx.pool.get(parent_idx).height);

                            let body_phys = snap_bounds_region_scaled(op.bounds, sf);
                            let clip_phys = snap_clip_region_scaled(raw_clip, sf);
                            let local_body = [
                                body_phys.x as f32 - parent_ox,
                                body_phys.y as f32 - parent_oy,
                                body_phys.w as f32,
                                body_phys.h as f32,
                            ];
                            let clip_f = [
                                clip_phys.x as f32 - parent_ox,
                                clip_phys.y as f32 - parent_oy,
                                clip_phys.right() as f32 - parent_ox,
                                clip_phys.bottom() as f32 - parent_oy,
                            ];

                            let use_hw_blend = ext.blend_mode.is_normal();
                            let (snap_opt, snap_ox, snap_oy, snap_tw, snap_th) = if use_hw_blend {
                                (None, 0.0, 0.0, 1.0, 1.0)
                            } else {
                                let (sidx, sx0, sy0, stw, sth) = fx.acquire_subrect_snapshot(
                                    device,
                                    parent_idx,
                                    parent_aw,
                                    parent_ah,
                                    local_body,
                                    clip_f,
                                );
                                (Some(sidx), sx0, sy0, stw, sth)
                            };

                            let mut eu = EffectUniform::default();
                            eu.tex_sizes = [
                                parent_tex_sz.0 as f32,
                                parent_tex_sz.1 as f32,
                                1.0,
                                1.0,
                            ];
                            eu.dst_rect = local_body;
                            eu.src_rect = ext.uv_rect;
                            eu.clip_rect = clip_f;
                            eu.body_rect = local_body;
                            eu.radii = [
                                (ext.radii.tl as f32) * sf,
                                (ext.radii.tr as f32) * sf,
                                (ext.radii.br as f32) * sf,
                                (ext.radii.bl as f32) * sf,
                            ];
                            eu.params = [
                                (ext.opacity as f32) * (1.0 / 255.0),
                                if use_hw_blend {
                                    -1.0
                                } else {
                                    ext.blend_mode as u8 as f32
                                },
                                EFFECT_KIND_EXTERNAL_TEX,
                                0.0,
                            ];
                            eu.flags = [
                                if ext.premultiplied { 1.0 } else { 0.0 },
                                0.0,
                                0.0,
                                if is_srgb { 1.0 } else { 0.0 },
                            ];
                            eu.inv_xform = [1.0, 0.0, 0.0, 1.0];
                            eu.extra = [snap_ox, snap_oy, snap_tw, snap_th];
                            let eslot = fx.push_slot(eu);

                            fx.steps.push(GpuPassStep::DrawExternal {
                                dst_idx: parent_idx,
                                dst_snap_idx: snap_opt.unwrap_or(root_idx),
                                tex_id: ext.id,
                                effect_slot: eslot,
                                use_hw_blend,
                            });
                            if let Some(sidx) = snap_opt {
                                fx.pool.release(sidx);
                            }
                        } else if ext.fallback_bg.a() > 0 {
                            let fallback_sq = StyledQuad {
                                paint: Paint::Solid(ext.fallback_bg),
                                border_color: Color::TRANSPARENT,
                                border_width: 0,
                                radii: ext.radii,
                                opacity: ext.opacity,
                                blend_mode: BlendMode::SrcOver,
                                transform: None,
                            };
                            let tmp_op = DrawOp {
                                payload: DrawPayload::StyledQuad(&fallback_sq),
                                ..*op
                            };
                            let (ox, oy, tw, th) = {
                                let top = stack.last().unwrap();
                                let t = fx.pool.get(top.tex_idx);
                                (top.origin_x, top.origin_y, t.width, t.height)
                            };
                            self.encode_single_op(
                                device, queue, &tmp_op, raw_clip, viewport, sf, use_ttf, ox, oy, tw, th,
                            )?;
                        }
                    }

                    // 含图层/毛玻璃页内的 `SceneCanvas` 依然原地走原生硬件 `CanvasGpuEngine` 实例化管线（0 CPU 光栅、0 大图集拷贝）
                    DrawPayload::SceneCanvas(spec) => {
                        let raw_clip = viewport.intersect(op.clip).intersect(op.bounds);
                        if raw_clip.is_empty() {
                            continue;
                        }
                        let (parent_idx, ox, oy) = {
                            let top = stack.last().unwrap();
                            (top.tex_idx, top.origin_x, top.origin_y)
                        };
                        let raw_b = snap_bounds(op.bounds, sf);
                        let raw_c = snap_clip(raw_clip, sf);
                        let bounds_phys = [raw_b[0] - ox, raw_b[1] - oy, raw_b[2], raw_b[3]];
                        let clip_phys = [raw_c[0] - ox, raw_c[1] - oy, raw_c[2] - ox, raw_c[3] - oy];
                        let raw_sc = snap_clip_region_scaled(raw_clip, sf);
                        let scissor = Region::new(
                            raw_sc.x.saturating_sub(ox.round() as i16),
                            raw_sc.y.saturating_sub(oy.round() as i16),
                            raw_sc.w,
                            raw_sc.h,
                        );
                        if spec.bg.a() > 0 && spec.opacity > 0 {
                            let bg = spec.bg.scale_alpha(spec.opacity);
                            self.primitives.push(make_quad_primitive(
                                bounds_phys,
                                clip_phys,
                                (op.clip_radius as f32) * sf,
                                0.0,
                                0.0,
                                false,
                                bg,
                                Color::TRANSPARENT,
                                Color::TRANSPARENT,
                            ));
                        }
                        flush_top(&mut stack, &mut fx, self.primitives.len() as u32);
                        fx.steps.push(GpuPassStep::DrawSceneCanvas {
                            target_idx: parent_idx,
                            op_idx,
                            bounds_phys,
                            clip_phys,
                            scissor,
                            sf,
                            is_srgb,
                        });
                        self.encode_canvas_overlay_primitives(
                            queue,
                            spec,
                            bounds_phys,
                            clip_phys,
                            (op.clip_radius as f32) * sf,
                            sf,
                        )?;
                    }

                    _ => {
                        let raw_clip = match op.payload {
                            DrawPayload::StyledQuad(sq) if sq.transform.is_some() => {
                                viewport.intersect(op.clip)
                            }
                            _ => viewport.intersect(op.clip).intersect(op.bounds),
                        };
                        if raw_clip.is_empty() {
                            continue;
                        }
                        let (ox, oy, tw, th) = {
                            let top = stack.last().unwrap();
                            let t = fx.pool.get(top.tex_idx);
                            (top.origin_x, top.origin_y, t.width, t.height)
                        };
                        let prim_before = self.primitives.len() as u32;
                        let slug_before = self
                            .ui_slug_engine
                            .as_ref()
                            .map(|s| s.instance_count())
                            .unwrap_or(0);
                        self.encode_single_op(
                            device, queue, op, raw_clip, viewport, sf, use_ttf, ox, oy, tw, th,
                        )?;
                        let slug_after = self
                            .ui_slug_engine
                            .as_ref()
                            .map(|s| s.instance_count())
                            .unwrap_or(0);
                        if slug_after > slug_before {
                            flush_top(&mut stack, &mut fx, prim_before);
                            let top = stack.last_mut().unwrap();
                            let clear = if !top.cleared {
                                top.cleared = true;
                                Some(top.clear_col)
                            } else {
                                None
                            };
                            fx.steps.push(GpuPassStep::DrawSlugBatch {
                                target_idx: top.tex_idx,
                                clear,
                                slug_start: slug_before,
                                slug_end: slug_after,
                            });
                        }
                    }
                }
            }

            // 冲刷根画布最后一批图元并 Blit 至外部 `target_view`
            flush_top(&mut stack, &mut fx, self.primitives.len() as u32);
            let mut blit_u = EffectUniform::default();
            blit_u.tex_sizes = [pw as f32, ph as f32, root_tex_sz.0 as f32, root_tex_sz.1 as f32];
            blit_u.dst_rect = [0.0, 0.0, pw as f32, ph as f32];
            blit_u.src_rect = [0.0, 0.0, pw as f32, ph as f32];
            blit_u.params = [1.0, 0.0, EFFECT_KIND_BLIT, 0.0];
            let blit_slot = fx.push_slot(blit_u);
            fx.steps.push(GpuPassStep::BlitToTarget {
                src_idx: root_idx,
                effect_slot: blit_slot,
            });
            Ok(())
        })();

        self.effects = Some(fx);
        result
    }

    /// 针对包含 `DrawPayload::SceneCanvas` 的无图层帧执行零离屏拷贝交错渲染：
    /// 画布前 UI → 背景填充 → `CanvasGpuEngine`（bg_shaders + StaticMesh + 32B+4B Instanced SDF + fg_shaders）
    /// → ASCII 快速字形标签（含 Zoom Font Lock）+ 橡皮筋框选 → 画布后浮层 UI（Tooltip / Minimap）。
    #[allow(clippy::too_many_arguments)]
    fn render_to_view_with_scene_canvas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
        target_view: &wgpu::TextureView,
        phys_width: u32,
        phys_height: u32,
        clear_color: wgpu::Color,
    ) {
        enum InterleavedStep {
            UiBatch(u32, u32),
            UiSlugBatch(u32, u32),
            Canvas {
                op_idx: usize,
                bounds_phys: [f32; 4],
                clip_phys: [f32; 4],
                scissor: Region,
            },
        }

        let sf = scale_factor.clamp(0.5, 4.0);
        let pw = phys_width.max(1);
        let ph = phys_height.max(1);
        let use_ttf = crate::display::ttf_atlas::has_system_font();
        let mut steps: Vec<InterleavedStep> = Vec::with_capacity(8);

        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            self.primitives.clear();
            steps.clear();
            if let Some(slug) = &mut self.ui_slug_engine {
                slug.begin_frame();
            }
            let mut batch_start = 0u32;
            let mut err_need: Option<AtlasGrowNeed> = None;

            for (op_idx, op) in ops.iter().enumerate() {
                let raw_clip = match op.payload {
                    DrawPayload::StyledQuad(sq) if sq.transform.is_some() => {
                        viewport.intersect(op.clip)
                    }
                    _ => viewport.intersect(op.clip).intersect(op.bounds),
                };
                if raw_clip.is_empty() {
                    continue;
                }
                if let DrawPayload::SceneCanvas(spec) = op.payload {
                    let bounds_phys = snap_bounds(op.bounds, sf);
                    let clip_phys = snap_clip(raw_clip, sf);
                    let scissor = snap_clip_region_scaled(raw_clip, sf);

                    // 1. 先向 UI 批次推入画布纯色背景（位于 CustomShader / StaticMesh 下方）
                    if spec.bg.a() > 0 && spec.opacity > 0 {
                        let bg = spec.bg.scale_alpha(spec.opacity);
                        self.primitives.push(make_quad_primitive(
                            bounds_phys,
                            clip_phys,
                            (op.clip_radius as f32) * sf,
                            0.0,
                            0.0,
                            false,
                            bg,
                            Color::TRANSPARENT,
                            Color::TRANSPARENT,
                        ));
                    }
                    let batch_end = self.primitives.len() as u32;
                    if batch_end > batch_start {
                        steps.push(InterleavedStep::UiBatch(batch_start, batch_end));
                        batch_start = batch_end;
                    }

                    // 2. 插入画布 GPU Instanced + Mesh + CustomShader Pass
                    steps.push(InterleavedStep::Canvas {
                        op_idx,
                        bounds_phys,
                        clip_phys,
                        scissor,
                    });

                    // 3. 画布上方编码 ASCII 快速字形标签（Zoom Font Lock）与橡皮筋框选
                    if let Err(need) = self.encode_canvas_overlay_primitives(
                        queue,
                        spec,
                        bounds_phys,
                        clip_phys,
                        (op.clip_radius as f32) * sf,
                        sf,
                    ) {
                        err_need = Some(need);
                        break;
                    }
                } else {
                    let prim_before = self.primitives.len() as u32;
                    let slug_before = self
                        .ui_slug_engine
                        .as_ref()
                        .map(|s| s.instance_count())
                        .unwrap_or(0);
                    if let Err(need) = self.encode_single_op(
                        device, queue, op, raw_clip, viewport, sf, use_ttf, 0.0, 0.0, pw, ph,
                    ) {
                        err_need = Some(need);
                        break;
                    }
                    let slug_after = self
                        .ui_slug_engine
                        .as_ref()
                        .map(|s| s.instance_count())
                        .unwrap_or(0);
                    if slug_after > slug_before {
                        if prim_before > batch_start {
                            steps.push(InterleavedStep::UiBatch(batch_start, prim_before));
                        }
                        steps.push(InterleavedStep::UiSlugBatch(slug_before, slug_after));
                        batch_start = prim_before;
                    }
                }
            }

            if let Some(need) = err_need {
                let (dirty, can_retry) = self.handle_atlas_grow(device, need);
                if dirty {
                    bind_group_dirty = true;
                }
                if !can_retry {
                    break;
                }
            } else {
                let final_end = self.primitives.len() as u32;
                if final_end > batch_start {
                    steps.push(InterleavedStep::UiBatch(batch_start, final_end));
                }
                break;
            }
        }
        if let Some(slug) = &mut self.ui_slug_engine {
            slug.sync_and_finalize(device, queue);
        }

        if self.primitives.len() > self.instance_capacity {
            self.instance_capacity = self.primitives.len().next_power_of_two();
            self.instance_buf = Self::create_instance_buffer(device, self.instance_capacity);
            self.instance_buf_dirty_all = true;
            bind_group_dirty = true;
        }
        if bind_group_dirty {
            self.rebuild_bind_group(device);
        }

        let slot0 = AlignedViewportSlot {
            uniform: ViewportUniform {
                size: [pw as f32, ph as f32],
                is_srgb: if self.target_is_srgb { 1.0 } else { 0.0 },
                _pad: 0.0,
            },
            _pad: [0u8; 256 - core::mem::size_of::<ViewportUniform>()],
        };
        queue.write_buffer(&self.viewport_buf, 0, pod_as_bytes(&[slot0]));
        self.sync_instance_buffer(queue);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("velox_wgpu_scene_canvas_encoder"),
        });

        let mut cleared = false;
        let is_srgb = self.target_is_srgb;
        self.poll_warmed_subengines();
        let raw_cache = self.pipeline_cache.as_deref().map(|c| c.wgpu_cache());
        let had_canvas = self.canvas_engine.is_some();
        let mut canvas_eng = self.canvas_engine.take().unwrap_or_else(|| {
            super::canvas_pipeline::CanvasGpuEngine::new(device, self.target_format, raw_cache)
        });
        if !had_canvas {
            if let Some(c) = &self.pipeline_cache {
                c.persist();
            }
        }

        for step in &steps {
            match *step {
                InterleavedStep::UiBatch(start, end) => {
                    let load = if !cleared {
                        cleared = true;
                        wgpu::LoadOp::Clear(clear_color)
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_canvas_ui_batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    });
                    if end > start {
                        rpass.set_pipeline(&self.pipeline);
                        rpass.set_bind_group(0, &self.bind_group, &[0]);
                        rpass.draw(0..6, start..end);
                    }
                }
                InterleavedStep::UiSlugBatch(start, end) => {
                    let load = if !cleared {
                        cleared = true;
                        wgpu::LoadOp::Clear(clear_color)
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_canvas_ui_slug_batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    });
                    if let Some(slug) = &self.ui_slug_engine {
                        slug.draw_range(&mut rpass, start, end);
                    }
                }
                InterleavedStep::Canvas {
                    op_idx,
                    bounds_phys,
                    clip_phys,
                    scissor,
                } => {
                    if !cleared {
                        cleared = true;
                        let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("velox_canvas_clear"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: target_view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(clear_color),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            occlusion_query_set: None,
                            timestamp_writes: None,
                            multiview_mask: None,
                        });
                    }
                    if let DrawPayload::SceneCanvas(spec) = ops[op_idx].payload {
                        let slug_eng = self.ui_slug_engine.get_or_insert_with(|| {
                            super::slug_glyph::SlugTextEngine::new(
                                device,
                                self.target_format,
                                raw_cache,
                            )
                        });
                        canvas_eng.render_canvas(
                            device,
                            queue,
                            &mut encoder,
                            target_view,
                            pw,
                            ph,
                            bounds_phys,
                            clip_phys,
                            scissor,
                            sf,
                            is_srgb,
                            spec,
                            slug_eng,
                        );
                    }
                }
            }
        }

        canvas_eng.tick_frame_reclaim();
        self.canvas_engine = Some(canvas_eng);
        queue.submit([encoder.finish()]);
        if let Some(fx) = &mut self.effects {
            fx.tick_idle_frame();
        }
    }

    /// 编码画布近景 ASCII 快速标签（手势缩放时自动锁定 16px/32px 离散基准字号 + `O(1)` 查表防图集抖动）与橡皮筋框选矩形。
    fn encode_canvas_overlay_primitives(
        &mut self,
        queue: &wgpu::Queue,
        spec: &crate::canvas_scene::SceneCanvasSpec<'_>,
        bounds_phys: [f32; 4],
        clip_phys: [f32; 4],
        clip_radius_px: f32,
        sf: f32,
    ) -> Result<(), AtlasGrowNeed> {
        let vp_zoom = spec.viewport.zoom * sf;
        let vp_ox = bounds_phys[0] + spec.viewport.offset_x * sf;
        let vp_oy = bounds_phys[1] + spec.viewport.offset_y * sf;

        // 1. 存在系统 TrueType 字体时，所有画布世界标签已由 `CanvasGpuEngine` 内的 Slug GPU 二次贝塞尔曲线管线
        //    在同一 RenderPass 中无限清晰直出（零图集、零缩放重光栅化）；仅在无 TTF 的极简环境中走内置位图兜底。
        if !crate::display::ttf_atlas::has_system_font() {
            for lbl in spec.labels {
            if lbl.text.is_empty() {
                continue;
            }
            let target_px = (lbl.font_px * sf).clamp(6.0, 48.0);
            let col_f32 = color_to_f32(lbl.color.scale_alpha(spec.opacity));
            let center_x = lbl.wx * vp_zoom + vp_ox;
            let center_y = lbl.wy * vp_zoom + vp_oy;

            // Zoom Font Lock：缩放态锁定 16px / 32px 离散基准字号并顶点缩放纹理，手势停稳 120ms 后恢复 0.25px 精确光栅化
            let use_fast_lock = spec.zooming || lbl.text.is_ascii();
            if use_fast_lock && lbl.text.is_ascii() {
                let (large_32, base_px, q4_exact) = if spec.zooming {
                    let large = target_px > 20.0;
                    (large, if large { 32.0 } else { 16.0 }, 0u16)
                } else {
                    let q4 = (target_px * 4.0).round().clamp(24.0, 192.0) as u16;
                    (false, target_px, q4)
                };
                let ratio = if spec.zooming {
                    target_px / base_px
                } else {
                    1.0
                };
                let mut total_adv = 0.0f32;
                let mut cached_entries: [Option<super::atlas::GlyphAtlasEntry>; 16] = [None; 16];
                let bytes = lbl.text.as_bytes();
                let count = bytes.len().min(16);
                for i in 0..count {
                    let b = bytes[i];
                    let entry = if spec.zooming {
                        self.glyph_atlas.get_ascii_fast(queue, b, large_32)
                    } else {
                        self.glyph_atlas.get_or_insert(queue, b as char, q4_exact, true)
                    };
                    let Some(e) = entry else {
                        return Err(AtlasGrowNeed {
                            glyph_full: true,
                            sprite_need: None,
                            ramp_full: false,
                        });
                    };
                    total_adv += (e.advance.max(4) as f32) * ratio;
                    cached_entries[i] = Some(e);
                }
                let mut pen_x = center_x - total_adv * 0.5;
                let baseline_y = center_y + target_px * 0.34;
                for i in 0..count {
                    if let Some(e) = cached_entries[i] {
                        if e.width > 0 && e.height > 0 {
                            let gw = (e.width as f32) * ratio;
                            let gh = (e.height as f32) * ratio;
                            let gx = pen_x + (e.xmin as f32) * ratio;
                            let gy = baseline_y - gh - (e.ymin as f32) * ratio;
                            if gx + gw >= clip_phys[0]
                                && gx <= clip_phys[2]
                                && gy + gh >= clip_phys[1]
                                && gy <= clip_phys[3]
                            {
                                self.primitives.push(GpuPrimitive {
                                    bounds: [gx, gy, gw, gh],
                                    clip: clip_phys,
                                    uv: e.uv,
                                    color0: col_f32,
                                    color1: [0.0; 4],
                                    border_color: [0.0; 4],
                                    params: [0.0, 0.0, KIND_GLYPH, 0.0],
                                    extra: [clip_radius_px, 0.0, 0.0, 0.0],
                                    radii: [0.0; 4],
                                    grad_a: [0.0; 4],
                                    grad_b: [0.0; 4],
                                    xform: IDENTITY_XFORM,
                                });
                            }
                        }
                        pen_x += (e.advance.max(4) as f32) * ratio;
                    }
                }
            }
            }
        }

        // 2. 橡皮筋框选矩形
        if let Some([mx0, my0, mx1, my1]) = spec.marquee {
            let x0 = bounds_phys[0] + mx0.min(mx1) * sf;
            let y0 = bounds_phys[1] + my0.min(my1) * sf;
            let w = (mx1 - mx0).abs() * sf;
            let h = (my1 - my0).abs() * sf;
            if w > 1.0 && h > 1.0 {
                self.primitives.push(make_quad_primitive(
                    [x0, y0, w, h],
                    clip_phys,
                    clip_radius_px,
                    2.0 * sf,
                    1.5 * sf,
                    false,
                    spec.marquee_color.scale_alpha(52),
                    Color::TRANSPARENT,
                    spec.marquee_color.scale_alpha(220),
                ));
            }
        }
        Ok(())
    }
}
