# 视图边界、状态池与上下文（View / Widget / PodPool / Handle / Scopes）

**Contents:** [View vs Widget 抉择](#view-vs-widget-抉择) · [PodPool 与防 ABA 句柄 `Handle<T>`](#podpool-与防-aba-句柄-handlet) · [三大上下文：`FrameScope`、`Scope<V>`、`ActionScope`（及异步 `AsyncApp`）](#三大上下文framescopescopevactionscope及异步-asyncapp) · [跨视图状态读写与局部重排边界](#跨视图状态读写与局部重排边界)

---

## View vs Widget 抉择

Velox 将 UI 抽象严格划分为两类核心 Trait（定义于 `velox_core::element`）：

```rust
pub trait View: Sized + 'static {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a>;
}

pub trait Widget<'a>: Sized {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a>;
}
```

- **`Widget<'a>`（无状态轻量展开）**：不占用 `PodPool` 槽位，只在渲染期按值展开为 `Element<'a>`。所有内置 70+ 基础组件（`Button`、`Switch`、`Slider`、`Table`、`Form`、`Card` 等）以及应用内轻量封装结构均实现 `Widget<'a>`。
- **`View`（有状态局部更新边界）**：通过 `engine.mount_root(view)` 或 `engine.insert_view(view)` 存入 `PodPool`（按需动态扩展支持任意数量子视图）。每个 `View` 拥有独立脏标记、`render_count`、`Element` 缓存与局部重排边界。

---

## PodPool 与防 ABA 句柄 `Handle<T>`

`Handle<T>` 为 4 字节紧凑 `Copy` 句柄（`PodId(u16)` + `generation: u16`）：

```rust
let mut engine = VeloxEngine::new(960, 640);
let child_handle: Handle<MetricsPanel> = engine.insert_view(MetricsPanel::new());
let root_handle: Handle<MainApp> = engine.mount_root(MainApp { child: child_handle });

// 外部安全访问：
let _ro: Option<&MetricsPanel> = engine.view(child_handle);
engine.update_view(child_handle, |panel| {
    panel.fps = 120; // 返回后自动标脏该 Pod
});
// engine.view_mut(handle) 会立即标脏并返还 &mut V
// engine.remove_view(child_handle) 会调用 Drop 并递增 generation，旧 Handle 自动失效
```

父视图 `View::render` 内直接通过 `.child(cx, self.child)` 挂载子视图句柄（因为 `impl<'a, V: View> IntoElement<'a> for Handle<V>` 会生成 `Element::SubView`）。使用桌面极简启动 API 时，也可在 `open_window` 回调中通过 `wcx.new(|_| RootView)` 与 `wcx.insert_view(SubView)` 完成同等注册。

在 `std` 异步前景协程中，`Handle<T>` 直接提供与 GPUI `WeakEntity` 对齐的安全读写方法（自动校验 ABA `generation`、防重入、标脏并刷新信号；视图已卸载时安全返回 `None`）：

```rust
let _ = this.update(&mut cx, |view, acx| {
    view.loaded = true;
});
let snapshot = this.read(&cx, |view| view.loaded);
```

---

## 三大上下文：`FrameScope`、`Scope<V>`、`ActionScope`（及异步 `AsyncApp`）

1. **`FrameScope`**：传入 `Widget::render_once(self, cx: &'a FrameScope)` 与 `.child(cx, ...)`：
   - `cx.arena()`：获取瞬态 `ScratchArena`；
   - `cx.theme()` / `cx.override_theme(theme)`：读取或帧内临时覆盖 `CoreTheme`（分配于 `ScratchArena`）；
   - `cx.safe_area()` / `cx.keyboard_inset()`：读取安全区与软键盘遮挡高度；
   - `cx.now_ms()` / `cx.request_frame()` / `cx.keep_animating(running)`：访问引擎单调时钟并调度连续动画帧。
2. **`Scope<V>`**：传入 `View::render<'a>(&mut self, cx: &'a mut Scope<Self>)`：
   - 内存布局 `#[repr(transparent)]` 且实现 `Deref<Target = FrameScope>`；
   - `cx.listener(|this, event: E, acx| { ... })`：生成 `&'a dyn Fn(E, &mut ActionScope)`；
   - `cx.action(|this, acx| { ... })`：生成 `&'a dyn Fn(&mut ActionScope)`。
3. **`ActionScope<'p>`**：事件执行回调期间传入 `&mut ActionScope`：
   - 标脏与事件控制：`acx.notify()`、`acx.skip_notify()`（`listener` / `action` 默认自动标脏）、`acx.stop_propagation()`；
   - 输入快照：`acx.click_position()`、`acx.drag_event()`、`acx.now_ms()`；
   - 跨视图访问：`acx.read(other_handle)`、`acx.update(other_handle, |v, acx| ...)`、`acx.notify_handle(handle)`；
   - 异步与延迟调度（`std`）：`acx.handle::<Self>()`、`acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| ...)`、`acx.spawn_app(...)`、`acx.background_executor()`、`acx.background_spawn(...)`、`acx.timer(dur)`、`acx.defer(...)`、`acx.defer_self::<Self, _>(...)`。
4. **`AsyncApp`**（`std`）：跨 `.await` 存活于 UI 主线程的前景异步上下文（`!Send`），支持 `cx.background_spawn`、`cx.background_executor`、`cx.timer`、`cx.spawn`、`cx.spawn_for`、`cx.defer`、`cx.update`、`cx.read` 与 `cx.now_ms()`（完整用法见 [async.md](async.md)）。

---

## 跨视图状态读写与局部重排边界

- **渲染期跨视图只读**：在视图 A 的 `render` 中调用 `cx.read(self.b_handle)` 返回 `Option<PodReadGuard<'s, B>>`（或 `cx.read_with(h, |b| b.count)`）。借用守卫 `PodReadGuard` 保证生命周期不可逃逸进 `Element<'a>`，同时自动建立依赖记录；当视图 B 变更时同步通知读取者。
- **事件期跨视图可变更新**：在事件回调中使用 `acx.update(self.b_handle, |b, b_acx| { ... })`：
  - 返回 `Option<R>`，自动检测当前借用状态：若发生 `A -> B -> A` 循环重入，内层对已借用视图的 `update` 安全返回 `None`，在安全 Rust 层杜绝双 `&mut` 别名 UB；
  - 也可通过 `acx.notify_handle(handle)` 显式标脏某个兄弟句柄。
- **局部重排边界（Relayout Boundary）**：当父视图未脏而仅叶子 `SubView` 标脏且其容器在父级具有明确非 `Auto` 尺寸约束时，引擎可在 `ScratchArena` 中就地局部重排该 `SubView`（通过 `engine.partial_relayout_count()` / `engine.subview_cache_hits()` 可观测）。
