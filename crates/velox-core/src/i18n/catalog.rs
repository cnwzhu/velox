//! 静态零堆翻译目录（[`StaticCatalog`]）、`std` 运行时动态目录（[`DynamicCatalog`]）与零堆参数模板插值引擎。

use core::fmt::Write as _;

use crate::arena::{ScratchArena, StrRef};

use super::locale::{Locale, PluralCategory};

/// 单条静态键值对翻译条目（驻留 `.rodata` / Flash，零运行时开销）。
#[derive(Copy, Clone, Debug)]
pub struct MessageEntry {
    pub key: &'static str,
    pub value: &'static str,
}

impl MessageEntry {
    /// 编译期构造静态翻译条目。
    #[inline(always)]
    pub const fn new(key: &'static str, value: &'static str) -> Self {
        Self { key, value }
    }
}

/// 单个语言区域对应的静态翻译表。
#[derive(Copy, Clone, Debug)]
pub struct StaticLocaleTable {
    pub locale: Locale,
    pub entries: &'static [MessageEntry],
}

impl StaticLocaleTable {
    /// 编译期构造单语言静态词典表。
    #[inline(always)]
    pub const fn new(locale: Locale, entries: &'static [MessageEntry]) -> Self {
        Self { locale, entries }
    }

    /// 在本语言表中查找 `key`（若表按字典序排列优先 $O(\log N)$ 二分，否则自动线性回退）。
    #[inline]
    pub fn lookup(&self, key: &str) -> Option<&'static str> {
        if let Ok(idx) = self.entries.binary_search_by(|e| e.key.cmp(key)) {
            return Some(self.entries[idx].value);
        }
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.value)
    }
}

/// 多语言静态翻译词典集合（100% `#![no_std]` 零堆，支持 BCP-47 层级回退链）。
#[derive(Copy, Clone, Debug)]
pub struct StaticCatalog {
    /// 最终兜底语言区域（通常为 [`Locale::EN_US`]）。
    pub fallback: Locale,
    /// 各语言区域的静态词典切片。
    pub tables: &'static [StaticLocaleTable],
}

impl StaticCatalog {
    /// 编译期构造静态多语言词典集合。
    #[inline(always)]
    pub const fn new(fallback: Locale, tables: &'static [StaticLocaleTable]) -> Self {
        Self { fallback, tables }
    }

    #[inline]
    fn find_exact(&self, locale: Locale, key: &str) -> Option<&'static str> {
        self.tables
            .iter()
            .find(|t| t.locale == locale)
            .and_then(|t| t.lookup(key))
    }

    #[inline]
    fn find_same_lang(&self, locale: Locale, key: &str) -> Option<&'static str> {
        self.tables
            .iter()
            .find(|t| t.locale.same_language(locale))
            .and_then(|t| t.lookup(key))
    }

    /// 按 BCP-47 优先级链路查找翻译模板：
    /// 1. 精确匹配 `locale`（如 `zh-Hans-CN`）；
    /// 2. 去除脚本匹配（`zh-CN`）；
    /// 3. 纯语言码匹配（`zh`）或同主语言表匹配（如 `zh-SG` 回退 `zh-CN`）；
    /// 4. 兜底语言 `self.fallback` 精确或同语言匹配。
    pub fn lookup(&self, locale: Locale, key: &str) -> Option<&'static str> {
        if let Some(v) = self.find_exact(locale, key) {
            return Some(v);
        }
        let no_script = locale.without_script();
        if no_script != locale {
            if let Some(v) = self.find_exact(no_script, key) {
                return Some(v);
            }
        }
        let lang_only = locale.language_only();
        if lang_only != locale {
            if let Some(v) = self.find_exact(lang_only, key) {
                return Some(v);
            }
        }
        if let Some(v) = self.find_same_lang(locale, key) {
            return Some(v);
        }
        if self.fallback != locale {
            if let Some(v) = self.find_exact(self.fallback, key) {
                return Some(v);
            }
            if let Some(v) = self.find_same_lang(self.fallback, key) {
                return Some(v);
            }
        }
        None
    }

    /// 按 CLDR 复数规则查找复数模板：优先查 `"{key}.{category}"` -> `"{key}.other"` -> `"{key}"`。
    pub fn lookup_plural(&self, locale: Locale, key: &str, count: i64) -> Option<&'static str> {
        let cat = locale.plural_category(count);
        let mut buf = [0u8; 96];
        let cat_key = format_plural_key(&mut buf, key, cat);
        if let Some(v) = self.lookup(locale, cat_key) {
            return Some(v);
        }
        if cat != PluralCategory::Other {
            let other_key = format_plural_key(&mut buf, key, PluralCategory::Other);
            if let Some(v) = self.lookup(locale, other_key) {
                return Some(v);
            }
        }
        self.lookup(locale, key)
    }
}

