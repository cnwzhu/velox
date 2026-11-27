//! `desktop_app` 可插拔调度器（[`crate::TokioScheduler`]）+ `reqwest` HTTP 请求 + `AsyncPlatform` 示例模块。
//!
//! 完整演示 Velox 的可插拔异步运行时架构：
//! 1. **UI 前景 + Tokio 后台双执行器工作流（[`AsyncTaskView`]）**：
//!    - 按钮 `"Run Async Sync"`：通过 `acx.spawn` + `cx.background_spawn` + `cx.timer` 执行 3 阶段异步流水线，
//!      零代码改动即可在 [`crate::TokioScheduler`]、[`NativeStdScheduler`] 与 [`ManualScheduler`] 之间自由切换；
//!    - 按钮 `"HTTP GET"`：通过 `acx.spawn` + `cx.background_spawn` 直接在 Tokio 运行时上调用 `reqwest::Client::get(...)`
//!      发起真实异步 HTTP 请求，并将状态码、字节数与耗时安全回写到视图和线程局部 `Signal<T>`；
//!    - 按钮 `"Cancel Task"`：仅需 `self.sync_task = None` 触发 RAII `Drop` 自动取消（含飞行中的 `reqwest` 请求）；
//!    - 封装为固定高度（`126px`）独立子 `View`，全程命中 **L2 原地局部重排**（根视图 0 次重跑）并回归 **0% CPU `Wait` 休眠**。
//! 2. **异步平台驱动循环（[`HeadlessAsyncPlatform`] + [`run_async_platform_demo`] + [`run_tokio_http_demo`]）**：
//!    - 使用框架全平台通用的 [`run_async_event_loop`] 与 [`TokioScheduler`] / [`StdAsyncDriver`] 驱动异步帧循环。

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::i18n::DESKTOP_CATALOG;
use crate::reactive::GallerySignals;
use crate::stories::state_pill;
use velox_kit::component::{Badge, Button, ProgressBar, Spinner};
use velox_kit::platform::shm_driver::{SoftwareSurfaceDriver, WlSurface};
use velox_kit::prelude::*;

/// 默认用于桌面窗口手动点击 `"HTTP GET"` 按钮测试的接口地址（可通过 `--http-url <URL>` 覆盖）。
pub const DEFAULT_HTTP_TEST_URL: &str = "https://httpbin.org/get";

/// 进程级共享复用连接池的 `reqwest::Client`。
pub fn shared_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .user_agent("Velox-DesktopApp/0.1")
            .build()
            .expect("failed to build reqwest::Client")
    })
}

// ============================================================================
// 1. 异步任务 UI 状态机与独立隔离边界子视图 `AsyncTaskView`
// ============================================================================

/// 异步任务在 UI 侧的可观测状态机（覆盖多阶段同步与真实 `reqwest` HTTP 请求）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AsyncTaskStatus {
    Idle,
    Running {
        progress_pct: u8,
        stage_key: &'static str,
    },
    HttpPending {
        url: String,
    },
    HttpOk {
        status_code: u16,
        bytes: usize,
        elapsed_ms: u32,
        preview: String,
    },
    HttpErr {
        elapsed_ms: u32,
        error: String,
    },
    Completed {
        elapsed_ms: u32,
        crates_synced: u8,
    },
    Cancelled,
}

impl AsyncTaskStatus {
    /// 当前是否处于活跃异步任务执行态。
    #[inline]
    pub const fn is_busy(&self) -> bool {
        matches!(self, Self::Running { .. } | Self::HttpPending { .. })
    }
}

/// 独立固定高度（`126px`）的异步任务与 `reqwest` HTTP 测试子视图：
/// - 点击 `"Run Async Sync"` 触发 3 阶段后台异步同步流水线；
/// - 点击 `"HTTP GET"` 经由 [`TokioScheduler`] 发起真实 `reqwest::Client::get(...)` 异步请求；
/// - 点击 `"Cancel Task"` 仅需 `this.sync_task = None` 通过 RAII `Drop` 立即取消飞行中的协程与 HTTP 连接；
/// - 固定外框隔离，全程命中 **L2 原地局部重排**，根视图 `GalleryApp` 0 次重跑。
pub struct AsyncTaskView {
    pub signals: GallerySignals,
    pub status: AsyncTaskStatus,
    pub completed_runs: u32,
    pub http_requests_ok: u32,
    http_url: String,
    scheduler_label: &'static str,
    step_delay: Duration,
    /// RAII 前景任务句柄：覆盖赋值、置 `None` 或视图卸载时自动取消运行中的协程与后台任务。
    sync_task: Option<Task<()>>,
}

