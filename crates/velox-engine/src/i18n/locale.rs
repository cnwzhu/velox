//! BCP-47 语言区域标识（[`Locale`]）、书写方向（[`TextDirection`]）、CLDR 复数分类与系统语言自动检测。

use core::fmt;

/// 文本与布局书写方向（从左到右 / 从右到左）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum TextDirection {
    /// 从左到右（中文、英文、日文、韩文、拉丁/西里尔语系等）。
    #[default]
    Ltr,
    /// 从右到左（阿拉伯语 `ar`、希伯来语 `he`、波斯语 `fa`、乌尔都语 `ur` 等）。
    Rtl,
}

impl TextDirection {
    /// 是否为从右到左（RTL）方向。
    #[inline(always)]
    pub const fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }
}

/// Unicode CLDR 标准复数分类（驱动 `cx.t_plural(key, count)`）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl PluralCategory {
    /// 返回 CLDR 标准复数后缀名（用于组合翻译键，如 `"items.one"` / `"items.other"`）。
    #[inline(always)]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::One => "one",
            Self::Two => "two",
            Self::Few => "few",
            Self::Many => "many",
            Self::Other => "other",
        }
    }
}

/// 零堆、16 字节 `Copy` BCP-47 语言区域标签（支持 `language[-Script][-REGION]`，兼容 `#![no_std]` 常量表）。
///
/// 示例：`Locale::EN_US`（`en-US`）、`Locale::ZH_CN`（`zh-CN`）、`Locale::ZH_TW`（`zh-TW`）、`Locale::JA_JP`（`ja-JP`）、`Locale::AR_SA`（`ar-SA`）。
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Locale {
    lang: [u8; 3],
    lang_len: u8,
    script: [u8; 4],
    script_len: u8,
    region: [u8; 3],
    region_len: u8,
}

impl Default for Locale {
    #[inline(always)]
    fn default() -> Self {
        Self::EN_US
    }
}

const fn ascii_lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

const fn ascii_upper(b: u8) -> u8 {
    if b >= b'a' && b <= b'z' {
        b - 32
    } else {
        b
    }
}

const fn copy_lang(s: &str) -> ([u8; 3], u8) {
    let bytes = s.as_bytes();
    let mut out = [0u8; 3];
    let n = if bytes.len() > 3 { 3 } else { bytes.len() };
    let mut i = 0;
    while i < n {
        out[i] = ascii_lower(bytes[i]);
        i += 1;
    }
    (out, n as u8)
}

const fn copy_script(s: &str) -> ([u8; 4], u8) {
    let bytes = s.as_bytes();
    let mut out = [0u8; 4];
    let n = if bytes.len() > 4 { 4 } else { bytes.len() };
    let mut i = 0;
    while i < n {
        out[i] = if i == 0 {
            ascii_upper(bytes[i])
        } else {
            ascii_lower(bytes[i])
        };
        i += 1;
    }
    (out, n as u8)
}

const fn copy_region(s: &str) -> ([u8; 3], u8) {
    let bytes = s.as_bytes();
    let mut out = [0u8; 3];
    let n = if bytes.len() > 3 { 3 } else { bytes.len() };
    let mut i = 0;
    while i < n {
        out[i] = ascii_upper(bytes[i]);
        i += 1;
    }
    (out, n as u8)
}

impl Locale {
    /// 英语（美国）`en-US`。
    pub const EN_US: Self = Self::from_lang_region("en", "US");
    /// 简体中文（中国大陆）`zh-CN`。
    pub const ZH_CN: Self = Self::from_lang_region("zh", "CN");
    /// 繁体中文（中国台湾）`zh-TW`。
    pub const ZH_TW: Self = Self::from_lang_region("zh", "TW");
    /// 日语（日本）`ja-JP`。
    pub const JA_JP: Self = Self::from_lang_region("ja", "JP");
    /// 韩语（韩国）`ko-KR`。
    pub const KO_KR: Self = Self::from_lang_region("ko", "KR");
    /// 德语（德国）`de-DE`。
    pub const DE_DE: Self = Self::from_lang_region("de", "DE");
    /// 法语（法国）`fr-FR`。
    pub const FR_FR: Self = Self::from_lang_region("fr", "FR");
    /// 西班牙语（西班牙）`es-ES`。
    pub const ES_ES: Self = Self::from_lang_region("es", "ES");
    /// 俄语（俄罗斯）`ru-RU`。
    pub const RU_RU: Self = Self::from_lang_region("ru", "RU");
    /// 阿拉伯语（沙特阿拉伯，RTL）`ar-SA`。
    pub const AR_SA: Self = Self::from_lang_region("ar", "SA");
    /// 希伯来语（以色列，RTL）`he-IL`。
    pub const HE_IL: Self = Self::from_lang_region("he", "IL");

    /// 编译期构造仅含语言码的 [`Locale`]（如 `"zh"`、`"en"`、`"ar"`）。
    #[inline]
    pub const fn from_lang(lang: &str) -> Self {
        let (l, ll) = copy_lang(lang);
        Self {
            lang: l,
            lang_len: ll,
            script: [0; 4],
            script_len: 0,
            region: [0; 3],
            region_len: 0,
        }
    }