/// 零堆拼接复数键 `"{base_key}.{suffix}"` 到栈缓冲区中。
#[inline]
pub fn format_plural_key<'a>(
    buf: &'a mut [u8; 96],
    base_key: &'a str,
    cat: PluralCategory,
) -> &'a str {
    let suffix = cat.suffix();
    let kb = base_key.as_bytes();
    let sb = suffix.as_bytes();
    let total = kb.len() + 1 + sb.len();
    if total > buf.len() {
        return base_key;
    }
    buf[..kb.len()].copy_from_slice(kb);
    buf[kb.len()] = b'.';
    buf[kb.len() + 1..total].copy_from_slice(sb);
    core::str::from_utf8(&buf[..total]).unwrap_or(base_key)
}

/// 模板插值参数（零堆 `Copy`，支持字符串、有/无符号整数与定点小数）。
#[derive(Copy, Clone, Debug)]
pub enum I18nArg<'a> {
    Str(&'a str),
    Int(i64),
    Uint(u64),
    /// 浮点数与保留小数位数（`0..=6`）。
    Float(f64, u8),
}

impl<'a> From<&'a str> for I18nArg<'a> {
    #[inline(always)]
    fn from(v: &'a str) -> Self {
        Self::Str(v)
    }
}

impl<'a> From<StrRef<'a>> for I18nArg<'a> {
    #[inline(always)]
    fn from(v: StrRef<'a>) -> Self {
        Self::Str(v.as_str())
    }
}

impl From<i32> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: i32) -> Self {
        Self::Int(v as i64)
    }
}

impl From<i64> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<u8> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: u8) -> Self {
        Self::Uint(v as u64)
    }
}

impl From<u16> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: u16) -> Self {
        Self::Uint(v as u64)
    }
}

impl From<u32> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: u32) -> Self {
        Self::Uint(v as u64)
    }
}

impl From<u64> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: u64) -> Self {
        Self::Uint(v)
    }
}

impl From<usize> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: usize) -> Self {
        Self::Uint(v as u64)
    }
}

impl From<f32> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: f32) -> Self {
        Self::Float(v as f64, 1)
    }
}

impl From<f64> for I18nArg<'_> {
    #[inline(always)]
    fn from(v: f64) -> Self {
        Self::Float(v, 2)
    }
}

struct FixedBufWriter<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> FixedBufWriter<N> {
    #[inline]
    fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    #[inline]
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> core::fmt::Write for FixedBufWriter<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let avail = N.saturating_sub(self.len);
        let mut take = bytes.len().min(avail);
        while take > 0 && !s.is_char_boundary(take) {
            take -= 1;
        }
        self.buf[self.len..self.len + take].copy_from_slice(&bytes[..take]);
        self.len += take;
        Ok(())
    }
}

