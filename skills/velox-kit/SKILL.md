---
name: velox-kit
description: >-
  How to build cross-platform GUI applications with Velox (`velox-kit`, re-exporting
  `velox_kit::core`, `velox_kit::base`, `velox_kit::component`, `velox_kit::platform`,
  `velox_kit::app`, `velox_kit::async_rt`, and `velox_kit::scheduler`).
  Use when setting up a Velox desktop (`application()` / `open_window()` on Windows/macOS/Pure Wayland),
  mobile (Android/iOS/HarmonyOS), or WebAssembly (`<canvas>`) app; choosing or composing
  the 70+ components (`Button`, `TextInput`, `CodeEditor`, `Select`, `Dialog`, `BottomSheet`, `Sidebar`,
  `Tabs`, `Table`, `DataTable`, `VirtualList`, `Tree`, `Chart`, `ArcGauge`, etc.); writing GPUI-style
  dual-executor async tasks (`acx.spawn`, `cx.background_spawn`, RAII `Task<T>`, `ManualScheduler`,
  optional `velox-async` features `tokio`/`smol`, and `AsyncDriver`); managing `View` /
  `Handle<T>` / `Signal<T>` (including `.bind()`), `ScratchArena` lifetimes, `PointerKind`, theming
  (`CoreTheme` / `ThemeMode`), overlays, motion, custom SDF sprites, or deterministic headless UI tests.
  Holds the normative Coding Guides; pairs with `velox-design-guides` for visual and interaction design
  decisions.
  用于使用 Velox（`velox-kit`）构建跨平台 Rust GUI 应用、极简桌面启动、GPUI 风格异步任务与可插拔调度器
  （NativeStd/Tokio/smol/Manual）、选择与组合组件、状态与信号绑定、布局主题配色、动画手势与无头集成测试。
---

# Velox Kit (`velox-kit`)

应用程序只需依赖唯一入口 Crate：`velox-kit`。常用类型与构建器通过 `use velox_kit::prelude::*;` 一次性导入，同时各分层命名空间也可按需显式访问：

- `velox_kit::core`（`velox-engine`）：帧级零堆 UI 引擎内核（`ScratchArena`、`PodPool`、`Signal<T>`、24B `Style` / `Styled`、四步一维弹性布局求解器、`MotionCache`、`DamageTracker`、`i18n`、`a11y`），默认开启 `gpu`（自研轻量级 `wgpu` 2D SDF 渲染器）
- `velox_kit::app`（由 `velox_kit::*` 与 `prelude` 重导出）：极简桌面启动层（`application()`、`init(cx)`、`open_window(...)`、`WindowOptions`、`Application::build_headless`）
- `velox_kit::async_rt` / `velox_kit::scheduler`：跨平台分层异步运行时（通用 `SchedulerClock` / `AsyncDriver` / `run_async_event_loop`；双执行器 `ForegroundExecutor` / `AsyncApp` / `BackgroundExecutor` / RAII `Task<T>` / `NativeStdScheduler` / `ManualScheduler`；按可选 feature `tokio` / `smol` 接入独立 `velox-async` 适配层 `TokioScheduler` / `SmolScheduler`）
- `velox_kit::base`（`velox-base`，通过 `base` feature 启用，默认随 `component` 开启，亦有不随 feature 变更类型的 `Base*` 别名）：70+ 无样式交互行为组件、`EditableText`、`TextBuffer`、`CodeEditor`、`Navigator`、`UniformList` / `VirtualList`、`Sizable` / `Variants` / `Disableable`
- `velox_kit::component`（`velox-component`，通过 `component` feature 启用，**默认开启**，亦有 `Themed*` 别名）：Shadcn Zinc + Blue Tailwind 风格主题化包装组件与 `TailwindStyled`
- `velox_kit::platform`：跨平台窗口与事件适配器（`WinitPlatform`、`WaylandPlatform`、`mobile`、`android`、`ios`、`harmony`、`web`、`TestingPlatform`、`Platform` / `AsyncPlatform`）

先阅读 [组件家族约定](references/conventions.md) 了解各类组件的构造器签名、状态归属与回调契约；完整可编译应用写法见 [经过测试的应用完整配方](references/recipes.md)。

## 先读这两份规范指南

以下两份指南定义了 Velox 开发必须遵守的工程与设计红线，动笔前请直接按需阅读对应章节，切勿凭其他 GUI 框架/React/CSS 习惯臆测 API。

