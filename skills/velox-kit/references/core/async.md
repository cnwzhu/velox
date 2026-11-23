# 异步运行时与可插拔调度器（Async Runtime & Pluggable Schedulers）

**Contents:** [分层隔离架构](#分层隔离架构) · [双执行器与 RAII `Task<T>`](#双执行器与-raii-taskt) · [在 View 事件中发起异步任务](#在-view-事件中发起异步任务) · [延迟回调：`defer` 与 `defer_self`](#延迟回调defer-与-defer_self) · [可插拔调度器与 `velox-async`（Tokio / smol）](#可插拔调度器与-velox-asynctokio--smol) · [确定性虚拟时钟测试（ManualScheduler）](#确定性虚拟时钟测试manualscheduler) · [零轮询平台唤醒与异步事件循环](#零轮询平台唤醒与异步事件循环) · [异步编码铁律与常见坑位](#异步编码铁律与常见坑位)

---

## 分层隔离架构

Velox 的异步子系统位于 `velox_core::async_rt`（由 `velox_kit::async_rt`、`velox_kit::scheduler` 与 `velox_kit::prelude::*` 统一导出），采用严格三层隔离设计，使业务 `View` 代码与底层调度器完全解耦：

1. **通用异步抽象与事件循环层**：
   - `SchedulerCapabilities`：描述调度器能力（`name`、`supports_background_threads`、`supports_dynamic_spawn`、`deterministic_clock`）；
   - `SchedulerClock` / `AsyncDriver`：抽象单调时钟 `now_ms`、非阻塞 `sleep`、`yield_now`（`YieldNow`）与空闲挂起 `wait_for_event`；
   - `run_async_event_loop(platform, engine, driver, AsyncLoopConfig)` -> `AsyncLoopReport`：跨平台通用的异步主循环驱动函数。
2. **可插拔多调度器与 GPUI 风格双执行器层**：
   - `SchedulerBackend`：仅需实现 4 个核心方法（`name`、`spawn_background`、`timer`、`now_ms`）即可接入任意运行时；
   - `NativeStdScheduler`：默认内置零外部依赖多线程池 + 单常驻最小堆定时器；
   - `ManualScheduler`：确定性虚拟时钟 + 手动步进调度器（零真睡、0ms 瞬时快进单元测试）；
   - `ForegroundExecutor`、`AsyncApp`、`BackgroundExecutor`、`Task<T>`、`Timer`、`UiWakeSignal`、`StdAsyncDriver`、`block_on`。
3. **独立按需调度器适配 Crate（`velox-async`）**：
   - 通过 `velox-kit` 的可选 Cargo Feature **`tokio`** 或 **`smol`** 按需开启（默认全部关闭，保持 0 外部异步运行时依赖）；
   - 分别提供 `TokioScheduler`（`VeloxTokioExt` / `VeloxEngineTokioExt`）与 `SmolScheduler`（`VeloxSmolExt` / `VeloxEngineSmolExt`），均同时实现 `SchedulerBackend` 与 `AsyncDriver`。

---

## 双执行器与 RAII `Task<T>`

Velox 借鉴 GPUI 的前景/后台双执行器模型，彻底隔离 UI 状态安全与后台并发：

- **前景执行器（`ForegroundExecutor` / `AsyncApp`）**：
  - 始终运行于 **UI 主线程**（`!Send`），可在 `.await` 恢复后通过 `Handle<V>::update(&mut cx, ...)` 与 `Handle<V>::read(&cx, ...)` 安全访问 `PodPool` 与线程局部 `Signal<T>`；
  - 仅在 `VeloxEngine::poll_async_tasks()` 期间通过 RAII 守卫临时挂载 `PodPool`，协程跨 `.await` 挂起时自动摘除，从根源杜绝悬垂指针与双 `&mut` 别名。
- **后台执行器（`BackgroundExecutor`）**：
  - 委托给当前 `SchedulerBackend`（默认 `NativeStdScheduler`，或可切换为 `TokioScheduler` / `SmolScheduler` / `ManualScheduler`）；
  - 用于卸载 CPU 密集型计算、阻塞磁盘或网络 I/O（要求 `Future: Send + 'static`）。
- **RAII 生命周期托管句柄 `Task<T>`**：
  - 标记了 `#[must_use]`：**直接丢弃 `Task<T>` 会立即触发取消**；
  - **绑定宿主生命周期**：将 `Task<()>` 存入 `View` 结构体字段（如 `sync_task: Option<Task<()>>`）；覆盖写入新任务、设为 `None` 或视图从 `PodPool` 卸载时，旧任务随 `Drop` 自动取消并回收；
  - **等待结果**：`Task<T>` 实现 `Future<Output = T>`，可在前景协程中直接 `.await` 后台任务返回值；
  - **独立运行**：确需“发后即忘”时显式调用 `.detach()`；
  - **条件分支同步返回**：使用 `Task::ready(value)`。

---

## 在 View 事件中发起异步任务

推荐在事件回调（`cx.listener` / `cx.action`）中通过 `acx.spawn(...)` 启动前景协程，再通过 `cx.background_spawn(...)` 卸载后台计算或网络请求，并将返回的 `Task<()>` 存放在视图字段中实现 RAII 自动取消：

```rust
use velox_kit::prelude::*;

pub struct SyncPanel {
    pub status: SmallString<64>,
    pub busy: bool,
    pub task: Option<Task<()>>,
}

impl SyncPanel {
    fn start_sync(&mut self, acx: &mut ActionScope) {
        self.busy = true;
        self.status = SmallString::from_str("Running...");

        // 覆盖 self.task 会先 Drop 旧 Task，自动取消上一轮未完成请求
        self.task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| {
            // 1. 经由当前调度器时钟等待或卸载到后台线程池
            let bg = cx.background_executor();
            let result = bg
                .spawn({
                    let bg = bg.clone();
                    async move {
                        bg.timer(Duration::from_millis(80)).await;
                        42u32
                    }
                })
                .await;

            // 2. 回到 UI 主线程安全写回视图状态（带 ABA generation 校验并自动标脏）
            let _ = this.update(&mut cx, |view, _acx| {
                view.busy = false;
                view.status = SmallString::from_str(if result == 42 { "Done (42)" } else { "Failed" });
                view.task = None;
            });
        }));
    }

    fn cancel_sync(&mut self) {
        self.task = None; // Drop 即刻取消进行中的前景与后台任务
        self.busy = false;
        self.status = SmallString::from_str("Cancelled");
    }
}
```

> 说明：工作区使用 Rust 2024 Edition，支持 `async move |this: Handle<Self>, mut cx: AsyncApp| { ... }` 异步闭包写法，也完全兼容经典的 `|this: Handle<Self>, mut cx: AsyncApp| async move { ... }` 写法。

### 异步入口 API 速查

| 所在上下文 | 关键方法 | 说明 |
| --- | --- | --- |
| `ActionScope` | `acx.handle::<Self>()` | 获取当前活跃视图的带代号 `Handle<Self>` |
| `ActionScope` | `acx.spawn(async move \|this: Handle<Self>, mut cx: AsyncApp\| ...)` | 为当前视图派生 UI 主线程前景异步任务，返回 `Task<T>` |
| `ActionScope` | `acx.spawn_app(async move \|mut cx: AsyncApp\| ...)` | 派生不绑定特定视图的前景异步任务 |
| `ActionScope` | `acx.background_executor()` / `acx.background_spawn(fut)` / `acx.timer(dur)` | 获取当前引擎调度器对应的后台执行器与定时器 |
| `AsyncApp` | `cx.background_spawn(fut)` / `cx.background_executor()` / `cx.timer(dur)` | 跨 `.await` 派生后台任务或等待调度器定时器 |
| `AsyncApp` | `cx.spawn(...)` / `cx.spawn_for(handle, ...)` / `cx.defer(...)` | 派生前景子任务或排入延迟回调 |
| `AsyncApp` | `cx.update(handle, \|v, acx\| ...)` / `cx.read(handle, \|v\| ...)` / `cx.notify_handle(handle)` | 读写任意视图状态（自动防 ABA 与重入，返回 `Option<R>`） |
| `Handle<V>` | `this.update(&mut cx, \|v, acx\| ...)` / `this.read(&cx, \|v\| ...)` | `AsyncApp::update` / `read` 的便捷方法（对齐 GPUI `WeakEntity::update`） |
| `WindowContext` | `wcx.spawn_for(handle, ...)` / `wcx.scheduler_name()` | 在 `open_window` 初始化阶段派生启动期前景任务 |
| `VeloxEngine` | `with_scheduler` / `set_scheduler` / `scheduler_name` / `scheduler_capabilities` | 注入或热切换 `Arc<dyn SchedulerBackend>` |
| `VeloxEngine` | `spawn` / `spawn_for` / `background_spawn` / `background_executor` / `timer` / `defer` | 引擎级异步任务派生入口 |
| `VeloxEngine` | `set_async_waker` / `poll_async_tasks` / `has_pending_async_tasks` / `active_async_task_count` | 平台唤醒注入、轮询排空与测试观测 |

---

## 延迟回调：`defer` 与 `defer_self`

当事件回调内部需要在本轮事件冒泡结束后再执行状态变更（例如跨视图重入更新、延迟关闭弹窗避免破坏当前借用栈），使用 `defer` / `defer_self`：

```rust
// 延迟到当前事件冒泡末尾在当前视图执行（自动校验 ABA 并标脏）
acx.defer_self::<Self, _>(|this, _acx| {
    this.dialog_open = false;
});

// 延迟执行任意跨视图操作
acx.defer(move |acx| {
    let _ = acx.update(other_handle, |other, _| {
        other.refresh_count += 1;
    });
});
```

---

## 可插拔调度器与 `velox-async`（Tokio / smol）

默认情况下，`velox-kit` 使用内置零外部依赖的 `NativeStdScheduler`。当业务需要直接复用 **Tokio** 生态（如 `reqwest`、`hyper`、`sqlx`、`tokio::net`、`tokio::fs`）或轻量级 **smol** 生态时，在 `Cargo.toml` 中按需开启 `velox-kit` 对应 feature：

```toml
[dependencies]
# 方式 A：接入 Tokio 多线程调度器（支持 reqwest / sqlx / tokio::net）
velox-kit = { path = "../../crates/velox-kit", features = ["tokio"] }

# 方式 B：接入轻量级 smol（async-executor + async-io）调度器
# velox-kit = { path = "../../crates/velox-kit", features = ["smol"] }
```

### 一行切换调度器（业务 `View` 零改动）

启用对应 feature 后，`velox_kit::prelude::*` 自动导出 `TokioScheduler` / `VeloxTokioExt` / `VeloxEngineTokioExt`（或 `SmolScheduler` / `VeloxSmolExt` / `VeloxEngineSmolExt`），并可通过 `velox_kit::scheduler::{tokio, smol}` 访问适配子模块：

```rust
use velox_kit::prelude::*;

// 1. 桌面极简启动链式注入 Tokio（或 .with_smol() / .with_tokio_handle(existing_handle)）
fn main() {
    velox_kit::application()
        .with_tokio()
        .run(|cx| {
            velox_kit::init(cx);
            velox_kit::open_window(
                WindowOptions::new("Tokio App").size(960, 640),
                cx,
                |cx| cx.new(|_| MyApp::new()),
            )
            .expect("Failed to open window");
        });
}

// 2. 引擎级注入或运行时热切换（测试 / 无头 / 自定义宿主）
let mut engine = VeloxEngine::new(960, 640).with_tokio();
// 或 engine.use_tokio(); / engine.use_tokio_handle(handle); / engine.use_smol();
```

### 在 Tokio 调度器下直接使用 `reqwest`

参考真实示例 [`examples/desktop_app/src/async_demo.rs`](../../../../examples/desktop_app/src/async_demo.rs)：由于 `TokioScheduler::spawn_background` 将后台 Future 派发到 Tokio 多线程运行时，`cx.background_spawn(...)` 内可直接 `.await` 任意 Tokio 生态异步客户端；同时 `TokioScheduler::timer` 内部会在每次 `poll` 自动 `handle.enter()`，确保在 `winit` UI 主线程调用 `cx.timer(...)` 永不触发 `no reactor running` panic：

```rust
self.http_task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| {
    let url = url_owned;
    let res = cx
        .background_spawn(async move {
            let resp = shared_http_client().get(&url).send().await?;
            let status = resp.status().as_u16();
            let body = resp.text().await?;
            Ok::<(u16, String), reqwest::Error>((status, body))
        })
        .await;

    let _ = this.update(&mut cx, |view, _| {
        match res {
            Ok((status, body)) => view.on_http_ok(status, body),
            Err(err) => view.on_http_err(err.to_string()),
        }
        view.http_task = None;
    });
}));
```

### 接入自定义第三方调度器

任意自定义异步运行时只需实现 `SchedulerBackend` 的 4 个核心方法即可接入 `Application::with_scheduler(arc)` 或 `VeloxEngine::with_scheduler(arc)`：

```rust
pub trait SchedulerBackend: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn spawn_background(&self, fut: BoxFuture<'static, ()>);
    fn timer(&self, duration: Duration) -> Timer;
    fn now_ms(&self) -> u64;
}
```

---

## 确定性虚拟时钟测试（ManualScheduler）

`ManualScheduler` 同时实现 `SchedulerBackend` 与 `AsyncDriver`，内置 0 线程、0 真睡的虚拟时钟与最小堆定时器，可在 `<1ms` 内确定性地验证包含数十秒定时与多阶段后台任务的异步界面逻辑：

```rust
use velox_kit::prelude::*;

#[test]
fn test_async_flow_with_manual_clock() {
    let sched = ManualScheduler::new();
    let mut engine = VeloxEngine::new(640, 400).with_scheduler(sched.clone());
    let root = engine.mount_root(SyncPanel::new());

    let _ = engine.step_frame();
    // 触发点击启动异步任务
    assert!(engine.dispatch_click(100, 40));

    // 推进已就绪的前景/后台任务直至挂起在定时器上
    sched.drive_engine(&mut engine);
    assert!(engine.view(root).unwrap().busy);

    // 零真睡快进 80ms 虚拟时钟并再次驱动至完成
    sched.advance(Duration::from_millis(80));
    sched.drive_engine(&mut engine);

    assert!(!engine.view(root).unwrap().busy);
    assert_eq!(engine.active_async_task_count(), 0);
}
```

---

## 零轮询平台唤醒与异步事件循环

Velox 绝不通过高频 `16ms` 空轮询检查异步完成状态，所有平台在等待异步任务期间均维持 0% 空闲 CPU：

- **Desktop (`WinitPlatform` / `application().run(...)`)**：自动注册 `EventLoopProxy<WinitUserEvent>`，后台任务完成或定时器到期瞬间发送 `WinitUserEvent::WakeAsync` 精准唤醒处于 `ControlFlow::Wait` 的 OS 事件循环并执行 `engine.poll_async_tasks()`；
- **Mobile (`MobileEmbedApp`) & Web (`WebCanvasApp`)**：均暴露 `set_async_waker(...)`、`poll_async_tasks()` 与 `needs_redraw()`，宿主可在异步就绪时调度一帧原生重绘（Android `Choreographer` / iOS `CADisplayLink` / Web `requestAnimationFrame`）；
- **自定义异步主循环**：用 `StdAsyncDriver::new()`（或 `StdAsyncDriver::with_backend(...)` / `TokioScheduler` / `SmolScheduler`），将 `driver.wake_signal().notifier()` 注入 `engine.set_async_waker(...)`，再运行 `run_async_event_loop(&mut platform, &mut engine, &driver, AsyncLoopConfig::default()).await`。

---

## 异步编码铁律与常见坑位

1. **绝不直接丢弃 `Task<T>`**：`Task<T>` 带 `#[must_use]`，不保存、不 `.await` 且不 `.detach()` 会导致任务创建即被取消。页面绑定的请求应存入 `View` 字段（`Option<Task<()>>`），以获得覆盖自动取消和卸载自动取消的 RAII 保障。
2. **`Signal<T>` 严禁跨线程读写**：`Signal<T>` 在 `std` 下采用 `thread_local!` 存储，仅属于 UI 主线程。绝不要在 `cx.background_spawn(...)` 闭包内读/写 `Signal<T>`；务必将计算结果返回给前景 `acx.spawn` 协程后，在 `this.update(&mut cx, |view, _| { view.sig.set(...); })` 中更新。
3. **跨调度器可移植的定时器写法**：如果希望异步任务同时兼容 `NativeStdScheduler`、`TokioScheduler`、`SmolScheduler` 与 `ManualScheduler`，始终使用 `cx.timer(dur)` 或 `let bg = cx.background_executor();` 后的 `bg.timer(dur)`，而不要在后台闭包内硬编码 `Timer::after(dur)`（`Timer::after` 直接绑定 `NativeStdScheduler` 的真实系统时钟，无法被 `ManualScheduler::advance` 虚拟快进）。
4. **不在 `View::render` 内 `spawn` 异步任务**：`render` 是纯声明式帧构建阶段；所有异步任务应在事件回调（`acx.spawn`）或启动初始化阶段（`WindowContext::spawn_for` / `VeloxEngine::spawn_for`）派生。
5. **保持空闲 0% CPU**：仅在异步状态为 `busy` 时驱动加载 `Spinner` 动画（`cx.keep_animating(self.busy)`），任务结束或取消后立刻关闭 `busy` 标志，使引擎自动回落到 `PlatformControlFlow::Wait`。
