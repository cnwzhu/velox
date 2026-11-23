//! 零拷贝文本引用 `StrRef<'a>` 与帧内格式化宏 `fmt!`。

use core::ops::Deref;

/// 零堆分配文本引用：同时支持 Flash 静态字符串与 `ScratchArena` 帧内动态格式化文本。
#[derive(Copy, Clone, Eq, Debug)]
pub enum StrRef<'a> {
    /// 编译期静态字符串切片。
    Static(&'static str),
    /// 分配在当前帧 `ScratchArena` 中的瞬态字符串切片。
    Scratch(&'a str),
}

impl<'a> StrRef<'a> {
    /// 获取底层 `&'a str` 切片。
    #[inline(always)]
    pub const fn as_str(&self) -> &'a str {
        match *self {
            StrRef::Static(s) => s,
            StrRef::Scratch(s) => s,
        }
    }
}

impl<'a> PartialEq for StrRef<'a> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<'a> Deref for StrRef<'a> {
    type Target = str;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl<'a> core::fmt::Display for StrRef<'a> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'a> From<&'a str> for StrRef<'a> {
    #[inline(always)]
    fn from(s: &'a str) -> Self {
        StrRef::Scratch(s)
    }
}

/// 帧内零堆分配格式化宏：直接将 `format_args!` 字节流写入 `ScratchArena` 当前游标处。
#[macro_export]
macro_rules! fmt {
    ($cx:expr, $($arg:tt)*) => {
        $cx.arena().alloc_fmt(core::format_args!($($arg)*))
    };
}