| 指南                                            | 何时必读                                                                                 |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------- |
| 设计指南：`velox-design-guides` 技能            | 组件选型、布局结构、间距圆角、视觉层级、语义配色、交互状态、浮层遮罩、动效节奏与文案规范 |
| [编码规范](references/coding-guides.md)         | Crate 分层、`View` vs `Widget`、`PodPool` & `Signal`、生命周期与借用、回调、公共 API 与测试 |

当需求涉及可见界面时，先加载 `velox-design-guides`（直接读取 `../velox-design-guides/references/design-guides.md`）。

### Coding Guides 章节索引

编写新 Crate、模块或大型特性时建议通读 [编码规范](references/coding-guides.md)；局部修改可先看“架构速览”与“Agent 编码铁律”，再用关键字定位对应章节：

| 章节                        | 何时阅读                                                        |
| --------------------------- | --------------------------------------------------------------- |
| 架构速览与分层边界          | 永远必读；理解 `velox-engine` / `base` / `component` / `async` / `kit` 边界 |
| 启动引导与根视图归属        | `application()`、`open_window()`、`WindowOptions`、`VeloxEngine`、`Root` |
| 理解 Velox 的上下文阶段     | 涉及 `FrameScope`、`Scope<V>`、`ActionScope`、`AsyncApp`、渲染与事件阶段 |
| 选择正确的抽象单元          | 抉择无状态 `Widget<'a>`、有状态 `View` (`Handle<T>`)、或 `img_shader` |
| 状态归属与响应式信号        | 状态存放位置、`PodPool`、`Signal<T>` / `.bind()` / `batch` / `Computed<T>` / `Effect` |
| 异步运行时与可插拔调度器    | `acx.spawn`、`background_spawn`、RAII `Task<T>`、`velox-async` (`tokio`/`smol`)、`ManualScheduler` |
| 生命周期、ScratchArena 与 ID | `'a` 生命周期、`fmt!`、`const`/Arena 切片、稳定 `u32` ID / `.key()` |
| 渲染与链式组合              | `.child(cx, ...)`、`.into_element(cx)`、侵入式链表与零堆分配    |
| 行为与视觉表现边界          | `velox-base` vs `velox-component` vs 应用层样式定制             |
| 主题与 24 字节紧凑样式      | `cx.theme()`、`ThemeMode`、`Styled`、`TailwindStyled`、显式优先规则 |
| 事件、手势、焦点与路由栈    | `cx.listener`、`cx.action`、`PointerKind`、`GestureConfig`、`focusable`、`Navigator` |
| 动画与连续帧控制            | `Div` 声明式过渡、`Tween`、`Presence`、`cx.request_frame()`、0% 休眠 |
| 布局求解与滚动              | `flex_grow`、`Two-Pass Axis Solver`、`scrollable`、`SafeAreaInsets` |
| 列表、表格与代码编辑器      | `VirtualList` / `UniformList`、`Table` / `DataTable`、`CodeEditor` |
| 公共 API 与 Feature 设计    | 构造器参数、公开字段、Trait 扩展、可加性 Feature 与零外部异步依赖约束 |
| 跨平台后端与渲染器边界      | `Winit` / `Wayland` / `Mobile` / `Web` 与双渲染后端               |
| 文件与命名约定              | 模块划分、结构体与方法命名、文档风格                            |
| 测试策略与无头集成测试      | `build_headless`、`VeloxEngine`、`ManualScheduler`、`TestingPlatform`、PNG 快照 |
| 性能快路径与红线            | `Pass-4-Only`、`SubView` 局部重排边界、`DamageTracker`          |
| 常见失败模式与 Agent 铁律   | 提交前自检：杜绝臆测 API、`E0515`/`E0716`、丢弃 `Task` 与模态穿透 |

### 不可违背的开发铁律（Non-negotiables）

- **严禁凭空捏造 API**：写任何方法前先查阅 `crates/` 源码确认签名，**绝不**将其他 GUI 框架/React/Tailwind 的方法直接套用过来（例如：Velox 没有 `div().flex_1()`，应使用 `.flex_grow(1)`；没有字符串 ID `.id("btn")`，应使用 `.id(u32)` 或 `.key("btn")`；主题危险色是 `theme.danger`，不是 `theme.destructive`）。
- **唯一入口依赖**：应用程序只在 `Cargo.toml` 中引入 `velox-kit`（接入 Tokio / smol 时仅需开启 `features = ["tokio"]` 或 `features = ["smol"]`）。
- **所有容器组合与转换都显式传 `cx`**：
  - 追加子节点必须写 `.child(cx, ...)` / `.children(cx, ...)`；
  - 结尾收口必须写 `.into_element(cx)`。
