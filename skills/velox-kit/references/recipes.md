# 经过验证的应用完整配方（Tested Application Recipe）

**Contents:** [极简桌面启动配方（Hello World）](#极简桌面启动配方hello-world) · [完整设置面板与浮层视图](#完整设置面板与浮层视图) · [异步任务、RAII 取消与 Tokio + reqwest 配方](#异步任务raii-取消与-tokio--reqwest-配方) · [无头集成测试与虚拟时钟配方](#无头集成测试与虚拟时钟配方) · [桌面启动与离线验证命令](#桌面启动与离线验证命令)

本配方对照仓库内的真实示例（[`examples/hello_world/src/main.rs`](../../../examples/hello_world/src/main.rs)、[`examples/desktop_app/src/{lib.rs,main.rs,async_demo.rs}`](../../../examples/desktop_app/src/async_demo.rs)）编写，只依赖唯一入口 Crate `velox-kit`，完整覆盖：

- 极简桌面应用启动（`velox_kit::application()` / `init(cx)` / `open_window(...)` / `.build_headless(...)`）；
- 宿主 `View` 状态管理（`EditableText<64>`、`ThemeMode`、受控开关与单选组）；
- GPUI 风格双执行器异步任务（`acx.spawn` + `cx.background_spawn` + `Task<T>` RAII 取消 + `velox-kit` `tokio` feature 下的 `reqwest` HTTP 请求）；
- 帧级 `ScratchArena` 零堆分配格式化（`fmt!` 与 `as_arena_str`）与 `const` 静态切片；
- `Notification` 浮动通知层 + `Dialog` 模态遮罩防穿透（`.overlay().occlude_hit()`）；
- 基于 `Application::build_headless`、`VeloxEngine`、`ManualScheduler` 与 `WinitPlatform` 的无头自动化测试及 0% CPU 休眠断言。

---

## 极简桌面启动配方（Hello World）

对应 [`examples/hello_world/src/main.rs`](../../../examples/hello_world/src/main.rs)：

```rust
use velox_kit::prelude::*;

pub struct HelloWorld {
    pub clicks: u32,
}

impl View for HelloWorld {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let on_click = cx.listener(|this, _: ClickEvent, _| {
            this.clicks += 1;
        });

        div()
            .flex_col()
            .gap(12)
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .child(
                cx,
                text(fmt!(cx, "Hello, World! ({})", self.clicks))
                    .size(18)
                    .color(cx.theme().foreground),
            )
            .child(cx, Button::new(1, "Let's Go!").primary().medium().on_click(on_click))
            .into_element(cx)
    }
}

fn main() {
    velox_kit::application().run(|cx| {
        velox_kit::init(cx);
        velox_kit::open_window(
            WindowOptions::new("Hello Velox").size(480, 320),
            cx,
            |cx| cx.new(|_| HelloWorld { clicks: 0 }),
        )
        .expect("Failed to open window");
    });
}
```

---

## 完整设置面板与浮层视图

```rust
use velox_kit::prelude::*;

const DELIVERY_MODES: &[&str] = &["Immediately", "Daily summary"];

/// 独立可运行的桌面设置面板主视图。
pub struct SettingsWorkbench {
    pub name: EditableText<64>,
    pub name_focused: bool,
    pub remember: bool,
    pub notifications_enabled: bool,
    pub delivery_index: Option<usize>,
    pub theme_mode: ThemeMode,
    pub save_count: u32,
    pub notification_open: bool,
    pub about_dialog_open: bool,
}

impl Default for SettingsWorkbench {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsWorkbench {
    pub fn new() -> Self {
        Self {
            name: EditableText::from_str("Velox"),
            name_focused: false,
            remember: true,
            notifications_enabled: true,
            delivery_index: Some(0),
            theme_mode: ThemeMode::Dark,
            save_count: 0,
            notification_open: false,
            about_dialog_open: false,
        }
    }
}

impl View for SettingsWorkbench {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = self.theme_mode.to_theme();
        cx.override_theme(theme);

        // 1. 事件与动作回调（全部分配在帧级 ScratchArena 中，默认自动标记当前 PodId 为脏）
        let on_toggle_theme = cx.listener(|this, _: ClickEvent, _acx| {
            this.theme_mode = this.theme_mode.toggle();
        });
        let on_open_about = cx.listener(|this, _: ClickEvent, _acx| {
            this.about_dialog_open = true;
        });
        let on_close_about_btn = cx.listener(|this, _: ClickEvent, _acx| {
            this.about_dialog_open = false;
        });
        let raw_close_about = cx.action(|this, _acx| {
            this.about_dialog_open = false;
        });
        let raw_dismiss_notif = cx.action(|this, _acx| {
            this.notification_open = false;
        });
        let raw_focus_name = cx.action(|this, _acx| {
            this.name_focused = true;
        });
        let on_name_key = cx.listener(|this, key: KeyCode, _acx| {
            let _ = this.name.handle_key(key);
        });
        let on_remember = cx.listener(|this, next: bool, _acx| {
            this.remember = next;
        });
        let on_notif_switch = cx.listener(|this, next: bool, _acx| {
            this.notifications_enabled = next;
        });
        let on_delivery = cx.listener(|this, idx: usize, _acx| {
            this.delivery_index = Some(idx);
        });
        let raw_submit_form = cx.action(|this, _acx| {
            this.save_count = this.save_count.saturating_add(1);
            this.notification_open = true;
        });

        let name_str = self.name.as_arena_str(cx);

        // 2. 顶部栏 (h = 44)
        let header = div()
            .id(10)
            .flex_row()
            .w_full()
            .h(44)
            .px(16)
            .items_center()
            .justify_between()
            .bg(theme.card)
            .border(1, theme.border)
            .child(cx, text("Velox Settings Workbench").size(15).color(theme.foreground))
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(8)
                    .items_center()
                    .child(
                        cx,
                        Button::new(11, if self.theme_mode.is_dark() { "Light Mode" } else { "Dark Mode" })
                            .secondary()
                            .small()
                            .w(96)
                            .h(28)
                            .on_click(on_toggle_theme),
                    )
                    .child(
                        cx,
                        Button::new(12, "About")
                            .outline()
                            .small()
                            .w(72)
                            .h(28)
                            .on_click(on_open_about),
                    ),
            );

        // 3. 表单主体字段
        let form_fields = div()
            .flex_col()
            .w_full()
            .gap(12)
            .child(
                cx,
                FormField::new(101, "Profile Name")
                    .required(true)
                    .help(fmt!(cx, "Preview: {} (saves: {})", name_str, self.save_count).as_str())
                    .control(
                        cx,
                        TextInput::new(102, name_str, self.name_focused)
                            .size(280, 32)
                            .placeholder("Enter display name")
                            .cursor(self.name.cursor())
                            .selection(self.name.selected_range())
                            .on_focus(raw_focus_name)
                            .on_key_input(on_name_key),
                    ),
            )
            .child(
                cx,
                Checkbox::new(103, self.remember)
                    .label("Remember profile across sessions")
                    .on_change(on_remember),
            )
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(cx, text("Enable desktop notifications").size(13).color(theme.foreground))
                    .child(cx, Switch::new(104, self.notifications_enabled).on_change(on_notif_switch)),
            )
            .child(
                cx,
                FormField::new(105, "Digest Delivery").control(
                    cx,
                    RadioGroup::new(106, DELIVERY_MODES, self.delivery_index)
                        .horizontal()
                        .on_change(on_delivery),
                ),
            );

        let body = div()
            .id(20)
            .flex_col()
            .w_full()
            .flex_grow(1)
            .p(20)
            .items_center()
            .child(
                cx,
                Form::new(100, 480)
                    .title("Preferences")
                    .layout(FormLayout::Vertical)
                    .content(cx, form_fields)
                    .on_submit(raw_submit_form),
            );

        let mut root = div()
            .id(1)
            .size_full()
            .flex_col()
            .bg(theme.background)
            .child(cx, header)
            .child(cx, body);

        // 4. 非阻塞浮动通知层
        if self.notification_open {
            root = root.child(
                cx,
                div()
                    .id(200)
                    .pos(0, 52)
                    .w_full()
                    .overlay()
                    .flex_row()
                    .justify_end()
                    .pr(16)
                    .child(
                        cx,
                        Notification::new(201, "Settings Saved", "Profile preferences updated.")
                            .variant(NotificationVariant::Success)
                            .on_dismiss(raw_dismiss_notif),
                    ),
            );
        }

        // 5. 顶层模态 Dialog 遮罩层（必须同时挂载 .overlay().occlude_hit()）
        if self.about_dialog_open {
            root = root.child(
                cx,
                div()
                    .id(300)
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
                        Dialog::new(301, "About Velox Workbench", true)
                            .width(360)
                            .description("MPL-2.0 Cross-Platform Rust GUI Framework")
                            .content(
                                cx,
                                text("Zero global heap allocations during frame render.")
                                    .size(12)
                                    .color(theme.muted_foreground),
                            )
                            .on_close(raw_close_about)
                            .footer(
                                cx,
                                Button::new(302, "Close")
                                    .primary()
                                    .w(84)
                                    .h(30)
                                    .on_click(on_close_about_btn),
                            ),
                    ),
            );
        }

        root.into_element(cx)
    }
}
```

---

## 异步任务、RAII 取消与 Tokio + reqwest 配方

开启 `velox-kit = { ..., features = ["tokio"] }` 并在启动时链式调用 `velox_kit::application().with_tokio().run(...)` 后，可在任意 `View` 事件回调中直接发起多阶段异步计算或真实 `reqwest` HTTP 请求（完整演示见 [`examples/desktop_app/src/async_demo.rs`](../../../examples/desktop_app/src/async_demo.rs)）：

```rust
use velox_kit::prelude::*;

pub struct AsyncFetchPanel {
    pub busy: bool,
    pub summary: String,
    pub active_task: Option<Task<()>>,
}

impl AsyncFetchPanel {
    pub fn new() -> Self {
        Self {
            busy: false,
            summary: String::from("Idle"),
            active_task: None,
        }
    }

    fn start_http_get(&mut self, acx: &mut ActionScope, url: String) {
        self.busy = true;
        self.summary = format!("GET {url}...");

        // 覆盖 self.active_task 会先 Drop 旧 Task，自动取消上一轮未完成请求
        self.active_task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| {
            let outcome = cx
                .background_spawn(async move {
                    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
                    let code = resp.status().as_u16();
                    let body = resp.text().await.map_err(|e| e.to_string())?;
                    Ok::<(u16, usize), String>((code, body.len()))
                })
                .await;

            // 回到 UI 主线程更新视图（带 ABA generation 校验并自动标脏）
            let _ = this.update(&mut cx, |view, _| {
                view.busy = false;
                view.summary = match outcome {
                    Ok((code, bytes)) => format!("HTTP {code} ({bytes} bytes)"),
                    Err(err) => format!("Error: {err}"),
                };
                view.active_task = None;
            });
        }));
    }
}

impl View for AsyncFetchPanel {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = cx.theme();
        let on_fetch = cx.listener(|this, _: ClickEvent, acx| {
            this.start_http_get(acx, String::from("https://httpbin.org/get"));
        });
        let on_cancel = cx.listener(|this, _: ClickEvent, _| {
            this.active_task = None; // Drop 即刻取消进行中的协程与请求
            this.busy = false;
            this.summary = String::from("Cancelled");
        });

        // 仅在 busy 期间请求连续帧驱动 Spinner，完成后自动恢复 0% CPU Wait
        cx.keep_animating(self.busy);

        div()
            .id(400)
            .w_full()
            .h(96)
            .p(12)
            .flex_col()
            .gap(8)
            .bg(theme.card)
            .border(1, theme.border)
            .rounded(8)
            .child(cx, text(cx.arena().alloc_str(&self.summary)).size(12).color(theme.foreground))
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(8)
                    .child(cx, Button::new(401, "HTTP GET").primary().small().disabled(self.busy).on_click(on_fetch))
                    .child(cx, Button::new(402, "Cancel").outline().small().disabled(!self.busy).on_click(on_cancel)),
            )
            .into_element(cx)
    }
}
```

---

## 无头集成测试与虚拟时钟配方

直接测试生产视图（不另写测试替身视图），验证 `build_headless`、首次绘制、空闲休眠（0% CPU）、聚焦键盘输入、模态遮罩拦截与 `ManualScheduler` 零真睡异步步进：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use velox_kit::platform::winit::WinitPlatform;
    use velox_kit::platform::{Platform, PlatformControlFlow};

    #[test]
    fn settings_workbench_flow_and_modal_occlusion() {
        let mut root_opt = None;
        let mut built = velox_kit::application()
            .build_headless(|cx| {
                velox_kit::init(cx);
                let win = velox_kit::open_window(
                    WindowOptions::new("Settings Test").size(800, 560),
                    cx,
                    |cx| cx.new(|_| SettingsWorkbench::new()),
                )
                .expect("open window");
                root_opt = Some(win.root());
            })
            .expect("built window");
        let root = root_opt.unwrap();
        let mut plat = WinitPlatform::new("Settings Test", 800, 560).with_dpi_scale(1);

        // 首帧触发绘制并回到 Wait 空闲休眠（0% CPU）
        assert_eq!(plat.step_platform(&mut built.engine, 16), PlatformControlFlow::Poll);
        assert_eq!(plat.step_platform(&mut built.engine, 16), PlatformControlFlow::Wait);

        // 模拟程序聚焦并键入字符
        built.engine.view_mut(root).unwrap().name_focused = true;
        built.engine.view_mut(root).unwrap().name.insert_str("Kit");
        let _ = built.engine.step_frame();
        assert_eq!(built.engine.view(root).unwrap().name.as_str(), "VeloxKit");

        // 打开模态 Dialog 后，任意底层区域点击均应被 .occlude_hit() 拦截
        built.engine.view_mut(root).unwrap().about_dialog_open = true;
        let _ = built.engine.step_frame();
        assert!(!built.engine.dispatch_click(200, 120));
    }

    #[test]
    fn manual_scheduler_deterministic_async_step() {
        let sched = ManualScheduler::new();
        let mut engine = VeloxEngine::new(640, 400).with_scheduler(sched.clone());
        let root = engine.mount_root(SettingsWorkbench::new());

        let task = engine.spawn_for(root, async move |this: Handle<SettingsWorkbench>, mut cx: AsyncApp| {
            cx.timer(Duration::from_secs(5)).await;
            let _ = this.update(&mut cx, |view, _| view.save_count += 1);
        });

        sched.drive_engine(&mut engine);
        assert_eq!(engine.view(root).unwrap().save_count, 0);

        // 零真睡快进 5 秒虚拟时钟
        sched.advance(Duration::from_secs(5));
        sched.drive_engine(&mut engine);
        assert_eq!(engine.view(root).unwrap().save_count, 1);
        drop(task);
    }
}
```

---

## 桌面启动与离线验证命令

在 `velox` 仓库工作区中，参考真实示例 [`examples/hello_world`](../../../examples/hello_world/src/main.rs) 与 [`examples/desktop_app`](../../../examples/desktop_app/src/lib.rs) 执行以下命令验证：

```powershell
# 运行极简 Hello World 示例测试
cargo test -p hello_world

# 运行桌面画廊端到端集成测试（含 Tokio + reqwest 回环、AsyncPlatform 与 PointerKind 验证）
cargo test -p desktop_app

# 运行可插拔异步适配层（tokio + smol）测试
cargo test -p velox-async --all-features

# 运行全工作区测试
cargo test --workspace

# 运行桌面示例（默认启用 wgpu 2D SDF GPU 加速 + Tokio 调度器）
cargo run -p desktop_app -- --headless
cargo run -p desktop_app -- --async-demo
cargo run -p desktop_app -- --save-png target/desktop_app_preview.png

# 运行时切换内置软件光栅渲染器或原生零依赖调度器
cargo run -p desktop_app -- --headless --software --native-scheduler
```