fn write_arg(out: &mut impl core::fmt::Write, arg: I18nArg<'_>) {
    match arg {
        I18nArg::Str(s) => {
            let _ = out.write_str(s);
        }
        I18nArg::Int(v) => {
            let _ = write!(out, "{}", v);
        }
        I18nArg::Uint(v) => {
            let _ = write!(out, "{}", v);
        }
        I18nArg::Float(v, prec) => {
            let p = (prec as usize).min(6);
            let _ = write!(out, "{:.*}", p, v);
        }
    }
}

/// 将翻译模板中的 `{name}` 命名占位符与 `{0}` 位置占位符零堆插值到 [`ScratchArena`] 并返回 [`StrRef<'a>`]。
///
/// 支持 `{{` -> `{` 与 `}}` -> `}` 转义；未匹配的占位符原样保留，永不 panic。
pub fn format_template<'a>(
    arena: &'a ScratchArena,
    template: &str,
    args: &[(&str, I18nArg<'_>)],
) -> StrRef<'a> {
    if args.is_empty() && !template.contains("{{") && !template.contains("}}") {
        return arena.alloc_str(template);
    }

    let mut writer = FixedBufWriter::<512>::new();
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                let _ = writer.write_char('{');
                i += 2;
                continue;
            }
            if let Some(rel_end) = template[i + 1..].find('}') {
                let placeholder = &template[i + 1..i + 1 + rel_end];
                let mut matched = false;
                for &(k, v) in args {
                    if k == placeholder {
                        write_arg(&mut writer, v);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    if let Ok(pos_idx) = placeholder.parse::<usize>() {
                        if let Some(&(_, v)) = args.get(pos_idx) {
                            write_arg(&mut writer, v);
                            matched = true;
                        }
                    }
                }
                if !matched {
                    let _ = writer.write_str(&template[i..=i + 1 + rel_end]);
                }
                i += rel_end + 2;
                continue;
            }
        } else if bytes[i] == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
            let _ = writer.write_char('}');
            i += 2;
            continue;
        }
        let ch = template[i..].chars().next().unwrap_or(' ');
        let _ = writer.write_char(ch);
        i += ch.len_utf8();
    }

    arena.alloc_str(writer.as_str())
}

/// 面向桌面、移动端与 Web 的运行时动态翻译目录（支持运行时热加载 `key = value` 与扁平 JSON 语言包，零外部 serde 依赖）。
#[derive(Clone, Debug)]
pub struct DynamicCatalog {
    fallback: Locale,
    tables: std::collections::HashMap<Locale, std::collections::HashMap<std::string::String, std::string::String>>,
}

impl Default for DynamicCatalog {
    fn default() -> Self {
        Self::new(Locale::EN_US)
    }
}

impl DynamicCatalog {
    /// 创建指定兜底语言区域的动态翻译目录。
    pub fn new(fallback: Locale) -> Self {
        Self {
            fallback,
            tables: std::collections::HashMap::new(),
        }
    }

    /// 合并一份编译期 [`StaticCatalog`] 至动态目录（便于在此基础上做热覆盖）。
    pub fn merge_static(&mut self, catalog: &StaticCatalog) {
        for tbl in catalog.tables {
            let entry_map = self.tables.entry(tbl.locale).or_default();
            for e in tbl.entries {
                entry_map
                    .entry(std::string::String::from(e.key))
                    .or_insert_with(|| std::string::String::from(e.value));
            }
        }
    }

    /// 插入或覆盖一条翻译记录。
    pub fn insert(
        &mut self,
        locale: Locale,
        key: impl Into<std::string::String>,
        value: impl Into<std::string::String>,
    ) {
        self.tables
            .entry(locale)
            .or_default()
            .insert(key.into(), value.into());
    }

