//! UI 主线程前景异步执行器 [`ForegroundExecutor`] 与前景协程上下文 [`AsyncApp`]。
//!
//! 关键设计：
//! - **UI 主线程单线程执行**：前景协程不要求 `Send`，可直接安全操作 `PodPool` 与线程局部 `Signal<T>`；
//! - **RAII 活动引擎守卫**：仅在 `VeloxEngine::poll_async_tasks` 期间临时挂载 `*mut PodPool`，
//!   协程跨 `.await` 挂起时自动摘除，杜绝悬垂指针与双 `&mut` 别名；
//! - **零轮询事件驱动唤醒**：后台任务完成或定时器到期时通过 `wake_ui`（如 `WinitUserEvent::WakeAsync`）
//!   精准唤醒休眠中的事件循环，空闲期间保持 0% CPU。

use core::any::TypeId;
use core::cell::{Cell, RefCell};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};
use core::time::Duration;
use std::boxed::Box;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::Wake;
use std::vec::Vec;

use crate::element::View;
use crate::pod::{ActionScope, DragEvent, Handle, PodId, PodPool};

use super::backend::SchedulerBackend;
use super::scheduler::SchedulerCapabilities;
use super::std_scheduler::{BackgroundExecutor, NativeStdScheduler, Timer};
use super::task::{Task, TaskShared};

// ============================================================================
// 线程局部活动引擎上下文守卫（仅在 poll_ready 栈帧内有效）
// ============================================================================

#[derive(Copy, Clone)]
struct ActiveEngineCtx {
    pool_ptr: *mut PodPool,
    now_ms: u32,
}

impl ActiveEngineCtx {
    const EMPTY: Self = Self {
        pool_ptr: core::ptr::null_mut(),
        now_ms: 0,
    };
}

std::thread_local! {
    static ACTIVE_ENGINE_CTX: Cell<ActiveEngineCtx> = const { Cell::new(ActiveEngineCtx::EMPTY) };
}

struct ActiveEngineGuard {
    prev: ActiveEngineCtx,
}

impl ActiveEngineGuard {
    #[inline]
    fn enter(pool: &mut PodPool, now_ms: u32) -> Self {
        let prev = ACTIVE_ENGINE_CTX.with(|c| {
            c.replace(ActiveEngineCtx {
                pool_ptr: pool as *mut PodPool,
                now_ms,
            })
        });
        Self { prev }
    }
}

impl Drop for ActiveEngineGuard {
    #[inline]
    fn drop(&mut self) {
        ACTIVE_ENGINE_CTX.with(|c| c.set(self.prev));
    }
}

// ============================================================================
// 前景执行器跨线程唤醒与就绪队列
// ============================================================================

type WakeUiFn = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct ForegroundShared {
    next_id: AtomicU64,
    ready_queue: Mutex<VecDeque<u64>>,
    cancelled_ids: Mutex<Vec<u64>>,
    wake_ui: Mutex<Option<WakeUiFn>>,
    backend: Mutex<Arc<dyn SchedulerBackend>>,
}

impl ForegroundShared {
    fn new(backend: Arc<dyn SchedulerBackend>) -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            ready_queue: Mutex::new(VecDeque::new()),
            cancelled_ids: Mutex::new(Vec::new()),
            wake_ui: Mutex::new(None),
            backend: Mutex::new(backend),
        })
    }

    fn backend(&self) -> Arc<dyn SchedulerBackend> {
        let guard = self.backend.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(&guard)
    }

    fn set_backend(&self, backend: Arc<dyn SchedulerBackend>) {
        let mut guard = self.backend.lock().unwrap_or_else(|e| e.into_inner());
        *guard = backend;
    }

    fn set_wake_ui(&self, waker: WakeUiFn) {
        let mut guard = self.wake_ui.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(waker);
    }

    fn notify_ui(&self) {
        let cb = {
            let guard = self.wake_ui.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(Arc::clone)
        };
        if let Some(cb) = cb {
            cb();
        }
    }

    fn enqueue_ready(&self, task_id: u64) {
        {
            let mut q = self.ready_queue.lock().unwrap_or_else(|e| e.into_inner());
            q.push_back(task_id);
        }
        self.notify_ui();
    }

    fn enqueue_cancel(&self, task_id: u64) {
        {
            let mut c = self.cancelled_ids.lock().unwrap_or_else(|e| e.into_inner());
            c.push(task_id);
        }
        self.notify_ui();
    }

    fn has_ready(&self) -> bool {
        let q = self.ready_queue.lock().unwrap_or_else(|e| e.into_inner());
        !q.is_empty()
    }

    fn has_cancelled(&self) -> bool {
        let c = self
            .cancelled_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        !c.is_empty()
    }
}

