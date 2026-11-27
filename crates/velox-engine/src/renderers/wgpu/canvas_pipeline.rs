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

/// 默认 `CanvasGpuEngine` 空闲场景显存预算（64 MiB）。
pub(crate) const DEFAULT_CANVAS_IDLE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
/// 默认空闲场景最大存活帧数（约 5 秒 @60fps 未渲染后自动回收）。
pub(crate) const DEFAULT_CANVAS_IDLE_MAX_AGE_FRAMES: u64 = 300;
/// 状态数组相邻变更合并阈值（间隔 <= 64 项即 256B 时合并为单次 `queue.write_buffer`）。
const STATE_MERGE_GAP_ITEMS: usize = 64;
/// `CanvasUniformData` 动态偏移步长（256 字节对齐，支持单帧内主画布 + Minimap 等多 `SceneCanvas` 零覆盖并行绘制）。
const CANVAS_UNIFORM_STRIDE: u32 =
    ((core::mem::size_of::<CanvasUniformData>() as u32) + 255) & !255;
/// 初始动态 Uniform 槽位容量（16 槽 ≈ 12 KiB，覆盖常规多画布 + 鹰眼场景零扩容）。
const INITIAL_UNIFORM_SLOTS: usize = 16;

struct CachedGpuScene {
    geom_revision: u32,
    state_revision: u32,
    mesh_revision: u32,
    geom_buf: Option<wgpu::Buffer>,
    geom_bytes: usize,
    geom_count: u32,
    state_buf: wgpu::Buffer,
    state_capacity: usize,
    state_bytes: usize,
    /// 已上传至 GPU `state_buf` 的打包状态镜像，用于无分配局部变更区间 diff 与相邻区间合并上传。
    uploaded_states: Vec<u32>,
    bind_group: wgpu::BindGroup,
    mesh_vbuf: Option<wgpu::Buffer>,
    mesh_ibuf: Option<wgpu::Buffer>,
    mesh_bytes: usize,
    mesh_index_count: u32,
    last_used_frame: u64,
}

impl CachedGpuScene {
    #[inline]
    fn gpu_bytes(&self) -> usize {
        self.geom_bytes
            .saturating_add(self.state_bytes)
            .saturating_add(self.mesh_bytes)
    }
}

