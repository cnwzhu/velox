//! 强类型 4 字节零堆响应式句柄：`Signal<T>`、`Computed<T>` 与 `Effect`。

use core::marker::PhantomData;

use crate::pod::{ActionScope, FrameScope};

use super::runtime::{
    ensure_computed_fresh, notify_signal_changed, record_computed_read, record_signal_read,
    run_effect_slot,
};
use super::table::{with_table, NodeKind, NodeState};
use super::table::{ComputedSlot, SignalSlot};
use super::storage::{
    default_eq_fn, get_active_render_context, InlineFnStorage, InlineValueStorage, NO_EDGE,
};
use super::storage::TRACKER_INDEX_MASK;
use super::MAX_EFFECTS;

/// NSH 风格细粒度响应式信号句柄（4 字节 `Copy`，含 `generation` 代号校验）。
///
/// 支持 `T: Copy + 'static`（尺寸 `<= 16` 字节，对齐 `<= 8` 字节，零堆分配）。
#[repr(C)]
pub struct Signal<T: Copy + 'static> {
    id: u16,
    generation: u16,
    _marker: PhantomData<T>,
}

impl<T: Copy + 'static> Copy for Signal<T> {}
impl<T: Copy + 'static> Clone for Signal<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy + 'static> PartialEq for Signal<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.generation == other.generation
    }
}
impl<T: Copy + 'static> Eq for Signal<T> {}

impl<T: Copy + 'static> core::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Signal")
            .field("id", &self.id)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T: Copy + core::fmt::Display + 'static> core::fmt::Display for Signal<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.get(), f)
    }
}

impl<T: Copy + core::fmt::Binary + 'static> core::fmt::Binary for Signal<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Binary::fmt(&self.get(), f)
    }
}

impl<T: Copy + core::fmt::LowerHex + 'static> core::fmt::LowerHex for Signal<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.get(), f)
    }
}

impl<T: Copy + core::fmt::UpperHex + 'static> core::fmt::UpperHex for Signal<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::UpperHex::fmt(&self.get(), f)
    }
}

impl<T: Copy + PartialEq + 'static> PartialEq<T> for Signal<T> {
    #[inline(always)]
    fn eq(&self, other: &T) -> bool {
        self.get() == *other
    }
}

impl<T: Copy + PartialOrd + 'static> PartialOrd<T> for Signal<T> {
    #[inline(always)]
    fn partial_cmp(&self, other: &T) -> Option<core::cmp::Ordering> {
        self.get().partial_cmp(other)
    }
}

impl<T: Copy + core::ops::AddAssign<Rhs> + 'static, Rhs> core::ops::AddAssign<Rhs> for Signal<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Rhs) {
        self.update(|v| *v += rhs);
    }
}

impl<T: Copy + core::ops::SubAssign<Rhs> + 'static, Rhs> core::ops::SubAssign<Rhs> for Signal<T> {
    #[inline]
    fn sub_assign(&mut self, rhs: Rhs) {
        self.update(|v| *v -= rhs);
    }
}

impl<T: Copy + core::ops::BitOrAssign<Rhs> + 'static, Rhs> core::ops::BitOrAssign<Rhs>
    for Signal<T>
{
    #[inline]
    fn bitor_assign(&mut self, rhs: Rhs) {
        self.update(|v| *v |= rhs);
    }
}

impl<T: Copy + core::ops::BitXorAssign<Rhs> + 'static, Rhs> core::ops::BitXorAssign<Rhs>
    for Signal<T>
{
    #[inline]
    fn bitxor_assign(&mut self, rhs: Rhs) {
        self.update(|v| *v ^= rhs);
    }
}

/// 创建一个新的响应式信号 [`Signal<T>`]。
#[inline]
pub fn signal<T: Copy + 'static>(initial: T) -> Signal<T> {
    Signal::new(initial)
}

