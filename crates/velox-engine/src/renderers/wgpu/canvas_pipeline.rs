//! 高密度 2D 交互画布专用 GPU 渲染子引擎（`CanvasGpuEngine`）。
//!
//! 职责：
//! - 管理 `StaticSceneId -> CachedGpuScene` 常驻显存缓冲（`32B` 静态几何 `VertexBuffer` + `4B` 动态状态 `StorageBuffer`）；
//! - 静态几何仅在 `geom_revision` 变化时上传；动态状态变更仅局部 `queue.write_buffer` 刷新；
//! - 静态多边形底图网格（`CanvasMeshVertex`）按 `mesh_revision` 常驻显存；
//! - 支持 OBB 任意旋转圆角矩形 / 圆形 / 胶囊线段 SDF、橡皮筋框选，以及 `#[cfg(feature = "custom-shader")]` 逃生舱调度。

use ::wgpu;
use std::collections::HashMap;
use std::vec::Vec;

use crate::canvas_scene::{
    CanvasItemState, CanvasMeshVertex, CanvasStaticGeom, SceneCanvasSpec, StaticSceneId,
};
use crate::solver::Region;

use super::{color_to_f32, pod_as_bytes};

const CANVAS_WGSL: &str = include_str!("canvas.wgsl");

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct CanvasUniformData {
    screen_info: [f32; 4],
    camera: [f32; 4],
    clip_rect: [f32; 4],
    flags: [f32; 4],
    palette: [[f32; 4]; 32],
}

impl Default for CanvasUniformData {
    fn default() -> Self {
        Self {
            screen_info: [1.0, 1.0, 0.0, 0.0],
            camera: [0.0, 0.0, 1.0, 1.0],
            clip_rect: [0.0, 0.0, 1.0, 1.0],
            flags: [1.0, 0.0, 0.0, 0.0],
            palette: [[0.23, 0.51, 0.96, 1.0]; 32],
        }
    }
}

struct CachedGpuScene {
    geom_revision: u32,
    state_revision: u32,
    mesh_revision: u32,
    geom_buf: Option<wgpu::Buffer>,
    geom_count: u32,
    state_buf: wgpu::Buffer,
    state_capacity: usize,
    bind_group: wgpu::BindGroup,
    mesh_vbuf: Option<wgpu::Buffer>,
    mesh_ibuf: Option<wgpu::Buffer>,
    mesh_index_count: u32,
}

pub(crate) struct CanvasGpuEngine {
    mesh_pipeline: wgpu::RenderPipeline,
    inst_pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    #[cfg_attr(not(feature = "custom-shader"), allow(dead_code))]
    target_format: wgpu::TextureFormat,
    scenes: HashMap<StaticSceneId, CachedGpuScene>,
}

