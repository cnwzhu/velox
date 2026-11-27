//! 全局组件状态槽位池 `PodPool`（基于静态虚表与强类型 `Handle<V>` 代号校验）。

use core::any::TypeId;
use core::cell::Cell;
use core::marker::PhantomData;
use core::mem::size_of;

use crate::arena::SmallVec;
use crate::element::{Element, View};
use crate::solver::Region;

use super::{FrameScope, Handle, PodId, Scope};

/// 单个 `Pod` 状态默认槽位字节基准常量。
pub const POD_SLOT_BYTES: usize = 256;

/// `PodPool` 初始状态槽位数基准。
pub const MAX_PODS: usize = 16;

pub(super) type ReadersMask = u64;

/// 驻留在编译期 `.rodata` 中的组件静态虚表，避免在每个槽位重复存储 `TypeId` 与双函数指针。
pub(crate) struct PodVTable {
    type_id: fn() -> TypeId,
    pub(super) render_fn: unsafe fn(*mut u8, *mut FrameScope) -> Element<'static>,
    drop_fn: unsafe fn(*mut u8),
}

const STATE_FLAG_BORROWED: u8 = 1 << 0;
const STATE_FLAG_EXPANDING: u8 = 1 << 1;
const STATE_FLAG_NEEDS_NEXT_FRAME: u8 = 1 << 2;

pub(super) const LAYOUT_FLAG_SUBTREE_CLEAN: u8 = 1 << 0;
pub(super) const LAYOUT_FLAG_HAS_SUBVIEWS: u8 = 1 << 1;
pub(super) const LAYOUT_FLAG_DIRTY_DESCENDANT: u8 = 1 << 2;
pub(super) const LAYOUT_FLAG_CLAIMED: u8 = 1 << 3;
pub(super) const LAYOUT_FLAG_MULTI_MOUNTED: u8 = 1 << 4;
pub(super) const LAYOUT_FLAG_PARENT_NON_AUTO_W: u8 = 1 << 5;
pub(super) const LAYOUT_FLAG_PARENT_NON_AUTO_H: u8 = 1 << 6;

pub(crate) struct PodSlot {
    pub(super) vtable: Option<&'static PodVTable>,
    pub(crate) cached_element: Cell<*const Element<'static>>,
    pub(crate) cached_layout: Cell<*mut u8>,
    pub(crate) cached_bounds: Cell<Region>,
    pub(crate) render_count: Cell<u32>,
    pub(crate) generation: u16,
    pub(crate) readers_mask: Cell<ReadersMask>,
    pub(crate) pass1_min_w: Cell<i16>,
    pub(crate) pass1_pref_w: Cell<i16>,
    pub(crate) pass3_min_h: Cell<i16>,
    pub(crate) pass3_pref_h: Cell<i16>,
    pub(crate) dirty: bool,
    pub(crate) active_readers: Cell<u8>,
    state_flags: Cell<u8>,
    pub(crate) layout_flags: Cell<u8>,
    heap_ptr: *mut u8,
}

impl PodSlot {
    const fn empty() -> Self {
        Self {
            vtable: None,
            cached_element: Cell::new(core::ptr::null()),
            cached_layout: Cell::new(core::ptr::null_mut()),
            cached_bounds: Cell::new(Region::EMPTY),
            render_count: Cell::new(0),
            generation: 0,
            readers_mask: Cell::new(0),
            pass1_min_w: Cell::new(0),
            pass1_pref_w: Cell::new(0),
            pass3_min_h: Cell::new(0),
            pass3_pref_h: Cell::new(0),
            dirty: false,
            active_readers: Cell::new(0),
            state_flags: Cell::new(0),
            layout_flags: Cell::new(0),
            heap_ptr: core::ptr::null_mut(),
        }
    }

    #[inline(always)]
    pub(super) const fn occupied(&self) -> bool {
        self.vtable.is_some()
    }

    #[inline(always)]
    pub(super) fn is_borrowed(&self) -> bool {
        (self.state_flags.get() & STATE_FLAG_BORROWED) != 0
    }

