//! 自适应序列容器 `SmallVec<T, N>` 与自适应字符串 `SmallString<N>`。

use core::ops::{Deref, DerefMut};


/// 自适应序列容器：
/// - 桌面/移动/Web 下编译为动态扩容 `std::vec::Vec<T>`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SmallVec<T, const N: usize> {
    inner: std::vec::Vec<T>,
}

impl<T, const N: usize> Default for SmallVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> SmallVec<T, N> {
    /// 创建一个空容器。
    #[inline]
    pub const fn new() -> Self {
        {
            Self {
                inner: std::vec::Vec::new(),
            }
        }
    }

    /// 向尾部压入一个元素。
    #[inline]
    pub fn push(&mut self, value: T) -> bool {
        {
            self.inner.push(value);
            true
        }
    }

    /// 弹出尾部元素。
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    /// 清空容器。
    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// 获取切片引用。
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self.inner.as_slice()
    }

    /// 获取可变切片引用。
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.inner.as_mut_slice()
    }

    /// 在指定索引插入一个元素（后续元素右移）。
    #[inline]
    pub fn insert(&mut self, index: usize, value: T) -> bool {
        {
            self.inner.insert(index, value);
            true
        }
    }

    /// 批量追加切片副本（`T: Copy`）。
    #[inline]
    pub fn extend_from_slice(&mut self, slice: &[T]) -> bool
    where
        T: Copy,
    {
        {
            self.inner.extend_from_slice(slice);
            true
        }
    }

    /// 按谓词原地保留元素。
    #[inline]
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
        self.inner.retain(|v| f(v));
    }

    /// 按索引移除并返回元素（保持顺序）。
    #[inline]
    pub fn remove(&mut self, index: usize) -> T {
        self.inner.remove(index)
    }

    /// 交换移除指定位置元素（$O(1)$）。
    #[inline]
    pub fn swap_remove(&mut self, index: usize) -> T {
        self.inner.swap_remove(index)
    }

    /// 截断到指定长度。
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        self.inner.truncate(len);
    }
}

/// 自适应 UTF-8 字符串容器。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct SmallString<const N: usize> {
    inner: std::string::String,
}

impl<const N: usize> SmallString<N> {
    #[inline]
    pub const fn new() -> Self {
        {
            Self {
                inner: std::string::String::new(),
            }
        }
    }

    #[inline]
    pub fn from_str_trunc(s: &str) -> Self {
        let mut out = Self::new();
        out.push_str_trunc(s);
        out
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        {
            self.inner.as_str()
        }
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    #[inline]
    pub fn push(&mut self, ch: char) -> bool {
        {
            self.inner.push(ch);
            true
        }
    }

    /// 追加字符串切片。
    pub fn push_str_trunc(&mut self, s: &str) -> usize {
        {
            self.inner.push_str(s);
            s.len()
        }
    }

    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        let mut cut = new_len.min(self.len());
        while cut > 0 && !self.as_str().is_char_boundary(cut) {
            cut -= 1;
        }
        self.inner.truncate(cut);
    }

    /// 在字符边界 `at` 处拆分，保留 `[..at]` 并返回 `[at..]`。
    pub fn split_off(&mut self, at: usize) -> Self {
        let mut idx = at.min(self.len());
        while idx > 0 && !self.as_str().is_char_boundary(idx) {
            idx -= 1;
        }
        {
            Self {
                inner: self.inner.split_off(idx),
            }
        }
    }

    /// 替换字节区间 `start..end` 为 `replacement`（自动对齐 UTF-8 字符边界），返回实际写入字节数。
    pub fn replace_range(&mut self, start: usize, end: usize, replacement: &str) -> usize {
        let mut s = start.min(self.len());
        while s > 0 && !self.as_str().is_char_boundary(s) {
            s -= 1;
        }
        let mut e = end.min(self.len()).max(s);
        while e > s && !self.as_str().is_char_boundary(e) {
            e -= 1;
        }
        {
            self.inner.replace_range(s..e, replacement);
            replacement.len()
        }
    }
}

impl<const N: usize> Deref for SmallString<N> {
    type Target = str;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl<const N: usize> AsRef<str> for SmallString<N> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<const N: usize> From<&str> for SmallString<N> {
    #[inline]
    fn from(s: &str) -> Self {
        Self::from_str_trunc(s)
    }
}

impl<T, const N: usize> Deref for SmallVec<T, N> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.as_slice()
    }
}

impl<T, const N: usize> DerefMut for SmallVec<T, N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut_slice()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a SmallVec<T, N> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}
