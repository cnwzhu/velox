//! `velox-base` 基础组件行为、状态结构与默认最简无样式集成测试。

use velox_base::{
    uniform_list, Button, Checkbox, Disableable, Navigator, PageTransition, Slider, Switch, Tabs,
};
use velox_engine::platform::testing::TestingPlatform;
use velox_engine::prelude::*;

struct BaseApp {
    count: i32,
    toggled: bool,
    slider_val: i32,
    tab_idx: usize,
    nav: Navigator<&'static str, 4>,
}

impl View for BaseApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let on_btn = cx.listener(|this, _evt: ClickEvent, _acx| {
            this.count += 1;
        });
        let on_toggle = cx.listener(|this, checked: bool, _acx| {
            this.toggled = checked;
        });
        let on_slide = cx.listener(|this, val: i32, _acx| {
            this.slider_val = val;
        });
        let on_tab = cx.listener(|this, idx: usize, _acx| {
            this.tab_idx = idx;
        });

        div()
            .size_full()
            .flex_col()
            .child(cx, Button::new(1, "BaseBtn").w(80).h(28).on_click(on_btn))
            .child(cx, Switch::new(2, self.toggled).on_change(on_toggle))
            .child(cx, Checkbox::new(5, self.toggled).label("Check"))
            .child(
                cx,
                Slider::new(3, self.slider_val)
                    .range(0, 200)
                    .w(120)
                    .h(20)
                    .on_change(on_slide),
            )
            .child(
                cx,
                Tabs::new(4, &["A", "B"], self.tab_idx).on_select(on_tab),
            )
            .child(
                cx,
                uniform_list(6, 100, 16, 64, 0, |i, cx| {
                    div()
                        .h(16)
                        .child(cx, text(velox_engine::fmt!(cx, "Item {}", i)))
                        .into_element(cx)
                }),
            )
            .into_element(cx)
    }
}

#[test]
fn test_velox_base_components_and_navigator() {
    let mut engine = VeloxEngine::new(320, 240);
    let mut plat: TestingPlatform<320, 240> = TestingPlatform::new();
    let root = engine.mount_root(BaseApp {
        count: 0,
        toggled: false,
        slider_val: 50,
        tab_idx: 0,
        nav: Navigator::new("home"),
    });

    {
        let app = engine.view_mut(root).unwrap();
        assert_eq!(app.nav.current(), "home");
        assert!(app.nav.push("settings", PageTransition::SlideLeft, 200));
        assert_eq!(app.nav.depth(), 2);
        assert_eq!(app.nav.pop(200), Some("settings"));
    }

    let _ = plat.step_platform(&mut engine, 16);
    plat.send_click(20, 10);
    let _ = plat.step_platform(&mut engine, 16);
    assert_eq!(engine.view(root).unwrap().count, 1);

    struct DisabledView {
        clicked: bool,
    }
    impl View for DisabledView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let cb = cx.listener(|this, _: ClickEvent, _| {
                this.clicked = true;
            });
            Button::new(10, "Disabled")
                .w(80)
                .h(28)
                .disabled(true)
                .on_click(cb)
                .into_element(cx)
        }
    }
    let mut engine2 = VeloxEngine::new(320, 240);
    let mut plat2: TestingPlatform<320, 240> = TestingPlatform::new();
    let droot = engine2.mount_root(DisabledView { clicked: false });
    let _ = plat2.step_platform(&mut engine2, 16);
    plat2.send_click(10, 10);
    let _ = plat2.step_platform(&mut engine2, 16);
    assert!(!engine2.view(droot).unwrap().clicked);

    // 验证 Navigator 双图层离屏合成转场 (step_dual_transition + composite_transition_buffers)
    let mut nav: Navigator<u8, 4> = Navigator::new(1);
    assert!(!nav.is_transitioning());
    assert!(nav.push(2, PageTransition::SlideLeft, 160));
    assert!(nav.is_transitioning());
    assert_eq!(nav.previous(), Some(1));

    let page1_buf = [Rgb565::from_color(Color::rgb(200, 20, 20)); 100 * 60];
    let page2_buf = [Rgb565::from_color(Color::rgb(20, 40, 220)); 100 * 60];
    let mut dst_buf = [Rgb565(0); 100 * 60];

    // 推进半程 (80ms / 160ms)：左侧为旧页 page1，右侧为新页 page2
    let still_anim = nav.composite_transition_buffers(
        80,
        100,
        60,
        Some(&page1_buf),
        &page2_buf,
        &mut dst_buf,
    );
    assert!(still_anim);
    assert_eq!(dst_buf[10 * 100 + 5], page1_buf[0]);
    assert_eq!(dst_buf[10 * 100 + 95], page2_buf[0]);

    // 推进至完成 (再推进 80ms)：整屏完全为新页 page2
    let still_anim_end = nav.composite_transition_buffers(
        80,
        100,
        60,
        Some(&page1_buf),
        &page2_buf,
        &mut dst_buf,
    );
    assert!(!still_anim_end);
    assert_eq!(nav.previous(), None);
    assert_eq!(dst_buf[10 * 100 + 5], page2_buf[0]);
    assert_eq!(dst_buf[10 * 100 + 95], page2_buf[0]);
}

#[test]
fn test_builtin_widget_i18n_localization() {
    use velox_base::{AlertDialog, Calendar};

    struct LocalizedWidgetsView;
    impl View for LocalizedWidgetsView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .flex_col()
                .size_full()
                .child(cx, Calendar::new(100, 2026, 11))
                .child(cx, AlertDialog::new(200, "Delete", "Sure?"))
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(480, 480);
    let _ = engine.mount_root(LocalizedWidgetsView);

    // 1. 默认英文 en-US
    engine.step_frame();
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "Confirm")
    }));
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "Su")
    }));

    // 2. 热切换至简体中文 zh-CN：Calendar 星期与 AlertDialog 按钮自动本地化
    engine.set_locale(Locale::ZH_CN);
    engine.step_frame();
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "确认")
    }));
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "取消")
    }));
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "日")
    }));

    // 3. 热切换至日语 ja-JP
    engine.set_locale(Locale::JA_JP);
    engine.step_frame();
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "キャンセル")
    }));
    assert!(engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "月")
    }));
}

