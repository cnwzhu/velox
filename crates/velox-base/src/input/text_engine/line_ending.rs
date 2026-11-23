//! 文本行尾结束符规范（[`LineEnding`]）与零分配多行切分迭代器。
//!
//! 参照 `pop-os/cosmic-text/src/line_ending.rs` 设计，完整兼容 `#![no_std]`。

/// 单行文本的行尾结束符类型（对应 `cosmic-text` `LineEnding`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineEnding {
    /// Unix / macOS 标准换行符 `'\n'` (`LF`)。
    #[default]
    Lf,
    /// Windows 标准换行符 `"\r\n"` (`CRLF`)。
    CrLf,
    /// 经典回车换行符 `'\r'` (`CR`)。
    Cr,
    /// 罕见反向换行符 `"\n\r"` (`LFCR`)。
    LfCr,
    /// 无换行符（文档最后一行）。
    None,
}

impl LineEnding {
    /// 返回当前行尾结束符对应的静态 UTF-8 字符串切片。
    #[inline(always)]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
            Self::LfCr => "\n\r",
            Self::None => "",
        }
    }

    /// 返回行尾结束符占用的字节数（`0..=2`）。
    #[inline(always)]
    pub const fn byte_len(self) -> usize {
        self.as_str().len()
    }

    /// 是否带有显式换行符（非 `LineEnding::None`）。
    #[inline(always)]
    pub const fn has_ending(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// 零分配多行切分迭代器：将任意 UTF-8 字符串按行尾约定切分为 `(line_without_ending, LineEnding)`。
///
/// 遵循 `cosmic-text` 语义：
/// - 空字符串 `""` 产出 1 条 `("", LineEnding::None)`；
/// - 以 `\n` 结尾的字符串 `"a\n"` 产出 `("a", LineEnding::Lf)` 与末尾空行 `("", LineEnding::None)`。
pub struct LineEndingIter<'a> {
    rest: &'a str,
    finished: bool,
}

impl<'a> LineEndingIter<'a> {
    #[inline]
    pub const fn new(text: &'a str) -> Self {
        Self {
            rest: text,
            finished: false,
        }
    }
}

impl<'a> Iterator for LineEndingIter<'a> {
    type Item = (&'a str, LineEnding);

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let bytes = self.rest.as_bytes();
        let len = bytes.len();
        let mut i = 0usize;
        while i < len {
            match bytes[i] {
                b'\r' => {
                    let line = &self.rest[..i];
                    if i + 1 < len && bytes[i + 1] == b'\n' {
                        self.rest = &self.rest[i + 2..];
                        return Some((line, LineEnding::CrLf));
                    } else {
                        self.rest = &self.rest[i + 1..];
                        return Some((line, LineEnding::Cr));
                    }
                }
                b'\n' => {
                    let line = &self.rest[..i];
                    if i + 1 < len && bytes[i + 1] == b'\r' {
                        self.rest = &self.rest[i + 2..];
                        return Some((line, LineEnding::LfCr));
                    } else {
                        self.rest = &self.rest[i + 1..];
                        return Some((line, LineEnding::Lf));
                    }
                }
                _ => i += 1,
            }
        }
        self.finished = true;
        Some((self.rest, LineEnding::None))
    }
}

/// 将任意 UTF-8 文档切片按行尾结束符切分为 `(line_text, LineEnding)` 序列。
#[inline]
pub const fn split_lines_with_ending(text: &str) -> LineEndingIter<'_> {
    LineEndingIter::new(text)
}
