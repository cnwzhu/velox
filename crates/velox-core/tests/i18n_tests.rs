//! `velox-core` 国际化（`i18n` / `l10n`）与本地化测试套件（同时验证 `#![no_std]` 与 `std`）。

use velox_core::prelude::*;

const TEST_EN: &[MessageEntry] = &[
    MessageEntry::new("files.one", "{count} file"),
    MessageEntry::new("files.other", "{count} files"),
    MessageEntry::new("greeting", "Hello, {name}! ({0})"),
    MessageEntry::new("only_en", "Fallback English Only"),
    MessageEntry::new("title", "Velox Workbench"),
];

const TEST_ZH_CN: &[MessageEntry] = &[
    MessageEntry::new("files.other", "{count} 个文件"),
    MessageEntry::new("greeting", "你好，{name}！（{0}）"),
    MessageEntry::new("title", "Velox 工作台"),
];

const TEST_AR_SA: &[MessageEntry] = &[
    MessageEntry::new("files.few", "{count} ملفات"),
    MessageEntry::new("files.many", "{count} ملفًا"),
    MessageEntry::new("files.one", "ملف واحد"),
    MessageEntry::new("files.other", "{count} ملف"),
    MessageEntry::new("files.two", "ملفان"),
    MessageEntry::new("files.zero", "لا توجد ملفات"),
    MessageEntry::new("title", "لوحة فيلوكس"),
];

static TEST_CATALOG: StaticCatalog = StaticCatalog::new(
    Locale::EN_US,
    &[
        StaticLocaleTable::new(Locale::EN_US, TEST_EN),
        StaticLocaleTable::new(Locale::ZH_CN, TEST_ZH_CN),
        StaticLocaleTable::new(Locale::AR_SA, TEST_AR_SA),
    ],
);

#[test]
fn test_locale_parse_normalization_and_rtl_helpers() {
    let zh = Locale::parse("zh_CN.UTF-8").expect("parse zh_CN.UTF-8");
    assert_eq!(zh, Locale::ZH_CN);
    assert_eq!(zh.language(), "zh");
    assert_eq!(zh.region(), Some("CN"));
    assert_eq!(zh.text_direction(), TextDirection::Ltr);
    assert!(!zh.is_rtl());

    let zh_hans = Locale::parse("zh-hans-cn").expect("parse zh-hans-cn");
    assert_eq!(zh_hans.script(), Some("Hans"));
    assert_eq!(zh_hans.without_script(), Locale::ZH_CN);
    let mut tag_buf = [0u8; 16];
    assert_eq!(zh_hans.format_tag(&mut tag_buf), "zh-Hans-CN");

    let ar = Locale::parse("ar-sa").expect("parse ar-sa");
    assert_eq!(ar, Locale::AR_SA);
    assert_eq!(ar.text_direction(), TextDirection::Rtl);
    assert!(ar.is_rtl());
    assert!(Locale::HE_IL.is_rtl());

    // 无效 POSIX 占位符安全返回 None
    assert_eq!(Locale::parse("POSIX"), None);
    assert_eq!(Locale::parse(""), None);

    // 验证 Text::text_start / text_end 与 Div::ps / pe / justify_start_localized 的 RTL 自动镜像
    let mut cx = FrameScope::new();
    cx.set_locale(Locale::EN_US);
    assert_eq!(text("hi").text_start(&cx).text_align(), TextAlign::Left);
    assert_eq!(text("hi").text_end(&cx).text_align(), TextAlign::Right);
    let d_ltr = div().ps(&cx, 12).pe(&cx, 4).justify_start_localized(&cx);
    assert_eq!((d_ltr.style.padding.left, d_ltr.style.padding.right), (12, 4));
    assert_eq!(d_ltr.style.justify(), Justify::Start);

    cx.set_locale(Locale::AR_SA);
    assert_eq!(text("مرحبا").text_start(&cx).text_align(), TextAlign::Right);
    assert_eq!(text("مرحبا").text_end(&cx).text_align(), TextAlign::Left);
    let d_rtl = div().ps(&cx, 12).pe(&cx, 4).justify_start_localized(&cx);
    assert_eq!((d_rtl.style.padding.left, d_rtl.style.padding.right), (4, 12));
    assert_eq!(d_rtl.style.justify(), Justify::End);
}

