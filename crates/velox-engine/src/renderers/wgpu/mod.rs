//! 自研轻量级 `wgpu` 2D SDF GPU 硬件加速渲染后端 `velox_engine::renderers::wgpu`。
//!
//! 模块划分：
//! - `shader.wgsl`：单 Pass / 单 Draw Call 2D SDF 合批着色器（Quad / 圆角裁切 / Evan Wallace 解析高斯阴影 / 字形 / 精灵 / 原生圆弧）；
//! - `atlas`：Guillotine 二维装箱动态 `R8Unorm` 字形图集与跨帧内容哈希驻留 `Rgba8Unorm` 精灵图集；
//! - `pipeline`：[`WgpuRenderer`] 核心管线、BindGroup、InstanceBuffer、文本排版缓存与 `DrawOp` 合批编码；
//! - `window`：[`WgpuWindowRenderer`] 跨平台 Swapchain 直出窗口渲染器（桌面 `winit` 与移动端共用）；
//! - `offscreen`：[`WgpuOffscreenRenderer`] 硬件离屏回读渲染器（无 GPU 自动回退 `software` 光栅）。

pub use ::wgpu;

mod atlas;
mod offscreen;
mod pipeline;
mod window;

pub use offscreen::WgpuOffscreenRenderer;
pub use pipeline::WgpuRenderer;
pub use window::{PreparedWgpuContext, WgpuWindowRenderer};

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};
use std::sync::{Arc, Condvar, Mutex};
use std::task::Wake;
use std::vec::Vec;

use crate::motion::{DrawOp, DrawPayload};
use crate::renderers::software::compute_unoccluded_clip;
use crate::solver::Region;
use crate::style::Color;

/// 单个 2D GPU 渲染图元实例（严格对齐 GPU 128B Cache Line 与 WGSL `std430`，`8 × vec4<f32> = 128 Bytes`）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct GpuPrimitive {
    /// `[x, y, w, h]` 物理像素绘制边界。
    pub bounds: [f32; 4],
    /// `[x0, y0, x1, y1]` 物理像素裁剪边界（片元级精确裁剪，避免打断合批）。
    pub clip: [f32; 4],
    /// `Glyph`/`Sprite` 归一化图集 UV `[u0, v0, u1, v1]`；`Arc` 为 `[start_rad, sweep_rad, fill_sweep_rad, thickness_px]`。
    pub uv: [f32; 4],
    /// 主填充色 / 文本色 / 阴影色 / `Arc` 轨道色（直通 RGBA `[0.0..=1.0]`）。
    pub color0: [f32; 4],
    /// 线性渐变终点色 / `Arc` 填充色（直通 RGBA `[0.0..=1.0]`）。
    pub color1: [f32; 4],
    /// 边框颜色（直通 RGBA `[0.0..=1.0]`）。
    pub border_color: [f32; 4],
    /// `[corner_radius, border_or_blur, kind, horiz_grad]`：
    /// - `kind`: `0.0` = Quad，`1.0` = SoftShadow，`2.0` = Glyph，`3.0` = Sprite，`4.0` = Arc；
    /// - `horiz_grad`: `1.0` = 水平渐变，`0.0` = 垂直渐变。
    pub params: [f32; 4],
    /// 扩展参数：`[clip_radius, 0.0, 0.0, 0.0]`（`clip_radius > 0` 时对 `clip` 启用抗锯齿圆角 SDF 裁切）。
    pub extra: [f32; 4],
}

pub(crate) const KIND_QUAD: f32 = 0.0;
pub(crate) const KIND_SHADOW: f32 = 1.0;
pub(crate) const KIND_GLYPH: f32 = 2.0;
pub(crate) const KIND_SPRITE: f32 = 3.0;
pub(crate) const KIND_ARC: f32 = 4.0;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ViewportUniform {
    pub(crate) size: [f32; 2],
    pub(crate) is_srgb: f32,
    pub(crate) _pad: f32,
}

#[inline]
pub(crate) fn pod_as_bytes<T: Copy>(slice: &[T]) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            core::mem::size_of_val(slice),
        )
    }
}

#[inline(always)]
pub(crate) fn color_to_f32(c: Color) -> [f32; 4] {
    const INV: f32 = 1.0 / 255.0;
    [
        (c.r() as f32) * INV,
        (c.g() as f32) * INV,
        (c.b() as f32) * INV,
        (c.a() as f32) * INV,
    ]
}

