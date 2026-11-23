//! Adapton / Reactively / Alien-Signals 风格的细粒度响应式信号图（`Signal<T>`、`Computed<T>`、`Effect` 与 `batch`）。
//!
//! 模块职责划分：
//! - `storage`：线程局部 / `#![no_std]` 追踪上下文、零堆内联存储与默认 Early Cutoff 比较器；
//! - `table`：侵入式双向链表依赖边池、节点槽位结构与 `ReactiveGraphTable` 存储表；
//! - `runtime`：三色 Push-Pull 运行时引擎、`batch` 事务与 `PodPool` 订阅桥接；
//! - `handles`：4 字节 `Copy` 强类型用户句柄（[`Signal`]、[`Computed`]、[`Effect`]）。

mod handles;
mod runtime;
mod storage;
mod table;

/// 响应式信号池最大槽位数（`std` 下支持动态扩容）。
pub const MAX_SIGNALS: usize = 512;

/// 派生计算节点最大槽位数（`std` 下支持动态扩容）。
pub const MAX_COMPUTEDS: usize = 64;
/// 响应式副作用观察器最大槽位数。
pub const MAX_EFFECTS: usize = 32;

/// 历史兼容常量（已由侵入式依赖边池替代，单个节点不再受 8 依赖硬上限限制）。
pub const MAX_DEPS_PER_NODE: usize = 8;


pub use handles::{computed, computed_eq, effect, signal, Computed, Effect, Signal};
pub use runtime::{batch, has_pending_dirty_pods};
pub(crate) use runtime::{
    begin_pod_tracking, drain_pending_dirty_pods, end_pod_tracking, remove_pod_subscriptions,
};
pub(crate) use storage::alloc_pool_id;
pub use table::active_dep_edge_count;
