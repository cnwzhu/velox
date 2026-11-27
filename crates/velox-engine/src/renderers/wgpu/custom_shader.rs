//! 可选 `custom-shader` 逃生舱：允许应用层注册自定义 `wgpu::RenderPipeline` 并按背景/前景顺序嵌入 `SceneCanvas` 渲染流程。
//!
//! 仅当启用 `custom-shader`（依赖 `gpu`）Feature 时编译，关闭时零依赖、零代码体积。

use ::wgpu;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::canvas_scene::{CustomShaderId, Viewport2D};

/// 传递给自定义 WGSL 着色器的 2D 视口与画布裁剪上下文（物理像素单位）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct CustomShaderViewport {
    /// 渲染目标物理宽高 `[phys_w, phys_h]`。
    pub target_size: [f32; 2],
    /// 画布屏幕左上角物理像素坐标 `[canvas_x, canvas_y]`。
    pub canvas_origin: [f32; 2],
    /// 画布物理宽高 `[canvas_w, canvas_h]`。
    pub canvas_size: [f32; 2],
    /// 相机偏移（物理像素）与缩放倍率 `[offset_x_phys, offset_y_phys, zoom_phys, scale_factor]`。
    pub camera: [f32; 4],
    /// 目标表面是否为 sRGB 格式（`1.0` / `0.0`）。
    pub is_srgb: f32,
    pub _pad: f32,
}

impl CustomShaderViewport {
    #[inline]
    pub fn from_viewport2d(
        target_w: u32,
        target_h: u32,
        canvas_x: f32,
        canvas_y: f32,
        canvas_w: f32,
        canvas_h: f32,
        vp: Viewport2D,
        scale_factor: f32,
        is_srgb: bool,
    ) -> Self {
        Self {
            target_size: [target_w as f32, target_h as f32],
            canvas_origin: [canvas_x, canvas_y],
            canvas_size: [canvas_w, canvas_h],
            camera: [
                canvas_x + vp.offset_x * scale_factor,
                canvas_y + vp.offset_y * scale_factor,
                vp.zoom * scale_factor,
                scale_factor,
            ],
            is_srgb: if is_srgb { 1.0 } else { 0.0 },
            _pad: 0.0,
        }
    }
}

/// 仅在启用 `custom-shader` feature 时开放的自定义 WGSL 渲染图元接口。
///
/// 实现者负责在 `prepare` 中按需创建/复用自身的 `wgpu::RenderPipeline` 与 `BindGroup`，
/// 并在 `render` 中向当前画布的 `wgpu::RenderPass`（已应用画布裁剪 `set_scissor_rect`）录制绘制指令。
pub trait CustomShaderPrimitive: Send + Sync + 'static {
    /// 首次绑定设备或每帧渲染前调用：用于懒初始化 `wgpu::RenderPipeline` 并上传本帧 `uniform_bytes` 与 `viewport`。
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        viewport: &CustomShaderViewport,
        uniform_bytes: &[u8],
    );

    /// 在当前画布裁剪区域与图层顺序（`Background` / `Foreground`）内录制自定义 GPU 绘制指令。
    fn render<'pass>(&'pass self, render_pass: &mut wgpu::RenderPass<'pass>);
}

struct RegisteredEntry {
    _type_id: TypeId,
    shader: Arc<dyn CustomShaderPrimitive>,
}

static NEXT_CUSTOM_SHADER_ID: AtomicU32 = AtomicU32::new(1);

fn global_registry() -> &'static Mutex<HashMap<CustomShaderId, RegisteredEntry>> {
    static REG: OnceLock<Mutex<HashMap<CustomShaderId, RegisteredEntry>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 在全局线程安全注册表中注册一个自定义 WGSL 着色器图元并返回稳定的 [`CustomShaderId`] 句柄。
///
/// 视图层无需直接持有 `&mut WgpuRenderer` 即可注册，并将 `CustomShaderId` 填入 `SceneCanvasSpec`。
pub fn register_custom_shader<S: CustomShaderPrimitive>(shader: S) -> CustomShaderId {
    let id = CustomShaderId(NEXT_CUSTOM_SHADER_ID.fetch_add(1, Ordering::Relaxed));
    let entry = RegisteredEntry {
        _type_id: TypeId::of::<S>(),
        shader: Arc::new(shader),
    };
    if let Ok(mut map) = global_registry().lock() {
        map.insert(id, entry);
    }
    id
}

/// 注销不再使用的自定义 WGSL 着色器并释放其 GPU 管线引用。
pub fn unregister_custom_shader(id: CustomShaderId) -> bool {
    global_registry()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&id))
        .is_some()
}

/// 内部查找已注册的自定义着色器句柄。
pub(crate) fn lookup_custom_shader(id: CustomShaderId) -> Option<Arc<dyn CustomShaderPrimitive>> {
    global_registry()
        .lock()
        .ok()
        .and_then(|map| map.get(&id).map(|e| Arc::clone(&e.shader)))
}