    #[inline(always)]
    pub(super) fn set_borrowed(&self, val: bool) {
        if val {
            self.state_flags
                .set(self.state_flags.get() | STATE_FLAG_BORROWED);
        } else {
            self.state_flags
                .set(self.state_flags.get() & !STATE_FLAG_BORROWED);
        }
    }

    #[inline(always)]
    pub(super) fn is_expanding(&self) -> bool {
        (self.state_flags.get() & STATE_FLAG_EXPANDING) != 0
    }

    #[inline(always)]
    pub(super) fn set_expanding(&self, val: bool) {
        if val {
            self.state_flags
                .set(self.state_flags.get() | STATE_FLAG_EXPANDING);
        } else {
            self.state_flags
                .set(self.state_flags.get() & !STATE_FLAG_EXPANDING);
        }
    }

    #[inline(always)]
    pub(super) fn set_needs_next_frame(&self, val: bool) {
        if val {
            self.state_flags
                .set(self.state_flags.get() | STATE_FLAG_NEEDS_NEXT_FRAME);
        } else {
            self.state_flags
                .set(self.state_flags.get() & !STATE_FLAG_NEEDS_NEXT_FRAME);
        }
    }

    #[inline(always)]
    fn take_needs_next_frame(&self) -> bool {
        let flags = self.state_flags.get();
        let had = (flags & STATE_FLAG_NEEDS_NEXT_FRAME) != 0;
        if had {
            self.state_flags.set(flags & !STATE_FLAG_NEEDS_NEXT_FRAME);
        }
        had
    }

    #[inline(always)]
    fn reset_runtime_state(&mut self, dirty: bool) {
        self.dirty = dirty;
        self.state_flags.set(0);
        self.layout_flags.set(0);
        self.active_readers.set(0);
        self.readers_mask.set(0);
        self.pass1_min_w.set(0);
        self.pass1_pref_w.set(0);
        self.pass3_min_h.set(0);
        self.pass3_pref_h.set(0);
        self.render_count.set(0);
        self.cached_element.set(core::ptr::null());
        self.cached_layout.set(core::ptr::null_mut());
        self.cached_bounds.set(Region::EMPTY);
    }
}

/// 跨视图只读借用守卫（由 [`FrameScope::read`] 返回）。
///
/// 通过 [`core::ops::Deref`] 将引用生命周期绑定至守卫自身的局部栈生命周期而非帧级 `'a`，
/// 并在守卫存活期间持有目标槽位的 `active_readers` 计数锁，从编译期与运行期双重杜绝将跨视图引用泄漏至 `Element<'a>`。
pub struct PodReadGuard<'s, T: View> {
    value: &'s T,
    readers: &'s Cell<u8>,
}

