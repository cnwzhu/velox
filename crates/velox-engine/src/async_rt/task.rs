//! RAII 生命周期托管的可取消异步任务句柄 [`Task<T>`]。
//!
//! - 默认绑定宿主生命周期：存入视图结构体字段后，视图销毁或被新任务覆盖时 `Drop` 自动取消；
//! - 调用 [`Task::detach`] 后脱离句柄托管，允许“发后即忘”任务运行至完成；
//! - 实现 [`core::future::Future`]，可直接在前台协程中 `.await` 获取后台计算或子任务返回值。

use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};
use std::sync::{Arc, Mutex};

enum TaskSlotState<T> {
    Pending(Option<Waker>),
    Ready(Option<T>),
}

pub(crate) struct TaskShared<T> {
    cancelled: Arc<AtomicBool>,
    state: Mutex<TaskSlotState<T>>,
    on_cancel: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl<T> TaskShared<T> {
    pub(crate) fn new(
        cancelled: Arc<AtomicBool>,
        on_cancel: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cancelled,
            state: Mutex::new(TaskSlotState::Pending(None)),
            on_cancel,
        })
    }

    pub(crate) fn complete(&self, value: T) {
        let waker = {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let prev = core::mem::replace(&mut *guard, TaskSlotState::Ready(Some(value)));
            match prev {
                TaskSlotState::Pending(w) => w,
                TaskSlotState::Ready(_) => None,
            }
        };
        if let Some(w) = waker {
            w.wake();
        }
    }

    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// 异步任务句柄（与 GPUI `Task<T>` 对齐：Drop 自动取消、可 `.detach()`、可 `.await`）。
#[must_use = "Task 被直接丢弃时会立即取消；请将其保存在视图字段中、调用 `.await` 或显式 `.detach()`"]
pub struct Task<T> {
    shared: Option<Arc<TaskShared<T>>>,
    immediate: Option<T>,
    detached: bool,
    completed: bool,
}

impl<T> Task<T> {
    pub(crate) fn from_shared(shared: Arc<TaskShared<T>>) -> Self {
        Self {
            shared: Some(shared),
            immediate: None,
            detached: false,
            completed: false,
        }
    }

    /// 构造一个已就绪的同步完成任务（便于条件分支统一返回 `Task<T>`）。
    #[inline]
    pub fn ready(value: T) -> Self {
        Self {
            shared: None,
            immediate: Some(value),
            detached: false,
            completed: false,
        }
    }

    /// 脱离句柄托管，允许该任务在后台或前台独立运行至完成而不随句柄销毁被取消。
    #[inline]
    pub fn detach(mut self) {
        self.detached = true;
    }

    /// 判断当前任务是否已被取消。
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.shared
            .as_ref()
            .map(|s| s.is_cancelled())
            .unwrap_or(false)
    }
}

impl<T> Unpin for Task<T> {}

impl<T> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(val) = this.immediate.take() {
            this.completed = true;
            return Poll::Ready(val);
        }
        let Some(shared) = &this.shared else {
            panic!("Velox Task polled after completion");
        };
        let mut guard = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        match &mut *guard {
            TaskSlotState::Ready(opt) => {
                let val = opt.take().expect("Velox Task polled after Ready");
                this.completed = true;
                Poll::Ready(val)
            }
            TaskSlotState::Pending(slot_waker) => {
                *slot_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if !self.detached && !self.completed {
            if let Some(shared) = &self.shared {
                if !shared.cancelled.swap(true, Ordering::AcqRel) {
                    if let Some(on_cancel) = &shared.on_cancel {
                        on_cancel();
                    }
                }
            }
        }
    }
}
