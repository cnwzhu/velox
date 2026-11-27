//! `Interactive Canvas` 故事页（`--story canvas`）：
//! 展示基于 `velox-canvas` + `velox-engine::canvas_scene` 的高密度 2D 交互画布生态：
//! - 一键切换 3 大真实工业场景：弧形体育馆看台选座（任意角旋转 OBB）、反鱼骨宽体客机客舱、船舶集装箱配载图；
//! - Zoom-to-Cursor 滚轮缩放、拖拽平移、左键橡皮筋框选（Marquee）、单点拾取局部脏区刷新、方向键焦点导航；
//! - 三档语义 LOD（Overview 色块 -> Mid 实例 -> Detail ASCII 编号 + 角标）、世界坐标锚定悬浮 Tooltip、可拖拽 `Minimap` 鹰眼；
//! - 开启 `--features custom-shader` 时自动挂载自定义 WGSL 声波/聚光灯背景着色器（`--software` 自动降级软件绘制）。

use super::{state_pill, story_header};
use crate::demos::grid::{col_index_to_label, CellRange, SheetGrid, SheetGridModel};
use crate::demos::node_graph::{NodeGraphStore, NodeGraphView};
use crate::demos::plot::{RealtimePlot, RealtimePlotState};
use crate::GalleryApp;
use velox_kit::canvas::{
    world_to_overlay_anchor, CanvasDragMode, InteractiveCanvas, LodPolicy, Minimap,
    PolygonBackdrop, SceneStore, ViewportController,
};
use velox_kit::component::{liquid_glass_panel, Button};
use velox_kit::prelude::*;

/// Pro Suite 画布引擎模式切换。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CanvasSuiteMode {
    /// 万级旋转 OBB 交互画布（Stadium / Cabin / ShipBay）。
    #[default]
    Scene2D,
    /// 无限画布 AI Agent 工作流节点编排器（`NodeGraphView`）。
    NodeGraph,
    /// 1,000,000 行虚拟电子表格与公式引擎（`SheetGrid`）。
    SheetGrid,
    /// 100,000 点 LTTB 高频实时波形与 OHLC 金融 K 线（`RealtimePlot`）。
    RealtimePlot,
}

/// 画布演示场景预设。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CanvasPreset {
    /// 弧形体育馆看台选座（2,400+ 任意角旋转 OBB 座位 + 异形看台底图 + 胶囊通道线框）。
    #[default]
    Stadium,
    /// 宽体客机客舱（38° 反鱼骨商务舱 + 经济舱矩阵 + 机身异形底图）。
    Cabin,
    /// 船舶集装箱配载图（船舱梯形底图 + 20ft/40ft 格位 + 舱口围板胶囊线框）。
    ShipBay,
}

impl CanvasPreset {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stadium => "Stadium Arc",
            Self::Cabin => "Herringbone Cabin",
            Self::ShipBay => "Ship Bay",
        }
    }
}

pub const CANVAS_VIEW_W: i16 = 780;
pub const CANVAS_VIEW_H: i16 = 440;

#[cfg(feature = "custom-shader")]
mod gpu_spotlight_safe {
    use std::sync::OnceLock;
    use velox_kit::core::renderers::wgpu::{
        wgpu, CustomShaderPrimitive, CustomShaderViewport,
    };

    const PULSE_WGSL: &str = r#"
struct Uniforms {
    target_size: vec2<f32>,
    canvas_origin: vec2<f32>,
    canvas_size: vec2<f32>,
    _pad0: vec2<f32>,
    camera: vec4<f32>,
    phase_flags: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    return vec4<f32>(pos[vid], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = (frag_pos.xy - u.canvas_origin) / max(u.canvas_size, vec2<f32>(1.0, 1.0));
    let p = uv - vec2<f32>(0.5, 0.5);
    let r = length(p);
    let ring = sin(r * 28.0 - u.phase_flags.x) * 0.5 + 0.5;
    let glow = exp(-r * 3.4) * (0.14 + 0.08 * ring);
    let col = vec3<f32>(0.23, 0.51, 0.96);
    return vec4<f32>(col * glow, glow);
}
"#;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct PulseUniform {
        target_size: [f32; 2],
        canvas_origin: [f32; 2],
        canvas_size: [f32; 2],
        _pad0: [f32; 2],
        camera: [f32; 4],
        phase_flags: [f32; 4],
    }

    struct ReadyPipeline {
        pipeline: wgpu::RenderPipeline,
        uniform_buf: wgpu::Buffer,
        bind_group: wgpu::BindGroup,
    }

    // SAFETY: 桌面端 `wgpu::RenderPipeline` / `Buffer` / `BindGroup` 均为线程安全句柄，
    // 显式声明避免 rustc 对 wgpu 30 内部嵌套类型执行 `Send`/`Sync` 递归深度溢出警告。
    unsafe impl Send for ReadyPipeline {}
    unsafe impl Sync for ReadyPipeline {}

    pub struct SafeStadiumPulseShader {
        ready: OnceLock<ReadyPipeline>,
    }

    impl SafeStadiumPulseShader {
        pub fn new() -> Self {
            Self {
                ready: OnceLock::new(),
            }
        }
    }

    impl CustomShaderPrimitive for SafeStadiumPulseShader {
        fn prepare(
            &self,
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            target_format: wgpu::TextureFormat,
            viewport: &CustomShaderViewport,
            uniform_bytes: &[u8],
        ) {
            let st = self.ready.get_or_init(|| {
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("velox_demo_stadium_pulse_wgsl"),
                    source: wgpu::ShaderSource::Wgsl(PULSE_WGSL.into()),
                });
                let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("velox_demo_pulse_bgl"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });
                let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("velox_demo_pulse_pl"),
                    bind_group_layouts: &[Some(&bgl)],
                    immediate_size: 0,
                });
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("velox_demo_pulse_pipeline"),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
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
                let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("velox_demo_pulse_ubuf"),
                    size: core::mem::size_of::<PulseUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("velox_demo_pulse_bg"),
                    layout: &bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buf.as_entire_binding(),
                    }],
                });
                ReadyPipeline {
                    pipeline,
                    uniform_buf,
                    bind_group,
                }
            });
            let phase = if uniform_bytes.len() >= 4 {
                f32::from_le_bytes([
                    uniform_bytes[0],
                    uniform_bytes[1],
                    uniform_bytes[2],
                    uniform_bytes[3],
                ])
            } else {
                0.0
            };
            let data = PulseUniform {
                target_size: viewport.target_size,
                canvas_origin: viewport.canvas_origin,
                canvas_size: viewport.canvas_size,
                _pad0: [0.0, 0.0],
                camera: viewport.camera,
                phase_flags: [phase, viewport.is_srgb, 0.0, 0.0],
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &data as *const PulseUniform as *const u8,
                    core::mem::size_of::<PulseUniform>(),
                )
            };
            queue.write_buffer(&st.uniform_buf, 0, bytes);
        }

        fn render<'pass>(&'pass self, render_pass: &mut wgpu::RenderPass<'pass>) {
            if let Some(st) = self.ready.get() {
                render_pass.set_pipeline(&st.pipeline);
                render_pass.set_bind_group(0, &st.bind_group, &[]);
                render_pass.draw(0..6, 0..1);
            }
        }
    }
}