impl AsyncTaskView {
    /// 创建异步任务演示子视图（默认每阶段异步等待 `180ms`）。
    pub fn new(signals: GallerySignals) -> Self {
        Self {
            signals,
            status: AsyncTaskStatus::Idle,
            completed_runs: 0,
            http_requests_ok: 0,
            http_url: String::from(DEFAULT_HTTP_TEST_URL),
            scheduler_label: "tokio",
            step_delay: Duration::from_millis(180),
            sync_task: None,
        }
    }

    /// 设置底部状态胶囊展示的调度器名称标签（通常传入 `engine.scheduler_name()`）。
    #[inline]
    pub fn with_scheduler_label(mut self, label: &'static str) -> Self {
        self.scheduler_label = label;
        self
    }

    /// 自定义每个异步阶段的延时（便于无头自动化测试快速完成异步闭环）。
    #[inline]
    pub fn set_step_delay(&mut self, delay: Duration) {
        self.step_delay = delay;
    }

    /// 自定义 `"HTTP GET"` 按钮请求的目标 URL（支持公网 HTTPS 或本地回环测试服务）。
    #[inline]
    pub fn set_http_url(&mut self, url: impl Into<String>) {
        self.http_url = url.into();
    }

    /// 获取当前配置的 HTTP 测试目标 URL。
    #[inline]
    pub fn http_url(&self) -> &str {
        self.http_url.as_str()
    }

    /// 启动新一轮原生 `acx.spawn` + `cx.background_spawn` 多阶段异步流水线。
    ///
    /// 若存在尚未结束的旧任务，覆盖 `self.sync_task` 时将自动触发旧 [`Task`] 的 RAII `Drop` 取消。
    pub fn start_sync(&mut self, acx: &mut ActionScope) {
        self.status = AsyncTaskStatus::Running {
            progress_pct: 10,
            stage_key: "async.stage_spawn",
        };
        let delay = self.step_delay;

        self.sync_task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| {
            const STAGES: &[(u8, &str, u8)] = &[
                (25, "async.stage_manifest", 1),
                (55, "async.stage_checksum", 2),
                (85, "async.stage_shader", 4),
            ];
            let started = Instant::now();
            let mut synced = 0u8;

            for &(pct, stage_key, batch_crates) in STAGES {
                if this
                    .update(&mut cx, |view, _| {
                        view.status = AsyncTaskStatus::Running {
                            progress_pct: pct,
                            stage_key,
                        };
                    })
                    .is_none()
                {
                    return;
                }
                // 通过当前引擎配置的后台调度器与时钟执行，彻底解耦具体运行时（支持 Tokio / Native / Manual）
                let stage_timer = cx.timer(delay);
                synced = cx
                    .background_spawn(async move {
                        stage_timer.await;
                        batch_crates
                    })
                    .await;
            }

            let elapsed_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
            let _ = this.update(&mut cx, |view, _| {
                view.status = AsyncTaskStatus::Completed {
                    elapsed_ms,
                    crates_synced: synced,
                };
                view.completed_runs = view.completed_runs.saturating_add(1);
                view.sync_task = None;
                view.signals.event_count.update(|n| *n = n.saturating_add(1));
            });
        }));
    }

    /// 经由当前引擎配置的 [`TokioScheduler`] 发起真实 `reqwest::Client::get(...)` 异步 HTTP 请求。
    pub fn start_http_get(&mut self, acx: &mut ActionScope) {
        let url = self.http_url.clone();
        self.status = AsyncTaskStatus::HttpPending { url: url.clone() };

        self.sync_task = Some(acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| {
            let started = Instant::now();
            // 后台任务由 TokioScheduler::spawn_background 派发至 Tokio 多线程运行时执行，原生支持 reqwest
            let outcome: Result<(u16, usize, String), String> = cx
                .background_spawn(async move {
                    let resp = shared_http_client()
                        .get(&url)
                        .send()
                        .await
                        .map_err(|e| summarize_error(&e.to_string()))?;
                    let code = resp.status().as_u16();
                    let body = resp
                        .bytes()
                        .await
                        .map_err(|e| summarize_error(&e.to_string()))?;
                    let len = body.len();
                    let raw_text = String::from_utf8_lossy(&body);
                    let preview = summarize_preview(&raw_text, 40);
                    Ok((code, len, preview))
                })
                .await;

            let elapsed_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
            let _ = this.update(&mut cx, |view, _| {
                match outcome {
                    Ok((status_code, bytes, preview)) => {
                        view.status = AsyncTaskStatus::HttpOk {
                            status_code,
                            bytes,
                            elapsed_ms,
                            preview,
                        };
                        view.http_requests_ok = view.http_requests_ok.saturating_add(1);
                    }
                    Err(error) => {
                        view.status = AsyncTaskStatus::HttpErr { elapsed_ms, error };
                    }
                }
                view.sync_task = None;
                view.signals.event_count.update(|n| *n = n.saturating_add(1));
            });
        }));
    }

    /// 通过 RAII Drop 立即取消当前运行中的异步同步或飞行中的 `reqwest` HTTP 请求。
    pub fn cancel_sync(&mut self) {
        self.sync_task = None;
        if self.status.is_busy() {
            self.status = AsyncTaskStatus::Cancelled;
        }
    }
}

