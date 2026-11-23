//! 文本字符分类、词边界（Word Boundary）与行边界（Line Boundary）定位算法（兼容 `#![no_std]`）。

use core::ops::Range;

/// 文本字符词法分类（用于双击选词与 `Ctrl+Left` / `Ctrl+Right` 按词跳转）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharacterKind {
    /// 标识符/单词字符（ASCII 字母数字、下划线、拉丁/西里尔扩展字符及 CJK 汉字）。
    Word,
    /// 非换行空白字符（空格、制表符等）。
    Whitespace,
    /// 换行符（`\n` 或 `\r`）。
    Newline,
    /// 标点符号或其他分隔符。
    Other,
}

impl From<char> for CharacterKind {
    #[inline]
    fn from(character: char) -> Self {
        if character == '_'
            || character.is_ascii_alphanumeric()
            || matches!(
                character,
                '\u{00C0}'..='\u{024F}'
                    | '\u{0400}'..='\u{04FF}'
                    | '\u{1E00}'..='\u{1EFF}'
                    | '\u{0300}'..='\u{036F}'
                    | '\u{4E00}'..='\u{9FFF}'
                    | '\u{3040}'..='\u{30FF}'
            )
        {
            Self::Word
        } else if matches!(character, '\n' | '\r') {
            Self::Newline
        } else if character.is_whitespace() {
            Self::Whitespace
        } else {
            Self::Other
        }
    }
}

/// 将任意字节偏移量向左对齐到最近的合法 UTF-8 字符边界。
#[inline]
pub fn clip_offset_left(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// 将任意字节偏移量向右对齐到最近的合法 UTF-8 字符边界。
#[inline]
pub fn clip_offset_right(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

pub(crate) fn word_range_from_chars(
    offset: usize,
    character: char,
    previous: impl Iterator<Item = char>,
    following: impl Iterator<Item = char>,
) -> Range<usize> {
    let kind = CharacterKind::from(character);
    let connects = |next: char| {
        matches!(
            (kind, CharacterKind::from(next)),
            (CharacterKind::Word, CharacterKind::Word)
                | (CharacterKind::Whitespace, CharacterKind::Whitespace)
        )
    };
    let start = previous
        .take(128)
        .take_while(|&ch| connects(ch))
        .fold(offset, |acc, ch| acc.saturating_sub(ch.len_utf8()));
    let end = following
        .take(128)
        .take_while(|&ch| connects(ch))
        .fold(offset + character.len_utf8(), |acc, ch| {
            acc + ch.len_utf8()
        });
    start..end
}

/// 获取指定字节偏移量 `offset` 处的单词字节范围 `Range<usize>`（用于双击选词）。
pub fn word_range_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let offset = clip_offset_left(text, offset);
    let character = text[offset..].chars().next()?;
    let end = offset + character.len_utf8();
    Some(word_range_from_chars(
        offset,
        character,
        text[..offset].chars().rev(),
        text[end..].chars(),
    ))
}

/// 获取指定字节偏移量 `offset` 所在行的字节范围 `Range<usize>`（不含行尾 `\n`，用于三击选行或 `Home`/`End`）。
pub fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = clip_offset_left(text, offset);
    let start = text[..offset].rfind('\n').map_or(0, |nl| nl + 1);
    let mut end = text[offset..]
        .find('\n')
        .map_or(text.len(), |nl| offset + nl);
    if end > start && text.as_bytes().get(end - 1) == Some(&b'\r') {
        end -= 1;
    }
    start..end
}

/// 查找 `offset` 左侧的前一个单词起始边界字节偏移（对应 `Ctrl+Left` / `Alt+Left`）。
pub fn prev_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = clip_offset_left(text, offset);
    if pos == 0 {
        return 0;
    }
    // 1. 先跳过紧邻左侧的空白与非单词标点
    while pos > 0 {
        let Some((idx, ch)) = text[..pos].char_indices().next_back() else {
            break;
        };
        if CharacterKind::from(ch) == CharacterKind::Word || ch == '\n' {
            break;
        }
        pos = idx;
    }
    // 2. 若停在换行符且刚好等于原位置前一个字符，则跳越该换行符
    if pos == clip_offset_left(text, offset) {
        if let Some((idx, '\n')) = text[..pos].char_indices().next_back() {
            return idx;
        }
    }
    // 3. 继续向左跳过连续的 Word 字符
    while pos > 0 {
        let Some((idx, ch)) = text[..pos].char_indices().next_back() else {
            break;
        };
        if CharacterKind::from(ch) != CharacterKind::Word {
            break;
        }
        pos = idx;
    }
    pos
}

/// 查找 `offset` 右侧的下一个单词结束边界字节偏移（对应 `Ctrl+Right` / `Alt+Right`）。
pub fn next_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = clip_offset_left(text, offset);
    if pos >= text.len() {
        return text.len();
    }
    // 1. 先跳过紧邻右侧的空白与标点
    while pos < text.len() {
        let Some(ch) = text[pos..].chars().next() else {
            break;
        };
        if CharacterKind::from(ch) == CharacterKind::Word || ch == '\n' {
            break;
        }
        pos += ch.len_utf8();
    }
    if pos == clip_offset_left(text, offset) {
        if let Some('\n') = text[pos..].chars().next() {
            return pos + 1;
        }
    }
    // 2. 跳过连续的 Word 字符
    while pos < text.len() {
        let Some(ch) = text[pos..].chars().next() else {
            break;
        };
        if CharacterKind::from(ch) != CharacterKind::Word {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}
