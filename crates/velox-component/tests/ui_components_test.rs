//! `velox-ui` 标准组件库（`Slider`、`Tabs`、`Checkbox`、`Modal`、`ArcGauge`）集成测试。

use velox_core::prelude::*;
use velox_component::{ArcGauge, Checkbox, Modal, Slider, Tabs};

struct ControlPanel {
    tab_idx: usize,
    brightness: i32,
    auto_mode: bool,
    show_modal: bool,
}

impl View for ControlPanel {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        const TAB_LABELS: &[&str] = &["控制", "设置", "关于"];
        let mut root = div()
            .flex_col()
            .size_full()
            .p(10)
            .gap(10)
            .child(
                cx,
                Tabs::new(100, TAB_LABELS, self.tab_idx)
                    .h(30)
                    .on_select(cx.listener(|this, idx: usize, acx| {
                        this.tab_idx = idx;
                        acx.notify();
                    })),
            )
            .child(
                cx,
                Slider::new(200, self.brightness)
                    .range(0, 100)
                    .w(200)
                    .h(24)
                    .on_change(cx.listener(|this, val: i32, acx| {
                        this.brightness = val;
                        acx.notify();
                    })),
            )
            .child(
                cx,
                Checkbox::new(300, self.auto_mode)
                    .label("自动调光")
                    .on_change(cx.listener(|this, checked: bool, acx| {
                        this.auto_mode = checked;
                        acx.notify();
                    })),
            )
            .child(
                cx,
                ArcGauge::new(500, 64, self.brightness as u8)
                    .thickness(6)
                    .label("BRI"),
            );

        if self.show_modal {
            root = root.child(
                cx,
                Modal::new(400, "系统提示")
                    .size(200, 100)
                    .content(cx, text("配置已保存")),
            );
        }

        root.into_element(cx)
    }
}

#[test]
fn test_slider_tabs_checkbox_modal_and_arc_gauge() {
    let mut engine = VeloxEngine::new(320, 240);
    let handle = engine.mount_root(ControlPanel {
        tab_idx: 0,
        brightness: 20,
        auto_mode: false,
        show_modal: true,
    });

    // 首帧渲染与光栅化验证（包含 Modal 叠层与 ArcGauge 圆弧）
    let d0 = engine.step_frame();
    assert_eq!(d0.len(), 1);
    let r0 = d0[0];
    let mut fb = [Rgb565(0); 320 * 240];
    engine.rasterize_region(r0, &mut fb);
    assert!(fb.iter().any(|p| p.0 != 0));

    // 0. 当 Modal 开启时，Modal 的 `.overlay().occlude_hit()` 应拦截对底层 Tabs 的点击穿透
    assert!(!engine.dispatch_click(160, 25));
    assert_eq!(engine.view(handle).unwrap().tab_idx, 0);

    // 关闭 Modal 弹窗后，底层控件恢复交互
    engine.view_mut(handle).unwrap().show_modal = false;
    engine.step_frame();

    // 1. 点击第二个 Tab ("设置", 位于 y ≈ 10..40, x ≈ 160)
    assert!(engine.dispatch_click(160, 25));
    assert_eq!(engine.view(handle).unwrap().tab_idx, 1);

    // 2. 拖拽 Slider (位于 x: 10..210, y: 50..74) 到右端
    assert!(engine.dispatch_pointer_down(20, 60));
    assert!(engine.dispatch_pointer_move(205, 60));
    engine.dispatch_pointer_up(205, 60);
    assert!(engine.view(handle).unwrap().brightness >= 95);

    // 3. 点击 Checkbox (位于 y: 84..102, x: 15)
    assert!(engine.dispatch_click(20, 92));
    assert!(engine.view(handle).unwrap().auto_mode);
}