fn summarize_preview(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for ch in text.chars() {
        let c = if ch.is_whitespace() { ' ' } else { ch };
        if c == ' ' && last_space {
            continue;
        }
        last_space = c == ' ';
        if out.chars().count() >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(c);
    }
    out.trim().to_string()
}

fn summarize_error(err: &str) -> String {
    summarize_preview(err, 56)
}

impl View for AsyncTaskView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = self.signals.theme_mode.get().to_theme();
        let locale = self.signals.locale.get();
        cx.override_theme(theme);
        cx.override_locale(locale);
        cx.override_i18n_catalog(&DESKTOP_CATALOG);

        let is_busy = self.status.is_busy();
        let progress_pct: u8 = match &self.status {
            AsyncTaskStatus::Idle => 0,
            AsyncTaskStatus::Running { progress_pct, .. } => *progress_pct,
            AsyncTaskStatus::HttpPending { .. } => 60,
            AsyncTaskStatus::HttpOk { .. } | AsyncTaskStatus::Completed { .. } => 100,
            AsyncTaskStatus::HttpErr { .. } | AsyncTaskStatus::Cancelled => 0,
        };

        let on_run = cx.listener(|this, _: ClickEvent, acx| {
            this.start_sync(acx);
            this.signals.event_count.update(|n| *n = n.saturating_add(1));
        });
        let on_http = cx.listener(|this, _: ClickEvent, acx| {
            this.start_http_get(acx);
            this.signals.event_count.update(|n| *n = n.saturating_add(1));
        });
        let on_cancel = cx.listener(|this, _: ClickEvent, _acx| {
            this.cancel_sync();
            this.signals.event_count.update(|n| *n = n.saturating_add(1));
        });

        let status_badge = match &self.status {
            AsyncTaskStatus::Idle => Badge::new(cx.t("async.badge_idle")).secondary(),
            AsyncTaskStatus::Running { progress_pct, .. } => {
                Badge::new(fmt!(cx, "{} {}%", cx.t("async.badge_running"), progress_pct)).info()
            }
            AsyncTaskStatus::HttpPending { .. } => Badge::new(cx.t("async.badge_http_req")).info(),
            AsyncTaskStatus::HttpOk { status_code, .. } => {
                Badge::new(fmt!(cx, "{} {}", cx.t("async.badge_http_ok"), status_code)).success()
            }
            AsyncTaskStatus::HttpErr { .. } => Badge::new(cx.t("async.badge_http_err")).danger(),
            AsyncTaskStatus::Completed { .. } => {
                Badge::new(fmt!(cx, "{} #{}", cx.t("async.badge_done"), self.completed_runs))
                    .success()
            }
            AsyncTaskStatus::Cancelled => Badge::new(cx.t("async.badge_cancelled")).warning(),
        };

        let detail_text = match &self.status {
            AsyncTaskStatus::Idle => cx.t("async.hint_idle"),
            AsyncTaskStatus::Running { stage_key, .. } => cx.t(stage_key),
            AsyncTaskStatus::HttpPending { url } => {
                cx.t_args("async.stage_http", &[("url", url.as_str().into())])
            }
            AsyncTaskStatus::HttpOk {
                status_code,
                bytes,
                elapsed_ms,
                preview,
            } => cx.t_args(
                "async.summary_http_ok",
                &[
                    ("code", (*status_code as u32).into()),
                    ("bytes", (*bytes as u32).into()),
                    ("ms", (*elapsed_ms).into()),
                    ("preview", preview.as_str().into()),
                ],
            ),
            AsyncTaskStatus::HttpErr { elapsed_ms, error } => cx.t_args(
                "async.summary_http_err",
                &[
                    ("ms", (*elapsed_ms).into()),
                    ("err", error.as_str().into()),
                ],
            ),
            AsyncTaskStatus::Completed {
                elapsed_ms,
                crates_synced,
            } => cx.t_args(
                "async.summary_done",
                &[
                    ("ms", (*elapsed_ms).into()),
                    ("crates", (*crates_synced as u32).into()),
                ],
            ),
            AsyncTaskStatus::Cancelled => cx.t("async.summary_cancelled"),
        };

        let mut footer_row = div().flex_row().w_full().gap(8).items_center().justify_between();
        let mut status_left = div().flex_row().gap(8).items_center();
        if is_busy {
            status_left = status_left.child(
                cx,
                Spinner::new(354, 24, 0)
                    .thickness(3)
                    .color(theme.foreground)
                    .spinning(cx, 900),
            );
        } else {
            let icon_color = match &self.status {
                AsyncTaskStatus::Completed { .. } | AsyncTaskStatus::HttpOk { .. } => theme.success,
                AsyncTaskStatus::HttpErr { .. } => theme.danger,
                AsyncTaskStatus::Cancelled => theme.warning,
                _ => theme.muted_foreground,
            };
            status_left = status_left.child(
                cx,
                svg_icon(14, 14, "M13 2 3 14h9l-1 8 10-12h-9l1-8Z").tint(icon_color),
            );
        }
        status_left = status_left.child(cx, text(detail_text).size(12).color(theme.muted_foreground));

        let pill_text = match &self.status {
            AsyncTaskStatus::HttpPending { .. } => {
                fmt!(cx, "{} • reqwest GET", self.scheduler_label)
            }
            AsyncTaskStatus::Running { .. } => {
                fmt!(cx, "{} • Running", self.scheduler_label)
            }
            _ => fmt!(cx, "{} • 0% CPU Wait", self.scheduler_label),
        };
        let idle_pill = state_pill(cx, theme, pill_text);
        footer_row = footer_row.child(cx, status_left).child(cx, idle_pill);

        div()
            .id(350)
            .flex_col()
            .w_full()
            .h(126)
            .p(16)
            .gap(10)
            .rounded(8)
            .bg(theme.card)
            .border(1, theme.border)
            .justify_between()
            .child(
                cx,
                div()
                    .flex_row()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(8)
                            .items_center()
                            .child(
                                cx,
                                svg_icon(
                                    14,
                                    14,
                                    "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8M21 3v5h-5",
                                )
                                .tint(theme.foreground),
                            )
                            .child(cx, text(cx.t("async.title")).size(13).color(theme.foreground))
                            .child(cx, status_badge),
                    )
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(6)
                            .items_center()
                            .child(
                                cx,
                                Button::new(352, cx.t("async.btn_cancel"))
                                    .outline()
                                    .small()
                                    .font_size(12)
                                    .h(28)
                                    .px(10)
                                    .disabled(!is_busy)
                                    .on_click(on_cancel),
                            )
                            .child(
                                cx,
                                Button::new(355, cx.t("async.btn_http"))
                                    .outline()
                                    .small()
                                    .font_size(12)
                                    .h(28)
                                    .px(10)
                                    .disabled(is_busy)
                                    .on_click(on_http),
                            )
                            .child(
                                cx,
                                Button::new(351, cx.t("async.btn_run"))
                                    .primary()
                                    .small()
                                    .font_size(12)
                                    .h(28)
                                    .px(10)
                                    .disabled(is_busy)
                                    .on_click(on_run),
                            ),
                    ),
            )
            .child(
                cx,
                ProgressBar::new(353, 720, progress_pct)
                    .height(8)
                    .animated(true),
            )
            .child(cx, footer_row)
            .into_element(cx)
    }
}