- **尊重 `'a` 借用生命周期（零堆分配约束）**：
  - 动态格式化文本用 `fmt!(cx, "Value: {}", self.v)`；
  - 传入组件切片参数（如 `SidebarItem`、`TableColumn`、`SelectOption`、`MenuItem`）的数组必须具有 `const` `'static` 生命周期或由 `cx.arena().alloc_iter(...)` 分配，绝不能引用局部临时切片 `let items = &[...]`。
- **正确区分回调构造器**：
  - 需要接收事件参数 `E`（`ClickEvent`、`usize`、`u32`、`bool`、`i32`、`KeyCode`、`SwipeDir`）：使用 `cx.listener(|this, ev, acx| { ... })`；
  - 无参数动作回调（`on_dismiss`、`on_close`、`on_focus`、`on_toggle`、`on_click_raw`）：使用 `cx.action(|this, acx| { ... })`。
  - 注意：`cx.listener` 与 `cx.action` 在回调返回后默认自动标脏当前 `PodId`（无需手动调用 `acx.notify()`；若仅只读未改状态可调用 `acx.skip_notify()`）。
- **异步任务 RAII 与线程安全红线**：
  - `acx.spawn(...)` / `cx.background_spawn(...)` 返回 `#[must_use]` 的 `Task<T>`，**直接丢弃会立即取消**——必须保存在 `View` 字段（`Option<Task<()>>`）、`.await` 或显式 `.detach()`；
  - `Signal<T>` 仅限 UI 主线程访问，**严禁在 `background_spawn` 后台线程内读写 `Signal<T>`**，必须在 `.await` 回到前景后于 `this.update(&mut cx, ...)` 内更新。
- **模态浮层必须阻断命中穿透**：任何模态弹窗背景遮罩（如 `Dialog`、`Modal` 外层）都必须同时链式调用 `.overlay().occlude_hit()`。

---

## 快速上手参考

接入方式与详尽组件示例见 [references/usage.md](references/usage.md)，异步与多调度器完整说明见 [references/core/async.md](references/core/async.md)。

```rust
use velox_kit::prelude::*;

struct HelloApp {
    count: i32,
}

impl View for HelloApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let on_inc = cx.listener(|this, _: ClickEvent, _acx| {
            this.count += 1;
        });

        div()
            .id(1)
            .size_full()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(12)
            .bg(cx.theme().background)
            .child(cx, text(fmt!(cx, "Count: {}", self.count)).size(18).color(cx.theme().foreground))
            .child(cx, Button::new(2, "Increment").primary().on_click(on_inc))
            .into_element(cx)
    }
}

fn main() {
    velox_kit::application().run(|cx| {
        velox_kit::init(cx);
        velox_kit::open_window(
            WindowOptions::new("Hello Velox").size(480, 320),
            cx,
            |cx| cx.new(|_| HelloApp { count: 0 }),
        )
        .expect("Failed to open window");
    });
}
```

- **极简桌面启动与无头复用**：使用 `velox_kit::application()`（支持 `.with_theme(...)`、`.with_locale(...)`、`.with_backend(...)`、`.with_dpi_scale(...)`、`.with_scheduler(...)`；开启 feature 后可链式 `.with_tokio()` / `.with_smol()`）调用 `.run(|cx| ...)` 启动原生窗口，或在测试/CI 中调用 `.build_headless(|cx| ...)` 获取 `BuiltWindow` 零弹窗复用同一初始化逻辑。
- **GPUI 风格双执行器异步 (`acx.spawn` + `Task<T>`)**：在事件回调中调用 `self.task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| { let v = cx.background_spawn(...).await; let _ = this.update(&mut cx, |view, _| { ... }); }));`；将 `self.task = None` 或覆盖赋值即可通过 RAII `Drop` 自动取消旧任务。
- **无状态组件 (`Widget<'a>`)**：在 `render` 中内联构造，不占 `PodPool` 槽位：
  `Button::new(101, "Save").primary().on_click(cx.listener(|this, _: ClickEvent, _acx| { ... }))`