struct FgTaskWaker {
    shared: Arc<ForegroundShared>,
    task_id: u64,
    queued: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

impl FgTaskWaker {
    fn schedule(&self) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        if !self.queued.swap(true, Ordering::AcqRel) {
            self.shared.enqueue_ready(self.task_id);
        }
    }
}

impl Wake for FgTaskWaker {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

struct FgTaskEntry {
    future: Pin<Box<dyn Future<Output = ()>>>,
    cancelled: Arc<AtomicBool>,
    queued: Arc<AtomicBool>,
}

pub(crate) struct ForegroundLocal {
    tasks: HashMap<u64, FgTaskEntry>,
    defers: VecDeque<Box<dyn FnOnce(&mut ActionScope)>>,
}

impl ForegroundLocal {
    fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            defers: VecDeque::new(),
        }
    }
}

/// UI 主线程前景异步执行器（由 [`crate::engine::VeloxEngine`] 持有）。
pub struct ForegroundExecutor {
    shared: Arc<ForegroundShared>,
    local: Rc<RefCell<ForegroundLocal>>,
}

impl Default for ForegroundExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ForegroundExecutor {
    /// 创建一个绑定默认内置 [`NativeStdScheduler`] 的主线程前景异步执行器。
    pub fn new() -> Self {
        Self::with_backend(NativeStdScheduler::global())
    }

    /// 创建一个绑定自定义可插拔 [`SchedulerBackend`] 的主线程前景异步执行器。
    pub fn with_backend(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self {
            shared: ForegroundShared::new(backend),
            local: Rc::new(RefCell::new(ForegroundLocal::new())),
        }
    }

    /// 切换当前前景执行器及其派生的所有 [`AsyncApp`] 所使用的底层 [`SchedulerBackend`]。
    pub fn set_backend(&self, backend: Arc<dyn SchedulerBackend>) {
        self.shared.set_backend(backend);
    }

    /// 获取当前配置的底层 [`SchedulerBackend`] 引用。
    #[inline]
    pub fn backend(&self) -> Arc<dyn SchedulerBackend> {
        self.shared.backend()
    }

    /// 获取绑定到当前调度器后端的 [`BackgroundExecutor`]。
    #[inline]
    pub fn background_executor(&self) -> BackgroundExecutor {
        BackgroundExecutor::with_backend(self.shared.backend())
    }