impl<T: View> core::ops::Deref for PodReadGuard<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: View> Drop for PodReadGuard<'_, T> {
    #[inline(always)]
    fn drop(&mut self) {
        self.readers.set(self.readers.get().saturating_sub(1));
    }
}
/// 组件状态槽位池。
/// 由于 `View` 状态允许包含 `!Send` / `!Sync` 类型（如 `Rc`、`Cell`），`PodPool` 显式标记为 `!Send` 与 `!Sync`。
pub struct PodPool {
    pool_id: Cell<u16>,
    pub(crate) skipped_renders: Cell<u16>,
    pub(crate) relayout_boundary_hits: Cell<u16>,
    pub(super) slots: std::vec::Vec<PodSlot>,
    dirty_queue: SmallVec<PodId, MAX_PODS>,
    /// 本帧是否收到来自外部状态变更（`view_mut` / `Signal` / `resize`）的新脏标记；
    /// 区别于 `clear_dirty()` 仅为下一帧连续动画（`cx.request_frame()`）预约的 `dirty_queue` 项，
    /// 避免软件光栅器在刚结束 `step_frame_dt()` 后误判当前帧缓存失效并击穿 L2 局部重排。
    external_dirty: bool,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Default for PodPool {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PodPool {
    fn drop(&mut self) {
        let pid = self.pool_id.get();
        crate::signal::remove_pool_subscriptions(pid);
        for slot in self.slots.iter_mut() {
            if let Some(vt) = slot.vtable.take() {
                if !slot.heap_ptr.is_null() {
                    let ptr = slot.heap_ptr;
                    slot.heap_ptr = core::ptr::null_mut();
                    unsafe {
                        (vt.drop_fn)(ptr);
                    }
                }
            }
        }
    }
}

impl PodPool {
    /// 创建空的状态槽位池。
    pub const fn new() -> Self {
        Self {
            pool_id: Cell::new(0),
            skipped_renders: Cell::new(0),
            relayout_boundary_hits: Cell::new(0),
            slots: std::vec::Vec::new(),
            dirty_queue: SmallVec::new(),
            external_dirty: false,
            _not_send_sync: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn ensure_pool_id(&self) -> u16 {
        let id = self.pool_id.get();
        if id != 0 {
            id
        } else {
            let next = crate::signal::alloc_pool_id();
            self.pool_id.set(next);
            next
        }
    }

    #[inline(always)]
    pub(super) fn slot_storage_ptr(&self, slot: &PodSlot) -> *mut u8 {
        slot.heap_ptr
    }

    /// 尝试在状态池中无 panic 分配并初始化一个有状态视图 `V`。
    pub fn try_insert<V: View>(&mut self, view: V) -> Option<Handle<V>> {

        unsafe fn render_trampoline<V: View>(
            ptr: *mut u8,
            frame_ptr: *mut FrameScope,
        ) -> Element<'static> {
            unsafe {
                let view = &mut *(ptr as *mut V);
                let scope_ptr = frame_ptr as *mut Scope<V>;
                let el = view.render(&mut *scope_ptr);
                core::mem::transmute::<Element<'_>, Element<'static>>(el)
            }
        }

        unsafe fn drop_trampoline<V>(ptr: *mut u8) {
            unsafe {
                let _ = std::boxed::Box::from_raw(ptr as *mut V);
            }
        }

        let vtable: &'static PodVTable = &PodVTable {
            type_id: TypeId::of::<V>,
            render_fn: render_trampoline::<V>,
            drop_fn: drop_trampoline::<V>,
        };

        if self.slots.is_empty() {
            self.slots.resize_with(MAX_PODS, PodSlot::empty);
        }

        let mut target_idx = self.slots.iter().position(|s| !s.occupied());
        if target_idx.is_none() && self.slots.len() < u16::MAX as usize {
            target_idx = Some(self.slots.len());
            self.slots.push(PodSlot::empty());
        }
        let idx = target_idx?;
        let slot = &mut self.slots[idx];
        let boxed = std::boxed::Box::new(view);
        slot.heap_ptr = std::boxed::Box::into_raw(boxed) as *mut u8;
        slot.vtable = Some(vtable);
        slot.reset_runtime_state(true);
        let generation = slot.generation;
        let pod_id = PodId(idx as u16);
        self.mark_dirty(pod_id);
        Some(Handle::with_generation(pod_id, generation))
    }

    /// 在状态池中分配并初始化一个有状态视图 `V`，返回强类型句柄 `Handle<V>`（携带槽位代号防止 ABA 悬垂引用）。
    pub fn insert<V: View>(&mut self, view: V) -> Handle<V> {
        self.try_insert(view)
            .expect("PodPool slot capacity exhausted")
    }

    /// 销毁并移除指定句柄对应的视图（调用其 `Drop`、递增槽位代号并回收槽位）。
    pub fn remove<V: View>(&mut self, handle: Handle<V>) -> bool {
        let idx = handle.id.0 as usize;
        let Some(slot) = self.slots.get_mut(idx) else {
            return false;
        };
        if slot.is_borrowed() || slot.active_readers.get() > 0 || slot.generation != handle.generation {
            return false;
        }
        let Some(vt) = slot.vtable else {
            return false;
        };
        if (vt.type_id)() != TypeId::of::<V>() {
            return false;
        }
        let vt = slot.vtable.take().unwrap();
        slot.generation = slot.generation.wrapping_add(1);
        slot.reset_runtime_state(false);
        crate::signal::remove_pod_subscriptions(self.pool_id.get(), handle.id);
        if !slot.heap_ptr.is_null() {
            let ptr = slot.heap_ptr;
            slot.heap_ptr = core::ptr::null_mut();
            unsafe {
                (vt.drop_fn)(ptr);
            }
        }
        if let Some(pos) = self.dirty_queue.iter().position(|&x| x == handle.id) {
            self.dirty_queue.remove(pos);
        }
        self.invalidate_all_cached_elements();
        true
    }

    /// 判断给定 `PodId` 当前是否仍驻留在状态池中。
    #[inline]
    pub fn contains_pod(&self, id: PodId) -> bool {
        self.slots
            .get(id.0 as usize)
            .map(|s| s.occupied())
            .unwrap_or(false)
    }

    /// 获取指定句柄对应的可变视图引用（校验代号 `generation`；若该槽位正被借用或已释放则返回 `None`）。
    pub fn get_mut<V: View>(&mut self, handle: Handle<V>) -> Option<&mut V> {
        let idx = handle.id.0 as usize;
        let slot = self.slots.get(idx)?;
        if slot.is_borrowed() || slot.active_readers.get() > 0 || slot.generation != handle.generation {
            return None;
        }
        let vt = slot.vtable?;
        if (vt.type_id)() == TypeId::of::<V>() {
            slot.cached_element.set(core::ptr::null());
            slot.layout_flags
                .set(slot.layout_flags.get() & !LAYOUT_FLAG_SUBTREE_CLEAN);
            let ptr = self.slot_storage_ptr(slot) as *mut V;
            Some(unsafe { &mut *ptr })
        } else {
            None
        }
    }

    /// 通过闭包更新视图状态并自动标记该视图为脏（消除对未初始化结构体对齐字节的裸扫描 UB，同时正确捕获 `Vec`/`String` 等堆内修改）。
    pub fn update<V: View, R>(&mut self, handle: Handle<V>, f: impl FnOnce(&mut V) -> R) -> Option<R> {
        let idx = handle.id.0 as usize;
        let slot = self.slots.get(idx)?;
        if slot.is_borrowed() || slot.active_readers.get() > 0 || slot.generation != handle.generation {
            return None;
        }
        let vt = slot.vtable?;
        if (vt.type_id)() != TypeId::of::<V>() {
            return None;
        }
        slot.set_borrowed(true);
        let ptr = self.slot_storage_ptr(slot);
        let res = f(unsafe { &mut *(ptr as *mut V) });
        slot.set_borrowed(false);
        if size_of::<V>() > 0 {
            self.mark_dirty(handle.id);
        }
        self.flush_signal_dirty_pods();
        Some(res)
    }

    /// 获取指定句柄对应的只读视图引用（校验代号 `generation`；若该槽位正被可变借用或已释放则返回 `None`）。
    pub fn get<V: View>(&self, handle: Handle<V>) -> Option<&V> {
        let idx = handle.id.0 as usize;
        let slot = self.slots.get(idx)?;
        if slot.is_borrowed() || slot.generation != handle.generation {
            return None;
        }
        let vt = slot.vtable?;
        if (vt.type_id)() == TypeId::of::<V>() {
            let ptr = self.slot_storage_ptr(slot) as *const V;
            Some(unsafe { &*ptr })
        } else {
            None
        }
    }

    /// 获取指定句柄对应的受控只读借用守卫 [`PodReadGuard`]（递增 `active_readers` 计数，析构时自动释放）。
    pub fn read_guarded<V: View>(&self, handle: Handle<V>) -> Option<PodReadGuard<'_, V>> {
        let idx = handle.id.0 as usize;
        let slot = self.slots.get(idx)?;
        if slot.is_borrowed() || slot.generation != handle.generation {
            return None;
        }
        let vt = slot.vtable?;
        if (vt.type_id)() == TypeId::of::<V>() {
            slot.active_readers
                .set(slot.active_readers.get().saturating_add(1));
            let ptr = self.slot_storage_ptr(slot) as *const V;
            Some(PodReadGuard {
                value: unsafe { &*ptr },
                readers: &slot.active_readers,
            })
        } else {
            None
        }
    }

    /// 获取并锁定指定 `PodId` 的可变裸指针（使用内部可变性设置 `borrowed = true`，无需 `&mut self`，严格遵循 Stacked Borrows）。
    pub(crate) fn acquire_raw_mut(
        &self,
        id: PodId,
        expected_gen: Option<u16>,
        expected_type: TypeId,
    ) -> Option<*mut u8> {
        let slot = self.slots.get(id.0 as usize)?;
        if slot.is_borrowed() || slot.active_readers.get() > 0 {
            return None;
        }
        if let Some(g) = expected_gen {
            if slot.generation != g {
                return None;
            }
        }
        let vt = slot.vtable?;
        if (vt.type_id)() == expected_type {
            slot.set_borrowed(true);
            Some(self.slot_storage_ptr(slot))
        } else {
            None
        }
    }

    /// 校验指定 `PodId` 当前在用槽位是否匹配类型 `expected_type`，并返回其当前代号 `generation`。
    #[inline]
    pub(crate) fn current_generation_for_type(
        &self,
        id: PodId,
        expected_type: TypeId,
    ) -> Option<u16> {
        let slot = self.slots.get(id.0 as usize)?;
        let vt = slot.vtable?;
        if (vt.type_id)() == expected_type {
            Some(slot.generation)
        } else {
            None
        }
    }

    /// 释放指定 `PodId` 的可变借用锁。
    #[inline]
    pub(crate) fn release_borrow(&self, id: PodId) {
        if let Some(slot) = self.slots.get(id.0 as usize) {
            slot.set_borrowed(false);
        }
    }

    /// 记录渲染期跨视图只读依赖（`reader` 通过 `cx.read(target)` 读取了 `target`）。
    #[inline]
    pub(crate) fn record_cross_read(&self, reader: PodId, target: PodId) {
        let bits = (size_of::<ReadersMask>() * 8) as u16;
        if reader.0 < bits {
            if let Some(slot) = self.slots.get(target.0 as usize) {
                let bit: ReadersMask = 1 << reader.0;
                slot.readers_mask.set(slot.readers_mask.get() | bit);
            }
        }
    }

    /// 将响应式 `Signal` 触发的待标记脏 `PodId` 批量刷入 `PodPool`。
    #[inline]
    pub fn flush_signal_dirty_pods(&mut self) {
        let pid = self.ensure_pool_id();
        crate::signal::drain_pending_dirty_pods(pid, |pod_id| {
            self.mark_dirty(pod_id);
        });
    }

    /// 将指定 `PodId` 标记为脏并压入脏队列（去重，同时级联标记所有通过 `cx.read` 依赖该视图的观察者视图）。
    pub fn mark_dirty(&mut self, id: PodId) {
        let mut cascade_mask: ReadersMask = 0;
        if let Some(slot) = self.slots.get_mut(id.0 as usize) {
            if slot.occupied() {
                slot.dirty = true;
                self.external_dirty = true;
                slot.cached_element.set(core::ptr::null());
                slot.layout_flags
                    .set(slot.layout_flags.get() & !LAYOUT_FLAG_SUBTREE_CLEAN);
                cascade_mask = slot.readers_mask.get();
                if !self.dirty_queue.iter().any(|&x| x == id) {
                    self.dirty_queue.push(id);
                }
            }
        }
        while cascade_mask != 0 {
            let bit = cascade_mask.trailing_zeros() as u16;
            cascade_mask &= cascade_mask - 1;
            let reader_id = PodId(bit);
            if reader_id != id {
                if let Some(r_slot) = self.slots.get_mut(reader_id.0 as usize) {
                    if r_slot.occupied() && !r_slot.dirty {
                        r_slot.dirty = true;
                        r_slot.cached_element.set(core::ptr::null());
                        r_slot
                            .layout_flags
                            .set(r_slot.layout_flags.get() & !LAYOUT_FLAG_SUBTREE_CLEAN);
                        cascade_mask |= r_slot.readers_mask.get();
                        if !self.dirty_queue.iter().any(|&x| x == reader_id) {
                            self.dirty_queue.push(reader_id);
                        }
                    }
                }
            }
        }
    }

    /// 失效所有槽位中缓存的 `Element` 树与 `LayoutNode` 指针（当 `ScratchArena` 发生 `rewind` 或扩容切换时调用）。
    #[inline]
    pub fn invalidate_all_cached_elements(&self) {
        for slot in self.slots.iter() {
            slot.cached_element.set(core::ptr::null());
            slot.cached_layout.set(core::ptr::null_mut());
            slot.layout_flags.set(0);
        }
    }

    /// 将所有在用槽位标记为脏并失效其 `Element` 与 `LayoutNode` 缓存（当视口尺寸、主题或安全区域变更时调用）。
    pub fn mark_all_dirty(&mut self) {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if slot.occupied() {
                slot.dirty = true;
                self.external_dirty = true;
                slot.cached_element.set(core::ptr::null());
                slot.cached_layout.set(core::ptr::null_mut());
                slot.layout_flags.set(0);
                let id = PodId(idx as u16);
                if !self.dirty_queue.iter().any(|&x| x == id) {
                    self.dirty_queue.push(id);
                }
            }
        }
    }