impl<T: Copy + 'static> Signal<T> {
    /// 分配并初始化一个响应式信号（`std` 下自动扩容；若槽位回收则强制递增 `generation` 保证类型安全）。
    pub fn new(initial: T) -> Self {
        with_table(|tbl| {
            let idx = if let Some(free_idx) = tbl.signals.iter().position(|s| !s.occupied) {
                free_idx
            } else {
                if tbl.signals.len() < (TRACKER_INDEX_MASK as usize) {
                    let new_idx = tbl.signals.len();
                    tbl.signals.push(SignalSlot::empty());
                    new_idx
                } else {
                    let cap = tbl.signals.len().max(1);
                    let start = tbl.recycle_cursor % cap;
                    (0..cap)
                        .map(|off| (start + off) % cap)
                        .find(|&i| !tbl.signals[i].has_any_subscribers())
                        .unwrap_or_else(|| {
                            let i = tbl.recycle_cursor % cap;
                            tbl.recycle_cursor = tbl.recycle_cursor.wrapping_add(1);
                            i
                        })
                }
            };
            tbl.recycle_cursor = idx.wrapping_add(1);
            if tbl.signals[idx].occupied {
                tbl.unlink_all_subs(NodeKind::Signal, idx as u16);
                tbl.signals[idx].generation = tbl.signals[idx].generation.wrapping_add(1);
            }
            let slot = &mut tbl.signals[idx];
            unsafe {
                slot.storage.write(initial);
            }
            slot.version = 1;
            slot.clear_all_subscribers();
            slot.first_sub = NO_EDGE;
            slot.occupied = true;
            Self {
                id: idx as u16,
                generation: slot.generation,
                _marker: PhantomData,
            }
        })
    }

    /// 读取信号当前值；若处于 `View::render`、`Computed` 或 `Effect` 上下文中，自动注册依赖订阅。
    #[inline]
    pub fn get(self) -> T {
        let (val, ver) = with_table(|tbl| {
            let slot = tbl
                .signals
                .get(self.id as usize)
                .expect("Attempted to read an out-of-range Signal");
            assert!(
                slot.occupied && slot.generation == self.generation,
                "Attempted to read a disposed Signal"
            );
            let v = unsafe { slot.storage.read::<T>() };
            (v, slot.version)
        });
        record_signal_read(self.id, self.generation, ver);
        val
    }

    /// 在给定 [`FrameScope`] 中读取信号当前值，并显式将 `cx.current_pod()` 注册为订阅者。
    #[inline]
    pub fn get_in(self, cx: &FrameScope) -> T {
        let pod_idx = cx.current_pod().0;
        let (active_pool, _) = get_active_render_context();
        let (val, ver) = with_table(|tbl| {
            let slot = tbl
                .signals
                .get_mut(self.id as usize)
                .expect("Attempted to read an out-of-range Signal");
            assert!(
                slot.occupied && slot.generation == self.generation,
                "Attempted to read a disposed Signal"
            );
            let added = slot.add_subscriber(active_pool, pod_idx);
            let v = unsafe { slot.storage.read::<T>() };
            let ver = slot.version;
            if !added {
                tbl.dropped_pool_subscriptions = tbl.dropped_pool_subscriptions.saturating_add(1);
            }
            (v, ver)
        });
        record_signal_read(self.id, self.generation, ver);
        val
    }

    /// 读取信号当前值但不触发依赖追踪。
    #[inline]
    pub fn get_untracked(self) -> T {
        with_table(|tbl| {
            let slot = tbl
                .signals
                .get(self.id as usize)
                .expect("Attempted to read an out-of-range Signal");
            assert!(
                slot.occupied && slot.generation == self.generation,
                "Attempted to read a disposed Signal"
            );
            unsafe { slot.storage.read::<T>() }
        })
    }

    /// 写入新值，递增信号版本号，通过三色 Push-Pull 图传播脏状态并自动通知订阅视图与 `Effect`。
    #[inline]
    pub fn set(self, val: T) {
        let notify_info = with_table(|tbl| {
            let idx = self.id as usize;
            let slot = tbl.signals.get_mut(idx)?;
            if !slot.occupied || slot.generation != self.generation {
                return None;
            }
            unsafe {
                slot.storage.write(val);
            }
            slot.version = slot.version.wrapping_add(1).max(1);
            Some(tbl.push_dirty_from_signal(idx))
        });
        if let Some((has_subs, eff_mask)) = notify_info {
            notify_signal_changed(has_subs, eff_mask);
        }
    }

    /// 当新值与旧值不相等时才写入并通知订阅者（返回 `true` 表示发生了实际变更）。
    #[inline]
    pub fn set_if_changed(self, val: T) -> bool
    where
        T: PartialEq,
    {
        if self.get_untracked() != val {
            self.set(val);
            true
        } else {
            false
        }
    }

    /// 原地修改信号值，递增版本号并自动通知所有订阅该信号的 `PodId`、`Computed` 与 `Effect`（闭包执行期间不持有全局表可变借用，100% 重入安全）。
    #[inline]
    pub fn update(self, f: impl FnOnce(&mut T)) {
        let cur_opt = with_table(|tbl| {
            let slot = tbl.signals.get(self.id as usize)?;
            if !slot.occupied || slot.generation != self.generation {
                return None;
            }
            Some(unsafe { slot.storage.read::<T>() })
        });
        let Some(mut cur_val) = cur_opt else {
            return;
        };
        f(&mut cur_val);
        let notify_info = with_table(|tbl| {
            let idx = self.id as usize;
            let slot = tbl.signals.get_mut(idx)?;
            if !slot.occupied || slot.generation != self.generation {
                return None;
            }
            unsafe {
                slot.storage.write(cur_val);
            }
            slot.version = slot.version.wrapping_add(1).max(1);
            Some(tbl.push_dirty_from_signal(idx))
        });
        if let Some((has_subs, eff_mask)) = notify_info {
            notify_signal_changed(has_subs, eff_mask);
        }
    }

    /// 在事件回调 [`ActionScope`] 中写入新值，并立即将所有订阅该信号的 `PodId` 标记到 `acx.pool` 脏队列中。
    #[inline]
    pub fn set_in(self, acx: &mut ActionScope, val: T) {
        self.set(val);
        acx.flush_signal_dirty_pods();
    }

    /// 在事件回调 [`ActionScope`] 中原地更新信号值，并立即将所有订阅该信号的 `PodId` 标记到 `acx.pool` 脏队列中。
    #[inline]
    pub fn update_in(self, acx: &mut ActionScope, f: impl FnOnce(&mut T)) {
        self.update(f);
        acx.flush_signal_dirty_pods();
    }

    /// 获取该信号当前的单调版本号。
    #[inline]
    pub fn version(self) -> u32 {
        with_table(|tbl| {
            tbl.signals
                .get(self.id as usize)
                .filter(|s| s.occupied && s.generation == self.generation)
                .map(|s| s.version)
                .unwrap_or(0)
        })
    }

    /// 获取当前订阅了该信号的 `PodId` 位掩码（`bit i == 1` 表示 `PodId(i)` 依赖该信号）。
    #[inline]
    pub fn subscribers_mask(self) -> u64 {
        with_table(|tbl| {
            tbl.signals
                .get(self.id as usize)
                .filter(|s| s.occupied && s.generation == self.generation)
                .map(|s| s.all_subscribers_mask())
                .unwrap_or(0)
        })
    }

    /// 销毁释放该信号槽位、回收其全部下游依赖边并递增 `generation`（防止 ABA 悬垂引用）。
    pub fn dispose(self) -> bool {
        with_table(|tbl| {
            let idx = self.id as usize;
            let valid = tbl
                .signals
                .get(idx)
                .map(|s| s.occupied && s.generation == self.generation)
                .unwrap_or(false);
            if !valid {
                return false;
            }
            tbl.unlink_all_subs(NodeKind::Signal, self.id);
            let slot = &mut tbl.signals[idx];
            slot.occupied = false;
            slot.clear_all_subscribers();
            slot.first_sub = NO_EDGE;
            slot.generation = slot.generation.wrapping_add(1);
            true
        })
    }
}