- **有状态子树 (`View` + `Handle<T>`)**：通过 `wcx.insert_view(SubView::new())` / `engine.insert_view(...)` 注册并用 `.child(cx, self.sub_handle)` 挂载，配合明确宽高自动成为 L2 局部重排与重绘隔离边界。
- **细粒度响应式 (`Signal<T>` 与 `.bind()`)**：`let count = signal(0i32);` 在 `View::render` 中调用 `count.get()` 自动订阅当前 `PodId`，在事件中调用 `count.set(...)` / `set_if_changed(...)` / `batch(|| ...)` 仅标脏读取过它的叶子 `SubView`；`Switch::bind(cx, id, sig)`、`Checkbox::bind`、`Slider::bind`、`Tabs::bind`、`RadioGroup::bind` 等支持零堆双向信号绑定。
- **尺寸 / 变体 / 禁用**：`Sizable`（`.small()` / `.medium()` / `.large()`）、`Variants`（`.primary()` / `.secondary()` / `.outline()` / `.warning()` / `.ghost()`）、`Disableable`（`.disabled(bool)`）。
- **主题令牌 (`CoreTheme`)**：`background` · `foreground` · `card` · `popover` · `primary` · `secondary` · `muted` / `muted_foreground` · `accent` · `danger` · `success` · `warning` · `info` · `border` · `input` · `ring` · `sidebar` · `list_hover` · `list_active`；通过 `ThemeMode::Dark / Light` 切换并在根视图调用 `cx.override_theme(theme)`。

---

## 组件目录（Component Catalog）

下列组件全部由 `velox_kit::prelude::*` 与 `velox_kit::{base, component}` 导出。注意：Velox 组件首参绝大多数为稳定 `u32` ID（或按族约定构造），请对照表中模块名在 `crates/velox-base/src/components/` 或 `crates/velox-base/src/input/` 中核对完整字段与链式方法。

### 输入与表单（Input & Form）

| 组件            | 源码模块              | 核心状态/说明                                                      |
| --------------- | --------------------- | ------------------------------------------------------------------ |
| `TextInput` / `Input` | `components::text_input` / `input` | 配合 `EditableText<CAP>`、聚焦态 `bool`、`cursor` / `selection` / `masked` |
| `Textarea`      | `input::textarea`     | 配合 `TextBuffer<CAP>` 多行编辑、`Selection`                       |
| `CodeEditor` / `Editor` | `input::editor` | 配合 `TextBuffer<CAP>`、行号栏、缩进导引线、`HighlightSpan`、`DiagnosticSpan` |
| `NumberInput`   | `components::number_input` | 受控 `i64` 数值与加减步进（`NumberStep`）、`.range(min, max)`      |
| `OtpInput`      | `components::otp_input` | 配合 `OtpState<N>` 一次性验证码分格输入                           |
| `Speech`        | `components::speech`  | 配合 `SpeechState` 与 `SpeechStatus` 语音听写状态展示              |
| `Select`        | `components::select`  | 受控下拉选择器，配合 `&'a [SelectOption<'a>]`、`selected_index`、`open` |
| `Combobox`      | `components::combobox`| 可输入过滤下拉选择器                                               |
| `Dropdown`      | `components::dropdown`| 轻量下拉列表，带 `.overlay()` 弹出面板                             |
| `Checkbox`      | `components::checkbox`| 受控布尔勾选框，`on_change` 接收 `bool`；支持 `Checkbox::bind(cx, id, sig)` |
| `Switch`        | `components::switch`  | 受控拨动开关，`on_change` 接收 `bool`；支持 `Switch::bind(cx, id, sig)` |
| `Radio` / `RadioGroup` | `components::radio` | 受控单选与单选组，`on_change` 接收 `usize`；支持 `RadioGroup::bind` |
| `Slider`        | `components::slider`  | 受控 `i32` 滑块，`.range(min, max)`、拖拽定位与 `Slider::bind(cx, id, sig)` |
| `Toggle` / `ToggleGroup` | `components::toggle` | 切换按钮与位掩码切换组                                        |
| `Rating`        | `components::rating`  | 星级评分控件（`u8`）                                               |
| `Stepper`       | `components::stepper` | 多步骤进度引导（`StepItem` / `StepStatus`）                        |
| `ColorPicker` / `ColorSwatch` | `components::color_picker` | 配合 `ColorPickerState` 选色器与色板色块                   |
| `DatePicker`    | `components::date_picker` | 配合 `SimpleDate` 与展开状态                                   |
| `TimeField`     | `components::time_field` | 配合 `TimeFieldState`（`HourCycle` / `TimePrecision` / `TimeSegment`） |
| `Calendar`      | `components::calendar`| 月历视图（`SimpleDate`、`DateSelection`）                          |
| `Roller`        | `components::roller`  | 滚轮选择器（`&'a [&'a str]`）                                      |
| `VirtualKeyboard` | `components::virtual_keyboard` | 屏幕虚拟键盘（`KeyboardMode`、`KeyAction`）               |
| `Form` / `FormField` | `components::form` | 表单容器与带必填/帮助/错误文案的字段包装（`FormLayout`）         |