impl CanvasGpuEngine {
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("velox_canvas_wgsl"),
            source: wgpu::ShaderSource::Wgsl(CANVAS_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velox_canvas_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            core::mem::size_of::<CanvasUniformData>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("velox_canvas_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velox_canvas_mesh_pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mesh"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: core::mem::size_of::<CanvasMeshVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint32,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_mesh"),
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

        let inst_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velox_canvas_inst_pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_inst"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: core::mem::size_of::<CanvasStaticGeom>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_inst"),
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

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_canvas_uniform_buf"),
            size: core::mem::size_of::<CanvasUniformData>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            mesh_pipeline,
            inst_pipeline,
            bgl,
            uniform_buf,
            target_format,
            scenes: HashMap::with_capacity(4),
        }
    }

    fn ensure_scene_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        spec: &SceneCanvasSpec<'_>,
    ) {
        let state_need = spec.states.len().max(1);
        let entry = self.scenes.entry(spec.scene_id).or_insert_with(|| {
            let cap = state_need.next_power_of_two().max(64);
            let state_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_canvas_state_storage"),
                size: (cap * core::mem::size_of::<CanvasItemState>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("velox_canvas_bg"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: state_buf.as_entire_binding(),
                    },
                ],
            });
            CachedGpuScene {
                geom_revision: u32::MAX,
                state_revision: u32::MAX,
                mesh_revision: u32::MAX,
                geom_buf: None,
                geom_count: 0,
                state_buf,
                state_capacity: cap,
                bind_group,
                mesh_vbuf: None,
                mesh_ibuf: None,
                mesh_index_count: 0,
            }
        });

        // 1. 静态几何仅在 geom_revision 变更时上传常驻 VertexBuffer
        if entry.geom_revision != spec.geom_revision || entry.geom_count != spec.geoms.len() as u32 {
            entry.geom_revision = spec.geom_revision;
            entry.geom_count = spec.geoms.len() as u32;
            if spec.geoms.is_empty() {
                entry.geom_buf = None;
            } else {
                let bytes = pod_as_bytes(spec.geoms);
                let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_canvas_static_geom_vbuf"),
                    size: bytes.len().max(32) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, bytes);
                entry.geom_buf = Some(vbuf);
            }
        }

        // 2. 动态状态 4B/inst 增量上传
        if state_need > entry.state_capacity {
            let cap = state_need.next_power_of_two();
            let state_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_canvas_state_storage"),
                size: (cap * core::mem::size_of::<CanvasItemState>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            entry.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("velox_canvas_bg"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: state_buf.as_entire_binding(),
                    },
                ],
            });
            entry.state_buf = state_buf;
            entry.state_capacity = cap;
            entry.state_revision = u32::MAX;
        }
        if entry.state_revision != spec.state_revision {
            entry.state_revision = spec.state_revision;
            if !spec.states.is_empty() {
                let packed: Vec<u32> = spec.states.iter().map(|s| s.pack_u32()).collect();
                queue.write_buffer(&entry.state_buf, 0, pod_as_bytes(&packed));
            }
        }

        // 3. 静态底图多边形网格仅在 mesh_revision 变更时上传
        if entry.mesh_revision != spec.mesh_revision {
            entry.mesh_revision = spec.mesh_revision;
            if spec.mesh_verts.is_empty() || spec.mesh_indices.is_empty() {
                entry.mesh_vbuf = None;
                entry.mesh_ibuf = None;
                entry.mesh_index_count = 0;
            } else {
                let vbytes = pod_as_bytes(spec.mesh_verts);
                let ibytes = pod_as_bytes(spec.mesh_indices);
                let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_canvas_mesh_vbuf"),
                    size: vbytes.len().max(16) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_canvas_mesh_ibuf"),
                    size: ibytes.len().max(16) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, vbytes);
                queue.write_buffer(&ibuf, 0, ibytes);
                entry.mesh_vbuf = Some(vbuf);
                entry.mesh_ibuf = Some(ibuf);
                entry.mesh_index_count = spec.mesh_indices.len() as u32;
            }
        }
    }

    /// 渲染单个 `SceneCanvas`：依次执行 `bg_shaders` → `StaticMesh` → `CanvasInstanced` → `fg_shaders`。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_canvas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        phys_w: u32,
        phys_h: u32,
        bounds_phys: [f32; 4],
        clip_phys: [f32; 4],
        scissor: Region,
        scale_factor: f32,
        is_srgb: bool,
        spec: &SceneCanvasSpec<'_>,
    ) {
        if scissor.is_empty() || spec.opacity == 0 {
            return;
        }
        self.ensure_scene_buffers(device, queue, spec);

        let mut uniform = CanvasUniformData {
            screen_info: [phys_w as f32, phys_h as f32, bounds_phys[0], bounds_phys[1]],
            camera: [
                bounds_phys[0] + spec.viewport.offset_x * scale_factor,
                bounds_phys[1] + spec.viewport.offset_y * scale_factor,
                spec.viewport.zoom * scale_factor,
                scale_factor,
            ],
            clip_rect: clip_phys,
            flags: [
                (spec.opacity as f32) / 255.0,
                if is_srgb { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ],
            palette: [[0.23, 0.51, 0.96, 1.0]; 32],
        };
        for (i, &c) in spec.palette.iter().take(32).enumerate() {
            uniform.palette[i] = color_to_f32(c);
        }
        queue.write_buffer(&self.uniform_buf, 0, pod_as_bytes(&[uniform]));

        #[cfg(feature = "custom-shader")]
        let cs_vp = super::custom_shader::CustomShaderViewport::from_viewport2d(
            phys_w,
            phys_h,
            bounds_phys[0],
            bounds_phys[1],
            bounds_phys[2],
            bounds_phys[3],
            spec.viewport,
            scale_factor,
            is_srgb,
        );
        #[cfg(feature = "custom-shader")]
        let bg_shaders: Vec<_> = spec
            .bg_shaders
            .iter()
            .filter_map(|call| {
                let s = super::custom_shader::lookup_custom_shader(call.id)?;
                s.prepare(device, queue, self.target_format, &cs_vp, call.uniform_bytes);
                Some(s)
            })
            .collect();
        #[cfg(feature = "custom-shader")]
        let fg_shaders: Vec<_> = spec
            .fg_shaders
            .iter()
            .filter_map(|call| {
                let s = super::custom_shader::lookup_custom_shader(call.id)?;
                s.prepare(device, queue, self.target_format, &cs_vp, call.uniform_bytes);
                Some(s)
            })
            .collect();

        let sx = (scissor.x.max(0) as u32).min(phys_w);
        let sy = (scissor.y.max(0) as u32).min(phys_h);
        let sw = (scissor.w.max(0) as u32).min(phys_w.saturating_sub(sx));
        let sh = (scissor.h.max(0) as u32).min(phys_h.saturating_sub(sy));
        if sw == 0 || sh == 0 {
            return;
        }

        let Some(scene) = self.scenes.get(&spec.scene_id) else {
            return;
        };

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("velox_scene_canvas_pass"),
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
        rpass.set_scissor_rect(sx, sy, sw, sh);

        // 1. 背景自定义 WGSL 着色器
        #[cfg(feature = "custom-shader")]
        for shader in &bg_shaders {
            shader.render(&mut rpass);
        }

        // 2. 静态多边形底图三角网格
        if let (Some(mv), Some(mi)) = (&scene.mesh_vbuf, &scene.mesh_ibuf) {
            if scene.mesh_index_count > 0 {
                rpass.set_pipeline(&self.mesh_pipeline);
                rpass.set_bind_group(0, &scene.bind_group, &[]);
                rpass.set_vertex_buffer(0, mv.slice(..));
                rpass.set_index_buffer(mi.slice(..), wgpu::IndexFormat::Uint32);
                rpass.draw_indexed(0..scene.mesh_index_count, 0, 0..1);
            }
        }

        // 3. 32B 静态几何 + 4B 动态状态 Instanced OBB/圆/胶囊 SDF（按视锥裁剪连续区间绘制）
        if let Some(gv) = &scene.geom_buf {
            if scene.geom_count > 0 {
                rpass.set_pipeline(&self.inst_pipeline);
                rpass.set_bind_group(0, &scene.bind_group, &[]);
                rpass.set_vertex_buffer(0, gv.slice(..));
                if spec.visible_ranges.is_empty() {
                    rpass.draw(0..6, 0..scene.geom_count);
                } else {
                    for &(start, end) in spec.visible_ranges {
                        let s = start.min(scene.geom_count);
                        let e = end.min(scene.geom_count);
                        if s < e {
                            rpass.draw(0..6, s..e);
                        }
                    }
                }
            }
        }

        // 4. 前景自定义 WGSL 着色器
        #[cfg(feature = "custom-shader")]
        for shader in &fg_shaders {
            shader.render(&mut rpass);
        }
    }
}
