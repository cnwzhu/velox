# 确定性无头 UI 集成测试（Headless UI Integration Testing）

**Contents:** [核心测试模式：直接验证真实 View](#核心测试模式直接验证真实-view) · [无头复用极简启动 `build_headless` 与异步虚拟时钟测试](#无头复用极简启动-build_headless-与异步虚拟时钟测试) · [TestingPlatform 像素与移动端仿真](#testingplatform-像素与移动端仿真) · [桌面工作台双平台与空闲休眠断言](#桌面工作台双平台与空闲休眠断言) · [无障碍语义树与 PNG 快照验证](#无障碍语义树与-png-快照验证) · [推荐 Cargo 验证命令矩阵](#推荐-cargo-验证命令矩阵)

---

## 核心测试模式：直接验证真实 View

Velox 的引擎与布局求解完全确定性，无需启动真实显示服务器即可在标准 Rust `#[test]` 中完成整套组件渲染、点击命中、键盘编辑与局部重绘断言。**始终测试真实生产视图，不造假替身页面**：

```rust
use velox_kit::prelude::*;

#[test]
fn test_real_view_interaction_and_occlusion() {
    let mut engine = VeloxEngine::new(960, 640);
    let root = engine.mount_root(DesktopStudioApp::new());

    // 1. 首次求值并产生全视口脏区
    let d0 = engine.step_frame();
    assert!(!d0.is_empty());

    // 2. 无事件与无活跃动画时，后续 step_frame 必须输出 0 脏区且 !needs_redraw()
    let d1 = engine.step_frame();
    assert!(d1.is_empty());
    assert!(!engine.needs_redraw());

    // 3. 派发点击与键盘事件，检查真实应用状态
    assert!(engine.dispatch_click(910, 22)); // 打开模态 Dialog
    assert!(engine.view(root).unwrap().about_dialog_open);
    let _ = engine.step_frame();

    // 4. 验证模态遮罩 .overlay().occlude_hit() 拦截底层菜单点击
    assert!(!engine.dispatch_click(100, 162));
}
```

### 常用驱动 API

- 帧步进：`engine.step_frame()`（默认 16ms）、`engine.step_frame_dt(dt_ms)`、`engine.finish_animations()`（快进所有过渡到终点）；
- 事件注入：`engine.dispatch_click(x, y)`、`dispatch_mouse_down(x, y)` / `dispatch_touch_down(x, y)` / `dispatch_pointer_down_with_kind(x, y, kind)`、`dispatch_pointer_down/move/up`、`dispatch_key(KeyCode)`、`dispatch_swipe(x, y, dir)`、`dispatch_navigate_back()`、`engine.scroll(list_id, delta_y)`；
- 异步观测与驱动：`engine.poll_async_tasks()`、`engine.has_pending_async_tasks()`、`engine.active_async_task_count()`；
- 视口与安全区：`engine.resize(w, h)`、`engine.set_safe_area(SafeAreaInsets)`、`engine.set_keyboard_inset(h)`；
- 性能与缓存观测：`engine.last_damage()`、`engine.last_arena_used_bytes()`、`engine.pod_render_count(handle)`、`engine.subview_cache_hits()`、`engine.partial_relayout_count()`。

---

## 无头复用极简启动 `build_headless` 与异步虚拟时钟测试

1. **复用极简启动配置**：使用 `velox_kit::application().build_headless(|cx| { ... })` 直接获得 `BuiltWindow { options, engine }`，无需弹出真实 OS 窗口即可验证窗口标题、初始尺寸、主题/语言、默认调度器与交互点击。
2. **零真睡异步虚拟时钟（`ManualScheduler`）**：将 `ManualScheduler::new()` 注入 `VeloxEngine::new(w, h).with_scheduler(sched.clone())`（或 `application().with_scheduler(sched.clone()).build_headless(...)`），通过 `sched.advance(Duration)` 与 `sched.drive_engine(&mut engine)` 在 `<1ms` 内确定性测试定时器、多阶段后台计算与 RAII `Task<T>` 取消：

```rust
use velox_kit::prelude::*;

#[test]
fn test_deterministic_async_and_mouse_scroll_isolation() {
    let sched = ManualScheduler::new();
    let mut built = velox_kit::application()
        .with_scheduler(sched.clone())
        .build_headless(|cx| {
            velox_kit::init(cx);
            velox_kit::open_window(
                WindowOptions::new("Async Test").size(640, 400),
                cx,
                |cx| cx.new(|_| MyAsyncView::new()),
            )
            .expect("open window");
        })
        .expect("built window");

    let _ = built.engine.step_frame();
    // 模拟桌面鼠标空白处拖拽：mouse_drag_scroll 默认 false，不应拖走滚动区
    assert!(!built.engine.dispatch_mouse_down(10, 10));
    built.engine.dispatch_pointer_move(10, 120);
    built.engine.dispatch_pointer_up(10, 120);

    // 快进 2 秒虚拟时钟并排空前景/后台任务
    sched.advance(Duration::from_secs(2));
    sched.drive_engine(&mut built.engine);
    assert_eq!(built.engine.active_async_task_count(), 0);
}
```

---

## TestingPlatform 像素与移动端仿真

`velox_kit::platform::testing::TestingPlatform<const W: usize, const H: usize, P: PixelFormat = Rgb565>` 及 `MobilePlatform` 提供固定分辨率离屏帧缓冲与虚拟时钟：

```rust
use velox_kit::core::display::Rgba8888;
use velox_kit::core::platform::mobile::{DeviceOrientation, TouchPhase};
use velox_kit::core::platform::testing::TestingPlatform;
use velox_kit::core::platform::{Platform, PlatformControlFlow};

let mut engine = VeloxEngine::new(320, 240);
engine.mount_root(MyView::new());

let mut plat: TestingPlatform<320, 240, Rgba8888> = TestingPlatform::new();
assert_eq!(plat.step_platform(&mut engine, 16), PlatformControlFlow::Poll);
assert_eq!(plat.step_platform(&mut engine, 16), PlatformControlFlow::Wait);

// 验证确切像素写入了帧缓冲且触摸路由生效
assert!(plat.pixels().iter().any(|p| p.0 != 0));
plat.on_touch(1, TouchPhase::Began, 40, 120);
plat.on_touch(1, TouchPhase::Ended, 40, 120);
plat.set_orientation(DeviceOrientation::LandscapeLeft);
let _ = plat.step_platform(&mut engine, 16);
```

---

## 桌面工作台双平台与空闲休眠断言

对于桌面应用，建议同步测试 `WinitPlatform` 与 `WaylandPlatform` 的事件流：

- `plat.step_platform(&mut engine, 16)` 第一帧应返回 `PlatformControlFlow::Poll`，次帧无待处理事件/动画应严格返回 `PlatformControlFlow::Wait`；
- 在无头命令行或 CI（无显示器/Wayland 合成器）下依然可以直接验证 `WaylandPlatform::new_xdg_toplevel(...)` 缓冲区像素生成与 `committed_damage()` 局部脏矩形列表。

---

## 无障碍语义树与 PNG 快照验证

- **无障碍语义审查**：调用 `let nodes = engine.build_a11y_tree::<128>();` 检查交互按钮（`A11yRole::Button`）、输入框（`TextInput`）、滑块（`Slider`）、`focusable` 与 `focused` 属性及可读标签 `label` 是否完整导出；
- **视觉像素走查**：利用 `WinitPlatform::new(title, w, h).with_dpi_scale(2).render_to_png(&mut engine, "target/preview.png")` 导出 HiDPI 无损 PNG 并人工或自动核验对比度、文本裁剪、边框与对齐。

---

## 推荐 Cargo 验证命令矩阵

| 验证目的 | 命令 |
| --- | --- |
| 极简启动示例端到端测试 | `cargo test -p hello_world` |
| 桌面画廊完整测试（含 `Tokio + reqwest`、`AsyncPlatform`、`PointerKind`） | `cargo test -p desktop_app` |
| 可插拔异步适配层（`tokio` + `smol`）测试 | `cargo test -p velox-async --all-features` |
| `velox-kit` 默认及全调度器 Feature 集成测试 | `cargo test -p velox-kit --features tokio,smol` |
| `velox-kit` 最小软光栅裁剪编译检查 | `cargo check -p velox-kit --no-default-features --features component` |
| `velox-engine` 最小无 GPU 裁剪编译检查 | `cargo check -p velox-engine --no-default-features` |
| 工作区全量测试 | `cargo test --workspace` |
