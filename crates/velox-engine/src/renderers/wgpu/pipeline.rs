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
}

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
    external_textures: HashMap<ExternalTextureId, ExternalTextureEntry>,
    next_external_id: u32,
    target_format: wgpu::TextureFormat,
    primitives: Vec<GpuPrimitive>,
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
            cache: None,
        });
        let t_pso = t0.elapsed();
        if trace {
            std::println!(
                "[VELOX_STARTUP_TRACE]   pipeline detail: create_shader_module={:.1}ms, create_render_pipeline=+{:.1}ms",
                t_shader.as_secs_f64() * 1000.0,
                (t_pso - t_shader).as_secs_f64() * 1000.0,
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
            external_textures: HashMap::new(),
            next_external_id: 1,
            target_format,
            primitives: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
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
        let tex_id = ExternalTextureId(id);
        self.external_textures
            .insert(tex_id, ExternalTextureEntry { view, sampler });
        tex_id
    }

    /// 按显式 ID 注册或替换外部零拷贝 GPU 纹理（便于固定槽位号与跨语言 FFI 绑定）。
    pub fn register_external_texture_with_id(
        &mut self,
        id: ExternalTextureId,
        view: wgpu::TextureView,
        sampler: Option<wgpu::Sampler>,
    ) {
        self.external_textures
            .insert(id, ExternalTextureEntry { view, sampler });
    }

    /// 原地更新已注册外部纹理的 `wgpu::TextureView`（如外部引擎 Resize 或双缓冲翻页时调用）。
    pub fn update_external_texture(&mut self, id: ExternalTextureId, view: wgpu::TextureView) -> bool {
        if let Some(entry) = self.external_textures.get_mut(&id) {
            entry.view = view;
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

    /// 将单条非图层 `DrawOp` 编码追加到 `self.primitives`，并将所有屏幕坐标减去 `(origin_x, origin_y)`。
    #[allow(clippy::too_many_arguments)]
    fn encode_single_op(
        &mut self,
        queue: &wgpu::Queue,
        op: &DrawOp<'_>,
        eff_clip: Region,
        viewport: Region,
        sf: f32,
        use_ttf: bool,
        origin_x: f32,
        origin_y: f32,
    ) -> Result<(), AtlasGrowNeed> {
        let mut sb_f = snap_bounds(op.bounds, sf);
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
            DrawPayload::PushLayer(_)
            | DrawPayload::PopLayer
            | DrawPayload::ExternalTexture(_) => {}
        }
        Ok(())
    }

    /// 单 Pass 无图层快路径：将 `DrawOp` 编码为 `self.primitives` 并按需上传字形、精灵与渐变图集。
    fn try_encode_ops(
        &mut self,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
    ) -> Result<(), AtlasGrowNeed> {
        self.primitives.clear();
        if viewport.is_empty() {
            return Ok(());
        }

        let sf = scale_factor.clamp(0.5, 4.0);
        let use_ttf = crate::display::ttf_atlas::has_system_font();

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
            self.encode_single_op(queue, op, eff_clip, viewport, sf, use_ttf, 0.0, 0.0)?;
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
    ) -> bool {
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
        let col_f32 = color_to_f32(color);

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
                    self.primitives.push(GpuPrimitive {
                        bounds: [
                            gx as f32,
                            gy as f32,
                            entry.width as f32,
                            entry.height as f32,
                        ],
                        clip: clip_f32,
                        uv: entry.uv,
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
            );
            return;
        }

        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            match self.try_encode_ops(queue, ops, viewport, scale_factor) {
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

        if self.primitives.len() > self.instance_capacity {
            self.instance_capacity = self.primitives.len().next_power_of_two();
            self.instance_buf = Self::create_instance_buffer(device, self.instance_capacity);
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

        if !self.primitives.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, pod_as_bytes(&self.primitives));
        }

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
            if !self.primitives.is_empty() {
                rpass.set_pipeline(&self.pipeline);
                rpass.set_bind_group(0, &self.bind_group, &[0]);
                rpass.draw(0..6, 0..(self.primitives.len() as u32));
            }
        }
        queue.submit([encoder.finish()]);
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
    ) {
        let pw = phys_width.max(1);
        let ph = phys_height.max(1);
        let sf = scale_factor.clamp(0.5, 4.0);
        let use_ttf = crate::display::ttf_atlas::has_system_font();
        let is_srgb = self.target_is_srgb;

        if self.effects.is_none() {
            self.effects = Some(GpuEffectsEngine::new(device, queue, self.target_format));
        }

        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            match self.try_plan_multi_pass(device, queue, ops, viewport, sf, use_ttf, pw, ph, clear_color, is_srgb) {
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

        if self.primitives.len() > self.instance_capacity {
            self.instance_capacity = self.primitives.len().next_power_of_two();
            self.instance_buf = Self::create_instance_buffer(device, self.instance_capacity);
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
        if !self.primitives.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, pod_as_bytes(&self.primitives));
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("velox_wgpu_multi_pass_encoder"),
        });
        if let Some(ref mut fx) = self.effects {
            fx.execute_steps(
                device,
                queue,
                &mut encoder,
                &self.pipeline,
                &self.bind_group,
                &self.linear_sampler,
                &self.external_textures,
                target_view,
            );
        }
        queue.submit([encoder.finish()]);
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
            for op in ops {
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
                                let bd_raw = fx.pool.acquire(device, sw, sh);
                                fx.steps.push(GpuPassStep::CopyRect {
                                    src_idx: parent_idx,
                                    src_origin: (sx, sy),
                                    dst_idx: bd_raw,
                                    dst_origin: (0, 0),
                                    size: (sw, sh),
                                });
                                let bd_blurred = if blur_px >= 0.5 {
                                    let b = fx.plan_kawase_blur(device, bd_raw, sw, sh, blur_px, is_srgb);
                                    if b != bd_raw {
                                        fx.pool.release(bd_raw);
                                    }
                                    b
                                } else {
                                    bd_raw
                                };

                                let snap_idx = fx.pool.acquire(device, parent_aw, parent_ah);
                                fx.steps.push(GpuPassStep::CopyRect {
                                    src_idx: parent_idx,
                                    src_origin: (0, 0),
                                    dst_idx: snap_idx,
                                    dst_origin: (0, 0),
                                    size: (parent_aw, parent_ah),
                                });

                                let bd_tex_sz = (fx.pool.get(bd_blurred).width, fx.pool.get(bd_blurred).height);
                                let snap_tex_sz = (fx.pool.get(snap_idx).width, fx.pool.get(snap_idx).height);
                                let mut u = EffectUniform::default();
                                u.tex_sizes = [
                                    parent_tex_sz.0 as f32,
                                    parent_tex_sz.1 as f32,
                                    bd_tex_sz.0 as f32,
                                    bd_tex_sz.1 as f32,
                                ];
                                u.dst_rect = [
                                    local_body.x as f32,
                                    local_body.y as f32,
                                    local_body.w as f32,
                                    local_body.h as f32,
                                ];
                                u.src_rect = [sx as f32, sy as f32, sw as f32, sh as f32];
                                u.clip_rect = [
                                    parent_clip_phys.x as f32 - parent_ox,
                                    parent_clip_phys.y as f32 - parent_oy,
                                    parent_clip_phys.right() as f32 - parent_ox,
                                    parent_clip_phys.bottom() as f32 - parent_oy,
                                ];
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
                                u.extra = [0.0, 0.0, snap_tex_sz.0 as f32, snap_tex_sz.1 as f32];

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
                                    src_idx: bd_blurred,
                                    dst_snap_idx: snap_idx,
                                    mask_path,
                                    effect_slot: slot,
                                });
                                fx.pool.release(bd_blurred);
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
                                    src_idx: layer.tex_idx,
                                    dst_snap_idx: layer.tex_idx,
                                    mask_path: sh_mask,
                                    effect_slot: eslot,
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

                                let snap_idx = fx.pool.acquire(device, parent_aw, parent_ah);
                                fx.steps.push(GpuPassStep::CopyRect {
                                    src_idx: parent_idx,
                                    src_origin: (0, 0),
                                    dst_idx: snap_idx,
                                    dst_origin: (0, 0),
                                    size: (parent_aw, parent_ah),
                                });
                                let sh_tex_sz = (fx.pool.get(sh_final).width, fx.pool.get(sh_final).height);
                                let snap_sz = (fx.pool.get(snap_idx).width, fx.pool.get(snap_idx).height);
                                let ox = (ds.offset_x as f32) * sf;
                                let oy = (ds.offset_y as f32) * sf;

                                let mut cu = EffectUniform::default();
                                cu.tex_sizes = [
                                    parent_tex_sz.0 as f32,
                                    parent_tex_sz.1 as f32,
                                    sh_tex_sz.0 as f32,
                                    sh_tex_sz.1 as f32,
                                ];
                                cu.dst_rect = [
                                    dst_rect[0] + ox,
                                    dst_rect[1] + oy,
                                    dst_rect[2],
                                    dst_rect[3],
                                ];
                                cu.src_rect = local_buf;
                                cu.clip_rect = clip_f;
                                cu.body_rect = local_body;
                                cu.params = [
                                    (spec.opacity as f32) * (1.0 / 255.0),
                                    spec.blend_mode as u8 as f32,
                                    EFFECT_KIND_COMPOSITE,
                                    0.0,
                                ];
                                cu.set_color_matrix(None);
                                cu.set_transform(spec.transform, sf);
                                cu.extra[0] += ox;
                                cu.extra[1] += oy;
                                cu.extra[2] = snap_sz.0 as f32;
                                cu.extra[3] = snap_sz.1 as f32;
                                let cslot = fx.push_slot(cu);
                                fx.steps.push(GpuPassStep::Effect {
                                    dst_idx: parent_idx,
                                    clear_dst: false,
                                    src_idx: sh_final,
                                    dst_snap_idx: snap_idx,
                                    mask_path: None,
                                    effect_slot: cslot,
                                });
                                fx.pool.release(sh_final);
                                fx.pool.release(snap_idx);
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

                        // 3. 合成回父层（ColorMatrix + SVG Path 遮罩 + Transform2D + Opacity + 27 种 BlendMode）
                        let snap_idx = fx.pool.acquire(device, parent_aw, parent_ah);
                        fx.steps.push(GpuPassStep::CopyRect {
                            src_idx: parent_idx,
                            src_origin: (0, 0),
                            dst_idx: snap_idx,
                            dst_origin: (0, 0),
                            size: (parent_aw, parent_ah),
                        });
                        let src_tex_sz = (fx.pool.get(src_final).width, fx.pool.get(src_final).height);
                        let snap_tex_sz = (fx.pool.get(snap_idx).width, fx.pool.get(snap_idx).height);

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
                            spec.blend_mode as u8 as f32,
                            EFFECT_KIND_COMPOSITE,
                            0.0,
                        ];
                        cu.set_color_matrix(spec.filter.color_matrix);
                        cu.flags[1] = if spec.clip_path.is_some() { 1.0 } else { 0.0 };
                        cu.flags[3] = if is_srgb { 1.0 } else { 0.0 };
                        cu.set_transform(spec.transform, sf);
                        cu.extra[2] = snap_tex_sz.0 as f32;
                        cu.extra[3] = snap_tex_sz.1 as f32;
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
                            src_idx: src_final,
                            dst_snap_idx: snap_idx,
                            mask_path,
                            effect_slot: cslot,
                        });
                        fx.pool.release(src_final);
                        fx.pool.release(snap_idx);
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
                        self.encode_single_op(
                            queue,
                            &tmp_op,
                            op.clip,
                            viewport,
                            sf,
                            use_ttf,
                            buf_rect.x as f32,
                            buf_rect.y as f32,
                        )?;
                        let pend = self.primitives.len() as u32;
                        fx.steps.push(GpuPassStep::DrawBatch {
                            target_idx: tmp_tex,
                            clear: Some(wgpu::Color::TRANSPARENT),
                            viewport_slot: vp_slot,
                            prim_start: pstart,
                            prim_end: pend,
                        });
                        if let Some(top) = stack.last_mut() {
                            top.batch_start = pend;
                        }

                        let snap_idx = fx.pool.acquire(device, parent_aw, parent_ah);
                        fx.steps.push(GpuPassStep::CopyRect {
                            src_idx: parent_idx,
                            src_origin: (0, 0),
                            dst_idx: snap_idx,
                            dst_origin: (0, 0),
                            size: (parent_aw, parent_ah),
                        });
                        let snap_sz = (fx.pool.get(snap_idx).width, fx.pool.get(snap_idx).height);
                        let clip_phys = snap_clip_region_scaled(op.clip, sf);
                        let local_buf = [
                            buf_rect.x as f32 - parent_ox,
                            buf_rect.y as f32 - parent_oy,
                            buf_rect.w as f32,
                            buf_rect.h as f32,
                        ];
                        let mut cu = EffectUniform::default();
                        cu.tex_sizes = [
                            parent_tex_sz.0 as f32,
                            parent_tex_sz.1 as f32,
                            tmp_sz.0 as f32,
                            tmp_sz.1 as f32,
                        ];
                        cu.dst_rect = local_buf;
                        cu.src_rect = local_buf;
                        cu.clip_rect = [
                            clip_phys.x as f32 - parent_ox,
                            clip_phys.y as f32 - parent_oy,
                            clip_phys.right() as f32 - parent_ox,
                            clip_phys.bottom() as f32 - parent_oy,
                        ];
                        cu.body_rect = local_buf;
                        cu.params = [1.0, sq.blend_mode as u8 as f32, EFFECT_KIND_COMPOSITE, 0.0];
                        cu.set_color_matrix(None);
                        cu.inv_xform = [1.0, 0.0, 0.0, 1.0];
                        cu.extra = [0.0, 0.0, snap_sz.0 as f32, snap_sz.1 as f32];
                        let cslot = fx.push_slot(cu);
                        fx.steps.push(GpuPassStep::Effect {
                            dst_idx: parent_idx,
                            clear_dst: false,
                            src_idx: tmp_tex,
                            dst_snap_idx: snap_idx,
                            mask_path: None,
                            effect_slot: cslot,
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

                            let snap_idx = fx.pool.acquire(device, parent_aw, parent_ah);
                            fx.steps.push(GpuPassStep::CopyRect {
                                src_idx: parent_idx,
                                src_origin: (0, 0),
                                dst_idx: snap_idx,
                                dst_origin: (0, 0),
                                size: (parent_aw, parent_ah),
                            });
                            let snap_sz =
                                (fx.pool.get(snap_idx).width, fx.pool.get(snap_idx).height);

                            let body_phys = snap_bounds_region_scaled(op.bounds, sf);
                            let clip_phys = snap_clip_region_scaled(raw_clip, sf);
                            let local_body = [
                                body_phys.x as f32 - parent_ox,
                                body_phys.y as f32 - parent_oy,
                                body_phys.w as f32,
                                body_phys.h as f32,
                            ];

                            let mut eu = EffectUniform::default();
                            eu.tex_sizes = [
                                parent_tex_sz.0 as f32,
                                parent_tex_sz.1 as f32,
                                1.0,
                                1.0,
                            ];
                            eu.dst_rect = local_body;
                            eu.src_rect = ext.uv_rect;
                            eu.clip_rect = [
                                clip_phys.x as f32 - parent_ox,
                                clip_phys.y as f32 - parent_oy,
                                clip_phys.right() as f32 - parent_ox,
                                clip_phys.bottom() as f32 - parent_oy,
                            ];
                            eu.body_rect = local_body;
                            eu.radii = [
                                (ext.radii.tl as f32) * sf,
                                (ext.radii.tr as f32) * sf,
                                (ext.radii.br as f32) * sf,
                                (ext.radii.bl as f32) * sf,
                            ];
                            eu.params = [
                                (ext.opacity as f32) * (1.0 / 255.0),
                                ext.blend_mode as u8 as f32,
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
                            eu.extra = [0.0, 0.0, snap_sz.0 as f32, snap_sz.1 as f32];
                            let eslot = fx.push_slot(eu);

                            fx.steps.push(GpuPassStep::DrawExternal {
                                dst_idx: parent_idx,
                                dst_snap_idx: snap_idx,
                                tex_id: ext.id,
                                effect_slot: eslot,
                            });
                            fx.pool.release(snap_idx);
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
                            let (ox, oy) = {
                                let top = stack.last().unwrap();
                                (top.origin_x, top.origin_y)
                            };
                            self.encode_single_op(
                                queue, &tmp_op, raw_clip, viewport, sf, use_ttf, ox, oy,
                            )?;
                        }
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
                        let (ox, oy) = {
                            let top = stack.last().unwrap();
                            (top.origin_x, top.origin_y)
                        };
                        self.encode_single_op(queue, op, raw_clip, viewport, sf, use_ttf, ox, oy)?;
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
}