#[test]
fn test_base_button_headless_default_loading_and_custom_styling() {
    struct BtnApp {
        clicks: u32,
    }
    impl View for BtnApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_click = cx.listener(|this, _: ClickEvent, _| {
                this.clicks += 1;
            });
            div()
                .flex_col()
                .size_full()
                .gap(4)
                .child(
                    cx,
                    Button::new(10, "Save changes")
                        .bg(Color::WHITE)
                        .text_color(Color::rgb(9, 9, 11))
                        .w(120)
                        .h(28)
                        .on_click(on_click),
                )
                .child(
                    cx,
                    Button::new(11, "Cancel")
                        .bg(Color::rgb(20, 20, 23))
                        .border(1, Color::rgb(68, 68, 74))
                        .text_color(Color::WHITE)
                        .selected(true)
                        .w(80)
                        .h(28),
                )
                .child(
                    cx,
                    Button::new(12, "Loading")
                        .loading(true)
                        .w(80)
                        .h(28)
                        .on_click(on_click),
                )
                .child(cx, Button::new(13, "Bare").w(80).h(28))
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 180);
    let root = engine.mount_root(BtnApp { clicks: 0 });
    engine.step_frame();

    let ops = engine.cached_draw_ops();
    // 1. 裸 velox_base::Button 默认完全无样式：不发射任何背景/边框 Quad（零绘制开销），仅保留文本与交互热区
    assert!(
        !ops.iter().any(|op| op.id == Some(13)),
        "Bare velox_base::Button must emit zero background/border Quad ops"
    );
    assert!(ops.iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, .. } if text.as_str() == "Bare"
    )));
    assert!(engine.hit_regions().iter().any(|h| h.id == Some(13)));

    // 2. 手动链式配置白底黑字直角 Save changes 与暗底灰框直角 Cancel 精准生效
    let save_op = ops.iter().find(|op| op.id == Some(10)).expect("Save button quad");
    assert!(matches!(
        save_op.payload,
        DrawPayload::Quad { bg, rounded: 0, .. } if bg == Color::WHITE
    ));
    let cancel_op = ops.iter().find(|op| op.id == Some(11)).expect("Cancel button quad");
    assert!(matches!(
        cancel_op.payload,
        DrawPayload::Quad { bg, border_width, rounded: 0, .. }
            if bg == Color::rgb(20, 20, 23) && (border_width & 0x7F) == 1
    ));

    // 3. Loading 按钮包含原生 SDF 旋转圆弧，且推进时钟后旋转角度真正持续变化并请求连续动画帧
    let first_angle = ops.iter().find_map(|op| match op.payload {
        DrawPayload::Sprite(SpriteSource::Arc { start_deg10, .. }) => Some(start_deg10),
        _ => None,
    });
    assert!(first_angle.is_some(), "Loading button must render animated SDF Arc spinner");
    assert!(engine.needs_redraw(), "Loading button must request continuous animation frames");

    // 验证按钮文字已启用水平居中标志（align_bits == 1）
    assert!(ops.iter().any(|op| matches!(
        op.payload,
        DrawPayload::TextRun { text, text_flags, .. } if text.as_str() == "Save changes" && (text_flags & 0x03) == 1
    )));

    // 推进 150ms：验证旋转等待图标角度切实顺时针递增（且绝不被 Retained SubView 缓存卡死）
    let dirty = engine.step_frame_dt(150);
    assert!(!dirty.is_empty(), "Spinning button must emit dirty region on next frame");
    let second_angle = engine.cached_draw_ops().iter().find_map(|op| match op.payload {
        DrawPayload::Sprite(SpriteSource::Arc { start_deg10, .. }) => Some(start_deg10),
        _ => None,
    });
    assert_ne!(
        first_angle, second_angle,
        "Loading spinner start_deg10 must rotate over time"
    );

    // 点击正常 Save changes 按钮触发回调，点击 Loading 按钮应被阻断
    assert!(engine.dispatch_click(20, 10));
    assert_eq!(engine.view(root).unwrap().clicks, 1);
    let _ = engine.dispatch_click(20, 74); // Loading 按钮行 y≈64..92
    assert_eq!(engine.view(root).unwrap().clicks, 1);
}

#[test]
fn test_base_checkbox_indeterminate_svg_and_switch_label() {
    use velox_base::SwitchLabelSide;

    struct FormApp {
        checked: bool,
        indeterminate: bool,
        switched: bool,
    }
    impl View for FormApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_cb = cx.listener(|this, val: bool, _| {
                this.checked = val;
                this.indeterminate = false;
            });
            let on_sw = cx.listener(|this, val: bool, _| {
                this.switched = val;
            });
            div()
                .flex_col()
                .size_full()
                .gap(8)
                .child(
                    cx,
                    Checkbox::new(20, self.checked)
                        .indeterminate(self.indeterminate)
                        .label("Select All")
                        .on_change(on_cb),
                )
                .child(
                    cx,
                    Switch::new(21, self.switched)
                        .label("Enable Sync")
                        .label_side(SwitchLabelSide::Left)
                        .on_change(on_sw),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 120);
    let root = engine.mount_root(FormApp {
        checked: false,
        indeterminate: true,
        switched: false,
    });
    engine.step_frame();

    // 半选态渲染原生矢量横杠 SVG ("M5 12h14")
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        op.payload,
        DrawPayload::Sprite(SpriteSource::Svg { source, .. }) if source.as_str() == "M5 12h14"
    )));

    // 点击半选态 Checkbox 转为选中态 (true) 并清除半选
    assert!(engine.dispatch_click(10, 8));
    assert!(engine.view(root).unwrap().checked);
    assert!(!engine.view(root).unwrap().indeterminate);

    // 勾选态渲染原生矢量对勾 SVG ("M20 6 9 17l-5-5")
    engine.step_frame();
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        op.payload,
        DrawPayload::Sprite(SpriteSource::Svg { source, .. }) if source.as_str() == "M20 6 9 17l-5-5"
    )));

    // Switch(label_side = Left) 点击左侧标签文字也能切换开关
    assert!(engine.dispatch_click(20, 32));
    assert!(engine.view(root).unwrap().switched);

    // 无样式 Switch 默认不自动填充 theme.primary（主题色皮肤由 velox-component 提供）
    engine.step_frame();
    let theme = CoreTheme::dark();
    assert!(!engine
        .cached_draw_ops()
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, .. } if bg == theme.primary)));
}

#[test]
fn test_base_keyboard_nav_slider_tabs_radiogroup_and_escape_overlays() {
    use velox_base::{AlertDialog, Dialog, Modal, RadioGroup, Select, SelectOption};

    struct NavApp {
        slider_val: i32,
        tab_idx: usize,
        radio_idx: usize,
        select_open: bool,
        select_idx: Option<usize>,
        dialog_open: bool,
        modal_open: bool,
    }
    impl View for NavApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_slide = cx.listener(|this, v: i32, _| this.slider_val = v);
            let on_tab = cx.listener(|this, i: usize, _| this.tab_idx = i);
            let on_radio = cx.listener(|this, i: usize, _| this.radio_idx = i);
            let on_sel_toggle = cx.action(|this, _| this.select_open = !this.select_open);
            let on_sel = cx.listener(|this, i: usize, _| this.select_idx = Some(i));
            let on_close_dialog = cx.action(|this, _| this.dialog_open = false);
            let on_close_modal = cx.action(|this, _| this.modal_open = false);

            let opts = cx.arena().alloc_iter([
                SelectOption::new("Alpha", "a"),
                SelectOption::new("Beta (Disabled)", "b").disabled(true),
                SelectOption::new("Gamma", "c"),
            ]);

            let mut root = div()
                .flex_col()
                .size_full()
                .gap(8)
                .child(
                    cx,
                    Slider::new(100, self.slider_val)
                        .range(0, 100)
                        .step(10)
                        .w(160)
                        .on_change(on_slide),
                )
                .child(
                    cx,
                    Tabs::new(101, &["Overview", "Metrics", "Logs"], self.tab_idx)
                        .underline()
                        .on_select(on_tab),
                )
                .child(
                    cx,
                    RadioGroup::new(102, &["Fast", "Balanced", "Safe"], Some(self.radio_idx))
                        .horizontal()
                        .on_change(on_radio),
                )
                .child(
                    cx,
                    Select::new(103, opts, self.select_idx, self.select_open)
                        .on_toggle(on_sel_toggle)
                        .on_select(on_sel),
                );

            if self.dialog_open {
                root = root.child(
                    cx,
                    Dialog::new(104, "Preferences", true).on_close(on_close_dialog),
                );
            }
            if self.modal_open {
                root = root.child(
                    cx,
                    Modal::new(105, "Confirm Action")
                        .animated(false)
                        .on_close(on_close_modal),
                );
            }
            root.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(480, 360);
    let root = engine.mount_root(NavApp {
        slider_val: 50,
        tab_idx: 0,
        radio_idx: 0,
        select_open: false,
        select_idx: Some(0),
        dialog_open: false,
        modal_open: false,
    });
    engine.step_frame();

    // 1. Tab 聚焦到 Slider（focus_index = 0），验证 Right/Left/Home/End 按 step=10 调节
    assert!(engine.dispatch_key(KeyCode::Tab));
    assert!(engine.dispatch_key(KeyCode::Right));
    assert_eq!(engine.view(root).unwrap().slider_val, 60);
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::End));
    assert_eq!(engine.view(root).unwrap().slider_val, 100);
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Home));
    assert_eq!(engine.view(root).unwrap().slider_val, 0);
    engine.step_frame();

    // 2. Tab 必须正常切离 Slider 到 Tabs（绝不被 Slider 吞掉！），验证方向键切换页签
    assert!(engine.dispatch_key(KeyCode::Tab));
    assert!(engine.dispatch_key(KeyCode::Right));
    assert_eq!(engine.view(root).unwrap().tab_idx, 1);
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::End));
    assert_eq!(engine.view(root).unwrap().tab_idx, 2);
    engine.step_frame();

    // 3. Tab 连续跳过 Tabs 剩余 2 项到达 RadioGroup 第一项，验证方向键循环切换
    assert!(engine.dispatch_key(KeyCode::Tab));
    assert!(engine.dispatch_key(KeyCode::Tab));
    assert!(engine.dispatch_key(KeyCode::Tab));
    assert!(engine.dispatch_key(KeyCode::Right));
    assert_eq!(engine.view(root).unwrap().radio_idx, 1);
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Left));
    assert_eq!(engine.view(root).unwrap().radio_idx, 0);

    // 4. 先按 Escape 清除焦点，再展开 Select：验证未显式聚焦时按 Down 自动跳过禁用项 "Beta" 选中 "Gamma"(2)，再按 Escape 收起
    assert!(engine.dispatch_key(KeyCode::Escape));
    engine.view_mut(root).unwrap().select_open = true;
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Down));
    assert_eq!(engine.view(root).unwrap().select_idx, Some(2));
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Escape));
    assert!(!engine.view(root).unwrap().select_open);

    // 5. Dialog 与 Modal 打开时按 Escape 均自动关闭
    engine.view_mut(root).unwrap().dialog_open = true;
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Escape));
    assert!(!engine.view(root).unwrap().dialog_open);

    engine.view_mut(root).unwrap().modal_open = true;
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Escape));
    assert!(!engine.view(root).unwrap().modal_open);

    // 6. AlertDialog 在 velox-base 中默认无主题红背景，但完整保留 Escape 取消交互
    struct DestructiveAlertApp {
        cancelled: bool,
    }
    impl View for DestructiveAlertApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_cancel = cx.action(|this, _| this.cancelled = true);
            AlertDialog::new(200, "Delete Project", "Irreversible action")
                .destructive(true)
                .on_cancel(on_cancel)
                .into_element(cx)
        }
    }
    let mut eng2 = VeloxEngine::new(400, 260);
    let r2 = eng2.mount_root(DestructiveAlertApp { cancelled: false });
    eng2.step_frame();
    let dark = CoreTheme::dark();
    assert!(!eng2
        .cached_draw_ops()
        .iter()
        .any(|op| matches!(op.payload, DrawPayload::Quad { bg, .. } if bg == dark.danger)));
    assert!(eng2.dispatch_key(KeyCode::Escape));
    assert!(eng2.view(r2).unwrap().cancelled);
}

