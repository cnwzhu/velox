//! Slug（2026 公有领域版）GPU 曲线字形直接光栅化引擎（`SlugTextEngine`）。
//!
//! 特点：
//! - **零位图字形图集**：直接通过 `ttf_parser::OutlineBuilder` 提取 TrueType 二次贝塞尔曲线
//!   （遇到 OpenType CFF 三次贝塞尔自动自适应细分为二次贝塞尔），打包进只读 GPU `StorageBuffer`；
//! - **双轴条带加速（Horizontal + Vertical Bands）**：每条带内曲线按最大坐标降序排列，片元着色器提前退出；
//! - **无限清晰缩放与任意角度 OBB 旋转**：`0.1×` 到 `1000×` 连续缩放时 **0 次 CPU 字形重光栅化、0 图集扩容**；
//! - 面向 `velox-canvas` 世界空间标签（同时原生支持 ASCII 与 CJK 字符）。

use ::wgpu;
use std::collections::HashMap;
use std::vec::Vec;

use ttf_parser::OutlineBuilder;

use crate::canvas_scene::SceneCanvasSpec;

use super::{color_to_f32, pod_as_bytes};

const SLUG_WGSL: &str = include_str!("slug_text.wgsl");

/// 零依赖 IEEE 754 `f32 -> f16` 量化（支持 EM 空间高精度半浮点压缩，与 WGSL `unpack2x16float` 严格对齐）。
#[inline]
pub(crate) fn f32_to_f16_bits(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let frac = bits & 0x007F_FFFF;

    if exp == 0xFF {
        return sign | 0x7C00 | if frac != 0 { 0x0200 } else { 0 };
    }
    let new_exp = exp - 127 + 15;
    if new_exp >= 31 {
        return sign | 0x7BFF; // clamp to max finite f16 (65504.0)
    }
    if new_exp <= 0 {
        if new_exp < -10 {
            return sign;
        }
        let mant = (frac | 0x0080_0000) >> (14 - new_exp);
        let round = (mant + 1) >> 1;
        return sign | (round as u16);
    }
    let rounded = frac + 0x0000_1000;
    if (rounded & 0x0080_0000) != 0 {
        let e = (new_exp + 1).min(30) as u16;
        return sign | (e << 10);
    }
    sign | ((new_exp as u16) << 10) | ((rounded >> 13) as u16)
}

#[inline]
fn pack_f16x2(x: f32, y: f32) -> u32 {
    (f32_to_f16_bits(x) as u32) | ((f32_to_f16_bits(y) as u32) << 16)
}

/// GPU 16B `f16` 量化二次贝塞尔曲线存储记录（相比 `32 B` `f32` 显存与片元采样带宽直接减半）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SlugCurve {
    pub(crate) p012_pad: [u32; 4],
}

const _: () = assert!(core::mem::size_of::<SlugCurve>() == 16);

/// GPU 条带描述符（严格 8 字节：`start` 与 `count` 指向 `band_indices`）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SlugBand {
    pub(crate) start: u32,
    pub(crate) count: u32,
}

const _: () = assert!(core::mem::size_of::<SlugBand>() == 8);

/// 单个 Slug 字形绘制实例（严格 112 字节对齐 `7 × vec4`，支持完整 2×2 线性变换与逆雅可比动态膨胀）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SlugGlyphInstance {
    pub(crate) xform_row0: [f32; 4],
    pub(crate) xform_row1: [f32; 4],
    pub(crate) em_bounds: [f32; 4],
    pub(crate) clip_rect: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) band_info: [u32; 4],
    pub(crate) viewport_info: [f32; 4],
}

const _: () = assert!(core::mem::size_of::<SlugGlyphInstance>() == 112);

/// 已上传至 GPU 曲线/条带缓冲的字形元数据（与字号完全无关，单字形全局仅提取一次）。
#[derive(Copy, Clone, Debug)]
struct CachedSlugGlyph {
    em_bounds: [f32; 4],
    advance_em: f32,
    h_band_offset: u32,
    num_h_bands: u32,
    v_band_offset: u32,
    num_v_bands: u32,
    has_outline: bool,
}

