//! WebAssembly (`wasm32-unknown-unknown`) 与 HTML5 `<canvas>` 平台后端。
//!
//! 设计特点：
//! - **零外部 JS 框架/DOM 树同步开销**：在 WebAssembly 线性内存中直接维护 `Rgba8888` 像素帧缓冲与脏矩形列表；
//! - **增量脏矩形 `putImageData` 提交**：浏览器 `requestAnimationFrame` 仅在 `dirty_rects_count > 0` 时调用
//!   `ctx.putImageData(imageData, 0, 0, rect.x, rect.y, rect.w, rect.h)` 局部刷新变化区域；
//! - **完整支持桌面与移动 Web 浏览器交互**：统一桥接 `PointerEvent`、触摸滚动、`WheelEvent`、键盘导航、
//!   浏览器历史后退 (`popstate` -> `InputEvent::NavigateBack`) 与 `env(safe-area-inset-*)`。

use std::vec;
use std::vec::Vec;

use crate::arena::SmallVec;
use crate::display::Rgba8888;
use crate::engine::VeloxEngine;
use crate::platform::mobile::CDirtyRect;
use crate::platform::testing::TestingPlatform;
use crate::solver::Region;

/// `#![no_std]` 固定分辨率 WebAssembly `<canvas>` 平台宿主（默认 `320x240`，动态或高分辨率建议使用 [`WebCanvasApp`]）。
///
/// 统一复用泛型 [`TestingPlatform<W, H, Rgba8888>`]。
pub type WebPlatform<const W: usize = 320, const H: usize = 240> =
    TestingPlatform<W, H, Rgba8888>;

/// 面向自适应网页窗口的动态分辨率 WebAssembly `<canvas>` 应用实例。
pub struct WebCanvasApp {
    pub engine: VeloxEngine,
    width: i16,
    height: i16,
    dpi_scale: i16,
    framebuffer: Vec<Rgba8888>,
    dirty_rects: Vec<CDirtyRect>,
}