    /// 获取指定句柄对应视图累计执行 `View::render` 的次数。
    #[inline]
    pub fn render_count(&self, id: PodId) -> u32 {
        self.slots
            .get(id.0 as usize)
            .map(|s| s.render_count.get())
            .unwrap_or(0)
    }

    /// 获取自创建以来因未脏而直接复用缓存 `Element` 跳过 `View::render` 的子视图次数。
    #[inline]
    pub fn skipped_render_count(&self) -> u32 {
        self.skipped_renders.get() as u32
    }

    /// 获取自创建以来命中子视图布局隔离边界（Relayout Boundary / 跳过子树 Pass 1~3 或原地局部重排）的次数。
    #[inline]
    pub fn relayout_boundary_hits(&self) -> u32 {
        self.relayout_boundary_hits.get() as u32
    }

    /// 判断指定 `PodId` 是否处于脏状态。
    #[inline]
    pub fn is_dirty(&self, id: PodId) -> bool {
        self.slots
            .get(id.0 as usize)
            .map(|s| s.occupied() && s.dirty)
            .unwrap_or(false)
    }

    /// 是否存在任何待重绘的脏视图（包含外部变更与 `cx.request_frame()` 预约的下一帧动画）。
    #[inline]
    pub fn has_dirty(&self) -> bool {
        !self.dirty_queue.is_empty()
    }