pub(crate) struct CanvasGpuEngine {
    mesh_pipeline: wgpu::RenderPipeline,
    inst_pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_capacity: usize,
    next_uniform_slot: usize,
    uniform_staging: Vec<CanvasUniformData>,
    #[cfg_attr(not(feature = "custom-shader"), allow(dead_code))]
    target_format: wgpu::TextureFormat,
    scenes: HashMap<StaticSceneId, CachedGpuScene>,
    frame_index: u64,
    total_gpu_bytes: usize,
    idle_budget_bytes: usize,
    idle_max_age_frames: u64,
    last_state_upload_bytes: usize,
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
                        has_dynamic_offset: true,
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
            size: (INITIAL_UNIFORM_SLOTS as u64) * (CANVAS_UNIFORM_STRIDE as u64),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            mesh_pipeline,
            inst_pipeline,
            bgl,
            uniform_buf,
            uniform_capacity: INITIAL_UNIFORM_SLOTS,
            next_uniform_slot: 0,
            uniform_staging: Vec::with_capacity(INITIAL_UNIFORM_SLOTS),
            target_format,
            scenes: HashMap::with_capacity(4),
            frame_index: 1,
            total_gpu_bytes: 0,
            idle_budget_bytes: DEFAULT_CANVAS_IDLE_BUDGET_BYTES,
            idle_max_age_frames: DEFAULT_CANVAS_IDLE_MAX_AGE_FRAMES,
            last_state_upload_bytes: 0,
        }
    }

    fn make_scene_bind_group(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        uniform_buf: &wgpu::Buffer,
        state_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("velox_canvas_bg"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniform_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(
                            core::mem::size_of::<CanvasUniformData>() as u64,
                        ),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: state_buf.as_entire_binding(),
                },
            ],
        })
    }

    fn alloc_uniform_slot(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniform: CanvasUniformData,
    ) -> u32 {
        let slot = self.next_uniform_slot;
        self.next_uniform_slot = self.next_uniform_slot.saturating_add(1);
        if self.uniform_staging.len() <= slot {
            self.uniform_staging.push(uniform);
        } else {
            self.uniform_staging[slot] = uniform;
        }
        if slot >= self.uniform_capacity {
            let new_cap = (slot + 1).next_power_of_two().max(INITIAL_UNIFORM_SLOTS);
            let new_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_canvas_uniform_buf"),
                size: (new_cap as u64) * (CANVAS_UNIFORM_STRIDE as u64),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            for (idx, item) in self.uniform_staging.iter().enumerate() {
                queue.write_buffer(
                    &new_buf,
                    (idx as u64) * (CANVAS_UNIFORM_STRIDE as u64),
                    pod_as_bytes(core::slice::from_ref(item)),
                );
            }
            self.uniform_buf = new_buf;
            self.uniform_capacity = new_cap;
            for scene in self.scenes.values_mut() {
                scene.bind_group = Self::make_scene_bind_group(
                    device,
                    &self.bgl,
                    &self.uniform_buf,
                    &scene.state_buf,
                );
            }
        } else {
            queue.write_buffer(
                &self.uniform_buf,
                (slot as u64) * (CANVAS_UNIFORM_STRIDE as u64),
                pod_as_bytes(&[uniform]),
            );
        }
        (slot as u32) * CANVAS_UNIFORM_STRIDE
    }

    /// 显式移除并释放指定 `StaticSceneId` 的所有常驻 GPU 缓冲（几何/状态/网格）。
    pub(crate) fn remove_scene(&mut self, scene_id: StaticSceneId) -> bool {
        if let Some(removed) = self.scenes.remove(&scene_id) {
            self.total_gpu_bytes = self.total_gpu_bytes.saturating_sub(removed.gpu_bytes());
            true
        } else {
            false
        }
    }

    /// 每帧推进存活计时并按闲置年龄与空闲显存预算自动回收已切走的旧场景，同时复位帧内动态 Uniform 槽位游标。
    pub(crate) fn tick_frame_reclaim(&mut self) {
        self.next_uniform_slot = 0;
        self.uniform_staging.clear();
        self.frame_index = self.frame_index.saturating_add(1);
        self.reclaim_idle_scenes();
    }

    /// 后台化或收到内存压力回调时立即释放全部非当前帧活跃的场景 GPU 缓冲。
    pub(crate) fn trim_idle_scenes(&mut self) {
        self.scenes.clear();
        self.total_gpu_bytes = 0;
    }

    /// 按闲置年龄与空闲字节预算淘汰最久未使用的非本帧活跃场景。
    fn reclaim_idle_scenes(&mut self) {
        let cur = self.frame_index;
        let max_age = self.idle_max_age_frames;
        if max_age > 0 {
            let mut stale: Vec<StaticSceneId> = Vec::new();
            for (&id, sc) in &self.scenes {
                if sc.last_used_frame < cur && cur.saturating_sub(sc.last_used_frame) >= max_age {
                    stale.push(id);
                }
            }
            for id in stale {
                self.remove_scene(id);
            }
        }

        while self.total_gpu_bytes > self.idle_budget_bytes {
            let Some((&victim_id, _)) = self
                .scenes
                .iter()
                .filter(|(_, sc)| sc.last_used_frame < cur)
                .min_by_key(|(_, sc)| (sc.last_used_frame, core::cmp::Reverse(sc.gpu_bytes())))
            else {
                break;
            };
            self.remove_scene(victim_id);
        }
    }

    #[inline]
    pub(crate) fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    #[inline]
    pub(crate) fn total_gpu_bytes(&self) -> usize {
        self.total_gpu_bytes
    }

    #[inline]
    pub(crate) fn last_state_upload_bytes(&self) -> usize {
        self.last_state_upload_bytes
    }

    #[inline]
    pub(crate) fn set_idle_budget_bytes(&mut self, bytes: usize) {
        self.idle_budget_bytes = bytes;
        self.reclaim_idle_scenes();
    }

    #[inline]
    pub(crate) fn set_idle_max_age_frames(&mut self, frames: u64) {
        self.idle_max_age_frames = frames;
        self.reclaim_idle_scenes();
    }

    fn ensure_scene_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        spec: &SceneCanvasSpec<'_>,
    ) {
        let cur_frame = self.frame_index;
        let state_need = spec.states.len().max(1);
        let mut created_bytes = 0usize;
        let entry = self.scenes.entry(spec.scene_id).or_insert_with(|| {
            let cap = state_need.next_power_of_two().max(64);
            let state_bytes = cap * core::mem::size_of::<CanvasItemState>();
            let state_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_canvas_state_storage"),
                size: state_bytes as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group =
                Self::make_scene_bind_group(device, &self.bgl, &self.uniform_buf, &state_buf);
            created_bytes = state_bytes;
            CachedGpuScene {
                geom_revision: u32::MAX,
                state_revision: u32::MAX,
                mesh_revision: u32::MAX,
                geom_buf: None,
                geom_bytes: 0,
                geom_count: 0,
                state_buf,
                state_capacity: cap,
                state_bytes,
                uploaded_states: Vec::new(),
                bind_group,
                mesh_vbuf: None,
                mesh_ibuf: None,
                mesh_bytes: 0,
                mesh_index_count: 0,
                last_used_frame: cur_frame,
            }
        });
        entry.last_used_frame = cur_frame;
        if created_bytes > 0 {
            self.total_gpu_bytes = self.total_gpu_bytes.saturating_add(created_bytes);
        }

        let old_scene_bytes = entry.gpu_bytes();

        // 1. 静态几何仅在 geom_revision 变更时上传常驻 VertexBuffer
        if entry.geom_revision != spec.geom_revision || entry.geom_count != spec.geoms.len() as u32 {
            entry.geom_revision = spec.geom_revision;
            entry.geom_count = spec.geoms.len() as u32;
            if spec.geoms.is_empty() {
                entry.geom_buf = None;
                entry.geom_bytes = 0;
            } else {
                let bytes = pod_as_bytes(spec.geoms);
                let alloc_size = bytes.len().max(32);
                let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_canvas_static_geom_vbuf"),
                    size: alloc_size as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, bytes);
                entry.geom_buf = Some(vbuf);
                entry.geom_bytes = alloc_size;
            }
        }

        // 2. 动态状态 4B/inst 增量合并区间上传（零每帧临时 Vec 分配，只上传实际变更区段）
        let mut force_full_state_upload = false;
        if state_need > entry.state_capacity {
            let cap = state_need.next_power_of_two();
            let state_bytes = cap * core::mem::size_of::<CanvasItemState>();
            let state_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_canvas_state_storage"),
                size: state_bytes as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            entry.bind_group =
                Self::make_scene_bind_group(device, &self.bgl, &self.uniform_buf, &state_buf);
            entry.state_buf = state_buf;
            entry.state_capacity = cap;
            entry.state_bytes = state_bytes;
            entry.state_revision = u32::MAX;
            force_full_state_upload = true;
        }
        if entry.state_revision != spec.state_revision {
            entry.state_revision = spec.state_revision;
            let mut uploaded_bytes = 0usize;
            if !spec.states.is_empty() {
                if force_full_state_upload || entry.uploaded_states.is_empty() {
                    entry.uploaded_states.clear();
                    entry
                        .uploaded_states
                        .extend(spec.states.iter().map(|s| s.pack_u32()));
                    let raw = pod_as_bytes(&entry.uploaded_states);
                    queue.write_buffer(&entry.state_buf, 0, raw);
                    uploaded_bytes = raw.len();
                } else {
                    let old_len = entry.uploaded_states.len();
                    let new_len = spec.states.len();
                    if new_len > old_len {
                        entry.uploaded_states.resize(new_len, 0);
                    } else if new_len < old_len {
                        entry.uploaded_states.truncate(new_len);
                    }

                    // 扫描变更项并按 `STATE_MERGE_GAP_ITEMS` 合并相邻区间，仅上传真实变动范围
                    let mut run_start: Option<usize> = None;
                    let mut run_end: usize = 0;
                    for i in 0..new_len {
                        let packed = spec.states[i].pack_u32();
                        let changed = i >= old_len || entry.uploaded_states[i] != packed;
                        if changed {
                            entry.uploaded_states[i] = packed;
                            match run_start {
                                None => {
                                    run_start = Some(i);
                                    run_end = i + 1;
                                }
                                Some(rs) => {
                                    if i.saturating_sub(run_end) <= STATE_MERGE_GAP_ITEMS {
                                        run_end = i + 1;
                                    } else {
                                        let slice = &entry.uploaded_states[rs..run_end];
                                        let raw = pod_as_bytes(slice);
                                        queue.write_buffer(&entry.state_buf, (rs * 4) as u64, raw);
                                        uploaded_bytes += raw.len();
                                        run_start = Some(i);
                                        run_end = i + 1;
                                    }
                                }
                            }
                        }
                    }
                    if let Some(rs) = run_start {
                        let slice = &entry.uploaded_states[rs..run_end];
                        let raw = pod_as_bytes(slice);
                        queue.write_buffer(&entry.state_buf, (rs * 4) as u64, raw);
                        uploaded_bytes += raw.len();
                    }
                }
            }
            self.last_state_upload_bytes = uploaded_bytes;
        } else {
            self.last_state_upload_bytes = 0;
        }

        // 3. 静态底图多边形网格仅在 mesh_revision 变更时上传
        if entry.mesh_revision != spec.mesh_revision {
            entry.mesh_revision = spec.mesh_revision;
            if spec.mesh_verts.is_empty() || spec.mesh_indices.is_empty() {
                entry.mesh_vbuf = None;
                entry.mesh_ibuf = None;
                entry.mesh_bytes = 0;
                entry.mesh_index_count = 0;
            } else {
                let vbytes = pod_as_bytes(spec.mesh_verts);
                let ibytes = pod_as_bytes(spec.mesh_indices);
                let v_alloc = vbytes.len().max(16);
                let i_alloc = ibytes.len().max(16);
                let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_canvas_mesh_vbuf"),
                    size: v_alloc as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_canvas_mesh_ibuf"),
                    size: i_alloc as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, vbytes);
                queue.write_buffer(&ibuf, 0, ibytes);
                entry.mesh_vbuf = Some(vbuf);
                entry.mesh_ibuf = Some(ibuf);
                entry.mesh_bytes = v_alloc.saturating_add(i_alloc);
                entry.mesh_index_count = spec.mesh_indices.len() as u32;
            }
        }

        let new_scene_bytes = entry.gpu_bytes();
        self.total_gpu_bytes = self
            .total_gpu_bytes
            .saturating_sub(old_scene_bytes)
            .saturating_add(new_scene_bytes);
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
        let dyn_offset = self.alloc_uniform_slot(device, queue, uniform);

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
                rpass.set_bind_group(0, &scene.bind_group, &[dyn_offset]);
                rpass.set_vertex_buffer(0, mv.slice(..));
                rpass.set_index_buffer(mi.slice(..), wgpu::IndexFormat::Uint32);
                rpass.draw_indexed(0..scene.mesh_index_count, 0, 0..1);
            }
        }

        // 3. 32B 静态几何 + 4B 动态状态 Instanced OBB/圆/胶囊 SDF（按视锥裁剪连续区间绘制）
        if let Some(gv) = &scene.geom_buf {
            if scene.geom_count > 0 {
                rpass.set_pipeline(&self.inst_pipeline);
                rpass.set_bind_group(0, &scene.bind_group, &[dyn_offset]);
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