### 展示与反馈（Display & Feedback）

| 组件             | 源码模块                  | 说明                                                               |
| ---------------- | ------------------------- | ------------------------------------------------------------------ |
| `Button`         | `components::button`      | 核心命令按钮，支持 `Variants`、`Sizable`、`Disableable`、`auto_repeat` |
| `DropdownButton` | `components::dropdown_button` | 主操作按钮 + 附加菜单展开触发                                  |
| `Icon`           | `components::icon`        | 内置矢量符号图标（`IconName`）或 `.symbol(...)`，支持 `Sizable`    |
| `Badge`          | `components::badge`       | 状态徽章（无需传 ID），支持 `Variants` 与 `Sizable`                |
| `Tag`            | `components::tag`         | 语义标签（`TagVariant`），支持关闭与点击回调                       |
| `Avatar`         | `components::avatar`      | 用户头像与在线状态点（`AvatarStatus`）                             |
| `Label`          | `components::label`       | 表单字段标题与补充描述文本                                         |
| `Link`           | `components::avatar`      | 超链接动作文本（语义上对应外部资源或导航跳转）                     |
| `Kbd`            | `components::kbd`         | 键盘快捷键胶囊徽章展示                                             |
| `Alert`          | `components::alert`       | 页内提示横幅（`AlertVariant`: Info/Success/Warning/Error）         |
| `Spinner`        | `components::spinner`     | 圆环加载指示器，传入旋转角度 `angle_deg`（配合 `cx.now_ms()`）     |
| `Skeleton`       | `components::skeleton`    | 骨架屏占位符（`SkeletonShape`）                                    |
| `Marker`         | `components::marker`      | 状态圆点/标签标记（`MarkerVariant`）                               |
| `Progress` / `ProgressBar` | `components::progress` | 水平百分比进度条（`0..=100`）                                 |
| `SelectableText` / `MarkdownView` / `TextView` | `components::text_view` | 富文本、Markdown 与纯文本展示容器（`TextViewFormat`） |
| `Card`           | `components::card`        | 自动回退 `theme.card` 与圆角边框的通用容器，支持 `Styled` / `.unstyled()` |
| `Image`          | `components::image` / `velox_engine::element::img` | 组件包装 `Image`（`ImageFit`）或内核零分配位图原语 `img()` / `svg()` |

### 浮层与弹窗（Overlay & Popups）

| 组件           | 源码模块                     | 说明                                                               |
| -------------- | ---------------------------- | ------------------------------------------------------------------ |
| `Root`         | `components::root`           | 窗口根容器，统一管理 `content`、`dialog` 遮罩层与 `notification` 浮层 |
| `Dialog`       | `components::dialog`         | 通用模态对话框（建议包裹在 `.overlay().occlude_hit()` 遮罩容器内） |
| `AlertDialog`  | `components::overlays`       | 重要/破坏性操作确认弹窗，`confirm` / `cancel` 双分支               |
| `Modal`        | `components::modal`          | 带半透明全屏拦截遮罩与入场动效的居中弹窗                           |
| `Sheet`        | `components::sheet`          | 四向桌面边缘抽屉面板（`SheetSide`: Left/Right/Top/Bottom）         |
| `BottomSheet`  | `components::bottom_sheet`   | 移动端底部弹出抽屉（自动避让 `safe_area.bottom` 与软键盘）         |
| `Notification` | `components::notification`   | 右上角浮动通知卡片（`NotificationVariant` / `action` / `on_dismiss`） |
| `Toast`        | `components::toast`          | 指定坐标的轻量短时提示浮层                                         |
| `Popover`      | `components::popover`        | 带触发器与 `.overlay().occlude_hit()` 弹出面板                     |
| `HoverCard`    | `components::overlays`       | 悬停详情浮卡                                                       |
| `Tooltip`      | `components::tooltip`        | 轻量悬停文字提示浮层                                               |
| `Menu` / `ContextMenu` | `components::menu`   | 命令菜单与右键上下文菜单（`MenuItem`、分隔线与快捷键文案）         |
| `Command`      | `components::command`        | 命令面板（`CommandGroup`、`CommandItem`、键盘筛选与选中）          |

