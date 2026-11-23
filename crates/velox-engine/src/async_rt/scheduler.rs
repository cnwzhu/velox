//! 通用异步调度器抽象与异步事件循环驱动器。
//!
//! 本模块作为 Velox 框架与各类异步调度器之间的隔离层：
//! 可由 `StdAsyncDriver`、[`ManualScheduler`](super::ManualScheduler) 或 `Tokio` / `smol` 适配器无缝接入 [`run_async_event_loop`]。

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;

use crate::engine::VeloxEngine;
use crate::platform::{AsyncPlatform, PlatformControlFlow};

/// 调度器能力描述符（用于诊断与运行时策略探测）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SchedulerCapabilities {
    /// 调度器名称（如 `"velox-native-std"`、`"velox-manual"`、`"tokio"`、`"smol"`）。
    pub name: &'static str,
    /// 是否支持独立后台工作线程卸载（单线程 / WASM 为 `false`，桌面多线程为 `true`）。
    pub supports_background_threads: bool,
    /// 是否支持在事件回调中动态派生异步协程。
    pub supports_dynamic_spawn: bool,
    /// 是否采用可控确定性虚拟时钟（用于单元测试与时间快进）。
    pub deterministic_clock: bool,
}

/// 主动向当前异步执行器让出一次调度片的 [`Future`]。
///
/// 在单线程协作式调度器（如 `LocalSet`）中，可防止连续动画渲染饿死同线程其他异步任务。
#[derive(Default)]
pub struct YieldNow {
    yielded: bool,
}

impl YieldNow {
    /// 创建一个单次让出调度片的 [`YieldNow`] Future。
    #[inline(always)]
    pub const fn new() -> Self {
        Self { yielded: false }
    }
}

impl Unpin for YieldNow {}

impl Future for YieldNow {
    type Output = ();

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// 单调时钟与非阻塞睡眠抽象。
///
/// 隔离框架核心与底层时钟源：
/// - 在桌面/移动端由默认原生调度器或第三方运行时（如 Tokio / smol）提供；
/// - 在单元测试中由确定性虚拟时钟提供 0ms 瞬时快进能力。
pub trait SchedulerClock {
    /// 返回单调递增的当前时间戳（毫秒）。
    fn now_ms(&self) -> u64;

    /// 非阻塞异步等待指定时长 `duration`。
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()>;
}

/// 异步主循环调度器抽象（面向 Tokio / smol / Std / 自定义测试宿主）。
///
/// 将 [`AsyncPlatform`] 与具体执行器彻底解耦：
/// - 活跃渲染期间：按 [`AsyncLoopConfig::frame_interval`] 节流，并在渲染超时时调用 [`Self::yield_now`] 让出调度片；
/// - 画面静止期间（[`PlatformControlFlow::Wait`]）：挂起在 [`Self::wait_for_event`] 上，实现 0% CPU 占用。
pub trait AsyncDriver: SchedulerClock {
    /// 返回当前调度器的能力元信息。
    fn capabilities(&self) -> SchedulerCapabilities;

    /// 当画面静止（`PlatformControlFlow::Wait`）时挂起等待外部事件（输入事件、后台异步完成等）唤醒。
    ///
    /// 实现方应保证黏性（latched）唤醒语义（例如 `UiWakeSignal`），避免在进入 `wait_for_event` 前夕触发的唤醒丢失。
    fn wait_for_event(&self) -> impl Future<Output = ()>;

    /// 主动向当前异步执行器让出一次调度片。
    #[inline]
    fn yield_now(&self) -> impl Future<Output = ()> {
        YieldNow::new()
    }
}

/// 通用异步事件循环配置参数。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct AsyncLoopConfig {
    /// 活跃帧期间的目标刷新间隔（默认 `16ms` 对应 60Hz；`Duration::ZERO` 表示每帧仅 `yield_now`）。
    pub frame_interval: Duration,
    /// 可选最大步进帧数上限（`None` 表示持续运行至 `PlatformControlFlow::Exit`）。
    pub max_steps: Option<u32>,
    /// 当画面首次进入 `PlatformControlFlow::Wait` 空闲态时是否直接退出循环（默认 `false`：挂起等待唤醒）。
    pub exit_on_idle: bool,
}