    /// 编译期构造 `language-REGION` 形式的 [`Locale`]（如 `"zh"`, `"CN"` -> `zh-CN`）。
    #[inline]
    pub const fn from_lang_region(lang: &str, region: &str) -> Self {
        let (l, ll) = copy_lang(lang);
        let (r, rl) = copy_region(region);
        Self {
            lang: l,
            lang_len: ll,
            script: [0; 4],
            script_len: 0,
            region: r,
            region_len: rl,
        }
    }

    /// 编译期构造完整 `language-Script-REGION` 形式的 [`Locale`]（如 `"zh"`, `"Hans"`, `"CN"`）。
    #[inline]
    pub const fn from_parts(lang: &str, script: &str, region: &str) -> Self {
        let (l, ll) = copy_lang(lang);
        let (s, sl) = copy_script(script);
        let (r, rl) = copy_region(region);
        Self {
            lang: l,
            lang_len: ll,
            script: s,
            script_len: sl,
            region: r,
            region_len: rl,
        }
    }

    /// 解析 BCP-47 或 POSIX 风格语言标签（兼容 `"zh_CN.UTF-8"`、`"zh-Hans-CN"`、`"en-us"`、`"ja"`）。
    ///
    /// 若输入为空或不包含有效 ASCII 字母语言子标签，返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        // 剥离 POSIX 编码后缀（如 `.UTF-8`）或修饰符（如 `@euro`）
        let trimmed = raw
            .trim()
            .split(['.', '@'])
            .next()
            .unwrap_or("")
            .trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("C") || trimmed.eq_ignore_ascii_case("POSIX") {
            return None;
        }

        let mut parts = trimmed.split(['-', '_']).filter(|s| !s.is_empty());
        let lang_part = parts.next()?;
        if lang_part.len() < 2 || lang_part.len() > 3 || !lang_part.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        let (lang, lang_len) = copy_lang(lang_part);
        let mut script = [0u8; 4];
        let mut script_len = 0u8;
        let mut region = [0u8; 3];
        let mut region_len = 0u8;

        for sub in parts {
            if script_len == 0 && region_len == 0 && sub.len() == 4 && sub.bytes().all(|b| b.is_ascii_alphabetic()) {
                let (s, sl) = copy_script(sub);
                script = s;
                script_len = sl;
            } else if region_len == 0
                && ((sub.len() == 2 && sub.bytes().all(|b| b.is_ascii_alphabetic()))
                    || (sub.len() == 3 && sub.bytes().all(|b| b.is_ascii_digit())))
            {
                let (r, rl) = copy_region(sub);
                region = r;
                region_len = rl;
            }
        }