### 导航与布局（Navigation & Layout）

| 组件            | 源码模块                 | 说明                                                               |
| --------------- | ------------------------ | ------------------------------------------------------------------ |
| `Sidebar`       | `components::sidebar`    | 可折叠桌面侧边导航栏（`SidebarSection`、`SidebarItem`、角标与折叠按钮） |
| `Tabs`          | `components::tabs`       | 分段切换页签栏，支持平滑背景色过渡                                 |
| `TabBar`        | `components::tab_bar`    | 移动端底部导航栏（自动避让 `safe_area.bottom`）                    |
| `NavBar`        | `components::nav_bar`    | 移动端顶部导航栏（自动避让 `safe_area.top`，含返回/右侧动作槽）    |
| `Toolbar`       | `components::toolbar`    | 桌面分组工具栏（`ToolbarGroup`）                                   |
| `StatusBar`     | `components::status_bar` | 桌面底部状态栏（`StatusBarItem`）                                  |
| `Pagination`    | `components::pagination` | 分页器（当前页 / 总页数 / 页码切换）                               |
| `Accordion` / `Collapsible` | `components::accordion` | 手风琴多组折叠（位掩码）与单栏可折叠容器                    |
| `GroupBox`      | `components::group_box`  | 带标题与说明的分组框容器                                           |
| `GridLayout`    | `components::grid_layout`| 多列仪表盘瓦片网格容器（`GridTile`）                               |
| `Resizable` / `ResizableSplit` | `components::resizable` | 可拖拽分栏容器（`SplitAxis`: Horizontal/Vertical）        |
| `Dock`          | `components::dock`       | 多停靠面板工作区容器（`DockWorkspaceState`、`DockPanel`）          |
| `Scrollable` / `Scrollbar` | `components::scrollable` | 可滚动容器与比例滚动条指示器（`ScrollbarAxis`）           |
| `Navigator`     | `velox_base::navigator`  | 定长栈深度无堆分配页面路由栈（`PageTransition`、`TransitionLayer`） |

### 数据展示（Data Display）

| 组件             | 源码模块                      | 说明                                                               |
| ---------------- | ----------------------------- | ------------------------------------------------------------------ |
| `Table` / `DataTable` | `components::table` / `data_table` | 斑马纹数据表格（`TableColumn`）与支持列排序（`SortDirection`）的数据表 |
| `VirtualList` / `UniformList` | `velox_base::list` / `components::virtual_list` | 恒定 `ScratchArena` 内存占用的等高虚拟滚动列表        |
| `List`           | `components::list`            | 轻量单选列表（`ListItem`、选中高亮与副标题/尾部摘要）              |
| `Tree`           | `components::tree`            | 层级树视图（`TreeNode`、`TreeState`、展开折叠与选中）              |
| `Diff`           | `components::diff`            | 代码对比视图（`DiffLine`、`DiffLineKind`、`DiffViewMode`）         |
| `DescriptionList`| `components::description_list`| 键值对详情列表（`DescriptionItem`）                                |
| `Settings`       | `components::settings`        | 结构化分组设置面板（`SettingsGroup`、`SettingsItem`、`SettingsValue`） |
| `Questionnaire`  | `components::questionnaire`   | 多步骤问卷/向导状态机与视图（`QuestionnaireState`、`QuestionItem`） |

### 对话与消息（Chat & Messaging）

| 组件             | 源码模块                       | 说明                                                               |
| ---------------- | ------------------------------ | ------------------------------------------------------------------ |
| `Message`        | `components::message`          | 对话消息行（`MessageRole`）                                        |
| `Bubble`         | `components::bubble`           | 聊天气泡（`BubbleAlign`、表情回应 `BubbleReaction`）               |
| `Attachment`     | `components::attachment`       | 文件/媒体附件卡片（`AttachmentKind`）                              |
| `MessageScroller`| `components::message_scroller` | 自动吸底滚动消息列表（`MessageScrollerState`）                     |