impl WebCanvasApp {
    /// 创建指定初始画布宽高 `(width, height)` 的 WebAssembly 应用宿主。
    pub fn new(width: i16, height: i16) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            engine: VeloxEngine::new(w, h),
            width: w,
            height: h,
            dpi_scale: 1,
            framebuffer: vec![Rgba8888(0); (w as usize) * (h as usize)],
            dirty_rects: Vec::with_capacity(8),
        }
    }

    /// 设置物理像素 HiDPI 缩放倍率（如 Retina `2x` / `3x`），布局与点击事件保持逻辑坐标 `(width, height)`。
    pub fn set_dpi_scale(&mut self, scale: i16) {
        let s = scale.clamp(1, 4);
        if s != self.dpi_scale {
            self.dpi_scale = s;
            self.engine.set_dpi_scale(s);
            let pw = (self.width as usize) * (s as usize);
            let ph = (self.height as usize) * (s as usize);
            self.framebuffer.resize(pw * ph, Rgba8888(0));
            self.engine.resize(self.width, self.height);
        }
    }

    /// 获取当前 HiDPI 缩放倍率。
    #[inline(always)]
    pub const fn dpi_scale(&self) -> i16 {
        self.dpi_scale
    }

    /// 响应浏览器 `ResizeObserver` 或 `window.onresize` 动态调整画布分辨率。
    pub fn resize(&mut self, width: i16, height: i16) {
        let w = width.max(1);
        let h = height.max(1);
        if w != self.width || h != self.height {
            self.width = w;
            self.height = h;
            let pw = (w as usize) * (self.dpi_scale as usize);
            let ph = (h as usize) * (self.dpi_scale as usize);
            self.framebuffer.resize(pw * ph, Rgba8888(0));
            self.engine.resize(w, h);
        }
    }

    /// 同步浏览器 `env(safe-area-inset-*)` 安全区边距。
    pub fn set_safe_area(&mut self, top: i16, right: i16, bottom: i16, left: i16) {
        self.engine
            .set_safe_area(crate::pod::SafeAreaInsets::new(top, right, bottom, left));
    }

    /// 注入 Web 宿主唤醒回调（例如 `queueMicrotask` / `requestAnimationFrame` 触发器）。
    pub fn set_async_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.engine.set_async_waker(waker);
    }

    /// 排空浏览器主线程中所有已就绪的前景异步协程与 `defer` 回调。
    #[inline]
    pub fn poll_async_tasks(&mut self) -> bool {
        self.engine.poll_async_tasks()
    }

    /// 是否存在待重绘脏视图、动画或已就绪异步任务（用于决定浏览器是否按需暂停 `requestAnimationFrame`）。
    #[inline]
    pub fn needs_redraw(&self) -> bool {
        self.engine.needs_redraw()
    }

    /// 推进一帧渲染并光栅化所有脏矩形，返回本帧脏矩形个数。
    pub fn step_frame(&mut self, dt_ms: u16) -> usize {
        self.dirty_rects.clear();
        let s = self.dpi_scale.max(1);
        let mut regions: SmallVec<Region, 8> = SmallVec::new();
        for &r in self.engine.step_frame_dt(dt_ms) {
            regions.push(r);
        }
        for &reg in regions.as_slice() {
            self.engine
                .rasterize_region(reg, self.framebuffer.as_mut_slice());
            self.dirty_rects.push(CDirtyRect {
                x: reg.x.saturating_mul(s),
                y: reg.y.saturating_mul(s),
                w: reg.w.saturating_mul(s),
                h: reg.h.saturating_mul(s),
            });
        }
        self.dirty_rects.len()
    }

    /// 获取供浏览器 `ImageData` 零拷贝读取的 `RGBA8888` 字节切片。
    #[inline(always)]
    pub fn rgba_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.framebuffer.as_ptr() as *const u8,
                self.framebuffer.len() * 4,
            )
        }
    }

    /// 获取本帧脏矩形切片。
    #[inline(always)]
    pub fn dirty_rects(&self) -> &[CDirtyRect] {
        self.dirty_rects.as_slice()
    }

    /// 获取当前指针下方组件要求的鼠标光标形状（供 `<canvas>` 实时同步 `canvas.style.cursor`）。
    #[inline]
    pub fn cursor_shape(&self) -> crate::pod::CursorShape {
        self.engine.cursor_shape()
    }

    /// 获取当前鼠标光标对应的标准 CSS `cursor` 字符串。
    #[inline]
    pub fn cursor_css(&self) -> &'static str {
        self.engine.cursor_shape().as_css_str()
    }
}