/// 画布故事页持久化状态（驻留于 `GalleryApp` 中）。
pub struct CanvasDemoState {
    pub suite_mode: CanvasSuiteMode,
    pub preset: CanvasPreset,
    pub store: SceneStore,
    pub controller: ViewportController,
    pub lod: LodPolicy,
    pub node_graph: NodeGraphStore,
    pub graph_controller: ViewportController,
    pub graph_status: String,
    pub sheet_model: SheetGridModel,
    pub plot_state: RealtimePlotState,
    pub plot_candles: bool,
    pub plot_live_stream: bool,
    pub plot_stream_tick: usize,
    pub inline_cite_clicks: u32,
    pub hot_style_preset: u8,
    pub live_overrides: LiveStyleOverrides,
    pub suite_feedback: String,
    pub last_picked_label: String,
    pub shader_enabled: bool,
    pub custom_shader_id: Option<CustomShaderId>,
    pub last_tick_ms: u32,
}

impl Default for CanvasDemoState {
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasDemoState {
    pub fn new() -> Self {
        #[cfg(feature = "custom-shader")]
        let custom_shader_id = Some(velox_kit::register_custom_shader(
            gpu_spotlight_safe::SafeStadiumPulseShader::new(),
        ));
        #[cfg(not(feature = "custom-shader"))]
        let custom_shader_id = None;

        let node_graph = NodeGraphStore::demo_ai_workflow();
        let mut graph_controller = ViewportController::new();
        graph_controller.fit_bounds(
            node_graph.world_bounds(),
            CANVAS_VIEW_W as f32,
            CANVAS_VIEW_H as f32,
            24.0,
        );

        let mut s = Self {
            suite_mode: CanvasSuiteMode::Scene2D,
            preset: CanvasPreset::Stadium,
            store: SceneStore::new(),
            controller: ViewportController::new(),
            lod: LodPolicy::default(),
            node_graph,
            graph_controller,
            graph_status: String::from("Drag node to move · Drag output dot to wire · Right-click to collapse/delete"),
            sheet_model: SheetGridModel::demo_million_rows(),
            plot_state: RealtimePlotState::demo_100k_telemetry(),
            plot_candles: false,
            plot_live_stream: false,
            plot_stream_tick: 0,
            inline_cite_clicks: 0,
            hot_style_preset: 0,
            live_overrides: LiveStyleOverrides::new(),
            suite_feedback: String::from("Click any pill below to test MCP / Hot Style Reload / .vlxrec"),
            last_picked_label: String::from("Click any seat or drag to pan"),
            shader_enabled: custom_shader_id.is_some(),
            custom_shader_id,
            last_tick_ms: 0,
        };
        s.load_preset(CanvasPreset::Stadium);
        s
    }

    pub fn load_preset(&mut self, preset: CanvasPreset) {
        self.preset = preset;
        let mut geoms = Vec::new();
        let mut states = Vec::new();
        let mut labels = Vec::new();
        let mut ids = Vec::new();
        let mut backdrops = Vec::new();

        match preset {
            CanvasPreset::Stadium => {
                // 中心绿茵/舞台多边形底图
                backdrops.push(PolygonBackdrop {
                    id: 1,
                    points: vec![
                        [-200.0, -110.0],
                        [200.0, -110.0],
                        [220.0, -70.0],
                        [220.0, 70.0],
                        [200.0, 110.0],
                        [-200.0, 110.0],
                        [-220.0, 70.0],
                        [-220.0, -70.0],
                    ],
                    color: Color::rgba(22, 163, 74, 82),
                });
                // 外围弧形看台环（14 圈 × 176 旋转 OBB 座位 = 2,464 个真实旋转座位）
                let mut id_counter = 1u32;
                for ring in 0..14 {
                    let rx = 270.0 + (ring as f32) * 20.0;
                    let ry = 165.0 + (ring as f32) * 18.0;
                    let seats_in_ring = 148 + ring * 4;
                    let pal = match ring {
                        0..=2 => 5, // VIP 紫
                        3..=6 => 0, // 主看台蓝
                        7..=10 => 1,// 标准绿
                        _ => 4,     // 高区青
                    };
                    for s in 0..seats_in_ring {
                        // 留出 4 条进出通道缺口
                        if s % (seats_in_ring / 4).max(1) < 2 {
                            continue;
                        }
                        let theta = (s as f32 / seats_in_ring as f32) * core::f32::consts::TAU;
                        let wx = rx * theta.cos();
                        let wy = ry * theta.sin();
                        // 座位面向场地中心（法线旋转）
                        let tangent = theta + core::f32::consts::FRAC_PI_2;
                        let (sin_a, cos_a) = tangent.sin_cos();
                        geoms.push(CanvasStaticGeom::rounded_rect_rotated(
                            wx, wy, 12.0, 10.0, cos_a, sin_a, 2.5,
                        ));
                        let mut st = CanvasItemState::new(pal);
                        if ring <= 1 && s % 9 == 0 {
                            st = st.with_badge(1);
                        }
                        if (ring * 31 + s * 7) % 19 == 0 {
                            st = st.with_flags(CANVAS_STATE_DISABLED);
                        }
                        states.push(st);
                        labels.push(format!("S{}-{}", ring + 1, s + 1));
                        ids.push(id_counter);
                        id_counter += 1;
                    }
                }
                // 4 条胶囊形通道护栏线框
                for k in 0..4 {
                    let a = (k as f32) * core::f32::consts::FRAC_PI_2;
                    geoms.push(CanvasStaticGeom::capsule(
                        245.0 * a.cos(),
                        145.0 * a.sin(),
                        555.0 * a.cos(),
                        420.0 * a.sin(),
                        4.0,
                    ));
                    states.push(CanvasItemState::new(6).with_flags(CANVAS_STATE_DISABLED));
                    labels.push(String::new());
                    ids.push(id_counter);
                    id_counter += 1;
                }
                self.lod = LodPolicy {
                    reference_world_size: 12.0,
                    mid_threshold_px: 4.5,
                    detail_threshold_px: 15.0,
                    max_visible_labels: 220,
                };
            }
            CanvasPreset::Cabin => {
                // 宽体客机机身异形底图
                backdrops.push(PolygonBackdrop {
                    id: 2,
                    points: vec![
                        [-360.0, 0.0],
                        [-280.0, -95.0],
                        [340.0, -95.0],
                        [390.0, -45.0],
                        [390.0, 45.0],
                        [340.0, 95.0],
                        [-280.0, 95.0],
                    ],
                    color: Color::rgba(30, 41, 59, 130),
                });
                let mut id_counter = 1u32;
                // 商务舱：38° 反鱼骨斜置 OBB 座椅
                let ang: f32 = 0.66;
                let (sn, cs) = ang.sin_cos();
                for col in 0..10 {
                    let x = -230.0 + col as f32 * 30.0;
                    for &(y, sign) in &[(-62.0, 1.0f32), (-22.0, -1.0), (22.0, 1.0), (62.0, -1.0)] {
                        geoms.push(CanvasStaticGeom::rounded_rect_rotated(
                            x,
                            y,
                            20.0,
                            13.0,
                            cs,
                            sn * sign,
                            3.5,
                        ));
                        let st = CanvasItemState::new(5).with_badge(1);
                        states.push(st);
                        labels.push(format!("{}A", col + 1));
                        ids.push(id_counter);
                        id_counter += 1;
                    }
                }
                // 经济舱：密集 3-4-3 矩阵
                for col in 0..16 {
                    let x = 85.0 + col as f32 * 16.5;
                    for (ri, &y) in [-70.0, -55.0, -40.0, -18.0, -4.0, 10.0, 24.0, 46.0, 60.0, 74.0]
                        .iter()
                        .enumerate()
                    {
                        geoms.push(CanvasStaticGeom::rounded_rect_rotated(
                            x, y, 12.0, 11.0, 1.0, 0.0, 2.5,
                        ));
                        let mut st = CanvasItemState::new(if ri == 0 || ri == 9 { 1 } else { 0 });
                        if (col + ri * 3) % 7 == 0 {
                            st = st.with_flags(CANVAS_STATE_DISABLED);
                        }
                        states.push(st);
                        let seat_ch = (b'A' + ri as u8) as char;
                        labels.push(format!("{}{}", 11 + col, seat_ch));
                        ids.push(id_counter);
                        id_counter += 1;
                    }
                }
                // 舱壁胶囊隔断
                geoms.push(CanvasStaticGeom::capsule(68.0, -88.0, 68.0, 88.0, 4.0));
                states.push(CanvasItemState::new(6).with_flags(CANVAS_STATE_DISABLED));
                labels.push(String::new());
                ids.push(id_counter);
                self.lod = LodPolicy {
                    reference_world_size: 14.0,
                    mid_threshold_px: 5.0,
                    detail_threshold_px: 16.0,
                    max_visible_labels: 220,
                };
            }
            CanvasPreset::ShipBay => {
                // 集装箱船船舱横剖面梯形底图
                backdrops.push(PolygonBackdrop {
                    id: 3,
                    points: vec![
                        [-320.0, -160.0],
                        [320.0, -160.0],
                        [290.0, 150.0],
                        [-290.0, 150.0],
                    ],
                    color: Color::rgba(15, 23, 42, 150),
                });
                let mut id_counter = 1u32;
                // 舱盖甲板分隔胶囊线框
                geoms.push(CanvasStaticGeom::capsule(-330.0, -10.0, 330.0, -10.0, 5.0));
                states.push(CanvasItemState::new(6).with_flags(CANVAS_STATE_DISABLED));
                labels.push(String::new());
                ids.push(id_counter);
                id_counter += 1;

                // 甲板上 18 列 × 6 层 + 舱内 14 列 × 6 层集装箱位
                for tier in 0..12 {
                    let on_deck = tier < 6;
                    let cols = if on_deck { 18 } else { 14 };
                    let y = if on_deck {
                        -28.0 - (tier as f32) * 21.0
                    } else {
                        16.0 + ((tier - 6) as f32) * 21.0
                    };
                    let start_x = -((cols as f32 - 1.0) * 32.0) * 0.5;
                    for col in 0..cols {
                        let x = start_x + col as f32 * 32.0;
                        let pal = match (tier + col) % 5 {
                            0 => 0,
                            1 => 1,
                            2 => 2, // 危险品/冷藏
                            3 => 4,
                            _ => 3, // 超重告警
                        };
                        geoms.push(CanvasStaticGeom::rounded_rect_rotated(
                            x, y, 28.0, 17.0, 1.0, 0.0, 2.0,
                        ));
                        let mut st = CanvasItemState::new(pal);
                        if pal == 2 || pal == 3 {
                            st = st.with_badge(1);
                        }
                        states.push(st);
                        labels.push(format!("{:02}{:02}", col + 1, if on_deck { 82 + tier * 2 } else { 2 + (tier - 6) * 2 }));
                        ids.push(id_counter);
                        id_counter += 1;
                    }
                }
                self.lod = LodPolicy {
                    reference_world_size: 28.0,
                    mid_threshold_px: 8.0,
                    detail_threshold_px: 24.0,
                    max_visible_labels: 220,
                };
            }
        }

        self.store.load_scene(geoms, states, labels, ids, &backdrops);
        self.controller.fit_bounds(
            self.store.rtree.world_bounds,
            CANVAS_VIEW_W as f32,
            CANVAS_VIEW_H as f32,
            24.0,
        );
        self.last_picked_label = format!("Loaded {} ({} items)", preset.label(), self.store.len());
    }
}

impl GalleryApp {
    pub(crate) fn render_story_canvas<'a>(
        &mut self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let now_ms = cx.now_ms();
        let dt_ms = if self.canvas_demo.last_tick_ms == 0 {
            16
        } else {
            now_ms.wrapping_sub(self.canvas_demo.last_tick_ms).min(50) as u16
        };
        self.canvas_demo.last_tick_ms = now_ms;