/// 三色 Push-Pull 惰性记忆化派生计算句柄（4 字节 `Copy`）。
///
/// 支持任意深度 `Signal -> Computed -> Computed -> Effect` 链与菱形依赖无 Glitch，
/// 当上游重算结果与缓存值相同时自动触发 Early Cutoff 剪枝，跳过全部下游冗余计算。
#[repr(C)]
pub struct Computed<T: Copy + 'static> {
    id: u16,
    generation: u16,
    _marker: PhantomData<T>,
}

impl<T: Copy + 'static> Copy for Computed<T> {}
impl<T: Copy + 'static> Clone for Computed<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

/// 创建一个新的惰性记忆化派生计算节点 [`Computed<T>`]（支持捕获 [`Signal`] / [`Computed`] 句柄的闭包，零堆分配）。
///
/// 对于内置标量与短元组（`bool`/`i*`/`u*`/`f32`/`f64`/`char` 等），自动启用 Early Cutoff 剪枝；
/// 若自定义结构体需启用 Early Cutoff，可使用 [`computed_eq`]。
#[inline]
pub fn computed<T: Copy + 'static, F: Fn() -> T + Copy + 'static>(f: F) -> Computed<T> {
    Computed::new(f)
}

/// 创建一个带有显式 `PartialEq` Early Cutoff 剪枝比较器的派生计算节点 [`Computed<T>`]。
#[inline]
pub fn computed_eq<T: Copy + PartialEq + 'static, F: Fn() -> T + Copy + 'static>(
    f: F,
) -> Computed<T> {
    Computed::new_eq(f)
}