#[inline]
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_quad_primitive(
    bounds: [f32; 4],
    clip: [f32; 4],
    clip_radius: f32,
    corner_radius: f32,
    border_width: f32,
    horiz_grad: bool,
    bg: Color,
    bg_grad: Color,
    border_color: Color,
) -> GpuPrimitive {
    GpuPrimitive {
        bounds,
        clip,
        uv: [0.0; 4],
        color0: color_to_f32(bg),
        color1: color_to_f32(bg_grad),
        border_color: color_to_f32(border_color),
        params: [
            corner_radius,
            border_width,
            KIND_QUAD,
            if horiz_grad { 1.0 } else { 0.0 },
        ],
        extra: [clip_radius, 0.0, 0.0, 0.0],
    }
}

#[inline]
pub(crate) fn make_shadow_primitive(
    bounds: [f32; 4],
    clip: [f32; 4],
    clip_radius: f32,
    corner_radius: f32,
    blur_radius: f32,
    color: Color,
) -> GpuPrimitive {
    GpuPrimitive {
        bounds,
        clip,
        uv: [0.0; 4],
        color0: color_to_f32(color),
        color1: [0.0; 4],
        border_color: [0.0; 4],
        params: [corner_radius, blur_radius, KIND_SHADOW, 0.0],
        extra: [clip_radius, 0.0, 0.0, 0.0],
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_arc_primitive(
    bounds: [f32; 4],
    clip: [f32; 4],
    clip_radius: f32,
    thickness_px: f32,
    start_deg10: u16,
    sweep_deg10: u16,
    fill_sweep_deg10: u16,
    track_color: Color,
    fill_color: Color,
) -> GpuPrimitive {
    const DEG10_TO_RAD: f32 = core::f32::consts::PI / 1800.0;
    let start_rad = ((start_deg10 % 3600) as f32) * DEG10_TO_RAD;
    let sweep_rad = (sweep_deg10.clamp(20, 3590) as f32) * DEG10_TO_RAD;
    let fill_rad = (fill_sweep_deg10.min(sweep_deg10) as f32) * DEG10_TO_RAD;
    GpuPrimitive {
        bounds,
        clip,
        uv: [start_rad, sweep_rad, fill_rad, thickness_px],
        color0: color_to_f32(track_color),
        color1: color_to_f32(fill_color),
        border_color: [0.0; 4],
        params: [0.0, 0.0, KIND_ARC, 0.0],
        extra: [clip_radius, 0.0, 0.0, 0.0],
    }
}

/// 将逻辑矩形按分数/整数 `scale` 映射并对齐至物理像素边界（Pixel Snapping）。
///
/// 对 `1px` 细线元素（如分割线、代码缩进参考线与文本输入光标）锁定恒定物理线宽 `scale.round().max(1.0)`，
/// 消除 125% / 150% / 175% 分数 DPI 下因端点各自取整导致的 1px/2px 粗细跳变。
#[inline(always)]
pub(crate) fn snap_bounds(r: Region, scale: f32) -> [f32; 4] {
    let x0 = ((r.x as f32) * scale).round();
    let y0 = ((r.y as f32) * scale).round();
    let w = if r.w == 1 {
        scale.round().max(1.0)
    } else {
        (((r.right() as f32) * scale).round() - x0).max(0.0)
    };
    let h = if r.h == 1 {
        scale.round().max(1.0)
    } else {
        (((r.bottom() as f32) * scale).round() - y0).max(0.0)
    };
    [x0, y0, w, h]
}

/// 将逻辑裁剪框按 `scale` 映射并对齐至 `[x0, y0, x1, y1]` 物理像素边界。
#[inline(always)]
pub(crate) fn snap_clip(r: Region, scale: f32) -> [f32; 4] {
    [
        ((r.x as f32) * scale).round(),
        ((r.y as f32) * scale).round(),
        ((r.right() as f32) * scale).round(),
        ((r.bottom() as f32) * scale).round(),
    ]
}

/// 创建低内存配置的 `wgpu::Instance`（Windows 默认使用系统 FXC、默认不加载体积庞大的 SDK 验证层，
/// 需要调试底层图形 API 校验错误时仍可通过 `WGPU_VALIDATION=1` 按需开启）。
pub(crate) fn create_wgpu_instance(backends: ::wgpu::Backends) -> ::wgpu::Instance {
    let mut backend_options = ::wgpu::BackendOptions::from_env_or_default();
    #[cfg(target_os = "windows")]
    if ::wgpu::Dx12Compiler::from_env().is_none() {
        backend_options.dx12.shader_compiler = ::wgpu::Dx12Compiler::Fxc;
    }
    ::wgpu::Instance::new(::wgpu::InstanceDescriptor {
        backends,
        flags: ::wgpu::InstanceFlags::empty().with_env(),
        backend_options,
        memory_budget_thresholds: ::wgpu::MemoryBudgetThresholds::default(),
        display: None,
    })
}

/// 阻塞推进同步 `wgpu` 初始化 Future（零外部 async runtime 依赖）。
pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
    struct ThreadSignal {
        ready: Mutex<bool>,
        cv: Condvar,
    }
    impl Wake for ThreadSignal {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if let Ok(mut guard) = self.ready.lock() {
                *guard = true;
                self.cv.notify_one();
            }
        }
    }

    let signal = Arc::new(ThreadSignal {
        ready: Mutex::new(false),
        cv: Condvar::new(),
    });
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => {
                if let Ok(mut guard) = signal.ready.lock() {
                    while !*guard {
                        guard = match signal.cv.wait(guard) {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                    }
                    *guard = false;
                }
            }
        }
    }
}

