# velox-kit Usage Guide

**Contents:** [依赖配置与 Feature 矩阵](#依赖配置与-feature-矩阵) · [初始化与启动](#初始化与启动) · [异步任务与可插拔调度器](#异步任务与可插拔调度器) · [有状态与无状态单元](#有状态与无状态单元) · [常用组件用法示例](#常用组件用法示例) · [主题切换与注入](#主题切换与注入) · [布局辅助与响应式拉伸](#布局辅助与响应式拉伸) · [浮层与模态遮罩规范](#浮层与模态遮罩规范) · [共享修饰 Trait](#共享修饰-trait)

## 依赖配置与 Feature 矩阵

### `Cargo.toml`

```toml
# 桌面应用（零配置开箱即用：默认启用 gpu + winit + component，使用内置零依赖 NativeStdScheduler）
[dependencies]
velox-kit = { path = "../../crates/velox-kit" }

# 需要复用 Tokio 生态（reqwest / sqlx / tokio::net）或轻量级 smol 时按需启用对应 feature：
# velox-kit = { path = "../../crates/velox-kit", features = ["tokio"] }
# velox-kit = { path = "../../crates/velox-kit", features = ["smol"] }

# 移动端 / WebAssembly 极简软光栅包体（关闭 default-features，按需加 "gpu" 或 "winit"）
# velox-kit = { path = "../../crates/velox-kit", default-features = false, features = ["component"] }
```

### 常用 Feature 一览

| Feature | 含义 |
| --- | --- |
| `default` | `gpu` + `winit` + `component`（桌面应用开箱即用 GPU 窗口与全套组件；默认 0 外部异步运行时依赖） |
| `std` | 标准库运行时、极简 `app` 启动层、内置双执行器异步与全部纯 Rust 跨平台桥接（Wayland / Android / iOS / HarmonyOS / Web） |
| `winit` | 跨平台桌面窗口（Windows Win32 / macOS AppKit / Linux Wayland）+ `softbuffer`（隐含 `std`） |
| `base` | 开启 `velox-base` 无样式基础组件库与 `Base*` 类型别名 |
| `component` | 开启 `velox-component` Shadcn Zinc + Blue 主题化包装组件与 `Themed*` 类型别名（隐含 `base`） |
| `gpu` | 开启自研轻量级 `wgpu` 2D SDF 硬件渲染后端（隐含 `std`；单 Pass Instanced 合批直出 Swapchain；内置 `software` 软光栅始终可用） |
| `tokio` | 按需启用 `velox-async/tokio` 适配层，导出 `TokioScheduler`、`.with_tokio()` / `.use_tokio()`（默认关闭） |
| `smol` | 按需启用 `velox-async/smol` 适配层，导出 `SmolScheduler`、`.with_smol()` / `.use_smol()`（默认关闭） |

---

## 初始化与启动

### 桌面极简启动（推荐：`velox_kit::application()`）

参照 `gpui-kit` 极简启动范式（详见 `examples/hello_world/src/main.rs`），数行代码即可拉起原生窗口并自动接管 `WinitUserEvent::WakeAsync` 零轮询唤醒与鼠标指针类型（`PointerKind::Mouse`）：

```rust
use velox_kit::prelude::*;

fn main() {
    velox_kit::application()
        // 可选链式配置：.with_theme(...) / .with_locale(...) / .with_dpi_scale(2) / .with_backend(...)
        // 启用 feature 后可一行切换调度器：.with_tokio() 或 .with_smol()
        .run(|cx| {
            velox_kit::init(cx);
            velox_kit::open_window(
                WindowOptions::new("My Velox App").size(960, 640),
                cx,
                |cx| cx.new(|_| MyApp::new()),
            )
            .expect("Failed to open window");
        });
}
```

需要复用同一初始化回调进行测试、`--headless` 或 `--save-png` 时，只需把 `.run(...)` 换成 `.build_headless(...)` 即可拿到 `Option<BuiltWindow>`（包含 `options` 与 `engine`）。

### 底层手动驱动（`WinitPlatform` / 纯 `WaylandPlatform`）

```rust
use velox_kit::platform::wayland::WaylandPlatform;
use velox_kit::platform::winit::WinitPlatform;
use velox_kit::platform::{Platform, PlatformControlFlow};
use velox_kit::prelude::*;

// 1. 直接持有 VeloxEngine 与 WinitPlatform（适合高度定制循环或截图导出）
let mut engine = VeloxEngine::new(960, 640);
engine.mount_root(MyApp::new());
WinitPlatform::new("My Velox App", 960, 640)
    .with_dpi_scale(2)
    .run_event_loop(engine);

// 2. 纯 Wayland 客户端（零 winit / 零 X11）
let mut wl_engine = VeloxEngine::new(960, 640);
wl_engine.mount_root(MyApp::new());
let mut plat = WaylandPlatform::new_xdg_toplevel("Velox App", "dev.velox.app", 960, 640);
match plat.step_platform(&mut wl_engine, 16) {
    PlatformControlFlow::Wait => { /* 0% CPU 休眠等待事件 */ }
    PlatformControlFlow::Poll => { /* 存在动画或待刷帧 */ }
    PlatformControlFlow::Exit => {}
}
```

---

## 异步任务与可插拔调度器

在事件回调（`cx.listener` / `cx.action`）中通过 `acx.spawn` 启动 UI 主线程前景协程，通过 `cx.background_spawn` 卸载耗时任务到后台调度器，并将 `Task<()>` 存入 `View` 字段实现 RAII 自动取消（完整细节与多调度器切换见 [core/async.md](core/async.md)）：

```rust
// 宿主 View 字段：pub task: Option<Task<()>>, pub loading: bool, pub result: SmallString<64>
let on_fetch = cx.listener(|this, _: ClickEvent, acx| {
    this.loading = true;
    // 覆盖旧 Task 自动 Drop 取消上一轮未完成请求
    this.task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| {
        let delay = cx.timer(Duration::from_millis(120));
        let answer = cx
            .background_spawn(async move {
                delay.await;
                "Sync OK"
            })
            .await;

        // 回到 UI 主线程安全写回状态（带 ABA 保护并自动标脏重绘）
        let _ = this.update(&mut cx, |view, _| {
            view.loading = false;
            view.result = SmallString::from_str(answer);
            view.task = None;
        });
    }));
});

// 点击取消按钮仅需释放 Task 句柄：
let on_cancel = cx.listener(|this, _: ClickEvent, _| {
    this.task = None;
    this.loading = false;
});
```

- 接入 Tokio 生态（如 `reqwest::Client::get(...)`）时，仅需在 `Cargo.toml` 开启 `velox-kit` 的 `tokio` feature 并在启动时调用 `velox_kit::application().with_tokio()`（或 `engine.use_tokio()`），业务 `View` 代码无需任何改动。

---

## 有状态与无状态单元

- **无状态 `Widget<'a>`**：直接在 `View::render` 内链式构建，例如 `Button`、`Switch`、`Card`、`Table`；
- **多视图隔离 `View` + `Handle<V>`**：通过 `engine.insert_view(SubPanel::new())` 注册子视图句柄，在父视图通过 `.child(cx, self.sub_handle)` 引用，子视图拥有独立的布局与重绘隔离边界；
- **细粒度响应式 `Signal<T>`**：适合高频变化的数值、开关或状态枚举，`count.get()` 在 `render` 内自动订阅当前 `PodId`。

---

## 常用组件用法示例

### Button

```rust
use velox_kit::prelude::*;

Button::new(101, "Run Check").primary().small().w(92).h(28).on_click(on_click)
Button::new(102, "Secondary").secondary()
Button::new(103, "Outline").outline()
Button::new(104, "Warning").warning()
Button::new(105, "Ghost").ghost()
Button::new(106, "Disabled").disabled(true)
Button::new(107, "Repeat +1").auto_repeat(true).on_click(on_inc)
```

### TextInput (`EditableText`)

```rust
use velox_kit::prelude::*;

// 在宿主 View 中保留 EditableText<CAP> 与焦点标记：
// pub struct MyView { name: EditableText<64>, name_focused: bool }

let on_focus = cx.action(|this, _acx| {
    this.name_focused = true;
});
let on_key = cx.listener(|this, key: KeyCode, _acx| {
    let _ = this.name.handle_key(key);
});

TextInput::new(201, self.name.as_arena_str(cx), self.name_focused)
    .size(220, 32)
    .placeholder("Enter name...")
    .cursor(self.name.cursor())
    .selection(self.name.selected_range())
    .on_focus(on_focus)
    .on_key_input(on_key)
```

### CodeEditor (`TextBuffer`)

```rust
use velox_kit::prelude::*;

// 宿主 View 字段：pub editor_buf: TextBuffer<1024>, pub editor_focused: bool
let code_str = self.editor_buf.as_arena_str(cx);
let sel = self.editor_buf.selection();
let cur_pt = self.editor_buf.cursor_point();

CodeEditor::new(320, code_str, self.editor_focused)
    .selection(sel, cur_pt)
    .size(0, 0) // 配合父容器尺寸或 .size(w, h)
    .highlights(spans)
    .on_focus(cx.action(|this, _| this.editor_focused = true))
    .on_key_input(cx.listener(|this, key: KeyCode, _| {
        let _ = this.editor_buf.handle_key(key);
    }))
    .on_pointer_select(cx.listener(|this, (pt, is_drag): (BufferPoint, bool), _| {
        this.editor_focused = true;
        if is_drag {
            let anchor = this.editor_buf.offset_to_point(this.editor_buf.selection().anchor_offset());
            this.editor_buf.select_to_point(anchor, pt);
        } else {
            this.editor_buf.set_cursor_point(pt);
        }
    }))
```

### Select

```rust
use velox_kit::prelude::*;

const ROLE_OPTIONS: &[SelectOption<'static>] = &[
    SelectOption::new("Viewer", "viewer"),
    SelectOption::new("Editor", "editor"),
    SelectOption::new("Owner (locked)", "owner").disabled(true),
];

Select::new(220, ROLE_OPTIONS, self.selected_role, self.select_open)
    .width(200)
    .placeholder("Select role...")
    .on_toggle(cx.action(|this, _| {
        this.select_open = !this.select_open;
    }))
    .on_select(cx.listener(|this, idx: usize, _| {
        this.selected_role = Some(idx);
        this.select_open = false;
    }))
```

### Checkbox / Switch / RadioGroup / Slider（支持 `.bind()` 信号双向绑定）

```rust
use velox_kit::prelude::*;

const MODES: &[&str] = &["Fast", "Balanced", "Power Save"];

// 1. 常规受控回调模式（状态存于宿主 View 字段）
Checkbox::new(301, self.remember)
    .label("Remember settings")
    .on_change(cx.listener(|this, next: bool, _| {
        this.remember = next;
    }))

Switch::new(302, self.enabled)
    .small()
    .on_change(cx.listener(|this, next: bool, _| {
        this.enabled = next;
    }))

RadioGroup::new(303, MODES, self.mode_idx)
    .horizontal()
    .on_change(cx.listener(|this, idx: usize, _| {
        this.mode_idx = Some(idx);
    }))

// 2. Signal<T> 零堆双向绑定简写（自动建立细粒度订阅并通过 set_if_changed 回写）
// Switch::bind(cx, 304, self.vsync_sig).small()
// Checkbox::bind(cx, 305, self.auto_save_sig).label("Auto Save")
// Slider::bind(cx, 306, self.volume_sig).range(0, 100)
// Tabs::bind(cx, 307, MODES, self.tab_sig)
```

### Form / FormField

```rust
use velox_kit::prelude::*;

let fields = div()
    .flex_col()
    .gap(10)
    .child(
        cx,
        FormField::new(401, "Project Name")
            .required(true)
            .help("Used as crate identifier")
            .control(cx, TextInput::new(402, self.name.as_arena_str(cx), self.focused)),
    );

Form::new(400, 360)
    .title("Create Project")
    .layout(FormLayout::Vertical)
    .content(cx, fields)
    .on_submit(cx.action(|this, _| {
        this.submitted = true;
    }))
```

### Dialog / AlertDialog / Notification

```rust
use velox_kit::prelude::*;

// 1. 右上角非阻塞浮动通知（搭配 .overlay() 容器）
if self.notification_open {
    root = root.child(
        cx,
        div()
            .id(850)
            .pos(0, 52)
            .w_full()
            .overlay()
            .flex_row()
            .justify_end()
            .pr(16)
            .child(
                cx,
                Notification::new(800, "Build Passed", "All checks succeeded.")
                    .variant(NotificationVariant::Success)
                    .on_dismiss(cx.action(|this, _| this.notification_open = false)),
            ),
    );
}

// 2. 模态 Dialog（外层遮罩必须加 .overlay().occlude_hit() 阻断点击穿透）
if self.dialog_open {
    root = root.child(
        cx,
        div()
            .id(750)
            .pos(0, 0)
            .size_full()
            .overlay()
            .occlude_hit()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(Color::rgba(0, 0, 0, 140))
            .child(
                cx,
                Dialog::new(700, "About Velox Studio", true)
                    .width(380)
                    .description("Cross-Platform Desktop Workbench (MPL-2.0)")
                    .content(cx, text("Built with velox-kit.").size(12).color(cx.theme().muted_foreground))
                    .on_close(cx.action(|this, _| this.dialog_open = false))
                    .footer(
                        cx,
                        Button::new(705, "Close")
                            .primary()
                            .w(84)
                            .h(30)
                            .on_click(cx.listener(|this, _: ClickEvent, _| this.dialog_open = false)),
                    ),
            ),
    );
}
```

### Root 根容器与 Tabs / Sidebar / Table / VirtualList

```rust
const WORKSPACE_TABS: &[&str] = &["Dashboard", "Packages", "Logs"];
const COLS: &[TableColumn<'static>] = &[
    TableColumn::new("Crate", 160),
    TableColumn::new("License", 100),
];
const ROWS: &[&[&str]] = &[
    &["velox-core", "MPL-2.0"],
    &["velox-kit", "MPL-2.0"],
];

Tabs::new(120, WORKSPACE_TABS, self.tab_idx)
    .on_select(cx.listener(|this, idx: usize, _| this.tab_idx = idx))

Table::new(410, COLS, ROWS)
    .selected_row(Some(self.selected_row))
    .on_row_click(cx.listener(|this, row: usize, _| this.selected_row = row))

virtual_list(500, 10_000, 28, 280, self.scroll_y, |idx, cx| {
    text(fmt!(cx, "Row #{}", idx)).size(12).color(cx.theme().foreground).into_element(cx)
})
```

---

## 主题切换与注入

```rust
use velox_kit::prelude::*;

impl View for MyApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = self.theme_mode.to_theme(); // ThemeMode::Dark / Light
        cx.override_theme(theme);               // 本帧内生效，零可变别名风险

        // 若使用 Root 容器，也可以通过 theme_in 自动向子树注入主题：
        // Root::new(1, 960, 640).theme_in(cx, theme).content_with(cx, |cx| { ... })

        div()
            .size_full()
            .bg(theme.background)
            .child(cx, text("Hello Theme").color(cx.theme().foreground))
            .into_element(cx)
    }
}
```

---

## 布局辅助与响应式拉伸

Velox 核心布局基于 24 字节紧凑 `Style` 与 `Two-Pass Axis Solver`（`i16` 四步一维弹性求解）：

- **横/纵流式布局**：`.flex_row()` / `.flex_col()` + `.gap(px)` + `.items_center()` + `.justify_between()`
- **自适应主区分配**：使用 `.flex_grow(1)`（支持 `0..=3` 权重，**注意没有 `.flex_1()`**）搭配 `.w_full()` / `.h_full()` / `.size_full()`
- **网格容器**：`.grid(cols)` 将 `div()` 设为指定列数的二维网格
- **移动安全区避让**：`.safe_area(cx)`、`.pt_safe(cx)`、`.pb_safe(cx)`

---

## 浮层与模态遮罩规范

- **常规浮层**：`Notification`、`Tooltip`、`Dropdown` 弹出层放在 `.pos(x, y).overlay()` 容器内，脱离父裁剪并在最顶层 Z-Order 渲染；
- **阻断型模态遮罩**：`Dialog`、`AlertDialog`、`Modal`、`Sheet`、`BottomSheet` 外层半透明遮罩必须调用 `.overlay().occlude_hit()`，确保底层控件收不到点击与滚动命中；
- `Root` 组件内置 `dialog(...)` 与 `notification(...)` 两个标准浮层槽位，也支持直接在应用根视图手动挂载 `.overlay()` 层。

---

## 共享修饰 Trait

- `Styled`：布局方向、宽高策略、边距、圆角、边框、背景与主/交叉轴对齐；
- `TailwindStyled`（启用 `component` 时，默认开启）：`.tw_card_surface(theme)`、`.tw_button_size(size)`、`.tw_button_variant(theme, variant)`；
- `Sizable`：`.small()` / `.medium()` / `.large()`；
- `Variants`：`.primary()` / `.secondary()` / `.outline()` / `.warning()` / `.ghost()`；
- `Disableable`：`.disabled(bool)`。