// ============================================================================
// 2. `AsyncPlatform` / `AsyncPlatformSurface` 异步帧驱动器与 Tokio + reqwest 演示
// ============================================================================

/// 示范如何实现 [`AsyncPlatformSurface`] 与 [`AsyncPlatform`]：
/// 以非阻塞 `.await` 驱动 [`VeloxEngine`] 帧刷新与空闲让出。
pub struct HeadlessAsyncPlatform {
    width: i16,
    height: i16,
    started_at: Instant,
    driver: SoftwareSurfaceDriver,
    wl_surface: WlSurface,
}

impl HeadlessAsyncPlatform {
    pub fn new(width: i16, height: i16) -> Self {
        let w = width.max(1) as usize;
        let h = height.max(1) as usize;
        Self {
            width: width.max(1),
            height: height.max(1),
            started_at: Instant::now(),
            driver: SoftwareSurfaceDriver::new(w, h)
                .with_backend(DesktopRenderBackend::Software),
            wl_surface: WlSurface::new(),
        }
    }
}

impl AsyncPlatformSurface for HeadlessAsyncPlatform {
    #[inline]
    fn surface_size(&self) -> (i16, i16) {
        (self.width, self.height)
    }

    async fn flush_frame_dt_async(
        &mut self,
        engine: &mut VeloxEngine,
        dt_ms: u16,
    ) -> FlushReport {
        if let Some(old_id) = self.wl_surface.attached_buffer_id {
            self.driver.release_buffer(old_id);
        }
        let updated = self
            .driver
            .flush_frame_dt(engine, &mut self.wl_surface, dt_ms);
        if updated {
            let rects = self.wl_surface.committed_damage.len();
            let px: usize = self
                .wl_surface
                .committed_damage
                .iter()
                .map(|r| (r.w.max(0) as usize) * (r.h.max(0) as usize))
                .sum();
            FlushReport::with_engine(rects, 1, px, engine)
        } else {
            FlushReport::default()
        }
    }
}