#[test]
fn test_headless_base_controls_hit_passthrough_and_zero_default_focus_ring() {
    use velox_base::{Radio, TextInput};

    struct HeadlessApp {
        btn_clicks: u32,
        slider_val: i32,
        checked: bool,
        switched: bool,
        radio_sel: bool,
        input_focused: bool,
    }

    impl View for HeadlessApp {
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
                .child(cx, Button::new(501, "Action").w(100).h(32).on_click(on_btn))
                .child(
                    cx,
                    Slider::new(503, self.slider_val)
                        .range(0, 100)
                        .step(10)
                        .w(200)
                        .on_change(on_slide),
                )
                .child(
                    cx,
                    Checkbox::new(504, self.checked)
                        .label("Enable Sync")
                        .on_change(on_chk),
                )
                .child(
                    cx,
                    Switch::new(505, self.switched)
                        .label("Auto Save")
                        .on_change(on_sw),
                )
                .child(
                    cx,
                    Radio::new(506, "Option A", self.radio_sel).on_select(on_rad),
                )
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
    let root = engine.mount_root(HeadlessApp {
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

    // 1. 鼠标点击无样式 Button 触发回调且无边框
    let (bx, by) = hit_center(&engine, 501);
    engine.dispatch_pointer_down(bx, by);
    engine.dispatch_pointer_up(bx, by);
    engine.step_frame();
    assert_eq!(engine.view(root).unwrap().btn_clicks, 1);

    // 2. 直接拖动 Slider（Thumb 位于轨道中央）更新数值且绝不绘制外框
    let (tx, ty) = hit_center(&engine, 503);
    engine.dispatch_pointer_down(tx, ty);
    engine.dispatch_pointer_move(tx + 50, ty);
    engine.dispatch_pointer_up(tx + 50, ty);
    engine.step_frame();
    assert!(engine.view(root).unwrap().slider_val > 50);

    // 3. 点击 Checkbox 小框、Switch 轨道、Radio 圆圈均触发状态更新
    let cb_bounds = engine
        .hit_regions()
        .iter()
        .find(|h| h.id == Some(504))
        .expect("checkbox hit region")
        .bounds;
    engine.dispatch_pointer_down(cb_bounds.x + 8, cb_bounds.y + cb_bounds.h / 2);
    engine.dispatch_pointer_up(cb_bounds.x + 8, cb_bounds.y + cb_bounds.h / 2);
    engine.step_frame();
    assert!(engine.view(root).unwrap().checked);

    let sw_bounds = engine
        .hit_regions()
        .iter()
        .find(|h| h.id == Some(505))
        .expect("switch hit region")
        .bounds;
    engine.dispatch_pointer_down(sw_bounds.x + 18, sw_bounds.y + sw_bounds.h / 2);
    engine.dispatch_pointer_up(sw_bounds.x + 18, sw_bounds.y + sw_bounds.h / 2);
    engine.step_frame();
    assert!(engine.view(root).unwrap().switched);

    let (rx0, ry0) = hit_center(&engine, 506);
    engine.dispatch_pointer_down(rx0, ry0);
    engine.dispatch_pointer_up(rx0, ry0);
    engine.step_frame();
    assert!(engine.view(root).unwrap().radio_sel);

    // 4. 点击空白处清除焦点后，键盘 Tab 聚焦无样式 Button：默认 focus_ring == false，不强行画蓝框
    engine.dispatch_pointer_down(390, 350);
    engine.dispatch_pointer_up(390, 350);
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame();
    assert!(engine.is_focus_visible());
    assert!(
        !engine.cached_draw_ops().iter().any(|op| {
            op.id == Some(501)
                && matches!(op.payload, DrawPayload::Quad { border_width, .. } if (border_width & 0x7F) > 0)
        }),
        "Headless velox_base::Button must not force a focus ring unless opted in"
    );

    // 5. Tab 聚焦 Slider 可正常使用键盘方向键调整数值
    let before_val = engine.view(root).unwrap().slider_val;
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Right));
    assert_eq!(
        engine.view(root).unwrap().slider_val,
        (before_val + 10).min(100)
    );
}

#[test]
fn test_dynamic_cursor_shapes_across_widgets_and_actions() {
    use velox_base::{Dialog, ResizableSplit, SplitAxis, TextInput};

    struct CursorDemoApp {
        slider_val: i32,
        split_w: i16,
        dialog_open: bool,
    }

    impl View for CursorDemoApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_click = cx.listener(|_, _: ClickEvent, _| {});
            let on_slide = cx.listener(|this, v: i32, _| this.slider_val = v);
            let on_resize = cx.listener(|this, w: i16, _| this.split_w = w);
            let on_key = cx.listener(|_, _: KeyCode, _| {});

            let mut root = div()
                .size_full()
                .flex_col()
                .p(16)
                .gap(10)
                .bg(cx.theme().background)
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(10)
                        .child(cx, Button::new(601, "Action").w(90).h(32).on_click(on_click))
                        .child(cx, Button::new(602, "Disabled").w(90).h(32).disabled(true).on_click(on_click))
                        .child(cx, Button::new(603, "Loading").w(90).h(32).loading(true).on_click(on_click)),
                )
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(10)
                        .child(cx, TextInput::new(604, "edit", false).size(140, 32).on_key_input(on_key))
                        .child(cx, TextInput::new(605, "locked", false).size(120, 32).disabled(true)),
                )
                .child(
                    cx,
                    Slider::new(606, self.slider_val)
                        .range(0, 100)
                        .w(180)
                        .on_change(on_slide),
                )
                .child(
                    cx,
                    ResizableSplit::new(607, SplitAxis::Horizontal, 240, 48, self.split_w)
                        .first(cx, div().size_full().bg(cx.theme().card))
                        .second(cx, div().size_full().bg(cx.theme().card))
                        .on_resize(on_resize),
                );

            if self.dialog_open {
                root = root.child(
                    cx,
                    div()
                        .pos(0, 0)
                        .size_full()
                        .overlay()
                        .occlude_hit()
                        .bg(Color::rgba(0, 0, 0, 140))
                        .items_center()
                        .justify_center()
                        .child(
                            cx,
                            Dialog::new(608, "Modal", true)
                                .description("Blocks cursor underneath"),
                        ),
                );
            }
            root.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(420, 260);
    let root = engine.mount_root(CursorDemoApp {
        slider_val: 40,
        split_w: 110,
        dialog_open: false,
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

    // 1. 空白背景 -> Default
    engine.dispatch_pointer_move(400, 240);
    assert_eq!(engine.cursor_shape(), CursorShape::Default);

    // 2. 按钮正常 -> Pointer；禁用 -> NotAllowed；加载中 -> Progress
    let (x1, y1) = hit_center(&engine, 601);
    engine.dispatch_pointer_move(x1, y1);
    assert_eq!(engine.cursor_shape(), CursorShape::Pointer);

    let (x2, y2) = hit_center(&engine, 602);
    engine.dispatch_pointer_move(x2, y2);
    assert_eq!(engine.cursor_shape(), CursorShape::NotAllowed);

    let (x3, y3) = hit_center(&engine, 603);
    engine.dispatch_pointer_move(x3, y3);
    assert_eq!(engine.cursor_shape(), CursorShape::Progress);

    // 3. 输入框正常 -> Text；禁用 -> NotAllowed
    let (x4, y4) = hit_center(&engine, 604);
    engine.dispatch_pointer_move(x4, y4);
    assert_eq!(engine.cursor_shape(), CursorShape::Text);

    let (x5, y5) = hit_center(&engine, 605);
    engine.dispatch_pointer_move(x5, y5);
    assert_eq!(engine.cursor_shape(), CursorShape::NotAllowed);

    // 4. Slider 悬停与按住拖拽（哪怕拖离滑块纵向边界）均显示标准小手 Pointer
    let (sx, sy) = hit_center(&engine, 606);
    engine.dispatch_pointer_move(sx, sy);
    assert_eq!(engine.cursor_shape(), CursorShape::Pointer);

    engine.dispatch_pointer_down(sx, sy);
    assert_eq!(engine.cursor_shape(), CursorShape::Pointer);

    engine.dispatch_pointer_move(sx + 40, sy + 60);
    assert_eq!(
        engine.cursor_shape(),
        CursorShape::Pointer,
        "Dragging Slider should keep Pointer cursor even if pointer drifts vertically"
    );

    engine.dispatch_pointer_up(sx + 40, sy);
    assert_eq!(engine.cursor_shape(), CursorShape::Pointer);

    // 5. ResizableSplit 水平分隔条 -> ResizeEW，拖拽期间保持 ResizeEW
    let (rx, ry) = hit_center(&engine, 607 ^ 0x5350_4C54);
    engine.dispatch_pointer_move(rx, ry);
    assert_eq!(engine.cursor_shape(), CursorShape::ResizeEW);
    engine.dispatch_pointer_down(rx, ry);
    engine.dispatch_pointer_move(rx + 15, ry + 40);
    assert_eq!(engine.cursor_shape(), CursorShape::ResizeEW);
    engine.dispatch_pointer_up(rx + 15, ry);

    // 6. 打开 Dialog 遮罩覆盖在按钮 601 上方：光标必须尊重 occlude 返回 Default，绝不透穿底层 Pointer
    engine.view_mut(root).unwrap().dialog_open = true;
    engine.step_frame();
    engine.dispatch_pointer_move(x1, y1);
    assert_eq!(engine.cursor_shape(), CursorShape::Default);

    // 7. 离开窗口 -> Default
    engine.dispatch_pointer_leave();
    assert_eq!(engine.cursor_shape(), CursorShape::Default);
}

#[test]
fn test_dropdown_select_and_color_picker_popups_align_with_triggers() {
    use velox_base::components::color_picker::{ColorPicker, ColorPickerState};
    use velox_base::{Dropdown, Select, SelectOption};

    const SEL_OPTS: &[SelectOption<'static>] = &[
        SelectOption::new("Velox Core", "velox"),
        SelectOption::new("Velox Base", "base"),
    ];
    const DD_OPTS: &[&str] = &[
        "wgpu 2D SDF (GPU)",
        "Built-in Software SDF",
        "Pure Wayland SHM",
    ];
    const PALETTE: &[Color] = &[Color::rgb(59, 130, 246), Color::rgb(16, 185, 129)];

    struct PopupAlignApp {
        dd_open: bool,
        sel_open: bool,
        cp_open: bool,
    }

    impl View for PopupAlignApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut cp_state = ColorPickerState::from_color(PALETTE[0]);
            cp_state.open = self.cp_open;
            let mut cp = ColorPicker::new(930, cp_state);
            cp.palette = PALETTE;
            div()
                .id(900)
                .flex_col()
                .size_full()
                .p(24)
                .gap(16)
                .scrollable(300)
                .overflow_hidden()
                .child(cx, div().w(100).h(40).bg(Color::rgb(24, 24, 27)))
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(20)
                        .child(
                            cx,
                            Select::new(910, SEL_OPTS, Some(0), self.sel_open).width(200),
                        )
                        .child(
                            cx,
                            Dropdown::new(920, DD_OPTS, 0, self.dd_open)
                                .size(220, 32)
                                .animated(false),
                        )
                        .child(cx, cp),
                )
                .child(cx, div().w(100).h(360).bg(Color::rgb(24, 24, 27)))
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(720, 320);
    engine.mount_root(PopupAlignApp {
        dd_open: true,
        sel_open: true,
        cp_open: true,
    });
    engine.step_frame();

    let trigger_bounds = |eng: &VeloxEngine, id: u32| -> Region {
        eng.hit_regions()
            .iter()
            .find(|h| h.id == Some(id))
            .expect("expected trigger hit region id")
            .bounds
    };
    let find_popup_by_w = |eng: &VeloxEngine, expected_w: i16| -> Region {
        eng.hit_regions()
            .iter()
            .find(|h| h.occlude && h.bounds.w == expected_w)
            .expect("expected occluding popup hit region")
            .bounds
    };

    let sel_trigger = trigger_bounds(&engine, 910);
    let sel_popup = find_popup_by_w(&engine, 200);
    assert_eq!(sel_popup.x, sel_trigger.x, "Select popup must left-align with trigger");
    assert_eq!(sel_popup.w, sel_trigger.w);
    assert_eq!(sel_popup.y, sel_trigger.y + 36);

    let dd_trigger = trigger_bounds(&engine, 920);
    let dd_popup = find_popup_by_w(&engine, 220);
    assert_eq!(
        dd_popup.x, dd_trigger.x,
        "Dropdown popup must left-align with trigger (no +10px header padding shift)"
    );
    assert_eq!(dd_popup.w, dd_trigger.w);
    assert_eq!(dd_popup.y, dd_trigger.y + 36);

    let cp_trigger = trigger_bounds(&engine, 930);
    let cp_popup = find_popup_by_w(&engine, 172);
    assert_eq!(
        cp_popup.x, cp_trigger.x,
        "ColorPicker popup must left-align with trigger (no +8px trigger padding shift)"
    );
    assert_eq!(cp_popup.y, cp_trigger.y + 34);

    // 滚动页面后，所有弹出浮层必须与各自触发器保持完全同步对齐
    engine.scroll(900, 36);
    engine.step_frame();
    let dd_trigger_s = trigger_bounds(&engine, 920);
    let dd_popup_s = find_popup_by_w(&engine, 220);
    assert_eq!(dd_trigger_s.y, dd_trigger.y - 36);
    assert_eq!(dd_popup_s.x, dd_trigger_s.x);
    assert_eq!(dd_popup_s.y, dd_trigger_s.y + 36);
}

