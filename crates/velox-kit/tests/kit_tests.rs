use velox_kit::prelude::*;

struct KitApp {
    clicks: u32,
}

impl View for KitApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let on_click = cx.listener(|this, _: ClickEvent, _| {
            this.clicks += 1;
        });

        #[cfg(feature = "component")]
        let btn = Button::new(100, "Launch").primary().medium().on_click(on_click);

        #[cfg(all(not(feature = "component"), feature = "base"))]
        let btn = Button::new(100, "Launch")
            .px(14)
            .py(6)
            .bg(Color::WHITE)
            .text_color(Color::rgb(9, 9, 11))
            .on_click(on_click);

        div()
            .id(1)
            .size_full()
            .flex_col()
            .p(16)
            .gap(12)
            .bg(cx.theme().background)
            .child(cx, text("Velox Kit").size(16).color(cx.theme().foreground))
            .child(cx, btn)
            .into_element(cx)
    }
}

#[test]
fn test_velox_kit_unified_prelude_and_click() {
    let mut engine = VeloxEngine::new(320, 200);
    let handle = engine.mount_root(KitApp { clicks: 0 });
    engine.step_frame();

    let btn_bounds = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(100))
        .expect("Kit Button draw op must exist")
        .bounds;

    assert!(engine.dispatch_click(
        btn_bounds.x + btn_bounds.w / 2,
        btn_bounds.y + btn_bounds.h / 2,
    ));
    assert_eq!(engine.view(handle).unwrap().clicks, 1);
}

#[test]
fn test_velox_kit_layer_exports_match_gpui_architecture() {
    // 1. 无论是否启用 component，velox_kit::base / Base* 永远导出无样式原语
    #[cfg(feature = "base")]
    {
        struct BaseOnlyView;
        impl View for BaseOnlyView {
            fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
                div()
                    .size_full()
                    .child(cx, velox_kit::BaseButton::new(201, "Bare"))
                    .into_element(cx)
            }
        }
        let mut engine = VeloxEngine::new(200, 80);
        engine.mount_root(BaseOnlyView);
        engine.step_frame();
        // 裸 BaseButton 无背景/边框/阴影，核心引擎直接跳过多余 Quad（零绘制开销），仅保留文字与交互热区
        assert!(
            !engine.cached_draw_ops().iter().any(|op| op.id == Some(201)),
            "velox_kit::BaseButton must stay completely unstyled (zero Quad) even when `component` feature is enabled"
        );
        assert!(engine.cached_draw_ops().iter().any(|op| matches!(
            &op.payload,
            DrawPayload::TextRun { text, .. } if text.as_str() == "Bare"
        )));
        assert!(engine.hit_regions().iter().any(|h| h.id == Some(201)));
    }

    // 2. 启用 component feature 时，prelude 与顶层导出指向带主题样式的 velox_component
    #[cfg(feature = "component")]
    {
        assert_eq!(
            core::any::TypeId::of::<velox_kit::Button<'static>>(),
            core::any::TypeId::of::<velox_kit::ThemedButton<'static>>(),
            "Default velox_kit::Button must re-export ThemedButton when `component` is active"
        );
        assert_ne!(
            core::any::TypeId::of::<velox_kit::Button<'static>>(),
            core::any::TypeId::of::<velox_kit::BaseButton<'static>>(),
            "velox_kit::Button (themed) and velox_kit::BaseButton (headless) must be distinct layers"
        );

        struct ThemedView;
        impl View for ThemedView {
            fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
                div()
                    .size_full()
                    .child(cx, velox_kit::Button::new(202, "Themed").primary())
                    .into_element(cx)
            }
        }
        let mut engine = VeloxEngine::new(200, 80);
        engine.mount_root(ThemedView);
        engine.step_frame();
        let op = engine
            .cached_draw_ops()
            .iter()
            .find(|op| op.id == Some(202))
            .expect("Themed Button draw op must exist");
        match op.payload {
            DrawPayload::Quad { bg, rounded, .. } => {
                assert_eq!(bg, CoreTheme::dark().primary);
                assert_eq!(rounded, 6);
            }
            _ => panic!("expected Quad payload for Themed Button"),
        }
    }
}

#[test]
fn test_velox_kit_platform_reexports() {
    use velox_kit::platform::testing::TestingPlatform;
    use velox_kit::platform::{Platform, PlatformControlFlow};

    let mut engine = VeloxEngine::new(160, 100);
    engine.mount_root(KitApp { clicks: 0 });
    let mut plat: TestingPlatform<160, 100> = TestingPlatform::new();
    let flow = plat.step_platform(&mut engine, 16);
    assert_eq!(flow, PlatformControlFlow::Poll);
}

#[test]
fn test_velox_kit_core_theme_reexport() {
    let dark = CoreTheme::dark();
    let light = CoreTheme::light();
    assert_ne!(dark.background, light.background);
    assert_eq!(ThemeMode::Dark.toggle(), ThemeMode::Light);
}

#[test]
fn test_velox_kit_minimal_application_and_open_window_api() {
    let mut handle = None;
    let built = velox_kit::application()
        .with_theme(CoreTheme::light())
        .build_headless(|cx| {
            velox_kit::init(cx);
            assert_eq!(cx.scheduler_name(), "velox-native-std");
            let win = velox_kit::open_window(
                WindowOptions::new("Minimal App").size(400, 240),
                cx,
                |cx| cx.new(|_| KitApp { clicks: 0 }),
            )
            .expect("open_window must succeed");
            handle = Some(win.root());
        })
        .expect("primary window must be created");

    let mut engine = built.engine;
    let root = handle.unwrap();
    assert_eq!(built.options.title, "Minimal App");
    assert_eq!(engine.theme().background, CoreTheme::light().background);
    let _ = engine.step_frame();
    assert_eq!(engine.view(root).unwrap().clicks, 0);
}