impl AsyncPlatform for HeadlessAsyncPlatform {
    #[inline]
    fn duration_since_start(&self) -> Duration {
        self.started_at.elapsed()
    }

    async fn step_platform_async(
        &mut self,
        engine: &mut VeloxEngine,
        dt_ms: u16,
    ) -> PlatformControlFlow {
        let report = self.flush_frame_dt_async(engine, dt_ms).await;
        if report.has_updates() || engine.needs_redraw() {
            PlatformControlFlow::Poll
        } else {
            PlatformControlFlow::Wait
        }
    }
}

/// 无头异步运行统计结果（供 `--headless` / `--async-demo` 与集成测试验证）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AsyncDemoReport {
    pub frames_polled: u32,
    pub completed_runs: u32,
    pub root_render_count: u32,
    pub final_control_flow: PlatformControlFlow,
}

/// 本地回环 Tokio + `reqwest` HTTP 端到端测试报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpDemoReport {
    pub scheduler_name: &'static str,
    pub status_code: u16,
    pub bytes: usize,
    pub elapsed_ms: u32,
    pub preview: String,
    pub root_render_count: u32,
    pub final_control_flow: PlatformControlFlow,
}

/// 在 [`TokioScheduler`] 上启动一个单次本地回环 HTTP/1.1 测试服务端，返回监听地址 URL。
pub fn spawn_loopback_http_server(
    tokio_sched: &TokioScheduler,
    body: &'static str,
    delay: Duration,
) -> String {
    let listener = tokio_sched
        .handle()
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind loopback TcpListener");
    let addr = listener.local_addr().expect("local_addr");
    tokio_sched.handle().spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut req_buf = [0u8; 1024];
            let _ = socket.read(&mut req_buf).await;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    format!("http://{addr}/velox-test")
}