    /// 返回当前调度器后端名称。
    #[inline]
    pub fn scheduler_name(&self) -> &'static str {
        self.shared.backend().name()
    }

    /// 返回当前调度器能力描述符。
    #[inline]
    pub fn scheduler_capabilities(&self) -> SchedulerCapabilities {
        self.shared.backend().capabilities()
    }

    /// 注入平台级唤醒回调（如 `WinitUserEvent::WakeAsync` / Wayland `eventfd` / 移动端 Looper 唤醒）。
    pub fn set_wake_ui(&self, waker: impl Fn() + Send + Sync + 'static) {
        self.shared.set_wake_ui(Arc::new(waker));
    }

    /// 构造一个轻量 `Clone` 的前景协程上下文句柄 [`AsyncApp`]。
    #[inline]
    pub fn async_app(&self) -> AsyncApp {
        AsyncApp {
            shared: Arc::clone(&self.shared),
            local: Rc::clone(&self.local),
        }
    }

    /// 在 UI 主线程前景执行器上派生异步任务（接收 [`AsyncApp`]）。
    pub fn spawn<T, F, Fut>(&self, f: F) -> Task<T>
    where
        T: 'static,
        F: FnOnce(AsyncApp) -> Fut,
        Fut: Future<Output = T> + 'static,
    {
        self.async_app().spawn(f)
    }

    /// 排入一个延迟到当前事件冒泡结束后在 UI 主线程执行的回调（与 GPUI `cx.defer` 对齐）。
    pub fn defer<F>(&self, f: F)
    where
        F: FnOnce(&mut ActionScope) + 'static,
    {
        self.local.borrow_mut().defers.push_back(Box::new(f));
        self.shared.notify_ui();
    }

    /// 是否存在立即待执行的前景就绪任务或延迟 `defer` 回调（用于判断 `needs_redraw`）。
    #[inline]
    pub fn has_ready_work(&self) -> bool {
        self.shared.has_ready() || !self.local.borrow().defers.is_empty()
    }

    /// 是否存在待执行任务、延迟回调或待回收的已取消协程（用于事件结束后的轻量清理）。
    #[inline]
    pub(crate) fn has_work_to_poll(&self) -> bool {
        self.has_ready_work() || self.shared.has_cancelled()
    }

    /// 返回当前仍活跃挂载在前台执行器中的协程数量（用于测试验证取消与回收零泄漏）。
    #[inline]
    pub fn active_task_count(&self) -> usize {
        self.local.borrow().tasks.len()
    }

    /// 在 UI 主线程排空所有已就绪的前景协程与 `defer` 回调；返回是否至少推进了 1 项任务或回调。
    pub fn poll_ready(&self, pool: &mut PodPool, now_ms: u32) -> bool {
        let _guard = ActiveEngineGuard::enter(pool, now_ms);
        let mut did_work = false;

        // 上限 64 轮防止恶意自唤醒 `yield` 饿死主渲染帧
        for _round in 0..64 {
            // 1. 回收被 Drop 取消的协程，立即释放其捕获状态
            let cancelled_batch: Vec<u64> = {
                let mut c = self
                    .shared
                    .cancelled_ids
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                core::mem::take(&mut *c)
            };
            if !cancelled_batch.is_empty() {
                let mut loc = self.local.borrow_mut();
                for id in cancelled_batch {
                    loc.tasks.remove(&id);
                }
            }

            // 2. 取出当前一轮所有已就绪的前景任务 ID
            let ready_batch: Vec<u64> = {
                let mut q = self
                    .shared
                    .ready_queue
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                q.drain(..).collect()
            };

            let mut progressed_this_round = false;

            for task_id in ready_batch {
                // 先从 HashMap 移出到局部栈再 poll，允许协程内部重入调用 cx.spawn / cx.defer 而无 RefCell 冲突
                let entry_opt = self.local.borrow_mut().tasks.remove(&task_id);
                let Some(mut entry) = entry_opt else {
                    continue;
                };
                entry.queued.store(false, Ordering::Release);
                if entry.cancelled.load(Ordering::Acquire) {
                    continue;
                }

                did_work = true;
                progressed_this_round = true;

                let waker = Waker::from(Arc::new(FgTaskWaker {
                    shared: Arc::clone(&self.shared),
                    task_id,
                    queued: Arc::clone(&entry.queued),
                    cancelled: Arc::clone(&entry.cancelled),
                }));
                let mut cx = Context::from_waker(&waker);
                match entry.future.as_mut().poll(&mut cx) {
                    Poll::Ready(()) => {}
                    Poll::Pending => {
                        if !entry.cancelled.load(Ordering::Acquire) {
                            self.local.borrow_mut().tasks.insert(task_id, entry);
                        }
                    }
                }
            }

            // 3. 排空本轮积攒的所有 `defer` 延迟回调
            loop {
                let next_defer = self.local.borrow_mut().defers.pop_front();
                let Some(cb) = next_defer else {
                    break;
                };
                did_work = true;
                progressed_this_round = true;
                let ctx = ACTIVE_ENGINE_CTX.with(|c| c.get());
                if !ctx.pool_ptr.is_null() {
                    let pool_ref = unsafe { &mut *ctx.pool_ptr };
                    let mut acx = ActionScope::new(
                        PodId(u16::MAX),
                        pool_ref,
                        (0, 0),
                        DragEvent::default(),
                        ctx.now_ms,
                    )
                    .with_async_app(self.async_app());
                    cb(&mut acx);
                    acx.flush_signal_dirty_pods();
                }
            }

            // 将期间通过 Signal<T> 写入产生的脏视图同步刷入 PodPool
            let ctx = ACTIVE_ENGINE_CTX.with(|c| c.get());
            if !ctx.pool_ptr.is_null() {
                unsafe { &mut *ctx.pool_ptr }.flush_signal_dirty_pods();
            }

            if !progressed_this_round || !self.has_ready_work() {
                break;
            }
        }

        did_work
    }
}

// ============================================================================
// 前景协程跨 `.await` 上下文句柄 `AsyncApp`
// ============================================================================

/// 前景 UI 协程持有的异步应用上下文（与 GPUI `AsyncApp` 对齐）。
///
/// `!Send`（绑定 UI 主线程），可自由跨 `.await` 存活并随时安全访问 `PodPool`、派生子任务、
/// 发起 `background_spawn` 后台计算与 `timer` 定时器。
#[derive(Clone)]
pub struct AsyncApp {
    shared: Arc<ForegroundShared>,
    local: Rc<RefCell<ForegroundLocal>>,
}

impl AsyncApp {
    /// 获取绑定到当前引擎调度器后端的后台异步执行器句柄（用于 `.spawn(...)` / `.timer(Duration)`）。
    #[inline]
    pub fn background_executor(&self) -> BackgroundExecutor {
        BackgroundExecutor::with_backend(self.shared.backend())
    }

