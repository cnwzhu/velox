//! GPU 多 Pass 后处理与离屏合成引擎（`GpuEffectsEngine`）：
//! 负责 Dual Kawase Blur、实时毛玻璃（`backdrop-filter`）、4×5 `ColorMatrix`、任意子树 `DropShadow`、
//! 2D 仿射变换、SVG Path 裁剪遮罩与完整 27 种 W3C / Porter-Duff 混合模式的 GPU 编排。

use ::wgpu;
use std::collections::HashMap;
use std::vec::Vec;

use crate::paint::{ColorMatrix, Transform2D};
use crate::solver::Region;

use super::layer_pool::LayerTargetPool;
use super::pod_as_bytes;

const EFFECTS_WGSL: &str = include_str!("effects.wgsl");
const INIT_EFFECT_SLOTS: usize = 32;

/// 与 `effects.wgsl` 严格对齐的 256 字节动态 Uniform 槽位（`16 × vec4<f32>`）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct EffectUniform {
    pub(crate) tex_sizes: [f32; 4],
    pub(crate) dst_rect: [f32; 4],
    pub(crate) src_rect: [f32; 4],
    pub(crate) clip_rect: [f32; 4],
    pub(crate) body_rect: [f32; 4],
    pub(crate) radii: [f32; 4],
    pub(crate) params: [f32; 4],
    pub(crate) flags: [f32; 4],
    pub(crate) tint: [f32; 4],
    pub(crate) inv_xform: [f32; 4],
    pub(crate) extra: [f32; 4],
    pub(crate) cm0: [f32; 4],
    pub(crate) cm1: [f32; 4],
    pub(crate) cm2: [f32; 4],
    pub(crate) cm3: [f32; 4],
    pub(crate) cm_bias: [f32; 4],
}

const _: () = assert!(core::mem::size_of::<EffectUniform>() == 256);

pub(crate) const EFFECT_KIND_COMPOSITE: f32 = 0.0;
pub(crate) const EFFECT_KIND_BACKDROP: f32 = 1.0;
pub(crate) const EFFECT_KIND_KAWASE_DOWN: f32 = 2.0;
pub(crate) const EFFECT_KIND_KAWASE_UP: f32 = 3.0;
pub(crate) const EFFECT_KIND_SHADOW_EXTRACT: f32 = 4.0;
pub(crate) const EFFECT_KIND_BLIT: f32 = 5.0;
pub(crate) const EFFECT_KIND_EXTERNAL_TEX: f32 = 6.0;

impl EffectUniform {
    pub(crate) fn set_color_matrix(&mut self, cm: Option<ColorMatrix>) {
        if let Some(m) = cm {
            self.flags[0] = 1.0;
            self.cm0 = [m.0[0], m.0[1], m.0[2], m.0[3]];
            self.cm1 = [m.0[5], m.0[6], m.0[7], m.0[8]];
            self.cm2 = [m.0[10], m.0[11], m.0[12], m.0[13]];
            self.cm3 = [m.0[15], m.0[16], m.0[17], m.0[18]];
            self.cm_bias = [m.0[4], m.0[9], m.0[14], m.0[19]];
        } else {
            self.flags[0] = 0.0;
            self.cm0 = [1.0, 0.0, 0.0, 0.0];
            self.cm1 = [0.0, 1.0, 0.0, 0.0];
            self.cm2 = [0.0, 0.0, 1.0, 0.0];
            self.cm3 = [0.0, 0.0, 0.0, 1.0];
            self.cm_bias = [0.0; 4];
        }
    }

    pub(crate) fn set_transform(&mut self, t: Option<Transform2D>, scale: f32) {
        if let Some(tr) = t {
            if !tr.is_identity() {
                let lin = Transform2D::new(tr.a, tr.b, tr.c, tr.d, 0.0, 0.0);
                if let Some(inv) = lin.invert() {
                    self.inv_xform = [inv.a, inv.b, inv.c, inv.d];
                    self.extra[0] = tr.e * scale;
                    self.extra[1] = tr.f * scale;
                    return;
                }
            }
        }
        self.inv_xform = [1.0, 0.0, 0.0, 1.0];
        self.extra[0] = 0.0;
        self.extra[1] = 0.0;
    }
}