    /// 加载轻量 `key = value`（Fluent/Properties 风格，忽略 `#` / `//` 注释行）语言包文本，返回成功导入的条目数。
    pub fn load_kv(&mut self, locale: Locale, content: &str) -> usize {
        let mut count = 0usize;
        let map = self.tables.entry(locale).or_default();
        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim();
                let val = unescape_simple(v.trim());
                if !key.is_empty() {
                    map.insert(std::string::String::from(key), val);
                    count += 1;
                }
            }
        }
        count
    }

    /// 零外部依赖解析标准扁平 JSON 语言包（`{ "key": "value", ... }`，支持 `\"`/`\\`/`\n`/`\t`/`\uXXXX` 转义），返回导入条目数。
    pub fn load_json(&mut self, locale: Locale, json: &str) -> usize {
        let mut count = 0usize;
        let map = self.tables.entry(locale).or_default();
        let bytes = json.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let Some((key, next_i)) = parse_json_string(json, i) else {
                break;
            };
            i = next_i;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b':' {
                continue;
            }
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'"' {
                continue;
            }
            let Some((val, val_next)) = parse_json_string(json, i) else {
                break;
            };
            i = val_next;
            if !key.is_empty() {
                map.insert(key, val);
                count += 1;
            }
        }
        count
    }

    #[inline]
    fn find_exact(&self, locale: Locale, key: &str) -> Option<&str> {
        self.tables
            .get(&locale)
            .and_then(|m| m.get(key))
            .map(|s| s.as_str())
    }

    #[inline]
    fn find_same_lang(&self, locale: Locale, key: &str) -> Option<&str> {
        self.tables
            .iter()
            .find(|(loc, _)| loc.same_language(locale))
            .and_then(|(_, m)| m.get(key))
            .map(|s| s.as_str())
    }

    /// 按 BCP-47 层级回退链查询翻译模板。
    pub fn lookup(&self, locale: Locale, key: &str) -> Option<&str> {
        if let Some(v) = self.find_exact(locale, key) {
            return Some(v);
        }
        let no_script = locale.without_script();
        if no_script != locale {
            if let Some(v) = self.find_exact(no_script, key) {
                return Some(v);
            }
        }
        let lang_only = locale.language_only();
        if lang_only != locale {
            if let Some(v) = self.find_exact(lang_only, key) {
                return Some(v);
            }
        }
        if let Some(v) = self.find_same_lang(locale, key) {
            return Some(v);
        }
        if self.fallback != locale {
            if let Some(v) = self.find_exact(self.fallback, key) {
                return Some(v);
            }
            if let Some(v) = self.find_same_lang(self.fallback, key) {
                return Some(v);
            }
        }
        None
    }

    /// 按 CLDR 复数规则查询复数模板。
    pub fn lookup_plural(&self, locale: Locale, key: &str, count: i64) -> Option<&str> {
        let cat = locale.plural_category(count);
        let mut buf = [0u8; 96];
        let cat_key = format_plural_key(&mut buf, key, cat);
        if let Some(v) = self.lookup(locale, cat_key) {
            return Some(v);
        }
        if cat != PluralCategory::Other {
            let other_key = format_plural_key(&mut buf, key, PluralCategory::Other);
            if let Some(v) = self.lookup(locale, other_key) {
                return Some(v);
            }
        }
        self.lookup(locale, key)
    }
}

fn unescape_simple(raw: &str) -> std::string::String {
    let mut out = std::string::String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_json_string(src: &str, start_quote: usize) -> Option<(std::string::String, usize)> {
    let bytes = src.as_bytes();
    if bytes.get(start_quote) != Some(&b'"') {
        return None;
    }
    let mut out = std::string::String::new();
    let mut i = start_quote + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return Some((out, i + 1));
        }
        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return None;
            }
            match bytes[i] {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' if i + 4 < bytes.len() => {
                    let hex = &src[i + 1..i + 5];
                    if let Ok(cp) = u32::from_str_radix(hex, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                    i += 4;
                }
                other => out.push(other as char),
            }
            i += 1;
            continue;
        }
        let ch = src[i..].chars().next()?;
        out.push(ch);
        i += ch.len_utf8();
    }
    None
}
