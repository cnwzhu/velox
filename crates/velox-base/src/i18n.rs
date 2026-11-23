//! `velox-base` 组件库内置多语言词典（`en-US`、`zh-CN`、`zh-TW`、`ja-JP`、`ar-SA`）。
//!
//! 100% 兼容 `#![no_std]` 零堆分配：驻留 `.rodata`，且当应用层通过 `engine.set_i18n_catalog(...)`
//! 或 `engine.set_dynamic_catalog(...)` 覆盖同名键时，应用级翻译自动优先于组件默认文案。

use velox_engine::prelude::*;

const EN_US_ENTRIES: &[MessageEntry] = &[
    MessageEntry::new("velox.calendar.fri", "Fr"),
    MessageEntry::new("velox.calendar.mon", "Mo"),
    MessageEntry::new("velox.calendar.sat", "Sa"),
    MessageEntry::new("velox.calendar.sun", "Su"),
    MessageEntry::new("velox.calendar.thu", "Th"),
    MessageEntry::new("velox.calendar.tue", "Tu"),
    MessageEntry::new("velox.calendar.wed", "We"),
    MessageEntry::new("velox.common.cancel", "Cancel"),
    MessageEntry::new("velox.common.close", "Close"),
    MessageEntry::new("velox.common.confirm", "Confirm"),
    MessageEntry::new("velox.common.loading", "Loading..."),
    MessageEntry::new("velox.common.next", "Next"),
    MessageEntry::new("velox.common.no_results", "No results"),
    MessageEntry::new("velox.common.ok", "OK"),
    MessageEntry::new("velox.common.prev", "Prev"),
    MessageEntry::new("velox.common.search", "Search..."),
];

const ZH_CN_ENTRIES: &[MessageEntry] = &[
    MessageEntry::new("velox.calendar.fri", "五"),
    MessageEntry::new("velox.calendar.mon", "一"),
    MessageEntry::new("velox.calendar.sat", "六"),
    MessageEntry::new("velox.calendar.sun", "日"),
    MessageEntry::new("velox.calendar.thu", "四"),
    MessageEntry::new("velox.calendar.tue", "二"),
    MessageEntry::new("velox.calendar.wed", "三"),
    MessageEntry::new("velox.common.cancel", "取消"),
    MessageEntry::new("velox.common.close", "关闭"),
    MessageEntry::new("velox.common.confirm", "确认"),
    MessageEntry::new("velox.common.loading", "加载中..."),
    MessageEntry::new("velox.common.next", "下一步"),
    MessageEntry::new("velox.common.no_results", "无匹配结果"),
    MessageEntry::new("velox.common.ok", "确定"),
    MessageEntry::new("velox.common.prev", "上一步"),
    MessageEntry::new("velox.common.search", "搜索..."),
];

const ZH_TW_ENTRIES: &[MessageEntry] = &[
    MessageEntry::new("velox.calendar.fri", "五"),
    MessageEntry::new("velox.calendar.mon", "一"),
    MessageEntry::new("velox.calendar.sat", "六"),
    MessageEntry::new("velox.calendar.sun", "日"),
    MessageEntry::new("velox.calendar.thu", "四"),
    MessageEntry::new("velox.calendar.tue", "二"),
    MessageEntry::new("velox.calendar.wed", "三"),
    MessageEntry::new("velox.common.cancel", "取消"),
    MessageEntry::new("velox.common.close", "關閉"),
    MessageEntry::new("velox.common.confirm", "確認"),
    MessageEntry::new("velox.common.loading", "載入中..."),
    MessageEntry::new("velox.common.next", "下一步"),
    MessageEntry::new("velox.common.no_results", "無符合結果"),
    MessageEntry::new("velox.common.ok", "確定"),
    MessageEntry::new("velox.common.prev", "上一步"),
    MessageEntry::new("velox.common.search", "搜尋..."),
];

const JA_JP_ENTRIES: &[MessageEntry] = &[
    MessageEntry::new("velox.calendar.fri", "金"),
    MessageEntry::new("velox.calendar.mon", "月"),
    MessageEntry::new("velox.calendar.sat", "土"),
    MessageEntry::new("velox.calendar.sun", "日"),
    MessageEntry::new("velox.calendar.thu", "木"),
    MessageEntry::new("velox.calendar.tue", "火"),
    MessageEntry::new("velox.calendar.wed", "水"),
    MessageEntry::new("velox.common.cancel", "キャンセル"),
    MessageEntry::new("velox.common.close", "閉じる"),
    MessageEntry::new("velox.common.confirm", "確認"),
    MessageEntry::new("velox.common.loading", "読み込み中..."),
    MessageEntry::new("velox.common.next", "次へ"),
    MessageEntry::new("velox.common.no_results", "一致する結果なし"),
    MessageEntry::new("velox.common.ok", "OK"),
    MessageEntry::new("velox.common.prev", "前へ"),
    MessageEntry::new("velox.common.search", "検索..."),
];

const AR_SA_ENTRIES: &[MessageEntry] = &[
    MessageEntry::new("velox.calendar.fri", "ج"),
    MessageEntry::new("velox.calendar.mon", "ن"),
    MessageEntry::new("velox.calendar.sat", "س"),
    MessageEntry::new("velox.calendar.sun", "ح"),
    MessageEntry::new("velox.calendar.thu", "خ"),
    MessageEntry::new("velox.calendar.tue", "ث"),
    MessageEntry::new("velox.calendar.wed", "ر"),
    MessageEntry::new("velox.common.cancel", "إلغاء"),
    MessageEntry::new("velox.common.close", "إغلاق"),
    MessageEntry::new("velox.common.confirm", "تأكيد"),
    MessageEntry::new("velox.common.loading", "جارٍ التحميل..."),
    MessageEntry::new("velox.common.next", "التالي"),
    MessageEntry::new("velox.common.no_results", "لا توجد نتائج"),
    MessageEntry::new("velox.common.ok", "موافق"),
    MessageEntry::new("velox.common.prev", "السابق"),
    MessageEntry::new("velox.common.search", "بحث..."),
];

/// `velox-base` 内置 5 语组件默认文案词典（`en-US` / `zh-CN` / `zh-TW` / `ja-JP` / `ar-SA`）。
pub static BUILTIN_WIDGET_CATALOG: StaticCatalog = StaticCatalog::new(
    Locale::EN_US,
    &[
        StaticLocaleTable::new(Locale::EN_US, EN_US_ENTRIES),
        StaticLocaleTable::new(Locale::ZH_CN, ZH_CN_ENTRIES),
        StaticLocaleTable::new(Locale::ZH_TW, ZH_TW_ENTRIES),
        StaticLocaleTable::new(Locale::JA_JP, JA_JP_ENTRIES),
        StaticLocaleTable::new(Locale::AR_SA, AR_SA_ENTRIES),
    ],
);

/// 查询组件级国际化文案：优先采用应用侧词典覆写，未命中时自动回退至 [`BUILTIN_WIDGET_CATALOG`]。
#[inline]
pub fn widget_t<'a>(cx: &'a FrameScope, key: &'a str) -> StrRef<'a> {
    let app_val = cx.t(key);
    if app_val.as_str() != key {
        return app_val;
    }
    if let Some(builtin) = BUILTIN_WIDGET_CATALOG.lookup(cx.locale(), key) {
        return StrRef::from(builtin);
    }
    StrRef::from(key)
}