struct OutlineCollector {
    inv_upem: f32,
    curves: Vec<([f32; 2], [f32; 2], [f32; 2])>,
    start: [f32; 2],
    curr: [f32; 2],
    has_curr: bool,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl OutlineCollector {
    fn new(upem: u16) -> Self {
        Self {
            inv_upem: 1.0 / (upem.max(1) as f32),
            curves: Vec::with_capacity(32),
            start: [0.0, 0.0],
            curr: [0.0, 0.0],
            has_curr: false,
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    #[inline]
    fn extend_bounds(&mut self, p: [f32; 2]) {
        self.min_x = self.min_x.min(p[0]);
        self.min_y = self.min_y.min(p[1]);
        self.max_x = self.max_x.max(p[0]);
        self.max_y = self.max_y.max(p[1]);
    }

    fn push_quad(&mut self, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2]) {
        let d02 = (p2[0] - p0[0]).abs() + (p2[1] - p0[1]).abs();
        let d01 = (p1[0] - p0[0]).abs() + (p1[1] - p0[1]).abs();
        if d02 < 1e-6 && d01 < 1e-6 {
            return;
        }
        self.extend_bounds(p0);
        self.extend_bounds(p1);
        self.extend_bounds(p2);
        self.curves.push((p0, p1, p2));
    }

    /// 按弦高误差自适应递归将 OpenType CFF 三次贝塞尔细分为二次贝塞尔（相比固定 4 等分减少 ~40% 曲线数，且急弯精度更高）。
    fn push_cubic_as_quads(&mut self, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) {
        self.subdivide_cubic_adaptive(p0, p1, p2, p3, 0);
    }

    fn subdivide_cubic_adaptive(
        &mut self,
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        p3: [f32; 2],
        depth: u8,
    ) {
        // 单段二次逼近的理论最大弦高误差：||p3 - 3*p2 + 3*p1 - p0|| / (4 * sqrt(3))
        let dx = p3[0] - 3.0 * p2[0] + 3.0 * p1[0] - p0[0];
        let dy = p3[1] - 3.0 * p2[1] + 3.0 * p1[1] - p0[1];
        let err_sq = (dx * dx + dy * dy) * (1.0 / 48.0);
        const MAX_ERR_SQ: f32 = 0.0012 * 0.0012;
        if depth >= 4 || err_sq <= MAX_ERR_SQ {
            let ctrl = [
                0.75 * (p1[0] + p2[0]) - 0.25 * (p0[0] + p3[0]),
                0.75 * (p1[1] + p2[1]) - 0.25 * (p0[1] + p3[1]),
            ];
            self.push_quad(p0, ctrl, p3);
            return;
        }
        // de Casteljau 中点精确劈分
        let mid = |a: [f32; 2], b: [f32; 2]| [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
        let p01 = mid(p0, p1);
        let p12 = mid(p1, p2);
        let p23 = mid(p2, p3);
        let p012 = mid(p01, p12);
        let p123 = mid(p12, p23);
        let p0123 = mid(p012, p123);
        self.subdivide_cubic_adaptive(p0, p01, p012, p0123, depth + 1);
        self.subdivide_cubic_adaptive(p0123, p123, p23, p3, depth + 1);
    }
}

impl OutlineBuilder for OutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = [x * self.inv_upem, y * self.inv_upem];
        self.start = p;
        self.curr = p;
        self.has_curr = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = [x * self.inv_upem, y * self.inv_upem];
        if self.has_curr {
            let mid = [0.5 * (self.curr[0] + p[0]), 0.5 * (self.curr[1] + p[1])];
            self.push_quad(self.curr, mid, p);
        }
        self.curr = p;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let c = [x1 * self.inv_upem, y1 * self.inv_upem];
        let p = [x * self.inv_upem, y * self.inv_upem];
        if self.has_curr {
            self.push_quad(self.curr, c, p);
        }
        self.curr = p;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = [x1 * self.inv_upem, y1 * self.inv_upem];
        let c2 = [x2 * self.inv_upem, y2 * self.inv_upem];
        let p = [x * self.inv_upem, y * self.inv_upem];
        if self.has_curr {
            self.push_cubic_as_quads(self.curr, c1, c2, p);
        }
        self.curr = p;
    }

    fn close(&mut self) {
        if self.has_curr {
            let d = (self.curr[0] - self.start[0]).abs() + (self.curr[1] - self.start[1]).abs();
            if d > 1e-6 {
                let mid = [
                    0.5 * (self.curr[0] + self.start[0]),
                    0.5 * (self.curr[1] + self.start[1]),
                ];
                self.push_quad(self.curr, mid, self.start);
            }
            self.curr = self.start;
        }
    }
}

/// GPU 曲线字形渲染子引擎（零字形位图图集，供 `CanvasGpuEngine` 渲染任意缩放/旋转世界标签）。
pub(crate) struct SlugTextEngine {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,