impl<T: Copy + 'static> Computed<T> {
    /// 使用闭包或函数指针创建 `#![no_std]` 零分配派生计算节点。
    pub fn new<F: Fn() -> T + Copy + 'static>(f: F) -> Self {
        Self::new_with_eq(f, default_eq_fn::<T>())
    }

    /// 使用显式 `PartialEq` 比较器创建派生计算节点，当结果相等时阻止版本号递增以触发 Early Cutoff 剪枝。
    pub fn new_eq<F: Fn() -> T + Copy + 'static>(f: F) -> Self
    where
        T: PartialEq,
    {
        unsafe fn eq_trampoline<T: Copy + PartialEq>(
            a: *const InlineValueStorage,
            b: *const InlineValueStorage,
        ) -> bool {
            let va = unsafe { (*a).read::<T>() };
            let vb = unsafe { (*b).read::<T>() };
            va == vb
        }
        Self::new_with_eq(f, Some(eq_trampoline::<T>))
    }

    fn new_with_eq<F: Fn() -> T + Copy + 'static>(
        f: F,
        eq_fn: Option<unsafe fn(*const InlineValueStorage, *const InlineValueStorage) -> bool>,
    ) -> Self {
        unsafe fn trampoline<T: Copy + 'static, F: Fn() -> T + Copy + 'static>(
            fn_ptr: *const InlineFnStorage,
            out: *mut InlineValueStorage,
        ) {
            let func = unsafe { &*(fn_ptr as *const F) };
            let val = func();
            unsafe {
                (*out).write(val);
            }
        }

        with_table(|tbl| {
            let idx = if let Some(free_idx) = tbl.computeds.iter().position(|c| !c.occupied) {
                free_idx
            } else {
                if tbl.computeds.len() < (TRACKER_INDEX_MASK as usize) {
                    let new_idx = tbl.computeds.len();
                    tbl.computeds.push(ComputedSlot::EMPTY);
                    new_idx
                } else {
                    let i = tbl.computed_cursor % tbl.computeds.len().max(1);
                    tbl.computed_cursor = tbl.computed_cursor.wrapping_add(1);
                    i
                }
            };
            if tbl.computeds[idx].occupied {
                tbl.unlink_all_deps(NodeKind::Computed, idx as u16);
                tbl.unlink_all_subs(NodeKind::Computed, idx as u16);
                tbl.computeds[idx].generation = tbl.computeds[idx].generation.wrapping_add(1);
            }
            let slot = &mut tbl.computeds[idx];
            unsafe {
                slot.fn_storage.write(f);
            }
            slot.compute_fn = Some(trampoline::<T, F>);
            slot.eq_fn = eq_fn;
            slot.first_dep = NO_EDGE;
            slot.first_sub = NO_EDGE;
            slot.version = 0;
            slot.eval_count = 0;
            slot.state = NodeState::Dirty;
            slot.initialized = false;
            slot.occupied = true;
            slot.computing = false;
            Self {
                id: idx as u16,
                generation: slot.generation,
                _marker: PhantomData,
            }
        })
    }

    /// 读取派生计算值：
    /// - `Clean` 时 $O(1)$ 直接返回缓存；
    /// - `Check` 时先递归验证上游（命中 Early Cutoff 则零重算直接恢复 `Clean`）；
    /// - 若处于外层 `Computed`、`Effect` 或 `View::render` 期间，自动建立依赖边并传递底层源信号订阅。
    pub fn get(self) -> T {
        let idx = self.id as usize;
        let Some(ver) = ensure_computed_fresh(idx, self.generation) else {
            panic!("Attempted to read a disposed or out-of-range Computed");
        };
        let val = with_table(|tbl| {
            let slot = tbl
                .computeds
                .get(idx)
                .expect("Attempted to read an out-of-range Computed");
            assert!(
                slot.occupied && slot.generation == self.generation,
                "Attempted to read a disposed Computed"
            );
            unsafe { slot.cached.read::<T>() }
        });
        record_computed_read(self.id, self.generation, ver);
        val
    }

    /// 获取该派生计算节点当前的内容版本号（仅当重算结果实质变化时递增）。
    #[inline]
    pub fn version(self) -> u32 {
        with_table(|tbl| {
            tbl.computeds
                .get(self.id as usize)
                .filter(|c| c.occupied && c.generation == self.generation)
                .map(|c| c.version)
                .unwrap_or(0)
        })
    }

    /// 获取该派生计算节点实际执行计算函数的累计次数（用于验证记忆化缓存与 Early Cutoff 命中）。
    #[inline]
    pub fn eval_count(self) -> u32 {
        with_table(|tbl| {
            tbl.computeds
                .get(self.id as usize)
                .filter(|c| c.occupied && c.generation == self.generation)
                .map(|c| c.eval_count)
                .unwrap_or(0)
        })
    }

    /// 销毁释放该派生计算节点并回收全部上下游依赖边（递增 `generation` 防 ABA）。
    pub fn dispose(self) -> bool {
        with_table(|tbl| {
            let idx = self.id as usize;
            let valid = tbl
                .computeds
                .get(idx)
                .map(|c| c.occupied && c.generation == self.generation)
                .unwrap_or(false);
            if !valid {
                return false;
            }
            tbl.unlink_all_deps(NodeKind::Computed, self.id);
            tbl.unlink_all_subs(NodeKind::Computed, self.id);
            let slot = &mut tbl.computeds[idx];
            slot.occupied = false;
            slot.initialized = false;
            slot.computing = false;
            slot.first_dep = NO_EDGE;
            slot.first_sub = NO_EDGE;
            slot.generation = slot.generation.wrapping_add(1);
            true
        })
    }
}