    /// 是否存在尚未被当前帧消化的外部状态脏标记（不含仅为下一帧预约的 `cx.request_frame()`）。
    #[inline]
    pub fn has_external_dirty(&self) -> bool {
        self.external_dirty
    }

    /// 是否存在至少一个未脏且已缓存了 `Element` 树的在用视图槽位。
    #[inline]
    pub fn has_any_clean_occupied(&self) -> bool {
        self.slots
            .iter()
            .any(|s| s.occupied() && !s.dirty && !s.cached_element.get().is_null())
    }

    /// 获取当前脏视图队列切片。
    #[inline]
    pub fn dirty_pods(&self) -> &[PodId] {
        self.dirty_queue.as_slice()
    }

    /// 读取指定 `PodId` 上一帧缓存的几何边界框。
    #[inline]
    pub fn cached_bounds(&self, id: PodId) -> Region {
        self.slots
            .get(id.0 as usize)
            .map(|s| s.cached_bounds.get())
            .unwrap_or(Region::EMPTY)
    }

    /// 更新指定 `PodId` 的缓存几何边界框。
    #[inline]
    pub fn set_cached_bounds(&mut self, id: PodId, bounds: Region) {
        if let Some(slot) = self.slots.get_mut(id.0 as usize) {
            slot.cached_bounds.set(bounds);
        }
    }

    /// 清空当前帧已完成渲染的脏标记（若某个视图在渲染期间调用了 `cx.request_frame()`，则自动保留其下一帧脏标记）。
    pub fn clear_dirty(&mut self) {
        self.dirty_queue.clear();
        self.external_dirty = false;
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if slot.occupied() && slot.take_needs_next_frame() {
                slot.dirty = true;
                slot.cached_element.set(core::ptr::null());
                slot.layout_flags
                    .set(slot.layout_flags.get() & !LAYOUT_FLAG_SUBTREE_CLEAN);
                self.dirty_queue.push(PodId(idx as u16));
            } else {
                slot.dirty = false;
            }
        }
    }

}