    curves: Vec<SlugCurve>,
    curves_buf: wgpu::Buffer,
    curves_capacity: usize,
    curves_uploaded: usize,

    band_indices: Vec<u32>,
    band_indices_buf: wgpu::Buffer,
    band_indices_capacity: usize,
    band_indices_uploaded: usize,

    bands: Vec<SlugBand>,
    bands_buf: wgpu::Buffer,
    bands_capacity: usize,
    bands_uploaded: usize,

    instances: Vec<SlugGlyphInstance>,
    instances_buf: wgpu::Buffer,
    instances_capacity: usize,
    instances_uploaded: usize,

    glyphs: HashMap<(u64, u16), CachedSlugGlyph>,
    last_drawn_glyphs: usize,
}

// SAFETY: `SlugTextEngine` 仅持有跨线程安全的 `wgpu` 句柄与独占 CPU 容器。
unsafe impl Send for SlugTextEngine {}

const INIT_CURVES_CAP: usize = 512;
const INIT_INDICES_CAP: usize = 1024;
const INIT_BANDS_CAP: usize = 256;
const INIT_INSTANCES_CAP: usize = 256;

impl SlugTextEngine {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        pipeline_cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("velox_slug_text_wgsl"),
            source: wgpu::ShaderSource::Wgsl(SLUG_WGSL.into()),
        });

        let ro_storage = |binding: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velox_slug_bgl"),
            entries: &[
                ro_storage(0, wgpu::ShaderStages::FRAGMENT),
                ro_storage(1, wgpu::ShaderStages::FRAGMENT),
                ro_storage(2, wgpu::ShaderStages::FRAGMENT),
                ro_storage(3, wgpu::ShaderStages::VERTEX_FRAGMENT),
            ],
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("velox_slug_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velox_slug_text_pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_slug"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_slug"),
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

        let make_buf = |label: &'static str, bytes: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.max(64) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        let curves_buf = make_buf(
            "velox_slug_curves",
            INIT_CURVES_CAP * core::mem::size_of::<SlugCurve>(),
        );
        let band_indices_buf = make_buf(
            "velox_slug_band_indices",
            INIT_INDICES_CAP * core::mem::size_of::<u32>(),
        );
        let bands_buf = make_buf(
            "velox_slug_bands",
            INIT_BANDS_CAP * core::mem::size_of::<SlugBand>(),
        );
        let instances_buf = make_buf(
            "velox_slug_instances",
            INIT_INSTANCES_CAP * core::mem::size_of::<SlugGlyphInstance>(),
        );
        let bind_group = Self::create_bind_group(
            device,
            &bgl,
            &curves_buf,
            &band_indices_buf,
            &bands_buf,
            &instances_buf,
        );

        Self {
            pipeline,
            bgl,
            bind_group,
            curves: Vec::with_capacity(INIT_CURVES_CAP),
            curves_buf,
            curves_capacity: INIT_CURVES_CAP,
            curves_uploaded: 0,
            band_indices: Vec::with_capacity(INIT_INDICES_CAP),
            band_indices_buf,
            band_indices_capacity: INIT_INDICES_CAP,
            band_indices_uploaded: 0,
            bands: Vec::with_capacity(INIT_BANDS_CAP),
            bands_buf,
            bands_capacity: INIT_BANDS_CAP,
            bands_uploaded: 0,
            instances: Vec::with_capacity(INIT_INSTANCES_CAP),
            instances_buf,
            instances_capacity: INIT_INSTANCES_CAP,
            instances_uploaded: 0,
            glyphs: HashMap::with_capacity(128),
            last_drawn_glyphs: 0,
        }
    }

    fn create_bind_group(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        curves_buf: &wgpu::Buffer,
        band_indices_buf: &wgpu::Buffer,
        bands_buf: &wgpu::Buffer,
        instances_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("velox_slug_bg"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: curves_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: band_indices_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: bands_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: instances_buf.as_entire_binding(),
                },
            ],
        })
    }

    /// 帧开始时清空本帧字形实例列表（保留全部已解析的 GPU 曲线与条带缓冲，并保留上一帧 `last_drawn_glyphs` 统计）。
    #[inline]
    pub(crate) fn begin_frame(&mut self) {
        self.instances.clear();
        self.instances_uploaded = 0;
    }

    /// 返回 Slug 曲线/条带/实例 GPU 缓冲估算总字节数（用于四层内存账本统计）。
    #[inline]
    pub(crate) fn gpu_bytes(&self) -> usize {
        self.curves_capacity * core::mem::size_of::<SlugCurve>()
            + self.band_indices_capacity * core::mem::size_of::<u32>()
            + self.bands_capacity * core::mem::size_of::<SlugBand>()
            + self.instances_capacity * core::mem::size_of::<SlugGlyphInstance>()
    }

    /// 返回当前已缓存的独立曲线字形数（与缩放字号无关）。
    #[inline]
    pub(crate) fn cached_glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// 返回当前帧已累计的 Slug 字形实例数量（用于 Z-order 分段交错绘制）。
    #[inline]
    pub(crate) fn instance_count(&self) -> u32 {
        self.instances.len() as u32
    }

    /// 为常规 UI `text()` 提交单个 Slug 曲线字形实例（位于物理坐标 `(pen_x, baseline_y)`、字号 `font_px`）。
    /// 若该字符无矢量轮廓（如空白或彩色 Emoji），返回 `false` 供上层自动回退至 `GlyphAtlas`。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_ui_glyph(
        &mut self,
        ch: char,
        pen_x: f32,
        baseline_y: f32,
        font_px: f32,
        clip_rect: [f32; 4],
        color: [f32; 4],
        target_w: u32,
        target_h: u32,
        is_srgb: bool,
    ) -> bool {
        let Some(g) = self.ensure_glyph(ch) else {
            return false;
        };
        if !g.has_outline {
            return false;
        }
        let srgb_f = if is_srgb { 1.0 } else { 0.0 };
        self.instances.push(SlugGlyphInstance {
            xform_row0: [font_px, 0.0, pen_x, baseline_y],
            xform_row1: [0.0, -font_px, 1.5, srgb_f],
            em_bounds: g.em_bounds,
            clip_rect,
            color,
            band_info: [
                g.h_band_offset,
                g.num_h_bands,
                g.v_band_offset,
                g.num_v_bands,
            ],
            viewport_info: [target_w.max(1) as f32, target_h.max(1) as f32, font_px, 0.0],
        });
        true
    }

    /// 帧编码结束后将本帧全部 UI Slug 字形与新增曲线增量同步至 GPU 缓冲，并更新统计计数。
    pub(crate) fn sync_and_finalize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.last_drawn_glyphs = self.instances.len();
        if !self.instances.is_empty() {
            self.sync_gpu_buffers(device, queue);
        }
    }

    /// 返回最近一帧由 Slug GPU 管线绘制的字形实例总数。
    #[inline]
    pub(crate) fn last_drawn_glyphs(&self) -> usize {
        self.last_drawn_glyphs
    }

    /// 按字符提取（首次）或查表获取与字号无关的 Slug 二次贝塞尔曲线与双轴条带元数据。
    fn ensure_glyph(&mut self, ch: char) -> Option<CachedSlugGlyph> {
        let mut extracted: Option<(u64, u16, f32, OutlineCollector)> = None;
        let lookup_key = crate::display::ttf_atlas::with_char_face_and_glyph(ch, |face, gid, fk| {
            let key = (fk, gid.0);
            if !self.glyphs.contains_key(&key) {
                let upem = face.units_per_em().max(1);
                let adv = face.glyph_hor_advance(gid).unwrap_or(upem / 2) as f32 / (upem as f32);
                let mut collector = OutlineCollector::new(upem);
                let _ = face.outline_glyph(gid, &mut collector);
                extracted = Some((fk, gid.0, adv, collector));
            }
            key
        })?;

        if let Some(&hit) = self.glyphs.get(&lookup_key) {
            return Some(hit);
        }

        let (fk, gid_raw, advance_em, collector) = extracted?;
        if collector.curves.is_empty() || !collector.min_x.is_finite() {
            let empty = CachedSlugGlyph {
                em_bounds: [0.0; 4],
                advance_em,
                h_band_offset: 0,
                num_h_bands: 0,
                v_band_offset: 0,
                num_v_bands: 0,
                has_outline: false,
            };
            self.glyphs.insert((fk, gid_raw), empty);
            return Some(empty);
        }

        let curve_base = self.curves.len() as u32;
        for &(p0, p1, p2) in &collector.curves {
            self.curves.push(SlugCurve {
                p012_pad: [
                    pack_f16x2(p0[0], p0[1]),
                    pack_f16x2(p1[0], p1[1]),
                    pack_f16x2(p2[0], p2[1]),
                    0,
                ],
            });
        }

        let min_x = collector.min_x;
        let min_y = collector.min_y;
        let max_x = collector.max_x.max(min_x + 1e-4);
        let max_y = collector.max_y.max(min_y + 1e-4);
        let span_x = max_x - min_x;
        let span_y = max_y - min_y;

        let n_curves = collector.curves.len();
        let num_bands: u32 = if n_curves <= 12 {
            2
        } else if n_curves <= 32 {
            4
        } else {
            8
        };

        // 构建水平加速条带（带微幅边界重叠容差防条带缝隙，按曲线 max_x 降序排列支持片元 Early-Exit）
        let band_eps_y = span_y * 1e-3;
        let h_band_offset = self.bands.len() as u32;
        let mut h_buckets: Vec<Vec<(u32, f32)>> = (0..num_bands).map(|_| Vec::new()).collect();
        for (i, &(p0, p1, p2)) in collector.curves.iter().enumerate() {
            // 纯水平线段对水平射线无绕数贡献
            if (p0[1] - p1[1]).abs() < 1e-6 && (p1[1] - p2[1]).abs() < 1e-6 {
                continue;
            }
            let c_min_y = p0[1].min(p1[1]).min(p2[1]) - band_eps_y;
            let c_max_y = p0[1].max(p1[1]).max(p2[1]) + band_eps_y;
            let c_max_x = p0[0].max(p1[0]).max(p2[0]);
            let b0 = (((c_min_y - min_y) / span_y) * (num_bands as f32))
                .floor()
                .clamp(0.0, (num_bands - 1) as f32) as usize;
            let b1 = (((c_max_y - min_y) / span_y) * (num_bands as f32))
                .floor()
                .clamp(0.0, (num_bands - 1) as f32) as usize;
            let global_idx = curve_base + i as u32;
            for bucket in &mut h_buckets[b0..=b1] {
                bucket.push((global_idx, c_max_x));
            }
        }
        for mut bucket in h_buckets {
            bucket.sort_by(|a, b| b.1.total_cmp(&a.1));
            let start = self.band_indices.len() as u32;
            let count = bucket.len() as u32;
            for (cidx, _) in bucket {
                self.band_indices.push(cidx);
            }
            self.bands.push(SlugBand { start, count });
        }

        // 构建垂直加速条带（按曲线 max_y 降序排列支持片元 Early-Exit）
        let band_eps_x = span_x * 1e-3;
        let v_band_offset = self.bands.len() as u32;
        let mut v_buckets: Vec<Vec<(u32, f32)>> = (0..num_bands).map(|_| Vec::new()).collect();
        for (i, &(p0, p1, p2)) in collector.curves.iter().enumerate() {
            // 纯垂直线段对垂直射线无绕数贡献
            if (p0[0] - p1[0]).abs() < 1e-6 && (p1[0] - p2[0]).abs() < 1e-6 {
                continue;
            }
            let c_min_x = p0[0].min(p1[0]).min(p2[0]) - band_eps_x;
            let c_max_x = p0[0].max(p1[0]).max(p2[0]) + band_eps_x;
            let c_max_y = p0[1].max(p1[1]).max(p2[1]);
            let b0 = (((c_min_x - min_x) / span_x) * (num_bands as f32))
                .floor()
                .clamp(0.0, (num_bands - 1) as f32) as usize;
            let b1 = (((c_max_x - min_x) / span_x) * (num_bands as f32))
                .floor()
                .clamp(0.0, (num_bands - 1) as f32) as usize;
            let global_idx = curve_base + i as u32;
            for bucket in &mut v_buckets[b0..=b1] {
                bucket.push((global_idx, c_max_y));
            }
        }
        for mut bucket in v_buckets {
            bucket.sort_by(|a, b| b.1.total_cmp(&a.1));
            let start = self.band_indices.len() as u32;
            let count = bucket.len() as u32;
            for (cidx, _) in bucket {
                self.band_indices.push(cidx);
            }
            self.bands.push(SlugBand { start, count });
        }

        let entry = CachedSlugGlyph {
            em_bounds: [min_x, min_y, max_x, max_y],
            advance_em,
            h_band_offset,
            num_h_bands: num_bands,
            v_band_offset,
            num_v_bands: num_bands,
            has_outline: true,
        };
        self.glyphs.insert((fk, gid_raw), entry);
        Some(entry)
    }

    /// 将当前 `SceneCanvasSpec` 的全部可见世界标签编码为 Slug 实例，同步静态曲线缓冲并返回本批次实例区间 `(start, end)`。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_canvas_labels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        spec: &SceneCanvasSpec<'_>,
        phys_w: u32,
        phys_h: u32,
        bounds_phys: [f32; 4],
        clip_phys: [f32; 4],
        scale_factor: f32,
        is_srgb: bool,
    ) -> (u32, u32) {
        if spec.labels.is_empty() || spec.opacity == 0 {
            return (0, 0);
        }
        let start_idx = self.instances.len() as u32;
        let vp_zoom = spec.viewport.zoom * scale_factor;
        let vp_ox = bounds_phys[0] + spec.viewport.offset_x * scale_factor;
        let vp_oy = bounds_phys[1] + spec.viewport.offset_y * scale_factor;
        let srgb_f = if is_srgb { 1.0 } else { 0.0 };

        let mut glyph_scratch: Vec<CachedSlugGlyph> = Vec::with_capacity(16);
        for lbl in spec.labels {
            if lbl.text.is_empty() {
                continue;
            }
            let target_px = (lbl.font_px * scale_factor).clamp(5.0, 256.0);
            let center_x = lbl.wx * vp_zoom + vp_ox;
            let center_y = lbl.wy * vp_zoom + vp_oy;
            // 粗视锥剔除（避免在屏幕外的数千标签走曲线查表）
            let approx_half_w = (lbl.text.len() as f32) * target_px * 0.6 + 8.0;
            if center_x + approx_half_w < clip_phys[0]
                || center_x - approx_half_w > clip_phys[2]
                || center_y + target_px < clip_phys[1]
                || center_y - target_px > clip_phys[3]
            {
                continue;
            }

            glyph_scratch.clear();
            let mut total_adv_em = 0.0f32;
            for ch in lbl.text.chars() {
                if let Some(g) = self.ensure_glyph(ch) {
                    total_adv_em += g.advance_em;
                    glyph_scratch.push(g);
                }
            }
            if glyph_scratch.is_empty() {
                continue;
            }

            let col = color_to_f32(lbl.color.scale_alpha(spec.opacity));
            let mut pen_x = center_x - total_adv_em * target_px * 0.5;
            let baseline_y = center_y + target_px * 0.34;
            for g in &glyph_scratch {
                if g.has_outline {
                    self.instances.push(SlugGlyphInstance {
                        xform_row0: [target_px, 0.0, pen_x, baseline_y],
                        xform_row1: [0.0, -target_px, 1.5, srgb_f],
                        em_bounds: g.em_bounds,
                        clip_rect: clip_phys,
                        color: col,
                        band_info: [
                            g.h_band_offset,
                            g.num_h_bands,
                            g.v_band_offset,
                            g.num_v_bands,
                        ],
                        viewport_info: [phys_w.max(1) as f32, phys_h.max(1) as f32, target_px, 0.0],
                    });
                }
                pen_x += g.advance_em * target_px;
            }
        }

        let end_idx = self.instances.len() as u32;
        if end_idx > start_idx {
            self.sync_gpu_buffers(device, queue);
            self.last_drawn_glyphs = self.instances.len();
        }
        (start_idx, end_idx)
    }

    /// 供单元测试或高级任意 2×2 仿射变换（旋转/斜切/非均匀拉伸）直接提交 Slug 曲线文本。
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_transformed_text(
        &mut self,
        text: &str,
        origin_x: f32,
        origin_y: f32,
        font_px: f32,
        angle_rad: f32,
        shear_x: f32,
        clip_rect: [f32; 4],
        color: [f32; 4],
        phys_w: u32,
        phys_h: u32,
        is_srgb: bool,
    ) {
        let ca = angle_rad.cos();
        let sa = angle_rad.sin();
        // 组合 EM(Y向上) -> 斜切 -> 缩放 -> 旋转 -> 屏幕(Y向下) 的 2x2 雅可比矩阵
        let m00 = ca * font_px;
        let m01 = (ca * shear_x + sa) * font_px;
        let m10 = sa * font_px;
        let m11 = (sa * shear_x - ca) * font_px;
        let srgb_f = if is_srgb { 1.0 } else { 0.0 };
        let mut advance_px = 0.0f32;
        for ch in text.chars() {
            if let Some(g) = self.ensure_glyph(ch) {
                if g.has_outline {
                    self.instances.push(SlugGlyphInstance {
                        xform_row0: [m00, m01, origin_x + advance_px * ca, origin_y + advance_px * sa],
                        xform_row1: [m10, m11, 1.5, srgb_f],
                        em_bounds: g.em_bounds,
                        clip_rect,
                        color,
                        band_info: [
                            g.h_band_offset,
                            g.num_h_bands,
                            g.v_band_offset,
                            g.num_v_bands,
                        ],
                        viewport_info: [phys_w.max(1) as f32, phys_h.max(1) as f32, font_px, 0.0],
                    });
                }
                advance_px += g.advance_em * font_px;
            }
        }
    }

    fn sync_gpu_buffers(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut bg_dirty = false;

        if self.curves.len() > self.curves_capacity {
            self.curves_capacity = self.curves.len().next_power_of_two();
            self.curves_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_slug_curves"),
                size: (self.curves_capacity * core::mem::size_of::<SlugCurve>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.curves_uploaded = 0;
            bg_dirty = true;
        }
        if self.band_indices.len() > self.band_indices_capacity {
            self.band_indices_capacity = self.band_indices.len().next_power_of_two();
            self.band_indices_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_slug_band_indices"),
                size: (self.band_indices_capacity * core::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.band_indices_uploaded = 0;
            bg_dirty = true;
        }
        if self.bands.len() > self.bands_capacity {
            self.bands_capacity = self.bands.len().next_power_of_two();
            self.bands_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_slug_bands"),
                size: (self.bands_capacity * core::mem::size_of::<SlugBand>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.bands_uploaded = 0;
            bg_dirty = true;
        }
        if self.instances.len() > self.instances_capacity {
            self.instances_capacity = self.instances.len().next_power_of_two();
            self.instances_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("velox_slug_instances"),
                size: (self.instances_capacity * core::mem::size_of::<SlugGlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instances_uploaded = 0;
            bg_dirty = true;
        }
        if bg_dirty {
            self.bind_group = Self::create_bind_group(
                device,
                &self.bgl,
                &self.curves_buf,
                &self.band_indices_buf,
                &self.bands_buf,
                &self.instances_buf,
            );
        }

        // 静态曲线/条带仅增量追加上传新出现的字形（缩放过程中恒为 0 B 上传！）
        if self.curves.len() > self.curves_uploaded {
            let off = (self.curves_uploaded * core::mem::size_of::<SlugCurve>()) as u64;
            queue.write_buffer(&self.curves_buf, off, pod_as_bytes(&self.curves[self.curves_uploaded..]));
            self.curves_uploaded = self.curves.len();
        }
        if self.band_indices.len() > self.band_indices_uploaded {
            let off = (self.band_indices_uploaded * core::mem::size_of::<u32>()) as u64;
            queue.write_buffer(
                &self.band_indices_buf,
                off,
                pod_as_bytes(&self.band_indices[self.band_indices_uploaded..]),
            );
            self.band_indices_uploaded = self.band_indices.len();
        }
        if self.bands.len() > self.bands_uploaded {
            let off = (self.bands_uploaded * core::mem::size_of::<SlugBand>()) as u64;
            queue.write_buffer(&self.bands_buf, off, pod_as_bytes(&self.bands[self.bands_uploaded..]));
            self.bands_uploaded = self.bands.len();
        }
        if self.instances.len() > self.instances_uploaded {
            let off = (self.instances_uploaded * core::mem::size_of::<SlugGlyphInstance>()) as u64;
            queue.write_buffer(
                &self.instances_buf,
                off,
                pod_as_bytes(&self.instances[self.instances_uploaded..]),
            );
            self.instances_uploaded = self.instances.len();
        }
    }

    /// 在已有 `RenderPass`（与 OBB 座位同 Pass 零额外 RenderPass 切换！）中绘制区间 `start..end` 的 Slug 曲线字形。
    #[inline]
    pub(crate) fn draw_range<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>, start: u32, end: u32) {
        if end > start {
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &self.bind_group, &[]);
            rpass.draw(0..6, start..end);
        }
    }
}