/// 端到端异步演示函数：挂载 [`crate::GalleryApp`]（默认 [`TokioScheduler`]）、点击 `"Run Async Sync"`、
/// 通过框架原生 [`run_async_event_loop`] 推进直至完成并回归 `PlatformControlFlow::Wait`。
pub async fn run_async_platform_demo(step_delay: Duration) -> AsyncDemoReport {
    let (mut engine, root) = crate::create_desktop_app(
        crate::DEFAULT_DESKTOP_WIDTH,
        crate::DEFAULT_DESKTOP_HEIGHT,
    );
    let async_handle = engine
        .view(root)
        .and_then(|v| v.async_task_view)
        .expect("AsyncTaskView must be mounted by mount_desktop_app");
    engine
        .view_mut(async_handle)
        .unwrap()
        .set_step_delay(step_delay);

    let mut plat = HeadlessAsyncPlatform::new(
        crate::DEFAULT_DESKTOP_WIDTH,
        crate::DEFAULT_DESKTOP_HEIGHT,
    );

    let driver = TokioScheduler::global();
    engine.set_async_waker(driver.wake_signal().notifier());
    let _ = plat.step_platform_async(&mut engine, 16).await;

    let run_btn = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(351))
        .expect("Run Async Sync button id=351 must be rendered")
        .bounds;
    let clicked = engine.dispatch_click(run_btn.x + run_btn.w / 2, run_btn.y + run_btn.h / 2);
    assert!(clicked, "expected click on Run Async Sync button");

    let loop_report = run_async_event_loop(
        &mut plat,
        &mut engine,
        driver.as_ref(),
        AsyncLoopConfig::new(Duration::from_millis(8))
            .with_max_steps(200)
            .with_exit_on_idle(true),
    )
    .await;

    let completed_runs = engine.view(async_handle).unwrap().completed_runs;
    let root_render_count = engine.pod_render_count(root);
    AsyncDemoReport {
        frames_polled: loop_report.steps,
        completed_runs,
        root_render_count,
        final_control_flow: loop_report
            .last_control_flow
            .unwrap_or(PlatformControlFlow::Wait),
    }
}

/// 端到端 Tokio + `reqwest` 本地回环 HTTP GET 演示（零外网依赖、100% 可复现）。
pub async fn run_tokio_http_demo() -> HttpDemoReport {
    let driver = TokioScheduler::global();
    let loopback_url = spawn_loopback_http_server(
        &driver,
        r#"{"framework":"velox","scheduler":"tokio","client":"reqwest","ok":true}"#,
        Duration::from_millis(12),
    );

    let (mut engine, root) = crate::create_desktop_app(
        crate::DEFAULT_DESKTOP_WIDTH,
        crate::DEFAULT_DESKTOP_HEIGHT,
    );
    let async_handle = engine
        .view(root)
        .and_then(|v| v.async_task_view)
        .expect("AsyncTaskView must be mounted");
    engine
        .view_mut(async_handle)
        .unwrap()
        .set_http_url(loopback_url);

    let mut plat = HeadlessAsyncPlatform::new(
        crate::DEFAULT_DESKTOP_WIDTH,
        crate::DEFAULT_DESKTOP_HEIGHT,
    );
    engine.set_async_waker(driver.wake_signal().notifier());
    let _ = plat.step_platform_async(&mut engine, 16).await;

    // 真实点击 "HTTP GET" 按钮（id = 355）触发 Tokio + reqwest 请求
    let http_btn = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(355))
        .expect("HTTP GET button id=355 must be rendered")
        .bounds;
    assert!(engine.dispatch_click(http_btn.x + http_btn.w / 2, http_btn.y + http_btn.h / 2));

    let loop_report = run_async_event_loop(
        &mut plat,
        &mut engine,
        driver.as_ref(),
        AsyncLoopConfig::new(Duration::from_millis(8))
            .with_max_steps(200)
            .with_exit_on_idle(true),
    )
    .await;

    let (status_code, bytes, elapsed_ms, preview) = match &engine.view(async_handle).unwrap().status
    {
        AsyncTaskStatus::HttpOk {
            status_code,
            bytes,
            elapsed_ms,
            preview,
        } => (*status_code, *bytes, *elapsed_ms, preview.clone()),
        other => panic!("expected HttpOk after loopback reqwest GET, got {other:?}"),
    };

    HttpDemoReport {
        scheduler_name: engine.scheduler_name(),
        status_code,
        bytes,
        elapsed_ms,
        preview,
        root_render_count: engine.pod_render_count(root),
        final_control_flow: loop_report
            .last_control_flow
            .unwrap_or(PlatformControlFlow::Wait),
    }
}