/// 录制到单 `CommandEncoder` 中的原子 GPU 操作步骤。
pub(crate) enum GpuPassStep {
    /// 将 `primitives[start..end]` 渲染至 `target_idx` 离屏纹理。
    DrawBatch {
        target_idx: usize,
        clear: Option<wgpu::Color>,
        viewport_slot: u32,
        prim_start: u32,
        prim_end: u32,
    },
    /// 将全局 UI `SlugTextEngine` 实例区间 `slug_start..slug_end` 渲染至 `target_idx` 离屏纹理（严格保持 Z-order）。
    DrawSlugBatch {
        target_idx: usize,
        clear: Option<wgpu::Color>,
        slug_start: u32,
        slug_end: u32,
    },
    /// 在两张离屏池纹理之间拷贝局部矩形（用于安全获取 `dst_tex` 背景快照，规避读写冲突）。
    CopyRect {
        src_idx: usize,
        src_origin: (u32, u32),
        dst_idx: usize,
        dst_origin: (u32, u32),
        size: (u32, u32),
    },
    /// 执行单次后处理/混合/模糊/毛玻璃着色器 Pass。
    Effect {
        dst_idx: usize,
        clear_dst: bool,
        /// 非 `None` 时使用 `LoadOp::DontCare` + 精确 Scissor（专供 Dual Kawase 金字塔免除 Tile GPU 整纹理清屏）。
        dont_care_scissor: Option<[u32; 4]>,
        src_idx: usize,
        dst_snap_idx: usize,
        mask_path: Option<(std::string::String, u32, u32)>,
        effect_slot: u32,
        /// `true` 时使用硬件预乘 Alpha 直出管线（`SrcOver` 零背景快照、零纹理拷贝）。
        use_hw_blend: bool,
    },
    /// 将已注册的外部零拷贝 `wgpu::TextureView` 合成到 `dst_idx` 离屏纹理（支持独立圆角、UV 裁切与 27 种混合）。
    DrawExternal {
        dst_idx: usize,
        dst_snap_idx: usize,
        tex_id: crate::paint::ExternalTextureId,
        effect_slot: u32,
        use_hw_blend: bool,
    },
    /// 在多 Pass 图层合成流程中原地执行原生硬件加速 `SceneCanvas` 渲染（避免退化到 CPU 软件光栅与大纹理图集拷贝）。
    DrawSceneCanvas {
        target_idx: usize,
        op_idx: usize,
        bounds_phys: [f32; 4],
        clip_phys: [f32; 4],
        scissor: crate::solver::Region,
        sf: f32,
        is_srgb: bool,
    },
    /// 最终将根离屏画布 Blit 到外部传入的 `target_view`。
    BlitToTarget {
        src_idx: usize,
        effect_slot: u32,
    },
}

/// 懒初始化的 GPU 离屏合成与后处理子引擎（常规无图层 UI 启动与运行期 0 成本）。
pub(crate) struct GpuEffectsEngine {
    pipeline: wgpu::RenderPipeline,
    /// 硬件 `PREMULTIPLIED_ALPHA_BLENDING` 直出管线：供 `BlendMode::SrcOver` 图层/投影/外部纹理零快照直出。
    pipeline_src_over: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_capacity: usize,
    uniform_gen: u64,
    /// 跨帧 `BindGroup` 复用缓存：键 `(uniform_gen, src_gen, snap_gen, mask_key)`，消除每帧驱动层重分配。
    bg_cache: HashMap<(u64, u64, u64, u64), wgpu::BindGroup>,
    /// 外部零拷贝纹理跨帧 `BindGroup` 复用缓存：键 `(uniform_gen, ext_gen, snap_gen)`，消除视频/海图 60fps 建组开销。
    ext_bg_cache: HashMap<(u64, u64, u64), wgpu::BindGroup>,
    pub(crate) pool: LayerTargetPool,
    pub(crate) slots: Vec<EffectUniform>,
    pub(crate) steps: Vec<GpuPassStep>,
}