#[test]
fn test_number_input_inset_buttons_preserve_outer_border_and_bounds_cursors() {
    use velox_base::NumberInput;

    struct NumApp {
        val: i64,
    }

    impl View for NumApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_change = cx.listener(|this, v: i64, _| this.val = v);
            let mut ni = NumberInput::new(950, self.val)
                .range(0, 10)
                .step(5)
                .suffix(" px")
                .width(150)
                .on_change(on_change);
            ni.border_color = Color::rgb(63, 63, 70);
            ni.button_bg = Color::rgb(39, 39, 42);
            div()
                .size_full()
                .p(20)
                .child(cx, ni)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 100);
    let root = engine.mount_root(NumApp { val: 0 });
    engine.step_frame();

    let outer = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(950))
        .expect("NumberInput outer box must exist")
        .bounds;
    assert_eq!(outer.w, 150);
    assert_eq!(outer.h, 32);

    // 两个步进按钮（26×26）必须严格内嵌在外框 3px 内边距之内，绝不覆盖外框四周边框
    let mut step_btns: Vec<Region> = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| op.id != Some(950) && op.bounds.w == 26 && op.bounds.h == 26)
        .map(|op| op.bounds)
        .collect();
    step_btns.sort_by_key(|r| r.x);
    assert_eq!(step_btns.len(), 2, "expected dec and inc buttons");
    let dec = step_btns[0];
    let inc = step_btns[1];
    assert_eq!(dec.x, outer.x + 3);
    assert_eq!(dec.y, outer.y + 3);
    assert_eq!(inc.right(), outer.right() - 3);
    assert_eq!(inc.y, outer.y + 3);

    // val == min(0) 时：减号按钮显示 NotAllowed，加号按钮显示 Pointer 且点击递增为 5
    engine.dispatch_pointer_move(dec.x + 13, dec.y + 13);
    assert_eq!(engine.cursor_shape(), CursorShape::NotAllowed);
    engine.dispatch_pointer_move(inc.x + 13, inc.y + 13);
    assert_eq!(engine.cursor_shape(), CursorShape::Pointer);
    assert!(engine.dispatch_click(inc.x + 13, inc.y + 13));
    assert_eq!(engine.view(root).unwrap().val, 5);
}