#[test]
fn test_extended_widgets_and_navigator() {
    use velox_component::{
        Chart, ChartKind, Dropdown, KeyAction, KeyboardMode, Navigator, PageTransition, Roller,
        Spinner, TextInput, Toast, VirtualKeyboard,
    };

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    enum Page {
        Home,
        ChartPage,
        InputPage,
    }

    let mut nav: Navigator<Page, 4> = Navigator::new(Page::Home);
    assert_eq!(nav.current(), Page::Home);
    assert!(!nav.can_pop());

    assert!(nav.push(Page::ChartPage, PageTransition::SlideLeft, 200));
    assert_eq!(nav.current(), Page::ChartPage);
    assert_eq!(nav.depth(), 2);
    let (ox, oy, alpha) = nav.step_transition(100, 320, 240);
    assert!(ox > 0 && ox < 320);
    assert_eq!(oy, 0);
    assert_eq!(alpha, 255);
    let (ox_done, _, _) = nav.step_transition(100, 320, 240);
    assert_eq!(ox_done, 0);

    nav.replace(Page::InputPage, PageTransition::Fade, 100);
    assert_eq!(nav.current(), Page::InputPage);
    assert_eq!(nav.pop(150), Some(Page::InputPage));
    assert_eq!(nav.current(), Page::Home);

    struct ShowcaseView {
        page: u8,
        roller_idx: usize,
        drop_idx: usize,
        drop_open: bool,
        last_key: Option<KeyAction>,
    }

    impl View for ShowcaseView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            const MODES: &[&str] = &["ECO", "NORM", "BOOST", "TURBO"];
            const SERIES: &[i16] = &[10, 45, 30, 80, 60, 95];

            let root = div().flex_col().size_full().p(4).gap(4);
            if self.page == 0 {
                root.child(
                    cx,
                    Roller::new(10, MODES, self.roller_idx)
                        .w(90)
                        .row_h(20)
                        .on_change(cx.listener(|this, idx: usize, acx| {
                            this.roller_idx = idx;
                            acx.notify();
                        })),
                )
                .child(
                    cx,
                    Dropdown::new(20, MODES, self.drop_idx, self.drop_open)
                        .size(120, 24)
                        .on_select(cx.listener(|this, idx: usize, acx| {
                            this.drop_idx = idx;
                            this.drop_open = false;
                            acx.notify();
                        })),
                )
                .child(
                    cx,
                    Chart::new(30, 140, 40, SERIES, 0, 100).kind(ChartKind::Line),
                )
                .child(
                    cx,
                    Chart::new(31, 140, 36, SERIES, 0, 100).kind(ChartKind::Bar),
                )
                .child(
                    cx,
                    TextInput::new(40, "24.5C", true)
                        .placeholder("Target Temp")
                        .size(120, 24),
                )
                .into_element(cx)
            } else {
                root.child(
                    cx,
                    VirtualKeyboard::new(50, KeyboardMode::Numeric)
                        .size(150, 80)
                        .on_key(cx.listener(|this, act: KeyAction, acx| {
                            this.last_key = Some(act);
                            acx.notify();
                        })),
                )
                .child(cx, Spinner::new(60, 28, 120))
                .child(cx, Toast::new(70, "Saved OK", 60, 4).size(120, 24))
                .into_element(cx)
            }
        }
    }

    for page in [0u8, 1u8] {
        let mut engine = VeloxEngine::new(320, 240);
        let _handle = engine.mount_root(ShowcaseView {
            page,
            roller_idx: 1,
            drop_idx: 0,
            drop_open: true,
            last_key: None,
        });
        let damage = engine.step_frame();
        assert_eq!(damage.len(), 1);
        let r = damage[0];
        let mut fb = [Rgb565(0); 320 * 240];
        engine.rasterize_region(r, &mut fb);
        assert!(fb.iter().any(|p| p.0 != 0));
    }
}