impl Default for AsyncLoopConfig {
    #[inline(always)]
    fn default() -> Self {
        Self {
            frame_interval: Duration::from_millis(16),
            max_steps: None,
            exit_on_idle: false,
        }
    }
}

impl AsyncLoopConfig {
    /// 以指定目标帧间隔创建持续运行的异步事件循环配置。
    #[inline(always)]
    pub const fn new(frame_interval: Duration) -> Self {
        Self {
            frame_interval,
            max_steps: None,
            exit_on_idle: false,
        }
    }

    /// 设置最大步进帧数上限（常用于测试与确定性演练）。
    #[inline(always)]
    pub const fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = Some(max_steps);
        self
    }

    /// 设置是否在首次静止（`PlatformControlFlow::Wait`）时立即退出循环。
    #[inline(always)]
    pub const fn with_exit_on_idle(mut self, exit_on_idle: bool) -> Self {
        self.exit_on_idle = exit_on_idle;
        self
    }
}

/// [`run_async_event_loop`] 退出时返回的运行统计报告。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct AsyncLoopReport {
    /// 累计执行的 `step_platform_async` 步数。
    pub steps: u32,
    /// 累计进入 `PlatformControlFlow::Wait` 空闲休眠（或空闲退出）的次数。
    pub idle_waits: u32,
    /// 循环退出前最后一次返回的控制流状态。
    pub last_control_flow: Option<PlatformControlFlow>,
}

/// 跨调度器、全平台通用的异步主循环驱动函数。
///
/// 统一协调 [`AsyncPlatform`]、[`VeloxEngine`] 与可插拔 [`AsyncDriver`]：
/// 1. 使用 [`SchedulerClock::now_ms`] 自动计算真实增量 `dt_ms`（钳位至 `1..=100` ms）；
/// 2. 异步调用 `platform.step_platform_async(engine, dt_ms).await`；
/// 3. 返回 `Poll` 时按剩余帧预算 `driver.sleep(...).await` 或 `driver.yield_now().await`；
/// 4. 返回 `Wait` 且无新增脏状态时，挂起在 `driver.wait_for_event().await` 进入 0% CPU 休眠；
/// 5. 返回 `Exit` 或达到 `config.max_steps` 时干净退出并返回 [`AsyncLoopReport`]。
pub async fn run_async_event_loop<P, D>(
    platform: &mut P,
    engine: &mut VeloxEngine,
    driver: &D,
    config: AsyncLoopConfig,
) -> AsyncLoopReport
where
    P: AsyncPlatform,
    D: AsyncDriver,
{
    let mut report = AsyncLoopReport::default();
    let mut last_ms = driver.now_ms();
    let frame_ms = config.frame_interval.as_millis().min(u64::MAX as u128) as u64;

    loop {
        if let Some(limit) = config.max_steps {
            if report.steps >= limit {
                break;
            }
        }

        let step_start_ms = driver.now_ms();
        let dt_ms = step_start_ms.saturating_sub(last_ms).clamp(1, 100) as u16;
        last_ms = step_start_ms;

        let flow = platform.step_platform_async(engine, dt_ms).await;
        report.steps = report.steps.saturating_add(1);
        report.last_control_flow = Some(flow);

        match flow {
            PlatformControlFlow::Exit => break,
            PlatformControlFlow::Wait => {
                // 若在步进末尾恰有异步事件或外设中断标脏，立即让出一次并继续下一帧，零延迟响应
                if engine.needs_redraw() {
                    driver.yield_now().await;
                    continue;
                }
                report.idle_waits = report.idle_waits.saturating_add(1);
                if config.exit_on_idle {
                    break;
                }
                driver.wait_for_event().await;
                last_ms = driver.now_ms();
            }
            PlatformControlFlow::Poll => {
                let elapsed_ms = driver.now_ms().saturating_sub(step_start_ms);
                if frame_ms > 0 && elapsed_ms < frame_ms {
                    driver
                        .sleep(Duration::from_millis(frame_ms - elapsed_ms))
                        .await;
                } else {
                    driver.yield_now().await;
                }
            }
        }
    }

    report
}