/// 生成可直接由浏览器加载的 HTML5 `<canvas>` 宿主页面代码。
pub fn generate_web_index_html(wasm_url: &str, width: u16, height: u16) -> std::string::String {
    std::format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <link rel="icon" href="data:," />
  <title>Velox WebAssembly Application</title>
  <style>
    html, body {{ margin: 0; padding: 0; background: #0b0f17; display: flex; justify-content: center; align-items: center; height: 100vh; font-family: system-ui, sans-serif; }}
    canvas {{ width: {width}px; height: {height}px; border-radius: 12px; box-shadow: 0 16px 40px rgba(0,0,0,0.5); touch-action: none; }}
  </style>
</head>
<body>
  <canvas id="velox-canvas" width="{width}" height="{height}"></canvas>
  <script type="module">
{WEB_CANVAS_BOOTSTRAP_JS}
    mountVeloxCanvas("{wasm_url}", document.getElementById("velox-canvas"), "velox_wasm_app_init");
  </script>
</body>
</html>"#
    )
}

/// 为指定业务视图类型一键导出完整的 WebAssembly `<canvas>` C ABI (`velox_wasm_app_init` / `velox_web_app_*`) 符号。
#[macro_export]
macro_rules! export_web_app {
    ($app_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn velox_wasm_app_init(
            width: i16,
            height: i16,
        ) -> *mut $crate::platform::web::WebCanvasApp {
            let mut app = $crate::platform::web::WebCanvasApp::new(width, height);
            let _ = app.engine.mount_root(<$app_ty as Default>::default());
            std::boxed::Box::into_raw(std::boxed::Box::new(app))
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_destroy(
            app: *mut $crate::platform::web::WebCanvasApp,
        ) {
            if !app.is_null() {
                unsafe {
                    drop(std::boxed::Box::from_raw(app));
                }
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_resize(
            app: *mut $crate::platform::web::WebCanvasApp,
            width: i16,
            height: i16,
        ) {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                app_ref.resize(width, height);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_set_dpi_scale(
            app: *mut $crate::platform::web::WebCanvasApp,
            scale: i16,
        ) {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                app_ref.set_dpi_scale(scale);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_set_safe_area(
            app: *mut $crate::platform::web::WebCanvasApp,
            top: i16,
            right: i16,
            bottom: i16,
            left: i16,
        ) {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                app_ref.set_safe_area(top, right, bottom, left);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_pointer_kind(
            app: *mut $crate::platform::web::WebCanvasApp,
            pointer_kind: u8,
        ) {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                let kind = match pointer_kind {
                    0 => $crate::pod::PointerKind::Touch,
                    2 => $crate::pod::PointerKind::Pen,
                    _ => $crate::pod::PointerKind::Mouse,
                };
                app_ref.engine.set_pointer_kind(kind);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_pointer_event(
            app: *mut $crate::platform::web::WebCanvasApp,
            kind: u8,
            x: i16,
            y: i16,
        ) {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                let ev = match kind {
                    0 => $crate::pod::InputEvent::PointerDown { x, y },
                    1 => $crate::pod::InputEvent::PointerMove { x, y },
                    2 => $crate::pod::InputEvent::PointerUp { x, y },
                    _ => $crate::pod::InputEvent::PointerCancel,
                };
                app_ref.engine.dispatch_event(ev);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_wheel_event(
            app: *mut $crate::platform::web::WebCanvasApp,
            x: i16,
            y: i16,
            delta_y: i16,
        ) {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                app_ref
                    .engine
                    .dispatch_event($crate::pod::InputEvent::Scroll { x, y, delta_y });
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_popstate_back(
            app: *mut $crate::platform::web::WebCanvasApp,
        ) -> bool {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                app_ref.engine.dispatch_navigate_back()
            } else {
                false
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_step_frame(
            app: *mut $crate::platform::web::WebCanvasApp,
            dt_ms: u16,
        ) -> usize {
            if let Some(app_ref) = unsafe { app.as_mut() } {
                app_ref.step_frame(dt_ms)
            } else {
                0
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_rgba_ptr(
            app: *const $crate::platform::web::WebCanvasApp,
        ) -> *const u8 {
            if let Some(app_ref) = unsafe { app.as_ref() } {
                app_ref.rgba_bytes().as_ptr()
            } else {
                core::ptr::null()
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_rgba_byte_len(
            app: *const $crate::platform::web::WebCanvasApp,
        ) -> usize {
            if let Some(app_ref) = unsafe { app.as_ref() } {
                app_ref.rgba_bytes().len()
            } else {
                0
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_dirty_rects_ptr(
            app: *const $crate::platform::web::WebCanvasApp,
        ) -> *const $crate::platform::mobile::CDirtyRect {
            if let Some(app_ref) = unsafe { app.as_ref() } {
                app_ref.dirty_rects().as_ptr()
            } else {
                core::ptr::null()
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_web_app_dirty_rects_count(
            app: *const $crate::platform::web::WebCanvasApp,
        ) -> usize {
            if let Some(app_ref) = unsafe { app.as_ref() } {
                app_ref.dirty_rects().len()
            } else {
                0
            }
        }
    };
}

/// 浏览器端加载 `.wasm` 并绑定 `<canvas>` 增量脏矩形渲染与指针事件的最小 JS 引导脚本。
pub const WEB_CANVAS_BOOTSTRAP_JS: &str = r#"
function unwrapCoreWasm(u8) {
  if (u8[4] === 1) return u8;
  let pos = 8;
  while (pos < u8.length) {
    const secId = u8[pos++];
    let len = 0, shift = 0;
    while (pos < u8.length) {
      const b = u8[pos++];
      len |= (b & 0x7f) << shift;
      if ((b & 0x80) === 0) break;
      shift += 7;
    }
    if (secId === 1 && u8[pos] === 0x00 && u8[pos + 1] === 0x61 && u8[pos + 2] === 0x73 && u8[pos + 3] === 0x6d && u8[pos + 4] === 0x01) {
      return u8.subarray(pos, pos + len);
    }
    pos += len;
  }
  return u8;
}

export async function mountVeloxCanvas(wasmUrl, canvas, createAppExport = "velox_wasm_app_init") {
  const wasiStub = new Proxy({}, { get: () => new Proxy({}, { get: () => () => 0 }) });
  const sep = wasmUrl.includes("?") ? "&" : "?";
  const resp = await fetch(`${wasmUrl}${sep}_t=${Date.now()}`, { cache: "no-store" });
  const rawBytes = new Uint8Array(await resp.arrayBuffer());
  const coreBytes = unwrapCoreWasm(rawBytes);
  const { instance } = await WebAssembly.instantiate(coreBytes, wasiStub);
  const exports = instance.exports;
  const rect = canvas.getBoundingClientRect();
  const logicalW = (rect.width | 0) || canvas.width;
  const logicalH = (rect.height | 0) || canvas.height;
  const dpr = Math.min(3, Math.max(1, Math.round(window.devicePixelRatio || 1)));
  canvas.width = logicalW * dpr;
  canvas.height = logicalH * dpr;
  const app = exports[createAppExport](logicalW, logicalH);
  if (typeof exports.velox_web_app_set_dpi_scale === "function" && dpr > 1) {
    exports.velox_web_app_set_dpi_scale(app, dpr);
  }
  const ctx = canvas.getContext("2d");

  canvas.addEventListener("pointerdown", e => {
    if (typeof exports.velox_web_app_pointer_kind === "function") {
      const pk = e.pointerType === "touch" ? 0 : (e.pointerType === "pen" ? 2 : 1);
      exports.velox_web_app_pointer_kind(app, pk);
    }
    exports.velox_web_app_pointer_event(app, 0, e.offsetX | 0, e.offsetY | 0);
  });
  canvas.addEventListener("pointermove", e => exports.velox_web_app_pointer_event(app, 1, e.offsetX | 0, e.offsetY | 0));
  canvas.addEventListener("pointerup", e => exports.velox_web_app_pointer_event(app, 2, e.offsetX | 0, e.offsetY | 0));
  canvas.addEventListener("pointercancel", e => exports.velox_web_app_pointer_event(app, 3, 0, 0));
  canvas.addEventListener("wheel", e => {
    e.preventDefault();
    exports.velox_web_app_wheel_event(app, e.offsetX | 0, e.offsetY | 0, (-e.deltaY * 0.5) | 0);
  }, { passive: false });
  window.addEventListener("popstate", () => exports.velox_web_app_popstate_back(app));

  let lastTs = performance.now();
  function tick(now) {
    const dt = Math.min(100, Math.max(1, (now - lastTs) | 0));
    lastTs = now;
    const dirtyCount = exports.velox_web_app_step_frame(app, dt);
    if (dirtyCount > 0) {
      const w = canvas.width, h = canvas.height;
      const rgbaPtr = exports.velox_web_app_rgba_ptr(app);
      const rgbaLen = exports.velox_web_app_rgba_byte_len(app);
      const img = new ImageData(new Uint8ClampedArray(exports.memory.buffer, rgbaPtr, rgbaLen), w, h);
      const dirtyPtr = exports.velox_web_app_dirty_rects_ptr(app);
      const rects = new Int16Array(exports.memory.buffer, dirtyPtr, dirtyCount * 4);
      for (let i = 0; i < dirtyCount; i++) {
        const x = rects[i * 4], y = rects[i * 4 + 1], rw = rects[i * 4 + 2], rh = rects[i * 4 + 3];
        ctx.putImageData(img, 0, 0, x, y, rw, rh);
      }
    }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);
  return { app, exports };
}
"#;

/// WebAssembly 浏览器硬件渲染后端偏好（WebGPU 优先，自动回退 WebGL2 或 Canvas 2D 软光栅）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum WebGpuBackendPreference {
    #[default]
    WebGpuFirst = 0,
    WebGl2Only = 1,
    Software2dFallback = 2,
}

/// 面向浏览器 WebGPU / WebGL2 硬件直出的 `<canvas>` 宿主控制器（与软件 [`WebCanvasApp`] 并行）。
pub struct WebGpuCanvasHost {
    pub engine: VeloxEngine,
    logical_width: i16,
    logical_height: i16,
    device_pixel_ratio: f32,
    preference: WebGpuBackendPreference,
    gpu_active: bool,
}

impl WebGpuCanvasHost {
    pub fn new(
        logical_width: i16,
        logical_height: i16,
        device_pixel_ratio: f32,
        preference: WebGpuBackendPreference,
    ) -> Self {
        let w = logical_width.max(1);
        let h = logical_height.max(1);
        let dpr = device_pixel_ratio.clamp(0.5, 4.0);
        let mut engine = VeloxEngine::new(w, h);
        engine.set_scale_factor(dpr);
        Self {
            engine,
            logical_width: w,
            logical_height: h,
            device_pixel_ratio: dpr,
            preference,
            gpu_active: preference != WebGpuBackendPreference::Software2dFallback,
        }
    }

    #[inline]
    pub const fn physical_size(&self) -> (u32, u32) {
        let pw = ((self.logical_width as f32) * self.device_pixel_ratio) as u32;
        let ph = ((self.logical_height as f32) * self.device_pixel_ratio) as u32;
        (if pw < 1 { 1 } else { pw }, if ph < 1 { 1 } else { ph })
    }

    #[inline]
    pub const fn preference(&self) -> WebGpuBackendPreference {
        self.preference
    }

    #[inline]
    pub const fn is_gpu_active(&self) -> bool {
        self.gpu_active
    }

    pub fn resize(&mut self, logical_width: i16, logical_height: i16, device_pixel_ratio: f32) {
        self.logical_width = logical_width.max(1);
        self.logical_height = logical_height.max(1);
        self.device_pixel_ratio = device_pixel_ratio.clamp(0.5, 4.0);
        self.engine.set_scale_factor(self.device_pixel_ratio);
        self.engine.resize(self.logical_width, self.logical_height);
    }

    /// 步进一帧逻辑并返回是否存在脏区需要驱动 WebGPU/WebGL2 `WgpuRenderer::render_to_view`。
    pub fn step_needs_present(&mut self, dt_ms: u16) -> bool {
        !self.engine.step_frame_dt(dt_ms).is_empty()
    }
}

/// WebGPU / WebGL2 硬件直出前端桥接脚本模板（探测 `navigator.gpu` 并在无 WebGPU 环境优雅降级）。
pub const WEBGPU_HOST_JS_BRIDGE: &str = r#"// Velox WebGPU / WebGL2 Hardware Surface Bridge
export async function mountVeloxWebGpu(canvas, wasmUrl) {
  const hasWebGpu = typeof navigator !== "undefined" && "gpu" in navigator;
  const dpr = Math.min(4, Math.max(1, window.devicePixelRatio || 1));
  canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
  canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
  const { instance } = await WebAssembly.instantiateStreaming(fetch(wasmUrl), {});
  const exports = instance.exports;
  const app = exports.velox_webgpu_app_new(canvas.clientWidth | 0, canvas.clientHeight | 0, dpr, hasWebGpu ? 0 : 1);
  return { app, exports, backend: hasWebGpu ? "webgpu" : "webgl2" };
}
"#;