#[test]
fn test_editable_text_and_text_input_keyboard_editing() {
    use velox_component::{EditableText, TextInput};

    struct EditorApp {
        buf: EditableText<64>,
        focused: bool,
    }

    impl View for EditorApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let val = self.buf.as_arena_str(cx);
            div()
                .size_full()
                .p(8)
                .child(
                    cx,
                    TextInput::new(100, val, self.focused)
                        .cursor(self.buf.cursor())
                        .size(200, 32)
                        .on_focus(cx.action(|this, acx| {
                            this.focused = true;
                            acx.notify();
                        }))
                        .on_key_input(cx.listener(|this, key: KeyCode, acx| {
                            if this.buf.handle_key(key) {
                                acx.notify();
                            }
                        })),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 80);
    let h = engine.mount_root(EditorApp {
        buf: EditableText::from_str("Hi"),
        focused: true,
    });
    engine.step_frame();

    // 点击激活焦点并将键盘字符路由到 TextInput
    assert!(engine.dispatch_click(20, 20));
    assert_eq!(engine.focused_index(), Some(0));

    assert!(engine.dispatch_key(KeyCode::Char('!')));
    assert_eq!(engine.view(h).unwrap().buf.as_str(), "Hi!");

    // 光标左移一位，插入中文字符 '好'，验证 UTF-8 字符边界安全性
    assert!(engine.dispatch_key(KeyCode::Left));
    assert!(engine.dispatch_key(KeyCode::Char('好')));
    assert_eq!(engine.view(h).unwrap().buf.as_str(), "Hi好!");

    // Backspace 删除 '好'
    assert!(engine.dispatch_key(KeyCode::Backspace));
    assert_eq!(engine.view(h).unwrap().buf.as_str(), "Hi!");

    // Delete 删除光标后的 '!'
    assert!(engine.dispatch_key(KeyCode::Delete));
    assert_eq!(engine.view(h).unwrap().buf.as_str(), "Hi");
}

#[test]
fn test_themed_components_apply_theme_styles() {
    use velox_component::{
        Alert, AlertVariant, Kbd, Menu, MenuItem, Select, SelectOption, Tag, TagVariant,
    };

    struct ThemedSuiteView;

    impl View for ThemedSuiteView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            const ITEMS: &[MenuItem<'static>] = &[
                MenuItem::new("Open", "Ctrl+O"),
                MenuItem::separator(),
                MenuItem::new("Delete", "Del").destructive(true),
            ];
            const OPTS: &[SelectOption<'static>] = &[SelectOption::new("Dark", "dark")];

            div()
                .flex_col()
                .size_full()
                .gap(4)
                .child(cx, Alert::new(10, "Ready", "Themed").variant(AlertVariant::Success))
                .child(cx, Menu::new(20, ITEMS))
                .child(cx, Select::new(30, OPTS, Some(0), false))
                .child(cx, Tag::new(40, "v1.0").variant(TagVariant::Primary))
                .child(cx, Kbd::new(50, "Ctrl+P"))
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 240);
    engine.mount_root(ThemedSuiteView);
    let damage = engine.step_frame();
    assert!(!damage.is_empty());
    assert!(!engine.cached_draw_ops().is_empty());
}

#[test]
fn test_layered_component_types_and_tailwind_styles() {
    use core::any::TypeId;
    use velox_component::{Button, Card, Modal, Sizable, TailwindStyled, Theme, Variants};

    // velox-component 拥有独立的 Tailwind 主题化包装类型，不再与 velox-base 类型混同
    assert_ne!(
        TypeId::of::<Button<'static>>(),
        TypeId::of::<velox_base::Button<'static>>()
    );
    assert_ne!(
        TypeId::of::<Modal<'static>>(),
        TypeId::of::<velox_base::Modal<'static>>()
    );
    assert_ne!(
        TypeId::of::<Card<'static>>(),
        TypeId::of::<velox_base::Card<'static>>()
    );

    // 包装组件支持直接降级为底层 velox-base 原语，并支持 Tailwind 风格链式修饰
    let themed_btn = Button::new(1, "Save").primary().large();
    let _base_btn: velox_base::Button<'static> = themed_btn.into_base();

    let theme = Theme::default();
    let card = Card::new().tw_card_surface(&theme);
    assert_eq!(card.inner.inner.style.bg, theme.card);
    assert_eq!(card.inner.inner.style.border_px(), 1);
}