#[test]
fn test_cldr_plural_rules_and_static_catalog_interpolation() {
    // 英语复数
    assert_eq!(Locale::EN_US.plural_category(1), PluralCategory::One);
    assert_eq!(Locale::EN_US.plural_category(0), PluralCategory::Other);
    assert_eq!(Locale::EN_US.plural_category(5), PluralCategory::Other);

    // 法语复数（0 和 1 均归入 One）
    assert_eq!(Locale::FR_FR.plural_category(0), PluralCategory::One);
    assert_eq!(Locale::FR_FR.plural_category(1), PluralCategory::One);
    assert_eq!(Locale::FR_FR.plural_category(2), PluralCategory::Other);

    // 俄语东斯拉夫 CLDR 复数
    assert_eq!(Locale::RU_RU.plural_category(1), PluralCategory::One);
    assert_eq!(Locale::RU_RU.plural_category(21), PluralCategory::One);
    assert_eq!(Locale::RU_RU.plural_category(11), PluralCategory::Many);
    assert_eq!(Locale::RU_RU.plural_category(3), PluralCategory::Few);
    assert_eq!(Locale::RU_RU.plural_category(14), PluralCategory::Many);

    // 阿拉伯语 6 档 CLDR 复数
    assert_eq!(Locale::AR_SA.plural_category(0), PluralCategory::Zero);
    assert_eq!(Locale::AR_SA.plural_category(1), PluralCategory::One);
    assert_eq!(Locale::AR_SA.plural_category(2), PluralCategory::Two);
    assert_eq!(Locale::AR_SA.plural_category(5), PluralCategory::Few);
    assert_eq!(Locale::AR_SA.plural_category(25), PluralCategory::Many);
    assert_eq!(Locale::AR_SA.plural_category(100), PluralCategory::Other);

    // FrameScope 静态查表 + BCP-47 回退 + 参数插值 + 复数翻译（零堆）
    let mut cx = FrameScope::new();
    cx.set_i18n_catalog(&TEST_CATALOG);

    // 1. 英文
    cx.set_locale(Locale::EN_US);
    assert_eq!(cx.t("title").as_str(), "Velox Workbench");
    assert_eq!(cx.t_plural("files", 1).as_str(), "1 file");
    assert_eq!(cx.t_plural("files", 42).as_str(), "42 files");
    assert_eq!(
        cx.t_args("greeting", &[("name", "World".into())]).as_str(),
        "Hello, World! (World)"
    );

    // 2. 简体中文 + BCP-47 层级回退（zh-Hans-SG -> 同语言 zh-CN -> 缺失键回退 en-US）
    cx.set_locale(Locale::parse("zh-Hans-SG").unwrap());
    assert_eq!(cx.t("title").as_str(), "Velox 工作台");
    assert_eq!(cx.t("only_en").as_str(), "Fallback English Only");
    assert_eq!(cx.t_plural("files", 7).as_str(), "7 个文件");
    assert_eq!(
        cx.t_args("greeting", &[("name", "小明".into())]).as_str(),
        "你好，小明！（小明）"
    );

    // 3. 阿拉伯语复数查表
    cx.set_locale(Locale::AR_SA);
    assert_eq!(cx.t_plural("files", 0).as_str(), "لا توجد ملفات");
    assert_eq!(cx.t_plural("files", 1).as_str(), "ملف واحد");
    assert_eq!(cx.t_plural("files", 2).as_str(), "ملفان");
    assert_eq!(cx.t_plural("files", 5).as_str(), "5 ملفات");

    // 4. 转义大括号 {{ / }} 与缺失占位符安全保留
    let escaped = format_template(cx.arena(), "{{key}} = {val} / {missing}", &[("val", 99i32.into())]);
    assert_eq!(escaped.as_str(), "{key} = 99 / {missing}");
}

#[test]
fn test_engine_hot_locale_switch_and_dynamic_json_kv_catalog() {
    use std::sync::Arc;

    struct I18nDemoView {
        last_title: std::string::String,
    }
    impl View for I18nDemoView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let t = cx.t("title");
            self.last_title.clear();
            self.last_title.push_str(t.as_str());
            div()
                .size_full()
                .child(cx, text(t).size(16).color(Color::WHITE))
                .into_element(cx)
        }
    }

    // 1. 验证 DynamicCatalog 零依赖解析扁平 JSON 与 key=value 语言包
    let mut dyn_cat = DynamicCatalog::new(Locale::EN_US);
    dyn_cat.merge_static(&TEST_CATALOG);
    let json_n = dyn_cat.load_json(
        Locale::JA_JP,
        r#"{
            "title": "Velox \u30ef\u30fc\u30af\u30d9\u30f3\u30c1",
            "greeting": "こんにちは、{name}さん！"
        }"#,
    );
    assert_eq!(json_n, 2);

    let kv_n = dyn_cat.load_kv(
        Locale::DE_DE,
        "# Deutsch language pack\ntitle = Velox Arbeitsplatz\nwelcome = Hallo {user}\\nWillkommen",
    );
    assert_eq!(kv_n, 2);

    let mut engine = VeloxEngine::new(320, 120);
    engine.set_i18n_catalog(&TEST_CATALOG);
    engine.set_dynamic_catalog(Arc::new(dyn_cat));
    let root = engine.mount_root(I18nDemoView {
        last_title: std::string::String::new(),
    });

    engine.step_frame();
    assert_eq!(engine.view(root).unwrap().last_title, "Velox Workbench");

    // 2. 热切换至中文 -> 自动标脏并立即重绘
    engine.set_locale(Locale::ZH_CN);
    assert!(engine.needs_redraw());
    let dmg = engine.step_frame();
    assert!(!dmg.is_empty());
    assert_eq!(engine.view(root).unwrap().last_title, "Velox 工作台");

    // 3. 热切换至 JSON 加载的日文（含 \uXXXX 解码）
    engine.set_locale(Locale::JA_JP);
    engine.step_frame();
    assert_eq!(engine.view(root).unwrap().last_title, "Velox ワークベンチ");

    // 4. 热切换至 key=value 加载的德文
    engine.set_locale(Locale::DE_DE);
    engine.step_frame();
    assert_eq!(engine.view(root).unwrap().last_title, "Velox Arbeitsplatz");

    // 5. 探测宿主系统默认语言（确保 Windows/Unix 零崩溃）
    let sys_loc = detect_system_locale();
    assert!(!sys_loc.language().is_empty());
}