#[test]
fn test_gpui_fluent_tailwind_hover_active_focus_transition_and_text_inheritance() {
    struct FluentApp {
        highlighted: bool,
        badge_opt: Option<&'static str>,
    }

    impl View for FluentApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_col()
                .p_3()
                .gap_2()
                .bg(rgb(0x09090b))
                .child(
                    cx,
                    div()
                        .id(8001)
                        .w(px(250.))
                        .h_8()
                        .px_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_1()
                        .border_color(rgb(0xd4d4d4))
                        .rounded_md()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(rgb(0x737373))
                        .transition_colors()
                        .duration_150()
                        .ease_out()
                        .when(self.highlighted, |s| s.bg(rgb(0x171717)))
                        .hover(|s| {
                            s.bg(rgb(0xf5f5f5))
                                .border_color(rgb(0x111111))
                                .text_color(rgb(0x000000))
                        })
                        .active(|s| s.bg(rgb(0xe5e5e5)))
                        .focus(|s| s.border_2().border_color(rgb(0x3b82f6)))
                        .child(cx, text("Inherited"))
                        .child(cx, text("Explicit").size(16).color(rgb(0x22c55e)))
                        .when_some(self.badge_opt, |s, lbl| s.child(cx, text(lbl))),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(360, 140);
    let _ = engine.mount_root(FluentApp {
        highlighted: true,
        badge_opt: Some("OPT"),
    });
    engine.step_frame();

    // 1. 验证 px(250.) / h_8() (32px) / border_1().border_color(0xd4d4d4) / when(highlighted) / rounded_md (6)
    let box_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(8001))
        .expect("box 8001 quad must exist");
    assert_eq!(box_op.bounds.w, 250);
    assert_eq!(box_op.bounds.h, 32);
    assert!(matches!(
        box_op.payload,
        DrawPayload::Quad {
            bg,
            border_color,
            rounded: 6,
            border_width,
            ..
        } if bg == rgb(0x171717) && border_color == rgb(0xd4d4d4) && (border_width & 0x7F) == 1
    ));

    // 2. 验证容器级 .text_xs().text_color(0x737373) 自动向下继承，且显式设置不被覆盖
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, font_size: 11, color, .. }
            if text.as_str() == "Inherited" && *color == rgb(0x737373)
    )));
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, font_size: 16, color, .. }
            if text.as_str() == "Explicit" && *color == rgb(0x22c55e)
    )));
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, font_size: 11, color, .. }
            if text.as_str() == "OPT" && *color == rgb(0x737373)
    )));

    // 3. 验证 .hover(|s| s.bg().border_color().text_color()) 完整生效（推进过渡至完成）
    let bx = box_op.bounds.x + 20;
    let by = box_op.bounds.y + 16;
    engine.dispatch_pointer_move(bx, by);
    engine.step_frame_dt(16);
    engine.step_frame_dt(200);
    let hover_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(8001))
        .unwrap();
    assert!(matches!(
        hover_op.payload,
        DrawPayload::Quad { bg, border_color, .. }
            if bg == rgb(0xf5f5f5) && border_color == rgb(0x111111)
    ));
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, color, .. }
            if text.as_str() == "Inherited" && *color == rgb(0x000000)
    )));

    // 4. 验证 .active(|s| s.bg(0xe5e5e5)) 生效
    engine.dispatch_pointer_down(bx, by);
    engine.step_frame_dt(16);
    engine.step_frame_dt(200);
    let active_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(8001))
        .unwrap();
    assert!(matches!(
        active_op.payload,
        DrawPayload::Quad { bg, .. } if bg == rgb(0xe5e5e5)
    ));
    engine.dispatch_pointer_up(bx, by);
    engine.dispatch_pointer_leave();
    engine.step_frame_dt(200);

    // 5. 验证 .focus(|s| s.border_2().border_color(0x3b82f6)) 在 Tab 聚焦后生效
    assert!(engine.dispatch_key(KeyCode::Tab));
    engine.step_frame_dt(16);
    engine.step_frame_dt(200);
    let focus_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(8001))
        .unwrap();
    assert!(matches!(
        focus_op.payload,
        DrawPayload::Quad { border_width, border_color, .. }
            if (border_width & 0x7F) == 2 && border_color == rgb(0x3b82f6)
    ));
}

