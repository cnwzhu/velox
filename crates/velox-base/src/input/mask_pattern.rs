//! 格式化输入掩码模式（[`MaskPattern`] / [`MaskToken`]）与全角数字规范化（兼容 `#![no_std]`）。

use velox_engine::arena::SmallVec;

/// 密码模式默认掩码字符。
pub const MASK_CHAR: char = '•';

/// 单个输入掩码令牌规则。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MaskToken {
    /// 数字 `[0-9]`（对应模式字符 `'9'`）。
    Digit,
    /// ASCII 字母 `[a-zA-Z]`（对应模式字符 `'A'`）。
    Letter,
    /// ASCII 字母或数字 `[a-zA-Z0-9]`（对应模式字符 `'#'`）。
    LetterOrDigit,
    /// 固定分隔符（如 `'('`、`')'`、`'-'`、`' '`）。
    Sep(char),
    /// 任意字符（对应模式字符 `'*'`）。
    Any,
}

impl MaskToken {
    #[inline]
    pub const fn is_any(&self) -> bool {
        matches!(self, Self::Any)
    }

    #[inline]
    pub const fn is_sep(&self) -> bool {
        matches!(self, Self::Sep(_))
    }

    #[inline]
    pub const fn is_number(&self) -> bool {
        matches!(self, Self::Digit)
    }

    #[inline]
    pub const fn is_match(&self, ch: char) -> bool {
        match self {
            Self::Digit => ch.is_ascii_digit(),
            Self::Letter => ch.is_ascii_alphabetic(),
            Self::LetterOrDigit => ch.is_ascii_alphanumeric(),
            Self::Any => true,
            Self::Sep(c) => *c == ch,
        }
    }

    #[inline]
    pub const fn placeholder(&self) -> char {
        match self {
            Self::Sep(c) => *c,
            _ => '_',
        }
    }

    #[inline]
    const fn mask_char(&self, ch: char) -> char {
        match self {
            Self::Sep(c) => *c,
            _ => ch,
        }
    }

    #[inline]
    const fn unmask_char(&self, ch: char) -> Option<char> {
        match self {
            Self::Digit | Self::Letter | Self::LetterOrDigit | Self::Any => Some(ch),
            Self::Sep(_) => None,
        }
    }
}

/// 输入格式掩码配置（支持电话号码/邮编/日期模板掩码以及千分位数值分组掩码）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum MaskPattern {
    #[default]
    None,
    /// 模板掩码：`'9'` 表示数字，`'A'` 表示字母，`'|'`/`'#'` 表示字母或数字，`'*'` 表示任意字符，其余为自动补全分隔符。
    Pattern {
        tokens: SmallVec<MaskToken, 32>,
    },
    /// 千分位与小数精度数值掩码（如 `separator: Some(',')`, `fraction: Some(2)`）。
    Number {
        separator: Option<char>,
        fraction: Option<usize>,
    },
}

impl MaskPattern {
    /// 从格式字符串构建模板掩码（例如 `"(999)999-9999"`、`"9999-99-99"`、`"AAAA-99-####"`）。
    pub fn new(pattern: &str) -> Self {
        let mut tokens = SmallVec::new();
        for ch in pattern.chars() {
            let tok = match ch {
                '9' => MaskToken::Digit,
                'A' => MaskToken::Letter,
                '#' => MaskToken::LetterOrDigit,
                '*' => MaskToken::Any,
                _ => MaskToken::Sep(ch),
            };
            let _ = tokens.push(tok);
        }
        Self::Pattern { tokens }
    }

    /// 创建千分位数值掩码（如 `MaskPattern::number(Some(','))`）。
    #[inline]
    pub const fn number(separator: Option<char>) -> Self {
        Self::Number {
            separator,
            fraction: None,
        }
    }

    /// 创建带千分位分隔符与小数位数上限的数值掩码。
    #[inline]
    pub const fn number_with_fraction(separator: Option<char>, fraction: usize) -> Self {
        Self::Number {
            separator,
            fraction: Some(fraction),
        }
    }

