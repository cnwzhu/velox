# 跨平台后端与多渲染器（Platforms & Renderers）

**Contents:** [统一跨平台后端概览](#统一跨平台后端概览) · [桌面端：极简启动 `application()`、`WinitPlatform` 与纯 `WaylandPlatform`](#桌面端极简启动-applicationwinitplatform-与纯-waylandplatform) · [移动端与 WebAssembly 宿主导出](#移动端与-webassembly-宿主导出) · [多渲染后端调度 DesktopRenderBackend](#多渲染后端调度-desktoprenderbackend) · [为自定义硬件实现 Platform / AsyncPlatform 与 AsyncDriver](#为自定义硬件实现-platform--asyncplatform-与-asyncdriver)

---

## 统一跨平台后端概览

Velox 将**窗口事件收集与像素/交换链呈现（`velox_kit::platform`）**与**图形光栅化渲染器（`velox_core::renderers`）**解耦，业务 `View` 代码在桌面、移动端与 Web 浏览器之间无需修改即可复用。

---

## 桌面端：极简启动 `application()`、`WinitPlatform` 与纯 `WaylandPlatform`

1. **极简桌面启动层（`velox_kit::app`，推荐）**：
   - `velox_kit::application().run(|cx| { velox_kit::init(cx); velox_kit::open_window(WindowOptions::new(title).size(w, h), cx, |cx| cx.new(|_| MyApp)).expect(...); })`；
   - 支持 `.with_theme(...)`、`.with_locale(...)`、`.with_backend(...)`、`.with_dpi_scale(scale)`、`.with_scheduler(...)`（开启 feature 后支持 `.with_tokio()` / `.with_smol()`），以及无弹窗复用的 `.build_headless(...)`。
2. **`WinitPlatform`**（Windows Win32 / macOS AppKit / Linux Wayland + `softbuffer`）：
   - 底层支持 `WinitPlatform::new(title, w, h).with_dpi_scale(2).with_backend(backend).run_event_loop(engine)`；
   - 自动注册 `EventLoopProxy<WinitUserEvent>`，后台异步任务完成或定时器到期时通过 `WinitUserEvent::WakeAsync` 精准唤醒处于 `ControlFlow::Wait` 的 OS 事件循环（空闲 0% CPU）；
   - 同时支持桌面鼠标（`PointerKind::Mouse`，禁用空白处误拖滚动）、触控屏（`WindowEvent::Touch` 自动切换 `PointerKind::Touch`）、实时缩放（`Resized`）、HiDPI 切换（`ScaleFactorChanged`）及 `render_to_png(&mut engine, path)` 无损快照导出。
3. **`WaylandPlatform`**（零 `winit`、零 X11 依赖的纯 Wayland 二进制线协议客户端）：
   - `WaylandPlatform::new_xdg_toplevel(title, app_id, w, h)`（标准应用窗口）；
   - `WaylandPlatform::new_layer_shell(namespace, exclusive_zone, w, h)`（面板、锁屏、壁纸或停靠栏服务）；
   - `WaylandPlatform::new_subsurface_decoration(parent_surface_id, w, h)`；
   - 内置 `wl_shm` 双缓冲及 Buffer Age = 2 历史脏区回溯补齐机制，仅通过 `wl_surface.damage_buffer` 向合成器提交局部脏矩形。

---

## 移动端与 WebAssembly 宿主导出

启用 `std`（或任意隐含 `std` 的默认 Feature）后，直接调用 `velox_kit` 顶层导出宏即可为你的业务 `View` 自动生成完整平台胶水 C ABI / JNI 符号（参见 `examples/{android_app,ios_app,harmony_app,web_app}`）：

| 目标平台 | 依赖配置 | 导出宏与核心宿主类型 | 特色能力 |
| --- | --- | --- | --- |
| Android | `default-features = false, features = ["std", "component"]`（可选加 `"gpu"`） | `velox_kit::export_android_app!(MyApp)` + `AndroidHostBridge<MyApp>` | `ANativeWindow`、`WindowInsetsCompat` 安全区/IME、物理返回键消费 |
| iOS / iPadOS | `default-features = false, features = ["std", "component"]`（可选加 `"gpu"`） | `velox_kit::export_ios_app!(MyApp)` + `IosHostBridge<MyApp>` | `UIKit`、`CAMetalLayer`、灵动岛/Home Indicator、Retina `@2x`/`@3x` |
| HarmonyOS NEXT | `default-features = false, features = ["std", "component"]`（可选加 `"gpu"`） | `velox_kit::export_harmony_app!(MyApp)` + `HarmonyHostBridge<MyApp>` | `OH_NativeXComponent`、`OHNativeWindow`、`AvoidArea` 与 `onBackPress` |
| WebAssembly (`<canvas>`) | `default-features = false, features = ["std", "component"]` | `velox_kit::export_web_app!(MyApp)` + `WebCanvasApp` / `generate_web_index_html` | `Rgba8888` 零拷贝 `Uint8ClampedArray`、`putImageData` 局部脏区、`e.pointerType` 桥接、`popstate` |

- **移动端与 Web 零轮询异步唤醒**：`MobileEmbedApp` 与 `WebCanvasApp` 均提供 `set_async_waker(...)`、`poll_async_tasks()` 与 `needs_redraw()`，供原生宿主按需触发单次重绘（避免无休止高频空转）。

---

## 多渲染后端调度 DesktopRenderBackend

`DesktopRenderBackend::default_desktop()` 在编译期按启用的 Feature 自动选取最优后端（运行时也可 `.with_backend(...)` 切换）：

1. **`Wgpu`**（`gpu`，**默认启用**）：基于纯 `wgpu`（Win32 D3D12 / macOS Metal / Linux & Android Vulkan / WebGPU）的自研轻量级 2D SDF 单 Pass Instanced 硬件渲染器（含动态字形图集与精灵图集，零重型矢量计算池，直出 Swapchain）；
2. **`Software`**（始终内置，无需单独 Feature）：纯粹基于整型定点 SDF 扫描线的多线程条带软光栅渲染器，带前向遮挡裁剪剔除（Front-to-Back Culling）。

---

## 为自定义硬件实现 Platform / AsyncPlatform 与 AsyncDriver

对于自有显示表面或异步执行器，只需为你的宿主驱动实现核心平台与调度 Trait：

- **同步接口**：`PlatformSurface`（`surface_size`、`flush_frame_dt` -> `FlushReport`）与 `Platform`（`duration_since_start`、`step_platform` -> `PlatformControlFlow`、`run_event_loop`）；
- **异步接口**：`AsyncPlatformSurface`（`flush_frame_dt_async`）与 `AsyncPlatform`（`step_platform_async` -> `impl Future<Output = PlatformControlFlow>`），支持帧提交与空闲等待全程 `.await` 让出调度器而不阻塞；
- **跨调度器主循环驱动**：实现 `SchedulerClock` + `AsyncDriver`（可直接复用 `StdAsyncDriver`、`TokioScheduler`、`SmolScheduler` 或 `ManualScheduler`），并调用 `run_async_event_loop(&mut platform, &mut engine, &driver, AsyncLoopConfig::default()).await`，在静止期直接进入 0% CPU 休眠。