#[test]
fn test_gpui_base_calendar_slot_callbacks_and_views() {
    use velox_base::{
        Calendar, CalendarItemKind, CalendarState, CalendarViewMode, DateSelection, SimpleDate,
    };

    struct CalSlotApp {
        cal: CalendarState,
    }

    impl View for CalSlotApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_prev = cx.action(|this, _| this.cal.prev());
            let on_next = cx.action(|this, _| this.cal.next());
            let on_mtog = cx.action(|this, _| this.cal.toggle_months());
            let on_ytog = cx.action(|this, _| this.cal.toggle_years());
            let on_day = cx.listener(|this, d: SimpleDate, _| this.cal.select_day(d));
            let on_month = cx.listener(|this, m: u8, _| this.cal.select_month(m));
            let on_year = cx.listener(|this, y: i32, _| this.cal.select_year(y));

            let calendar = Calendar::from_state(9001, &self.cal)
                .w(px(250.))
                .p_3()
                .border_1()
                .border_color(rgb(0xd4d4d4))
                .on_prev_month(on_prev)
                .on_next_month(on_next)
                .on_toggle_month_view(on_mtog)
                .on_toggle_year_view(on_ytog)
                .on_select_day(on_day)
                .on_select_month(on_month)
                .on_select_year(on_year)
                .item(cx, |item, state| match state.kind() {
                    CalendarItemKind::Previous | CalendarItemKind::Next => item
                        .size_7()
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(rgb(0xf5f5f5))),
                    CalendarItemKind::MonthToggle | CalendarItemKind::YearToggle => item
                        .px_1()
                        .h_7()
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(rgb(0xf5f5f5))),
                    CalendarItemKind::Weekday => item
                        .size_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(0x737373)),
                    CalendarItemKind::Day => item
                        .size_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(state.is_muted(), |s| s.text_color(rgb(0xa3a3a3)))
                        .when(state.is_today() && !state.is_active(), |s| {
                            s.border_1().border_color(rgb(0xd4d4d4))
                        })
                        .when(state.is_active(), |s| {
                            s.bg(rgb(0x171717)).text_color(rgb(0xffffff))
                        })
                        .when(!state.is_disabled() && !state.is_active(), |s| {
                            s.hover(|h| h.bg(rgb(0xf5f5f5)))
                        }),
                    CalendarItemKind::Month | CalendarItemKind::Year => item
                        .w(px(74.))
                        .h_7()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(state.is_active(), |s| {
                            s.bg(rgb(0x171717)).text_color(rgb(0xffffff))
                        })
                        .when(!state.is_active(), |s| s.hover(|h| h.bg(rgb(0xf5f5f5)))),
                })
                .label(cx, |kind, value| match kind {
                    CalendarItemKind::Previous => StrRef::from("‹"),
                    CalendarItemKind::Next => StrRef::from("›"),
                    CalendarItemKind::MonthToggle | CalendarItemKind::Month => StrRef::from(
                        [
                            "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep",
                            "Oct", "Nov", "Dec",
                        ][(value as usize).clamp(1, 12)],
                    ),
                    CalendarItemKind::Weekday => {
                        StrRef::from(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][(value as usize) % 7])
                    }
                    _ => velox_engine::fmt!(cx, "{}", value),
                });

            div().size_full().p_2().child(cx, calendar).into_element(cx)
        }
    }

    // 在 4992B 极限 no_std 竞技场内验证 Months / Years 视图切换、外层 Styled 与 .item() / .label() 插槽回调
    let mut engine = VeloxEngine::new(320, 320);
    let mut initial_cal = CalendarState::new(2026, 11)
        .with_today(SimpleDate::new(2026, 11, 23))
        .with_selection(DateSelection::Single(SimpleDate::new(2026, 11, 18)));
    initial_cal.view_mode = CalendarViewMode::Months;
    let root = engine.mount_root(CalSlotApp { cal: initial_cal });
    engine.step_frame();

    let cal_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(9001))
        .expect("Calendar outer box must exist");
    assert_eq!(cal_op.bounds.w, 250);
    assert!(matches!(
        cal_op.payload,
        DrawPayload::Quad { border_color, border_width, .. }
            if border_color == rgb(0xd4d4d4) && (border_width & 0x7F) == 1
    ));
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, .. } if text.as_str() == "‹"
    )));
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        op.payload,
        DrawPayload::Quad { bg, .. } if bg == rgb(0x171717)
    )));

    let find_text_center = |eng: &VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle))
            .expect("expected text run");
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    // 点击年份标题 "2026" 切到 Years 视图，再点击 "2025" 选中 2025 年并回到 Days 视图
    let (yx, yy) = find_text_center(&engine, "2026");
    assert!(engine.dispatch_click(yx, yy));
    assert_eq!(engine.view(root).unwrap().cal.view_mode, CalendarViewMode::Years);
    engine.step_frame();

    let (y25x, y25y) = find_text_center(&engine, "2025");
    assert!(engine.dispatch_click(y25x, y25y));
    assert_eq!(engine.view(root).unwrap().cal.view_year, 2025);
    assert_eq!(engine.view(root).unwrap().cal.view_mode, CalendarViewMode::Days);

    // 切回 Months 视图并点击 "Jan" 选中 1 月
    engine.view_mut(root).unwrap().cal.view_mode = CalendarViewMode::Months;
    engine.step_frame();
    let (jx, jy) = find_text_center(&engine, "Jan");
    assert!(engine.dispatch_click(jx, jy));
    assert_eq!(engine.view(root).unwrap().cal.view_month, 1);
    assert_eq!(engine.view(root).unwrap().cal.view_mode, CalendarViewMode::Days);
}

#[test]
fn test_gpui_base_composite_slot_callbacks() {
    use velox_base::{
        Accordion, AccordionItem, List, ListItem, Pagination, PaginationItemKind, RadioGroup,
        Select, SelectOption, ToggleGroup,
    };

    const SEL_OPTS: &[SelectOption<'static>] = &[
        SelectOption::new("Alpha", "a"),
        SelectOption::new("Beta", "b"),
    ];
    const ACC_ITEMS: &[AccordionItem<'static>] = &[
        AccordionItem::new("FAQ 1", "Body 1"),
        AccordionItem::new("FAQ 2", "Body 2"),
    ];
    const LIST_ITEMS: &[ListItem<'static>] = &[
        ListItem::new("Row A", "", ""),
        ListItem::new("Row B", "", ""),
    ];

    struct CompositeSlotApp {
        tab: usize,
        sel_idx: Option<usize>,
        radio_idx: usize,
        acc_mask: u32,
        toggle_mask: u32,
        list_idx: usize,
        page: usize,
    }

    impl View for CompositeSlotApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_tab = cx.listener(|this, i: usize, _| this.tab = i);
            let on_sel = cx.listener(|this, i: usize, _| this.sel_idx = Some(i));
            let on_rad = cx.listener(|this, i: usize, _| this.radio_idx = i);
            let on_acc_in = cx.listener(|this, (i, open): (usize, bool), _| {
                if open {
                    this.acc_mask |= 1 << i;
                } else {
                    this.acc_mask &= !(1 << i);
                }
            });
            let on_acc = cx.arena().alloc(move |i, o, acx: &mut ActionScope| on_acc_in((i, o), acx));
            let on_tog_in = cx.listener(|this, (i, on): (usize, bool), _| {
                if on {
                    this.toggle_mask |= 1 << i;
                } else {
                    this.toggle_mask &= !(1 << i);
                }
            });
            let on_tog = cx.arena().alloc(move |i, o, acx: &mut ActionScope| on_tog_in((i, o), acx));
            let on_list = cx.listener(|this, i: usize, _| this.list_idx = i);
            let on_page = cx.listener(|this, p: usize, _| this.page = p);

            let tabs = Tabs::new(9010, &["One", "Two"], self.tab)
                .w(px(160.))
                .h_7()
                .on_select(on_tab)
                .item(cx, |item, st| {
                    item.flex_1()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(st.is_active(), |s| s.bg(rgb(0x171717)).text_color(rgb(0xffffff)))
                });

            let radio = RadioGroup::new(9020, &["R1", "R2"], Some(self.radio_idx))
                .horizontal()
                .on_change(on_rad)
                .item(cx, |item, st| {
                    item.px_2()
                        .h_7()
                        .flex()
                        .items_center()
                        .when(st.is_selected(), |s| s.border_1().border_color(rgb(0x171717)))
                });

            let toggle = ToggleGroup::new(9030, &["B", "I"], self.toggle_mask)
                .on_toggle(on_tog)
                .item(cx, |item, st| {
                    item.size_7()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(st.is_active(), |s| s.bg(rgb(0x171717)).text_color(rgb(0xffffff)))
                });

            let list = List::new(9040, LIST_ITEMS)
                .w(px(140.))
                .selected(Some(self.list_idx))
                .on_select(on_list)
                .item(cx, |row, st| {
                    row.h_7()
                        .px_2()
                        .flex()
                        .items_center()
                        .when(st.is_selected(), |s| s.bg(rgb(0x27272a)))
                });

            let pagination = Pagination::new(9050, self.page, 3)
                .on_change(on_page)
                .item(cx, |btn, st| {
                    btn.size_7()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(st.is_active(), |s| s.bg(rgb(0x171717)).text_color(rgb(0xffffff)))
                })
                .label(cx, |kind, p| match kind {
                    PaginationItemKind::Previous => StrRef::from("Prev"),
                    PaginationItemKind::Next => StrRef::from("Next"),
                    PaginationItemKind::Page => velox_engine::fmt!(cx, "{}", p),
                });

            let acc = Accordion::new(9060, ACC_ITEMS, self.acc_mask)
                .w(px(140.))
                .on_toggle_item(on_acc)
                .item(cx, |hdr, st| {
                    hdr.h_7()
                        .px_2()
                        .flex()
                        .items_center()
                        .when(st.is_open(), |s| s.bg(rgb(0x27272a)))
                });

            let sel = Select::new(9070, SEL_OPTS, self.sel_idx, false)
                .w(px(140.))
                .on_select(on_sel)
                .item(cx, |row, st| {
                    row.h_7()
                        .px_2()
                        .flex()
                        .items_center()
                        .when(st.is_selected(), |s| s.bg(rgb(0x171717)))
                });

            div()
                .size_full()
                .flex_col()
                .p_2()
                .gap_2()
                .child(cx, tabs)
                .child(cx, radio)
                .child(cx, toggle)
                .child(cx, list)
                .child(cx, pagination)
                .child(cx, acc)
                .child(cx, sel)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(360, 420);
    let root = engine.mount_root(CompositeSlotApp {
        tab: 0,
        sel_idx: Some(0),
        radio_idx: 0,
        acc_mask: 0b01,
        toggle_mask: 0b01,
        list_idx: 0,
        page: 2,
    });
    engine.step_frame();

    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, .. } if text.as_str() == "Prev"
    )));

    let find_text_center = |eng: &VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle))
            .expect("expected text run");
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    let (tx, ty) = find_text_center(&engine, "Two");
    assert!(engine.dispatch_click(tx, ty));
    assert_eq!(engine.view(root).unwrap().tab, 1);

    let (rx, ry) = find_text_center(&engine, "R2");
    assert!(engine.dispatch_click(rx, ry));
    assert_eq!(engine.view(root).unwrap().radio_idx, 1);

    let (gx, gy) = find_text_center(&engine, "I");
    assert!(engine.dispatch_click(gx, gy));
    assert_eq!(engine.view(root).unwrap().toggle_mask, 0b11);

    let (px_n, py_n) = find_text_center(&engine, "Next");
    assert!(engine.dispatch_click(px_n, py_n));
    assert_eq!(engine.view(root).unwrap().page, 3);

    let (lx, ly) = find_text_center(&engine, "Row B");
    assert!(engine.dispatch_click(lx, ly));
    assert_eq!(engine.view(root).unwrap().list_idx, 1);

    let (ax, ay) = find_text_center(&engine, "FAQ 2");
    assert!(engine.dispatch_click(ax, ay));
    assert_eq!(engine.view(root).unwrap().acc_mask, 0b11);
}

