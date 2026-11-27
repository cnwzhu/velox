//! 信号图线程局部/`#![no_std]` 追踪上下文槽、零堆内联存储与编译期默认等值比较器。

use core::any::TypeId;
use core::mem::{align_of, size_of};
use core::sync::atomic::{AtomicU16, Ordering};

pub(super) const NO_ACTIVE_POD: u16 = u16::MAX;
pub(super) const NO_ACTIVE_TRACKER: u16 = u16::MAX;
pub(super) const TRACKER_KIND_COMPUTED: u16 = 0x4000;
pub(super) const TRACKER_KIND_EFFECT: u16 = 0x8000;
pub(super) const TRACKER_INDEX_MASK: u16 = 0x3FFF;
/// 侵入式依赖边池索引类型（`std` 下动态扩容，使用 `u32`）。
pub(super) type EdgeIdx = u32;
pub(super) const NO_EDGE: EdgeIdx = u32::MAX;

static NEXT_POOL_ID: AtomicU16 = AtomicU16::new(1);


std::thread_local! {
    static TLS_HAS_PENDING_DIRTY: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    static TLS_ACTIVE_RENDER_POD: core::cell::Cell<u16> = const { core::cell::Cell::new(NO_ACTIVE_POD) };
    static TLS_ACTIVE_RENDER_POOL: core::cell::Cell<u16> = const { core::cell::Cell::new(0) };
    static TLS_ACTIVE_DEP_TRACKER: core::cell::Cell<u16> = const { core::cell::Cell::new(NO_ACTIVE_TRACKER) };
    static TLS_BATCH_DEPTH: core::cell::Cell<u16> = const { core::cell::Cell::new(0) };
    static TLS_BATCH_PENDING_EFFECTS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

#[inline(always)]
pub(super) fn get_has_pending_dirty() -> bool {
    {
        TLS_HAS_PENDING_DIRTY.with(|c| c.get())
    }
}

#[inline(always)]
pub(super) fn set_has_pending_dirty(val: bool) {
    {
        TLS_HAS_PENDING_DIRTY.with(|c| c.set(val));
    }
}

#[inline(always)]
pub(super) fn get_active_render_context() -> (u16, u16) {
    {
        (
            TLS_ACTIVE_RENDER_POOL.with(|c| c.get()),
            TLS_ACTIVE_RENDER_POD.with(|c| c.get()),
        )
    }
}

#[inline(always)]
pub(super) fn swap_active_render_context(pool_id: u16, pod_idx: u16) -> (u16, u16) {
    {
        let prev_pool = TLS_ACTIVE_RENDER_POOL.with(|c| c.replace(pool_id));
        let prev_pod = TLS_ACTIVE_RENDER_POD.with(|c| c.replace(pod_idx));
        (prev_pool, prev_pod)
    }
}

#[inline(always)]
pub(super) fn get_active_dep_tracker() -> u16 {
    {
        TLS_ACTIVE_DEP_TRACKER.with(|c| c.get())
    }
}

#[inline(always)]
pub(super) fn swap_active_dep_tracker(val: u16) -> u16 {
    {
        TLS_ACTIVE_DEP_TRACKER.with(|c| c.replace(val))
    }
}

#[inline(always)]
pub(super) fn get_batch_depth() -> u16 {
    {
        TLS_BATCH_DEPTH.with(|c| c.get())
    }
}

#[inline(always)]
pub(super) fn inc_batch_depth() {
    {
        TLS_BATCH_DEPTH.with(|c| c.set(c.get().saturating_add(1)));
    }
}

#[inline(always)]
pub(super) fn dec_batch_depth() -> u16 {
    {
        TLS_BATCH_DEPTH.with(|c| {
            let next = c.get().saturating_sub(1);
            c.set(next);
            next
        })
    }
}

#[inline(always)]
pub(super) fn push_batch_effects(mask: u32) {
    {
        TLS_BATCH_PENDING_EFFECTS.with(|c| c.set(c.get() | mask));
    }
}

#[inline(always)]
pub(super) fn take_batch_effects() -> u32 {
    {
        TLS_BATCH_PENDING_EFFECTS.with(|c| c.replace(0))
    }
}

/// 为每个 `PodPool` 分配唯一的非零标识（用于多实例 / 多小程序隔离信号订阅）。
#[inline]
pub(crate) fn alloc_pool_id() -> u16 {
    loop {
        let id = NEXT_POOL_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

#[repr(C, align(8))]
#[derive(Copy, Clone)]
pub(super) struct InlineValueStorage {
    words: [u64; 2],
}

impl InlineValueStorage {
    pub(super) const ZERO: Self = Self { words: [0; 2] };

    #[inline(always)]
    pub(super) unsafe fn write<T: Copy>(&mut self, val: T) {
        assert!(
            size_of::<T>() <= size_of::<Self>() && align_of::<T>() <= align_of::<Self>(),
            "Signal<T> inline storage requires size_of::<T>() <= 16 and align_of::<T>() <= 8"
        );
        self.words = [0; 2];
        unsafe {
            core::ptr::write(self.words.as_mut_ptr() as *mut T, val);
        }
    }

    #[inline(always)]
    pub(super) unsafe fn read<T: Copy>(&self) -> T {
        unsafe { core::ptr::read(self.words.as_ptr() as *const T) }
    }
}

#[repr(C, align(8))]
#[derive(Copy, Clone)]
pub(super) struct InlineFnStorage {
    words: [u64; 4],
}

impl InlineFnStorage {
    pub(super) const ZERO: Self = Self { words: [0; 4] };

    #[inline(always)]
    pub(super) unsafe fn write<F: Copy>(&mut self, val: F) {
        assert!(
            size_of::<F>() <= size_of::<Self>() && align_of::<F>() <= align_of::<Self>(),
            "Computed/Effect closure inline storage requires size_of::<F>() <= 32 and align_of::<F>() <= 8"
        );
        self.words = [0; 4];
        unsafe {
            core::ptr::write(self.words.as_mut_ptr() as *mut F, val);
        }
    }
}

/// 针对常见内置标量 / 短元组的零 UB 默认等值比较器（编译期单态化折叠为单一函数指针），
/// 让标准 [`computed`] 无需改动签名即可开箱支持 Early Cutoff 剪枝。
pub(super) fn default_eq_fn<T: Copy + 'static>(
) -> Option<unsafe fn(*const InlineValueStorage, *const InlineValueStorage) -> bool> {
    unsafe fn eq_pod<U: Copy + PartialEq>(
        a: *const InlineValueStorage,
        b: *const InlineValueStorage,
    ) -> bool {
        let va = unsafe { (*a).read::<U>() };
        let vb = unsafe { (*b).read::<U>() };
        va == vb
    }
    unsafe fn eq_f32(a: *const InlineValueStorage, b: *const InlineValueStorage) -> bool {
        let va = unsafe { (*a).read::<f32>() };
        let vb = unsafe { (*b).read::<f32>() };
        va.to_bits() == vb.to_bits()
    }
    unsafe fn eq_f64(a: *const InlineValueStorage, b: *const InlineValueStorage) -> bool {
        let va = unsafe { (*a).read::<f64>() };
        let vb = unsafe { (*b).read::<f64>() };
        va.to_bits() == vb.to_bits()
    }
    unsafe fn eq_f32_pair(a: *const InlineValueStorage, b: *const InlineValueStorage) -> bool {
        let va = unsafe { (*a).read::<(f32, f32)>() };
        let vb = unsafe { (*b).read::<(f32, f32)>() };
        va.0.to_bits() == vb.0.to_bits() && va.1.to_bits() == vb.1.to_bits()
    }

    let tid = TypeId::of::<T>();
    if tid == TypeId::of::<bool>() {
        Some(eq_pod::<bool>)
    } else if tid == TypeId::of::<u8>() {
        Some(eq_pod::<u8>)
    } else if tid == TypeId::of::<u16>() {
        Some(eq_pod::<u16>)
    } else if tid == TypeId::of::<u32>() {
        Some(eq_pod::<u32>)
    } else if tid == TypeId::of::<u64>() {
        Some(eq_pod::<u64>)
    } else if tid == TypeId::of::<u128>() {
        Some(eq_pod::<u128>)
    } else if tid == TypeId::of::<usize>() {
        Some(eq_pod::<usize>)
    } else if tid == TypeId::of::<i8>() {
        Some(eq_pod::<i8>)
    } else if tid == TypeId::of::<i16>() {
        Some(eq_pod::<i16>)
    } else if tid == TypeId::of::<i32>() {
        Some(eq_pod::<i32>)
    } else if tid == TypeId::of::<i64>() {
        Some(eq_pod::<i64>)
    } else if tid == TypeId::of::<i128>() {
        Some(eq_pod::<i128>)
    } else if tid == TypeId::of::<isize>() {
        Some(eq_pod::<isize>)
    } else if tid == TypeId::of::<char>() {
        Some(eq_pod::<char>)
    } else if tid == TypeId::of::<f32>() {
        Some(eq_f32)
    } else if tid == TypeId::of::<f64>() {
        Some(eq_f64)
    } else if tid == TypeId::of::<(i32, i32)>() {
        Some(eq_pod::<(i32, i32)>)
    } else if tid == TypeId::of::<(u32, u32)>() {
        Some(eq_pod::<(u32, u32)>)
    } else if tid == TypeId::of::<(i16, i16)>() {
        Some(eq_pod::<(i16, i16)>)
    } else if tid == TypeId::of::<(u16, u16)>() {
        Some(eq_pod::<(u16, u16)>)
    } else if tid == TypeId::of::<(f32, f32)>() {
        Some(eq_f32_pair)
    } else {
        None
    }
}