#[test]
fn test_themed_button_variants_switch_transition_and_destructive_alert() {
    use velox_component::{AlertDialog, Button, Switch, SwitchLabelSide, Variants};

    struct ThemedVariantsApp {
        clicks: u32,
        switched: bool,
    }
    impl View for ThemedVariantsApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_click = cx.listener(|this, _: ClickEvent, _| this.clicks += 1);
            let on_sw = cx.listener(|this, v: bool, _| this.switched = v);
            div()
                .flex_col()
                .size_full()
                .gap(6)
                .child(cx, Button::new(10, "Danger").danger().w(80).h(28).on_click(on_click))
                .child(cx, Button::new(11, "Success").success().selected(true).w(80).h(28))
                .child(cx, Button::new(12, "Bare").unstyled().w(80).h(28))
                .child(
                    cx,
                    Switch::new(13, self.switched)
                        .label("Enable Sync")
                        .label_side(SwitchLabelSide::Left)
                        .on_change(on_sw),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(280, 180);
    let root = engine.mount_root(ThemedVariantsApp {
        clicks: 0,
        switched: false,
    });
    engine.step_frame();

    let dark = CoreTheme::dark();
    let ops = engine.cached_draw_ops();
    // 1. Danger 与 Success 按钮正确应用 Shadcn 语义背景色与圆角 6
    assert!(ops
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, rounded: 6, .. } if bg == dark.danger)));
    assert!(ops
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, rounded: 6, .. } if bg == dark.success)));
    // 2. `.unstyled()` 逃生舱剥离全部背景与边框（核心引擎直接跳过多余 Quad，仅保留文字与交互热区）
    assert!(
        !ops.iter().any(|op| op.id == Some(12)),
        "unstyled Button must emit zero background/border Quad ops"
    );
    assert!(ops.iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, .. } if text.as_str() == "Bare"
    )));

    // 3. 切换 Switch 后 300ms 过渡精确抵达 theme.primary
    engine.view_mut(root).unwrap().switched = true;
    engine.step_frame_dt(16);
    engine.step_frame_dt(300);
    assert!(engine
        .cached_draw_ops()
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, .. } if bg == dark.primary)));

    // 4. AlertDialog::destructive(true) 应用 theme.danger 并支持 Escape 取消
    struct AlertApp {
        cancelled: bool,
    }
    impl View for AlertApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_cancel = cx.action(|this, _| this.cancelled = true);
            AlertDialog::new(200, "Delete Project", "Irreversible action")
                .destructive(true)
                .on_cancel(on_cancel)
                .into_element(cx)
        }
    }
    let mut eng2 = VeloxEngine::new(400, 260);
    let r2 = eng2.mount_root(AlertApp { cancelled: false });
    eng2.step_frame();
    assert!(eng2
        .cached_draw_ops()
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, .. } if bg == dark.danger)));
    assert!(eng2.dispatch_key(KeyCode::Escape));
    assert!(eng2.view(r2).unwrap().cancelled);
}