        Some(Self {
            lang,
            lang_len,
            script,
            script_len,
            region,
            region_len,
        })
    }

    /// 返回归一化小写语言子标签（如 `"zh"`、`"en"`、`"ja"`）。
    #[inline]
    pub fn language(&self) -> &str {
        let n = (self.lang_len as usize).min(3);
        core::str::from_utf8(&self.lang[..n]).unwrap_or("en")
    }

    /// 返回可选的文字脚本子标签（如 `Some("Hans")` / `Some("Hant")`）。
    #[inline]
    pub fn script(&self) -> Option<&str> {
        if self.script_len == 0 {
            None
        } else {
            let n = (self.script_len as usize).min(4);
            core::str::from_utf8(&self.script[..n]).ok()
        }
    }

    /// 返回可选的大写地区子标签（如 `Some("CN")`、`Some("US")`、`Some("TW")`）。
    #[inline]
    pub fn region(&self) -> Option<&str> {
        if self.region_len == 0 {
            None
        } else {
            let n = (self.region_len as usize).min(3);
            core::str::from_utf8(&self.region[..n]).ok()
        }
    }

    /// 去除脚本子标签（如 `zh-Hans-CN` -> `zh-CN`）。
    #[inline]
    pub const fn without_script(self) -> Self {
        Self {
            script: [0; 4],
            script_len: 0,
            ..self
        }
    }

    /// 退化为纯语言码（如 `zh-CN` -> `zh`），用于 BCP-47 层级回退查找。
    #[inline]
    pub const fn language_only(self) -> Self {
        Self {
            lang: self.lang,
            lang_len: self.lang_len,
            script: [0; 4],
            script_len: 0,
            region: [0; 3],
            region_len: 0,
        }
    }

    /// 判断两个 `Locale` 的主语言码是否一致（忽略地区与脚本差异）。
    #[inline]
    pub const fn same_language(self, other: Self) -> bool {
        self.lang_len == other.lang_len
            && self.lang[0] == other.lang[0]
            && self.lang[1] == other.lang[1]
            && self.lang[2] == other.lang[2]
    }

    /// 根据主语言码返回默认书写方向（`ar` / `he` / `iw` / `fa` / `ur` / `ps` / `sd` / `yi` -> `TextDirection::Rtl`）。
    #[inline]
    pub fn text_direction(self) -> TextDirection {
        match self.language() {
            "ar" | "he" | "iw" | "fa" | "ur" | "ps" | "sd" | "yi" => TextDirection::Rtl,
            _ => TextDirection::Ltr,
        }
    }

    /// 当前语言是否为从右到左（RTL）书写系统。
    #[inline]
    pub fn is_rtl(self) -> bool {
        self.text_direction().is_rtl()
    }

    /// 将规范化 BCP-47 标签零堆写入调用方提供的 16 字节栈缓冲区并返回切片（如 `"zh-CN"`）。
    pub fn format_tag<'b>(&self, buf: &'b mut [u8; 16]) -> &'b str {
        let mut len = 0usize;
        let ll = (self.lang_len as usize).min(3);
        buf[..ll].copy_from_slice(&self.lang[..ll]);
        len += ll;
        if self.script_len > 0 {
            let sl = (self.script_len as usize).min(4);
            buf[len] = b'-';
            len += 1;
            buf[len..len + sl].copy_from_slice(&self.script[..sl]);
            len += sl;
        }
        if self.region_len > 0 {
            let rl = (self.region_len as usize).min(3);
            buf[len] = b'-';
            len += 1;
            buf[len..len + rl].copy_from_slice(&self.region[..rl]);
            len += rl;
        }
        core::str::from_utf8(&buf[..len]).unwrap_or("en")
    }

    /// 返回当前语言友好的原生短标签（供工具栏语言切换按钮显示）。
    pub fn native_short_name(self) -> &'static str {
        match (self.language(), self.region()) {
            ("zh", Some("TW" | "HK" | "MO")) => "繁體",
            ("zh", _) => "中文",
            ("ja", _) => "日本語",
            ("ko", _) => "한국어",
            ("de", _) => "DE",
            ("fr", _) => "FR",
            ("es", _) => "ES",
            ("ru", _) => "RU",
            ("ar", _) => "العربية",
            ("he", _) => "עברית",
            _ => "EN",
        }
    }

    /// 按 Unicode CLDR 整数复数规则计算 `count` 所属的 [`PluralCategory`]。
    pub fn plural_category(self, count: i64) -> PluralCategory {
        let n = count.unsigned_abs();
        match self.language() {
            // 中/日/韩/越/泰/印尼等无语法复数变化语言
            "zh" | "ja" | "ko" | "vi" | "th" | "id" | "ms" => PluralCategory::Other,
            // 法语 / 巴西葡语：0 和 1 均归入 One
            "fr" | "hi" => {
                if n <= 1 {
                    PluralCategory::One
                } else {
                    PluralCategory::Other
                }
            }
            // 俄语 / 乌克兰语 / 塞尔维亚-克罗地亚语（东斯拉夫 CLDR 规则）
            "ru" | "uk" | "sr" | "hr" | "bs" => {
                let m10 = n % 10;
                let m100 = n % 100;
                if m10 == 1 && m100 != 11 {
                    PluralCategory::One
                } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Many
                }
            }
            // 阿拉伯语完整 CLDR 6 档复数规则
            "ar" => {
                let m100 = n % 100;
                if n == 0 {
                    PluralCategory::Zero
                } else if n == 1 {
                    PluralCategory::One
                } else if n == 2 {
                    PluralCategory::Two
                } else if (3..=10).contains(&m100) {
                    PluralCategory::Few
                } else if (11..=99).contains(&m100) {
                    PluralCategory::Many
                } else {
                    PluralCategory::Other
                }
            }
            // 英语 / 德语 / 西班牙语 / 意大利语 / 荷兰语等默认规则
            _ => {
                if n == 1 {
                    PluralCategory::One
                } else {
                    PluralCategory::Other
                }
            }
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0u8; 16];
        f.write_str(self.format_tag(&mut buf))
    }
}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0u8; 16];
        write!(f, "Locale(\"{}\")", self.format_tag(&mut buf))
    }
}

/// 自动探测宿主操作系统的默认用户界面语言区域（零外部 crate 依赖；无法探测时回退 [`Locale::EN_US`]）。
///
/// - **Windows**：优先调用 Win32 `GetUserDefaultLocaleName`；
/// - **Linux / macOS / BSD**：依次检查 `VELOX_LANG`、`LC_ALL`、`LC_MESSAGES`、`LANG` 环境变量；
/// - **WASM / 未知宿主**：回退 [`Locale::EN_US`]。
pub fn detect_system_locale() -> Locale {
    for var in ["VELOX_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            if let Some(loc) = Locale::parse(&val) {
                return loc;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_locale_name: i32) -> i32;
        }
        let mut wide = [0u16; 85];
        let written = unsafe { GetUserDefaultLocaleName(wide.as_mut_ptr(), wide.len() as i32) };
        if written > 1 {
            let slice = &wide[..(written as usize).saturating_sub(1)];
            let mut ascii = [0u8; 32];
            let mut len = 0usize;
            for &cu in slice {
                if cu < 128 && len < ascii.len() {
                    ascii[len] = cu as u8;
                    len += 1;
                }
            }
            if let Ok(s) = core::str::from_utf8(&ascii[..len]) {
                if let Some(loc) = Locale::parse(s) {
                    return loc;
                }
            }
        }
    }

    Locale::EN_US
}