// SAFETY: `GpuEffectsEngine` 仅持有 `wgpu` 跨线程安全句柄与独占容器，支持后台预热线程移交至渲染主线程。
unsafe impl Send for GpuEffectsEngine {}

impl GpuEffectsEngine {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        pipeline_cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("velox_wgpu_effects_shader"),
            source: wgpu::ShaderSource::Wgsl(EFFECTS_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velox_wgpu_effects_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(256),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("velox_wgpu_effects_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        // 混合逻辑完全在片元着色器中按 27 种 W3C 公式精确求值，直接覆写目标像素（BlendState::REPLACE）
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velox_wgpu_effects_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
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
                entry_point: Some("fs_effect"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipeline_cache,
        });

        // 硬件 `SrcOver` 直出管线：对所有 `BlendMode::SrcOver` 图层/投影/外部纹理跳过整屏快照与拷贝
        let pipeline_src_over = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velox_wgpu_effects_pipeline_src_over"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
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
                entry_point: Some("fs_effect"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: pipeline_cache,
        });

        let uniform_buf = Self::create_uniform_buf(device, INIT_EFFECT_SLOTS);
        let pool = LayerTargetPool::new(device, queue, target_format);
        Self {
            pipeline,
            pipeline_src_over,
            bgl,
            uniform_buf,
            uniform_capacity: INIT_EFFECT_SLOTS,
            uniform_gen: 1,
            bg_cache: HashMap::with_capacity(32),
            ext_bg_cache: HashMap::with_capacity(8),
            pool,
            slots: Vec::with_capacity(INIT_EFFECT_SLOTS),
            steps: Vec::with_capacity(32),
        }
    }

    /// 仅拷贝父画布与 `dst_rect ∩ clip_rect` 相交的局部矩形快照，将显存拷贝带宽从整屏降低 20×~100×。
    /// 返回 `(snap_idx, snap_origin_x, snap_origin_y, snap_tex_w, snap_tex_h)`。
    pub(crate) fn acquire_subrect_snapshot(
        &mut self,
        device: &wgpu::Device,
        parent_idx: usize,
        parent_aw: u32,
        parent_ah: u32,
        dst_rect: [f32; 4],
        clip_rect: [f32; 4],
    ) -> (usize, f32, f32, f32, f32) {
        let x0 = dst_rect[0].max(clip_rect[0]).floor().max(0.0) as u32;
        let y0 = dst_rect[1].max(clip_rect[1]).floor().max(0.0) as u32;
        let x1 = (dst_rect[0] + dst_rect[2])
            .min(clip_rect[2])
            .ceil()
            .max(0.0) as u32;
        let y1 = (dst_rect[1] + dst_rect[3])
            .min(clip_rect[3])
            .ceil()
            .max(0.0) as u32;
        let sx = x0.min(parent_aw.saturating_sub(1));
        let sy = y0.min(parent_ah.saturating_sub(1));
        let sw = x1.min(parent_aw).saturating_sub(sx).max(1);
        let sh = y1.min(parent_ah).saturating_sub(sy).max(1);

        let snap_idx = self.pool.acquire(device, sw, sh);
        self.steps.push(GpuPassStep::CopyRect {
            src_idx: parent_idx,
            src_origin: (sx, sy),
            dst_idx: snap_idx,
            dst_origin: (0, 0),
            size: (sw, sh),
        });
        let tex_sz = (self.pool.get(snap_idx).width, self.pool.get(snap_idx).height);
        (snap_idx, sx as f32, sy as f32, tex_sz.0 as f32, tex_sz.1 as f32)
    }

    fn create_uniform_buf(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velox_wgpu_effects_uniforms"),
            size: (capacity.max(1) * 256) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub(crate) fn begin_frame(&mut self) {
        self.pool.begin_frame();
        self.slots.clear();
        self.steps.clear();
    }

    #[inline]
    pub(crate) fn push_slot(&mut self, u: EffectUniform) -> u32 {
        let idx = self.slots.len() as u32;
        self.slots.push(u);
        idx
    }

    /// 录制 Dual Kawase Blur 金字塔步骤：对 `src_idx` 纹理的 `[0..active_w, 0..active_h]` 执行平滑模糊，
    /// 返回模糊结果的离屏纹理索引（有效区域同样位于 `[0..active_w, 0..active_h]`）。
    pub(crate) fn plan_kawase_blur(
        &mut self,
        device: &wgpu::Device,
        src_idx: usize,
        active_w: u32,
        active_h: u32,
        blur_px: f32,
        is_srgb: bool,
    ) -> usize {
        if blur_px < 0.5 || active_w == 0 || active_h == 0 {
            return src_idx;
        }
        // 根据目标模糊半径自适应选择金字塔级数（1~4 级）与偏移步长
        let levels: usize = if blur_px <= 3.0 {
            1
        } else if blur_px <= 10.0 {
            2
        } else if blur_px <= 26.0 {
            3
        } else {
            4
        };
        let step_offset = (blur_px / ((1u32 << levels) as f32) * 1.15).clamp(0.5, 4.0);

        // 1. Downsample 金字塔：每下采样一级生成 `dst_idx` 后立即释放前一级中间纹理，
        //    整个金字塔最多仅并发占用 2 张乒乓小纹理，避免同时占住全部中间级导致离屏池膨胀。
        let mut dims: Vec<(u32, u32)> = Vec::with_capacity(levels + 1);
        dims.push((active_w, active_h));
        let mut cur_idx = src_idx;
        let mut cur_w = active_w;
        let mut cur_h = active_h;
        for _ in 0..levels {
            let next_w = (cur_w / 2).max(1);
            let next_h = (cur_h / 2).max(1);
            let dst_idx = self.pool.acquire(device, next_w, next_h);
            let src_tex_sz = (self.pool.get(cur_idx).width, self.pool.get(cur_idx).height);
            let dst_tex_sz = (self.pool.get(dst_idx).width, self.pool.get(dst_idx).height);

            let mut u = EffectUniform::default();
            u.tex_sizes = [
                dst_tex_sz.0 as f32,
                dst_tex_sz.1 as f32,
                src_tex_sz.0 as f32,
                src_tex_sz.1 as f32,
            ];
            u.dst_rect = [0.0, 0.0, next_w as f32, next_h as f32];
            u.src_rect = [0.0, 0.0, cur_w as f32, cur_h as f32];
            u.params = [1.0, 0.0, EFFECT_KIND_KAWASE_DOWN, step_offset];
            u.flags[3] = if is_srgb { 1.0 } else { 0.0 };
            let slot = self.push_slot(u);
            self.steps.push(GpuPassStep::Effect {
                dst_idx,
                clear_dst: false,
                dont_care_scissor: Some([0, 0, next_w, next_h]),
                src_idx: cur_idx,
                dst_snap_idx: cur_idx,
                mask_path: None,
                effect_slot: slot,
                use_hw_blend: false,
            });
            if cur_idx != src_idx {
                self.pool.release(cur_idx);
            }
            cur_idx = dst_idx;
            cur_w = next_w;
            cur_h = next_h;
            dims.push((cur_w, cur_h));
        }

        // 2. Upsample 金字塔（回升至原始 active_w × active_h，每级完成后立即释放上一级小纹理）
        let mut cw = cur_w;
        let mut ch = cur_h;
        for lvl in (0..levels).rev() {
            let (target_w, target_h) = dims[lvl];
            let out_idx = self.pool.acquire(device, target_w, target_h);
            let src_tex_sz = (self.pool.get(cur_idx).width, self.pool.get(cur_idx).height);
            let dst_tex_sz = (self.pool.get(out_idx).width, self.pool.get(out_idx).height);

            let mut u = EffectUniform::default();
            u.tex_sizes = [
                dst_tex_sz.0 as f32,
                dst_tex_sz.1 as f32,
                src_tex_sz.0 as f32,
                src_tex_sz.1 as f32,
            ];
            u.dst_rect = [0.0, 0.0, target_w as f32, target_h as f32];
            u.src_rect = [0.0, 0.0, cw as f32, ch as f32];
            u.params = [1.0, 0.0, EFFECT_KIND_KAWASE_UP, step_offset];
            u.flags[3] = if is_srgb { 1.0 } else { 0.0 };
            let slot = self.push_slot(u);
            self.steps.push(GpuPassStep::Effect {
                dst_idx: out_idx,
                clear_dst: false,
                dont_care_scissor: Some([0, 0, target_w, target_h]),
                src_idx: cur_idx,
                dst_snap_idx: cur_idx,
                mask_path: None,
                effect_slot: slot,
                use_hw_blend: false,
            });

            if cur_idx != src_idx {
                self.pool.release(cur_idx);
            }
            cur_idx = out_idx;
            cw = target_w;
            ch = target_h;
        }
        cur_idx
    }

    /// 批量上传全部 `EffectUniform` 槽位并在单 `CommandEncoder` 内执行全部 `GpuPassStep`。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_steps(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        main_pipeline: &wgpu::RenderPipeline,
        main_bind_group: &wgpu::BindGroup,
        linear_sampler: &wgpu::Sampler,
        external_textures: &std::collections::HashMap<
            crate::paint::ExternalTextureId,
            super::pipeline::ExternalTextureEntry,
        >,
        ops: &[crate::motion::DrawOp<'_>],
        canvas_engine: &mut Option<super::canvas_pipeline::CanvasGpuEngine>,
        slug_engine: &mut Option<super::slug_glyph::SlugTextEngine>,
        pipeline_cache: Option<&wgpu::PipelineCache>,
        target_format: wgpu::TextureFormat,
        target_view: &wgpu::TextureView,
    ) {
        if self.slots.len() > self.uniform_capacity {
            self.uniform_capacity = self.slots.len().next_power_of_two();
            self.uniform_buf = Self::create_uniform_buf(device, self.uniform_capacity);
            self.uniform_gen = self.uniform_gen.wrapping_add(1).max(1);
            self.bg_cache.clear();
            self.ext_bg_cache.clear();
        }
        if self.bg_cache.len() > 256 {
            self.bg_cache.clear();
        }
        if self.ext_bg_cache.len() > 128 {
            self.ext_bg_cache.clear();
        }
        if !self.slots.is_empty() {
            queue.write_buffer(&self.uniform_buf, 0, pod_as_bytes(&self.slots));
        }

        // 预构建所有需要用到的 SVG Path R8Unorm 遮罩纹理
        for step in &self.steps {
            if let GpuPassStep::Effect {
                mask_path: Some((path_d, mw, mh)),
                ..
            } = step
            {
                let _ = self.pool.ensure_path_mask(device, queue, path_d, *mw, *mh);
            }
        }

        for step in &self.steps {
            match step {
                GpuPassStep::DrawBatch {
                    target_idx,
                    clear,
                    viewport_slot,
                    prim_start,
                    prim_end,
                } => {
                    let view = &self.pool.get(*target_idx).view;
                    let load = match *clear {
                        Some(c) => wgpu::LoadOp::Clear(c),
                        None => wgpu::LoadOp::Load,
                    };
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_layer_draw_batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
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
                    if *prim_end > *prim_start {
                        rpass.set_pipeline(main_pipeline);
                        rpass.set_bind_group(0, main_bind_group, &[*viewport_slot * 256]);
                        rpass.draw(0..6, *prim_start..*prim_end);
                    }
                }

                GpuPassStep::DrawSlugBatch {
                    target_idx,
                    clear,
                    slug_start,
                    slug_end,
                } => {
                    let view = &self.pool.get(*target_idx).view;
                    let load = match *clear {
                        Some(c) => wgpu::LoadOp::Clear(c),
                        None => wgpu::LoadOp::Load,
                    };
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_layer_slug_batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
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
                    if let Some(slug) = slug_engine.as_ref() {
                        slug.draw_range(&mut rpass, *slug_start, *slug_end);
                    }
                }

                GpuPassStep::CopyRect {
                    src_idx,
                    src_origin,
                    dst_idx,
                    dst_origin,
                    size,
                } => {
                    if size.0 == 0 || size.1 == 0 {
                        continue;
                    }
                    let src = self.pool.get(*src_idx);
                    let dst = self.pool.get(*dst_idx);
                    let cw = size
                        .0
                        .min(src.width.saturating_sub(src_origin.0))
                        .min(dst.width.saturating_sub(dst_origin.0));
                    let ch = size
                        .1
                        .min(src.height.saturating_sub(src_origin.1))
                        .min(dst.height.saturating_sub(dst_origin.1));
                    if cw == 0 || ch == 0 {
                        continue;
                    }
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &src.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: src_origin.0,
                                y: src_origin.1,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &dst.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: dst_origin.0,
                                y: dst_origin.1,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: cw,
                            height: ch,
                            depth_or_array_layers: 1,
                        },
                    );
                }

                GpuPassStep::Effect {
                    dst_idx,
                    clear_dst,
                    dont_care_scissor,
                    src_idx,
                    dst_snap_idx,
                    mask_path,
                    effect_slot,
                    use_hw_blend,
                } => {
                    let mask_key = mask_path
                        .as_ref()
                        .map(|(path_d, mw, mh)| {
                            self.pool.ensure_path_mask(device, queue, path_d, *mw, *mh)
                        });
                    if self.pool.take_masks_evicted() {
                        let uniform_gen = self.uniform_gen;
                        let pool = &self.pool;
                        self.bg_cache.retain(|&(u_gen, src_gen, snap_gen, mask_k), _| {
                            u_gen == uniform_gen
                                && pool.has_live_generation(src_gen)
                                && pool.has_live_generation(snap_gen)
                                && pool.has_live_mask(mask_k)
                        });
                    }
                    let src_gen = self.pool.get(*src_idx).generation;
                    let use_src_as_dummy_snap = *use_hw_blend || *dst_snap_idx == *dst_idx;
                    let snap_gen = if use_src_as_dummy_snap {
                        src_gen
                    } else {
                        self.pool.get(*dst_snap_idx).generation
                    };
                    let cache_key = (self.uniform_gen, src_gen, snap_gen, mask_key.unwrap_or(0));

                    if !self.bg_cache.contains_key(&cache_key) {
                        let mask_view = self.pool.path_mask_view(mask_key);
                        let src_view = &self.pool.get(*src_idx).view;
                        let snap_view = if use_src_as_dummy_snap {
                            src_view
                        } else {
                            &self.pool.get(*dst_snap_idx).view
                        };
                        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("velox_effect_bg"),
                            layout: &self.bgl,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                        buffer: &self.uniform_buf,
                                        offset: 0,
                                        size: wgpu::BufferSize::new(256),
                                    }),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(src_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::TextureView(snap_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 3,
                                    resource: wgpu::BindingResource::TextureView(mask_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 4,
                                    resource: wgpu::BindingResource::Sampler(linear_sampler),
                                },
                            ],
                        });
                        self.bg_cache.insert(cache_key, bg);
                    }
                    let bg = &self.bg_cache[&cache_key];
                    let dst_view = &self.pool.get(*dst_idx).view;

                    let load_op = if dont_care_scissor.is_some() {
                        // SAFETY: Dual Kawase 着色器在 `dont_care_scissor` 内逐像素覆盖全部目标有效区，
                        // 且下游仅在该 `active_w × active_h` 范围内（带半像素 UV 钳位）采样。
                        wgpu::LoadOp::DontCare(unsafe { wgpu::LoadOpDontCare::enabled() })
                    } else if *clear_dst {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    };

                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_effect_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: dst_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: load_op,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    });
                    if let Some([sx, sy, sw, sh]) = *dont_care_scissor {
                        let dst_tex = self.pool.get(*dst_idx);
                        let cw = sw.min(dst_tex.width.saturating_sub(sx));
                        let ch = sh.min(dst_tex.height.saturating_sub(sy));
                        if cw > 0 && ch > 0 {
                            rpass.set_scissor_rect(sx, sy, cw, ch);
                        }
                    }
                    rpass.set_pipeline(if *use_hw_blend {
                        &self.pipeline_src_over
                    } else {
                        &self.pipeline
                    });
                    rpass.set_bind_group(0, bg, &[*effect_slot * 256]);
                    rpass.draw(0..6, 0..1);
                }

                GpuPassStep::DrawExternal {
                    dst_idx,
                    dst_snap_idx,
                    tex_id,
                    effect_slot,
                    use_hw_blend,
                } => {
                    let Some(entry) = external_textures.get(tex_id) else {
                        continue;
                    };
                    // 硬件 SrcOver 直出不采样背景快照，绑定只读外部纹理作占位以避免与 dst_view 同 Pass 冲突
                    let use_dummy_snap = *use_hw_blend || *dst_snap_idx == *dst_idx;
                    let snap_gen = if use_dummy_snap {
                        0
                    } else {
                        self.pool.get(*dst_snap_idx).generation
                    };
                    let ext_key = (self.uniform_gen, entry.generation, snap_gen);
                    if !self.ext_bg_cache.contains_key(&ext_key) {
                        let snap_view = if use_dummy_snap {
                            &entry.view
                        } else {
                            &self.pool.get(*dst_snap_idx).view
                        };
                        let mask_view = self.pool.white_mask_view();
                        let sampler = entry.sampler.as_ref().unwrap_or(linear_sampler);
                        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("velox_external_tex_bg"),
                            layout: &self.bgl,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                        buffer: &self.uniform_buf,
                                        offset: 0,
                                        size: wgpu::BufferSize::new(256),
                                    }),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(&entry.view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::TextureView(snap_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 3,
                                    resource: wgpu::BindingResource::TextureView(mask_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 4,
                                    resource: wgpu::BindingResource::Sampler(sampler),
                                },
                            ],
                        });
                        self.ext_bg_cache.insert(ext_key, bg);
                    }
                    let bg = &self.ext_bg_cache[&ext_key];
                    let dst_view = &self.pool.get(*dst_idx).view;
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_external_tex_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: dst_view,
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
                    rpass.set_pipeline(if *use_hw_blend {
                        &self.pipeline_src_over
                    } else {
                        &self.pipeline
                    });
                    rpass.set_bind_group(0, bg, &[*effect_slot * 256]);
                    rpass.draw(0..6, 0..1);
                }

                GpuPassStep::DrawSceneCanvas {
                    target_idx,
                    op_idx,
                    bounds_phys,
                    clip_phys,
                    scissor,
                    sf,
                    is_srgb,
                } => {
                    if let Some(op) = ops.get(*op_idx) {
                        if let crate::motion::DrawPayload::SceneCanvas(spec) = op.payload {
                            let canvas_eng = canvas_engine.get_or_insert_with(|| {
                                super::canvas_pipeline::CanvasGpuEngine::new(
                                    device,
                                    target_format,
                                    pipeline_cache,
                                )
                            });
                            let slug_eng = slug_engine.get_or_insert_with(|| {
                                super::slug_glyph::SlugTextEngine::new(
                                    device,
                                    target_format,
                                    pipeline_cache,
                                )
                            });
                            let dst = self.pool.get(*target_idx);
                            canvas_eng.render_canvas(
                                device,
                                queue,
                                encoder,
                                &dst.view,
                                dst.width,
                                dst.height,
                                *bounds_phys,
                                *clip_phys,
                                *scissor,
                                *sf,
                                *is_srgb,
                                spec,
                                slug_eng,
                            );
                        }
                    }
                }

                GpuPassStep::BlitToTarget {
                    src_idx,
                    effect_slot,
                } => {
                    let src_gen = self.pool.get(*src_idx).generation;
                    let cache_key = (self.uniform_gen, src_gen, src_gen, 0);
                    if !self.bg_cache.contains_key(&cache_key) {
                        let src_view = &self.pool.get(*src_idx).view;
                        let mask_view = self.pool.white_mask_view();
                        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("velox_blit_bg"),
                            layout: &self.bgl,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                        buffer: &self.uniform_buf,
                                        offset: 0,
                                        size: wgpu::BufferSize::new(256),
                                    }),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(src_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::TextureView(src_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 3,
                                    resource: wgpu::BindingResource::TextureView(mask_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 4,
                                    resource: wgpu::BindingResource::Sampler(linear_sampler),
                                },
                            ],
                        });
                        self.bg_cache.insert(cache_key, bg);
                    }
                    let bg = &self.bg_cache[&cache_key];
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("velox_blit_to_target"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    });
                    rpass.set_pipeline(&self.pipeline);
                    rpass.set_bind_group(0, bg, &[*effect_slot * 256]);
                    rpass.draw(0..6, 0..1);
                }
            }
        }
    }

    /// 同步清理 `bg_cache` 中引用已被淘汰纹理或遮罩的 `BindGroup`，避免隐式延长 GPU 显存生命周期。
    pub(crate) fn prune_stale_bind_groups(&mut self) {
        let uniform_gen = self.uniform_gen;
        let pool = &self.pool;
        self.bg_cache.retain(|&(u_gen, src_gen, snap_gen, mask_key), _| {
            u_gen == uniform_gen
                && pool.has_live_generation(src_gen)
                && pool.has_live_generation(snap_gen)
                && pool.has_live_mask(mask_key)
        });
    }

    /// 多 Pass 帧提交后按字节预算与年龄回收多余空闲离屏纹理，并同步清理陈旧 `BindGroup`。
    pub(crate) fn end_frame_reclaim(&mut self) {
        if self.pool.end_frame_reclaim() {
            self.prune_stale_bind_groups();
        }
    }

    /// 普通单 Pass 帧推进空闲纹理老化并回收过期/超预算纹理。
    pub(crate) fn tick_idle_frame(&mut self) {
        if self.pool.tick_idle_frame() {
            self.prune_stale_bind_groups();
        }
    }

    /// 视口尺寸缩小/变更时回收远大于新视口的空闲大纹理。
    pub(crate) fn on_viewport_resized(&mut self, width: u32, height: u32) {
        if self.pool.on_viewport_resized(width, height) {
            self.prune_stale_bind_groups();
        }
    }

    /// 后台化或显式内存整理时立即清空全部空闲离屏纹理、遮罩与 `BindGroup` 缓存。
    pub(crate) fn trim_idle_all(&mut self) {
        if self.pool.trim_all_idle() {
            self.prune_stale_bind_groups();
        }
    }
}

/// 将任意浮点逻辑/物理坐标包围盒安全钳位到 `[0..max_w, 0..max_h]`。
#[inline]
pub(crate) fn clamp_rect_u32(r: Region, max_w: u32, max_h: u32) -> (u32, u32, u32, u32) {
    let x0 = (r.x.max(0) as u32).min(max_w);
    let y0 = (r.y.max(0) as u32).min(max_h);
    let x1 = (r.right().max(0) as u32).min(max_w);
    let y1 = (r.bottom().max(0) as u32).min(max_h);
    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}