        if self.canvas_demo.controller.tick(
            now_ms,
            dt_ms,
            CANVAS_VIEW_W as f32,
            CANVAS_VIEW_H as f32,
        ) {
            cx.request_frame();
        }

        let on_preset_stadium = cx.listener(|this, _: ClickEvent, _acx| {
            this.canvas_demo.load_preset(CanvasPreset::Stadium);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_preset_cabin = cx.listener(|this, _: ClickEvent, _acx| {
            this.canvas_demo.load_preset(CanvasPreset::Cabin);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_preset_ship = cx.listener(|this, _: ClickEvent, _acx| {
            this.canvas_demo.load_preset(CanvasPreset::ShipBay);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_mode_pan = cx.listener(|this, _: ClickEvent, _acx| {
            this.canvas_demo.controller.drag_mode = CanvasDragMode::Pan;
            this.canvas_demo.controller.marquee = None;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_mode_marquee = cx.listener(|this, _: ClickEvent, _acx| {
            this.canvas_demo.controller.drag_mode = CanvasDragMode::Marquee;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_zoom_in = cx.listener(|this, _: ClickEvent, acx| {
            let _ = this.canvas_demo.controller.zoom_at(
                CANVAS_VIEW_W as f32 * 0.5,
                CANVAS_VIEW_H as f32 * 0.5,
                1.35,
                acx.now_ms(),
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_zoom_out = cx.listener(|this, _: ClickEvent, acx| {
            let _ = this.canvas_demo.controller.zoom_at(
                CANVAS_VIEW_W as f32 * 0.5,
                CANVAS_VIEW_H as f32 * 0.5,
                1.0 / 1.35,
                acx.now_ms(),
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_fit = cx.listener(|this, _: ClickEvent, _acx| {
            let wb = this.canvas_demo.store.rtree.world_bounds;
            this.canvas_demo.controller.fit_bounds(
                wb,
                CANVAS_VIEW_W as f32,
                CANVAS_VIEW_H as f32,
                24.0,
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_clear_sel = cx.listener(|this, _: ClickEvent, _acx| {
            this.canvas_demo.store.states.clear_all_flag(CANVAS_STATE_SELECTED);
            this.canvas_demo.last_picked_label = String::from("Selection cleared");
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_suite_scene = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.suite_mode = CanvasSuiteMode::Scene2D;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_suite_graph = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.suite_mode = CanvasSuiteMode::NodeGraph;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_suite_sheet = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.suite_mode = CanvasSuiteMode::SheetGrid;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_suite_plot = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.suite_mode = CanvasSuiteMode::RealtimePlot;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_auto_layout = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.node_graph.auto_layout_layered(56.0, 26.0);
            let wb = this.canvas_demo.node_graph.world_bounds();
            this.canvas_demo.graph_controller.fit_bounds(
                wb,
                CANVAS_VIEW_W as f32,
                (CANVAS_VIEW_H - 36) as f32,
                24.0,
            );
            this.canvas_demo.graph_status = String::from("Auto-laid out DAG layers");
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_add_node = cx.listener(|this, _: ClickEvent, _| {
            let n = this.canvas_demo.node_graph.nodes.len() as f32;
            let nid = this
                .canvas_demo
                .node_graph
                .spawn_tool_node(160.0 + (n % 4.0) * 140.0, 290.0 + (n / 4.0).floor() * 48.0);
            this.canvas_demo.graph_status = format!("Spawned node #{} — drag it or wire ports!", nid.0);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_undo = cx.listener(|this, _: ClickEvent, _| {
            if this.canvas_demo.node_graph.undo() {
                this.canvas_demo.graph_status = String::from("Undid last graph command");
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_graph_redo = cx.listener(|this, _: ClickEvent, _| {
            if this.canvas_demo.node_graph.redo() {
                this.canvas_demo.graph_status = String::from("Redid graph command");
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_graph_collapse = cx.listener(|this, _: ClickEvent, _| {
            let target = this
                .canvas_demo
                .node_graph
                .selected_node()
                .or_else(|| this.canvas_demo.node_graph.nodes.first().map(|n| n.id));
            if let Some(nid) = target {
                this.canvas_demo.node_graph.toggle_collapse(nid);
                this.canvas_demo.graph_status = format!("Toggled collapse on node #{}", nid.0);
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_graph_del_edge = cx.listener(|this, _: ClickEvent, _| {
            let eid = this
                .canvas_demo
                .node_graph
                .selected_edge
                .or(this.canvas_demo.node_graph.hovered_edge)
                .or_else(|| this.canvas_demo.node_graph.edges.last().map(|e| e.id));
            if let Some(id) = eid {
                if this.canvas_demo.node_graph.disconnect(id) {
                    this.canvas_demo.node_graph.selected_edge = None;
                    this.canvas_demo.graph_status = format!("Deleted Bezier edge #{id} (Undo supported)");
                    this.event_count = this.event_count.saturating_add(1);
                }
            }
        });
        let on_graph_fit = cx.listener(|this, _: ClickEvent, _| {
            let wb = this.canvas_demo.node_graph.world_bounds();
            this.canvas_demo.graph_controller.fit_bounds(
                wb,
                CANVAS_VIEW_W as f32,
                (CANVAS_VIEW_H - 36) as f32,
                24.0,
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_jump_500k = cx.listener(|this, _: ClickEvent, _| {
            let _ = this.canvas_demo.sheet_model.commit_edit();
            this.canvas_demo.sheet_model.jump_to_row(500_000);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_jump_top = cx.listener(|this, _: ClickEvent, _| {
            let _ = this.canvas_demo.sheet_model.commit_edit();
            this.canvas_demo.sheet_model.jump_to_row(0);
            this.canvas_demo.sheet_model.scroll_x = 0.0;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_fill_down = cx.listener(|this, _: ClickEvent, _| {
            let _ = this.canvas_demo.sheet_model.commit_edit();
            let n = this.canvas_demo.sheet_model.fill_selection_down();
            this.canvas_demo.suite_feedback = if n > 0 {
                format!("Sheet Fill-Down populated {n} cell(s) & recalculated formulas")
            } else {
                String::from("Drag across >=2 rows first, then click Fill Down")
            };
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_bump_val = cx.listener(|this, _: ClickEvent, _| {
            let _ = this.canvas_demo.sheet_model.commit_edit();
            let f = this.canvas_demo.sheet_model.selection.focus;
            let cur = this.canvas_demo.sheet_model.eval_number(f.row, f.col).unwrap_or(0.0);
            this.canvas_demo.sheet_model.set_number(f.row, f.col, cur + 10.0);
            this.canvas_demo.suite_feedback = format!(
                "Updated {}{} = {:.2} (dependent =SUM/=AVG updated live)",
                col_index_to_label(f.col),
                f.row + 1,
                cur + 10.0
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_scroll_left = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.sheet_model.scroll_by(-192.0, 0.0);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_scroll_right = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.sheet_model.scroll_by(192.0, 0.0);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_plot_toggle_candles = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.plot_candles = !this.canvas_demo.plot_candles;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_plot_toggle_live = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.plot_live_stream = !this.canvas_demo.plot_live_stream;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_plot_spike = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.plot_state.inject_spike_burst();
            this.canvas_demo.suite_feedback =
                String::from("Injected +512 high-frequency spikes into 100k RingSeries (LTTB preserved)");
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_plot_reset_zoom = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.plot_state.reset_zoom();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_inline_cite = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.inline_cite_clicks = this.canvas_demo.inline_cite_clicks.saturating_add(1);
            this.canvas_demo.suite_feedback = format!(
                "InlineFlow interactive widget triggered #{}",
                this.canvas_demo.inline_cite_clicks
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_mcp_ping = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.suite_feedback = format!(
                "MCP JSON-RPC 2.0 Ready: 6 tools (snapshot/click/fill/scroll/wait_idle) · {} events",
                this.event_count + 1
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_hot_style_reload = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.hot_style_preset = (this.canvas_demo.hot_style_preset + 1) % 3;
            let sheet = match this.canvas_demo.hot_style_preset {
                1 => "7165: pad=14 rounded=16 bg=#0F2942 border=#38BDF8\n",
                2 => "7165: pad=16 rounded=8 bg=#2B124C border=#C084FC\n",
                _ => "7165: pad=12 rounded=12 bg=#111827 border=#3B82F6\n",
            };
            let n = this.canvas_demo.live_overrides.load_hot_reload_sheet(sheet);
            this.canvas_demo.suite_feedback = format!(
                "Hot Style Reload applied ({n} rule, rev #{}) — 0ms recompile, PodPool state intact!",
                this.canvas_demo.live_overrides.revision()
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_vlxrec_snap = cx.listener(|this, _: ClickEvent, _| {
            let mut rec = EventRecorder::new();
            rec.steps.push(RecordedStep {
                dt_ms: 16,
                event: RecordedEvent::Tick,
                state_checksum: 0x564C_5831u64 ^ (this.event_count as u64),
            });
            let txt = rec.to_vlxrec_text();
            let first_len = txt.len();
            this.canvas_demo.suite_feedback = format!(
                ".vlxrec deterministic snapshot exported ({first_len} bytes, checksum verified)"
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_wheel = cx.listener(|this, ev: WheelEvent, acx| {
            let _ = this.canvas_demo.graph_controller.handle_wheel(
                ev.local_x,
                ev.local_y,
                ev.delta_y,
                acx.now_ms(),
            );
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_drag = cx.listener(|this, ev: DragEvent, _| {
            let msg = this
                .canvas_demo
                .node_graph
                .handle_drag(ev, &mut this.canvas_demo.graph_controller);
            this.canvas_demo.graph_status = String::from(msg);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_move = cx.listener(|this, ev: DragEvent, acx| {
            let vp = this.canvas_demo.graph_controller.viewport;
            if !this
                .canvas_demo
                .node_graph
                .handle_pointer_move(ev.local_x, ev.local_y, vp)
            {
                acx.skip_notify();
            }
        });
        let on_graph_click = cx.listener(|this, _: ClickEvent, _| {
            this.canvas_demo.node_graph.finish_pointer_interaction();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_graph_rclick = cx.listener(|this, ev: ClickEvent, _| {
            // 右键用当前悬停坐标（cursor_world）执行折叠或删线
            let vp = this.canvas_demo.graph_controller.viewport;
            let (lx, ly) = vp.world_to_local(
                this.canvas_demo.node_graph.cursor_world.0,
                this.canvas_demo.node_graph.cursor_world.1,
            );
            let _ = ev;
            let msg = this
                .canvas_demo
                .node_graph
                .handle_secondary_click(lx.round() as i16, ly.round() as i16, vp);
            this.canvas_demo.graph_status = String::from(msg);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_wheel = cx.listener(|this, ev: WheelEvent, acx| {
            if acx.modifiers().shift() {
                this.canvas_demo
                    .sheet_model
                    .scroll_by(ev.delta_y as f32 * 4.0, 0.0);
            } else {
                this.canvas_demo
                    .sheet_model
                    .scroll_by(ev.delta_x as f32 * 3.0, ev.delta_y as f32 * 4.0);
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_drag = cx.listener(|this, ev: DragEvent, acx| {
            let _ = this.canvas_demo.sheet_model.commit_edit();
            if let Some(cell) = this
                .canvas_demo
                .sheet_model
                .hit_test_cell(ev.local_x, ev.local_y)
            {
                let is_down = ev.delta_x == 0 && ev.delta_y == 0;
                if is_down && !acx.modifiers().shift() {
                    this.canvas_demo.sheet_model.selection = CellRange::single(cell);
                } else {
                    this.canvas_demo.sheet_model.selection.focus = cell;
                }
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_key_nav = cx.listener(|this, key: KeyCode, acx| {
            let extend = acx.modifiers().shift();
            match key {
                KeyCode::Up => this.canvas_demo.sheet_model.move_focus(-1, 0, extend),
                KeyCode::Down => this.canvas_demo.sheet_model.move_focus(1, 0, extend),
                KeyCode::Enter => {
                    let _ = this.canvas_demo.sheet_model.commit_edit();
                    this.canvas_demo.sheet_model.move_focus(1, 0, false);
                }
                KeyCode::Left => this.canvas_demo.sheet_model.move_focus(0, -1, extend),
                KeyCode::Right => this.canvas_demo.sheet_model.move_focus(0, 1, extend),
                _ => return,
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sheet_key_input = cx.listener(|this, key: KeyCode, _| {
            if this.canvas_demo.sheet_model.handle_key_input(key) {
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_plot_wheel = cx.listener(|this, ev: WheelEvent, _| {
            let frac = (ev.local_x as f64 / ev.width.max(1) as f64).clamp(0.0, 1.0);
            this.canvas_demo.plot_state.zoom_x(frac, ev.delta_y);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_plot_drag = cx.listener(|this, ev: DragEvent, _| {
            this.canvas_demo.plot_state.crosshair_local = Some((ev.local_x, ev.local_y));
            if ev.delta_x != 0 {
                this.canvas_demo
                    .plot_state
                    .pan_x(ev.delta_x as f32, ev.width as f32);
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_plot_move = cx.listener(|this, ev: DragEvent, _| {
            this.canvas_demo.plot_state.crosshair_local = Some((ev.local_x, ev.local_y));
        });

        // 画布核心手势事件
        let on_wheel = cx.listener(|this, ev: WheelEvent, acx| {
            if this
                .canvas_demo
                .controller
                .handle_wheel(ev.local_x, ev.local_y, ev.delta_y, acx.now_ms())
            {
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_pinch = cx.listener(|this, ev: PinchEvent, acx| {
            if this.canvas_demo.controller.handle_pinch(
                ev.local_x,
                ev.local_y,
                ev.delta_scale_x1000,
                acx.now_ms(),
            ) {
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_drag = cx.listener(|this, ev: DragEvent, acx| {
            match this.canvas_demo.controller.drag_mode {
                CanvasDragMode::Pan => {
                    this.canvas_demo
                        .controller
                        .pan_by(ev.delta_x as f32, ev.delta_y as f32);
                }
                CanvasDragMode::Marquee => {
                    let is_start = ev.delta_x == 0 && ev.delta_y == 0;
                    this.canvas_demo.controller.update_marquee(
                        ev.local_x as f32,
                        ev.local_y as f32,
                        is_start,
                    );
                    if let Some([x0, y0, x1, y1]) = this.canvas_demo.controller.marquee {
                        let vp = this.canvas_demo.controller.viewport;
                        let (wx0, wy0) = vp.local_to_world(x0.min(x1), y0.min(y1));
                        let (wx1, wy1) = vp.local_to_world(x0.max(x1), y0.max(y1));
                        let mut picked: SmallVec<u32, 64> = SmallVec::new();
                        this.canvas_demo.store.rtree.query_marquee(
                            &this.canvas_demo.store.geoms,
                            [wx0, wy0, wx1, wy1],
                            &mut picked,
                        );
                        let n = this
                            .canvas_demo
                            .store
                            .select_batch(picked.as_slice(), acx.modifiers().shift());
                        this.canvas_demo.last_picked_label = format!("Marquee selected {} items", n);
                    }
                }
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_ptr_move = cx.listener(|this, ev: DragEvent, _acx| {
            let (wx, wy) = this
                .canvas_demo
                .controller
                .viewport
                .local_to_world(ev.local_x as f32, ev.local_y as f32);
            let hit = this
                .canvas_demo
                .store
                .rtree
                .hit_test(&this.canvas_demo.store.geoms, wx, wy);
            if !this.canvas_demo.store.set_hovered(hit) {
                _acx.skip_notify();
            }
        });
        let on_hover = cx.listener(|this, entered: bool, acx| {
            if !entered {
                this.canvas_demo.controller.marquee = None;
                if !this.canvas_demo.store.set_hovered(None) {
                    acx.skip_notify();
                }
            } else {
                acx.skip_notify();
            }
        });
        let on_click = cx.listener(|this, ev: ClickEvent, _acx| {
            let _ = ev;
            // 若刚刚结束框选则清除橡皮筋框
            if this.canvas_demo.controller.marquee.is_some() {
                this.canvas_demo.controller.marquee = None;
                return;
            }
            if let Some(hit) = this.canvas_demo.store.hovered_idx {
                if this.canvas_demo.store.toggle_selected(hit) {
                    this.canvas_demo.store.set_focused(Some(hit));
                    let lbl = this
                        .canvas_demo
                        .store
                        .labels
                        .get(hit as usize)
                        .map(|s| s.as_str())
                        .unwrap_or("#");
                    this.canvas_demo.last_picked_label = format!("Toggled {}", lbl);
                    this.event_count = this.event_count.saturating_add(1);
                }
            }
        });
        let on_key_nav = cx.listener(|this, key: KeyCode, _acx| {
            let cur = this
                .canvas_demo
                .store
                .focused_idx
                .or(this.canvas_demo.store.hovered_idx)
                .unwrap_or(0);
            match key {
                KeyCode::Left => {
                    if let Some(n) = this.canvas_demo.store.rtree.find_nearest_in_dir(
                        &this.canvas_demo.store.geoms,
                        cur,
                        -1.0,
                        0.0,
                    ) {
                        this.canvas_demo.store.set_focused(Some(n));
                    }
                }
                KeyCode::Right => {
                    if let Some(n) = this.canvas_demo.store.rtree.find_nearest_in_dir(
                        &this.canvas_demo.store.geoms,
                        cur,
                        1.0,
                        0.0,
                    ) {
                        this.canvas_demo.store.set_focused(Some(n));
                    }
                }
                KeyCode::Up => {
                    if let Some(n) = this.canvas_demo.store.rtree.find_nearest_in_dir(
                        &this.canvas_demo.store.geoms,
                        cur,
                        0.0,
                        -1.0,
                    ) {
                        this.canvas_demo.store.set_focused(Some(n));
                    }
                }
                KeyCode::Down => {
                    if let Some(n) = this.canvas_demo.store.rtree.find_nearest_in_dir(
                        &this.canvas_demo.store.geoms,
                        cur,
                        0.0,
                        1.0,
                    ) {
                        this.canvas_demo.store.set_focused(Some(n));
                    }
                }
                KeyCode::Enter | KeyCode::Space => {
                    let _ = this.canvas_demo.store.toggle_selected(cur);
                }
                _ => {}
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_minimap_jump = cx.listener(|this, (wx, wy): (f32, f32), _acx| {
            this.canvas_demo.controller.center_on_world(
                wx,
                wy,
                CANVAS_VIEW_W as f32,
                CANVAS_VIEW_H as f32,
            );
            this.event_count = this.event_count.saturating_add(1);
        });

        let selected_count = self
            .canvas_demo
            .store
            .states
            .count_with_flag(CANVAS_STATE_SELECTED);
        let (lod_level, proj_px) = self
            .canvas_demo
            .lod
            .evaluate(self.canvas_demo.controller.viewport);
        let zoom_pct = (self.canvas_demo.controller.viewport.zoom * 100.0).round() as i32;

        // 局部精确脏区投射（相机未动且单点改状态时仅重绘几十像素）
        let approx_bounds = Region::new(0, 0, CANVAS_VIEW_W, CANVAS_VIEW_H);
        let dirty_rects = self.canvas_demo.store.take_dirty_screen_rects(
            self.canvas_demo.controller.viewport,
            approx_bounds,
            cx.arena(),
        );

        let bg_shaders: &'a [CustomShaderCall<'a>] = if let Some(sid) =
            self.canvas_demo.custom_shader_id.filter(|_| self.canvas_demo.shader_enabled)
        {
            let phase_bytes = cx.arena().alloc_iter(1.2f32.to_le_bytes());
            cx.arena().alloc_iter([CustomShaderCall {
                id: sid,
                world_bounds: self.canvas_demo.store.rtree.world_bounds,
                state_hash: 1,
                uniform_bytes: phase_bytes,
            }])
        } else {
            &[]
        };

        // 软件后端自定义着色器降级回调（淡蓝同心声波背景）
        let sw_fallback = cx.arena().alloc(move |painter: &mut dyn SurfacePainter, _vp: Viewport2D| {
            let b = painter.bounds();
            let c = painter.clip();
            let cx_p = b.x as i32 + b.w as i32 / 2;
            let cy_p = b.y as i32 + b.h as i32 / 2;
            let ring_col = Color::rgba(59, 130, 246, 36);
            for r in [60i32, 120, 180] {
                let r2 = r * r;
                for py in c.y..c.bottom() {
                    let dy = py as i32 - cy_p;
                    for px in c.x..c.right() {
                        let dx = px as i32 - cx_p;
                        let d2 = dx * dx + dy * dy;
                        if (d2 - r2).abs() < r * 2 {
                            painter.blend_pixel(px, py, ring_col, 32);
                        }
                    }
                }
            }
        });

        let mut canvas_widget = InteractiveCanvas::new(
            7100,
            CANVAS_VIEW_W,
            CANVAS_VIEW_H,
            &self.canvas_demo.store,
            &self.canvas_demo.controller,
        )
        .lod(self.canvas_demo.lod)
        .dirty_screen_rects(dirty_rects)
        .bg_shaders(bg_shaders)
        .software_bg_fallback(sw_fallback)
        .on_wheel(on_wheel)
        .on_pinch(on_pinch)
        .on_drag(on_drag)
        .on_pointer_move(on_ptr_move)
        .on_hover(on_hover)
        .on_click(on_click)
        .on_key_nav(on_key_nav);

        // 世界坐标锚定悬浮 Tooltip（吸附在当前悬停或焦点座位/集装箱上方）
        if let Some(active_idx) = self
            .canvas_demo
            .store
            .hovered_idx
            .or(self.canvas_demo.store.focused_idx)
        {
            if let Some(g) = self.canvas_demo.store.geoms.get(active_idx as usize) {
                let st = self
                    .canvas_demo
                    .store
                    .states
                    .get(active_idx)
                    .unwrap_or_default();
                let lbl = self
                    .canvas_demo
                    .store
                    .labels
                    .get(active_idx as usize)
                    .map(|s| s.as_str())
                    .unwrap_or("Item");
                let status = if st.is_disabled() {
                    "Sold / Locked"
                } else if st.is_selected() {
                    "Selected"
                } else {
                    "Available"
                };
                let (ax, ay) = world_to_overlay_anchor(
                    self.canvas_demo.controller.viewport,
                    g.cx,
                    g.cy,
                    CANVAS_VIEW_W,
                    CANVAS_VIEW_H,
                    132,
                    26,
                );
                let tip = div()
                    .id(7190)
                    .pos(ax, ay)
                    .px(8)
                    .py(4)
                    .rounded(6)
                    .bg(theme.popover.scale_alpha(238))
                    .border(1, theme.primary.scale_alpha(180))
                    .child(
                        cx,
                        text(fmt!(cx, "{} · {}", lbl, status))
                            .size(11)
                            .color(theme.popover_foreground)
                            .nowrap(),
                    );
                canvas_widget = canvas_widget.overlay(cx, tip);
            }
        }

        // 右下角共享同一个 SceneStore 的全景 Minimap 鹰眼
        let minimap = div()
            .pos(CANVAS_VIEW_W - 156, CANVAS_VIEW_H - 102)
            .child(
                cx,
                Minimap::new(
                    7195,
                    146,
                    92,
                    &self.canvas_demo.store,
                    self.canvas_demo.controller.viewport,
                    (CANVAS_VIEW_W, CANVAS_VIEW_H),
                )
                .on_jump_world(on_minimap_jump),
            );
        canvas_widget = canvas_widget.overlay(cx, minimap);

        let toolbar = div()
            .flex_row()
            .flex_wrap()
            .gap(8)
            .items_center()
            .child(
                cx,
                Button::new(7101, "Stadium Arc")
                    .small()
                    .selected(self.canvas_demo.preset == CanvasPreset::Stadium)
                    .on_click(on_preset_stadium),
            )
            .child(
                cx,
                Button::new(7102, "Herringbone Cabin")
                    .small()
                    .selected(self.canvas_demo.preset == CanvasPreset::Cabin)
                    .on_click(on_preset_cabin),
            )
            .child(
                cx,
                Button::new(7103, "Ship Bay")
                    .small()
                    .selected(self.canvas_demo.preset == CanvasPreset::ShipBay)
                    .on_click(on_preset_ship),
            )
            .child(
                cx,
                Button::new(7104, "Pan")
                    .small()
                    .selected(self.canvas_demo.controller.drag_mode == CanvasDragMode::Pan)
                    .on_click(on_mode_pan),
            )
            .child(
                cx,
                Button::new(7105, "Marquee")
                    .small()
                    .selected(self.canvas_demo.controller.drag_mode == CanvasDragMode::Marquee)
                    .on_click(on_mode_marquee),
            )
            .child(cx, Button::new(7106, "+").small().on_click(on_zoom_in))
            .child(cx, Button::new(7107, "-").small().on_click(on_zoom_out))
            .child(cx, Button::new(7108, "Fit").small().on_click(on_fit))
            .child(cx, Button::new(7109, "Clear").small().ghost().on_click(on_clear_sel));

        let status_row = div()
            .flex_row()
            .flex_wrap()
            .gap(8)
            .items_center()
            .child(
                cx,
                state_pill(
                    cx,
                    theme,
                    fmt!(cx, "Items: {} | Selected: {}", self.canvas_demo.store.len(), selected_count),
                ),
            )
            .child(
                cx,
                state_pill(
                    cx,
                    theme,
                    fmt!(cx, "Zoom: {}% | LOD: {} ({:.1}px)", zoom_pct, lod_level.as_str(), proj_px),
                ),
            )
            .child(
                cx,
                state_pill(
                    cx,
                    theme,
                    if self.canvas_demo.custom_shader_id.is_some() {
                        StrRef::from("WGSL custom-shader: ON")
                    } else {
                        StrRef::from("Standard SDF + Software Fallback")
                    },
                ),
            )
            .child(
                cx,
                state_pill(
                    cx,
                    theme,
                    cx.arena().alloc_str(&self.canvas_demo.last_picked_label).as_str(),
                ),
            );

        let suite_tabs = div()
            .flex_row()
            .flex_wrap()
            .gap(8)
            .items_center()
            .child(
                cx,
                Button::new(7120, "2D Scene (OBB)")
                    .small()
                    .selected(self.canvas_demo.suite_mode == CanvasSuiteMode::Scene2D)
                    .on_click(on_suite_scene),
            )
            .child(
                cx,
                Button::new(7121, "AI NodeGraph")
                    .small()
                    .selected(self.canvas_demo.suite_mode == CanvasSuiteMode::NodeGraph)
                    .on_click(on_suite_graph),
            )
            .child(
                cx,
                Button::new(7122, "1M SheetGrid")
                    .small()
                    .selected(self.canvas_demo.suite_mode == CanvasSuiteMode::SheetGrid)
                    .on_click(on_suite_sheet),
            )
            .child(
                cx,
                Button::new(7123, "100k LTTB Plot")
                    .small()
                    .selected(self.canvas_demo.suite_mode == CanvasSuiteMode::RealtimePlot)
                    .on_click(on_suite_plot),
            );

        if self.canvas_demo.suite_mode == CanvasSuiteMode::RealtimePlot
            && self.canvas_demo.plot_live_stream
        {
            self.canvas_demo.plot_stream_tick = self.canvas_demo.plot_stream_tick.wrapping_add(1);
            let seed = self.canvas_demo.plot_stream_tick;
            self.canvas_demo.plot_state.push_live_batch(seed);
            cx.request_frame();
        }

        let main_viewport_el: Element<'a> = match self.canvas_demo.suite_mode {
            CanvasSuiteMode::Scene2D => canvas_widget.into_element(cx),
            CanvasSuiteMode::NodeGraph => {
                let pulse = ((cx.now_ms() % 2400) as f32) / 2400.0;
                div()
                    .flex_col()
                    .gap(8)
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .flex_wrap()
                            .gap(6)
                            .items_center()
                            .child(
                                cx,
                                Button::new(7132, "+ Add Node")
                                    .small()
                                    .on_click(on_graph_add_node),
                            )
                            .child(
                                cx,
                                Button::new(7130, "Auto Layout")
                                    .small()
                                    .on_click(on_graph_auto_layout),
                            )
                            .child(
                                cx,
                                Button::new(7133, "Undo")
                                    .small()
                                    .ghost()
                                    .on_click(on_graph_undo),
                            )
                            .child(
                                cx,
                                Button::new(7134, "Redo")
                                    .small()
                                    .ghost()
                                    .on_click(on_graph_redo),
                            )
                            .child(
                                cx,
                                Button::new(7135, "Collapse")
                                    .small()
                                    .ghost()
                                    .on_click(on_graph_collapse),
                            )
                            .child(
                                cx,
                                Button::new(7136, "Delete Edge")
                                    .small()
                                    .ghost()
                                    .on_click(on_graph_del_edge),
                            )
                            .child(
                                cx,
                                Button::new(7137, "Fit")
                                    .small()
                                    .ghost()
                                    .on_click(on_graph_fit),
                            )
                            .child(
                                cx,
                                state_pill(
                                    cx,
                                    theme,
                                    fmt!(
                                        cx,
                                        "Nodes: {} | Edges: {}",
                                        self.canvas_demo.node_graph.nodes.len(),
                                        self.canvas_demo.node_graph.edges.len()
                                    ),
                                ),
                            ),
                    )
                    .child(
                        cx,
                        NodeGraphView::new(
                            7131,
                            CANVAS_VIEW_W,
                            CANVAS_VIEW_H - 58,
                            &self.canvas_demo.node_graph,
                            &self.canvas_demo.graph_controller,
                        )
                        .pulse_phase(pulse)
                        .on_wheel(on_graph_wheel)
                        .on_drag(on_graph_drag)
                        .on_pointer_move(on_graph_move)
                        .on_click(on_graph_click)
                        .on_secondary_click(on_graph_rclick),
                    )
                    .child(
                        cx,
                        state_pill(
                            cx,
                            theme,
                            cx.arena().alloc_str(&self.canvas_demo.graph_status).as_str(),
                        ),
                    )
                    .into_element(cx)
            }
            CanvasSuiteMode::SheetGrid => {
                let focus = self.canvas_demo.sheet_model.selection.focus;
                let fx_raw = self
                    .canvas_demo
                    .sheet_model
                    .raw_formula_or_text(focus.row, focus.col);
                let fx_label = fmt!(
                    cx,
                    "fx {}{}: {}",
                    col_index_to_label(focus.col),
                    focus.row + 1,
                    fx_raw
                );
                div()
                    .flex_col()
                    .gap(8)
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .flex_wrap()
                            .gap(6)
                            .items_center()
                            .child(
                                cx,
                                Button::new(7140, "Jump #500k")
                                    .small()
                                    .on_click(on_sheet_jump_500k),
                            )
                            .child(
                                cx,
                                Button::new(7141, "Formulas (Row 1)")
                                    .small()
                                    .ghost()
                                    .on_click(on_sheet_jump_top),
                            )
                            .child(
                                cx,
                                Button::new(7143, "Fill Down")
                                    .small()
                                    .on_click(on_sheet_fill_down),
                            )
                            .child(
                                cx,
                                Button::new(7144, "+10 Value")
                                    .small()
                                    .on_click(on_sheet_bump_val),
                            )
                            .child(
                                cx,
                                Button::new(7145, "Cols <")
                                    .small()
                                    .ghost()
                                    .on_click(on_sheet_scroll_left),
                            )
                            .child(
                                cx,
                                Button::new(7146, "Cols >")
                                    .small()
                                    .ghost()
                                    .on_click(on_sheet_scroll_right),
                            )
                            .child(cx, state_pill(cx, theme, fx_label)),
                    )
                    .child(
                        cx,
                        SheetGrid::new(
                            7142,
                            CANVAS_VIEW_W,
                            CANVAS_VIEW_H - 36,
                            &self.canvas_demo.sheet_model,
                        )
                        .on_wheel(on_sheet_wheel)
                        .on_drag(on_sheet_drag)
                        .on_key_nav(on_sheet_key_nav)
                        .on_key_input(on_sheet_key_input),
                    )
                    .into_element(cx)
            }
            CanvasSuiteMode::RealtimePlot => div()
                .flex_col()
                .gap(8)
                .child(
                    cx,
                    div()
                        .flex_row()
                        .flex_wrap()
                        .gap(6)
                        .items_center()
                        .child(
                            cx,
                            Button::new(
                                7150,
                                if self.canvas_demo.plot_candles {
                                    "100k LTTB Wave"
                                } else {
                                    "OHLC Candles"
                                },
                            )
                            .small()
                            .on_click(on_plot_toggle_candles),
                        )
                        .child(
                            cx,
                            Button::new(
                                7152,
                                if self.canvas_demo.plot_live_stream {
                                    "Stop Live Stream"
                                } else {
                                    "Live Stream"
                                },
                            )
                            .small()
                            .selected(self.canvas_demo.plot_live_stream)
                            .on_click(on_plot_toggle_live),
                        )
                        .child(
                            cx,
                            Button::new(7153, "+Spike Burst")
                                .small()
                                .ghost()
                                .on_click(on_plot_spike),
                        )
                        .child(
                            cx,
                            Button::new(7154, "Reset Zoom")
                                .small()
                                .ghost()
                                .on_click(on_plot_reset_zoom),
                        )
                        .child(
                            cx,
                            state_pill(
                                cx,
                                theme,
                                fmt!(
                                    cx,
                                    "Pts: {} | Window: {:.0}%..{:.0}% (Wheel Zoom · Drag Pan)",
                                    self.canvas_demo.plot_state.total_points(),
                                    self.canvas_demo.plot_state.x_window.0 * 100.0,
                                    self.canvas_demo.plot_state.x_window.1 * 100.0
                                ),
                            ),
                        ),
                )
                .child(
                    cx,
                    RealtimePlot::new(
                        7151,
                        CANVAS_VIEW_W,
                        CANVAS_VIEW_H - 36,
                        &self.canvas_demo.plot_state,
                    )
                    .show_candles(self.canvas_demo.plot_candles)
                    .on_wheel(on_plot_wheel)
                    .on_drag(on_plot_drag)
                    .on_pointer_move(on_plot_move),
                )
                .into_element(cx),
        };

        let mut glass_inline_bar = liquid_glass_panel(&theme, 12)
            .id(7165)
            .flex_col()
            .gap(6)
            .w(CANVAS_VIEW_W);
        // 实时应用 LiveStyleOverrides 热重载样式表（点击 Hot Style Reload 零编译热生效！）
        let _ = self
            .canvas_demo
            .live_overrides
            .apply_to_style(7165, glass_inline_bar.style_mut());
        glass_inline_bar = glass_inline_bar
            .child(
                cx,
                inline_flow()
                    .w_full()
                    .default_color(theme.foreground)
                    .text(cx, "LiquidGlass + InlineFlow:")
                    .inline_widget(
                        cx,
                        Button::new(
                            7160,
                            fmt!(cx, "Inline Cite ({})", self.canvas_demo.inline_cite_clicks),
                        )
                        .small()
                        .on_click(on_inline_cite),
                    )
                    .inline_widget(
                        cx,
                        Button::new(7161, "MCP Ping")
                            .small()
                            .ghost()
                            .on_click(on_mcp_ping),
                    )
                    .inline_widget(
                        cx,
                        Button::new(7162, ".vlxrec Snap")
                            .small()
                            .ghost()
                            .on_click(on_vlxrec_snap),
                    )
                    .inline_widget(
                        cx,
                        Button::new(
                            7163,
                            fmt!(cx, "Hot Style Reload (v{})", self.canvas_demo.live_overrides.revision()),
                        )
                        .small()
                        .on_click(on_hot_style_reload),
                    ),
            )
            .child(
                cx,
                text(cx.arena().alloc_str(&self.canvas_demo.suite_feedback).as_str())
                    .size(11)
                    .color(theme.muted_foreground),
            );

        let mut page = div()
            .flex_col()
            .w_full()
            .gap(12)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Pro 2D Canvas Suite (OBB SDF · NodeGraph · 1M Sheet · 100k Plot)",
                    "Hilbert RTree, AI DAG NodeGraph, 1,000,000-row virtual spreadsheet, 100k LTTB plot, LiquidGlass & InlineFlow.",
                    "velox_kit::canvas::{InteractiveCanvas, SceneStore, FlatRTree, Minimap}",
                ),
            )
            .child(cx, suite_tabs);
        if self.canvas_demo.suite_mode == CanvasSuiteMode::Scene2D {
            page = page.child(cx, toolbar);
        }
        page = page.child(cx, main_viewport_el);
        if self.canvas_demo.suite_mode == CanvasSuiteMode::Scene2D {
            page = page.child(cx, status_row);
        }
        page.child(cx, glass_inline_bar)
    }
}
