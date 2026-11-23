//! 状态槽位唯一标识符 `PodId` 与强类型句柄 `Handle<T>`。

use core::marker::PhantomData;

/// 状态槽位唯一标识符（2 字节）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct PodId(pub u16);

/// 常驻 `PodPool` 的强类型组件句柄（4 字节：2 字节 `PodId` + 2 字节代号 `generation`，防止 ABA 释放后重用旧句柄误指向新实例）。
#[repr(C)]
pub struct Handle<T> {
    pub(crate) id: PodId,
    pub(crate) generation: u16,
    _marker: PhantomData<T>,
}

impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> PartialEq for Handle<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.generation == other.generation
    }
}
impl<T> Eq for Handle<T> {}

impl<T> core::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Handle")
            .field("id", &self.id)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T> Handle<T> {
    /// 创建一个指向给定 `PodId`（默认第 0 代）的强类型句柄。
    #[inline(always)]
    pub const fn new(id: PodId) -> Self {
        Self {
            id,
            generation: 0,
            _marker: PhantomData,
        }
    }

    /// 创建一个带有显式代号 `generation` 的强类型句柄。
    #[inline(always)]
    pub const fn with_generation(id: PodId, generation: u16) -> Self {
        Self {
            id,
            generation,
            _marker: PhantomData,
        }
    }

    /// 获取底层 `PodId`。
    #[inline(always)]
    pub const fn pod_id(self) -> PodId {
        self.id
    }

    /// 获取槽位代号 `generation`。
    #[inline(always)]
    pub const fn generation(self) -> u16 {
        self.generation
    }
}

impl<T: crate::element::View> Handle<T> {
    /// 在前景异步协程中安全更新当前句柄指向的视图状态（与 GPUI `WeakEntity::update` 对齐）。
    ///
    /// 自动校验 ABA `generation`、防重入、标脏当前视图并刷新 `Signal` 依赖；若视图已卸载则安全返回 `None`。
    #[inline]
    pub fn update<R>(
        self,
        cx: &mut crate::async_rt::AsyncApp,
        f: impl FnOnce(&mut T, &mut super::ActionScope) -> R,
    ) -> Option<R> {
        cx.update(self, f)
    }

    /// 在前景异步协程中只读访问当前句柄指向的视图状态（若视图已卸载则安全返回 `None`）。
    #[inline]
    pub fn read<R>(
        self,
        cx: &crate::async_rt::AsyncApp,
        f: impl FnOnce(&T) -> R,
    ) -> Option<R> {
        cx.read(self, f)
    }
}