/// 轻量 CPU 侧 `wgpu` 场景图元记录（用于无头驱动统计与测试断言，无需初始化 GPU 上下文）。
#[derive(Clone, Debug, Default)]
pub struct WgpuScene {
    pub primitives: Vec<GpuPrimitive>,
    pub quad_count: usize,
    pub shadow_count: usize,
    pub text_run_count: usize,
    pub sprite_count: usize,
    pub arc_count: usize,
}

impl WgpuScene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.primitives.clear();
        self.quad_count = 0;
        self.shadow_count = 0;
        self.text_run_count = 0;
        self.sprite_count = 0;
        self.arc_count = 0;
    }

    /// 将 [`DrawOp`] 流降维统计并收集为 CPU 侧 [`WgpuScene`]。
    pub fn record_draw_ops(
        &mut self,
        ops: &[DrawOp<'_>],
        dpi_scale: i16,
        dirty_regions: &[Region],
    ) -> usize {
        self.clear();
        if dirty_regions.is_empty() {
            return 0;
        }
        let s = (dpi_scale.max(1)) as f32;
        for &dirty in dirty_regions {
            if dirty.is_empty() {
                continue;
            }
            for (idx, op) in ops.iter().enumerate() {
                let raw_clip = dirty.intersect(op.clip).intersect(op.bounds);
                if raw_clip.is_empty() {
                    continue;
                }
                let eff_clip = compute_unoccluded_clip(raw_clip, &ops[(idx + 1)..]);
                if eff_clip.is_empty() {
                    continue;
                }
                let sb = snap_bounds(op.bounds, s);
                let clip_region = if op.clip_radius > 0 { op.clip } else { eff_clip };
                let sc = snap_clip(clip_region, s);
                let cr = (op.clip_radius as f32) * s;
                match op.payload {
                    DrawPayload::Quad {
                        bg,
                        bg_grad,
                        border_color,
                        rounded,
                        border_width,
                    } => {
                        let bw = ((border_width & 0x7F) as f32) * s;
                        let horiz = (border_width & 0x80) != 0;
                        let r = (rounded as f32) * s;
                        let is_shadow = border_color.a() == 0 && (border_width & 0x7F) > 0;
                        if is_shadow {
                            if bg.a() > 0 {
                                self.shadow_count += 1;
                                self.primitives
                                    .push(make_shadow_primitive(sb, sc, cr, r, bw, bg));
                            }
                        } else if bg.a() > 0 || bg_grad.a() > 0 || (bw > 0.0 && border_color.a() > 0)
                        {
                            self.quad_count += 1;
                            self.primitives.push(make_quad_primitive(
                                sb,
                                sc,
                                cr,
                                r,
                                bw,
                                horiz,
                                bg,
                                bg_grad,
                                border_color,
                            ));
                        }
                    }
                    DrawPayload::TextRun { text, color, .. } => {
                        if !text.is_empty() && color.a() > 0 {
                            self.text_run_count += 1;
                        }
                    }
                    DrawPayload::Sprite(crate::element::SpriteSource::Arc {
                        thickness,
                        start_deg10,
                        sweep_deg10,
                        fill_sweep_deg10,
                        track_color,
                        fill_color,
                    }) => {
                        self.arc_count += 1;
                        self.primitives.push(make_arc_primitive(
                            sb,
                            sc,
                            cr,
                            (thickness as f32) * s,
                            start_deg10,
                            sweep_deg10,
                            fill_sweep_deg10,
                            track_color,
                            fill_color,
                        ));
                    }
                    DrawPayload::Sprite(_) => {
                        self.sprite_count += 1;
                    }
                }
            }
        }
        self.primitives.len() + self.text_run_count + self.sprite_count
    }
}