#[test]
fn test_themed_focus_visible_blended_borders_and_slider_no_border() {
    use velox_component::{Button, Checkbox, Radio, Slider, Switch, TextInput, Variants};

    struct FocusDemoApp {
        btn_clicks: u32,
        slider_val: i32,
        checked: bool,
        switched: bool,
        radio_sel: bool,
        input_focused: bool,
    }

    impl View for FocusDemoApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_btn = cx.listener(|this, _: ClickEvent, _| this.btn_clicks += 1);
            let on_slide = cx.listener(|this, v: i32, _| this.slider_val = v);
            let on_chk = cx.listener(|this, v: bool, _| this.checked = v);
            let on_sw = cx.listener(|this, v: bool, _| this.switched = v);
            let on_rad = cx.action(|this, _| this.radio_sel = true);
            let on_in_focus = cx.action(|this, _| this.input_focused = true);
            let on_in_key = cx.listener(|_, _: KeyCode, _| {});

            div()
                .size_full()
                .flex_col()
                .p(16)
                .gap(12)
                .bg(cx.theme().background)
                .child(cx, Button::new(501, "Delete").danger().w(100).h(32).on_click(on_btn))
                .child(cx, Button::new(502, "Outline").outline().w(100).h(32).on_click(on_btn))
                .child(
                    cx,
                    Slider::new(503, self.slider_val)
                        .range(0, 100)
                        .step(10)
                        .w(200)
                        .on_change(on_slide),
                )
                .child(cx, Checkbox::new(504, self.checked).label("Enable Sync").on_change(on_chk))
                .child(cx, Switch::new(505, self.switched).label("Auto Save").on_change(on_sw))
                .child(cx, Radio::new(506, "Option A", self.radio_sel).on_select(on_rad))
                .child(
                    cx,
                    TextInput::new(507, "hello", self.input_focused)
                        .size(200, 32)
                        .on_focus(on_in_focus)
                        .on_key_input(on_in_key),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(400, 360);
    let dark = CoreTheme::dark();
    let root = engine.mount_root(FocusDemoApp {
        btn_clicks: 0,
        slider_val: 50,
        checked: false,
        switched: false,
        radio_sel: false,
        input_focused: false,
    });
    engine.step_frame();

    let hit_center = |eng: &VeloxEngine, id: u32| -> (i16, i16) {
        let h = eng
            .hit_regions()
            .iter()
            .find(|h| h.id == Some(id))
            .expect("hit region must exist");
        (h.bounds.x + h.bounds.w / 2, h.bounds.y + h.bounds.h / 2)
    };

    // 1. 鼠标点击 Danger 按钮不绘制焦环
    let (bx, by) = hit_center(&engine, 501);
    engine.dispatch_pointer_down(bx, by);
    engine.dispatch_pointer_up(bx, by);
    engine.step_frame();
    assert_eq!(engine.view(root).unwrap().btn_clicks, 1);
    assert!(!engine.is_focus_visible());
    let danger_op = engine.cached_draw_ops().iter().find(|op| op.id == Some(501)).unwrap();
    if let DrawPayload::Quad { border_width, .. } = danger_op.payload {
        assert_eq!(border_width & 0x7F, 0);
    }

    // 2. 点击空白清除焦点后，键盘 Tab 聚焦 Danger -> 融合浅红焦环，绝非蓝框
    engine.dispatch_pointer_down(390, 350);
    engine.dispatch_pointer_up(390, 350);
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame();
    let danger_tab = engine.cached_draw_ops().iter().find(|op| op.id == Some(501)).unwrap();
    if let DrawPayload::Quad {
        border_width,
        border_color,
        ..
    } = danger_tab.payload
    {
        assert_eq!(border_width & 0x7F, 1);
        assert_eq!(border_color, dark.danger.lerp(Color::WHITE, 128));
    }

    // 3. 键盘 Tab 聚焦 Outline -> 1px theme.ring 焦环
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame();
    let outline_tab = engine.cached_draw_ops().iter().find(|op| op.id == Some(502)).unwrap();
    if let DrawPayload::Quad {
        border_width,
        border_color,
        ..
    } = outline_tab.payload
    {
        assert_eq!(border_width & 0x7F, 1);
        assert_eq!(border_color, dark.ring);
    }

    // 4. 键盘 Tab 聚焦 Slider：不画矩形外框且方向键可调值
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame();
    assert!(!engine.cached_draw_ops().iter().any(|op| {
        op.id == Some(503)
            && matches!(op.payload, DrawPayload::Quad { border_width, .. } if (border_width & 0x7F) > 0)
    }));
    assert!(engine.dispatch_key(KeyCode::Right));
    assert_eq!(engine.view(root).unwrap().slider_val, 60);

    // 5. 鼠标点击 TextInput：呈现 1px theme.ring 焦点边框
    let (ix, iy) = hit_center(&engine, 507);
    engine.dispatch_pointer_down(ix, iy);
    engine.dispatch_pointer_up(ix, iy);
    engine.step_frame();
    let input_op = engine.cached_draw_ops().iter().find(|op| op.id == Some(507)).unwrap();
    if let DrawPayload::Quad {
        border_width,
        border_color,
        ..
    } = input_op.payload
    {
        assert_eq!(border_width & 0x7F, 1);
        assert_eq!(border_color, dark.ring);
    }
}

#[test]
fn test_themed_explicit_style_preserved_and_root_theme_injection() {
    use velox_component::{Button, Root, Theme};

    let custom_red = Color::rgb(220, 38, 38);

    struct CustomAndThemeView {
        custom_bg: Color,
    }
    impl View for CustomAndThemeView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let light = Theme::light();
            Root::new(1, 320, 240)
                .theme(light)
                .content_with(cx, |cx| {
                    div()
                        .flex_col()
                        .gap(8)
                        .child(cx, Button::new(10, "ExplicitRed").bg(self.custom_bg))
                        .child(cx, Button::new(11, "ThemedPrimary"))
                })
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 240);
    engine.set_theme(Theme::dark());
    engine.mount_root(CustomAndThemeView {
        custom_bg: custom_red,
    });
    let _ = engine.step_frame();

    let ops = engine.cached_draw_ops();
    // 1. 显式调用 `.bg(custom_red)` 的主题化 Button 必须保留 `custom_red` 且自动拥有圆角 6
    assert!(ops.iter().any(|op| {
        matches!(op.payload, DrawPayload::Quad { bg, rounded: 6, .. } if bg == custom_red)
    }));

    // 2. 未自定义背景色的 Button 与 Root 必须读取 Root 注入的 Theme::light() 配色
    let light_theme = Theme::light();
    assert!(ops
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, .. } if bg == light_theme.background)));
    assert!(ops
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, .. } if bg == light_theme.primary)));
}