### 图表与遥测（Charts & Telemetry）

| 组件       | 源码模块                 | 说明                                                               |
| ---------- | ------------------------ | ------------------------------------------------------------------ |
| `Chart`    | `components::chart`      | 纯整数 SDF 抗锯齿图表（`ChartKind`: Line/Bar/Area/Pie/Candlestick，`CandlestickBar`） |
| `ArcGauge` | `components::arc_gauge`  | 圆弧百分比仪表盘（`.thickness(...)`、`.label(...)`）               |

---

## 无头集成测试（UI Integration Testing）

Velox 原生支持无显示服务器、确定性时钟与像素级断言的无头集成测试，直接在标准 `#[test]` 中运行：

- 使用 `VeloxEngine::new(w, h)` 或 `velox_kit::application().build_headless(|cx| ...)` 挂载真实生产视图 `View`，依次驱动 `step_frame()` / `step_frame_dt(dt)`、`dispatch_click(x, y)`、`dispatch_mouse_down` / `dispatch_touch_down`、`dispatch_pointer_down/move/up`、`dispatch_key(KeyCode)`、`resize(w, h)` 并断言视图字段、`Pod` 局部重绘计数与脏矩形列表 `engine.last_damage()`；
- 使用 `ManualScheduler::new()` 配合 `sched.advance(Duration)` 与 `sched.drive_engine(&mut engine)` 零真睡确定性测试异步任务、定时阶段与 RAII 取消；
- 使用 `TestingPlatform<W, H, P>` / `WinitPlatform` / `WaylandPlatform` / `MobilePlatform` 验证 `PlatformControlFlow::Wait`（0% 空闲 CPU 休眠）与像素缓冲区输出。

完整测试指南与可运行示例见 [testing.md](references/core/testing.md)。

---

## Velox 内核深入参考文档（Core References）

当任务涉及引擎底层机制、生命周期调优或跨平台宿主接入时，按需加载对应文档（每篇首行均有目录索引）：

| 主题                       | 文档                                                             | 何时加载                                                                 |
| -------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------ |
| 帧内存池与零堆字符串       | [arena-and-strings.md](references/core/arena-and-strings.md)     | `ScratchArena`、`SmallVec`、`StrRef`、`fmt!`、解决 `E0515`/`E0716` 借用 |
| 视图边界、状态池与上下文   | [views-pods-and-scopes.md](references/core/views-pods-and-scopes.md) | `View` vs `Widget`、`PodPool`、`Handle<T>`、`Scope`/`ActionScope`、跨视图读写 |
| 细粒度响应式信号图         | [signals-and-reactivity.md](references/core/signals-and-reactivity.md) | `Signal<T>`、`Computed<T>`、`Effect`、`batch`、`.bind()`、局部标脏       |
| 异步运行时与可插拔调度器   | [async.md](references/core/async.md)                             | `acx.spawn`、`background_spawn`、`Task<T>`、`velox-async` (`tokio`/`smol`)、`ManualScheduler`、`AsyncDriver` |
| 布局求解与紧凑样式         | [layout-and-style.md](references/core/layout-and-style.md)       | 24B `Style`、`Styled`、`TailwindStyled`、四步求解器、`Grid`、`SafeArea` |
| 输入事件、焦点与文本引擎   | [events-focus-and-input.md](references/core/events-focus-and-input.md) | `InputEvent`、`PointerKind`、手势、焦点链、`EditableText`、`TextBuffer`、`Navigator` |
| 动画过渡与过程化 SDF 绘制  | [motion-and-custom-paint.md](references/core/motion-and-custom-paint.md) | 声明式动画、`Tween`、`Presence`、`cx.request_frame`、`img_shader`、`SurfacePainter` |
| 跨平台后端与多渲染器       | [platforms-and-renderers.md](references/core/platforms-and-renderers.md) | `application()`、`Winit`、`Wayland`、`Android`/`iOS`/`Harmony`/`Web`、`Wgpu`/`Software`、`AsyncPlatform` |
| 确定性无头 UI 集成测试     | [testing.md](references/core/testing.md)                         | `build_headless`、`VeloxEngine`、`ManualScheduler`、`TestingPlatform`、空闲休眠与 PNG 快照 |
