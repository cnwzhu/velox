# Velox 编码规范（Normative Coding Guides）

**Contents:** [架构速览与分层边界](#架构速览与分层边界) · [启动引导与根视图归属](#启动引导与根视图归属) · [理解 Velox 的上下文阶段](#理解-velox-的上下文阶段) · [选择正确的抽象单元](#选择正确的抽象单元) · [状态归属与响应式信号](#状态归属与响应式信号) · [异步运行时与可插拔调度器](#异步运行时与可插拔调度器) · [生命周期、ScratchArena 与 ID](#生命周期scratcharena-与-id) · [渲染与链式组合](#渲染与链式组合) · [行为与视觉表现边界](#行为与视觉表现边界) · [主题与 24 字节紧凑样式](#主题与-24-字节紧凑样式) · [事件、手势、焦点与路由栈](#事件手势焦点与路由栈) · [动画与连续帧控制](#动画与连续帧控制) · [布局求解与滚动](#布局求解与滚动) · [列表、表格与代码编辑器](#列表表格与代码编辑器) · [公共 API 与 Feature 设计](#公共-api-与-feature-设计) · [跨平台后端与渲染器边界](#跨平台后端与渲染器边界) · [文件与命名约定](#文件与命名约定) · [测试策略与无头集成测试](#测试策略与无头集成测试) · [性能快路径与红线](#性能快路径与红线) · [常见失败模式与 Agent 铁律](#常见失败模式与-agent-铁律) · [交付检查清单](#交付检查清单)

这是一份强制性工程规范。任何架构选型、生命周期设计、组件修改、公共 API 新增或测试编写，都必须以本规范和真实源码签名作为唯一依据。

---

## 架构速览与分层边界

Velox 采用严格单向依赖的分层 Workspace 架构（以 `MPL-2.0` 协议开源）：

1. **`crates/velox-core`**：核心 UI 引擎内核，严禁依赖上层组件库或任何外部异步运行时。
   - 帧内存管理（`ScratchArena`、`SmallVec<T, N>`、`StrRef<'a>`、`fmt!`）
   - 视图与状态池（`View`、`Widget<'a>`、`PodPool`、`Handle<V>`、`FrameScope`、`Scope<V>`、`ActionScope`）
   - 响应式信号图（`Signal<T>`、`Computed<T>`、`Effect`、`batch`）
   - 极简桌面启动层与异步运行时（`app`：`application` / `open_window`；`async_rt`：`AsyncDriver` / `run_async_event_loop` / `SchedulerBackend` / `ForegroundExecutor` / `BackgroundExecutor` / `Task<T>` / `NativeStdScheduler` / `ManualScheduler`）
   - 紧凑样式与四步布局引擎（`Style`、`Styled`、`Two-Pass Axis Solver`、`Region`、`DamageTracker`、`MotionCache`）
   - 跨平台与双渲染后端（`platform::{winit, wayland, mobile, android, ios, harmony, web, testing}`、`renderers::{software, wgpu}`）
2. **`crates/velox-base`**：构建在 `velox-core` 之上的无样式行为原语层：70+ 基础组件交互逻辑、文本/代码编辑状态机（`EditableText`、`TextBuffer`、`CodeEditor`）、页面路由栈（`Navigator`）、等高虚拟列表（`UniformList` / `VirtualList`）与统一修饰 Trait（`Sizable`、`Variants`、`Disableable`）。
3. **`crates/velox-component`**：构建在 `velox-base` 之上的 Shadcn Zinc + Blue Tailwind 风格主题化包装层（`define_themed_component!`、`TailwindStyled`、`Theme` / `ThemeMode`）。
4. **`crates/velox-async`**：独立可插拔第三方异步调度器适配 Crate，通过 Cargo feature 按需开启（`tokio` -> `TokioScheduler`；`smol` -> `SmolScheduler`），严禁被 `velox-core` / `velox-base` 反向依赖。
5. **`crates/velox-kit`**：**应用唯一依赖 Crate**，统一重导出上述层级并提供稳定无歧义的 `Base*` / `Themed*` 别名、可选 `tokio`/`smol` feature 透传与 `prelude`。

---

## 启动引导与根视图归属

应用初始化遵循以下简单明确的所有权模型：

- **桌面端首选极简启动 API**：使用 `velox_kit::application()` 链式配置全局主题、语言、渲染后端或调度器（`.with_tokio()` / `.with_smol()` / `.with_scheduler(...)`），在 `.run(|cx| { velox_kit::init(cx); velox_kit::open_window(WindowOptions::new(...).size(w, h), cx, |cx| cx.new(|_| MyView)).expect(...); })` 中打开主窗口；无头测试或截图直接调用 `.build_headless(...)` 获取 `BuiltWindow`；
- **引擎底层与多视图注册**：`VeloxEngine::new(width, height)` 持有 `PodPool`、`FrameScope`、`ForegroundExecutor`、`MotionCache` 与 `DamageTracker`；`WindowContext::new` / `engine.mount_root(root_view)` 注册唯一根视图并返回 `Handle<RootView>`；需要独立局部刷新边界的子面板通过 `wcx.insert_view(child_view)` / `engine.insert_view(child_view)` 注册并把 `Handle<ChildView>` 存放在根视图中；
- 平台适配器（`WinitPlatform`、`WaylandPlatform`、`AndroidHostBridge`、`IosHostBridge`、`HarmonyHostBridge`、`WebCanvasApp`、`TestingPlatform`）只负责驱动 `VeloxEngine::step_frame_dt`、输入事件与异步唤醒（`WinitUserEvent::WakeAsync` / `set_async_waker`）及脏矩形呈现，不应篡改应用领域状态。

---

## 理解 Velox 的上下文阶段

Velox 通过清晰的上下文区分生命周期阶段，绝不能混淆：

1. **渲染阶段 —— `FrameScope` 与 `Scope<V>`**：
   - `FrameScope` 管理本帧瞬态 `ScratchArena`、当前活跃主题 `cx.theme()`、移动端安全区 `cx.safe_area()` / `cx.keyboard_inset()`、当前单调时钟 `cx.now_ms()` 与下一帧动画请求 `cx.request_frame()`。
   - `Scope<V>` 通过 `#[repr(transparent)]` 与 `Deref<Target = FrameScope>` 继承上述全部能力，额外提供仅属于当前视图 `V` 的闭包绑定器 `cx.listener(...)` 与 `cx.action(...)`。
   - **渲染阶段禁止突变视图业务状态或 `spawn` 异步任务**：`render(&mut self, cx)` 可以读取 `self` 与 `Signal::get()`，也可以通过 `cx.read(sibling_handle)` 安全读取兄弟视图状态（借用被约束在 `PodReadGuard` 局部作用域内，无法逃逸入 `Element<'a>`），但不得在 `render` 内直接修改外部视图字段或派生异步任务制造无限循环。
2. **布局与拍平阶段 —— `Two-Pass Axis Solver`**：
   - 将闭合枚举 `Element<'a>` 树（仅 4 种节点：`Div`、`Text`、`Img`、`SubView`）在 `ScratchArena` 顶端临时区求解为屏幕坐标 `Region`，然后拍平为一维 `DrawOp<'a>` 与 `HitRegion<'a>`。
3. **事件执行阶段 —— `ActionScope<'p>`**：
   - 点击、拖拽、按键、滑块拖动等回调执行时获得 `&mut self`（通过 `cx.listener` / `cx.action` 自动校验 `PodId` 与 `generation`）以及 `&mut ActionScope`。
   - 同步控制：`acx.skip_notify()`、`acx.stop_propagation()`、`acx.read(other_handle)`、`acx.update(other_handle, |v, acx| ...)`、`acx.now_ms()`；
   - 异步与延迟调度：`acx.handle::<Self>()`、`acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| ...)`、`acx.spawn_app(...)`、`acx.background_executor()`、`acx.background_spawn(...)`、`acx.timer(dur)`、`acx.defer(...)`、`acx.defer_self::<Self, _>(...)`。
4. **前景异步恢复阶段 —— `AsyncApp`**：
   - `!Send`（绑定 UI 主线程），仅在 `VeloxEngine::poll_async_tasks()` 期间通过 RAII 守卫挂载 `PodPool`，在 `.await` 恢复后通过 `this.update(&mut cx, |view, acx| ...)` / `this.read(&cx, |view| ...)` 安全更新视图并自动校验 ABA 与标脏。

---

## 选择正确的抽象单元

| 需要封装的东西 | 应选类型 | 特点 |
| --- | --- | --- |
| 纯展示卡片、按钮、表单行、受控开关等复用 UI | `Widget<'a>` | 零 `PodPool` 槽位开销，直接在 `ScratchArena` 内联展开为 `Element<'a>` |
| 独立页面、需要隔离重绘的大模块（如复杂面板/列表头） | `View`（通过 `Handle<T>` 挂载） | 常驻 `PodPool`，拥有独立局部布局与局部重绘边界（`SubView` 命中缓存） |
| 高频逐像素矢量图表、仪表弧线、旋转贴图 | `img_shader(cx, w, h, state_hash, ...)` | 输出 `SpriteSource::Shader`，受裁剪窗口保护且基于 `state_hash` 自动参与脏矩形比对 |

**零虚表原则**：不要试图引入 `Box<dyn Component>` 集合或堆上组件树；组合变化内容使用 `.child(cx, el)`、条件分支或迭代器 `.children(cx, iter)`。

---

## 状态归属与响应式信号

Velox 提供三种层次清晰的状态管理方式，避免多份拷贝导致状态撕裂：

1. **宿主 `View` 字段**：如 `active_tab: DesktopTab`、`about_dialog_open: bool`、`editor_buf: TextBuffer<1024>`、`nav: Navigator<Route, 4>`。适合结构体集合与多字段事务性修改。
2. **子视图句柄 `Handle<ChildView>`**：当子区域频繁独立更新时，拆出独立 `View` 并由父视图持有 `Handle<ChildView>`；跨视图通信使用 `cx.read(h)`（渲染期）和 `acx.update(h, ...)`（事件期，自带防循环借用 `A -> B -> A` 检查）。
3. **细粒度信号 `Signal<T>` / `Computed<T>` / `Effect`**：
   - 当信号值为 `Copy + 'static` 且尺寸 `<= 16` 字节、对齐 `<= 8` 字节（如 `i32`、`u8`、`bool`、小枚举）时，极适合用 `signal(val)`。
   - 在任意 `View::render` 内调用 `sig.get()`，系统会自动将**实际读取该信号的叶子 `PodId`** 注册为订阅者；之后调用 `sig.set(...)`、`sig.set_if_changed(...)`、`sig.update(...)` 或 `batch(|| { ... })`（批量事务写入，末尾统一通知一次），引擎只标脏依赖了该信号的最小 `PodId` 集合，父视图与无关兄弟视图保持 0 次额外 `render()`。
   - 常用受控组件（`Switch`、`Checkbox`、`Slider`、`Toggle`、`Tabs`、`TabBar`、`RadioGroup`、`NumberInput`、`Rating`、`Pagination`、`Roller`、`Collapsible`、`Accordion`）支持 `Component::bind(cx, id, sig)` 零堆双向信号绑定。

**禁止状态反馈循环**：绝不要在 `render` 内调用 `sig.set(...)` 或在无条件分支里调用 `cx.request_frame()`（除非动画仍在运行 `tween.is_running(now)`）。

---

## 异步运行时与可插拔调度器

详见 [core/async.md](core/async.md)。编写异步界面代码必须严格遵守以下五项工程约束：

1. **托管 `Task<T>` 生命周期（RAII 自动取消）**：`acx.spawn(...)` 与 `cx.background_spawn(...)` 返回的 `Task<T>` 标记了 `#[must_use]`，直接丢弃会立刻取消任务。与页面状态绑定的异步任务应存入 `View` 字段（如 `sync_task: Option<Task<()>>`），通过覆盖赋值或置 `None` 自动取消旧请求，仅在明确发后即忘时调用 `.detach()`。
2. **`Signal<T>` 严禁跨线程访问**：`Signal<T>` 是 UI 主线程局部状态（`thread_local!`）。绝不可在 `cx.background_spawn(...)` 闭包内读写 `Signal<T>`；务必将后台计算或网络 I/O 结果 `.await` 返回前景后，在 `this.update(&mut cx, |view, _| { ... })` 中统一更新状态与信号。
3. **保持业务代码调度器无关**：业务 `View` 代码统一依赖 `acx.spawn`、`cx.background_spawn`、`cx.background_executor()` 与 `cx.timer(dur)`，不直接硬编码 `tokio::spawn` 或 `Timer::after`——从而使同一份业务代码无需修改即可在默认 `NativeStdScheduler`、`TokioScheduler`、`SmolScheduler` 与测试 `ManualScheduler` 之间自由切换。
4. **调度器适配层严格 Feature 隔离**：外部异步运行时适配器集中在 `crates/velox-async` 中并经由 `velox-kit` 的可选 feature（`tokio` / `smol`，默认全关）按需开启；`velox-core` 与 `velox-base` 严禁依赖任何第三方异步运行时。
5. **无真睡确定性测试**：凡涉及定时器或异步状态流转的逻辑，优先配合 `ManualScheduler::new()` + `sched.advance(dur)` + `sched.drive_engine(&mut engine)` 编写 0ms 瞬时确定性单测。

---

## 生命周期、ScratchArena 与 ID

1. **帧内借用生命周期 `'a`**：
   - `View::render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a>` 中的 `'a` 绑定到当前帧的 `ScratchArena`。
   - 需要跨 `render` 返回的动态字符串必须分配在 `'a` 中：`fmt!(cx, "FPS: {}", fps)`（`StrRef<'a>`）、`self.buf.as_arena_str(cx)`（`&'a str`）或 `cx.arena().alloc_str(s)`。
   - 需要传给 `Table`、`Sidebar`、`Tabs`、`Select`、`RadioGroup`、`Menu`、`Chart` 等组件的切片参数，若是常量数据务必声明为 `const DATA: &[...] = &[...];`，若是动态过滤结果务必写入 `cx.arena().alloc_iter(...)`。**绝对禁止写 `let slice = &[...]; Component::new(id, slice)` 并在末尾返回，必然触发 `E0515`/`E0716` 编译失败**。
2. **稳定身份 `u32` ID 与 `.key()`**：
   - 有交互、过渡动画、滚动记忆或焦点态的 `Div` 和组件必须拥有稳定且在同一父级作用域内不冲突的 `u32` ID（`.id(101)` 或动态条目 `.key(domain_id_bytes)`）。
   - **禁止使用易变列表下标做 `.id(idx as u32)` 作为有状态过渡元素的唯一身份**——条目重排或删除时会导致 `MotionCache` 错误复用前一元素的位置插值。优先采用业务主键：`.key(&item.id.to_le_bytes())`。

---

## 渲染与链式组合

- 从原子函数起步：`div()`、`text(...)`、`img(...)`、`svg(...)`、`img_shader(...)`。
- 始终记住第一个参数是 `cx`：`.child(cx, ...)`、`.children(cx, iter)`、`.content(cx, ...)`、`.footer(cx, ...)`。
- 条件挂载子元素采用结构化构建：

```rust
let mut panel = div().id(10).flex_col().gap(8);
if let Some(err) = self.error_msg {
    panel = panel.child(cx, Alert::new(11, "Validation Error", err).variant(AlertVariant::Error));
}
if show_footer {
    panel = panel.child(cx, footer_row);
}
panel.into_element(cx)
```

- 浮层层级控制：
  - 同级覆盖顺序使用 `.z_index(z)`；
  - 脱离滚动/父级裁剪并在窗口最顶层渲染使用 `.overlay()`；
  - 需要吞噬底层鼠标/触控点击使用 `.overlay().occlude_hit()`（所有模态弹窗、确认弹窗与弹出菜单都必须带此组合）。

---

## 行为与视觉表现边界

- **行为属于 `velox-base`**：命中测试、禁用拦截、输入缓冲、光标与选区、滚动边界、折叠展开逻辑与键盘处理全部封装在 `velox-base`；`velox-base` 组件只在**调用方未定制配色或尺寸**时温和回退 `cx.theme()` 默认值。
- **视觉定制保留在 `velox-component` 与应用层**：通过 `Variants`、`.text_color(...)`、`TailwindStyled` 或直接修饰外层容器完成视觉差异化；不要去修改 `velox-base` 的核心交互事件契约以迁就某个单一页面排版。

---

## 主题与 24 字节紧凑样式

- `Style` 严格占 `24 Bytes`（含位域打包的 `flex_dir`、`justify`、`items_align`、`flex_grow`、`rounded`、`flex_wrap`、`border_width`、`margin_px`），以保证 `ElementNode` 高速缓存命中与极低帧内存占用。
- 注意类型上限：`padding` 与 `gap` 为 `i8`（`-128..=127`），`rounded` 为 `u8`（`0..=127`），`border` 边框宽度与紧凑 `.m(px)` 外边距为 `0..=15px`（大于 15px 的外边距请使用 `Div` 上的 `.mt(i16)` / `.mb(i16)` / `.ml(i16)` / `.mr(i16)` / `.mx(i16)` / `.my(i16)`，它们在 `DivVisual` 中存储为 `i16`！）。
- 通过 `self.theme_mode.to_theme()` + `cx.override_theme(theme)`（或 `Root::theme_in(cx, theme)`）切换全应用配色。所有自定义绘图应读取 `cx.theme()` 字段而非手写 RGB。

---

## 事件、手势、焦点与路由栈

- 引擎统一手势参数由 `GestureConfig`（`drag_slop_px`、`swipe_threshold_px`、`edge_back_width_px`、`long_press_ms`、`repeat_delay_ms`、`repeat_interval_ms`、`mouse_drag_scroll`）控制，避免应用层重新造轮子去用定时器猜长按或连发。
- **指针设备语义隔离（`PointerKind`）**：引擎区分 `PointerKind::Touch`、`PointerKind::Mouse` 与 `PointerKind::Pen`：
  - 触摸屏（`Touch`）默认支持空白区域跟手拖拽滚动、松手惯性滚动与左边缘右滑返回；
  - 桌面鼠标（`Mouse`）下 `GestureConfig::mouse_drag_scroll` **默认为 `false`**，滚动由滚轮与滚动条驱动，防止空白处误拖整个页面（组件显式 `.on_drag` 如 `Slider` / `ResizableSplit` / 文本选区不受影响；仅地图/白板类画布可按需显式打开 `mouse_drag_scroll = true`）。
- 需要键盘导航（`Tab` / `BackTab` / `Enter` / `Escape`）的可交互区块使用 `.focusable()` 或 `.focus_border(2, theme.primary)`；输入框通过 `.on_key_input(cx.listener(|this, key: KeyCode, _| ...))` 配合 `EditableText::handle_key` 或 `TextBuffer::handle_key` 消费输入。
- 多页面或主从导航堆栈优先使用零堆分配 `Navigator<Route, DEPTH>`，配合 `PageTransition` 与 `.on_swipe(cx, ...)` / `InputEvent::NavigateBack` 处理移动端左边缘右滑返回、Android 返回键及浏览器 `popstate`。

---

## 动画与连续帧控制

1. **声明式简单过渡**：直接在具名 `Div`（`.id(u32)` 或 `.key(...)`）上声明 `.animate_color(dur)`、`.animate_bounds(dur)`、`.animate_opacity(dur)`、`.animate_all(dur)`、`.fade_in(dur)`、`.slide_in(dx, dy, dur)`、`.ping_pong_bg(from, dur)` 与 `.ease(Easing::Spring)`。`VeloxEngine` 会根据 `id` 自动比对前后帧差异并在 `MotionCache` 中完成全整数插值；动画结束自动静止，不浪费下一帧。
2. **自定义数值动画与退场动画**：在 `View` 字段中存放 `Tween` 或 `Presence`（`Copy` 零堆分配）：
   - 在事件中调用 `tween.animate_to(acx.now_ms(), target)` 或 `presence.hide(acx.now_ms())`；
   - 在 `render` 中通过 `let now = cx.now_ms();` 采样 `presence.opacity(now)`、`presence.offset(now, dist)`，仅当 `presence.is_mounted(now)` 为真时挂载元素，并调用 `cx.keep_animating(presence.is_animating(now));`——动画完成瞬间自动停止请求下一帧，恢复 0% CPU 休眠。

---

## 布局求解与滚动

`Two-Pass Axis Solver` 采用无 FPU 的 `i16` 四步一维弹性布局（Horizontal First, Height-for-Width）：

- 父容器需要把剩余宽度或高度分配给工作区主面板时，使用 `.flex_grow(1)`（支持 `1..=3`）；确保父链条具有明确约束（如根元素 `.size_full()`，行容器 `.w_full().flex_grow(1)`，右侧内容区 `.flex_grow(1).h_full()`）。
- 滚动容器使用 `.scrollable(max_scroll)` 或自适应测量内容的 `.scrollable_auto()`（会自动开启 `.overflow_hidden()`）；长内容或万级数据列表绝不要把所有节点直接 `.child()` 进滚动容器，必须改用 `VirtualList` / `UniformList`。
- 移动与全面屏根壳层记得使用 `NavBar`（默认 `.safe_top(true)`）、`TabBar`（默认 `.safe_bottom(true)`）或 `div().safe_area(cx)` / `.pt_safe(cx)` / `.pb_safe(cx)`。

---

## 列表、表格与代码编辑器

- **超长等高列表（100+ 至 100,000+ 行）**：统一使用 `uniform_list` / `virtual_list(id, count, row_h, viewport_h, scroll_y, |idx, cx| ...)`（大列表偏移调用 `.scroll_y_i32(...)`），保证单帧 `ScratchArena` 占用恒定 `O(visible_rows)`。
- **结构化表格**：列定义使用 `const COLS: &[TableColumn<'static>]`，交互表格使用 `DataTable` 处理行选中与列排序回调。
- **代码编辑器**：`CodeEditor::new(id, self.buf.as_arena_str(cx), self.focused)` 搭配 `TextBuffer<CAP>`、`HighlightSpan` 与 `DiagnosticSpan`；工作台编辑器的 `Undo` / `Redo` 应直接调用 `TextBuffer::undo()` / `TextBuffer::redo()`，避免手动维护另一套文本历史。

---

## 公共 API 与 Feature 设计

- **Cargo Feature 必须严格可加（Additive）**：开启 `component`、`gpu`、`winit`、`tokio`、`smol` 等任何 Feature 都不能破坏下游调用。`tokio` 与 `smol` 默认关闭，保持默认编译零外部异步运行时依赖；需要类型绝对不变时向用户暴露 `Base*` / `Themed*` 别名。
- **类型安全与显式构建器**：公开选项/配置结构体优先使用 `pub const fn new(...)` 搭配链式 Builder（`mut self -> Self`）；不暴露绕过不变量的原始可变内部指针。
- **帧级零堆分配约束**：`velox-core` 与 `velox-base` 的公共组件在渲染热路径上优先使用 `ScratchArena`、`StrRef<'a>`、`SmallVec<T, N>` 与常量泛型容量参数 `CAP`，避免逐帧堆分配。

---

## 跨平台后端与渲染器边界

- 跨平台后端全部内聚于 `velox_kit::platform`：
  - Desktop：`WinitPlatform`（Windows Win32 / macOS AppKit / Linux Pure Wayland）与 `WaylandPlatform`（无 `winit` 的纯 Wayland Wire 客户端，`xdg_toplevel` / `zwlr_layer_shell_v1`）；
  - Mobile：`export_android_app!`、`export_ios_app!`、`export_harmony_app!` 搭配 `AndroidHostBridge` / `IosHostBridge` / `HarmonyHostBridge`；
  - Web：`export_web_app!` 搭配 `WebCanvasApp`（`Rgba8888` + 局部脏矩形 `dirty_rects`）；
  - Custom / Async Hosts：通过实现公开 Trait `Platform`、`PlatformSurface`、`AsyncPlatform`、`AsyncPlatformSurface` 接入。
- 渲染后端由 `DesktopRenderBackend` 统一调度（`Wgpu`、`Software`）；业务逻辑代码**绝不对单一渲染器编写分支判断**，只描述 `Element<'a>` 与 `SurfacePainter`。

---

## 文件与命名约定

- 组件模块名与主类型对应：`text_input.rs` -> `TextInput`、`code_editor` (`input/editor.rs`) -> `CodeEditor`、`bottom_sheet.rs` -> `BottomSheet`。
- 事件回调统一以 `on_` 开头：`on_click`、`on_change`、`on_select`、`on_toggle`、`on_close`、`on_dismiss`、`on_focus`、`on_key_input`、`on_pointer_select`、`on_row_click`。
- 公开类型、字段与公共方法均应编写清晰简洁的中文 Rustdoc（与现行 Crate 文档风格保持一致，说明参数单位如逻辑像素 `i16`、时间 `ms` 与状态语义）。

---

## 测试策略与无头集成测试

- UI 逻辑测试直接复用真实 `View` 结构与 `VeloxEngine::new(w, h)`，**严禁仅为了测通过而构造假组件**；
- 必须覆盖：
  1. 初帧渲染 + 次帧空操作进入 `PlatformControlFlow::Wait`（确认没有遗漏清除脏标记或无限 `request_frame()` 循环）；
  2. 点击与手势坐标真实命中（`dispatch_click`、`dispatch_pointer_down/move/up`）；
  3. 模态弹出时底层元素命中必须返回 `false`（确认 `.overlay().occlude_hit()` 生效）；
  4. 动态尺寸变化 `engine.resize(w, h)` 产生全视口脏区并保持布局正确；
  5. 若改动 `velox-core`，务必额外运行 `cargo check -p velox-core --no-default-features` 确保最小软光栅裁剪构建不退化。

---

## 性能快路径与红线

- **Pass-4-Only 快路径**：仅悬停/按下态、滚动偏移或 `MotionCache` 过渡变化时，引擎自动跳过 `View::render()` 与 Pass 1~3；若你在每一帧无条件修改无关 `View` 状态或给容器设置变化随机值，会破坏此快路径。
- **`SubView` 局部重排边界**：当独立 `Handle<SubView>` 未标脏时，引擎复用缓存 `Element`；且叶子 `SubView` 若拥有明确非 `Auto` 宽高边界，修改其内部状态可在不重新构建全局布局树的前提下原地完成局部重排。
- **定长 8 槽位脏矩形合并（`DamageTracker`）**：仅对实际变化的脏区光栅化与提交；静态大背景与动态数字应分开封装，避免数字变动迫使无关大图重建。

---

## 常见失败模式与 Agent 铁律

编写或审查任何 Velox 代码前，核对以下常见坑位：

1. **误用其他 GUI/CSS 方法名与主题字段**：写 `.flex_1()`、`.size_full().child(btn)`（漏了 `cx`）、`.id("str")`、`.into_any_element()`、`theme.destructive` —— 全都会编译失败。真实写法：`.flex_grow(1)`、`.child(cx, btn)`、`.id(100)` / `.key("str")`、`.into_element(cx)`、`theme.danger`。
2. **`let slice = &[...];` 局部生命周期报错 (`E0515` / `E0716`)**：传入 `Table` / `Sidebar` / `Select` / `Tabs` 的切片在函数末尾被销毁；改为 `const` 或 `cx.arena().alloc_iter(...)`。
3. **直接丢弃 `Task<T>` 或在后台线程操作 `Signal<T>`**：调用 `acx.spawn(...)` 后未赋值给 `self.task` 也没 `.detach()`，导致任务创建即被 Drop 取消；或在 `cx.background_spawn` 内直接读写 `Signal<T>` 破坏主线程隔离。
4. **模态对话框未阻断底层点击**：只调用 `Dialog::new(...)` 或外部容器只写 `.overlay()` 而遗漏 `.occlude_hit()`，导致用户点弹窗外空白处意外触发背后侧边栏。
5. **`TextInput` / `CodeEditor` 绑定字符串直接传入字段 `&self.buf.as_str()` 与 `&mut Scope<Self>` 借用冲突**：先调用 `let s = self.buf.as_arena_str(cx);` 复制到当前帧 `ScratchArena`（零堆分配），再传 `s` 并使用 `cx.listener(...)`。
6. **在核心/组件文件直接引入第三方异步运行时或 GPU 依赖而未加 feature 门控**：直接破坏 `velox-core` / `velox-base` 的最小裁剪与零外部异步依赖契约。

---

## 交付检查清单

- [ ] 业务 Crate 只依赖 `velox-kit`，通过 `use velox_kit::prelude::*;` 导入（需要 Tokio/smol 时仅按需开启对应 feature）。
- [ ] 已对照 `velox-design-guides` 检查主题 Token、间距密度、状态反馈与弹窗阻断。
- [ ] 所有 `.child(cx, ...)`、`.into_element(cx)` 与切片生命周期 `'a` 无临时借用问题。
- [ ] 异步任务 `Task<T>` 均已正确托管（存入 `View` 字段、`.await` 或 `.detach()`），且无跨线程读写 `Signal<T>`。
- [ ] 需要拦截背景穿透的浮层均带有 `.overlay().occlude_hit()`。
- [ ] 无动画且无进行中异步动画时能回归 `PlatformControlFlow::Wait`（0% CPU 空闲休眠）。
- [ ] 已运行 `cargo test --workspace` 验证交互流，若修改核心库则通过 `cargo check -p velox-core --no-default-features`。