/// NSH 风格响应式副作用观察器句柄（4 字节 `Copy`）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Effect {
    id: u16,
    generation: u16,
}

/// 创建并立即执行一次响应式副作用观察器 [`Effect`]，当其读取的任何 [`Signal`] / [`Computed`] 发生实质变更时自动重跑。
#[inline]
pub fn effect<F: Fn() + Copy + 'static>(f: F) -> Effect {
    Effect::new(f)
}

impl Effect {
    /// 创建并立即执行一次响应式副作用观察器。
    pub fn new<F: Fn() + Copy + 'static>(f: F) -> Self {
        unsafe fn trampoline<F: Fn() + Copy + 'static>(fn_ptr: *const InlineFnStorage) {
            let func = unsafe { &*(fn_ptr as *const F) };
            func();
        }

        let (idx, generation) = with_table(|tbl| {
            let idx = tbl
                .effects
                .iter()
                .position(|e| !e.occupied)
                .unwrap_or_else(|| {
                    let i = tbl.effect_cursor % MAX_EFFECTS;
                    tbl.effect_cursor = tbl.effect_cursor.wrapping_add(1);
                    i
                });
            if tbl.effects[idx].occupied {
                tbl.unlink_all_deps(NodeKind::Effect, idx as u16);
                tbl.effects[idx].generation = tbl.effects[idx].generation.wrapping_add(1);
            }
            let slot = &mut tbl.effects[idx];
            unsafe {
                slot.fn_storage.write(f);
            }
            slot.run_fn = Some(trampoline::<F>);
            slot.first_dep = NO_EDGE;
            slot.run_count = 0;
            slot.occupied = true;
            slot.running = false;
            (idx, slot.generation)
        });

        run_effect_slot(idx);

        Self {
            id: idx as u16,
            generation,
        }
    }

    /// 获取该副作用观察器累计执行的次数。
    #[inline]
    pub fn run_count(self) -> u32 {
        with_table(|tbl| {
            tbl.effects
                .get(self.id as usize)
                .filter(|e| e.occupied && e.generation == self.generation)
                .map(|e| e.run_count)
                .unwrap_or(0)
        })
    }

    /// 停止并销毁该副作用观察器，并 $O(\text{deps})$ 精确回收其全部上游依赖边。
    pub fn dispose(self) -> bool {
        let idx = self.id as usize;
        with_table(|tbl| {
            let valid = tbl
                .effects
                .get(idx)
                .map(|e| e.occupied && e.generation == self.generation)
                .unwrap_or(false);
            if !valid {
                return false;
            }
            tbl.unlink_all_deps(NodeKind::Effect, self.id);
            let slot = &mut tbl.effects[idx];
            slot.occupied = false;
            slot.first_dep = NO_EDGE;
            slot.generation = slot.generation.wrapping_add(1);
            true
        })
    }
}
