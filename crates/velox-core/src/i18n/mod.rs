//! 跨平台国际化与本地化子系统（`velox_core::i18n`）。
//!
//! 面向 `#![no_std]` MCU 与桌面/移动端/Web `std` 双模设计：
//! - **零堆 BCP-47 [`Locale`] 与 [`TextDirection`]**：支持 `en-US`、`zh-CN`、`zh-TW`、`ja-JP`、`ko-KR`、`ar-SA`（RTL）等标准标签解析与层级回退链；
//! - **CLDR 复数规则（[`PluralCategory`]）**：内置东亚、拉丁/日耳曼、法语、东斯拉夫与阿拉伯语 6 档复数规则；
//! - **静态 Flash 词典（[`StaticCatalog`]）+ 运行时热加载词典（[`DynamicCatalog`]）**：支持零外部依赖解析 `key=value` 与扁平 JSON 语言包；
//! - **零堆参数模板插值（[`format_template`]）**：直接分配于 `ScratchArena`，配合 `cx.t(key)` / `cx.t_args(key, &[...])` / `cx.t_plural(key, count)` 开箱即用；
//! - **与渲染引擎热联动**：通过 `engine.set_locale(Locale::ZH_CN)` 即时标脏重绘整棵 UI 树，零重启切换语言。

mod catalog;
mod locale;

use std::sync::Arc;

use crate::arena::{ScratchArena, StrRef};

pub use catalog::DynamicCatalog;
pub use catalog::{
    format_plural_key, format_template, I18nArg, MessageEntry, StaticCatalog, StaticLocaleTable,
};
pub use locale::detect_system_locale;
pub use locale::{Locale, PluralCategory, TextDirection};

/// 帧上下文持有的轻量国际化状态（同时支持静态零堆词典与 `std` 运行时动态覆盖词典）。
pub struct I18nContext {
    pub locale: Locale,
    pub static_catalog: Option<&'static StaticCatalog>,
    pub dynamic_catalog: Option<Arc<DynamicCatalog>>,
}

impl Default for I18nContext {
    #[inline]
    fn default() -> Self {
        Self::new(Locale::EN_US)
    }
}

impl I18nContext {
    /// 使用给定默认语言区域创建空的国际化上下文。
    #[inline]
    pub const fn new(locale: Locale) -> Self {
        Self {
            locale,
            static_catalog: None,
            dynamic_catalog: None,
        }
    }

    /// 查找单条翻译模板（动态词典优先于静态词典；均未命中返回 `None`）。
    #[inline]
    pub fn lookup_template(&self, key: &str) -> Option<&str> {
        if let Some(dyn_cat) = &self.dynamic_catalog {
            if let Some(v) = dyn_cat.lookup(self.locale, key) {
                return Some(v);
            }
        }
        if let Some(st_cat) = self.static_catalog {
            if let Some(v) = st_cat.lookup(self.locale, key) {
                return Some(v);
            }
        }
        None
    }

    /// 按 CLDR 复数规则查找复数模板（动态词典优先于静态词典）。
    #[inline]
    pub fn lookup_plural_template(&self, key: &str, count: i64) -> Option<&str> {
        if let Some(dyn_cat) = &self.dynamic_catalog {
            if let Some(v) = dyn_cat.lookup_plural(self.locale, key, count) {
                return Some(v);
            }
        }
        if let Some(st_cat) = self.static_catalog {
            if let Some(v) = st_cat.lookup_plural(self.locale, key, count) {
                return Some(v);
            }
        }
        None
    }

    /// 翻译无参数文本键并分配为 `StrRef<'a>`（静态表直出零拷贝；未命中回退原 `key`）。
    #[inline]
    pub fn translate<'a>(&self, arena: &'a ScratchArena, key: &str) -> StrRef<'a> {
        if let Some(dyn_cat) = &self.dynamic_catalog {
            if let Some(v) = dyn_cat.lookup(self.locale, key) {
                return arena.alloc_str(v);
            }
        }
        if let Some(st_cat) = self.static_catalog {
            if let Some(v) = st_cat.lookup(self.locale, key) {
                return StrRef::from(v);
            }
        }
        arena.alloc_str(key)
    }

    /// 翻译带命名/位置参数的文本键并零堆插值至 `ScratchArena`。
    #[inline]
    pub fn translate_args<'a>(
        &self,
        arena: &'a ScratchArena,
        key: &str,
        args: &[(&str, I18nArg<'_>)],
    ) -> StrRef<'a> {
        let tmpl = self.lookup_template(key).unwrap_or(key);
        format_template(arena, tmpl, args)
    }

    /// 按 CLDR 复数规则翻译并自动注入 `{count}` 参数（零堆插值至 `ScratchArena`）。
    #[inline]
    pub fn translate_plural<'a>(
        &self,
        arena: &'a ScratchArena,
        key: &str,
        count: i64,
    ) -> StrRef<'a> {
        let tmpl = self.lookup_plural_template(key, count).unwrap_or(key);
        format_template(arena, tmpl, &[("count", I18nArg::Int(count))])
    }
}