    #[inline]
    pub fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            Self::Pattern { tokens } => tokens.is_empty(),
            Self::Number { .. } => false,
        }
    }

    /// 检查字符 `ch` 在模板位置 `pos` 是否合法。
    pub fn is_valid_at(&self, ch: char, pos: usize) -> bool {
        match self {
            Self::None | Self::Number { .. } => true,
            Self::Pattern { tokens } => {
                let slice = tokens.as_slice();
                if let Some(tok) = slice.get(pos) {
                    if tok.is_match(ch) {
                        return true;
                    }
                    if tok.is_sep() {
                        if let Some(next_tok) = slice.get(pos + 1) {
                            if next_tok.is_match(ch) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
        }
    }

    /// 校验文本 `mask_text` 是否符合当前掩码规则。
    pub fn is_valid(&self, mask_text: &str) -> bool {
        if self.is_none() {
            return true;
        }
        match self {
            Self::None => true,
            Self::Pattern { tokens } => {
                let mut chars: SmallVec<char, 64> = SmallVec::new();
                for ch in mask_text.chars() {
                    let _ = chars.push(ch);
                }
                let mut text_index = 0usize;
                for tok in tokens.as_slice() {
                    if text_index >= chars.len() {
                        break;
                    }
                    let ch = chars.as_slice()[text_index];
                    if tok.is_match(ch) {
                        text_index += 1;
                    } else if !tok.is_sep() {
                        return false;
                    }
                }
                text_index == chars.len()
            }
            Self::Number { separator, .. } => {
                if mask_text.is_empty() {
                    return true;
                }
                let mut parts = mask_text.split('.');
                let int_part = parts.next().unwrap_or("");
                let frac_part = parts.next();
                if parts.next().is_some() {
                    return false;
                }
                for (i, ch) in int_part.chars().enumerate() {
                    let is_sign = matches!(ch, '+' | '-');
                    if is_sign && i > 0 {
                        return false;
                    }
                    if !ch.is_ascii_digit() && !(is_sign && i == 0) && Some(ch) != *separator {
                        return false;
                    }
                }
                if let Some(frac) = frac_part {
                    if !frac
                        .chars()
                        .all(|ch| ch.is_ascii_digit() || Some(ch) == *separator)
                    {
                        return false;
                    }
                }
                true
            }
        }
    }

    /// 将原始输入文本按掩码规则格式化写入 `out` 缓冲区，返回格式化后的有效 UTF-8 字节长度。
    pub fn format_into(&self, raw: &str, out: &mut [u8]) -> usize {
        let mut cursor = 0usize;
        let mut push_ch = |ch: char| {
            let mut b = [0u8; 4];
            let enc = ch.encode_utf8(&mut b).as_bytes();
            if cursor + enc.len() <= out.len() {
                out[cursor..cursor + enc.len()].copy_from_slice(enc);
                cursor += enc.len();
            }
        };

        match self {
            Self::None => {
                for ch in raw.chars() {
                    push_ch(ch);
                }
            }
            Self::Pattern { tokens } => {
                let mut text_chars: SmallVec<char, 64> = SmallVec::new();
                for ch in raw.chars() {
                    let _ = text_chars.push(ch);
                }
                let mut text_idx = 0usize;
                for (pos, tok) in tokens.as_slice().iter().enumerate() {
                    if text_idx >= text_chars.len() {
                        break;
                    }
                    let ch = text_chars.as_slice()[text_idx];
                    if !tok.is_sep() && !self.is_valid_at(ch, pos) {
                        break;
                    }
                    let m_ch = tok.mask_char(ch);
                    push_ch(m_ch);
                    if ch == m_ch {
                        text_idx += 1;
                    }
                }
            }
            Self::Number {
                separator,
                fraction,
            } => {
                let Some(sep) = *separator else {
                    for ch in raw.chars() {
                        push_ch(ch);
                    }
                    return cursor;
                };
                let mut cleaned: SmallVec<char, 64> = SmallVec::new();
                for ch in raw.chars() {
                    if ch != sep {
                        let _ = cleaned.push(ch);
                    }
                }
                let mut sign: Option<char> = None;
                let mut start_idx = 0usize;
                if let Some(&first) = cleaned.as_slice().first() {
                    if matches!(first, '+' | '-') {
                        sign = Some(first);
                        start_idx = 1;
                    }
                }
                let slice = &cleaned.as_slice()[start_idx..];
                let dot_pos = slice.iter().position(|&c| c == '.');
                let int_slice = match dot_pos {
                    Some(dp) => &slice[..dp],
                    None => slice,
                };
                if let Some(s) = sign {
                    push_ch(s);
                }
                let int_len = int_slice.len();
                for (i, &ch) in int_slice.iter().enumerate() {
                    if i > 0 && (int_len - i) % 3 == 0 {
                        push_ch(sep);
                    }
                    push_ch(ch);
                }
                if let Some(dp) = dot_pos {
                    if *fraction != Some(0) {
                        push_ch('.');
                        let max_f = fraction.unwrap_or(usize::MAX);
                        for &ch in slice[dp + 1..].iter().take(max_f) {
                            push_ch(ch);
                        }
                    }
                }
            }
        }
        cursor
    }

    /// 去除掩码分隔符并提取原始有效文本写入 `out`，返回有效字节数。
    pub fn unmask_into(&self, masked: &str, out: &mut [u8]) -> usize {
        let mut cursor = 0usize;
        let mut push_ch = |ch: char| {
            let mut b = [0u8; 4];
            let enc = ch.encode_utf8(&mut b).as_bytes();
            if cursor + enc.len() <= out.len() {
                out[cursor..cursor + enc.len()].copy_from_slice(enc);
                cursor += enc.len();
            }
        };

        match self {
            Self::None => {
                for ch in masked.chars() {
                    push_ch(ch);
                }
            }
            Self::Pattern { tokens } => {
                for (ch, tok) in masked.chars().zip(tokens.as_slice().iter()) {
                    if let Some(u) = tok.unmask_char(ch) {
                        push_ch(u);
                    }
                }
            }
            Self::Number { separator, .. } => {
                for ch in masked.chars() {
                    if Some(ch) != *separator {
                        push_ch(ch);
                    }
                }
            }
        }
        cursor
    }
}

/// 将全角数字与中日韩标点（`０`–`９`、`＋`、`－`、`−`、`．`、`。`、`，`）规范化为半角 ASCII 数字字符。
#[inline]
pub fn normalize_number_char(ch: char) -> char {
    match ch {
        '\u{FF10}'..='\u{FF19}' => char::from_u32(ch as u32 - 0xFF10 + '0' as u32).unwrap_or(ch),
        '\u{FF0B}' => '+',
        '\u{FF0D}' | '\u{2212}' => '-',
        '\u{FF0E}' | '\u{3002}' => '.',
        '\u{FF0C}' => ',',
        _ => ch,
    }
}