#[test]
fn test_all_velox_component_widgets_implement_gpui_styled_ext() {
    use velox_component::*;
    fn assert_gpui_widget<T: Styled + StyledExt + FluentBuilder>() {}

    assert_gpui_widget::<Accordion<'static>>();
    assert_gpui_widget::<Alert<'static>>();
    assert_gpui_widget::<AlertDialog<'static>>();
    assert_gpui_widget::<ArcGauge<'static>>();
    assert_gpui_widget::<Attachment<'static>>();
    assert_gpui_widget::<Avatar<'static>>();
    assert_gpui_widget::<Badge<'static>>();
    assert_gpui_widget::<BottomSheet<'static>>();
    assert_gpui_widget::<Bubble<'static>>();
    assert_gpui_widget::<Button<'static>>();
    assert_gpui_widget::<Calendar<'static>>();
    assert_gpui_widget::<Card<'static>>();
    assert_gpui_widget::<Carousel<'static>>();
    assert_gpui_widget::<Chart<'static>>();
    assert_gpui_widget::<Checkbox<'static>>();
    assert_gpui_widget::<CodeEditor<'static>>();
    assert_gpui_widget::<Collapsible<'static>>();
    assert_gpui_widget::<ColorPicker<'static>>();
    assert_gpui_widget::<ColorSwatch<'static>>();
    assert_gpui_widget::<Combobox<'static>>();
    assert_gpui_widget::<Command<'static>>();
    assert_gpui_widget::<ContextMenu<'static>>();
    assert_gpui_widget::<DataTable<'static>>();
    assert_gpui_widget::<DatePicker<'static>>();
    assert_gpui_widget::<DescriptionList<'static>>();
    assert_gpui_widget::<Dialog<'static>>();
    assert_gpui_widget::<Diff<'static>>();
    assert_gpui_widget::<Dock<'static>>();
    assert_gpui_widget::<Dropdown<'static>>();
    assert_gpui_widget::<DropdownButton<'static>>();
    assert_gpui_widget::<Form<'static>>();
    assert_gpui_widget::<FormField<'static>>();
    assert_gpui_widget::<GridLayout<'static>>();
    assert_gpui_widget::<GroupBox<'static>>();
    assert_gpui_widget::<HoverCard<'static>>();
    assert_gpui_widget::<Icon<'static>>();
    assert_gpui_widget::<Image<'static>>();
    assert_gpui_widget::<Kbd<'static>>();
    assert_gpui_widget::<Label<'static>>();
    assert_gpui_widget::<Link<'static>>();
    assert_gpui_widget::<List<'static>>();
    assert_gpui_widget::<MarkdownView<'static>>();
    assert_gpui_widget::<Marker<'static>>();
    assert_gpui_widget::<Menu<'static>>();
    assert_gpui_widget::<Message<'static>>();
    assert_gpui_widget::<MessageScroller<'static>>();
    assert_gpui_widget::<Modal<'static>>();
    assert_gpui_widget::<NavBar<'static>>();
    assert_gpui_widget::<Notification<'static>>();
    assert_gpui_widget::<NumberInput<'static>>();
    assert_gpui_widget::<OtpInput<'static>>();
    assert_gpui_widget::<Pagination<'static>>();
    assert_gpui_widget::<Popover<'static>>();
    assert_gpui_widget::<ProgressBar>();
    assert_gpui_widget::<Questionnaire<'static, 8>>();
    assert_gpui_widget::<Radio<'static>>();
    assert_gpui_widget::<RadioGroup<'static>>();
    assert_gpui_widget::<Rating<'static>>();
    assert_gpui_widget::<ResizableSplit<'static>>();
    assert_gpui_widget::<Roller<'static>>();
    assert_gpui_widget::<Root<'static>>();
    assert_gpui_widget::<Scrollable<'static>>();
    assert_gpui_widget::<Scrollbar>();
    assert_gpui_widget::<Select<'static>>();
    assert_gpui_widget::<SelectableText<'static>>();
    assert_gpui_widget::<Settings<'static>>();
    assert_gpui_widget::<Sheet<'static>>();
    assert_gpui_widget::<Sidebar<'static>>();
    assert_gpui_widget::<Skeleton>();
    assert_gpui_widget::<Slider<'static>>();
    assert_gpui_widget::<Speech<'static>>();
    assert_gpui_widget::<Spinner>();
    assert_gpui_widget::<StatusBar<'static>>();
    assert_gpui_widget::<Stepper<'static>>();
    assert_gpui_widget::<Svg<'static>>();
    assert_gpui_widget::<Switch<'static>>();
    assert_gpui_widget::<TabBar<'static>>();
    assert_gpui_widget::<Table<'static>>();
    assert_gpui_widget::<Tabs<'static>>();
    assert_gpui_widget::<Tag<'static>>();
    assert_gpui_widget::<TextInput<'static>>();
    assert_gpui_widget::<TextView<'static>>();
    assert_gpui_widget::<Textarea<'static>>();
    assert_gpui_widget::<TimeField<'static>>();
    assert_gpui_widget::<Toast<'static>>();
    assert_gpui_widget::<Toggle<'static>>();
    assert_gpui_widget::<ToggleGroup<'static>>();
    assert_gpui_widget::<Toolbar<'static>>();
    assert_gpui_widget::<Tooltip<'static>>();
    assert_gpui_widget::<Tree<'static>>();
    assert_gpui_widget::<UniformList<'static, fn(usize, &'static FrameScope) -> Element<'static>>>();
    assert_gpui_widget::<VirtualKeyboard<'static>>();

    // 静态验证按单组件拆分后的子模块路径导出的是主题化组件（与 velox_base::components::<name> 1:1 镜像）
    let _: fn(velox_component::components::button::Button<'static>) -> Button<'static> = |x| x;
    let _: fn(velox_component::components::calendar::Calendar<'static>) -> Calendar<'static> = |x| x;
    let _: fn(velox_component::components::switch::Switch<'static>) -> Switch<'static> = |x| x;
    let _: fn(velox_component::components::table::Table<'static>) -> Table<'static> = |x| x;
    let _: fn(velox_component::components::dialog::Dialog<'static>) -> Dialog<'static> = |x| x;
}

#[test]
fn test_themed_components_slot_forwarding_and_styled_ext() {
    use velox_component::*;
    struct ThemedSlotView {
        sw: bool,
        cb: bool,
    }

    impl View for ThemedSlotView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_sw = cx.listener(|this, v: bool, _| this.sw = v);
            let on_cb = cx.listener(|this, v: bool, _| this.cb = v);
            div()
                .size_full()
                .flex_col()
                .p_3()
                .gap_3()
                .child(
                    cx,
                    Switch::new(8801, self.sw)
                        .label("ThemedSwitch")
                        .on_change(on_sw)
                        .track(cx, |tr, st| {
                            tr.w(px(38.))
                                .h(px(20.))
                                .rounded_full()
                                .p_0_5()
                                .transition_colors()
                                .when(st.is_checked(), |s| s.bg(rgb(0x10b981)))
                                .when(!st.is_checked(), |s| s.bg(rgb(0x52525b)))
                        })
                        .thumb(cx, |th, _| th.size_4().rounded_full().bg(rgb(0xffffff))),
                )
                .child(
                    cx,
                    Checkbox::new(8802, self.cb)
                        .label("ThemedCB")
                        .on_change(on_cb)
                        .indicator(cx, |el, st| {
                            el.size_4()
                                .rounded_sm()
                                .border_1()
                                .border_color(rgb(0x3b82f6))
                                .when(st.is_checked(), |s| s.bg(rgb(0x3b82f6)))
                        }),
                )
                .child(
                    cx,
                    Button::new(8803, "")
                        .h_8()
                        .px_3()
                        .rounded_md()
                        .transition_colors()
                        .hover(|s| s.opacity_f(0.9))
                        .child(cx, text("ThemedSlotBtn")),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 240);
    let root = engine.mount_root(ThemedSlotView {
        sw: false,
        cb: false,
    });
    engine.step_frame();

    let find_text_center = |eng: &VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle))
            .expect("expected text run");
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    let (sx, sy) = find_text_center(&engine, "ThemedSwitch");
    assert!(engine.dispatch_click(sx, sy));
    assert!(engine.view(root).unwrap().sw);

    let (cx_p, cy_p) = find_text_center(&engine, "ThemedCB");
    assert!(engine.dispatch_click(cx_p, cy_p));
    assert!(engine.view(root).unwrap().cb);

    let _ = find_text_center(&engine, "ThemedSlotBtn");
}

#[test]
fn test_themed_widgets_zero_heap_signal_bind() {
    use velox_component::*;

    struct ThemedBindView {
        sw: Signal<bool>,
        cb: Signal<bool>,
        tab: Signal<usize>,
        sl: Signal<i32>,
    }

    impl View for ThemedBindView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_col()
                .p_3()
                .gap_2()
                .child(cx, Switch::bind(cx, 8901, self.sw).label("TwBindSW"))
                .child(cx, Checkbox::bind(cx, 8902, self.cb).label("TwBindCB"))
                .child(cx, Tabs::bind(cx, 8903, &["TabA", "TabB"], self.tab))
                .child(cx, Slider::bind(cx, 8904, self.sl).range(0, 100).w(140))
                .into_element(cx)
        }
    }

    let sw = signal(false);
    let cb = signal(false);
    let tab = signal(0usize);
    let sl = signal(50i32);

    let mut engine = VeloxEngine::new(320, 240);
    let _root = engine.mount_root(ThemedBindView { sw, cb, tab, sl });
    engine.step_frame();

    let find_text = |eng: &VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle))
            .unwrap_or_else(|| panic!("expected text run {needle:?}"));
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    let (x, y) = find_text(&engine, "TwBindSW");
    assert!(engine.dispatch_click(x, y));
    assert!(sw.get_untracked());

    let (x, y) = find_text(&engine, "TwBindCB");
    assert!(engine.dispatch_click(x, y));
    assert!(cb.get_untracked());

    let (x, y) = find_text(&engine, "TabB");
    assert!(engine.dispatch_click(x, y));
    assert_eq!(tab.get_untracked(), 1);

    sw.dispose();
    cb.dispose();
    tab.dispose();
    sl.dispose();
}