#[test]
fn test_all_velox_base_widgets_implement_gpui_styled_ext() {
    use velox_base::*;
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
}

#[test]
fn test_extended_input_slots_and_styled_ext_runtime() {
    use velox_base::*;
    struct InputSlotApp {
        checked: bool,
        switched: bool,
        slider_val: i32,
        rating: u8,
        collapsed_open: bool,
        clicks: u32,
    }

    impl View for InputSlotApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_btn = cx.listener(|this, _: ClickEvent, _| this.clicks += 1);
            let on_cb = cx.listener(|this, v: bool, _| this.checked = v);
            let on_sw = cx.listener(|this, v: bool, _| this.switched = v);
            let on_sl = cx.listener(|this, v: i32, _| this.slider_val = v);
            let on_rt = cx.listener(|this, v: u8, _| this.rating = v);
            let on_col = cx.listener(|this, v: bool, _| this.collapsed_open = v);

            let btn = Button::new(9101, "")
                .h_8()
                .px_3()
                .bg(rgb(0x171717))
                .text_color(rgb(0xffffff))
                .rounded_md()
                .transition_colors()
                .duration_150()
                .ease_out()
                .overflow_hidden()
                .hover(|s| s.bg(rgb(0x262626)))
                .active(|s| s.opacity_f(0.85))
                .child(cx, text("CustomBtn").text_xs())
                .on_click(on_btn);

            let cb = Checkbox::new(9102, self.checked)
                .label("SlotCB")
                .on_change(on_cb)
                .indicator(cx, |box_el, st| {
                    box_el
                        .size_4()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(0x171717))
                        .when(st.is_checked(), |s| s.bg(rgb(0x171717)))
                });

            let sw = Switch::new(9103, self.switched)
                .label("SlotSW")
                .on_change(on_sw)
                .track(cx, |tr, st| {
                    tr.w(px(36.))
                        .h(px(20.))
                        .rounded_full()
                        .p_0_5()
                        .when(st.is_checked(), |s| s.bg(rgb(0x10b981)))
                        .when(!st.is_checked(), |s| s.bg(rgb(0xd4d4d4)))
                })
                .thumb(cx, |th, _| th.size_4().rounded_full().bg(rgb(0xffffff)));

            let sl = Slider::new(9104, self.slider_val)
                .range(0, 100)
                .w(px(160.))
                .on_change(on_sl)
                .track(cx, |tr, _| tr.h(px(6.)).rounded_full().bg(rgb(0xe5e5e5)))
                .indicator(cx, |ind, _| ind.rounded_full().bg(rgb(0x171717)))
                .thumb(cx, |th, _| th.size_4().rounded_full().bg(rgb(0x171717)));

            let rt = Rating::new(9105, self.rating)
                .max(5)
                .on_change(on_rt)
                .item(cx, |el, st| {
                    el.size_6()
                        .items_center()
                        .justify_center()
                        .when(st.is_filled(), |s| s.bg(rgb(0xf59e0b)))
                });

            let col = Collapsible::new(9106, "MoreDetails", self.collapsed_open)
                .on_toggle(on_col)
                .trigger(cx, |hdr, st| {
                    hdr.h_7()
                        .px_2()
                        .when(st.is_open(), |s| s.bg(rgb(0x27272a)))
                })
                .content(cx, text("InnerPanel"));

            let inp = TextInput::new(9107, "abc", false)
                .w(px(180.))
                .h_8()
                .rounded_md()
                .border_1()
                .border_color(rgb(0xd4d4d4))
                .prefix(cx, text("@"))
                .suffix(cx, text("OK"));

            let pb = ProgressBar::new(9108, 120, 60)
                .w(px(160.))
                .rounded_full()
                .overflow_hidden();

            div()
                .size_full()
                .flex_col()
                .p_2()
                .gap_2()
                .child(cx, btn)
                .child(cx, cb)
                .child(cx, sw)
                .child(cx, sl)
                .child(cx, rt)
                .child(cx, col)
                .child(cx, inp)
                .child(cx, pb)
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(360, 420);
    let root = engine.mount_root(InputSlotApp {
        checked: false,
        switched: false,
        slider_val: 40,
        rating: 2,
        collapsed_open: false,
        clicks: 0,
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

    // 验证 Button::child 与 TextInput::prefix/suffix 文本均已渲染
    let (bx, by) = find_text_center(&engine, "CustomBtn");
    assert!(engine.dispatch_click(bx, by));
    assert_eq!(engine.view(root).unwrap().clicks, 1);

    let (cx_pos, cy_pos) = find_text_center(&engine, "SlotCB");
    assert!(engine.dispatch_click(cx_pos, cy_pos));
    assert!(engine.view(root).unwrap().checked);

    let (sx, sy) = find_text_center(&engine, "SlotSW");
    assert!(engine.dispatch_click(sx, sy));
    assert!(engine.view(root).unwrap().switched);

    let (hx, hy) = find_text_center(&engine, "MoreDetails");
    assert!(engine.dispatch_click(hx, hy));
    assert!(engine.view(root).unwrap().collapsed_open);

    // 验证 prefix/suffix 存在
    let _ = find_text_center(&engine, "@");
    let _ = find_text_center(&engine, "OK");
}

#[test]
fn test_nav_table_overlay_slots_and_display_styled_ext() {
    use velox_base::*;
    static MENU_ITEMS: &[MenuItem] = &[MenuItem::new("Open", "Ctrl+O"), MenuItem::new("Quit", "")];
    static COLS: &[TableColumn] = &[TableColumn::new("Name", 80), TableColumn::new("Role", 80)];
    static ROWS: &[&[&str]] = &[&["Alice", "Admin"]];
    static STEPS: &[StepItem] = &[
        StepItem::new("Init", "", StepStatus::Completed),
        StepItem::new("Run", "", StepStatus::Active),
    ];

    struct NavOverlayApp {
        menu_picked: Option<usize>,
        step_picked: Option<usize>,
        dialog_open: bool,
    }

    impl View for NavOverlayApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_menu = cx.listener(|this, idx: usize, _| this.menu_picked = Some(idx));
            let on_step = cx.listener(|this, idx: usize, _| this.step_picked = Some(idx));
            let on_close = cx.action(|this, _| this.dialog_open = false);

            let menu = Menu::new(9201, MENU_ITEMS)
                .w(px(160.))
                .rounded_md()
                .border_1()
                .border_color(rgb(0xd4d4d4))
                .on_select(on_menu)
                .item(cx, |row, st, _| {
                    row.h_7()
                        .px_2()
                        .when(!st.is_disabled(), |s| s.hover(|h| h.bg(rgb(0xf5f5f5))))
                });

            let tbl = Table::new(9202, COLS, ROWS)
                .rounded_md()
                .border_1()
                .border_color(rgb(0xd4d4d4))
                .header_cell(cx, |c, _, _| c.px_2().bg(rgb(0xf5f5f5)))
                .cell(cx, |c, _, _| c.px_2());

            let stepper = Stepper::new(9203, STEPS)
                .on_step_click(on_step)
                .item(cx, |el, st, _| {
                    el.px_2()
                        .h_7()
                        .rounded_sm()
                        .when(st.status() == StepStatus::Active, |s| {
                            s.bg(rgb(0x171717)).text_color(rgb(0xffffff))
                        })
                });

            let badge = Badge::new("NEW")
                .px_2()
                .h_6()
                .bg(rgb(0x171717))
                .text_color(rgb(0xffffff))
                .rounded_full()
                .transition_colors()
                .hover(|s| s.opacity_f(0.8));

            let card = Card::new()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(0xd4d4d4))
                .overflow_hidden()
                .shadow_sm()
                .child(cx, badge);

            let skel = Skeleton::new(9204, 80, 12).rounded_full().opacity_f(0.5);
            let spin = Spinner::new(9205, 24, 90).opacity_f(0.9);

            let tip = Tooltip::new(9206, "HintTip", true)
                .trigger(cx, text("Anchor"))
                .popup(cx, |p, _| p.px_2().py_1().bg(rgb(0x171717)).rounded_sm());

            let mut root = div()
                .size_full()
                .flex_col()
                .p_2()
                .gap_2()
                .child(cx, menu)
                .child(cx, tbl)
                .child(cx, stepper)
                .child(cx, card)
                .child(cx, div().flex_row().gap_2().child(cx, skel).child(cx, spin))
                .child(cx, tip);

            if self.dialog_open {
                let dlg = Dialog::new(9207, "Confirm", true)
                    .w(px(200.))
                    .bg(rgb(0xffffff))
                    .rounded_lg()
                    .shadow_md()
                    .on_close(on_close)
                    .backdrop(cx, |b, _| b.bg(Color::rgba(0, 0, 0, 120)));
                root = root.child(cx, dlg);
            }

            root.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(360, 460);
    let root = engine.mount_root(NavOverlayApp {
        menu_picked: None,
        step_picked: None,
        dialog_open: false,
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

    let (mx, my) = find_text_center(&engine, "Open");
    assert!(engine.dispatch_click(mx, my));
    assert_eq!(engine.view(root).unwrap().menu_picked, Some(0));

    let (sx, sy) = find_text_center(&engine, "Run");
    assert!(engine.dispatch_click(sx, sy));
    assert_eq!(engine.view(root).unwrap().step_picked, Some(1));

    let _ = find_text_center(&engine, "Alice");
    let _ = find_text_center(&engine, "NEW");
    let _ = find_text_center(&engine, "HintTip");

    // 打开带 backdrop 插槽的 Dialog 并点击遮罩外缘关闭（root 有 p_2 = 8px 内边距，(16, 16) 位于遮罩左上角）
    engine.view_mut(root).unwrap().dialog_open = true;
    engine.step_frame();
    let _ = find_text_center(&engine, "Confirm");
    assert!(engine.dispatch_click(16, 16));
    assert!(!engine.view(root).unwrap().dialog_open);
}

#[test]
fn test_base_widgets_zero_heap_signal_bind() {
    use velox_base::*;

    // 分三批紧凑挂载以兼容纯 `#![no_std]` 4992B 固定 ScratchArena
    struct BindGroupA {
        sw: Signal<bool>,
        cb: Signal<bool>,
        sl: Signal<i32>,
        tg: Signal<bool>,
    }

    impl View for BindGroupA {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_col()
                .p_2()
                .gap_1()
                .child(cx, Switch::bind(cx, 9501, self.sw).label("BindSW"))
                .child(cx, Checkbox::bind(cx, 9502, self.cb).label("BindCB"))
                .child(cx, Slider::bind(cx, 9503, self.sl).range(0, 100).w(120))
                .child(cx, Toggle::bind(cx, 9504, "BindTG", self.tg).h_7().px_2())
                .into_element(cx)
        }
    }

    struct BindGroupB {
        rt: Signal<u8>,
        num: Signal<i64>,
        tab: Signal<usize>,
        tbar: Signal<usize>,
    }

    impl View for BindGroupB {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_col()
                .p_2()
                .gap_1()
                .child(cx, Rating::bind(cx, 9505, self.rt).max(3))
                .child(cx, NumberInput::bind(cx, 9506, self.num).range(0, 100))
                .child(cx, Tabs::bind(cx, 9507, &["T0", "T1"], self.tab))
                .child(cx, TabBar::bind(cx, 9508, &["B0", "B1"], self.tbar).safe_bottom(false))
                .into_element(cx)
        }
    }

    struct BindGroupC {
        rad: Signal<usize>,
        page: Signal<usize>,
        col: Signal<bool>,
        rol: Signal<usize>,
    }

    impl View for BindGroupC {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_col()
                .p_2()
                .gap_1()
                .child(cx, RadioGroup::bind(cx, 9509, &["R0", "R1"], self.rad).horizontal())
                .child(cx, Pagination::bind(cx, 9510, 3, self.page))
                .child(cx, Collapsible::bind(cx, 9511, "BindCol", self.col))
                .child(cx, Roller::bind(cx, 9512, &["X", "Y", "Z"], self.rol))
                .into_element(cx)
        }
    }

    let find_text = |eng: &VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle))
            .unwrap_or_else(|| panic!("expected text run {needle:?}"));
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    let sw = signal(false);
    let cb = signal(false);
    let sl = signal(20i32);
    let tg = signal(false);
    let mut eng_a = VeloxEngine::new(320, 240);
    let _ra = eng_a.mount_root(BindGroupA { sw, cb, sl, tg });
    eng_a.step_frame();

    let (x, y) = find_text(&eng_a, "BindSW");
    assert!(eng_a.dispatch_click(x, y));
    assert!(sw.get_untracked());

    let (x, y) = find_text(&eng_a, "BindCB");
    assert!(eng_a.dispatch_click(x, y));
    assert!(cb.get_untracked());

    let (x, y) = find_text(&eng_a, "BindTG");
    assert!(eng_a.dispatch_click(x, y));
    assert!(tg.get_untracked());

    let rt = signal(2u8);
    let num = signal(10i64);
    let tab = signal(0usize);
    let tbar = signal(0usize);
    let mut eng_b = VeloxEngine::new(320, 240);
    let _rb = eng_b.mount_root(BindGroupB { rt, num, tab, tbar });
    eng_b.step_frame();

    let (x, y) = find_text(&eng_b, "T1");
    assert!(eng_b.dispatch_click(x, y));
    assert_eq!(tab.get_untracked(), 1);

    let rad = signal(0usize);
    let page = signal(1usize);
    let col = signal(false);
    let rol = signal(1usize);
    let mut eng_c = VeloxEngine::new(320, 240);
    let _rc = eng_c.mount_root(BindGroupC { rad, page, col, rol });
    eng_c.step_frame();

    let (x, y) = find_text(&eng_c, "R1");
    assert!(eng_c.dispatch_click(x, y));
    assert_eq!(rad.get_untracked(), 1);

    let (x, y) = find_text(&eng_c, "BindCol");
    assert!(eng_c.dispatch_click(x, y));
    assert!(col.get_untracked());

    for s in [sw, cb, tg, col] {
        s.dispose();
    }
    sl.dispose();
    rt.dispose();
    num.dispose();
    for s in [tab, tbar, rad, page, rol] {
        s.dispose();
    }
}
