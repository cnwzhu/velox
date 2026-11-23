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
use super::{
    color_to_f32, make_arc_primitive, make_quad_primitive, make_shadow_primitive, pod_as_bytes,
    snap_bounds, snap_clip, GpuPrimitive, ViewportUniform, KIND_GLYPH, KIND_SPRITE,
};

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

/// 自研轻量级 `wgpu` 2D SDF 渲染管线核心（绑定任意目标 `TextureView` 即可渲染）。
pub struct WgpuRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    viewport_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    sampler: wgpu::Sampler,
    glyph_atlas: GlyphAtlas,
    sprite_atlas: SpriteAtlas,
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
                        has_dynamic_offset: false,
                        min_binding_size: None,
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

        let viewport_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_wgpu_viewport"),
            size: core::mem::size_of::<ViewportUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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

        let glyph_atlas = GlyphAtlas::new(device, GLYPH_ATLAS_INIT_SIZE);
        let sprite_atlas = SpriteAtlas::new(device, 1);
        let bind_group = Self::create_bind_group(
            device,
            &bind_group_layout,
            &viewport_buf,
            &instance_buf,
            &glyph_atlas.view,
            &sprite_atlas.view,
            &sampler,
        );

        Self {
            pipeline,
            bind_group_layout,
            bind_group,
            viewport_buf,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            sampler,
            glyph_atlas,
            sprite_atlas,
            primitives: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            sprite_scratch: Vec::new(),
            #[cfg(not(target_endian = "little"))]
            sprite_bytes_scratch: Vec::new(),
            text_layout_cache: HashMap::with_capacity(64),
            text_layout_hits: 0,
            target_is_srgb: target_format.is_srgb(),
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_wgpu_instances"),
            size: (capacity.max(1) * core::mem::size_of::<GpuPrimitive>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        viewport_buf: &wgpu::Buffer,
        instance_buf: &wgpu::Buffer,
        glyph_view: &wgpu::TextureView,
        sprite_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("velox_wgpu_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport_buf.as_entire_binding(),
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

    fn rebuild_bind_group(&mut self, device: &wgpu::Device) {
        self.bind_group = Self::create_bind_group(
            device,
            &self.bind_group_layout,
            &self.viewport_buf,
            &self.instance_buf,
            &self.glyph_atlas.view,
            &self.sprite_atlas.view,
            &self.sampler,
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

    /// 将 `DrawOp` 编码为 `self.primitives` 并按需上传字形与精灵图集；
    /// 若某个图集需要扩容则返回 `Err((need_glyph_grow, sprite_need))` 以便调用方扩容并重试一次。
    fn try_encode_ops(
        &mut self,
        queue: &wgpu::Queue,
        ops: &[DrawOp<'_>],
        viewport: Region,
        scale_factor: f32,
    ) -> Result<(), (bool, Option<(u32, u32)>)> {
        self.primitives.clear();
        if viewport.is_empty() {
            return Ok(());
        }

        let sf = scale_factor.clamp(0.5, 4.0);
        let use_ttf = crate::display::ttf_atlas::has_system_font();

        for (idx, op) in ops.iter().enumerate() {
            let raw_clip = viewport.intersect(op.clip).intersect(op.bounds);
            if raw_clip.is_empty() {
                continue;
            }
            let eff_clip = compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..]);
            if eff_clip.is_empty() {
                continue;
            }

            let sb_f = snap_bounds(op.bounds, sf);
            // 当父容器启用了圆角裁切时保留原始圆角裁剪盒（避免前向遮挡收缩破坏四角圆角中心）
            let clip_region = if op.clip_radius > 0 { op.clip } else { eff_clip };
            let mut sc_f = snap_clip(clip_region, sf);
            // 对未被遮挡的 1px 细线（光标、分割线、缩进参考线），避免 op.clip 预交集后的奇数坐标截断稳定物理线宽
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
                                sb_f,
                                sc_f,
                                clip_radius_px,
                                r,
                                (raw_bw as f32) * sf,
                                bg,
                            ));
                        }
                    } else if bg.a() > 0 || bg_grad.a() > 0 || (bw > 0.0 && border_color.a() > 0) {
                        self.primitives.push(make_quad_primitive(
                            sb_f,
                            sc_f,
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
                DrawPayload::TextRun {
                    text,
                    color,
                    font_size,
                    text_flags,
                } => {
                    if text.is_empty() || color.a() == 0 {
                        continue;
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
                    // 与软件光栅器共享同一套 0.25px 亚像素字号与单行扩框真源，确保首屏切 GPU 0.00px 零漂移
                    let (adj_bounds, adj_sc, adj_flags, logical_fs, fs_q4) =
                        compute_scaled_text_box(
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
                        adj_sc.x as f32,
                        adj_sc.y as f32,
                        adj_sc.right() as f32,
                        adj_sc.bottom() as f32,
                    ];
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
                        return Err((true, None));
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
                    // 原生 GPU 解析圆弧 SDF：零 CPU 光栅化、零纹理上传
                    self.primitives.push(make_arc_primitive(
                        sb_f,
                        sc_f,
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
                        continue;
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

                        let Some(uv) = self.sprite_atlas.insert_and_upload(queue, cache_key, rgba_bytes)
                        else {
                            return Err((false, Some((w as u32, h as u32))));
                        };
                        uv
                    };

                    self.primitives.push(GpuPrimitive {
                        bounds: [eff.x as f32, eff.y as f32, w as f32, h as f32],
                        clip: sc_f,
                        uv,
                        color0: [1.0; 4],
                        color1: [0.0; 4],
                        border_color: [0.0; 4],
                        params: [0.0, 0.0, KIND_SPRITE, 0.0],
                        extra: [clip_radius_px, 0.0, 0.0, 0.0],
                    });
                }
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
                    });
                }
            }
        }
        true
    }

    /// 将当前帧 `ops` 直接渲染至目标 `target_view`（单 RenderPass，单 Draw Call，支持分数 DPI 缩放因子 `scale_factor`）。
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
        let mut bind_group_dirty = false;
        for _attempt in 0..4 {
            match self.try_encode_ops(queue, ops, viewport, scale_factor) {
                Ok(()) => break,
                Err((glyph_full, sprite_need)) => {
                    if glyph_full && self.glyph_atlas.reset_or_grow(device) {
                        bind_group_dirty = true;
                    }
                    if let Some((sw, sh)) = sprite_need {
                        let (recreated, can_retry) =
                            self.sprite_atlas.reset_or_grow(device, sw, sh);
                        if recreated {
                            bind_group_dirty = true;
                        }
                        if !can_retry {
                            break;
                        }
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

        let uniform = ViewportUniform {
            size: [phys_width.max(1) as f32, phys_height.max(1) as f32],
            is_srgb: if self.target_is_srgb { 1.0 } else { 0.0 },
            _pad: 0.0,
        };
        queue.write_buffer(&self.viewport_buf, 0, pod_as_bytes(&[uniform]));

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
                rpass.set_bind_group(0, &self.bind_group, &[]);
                rpass.draw(0..6, 0..(self.primitives.len() as u32));
            }
        }
        queue.submit([encoder.finish()]);
    }
}