    /// 在当前引擎配置的后台调度器中执行 CPU 密集型计算或阻塞 I/O，返回可 `.await` 的 [`Task<T>`]。
    #[inline]
    pub fn background_spawn<T, Fut>(&self, fut: Fut) -> Task<T>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        self.background_executor().spawn(fut)
    }

    /// 创建由当前引擎调度器时钟驱动的非阻塞异步定时器（与 GPUI `cx.background_executor().timer(dur)` 等价简写）。
    #[inline]
    pub fn timer(&self, duration: Duration) -> Timer {
        self.background_executor().timer(duration)
    }

    /// 返回当前引擎配置的调度器名称。
    #[inline]
    pub fn scheduler_name(&self) -> &'static str {
        self.shared.backend().name()
    }

    /// 在 UI 主线程前景执行器上派生子异步任务（不绑定特定视图句柄）。
    pub fn spawn<T, F, Fut>(&self, f: F) -> Task<T>
    where
        T: 'static,
        F: FnOnce(AsyncApp) -> Fut,
        Fut: Future<Output = T> + 'static,
    {
        let task_id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicBool::new(true));

        let shared_for_cancel = Arc::clone(&self.shared);
        let on_cancel: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            shared_for_cancel.enqueue_cancel(task_id);
        });
        let task_shared = TaskShared::new(Arc::clone(&cancelled), Some(on_cancel));
        let task_out = Arc::clone(&task_shared);

        let fut = f(self.clone());
        let wrapped = async move {
            let output = fut.await;
            if !task_out.is_cancelled() {
                task_out.complete(output);
            }
        };

        self.local.borrow_mut().tasks.insert(
            task_id,
            FgTaskEntry {
                future: Box::pin(wrapped),
                cancelled,
                queued,
            },
        );
        self.shared.enqueue_ready(task_id);
        Task::from_shared(task_shared)
    }

    /// 在 UI 主线程前景执行器上为指定视图句柄派生绑定异步任务（闭包接收 `(Handle<V>, AsyncApp)`）。
    pub fn spawn_for<V, T, F, Fut>(&self, handle: Handle<V>, f: F) -> Task<T>
    where
        V: View,
        T: 'static,
        F: FnOnce(Handle<V>, AsyncApp) -> Fut,
        Fut: Future<Output = T> + 'static,
    {
        self.spawn(move |cx| f(handle, cx))
    }

    /// 排入一个延迟到本轮前景调度末尾执行的 UI 主线程回调。
    #[inline]
    pub fn defer<F>(&self, f: F)
    where
        F: FnOnce(&mut ActionScope) + 'static,
    {
        self.local.borrow_mut().defers.push_back(Box::new(f));
        self.shared.notify_ui();
    }

    /// 在前景协程中安全更新任意 [`Handle<V>`] 视图的状态（自动校验 ABA `generation`、防重入、标脏并刷新信号）。
    /// 若目标视图已卸载或正在被可变借用，则安全返回 `None` 绝不 panic。
    pub fn update<V: View, R>(
        &mut self,
        handle: Handle<V>,
        f: impl FnOnce(&mut V, &mut ActionScope) -> R,
    ) -> Option<R> {
        let ctx = ACTIVE_ENGINE_CTX.with(|c| c.get());
        if ctx.pool_ptr.is_null() {
            return None;
        }
        let pool = unsafe { &mut *ctx.pool_ptr };
        let target_ptr = pool.acquire_raw_mut(handle.id, Some(handle.generation), TypeId::of::<V>())?;
        let mut acx = ActionScope::new(
            handle.id,
            pool,
            (0, 0),
            DragEvent::default(),
            ctx.now_ms,
        )
        .with_async_app(self.clone());
        let view_ref = unsafe { &mut *(target_ptr as *mut V) };
        let res = f(view_ref, &mut acx);
        let skipped = acx.notify_skipped;
        acx.release_view_borrow(handle.id);
        if core::mem::size_of::<V>() > 0 && !skipped {
            acx.notify();
        }
        acx.flush_signal_dirty_pods();
        Some(res)
    }

    /// 在前景协程中只读读取任意 [`Handle<V>`] 视图的快照数据（若目标视图已卸载则返回 `None`）。
    pub fn read<V: View, R>(&self, handle: Handle<V>, f: impl FnOnce(&V) -> R) -> Option<R> {
        let ctx = ACTIVE_ENGINE_CTX.with(|c| c.get());
        if ctx.pool_ptr.is_null() {
            return None;
        }
        let pool = unsafe { &*ctx.pool_ptr };
        let view = pool.get(handle)?;
        Some(f(view))
    }

    /// 显式标记指定视图句柄为脏。
    pub fn notify_handle<V: View>(&mut self, handle: Handle<V>) {
        let ctx = ACTIVE_ENGINE_CTX.with(|c| c.get());
        if ctx.pool_ptr.is_null() {
            return;
        }
        let pool = unsafe { &mut *ctx.pool_ptr };
        if pool.get(handle).is_some() {
            pool.mark_dirty(handle.id);
        }
    }

    /// 获取当前引擎单调动画时钟（毫秒）。
    #[inline]
    pub fn now_ms(&self) -> u32 {
        ACTIVE_ENGINE_CTX.with(|c| c.get().now_ms)
    }
}
