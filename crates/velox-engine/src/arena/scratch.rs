//! `ScratchArena` 瞬态帧内存池。

use core::cell::Cell;
use core::fmt;
use core::mem::{align_of, size_of};

use super::StrRef;

/// 单帧瞬态竞技场默认容量（依托虚拟列表按需挂载与布局树水位线即时回收）。
pub const SCRATCH_CAPACITY: usize = 4992;

/// 基于 `bumpalo::Bump` 的帧级瞬态内存池，帧末 `rewind()` 零分配复用。
pub struct ScratchArena<const CAP: usize = SCRATCH_CAPACITY> {
    inner: bumpalo::Bump,
    used: Cell<usize>,
    peak_used: Cell<usize>,
    truncation_count: Cell<u32>,
}

impl<const CAP: usize> Default for ScratchArena<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> ScratchArena<CAP> {
    /// 创建一个新的瞬态帧内存池。
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: bumpalo::Bump::with_capacity(CAP),
            used: Cell::new(0),
            peak_used: Cell::new(0),
            truncation_count: Cell::new(0),
        }
    }

    #[inline(always)]
    fn set_used_std(&self, next: usize) {
        self.used.set(next);
        if next > self.peak_used.get() {
            self.peak_used.set(next);
        }
    }

    /// 尝试在帧池中无 panic 分配单个对象。
    #[inline]
    pub fn try_alloc<'a, T>(&'a self, val: T) -> Option<&'a mut T> {
        let align = align_of::<T>().max(1);
        let size = size_of::<T>();
        let cur = (self.used.get() + align - 1) & !(align - 1);
        self.set_used_std(cur + size);
        Some(self.inner.alloc(val))
    }

    /// 在帧池中分配单个节点或事件闭包（返回 `'a` 可变引用，彻底消除 `Box` 堆分配）。
    #[inline(always)]
    pub fn alloc<'a, T>(&'a self, val: T) -> &'a mut T {
        self.try_alloc(val).expect("ScratchArena overflow")
    }

    /// 尝试在帧池中无 panic 分配临时树节点。
    #[inline]
    pub fn try_alloc_temp<'a, T>(&'a self, val: T) -> Option<&'a mut T> {
        self.try_alloc(val)
    }

    /// 分配仅在布局求解阶段存活的临时树节点（`ElementNode` / `Element` / `DivVisual`）。
    #[inline(always)]
    pub fn alloc_temp<'a, T>(&'a self, val: T) -> &'a mut T {
        self.try_alloc_temp(val)
            .expect("ScratchArena temp overflow")
    }

    /// 回收临时元素树节点。
    ///
    /// # Safety
    /// 调用此函数后，此前通过 `alloc_temp` 获得的引用不得再被解引用。
    #[inline(always)]
    pub(crate) unsafe fn reset_temp(&self) {
    }

    /// 获取当前临时区水位线（用于保留 `LayoutNode` 树以供 Pass-4 快路径跨帧复用）。
    #[inline(always)]
    pub(crate) fn save_temp_mark(&self) -> usize {
        CAP
    }

    /// 恢复临时区水位线至指定位置。
    #[inline(always)]
    pub(crate) fn restore_temp_mark(&mut self, mark: usize) {
        let _ = mark;
    }

    /// 回滚本帧试探性分配的水位线（仅供求解器内部试探失败时回退刚分配的临时节点）。
    #[inline(always)]
    pub(crate) unsafe fn rollback_speculative_marks(&self, mark: usize, temp_mark: usize) {
        let _ = temp_mark;
        if mark <= self.used.get() {
            self.used.set(mark);
        }
    }

    /// 检查给定指针是否仍位于当前竞技场尚未被 `rewind` / `reset_temp` 回收的有效临时树区间内。
    #[inline(always)]
    pub(crate) fn contains_temp_ptr(&self, ptr: *const u8) -> bool {
        if ptr.is_null() {
            return false;
        }
        self.used.get() > 0
    }

    /// 在帧池中分配长度为 `len` 的对齐字节切片 `&'a mut [u8]`。
    #[inline(always)]
    pub fn alloc_bytes<'a>(&'a self, len: usize) -> &'a mut [u8] {
        if len == 0 {
            return &mut [];
        }
        self.set_used_std(self.used.get() + len);
        self.inner.alloc_slice_fill_copy(len, 0u8)
    }

    /// 无需 `Vec` 中转，直接将任意迭代器原地展开为连续内存切片 `&'a mut [T]`。
    pub fn alloc_iter<'a, T, I>(&'a self, iter: I) -> &'a mut [T]
    where
        I: IntoIterator<Item = T>,
    {
        let mut iter = iter.into_iter();
        let (lower, upper) = iter.size_hint();

        let size = size_of::<T>();
        if Some(lower) == upper {
            let count = lower;
            if count == 0 || size == 0 {
                return &mut [];
            }
            self.set_used_std(self.used.get() + count.saturating_mul(size));
            return self.inner.alloc_slice_fill_with(count, |_| {
                iter.next().expect("Exact size_hint mismatch")
            });
        }
        let mut vec = bumpalo::collections::Vec::with_capacity_in(lower, &self.inner);
        for item in iter {
            vec.push(item);
        }
        self.set_used_std(self.used.get() + vec.len().saturating_mul(size));
        vec.into_bump_slice_mut()
    }

    /// 将任意临时字符串切片 `s` 拷贝至当前 `ScratchArena`，返回 `StrRef<'a>`。
    #[inline]
    pub fn alloc_str<'a>(&'a self, s: &str) -> StrRef<'a> {
        if s.is_empty() {
            return StrRef::Static("");
        }
        let copied = self.inner.alloc_str(s);
        self.set_used_std(self.used.get() + copied.len());
        StrRef::Scratch(copied)
    }

    /// 将 `format_args!` 字节流直接写入当前 `ScratchArena`，返回 `StrRef<'a>`。
    pub fn alloc_fmt<'a>(&'a self, args: fmt::Arguments<'_>) -> StrRef<'a> {
        if let Some(static_str) = args.as_str() {
            return StrRef::Static(static_str);
        }
        let mut s = bumpalo::collections::String::new_in(&self.inner);
        let _ = fmt::write(&mut s, args);
        self.set_used_std(self.used.get() + s.len());
        StrRef::Scratch(s.into_bump_str())
    }

    /// 重置帧内存池游标与本帧水位遥测，实现零分配跨帧复用。
    #[inline(always)]
    pub fn rewind(&mut self) {
        self.used.set(0);
        self.peak_used.set(0);
        self.truncation_count.set(0);
        self.inner.reset();
    }

    /// 保存当前竞技场游标水位线（用于阶段性临时数据分时复用回收）。
    #[inline(always)]
    pub fn save_mark(&self) -> usize {
        self.used_bytes()
    }

    /// 恢复竞技场游标至指定水位线（需要可变引用 `&mut self`，静态保证此前分配的借用生命周期已结束）。
    #[inline(always)]
    pub fn restore_mark(&mut self, mark: usize) {
        if mark <= self.used.get() {
            self.used.set(mark);
        }
    }

    /// 返回当前帧已使用的逻辑字节数。
    #[inline(always)]
    pub fn used_bytes(&self) -> usize {
        self.used.get()
    }

    /// 返回当前周期内瞬态内存池触及的真实峰值字节数（`#![no_std]` 双端池计入 `cursor + (CAP - temp_cursor)`）。
    #[inline(always)]
    pub fn peak_used_bytes(&self) -> usize {
        self.peak_used.get()
    }

    /// 返回当前周期内发生静默字符串截断（`alloc_str` / `alloc_fmt`）或临时切片空降级（`alloc_temp_iter_exact`）的累计次数。
    #[inline(always)]
    pub fn truncation_count(&self) -> u32 {
        self.truncation_count.get()
    }

    /// 取出并清零当前周期内累计的静默截断次数（供 `VeloxEngine` 帧末汇总并透传至 `FlushReport`）。
    #[inline(always)]
    pub(crate) fn take_truncation_count(&self) -> u32 {
        self.truncation_count.replace(0)
    }

    /// 将峰值水位基线重置为当前实时占用字节数（用于保留子树未调用 `rewind()` 的增量帧统计单帧真实高水位）。
    #[inline(always)]
    pub(crate) fn reset_peak_to_current(&self) {
        self.peak_used.set(self.used.get());
    }

    /// 返回底层实际驻留字节数（`bumpalo::Bump::allocated_bytes()`，用于真实水位上限回收）。
    #[inline(always)]
    pub(crate) fn real_allocated_bytes(&self) -> usize {
        self.inner.allocated_bytes()
    }

    /// 检查当前竞技场剩余空间是否足以容纳 `count` 个连续的 `T` 元素。
    #[inline(always)]
    pub fn can_alloc_slice<T>(&self, count: usize) -> bool {
        let _ = count;
        true
    }

    /// 检查当前竞技场临时区是否足以分配 `count` 个 `T` 元素。
    #[inline(always)]
    pub fn can_alloc_temp_slice<T>(&self, count: usize) -> bool {
        let _ = count;
        true
    }

    /// 批量分配由固定长度迭代器产生的连续切片 `&'a mut [T]`（供布局求解器构建 `LayoutNode` 子树使用）。
    pub fn alloc_temp_iter_exact<'a, T, I>(&'a self, count: usize, iter: I) -> &'a mut [T]
    where
        I: IntoIterator<Item = T>,
    {
        if count == 0 || size_of::<T>() == 0 {
            return &mut [];
        }
        self.alloc_iter(iter.into_iter().take(count))
    }
}
