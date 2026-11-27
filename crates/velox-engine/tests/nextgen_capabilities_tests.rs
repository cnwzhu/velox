//! P0 + P1 + P2 下一代能力全维度集成测试：
//! - 国际多脚本分段、8 槽位字体回退链、`InlineFlow` 富文本内联真实交互组件与 `ime_cursor_rect` 跟随；
//! - 完整矢量路径（`CubicBezier` + 20 指令 `SvgPath`）、BT.709 NV12/I420 YUV→RGBA 转换与 `WebViewHostSync`；
//! - AI-Native MCP JSON-RPC 2.0 语义控制协议与 `.vlxrec` 确定性时间旅行事件录像回放；
//! - 多窗口 `MultiWindowManager` Dock 面板拖出（Tear-Off）与共享 `Signal` 跨窗同步及吸附回流；
//! - 富格式剪贴板（Image / FilePaths / CustomMime）、原生菜单/托盘/文件对话框/拖拽幽灵预览；
//! - 交互式 Live DevTools（64 帧耗时瀑布图、元素点选拾取、零编译样式表热重载）。

#![cfg(feature = "std")]

use velox_engine::platform::winit::{MultiWindowManager, MultiWindowSpec};
use velox_engine::prelude::*;

#[test]
fn test_p0_international_scripts_fallback_inline_flow_and_ime_cursor_rect() {
    // 1. 多脚本分类与 Emoji ZWJ 聚类
    let runs = split_script_runs("Hello 世界 مرحبا 🚀👨‍👩‍👧‍👦 123");
    assert!(runs.as_slice().iter().any(|r| r.script == ScriptKind::Latin));
    assert!(runs.as_slice().iter().any(|r| r.script == ScriptKind::Cjk));
    assert!(runs.as_slice().iter().any(|r| r.script == ScriptKind::Arabic));
    assert!(runs.as_slice().iter().any(|r| r.script == ScriptKind::Emoji));

    // 2. 8 槽位字体回退链
    let chain = FontFallbackChain::new()
        .with_script(ScriptKind::Cjk, FontFamily::Custom(1))
        .with_script(ScriptKind::Emoji, FontFamily::Custom(2));
    assert_eq!(
        chain.resolve_for_char('中', FontSpec::SANS_BOLD).family,
        FontFamily::Custom(1)
    );
    assert_eq!(
        chain.resolve_for_char('🚀', FontSpec::SANS).family,
        FontFamily::Custom(2)
    );
    assert_eq!(
        chain.resolve_for_char('A', FontSpec::SANS).family,
        FontFamily::Sans
    );

    // 3. InlineFlow 富文本内联真实交互按钮 + IME 光标跟随
    struct InlineImeView {
        inline_clicks: u32,
        text_buf: String,
    }
    impl View for InlineImeView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_inline_click = cx.listener(|this, _: ClickEvent, _| {
                this.inline_clicks += 1;
            });
            let on_text = cx.listener(|this, ev: KeyCode, _| {
                if let KeyCode::Char(c) = ev {
                    this.text_buf.push(c);
                }
            });
            div()
                .w(420)
                .h(200)
                .flex_col()
                .gap(10)
                .p(12)
                .child(
                    cx,
                    inline_flow()
                        .w(380)
                        .text(cx, "AI 引用验证：点击内联按钮")
                        .inline_widget(
                            cx,
                            div()
                                .id(901)
                                .px(8)
                                .py(3)
                                .rounded(6)
                                .bg(Color::rgb(59, 130, 246))
                                .focusable()
                                .on_click(cx, on_inline_click)
                                .child(cx, text("Cite #1").size(11).color(Color::WHITE).nowrap()),
                        )
                        .text(cx, "即可触发真实回调"),
                )
                .child(
                    cx,
                    div()
                        .id(902)
                        .w(240)
                        .h(30)
                        .bg(Color::rgb(30, 41, 59))
                        .focusable()
                        .on_key_input(on_text)
                        .child(cx, text("Input").size(12).color(Color::WHITE)),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(420, 200);
    let root = engine.mount_root(InlineImeView {
        inline_clicks: 0,
        text_buf: String::new(),
    });
    let _ = engine.step_frame_dt(16);

    // 点击内联组件 #901（通过 AutomationSession 精准查询边界）
    let mut auto = AutomationSession::new();
    let btn_bounds = auto
        .query_nodes(
            &engine,
            &SemanticQuery {
                id: Some(901),
                ..Default::default()
            },
        )
        .first()
        .expect("inline button #901 should exist")
        .bounds;
    assert!(engine.dispatch_click(
        btn_bounds.x + btn_bounds.w / 2,
        btn_bounds.y + btn_bounds.h / 2
    ));
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.view(root).unwrap().inline_clicks, 1);

    // 聚焦输入框 #902 并验证 IME 光标矩形随预编辑推进
    let inp_bounds = auto
        .query_nodes(
            &engine,
            &SemanticQuery {
                id: Some(902),
                ..Default::default()
            },
        )
        .first()
        .expect("input #902 should exist")
        .bounds;
    let _ = engine.dispatch_click(
        inp_bounds.x + inp_bounds.w / 2,
        inp_bounds.y + inp_bounds.h / 2,
    );
    let _ = engine.step_frame_dt(16);
    let r0 = engine.ime_cursor_rect().expect("focused input should yield IME rect");
    assert!(engine.dispatch_ime_preedit("zhongwen", Some((0, 8))));
    let r1 = engine.ime_cursor_rect().expect("preedit IME rect");
    assert!(r1.x > r0.x, "IME anchor should advance with preedit: r0={r0:?} r1={r1:?}");
}

#[test]
fn test_p1_vector_bezier_svg_path_yuv_media_and_webview_host_sync() {
    // 1. CubicBezier + 完整 20 指令 SvgPath 矢量渲染
    let prims = [
        VectorPrimitive::CubicBezier {
            p0: VecPoint::new(10.0, 80.0),
            p1: VecPoint::new(50.0, 10.0),
            p2: VecPoint::new(110.0, 150.0),
            p3: VecPoint::new(150.0, 80.0),
            color: Color::rgb(59, 130, 246),
            width: 3.0,
            dash_len: 0.0,
            gap_len: 0.0,
        },
        VectorPrimitive::SvgPath {
            d: "M 30 20 L 90 20 C 120 20 120 70 90 70 L 30 70 Z",
            fill: Color::rgb(16, 185, 129),
            fill_rule: SvgFillRule::NonZero,
            stroke: Color::WHITE,
            stroke_width: 2.0,
            line_cap: SvgLineCap::Round,
            line_join: SvgLineJoin::Round,
        },
    ];
    let spec = VectorDrawSpec {
        primitives: &prims,
        width: 160,
        height: 160,
        opacity: 255,
    };
    let bounds = Region::new(0, 0, 160, 160);
    let mut fb = vec![Xrgb8888(0xFF00_0000); 160 * 160];
    rasterize_vector_spec(&spec, bounds, bounds, 1.0, 0, 0, 160, &mut fb);
    let non_black = fb.iter().filter(|px| px.0 != 0xFF00_0000).count();
    assert!(non_black > 1_500, "bezier + svg_path should render pixels, got {non_black}");

    // 2. NV12 & I420 -> RGBA8 定点转换
    let y = vec![200u8; 4 * 4];
    let uv = vec![128u8; 2 * 2 * 2];
    let mut rgba = vec![0u8; 4 * 4 * 4];
    assert_eq!(convert_nv12_to_rgba8(&y, &uv, 4, 4, &mut rgba), 16);
    assert!(rgba[0] > 180 && rgba[3] == 255);
    let u = vec![128u8; 4];
    let v = vec![128u8; 4];
    assert_eq!(convert_i420_to_rgba8(&y, &u, &v, 4, 4, &mut rgba), 16);
    assert!(rgba[1] > 180);

    // 3. WebViewHostSync 零冗余几何 Diff
    let mut sync = WebViewHostSync::new();
    let reg = WebViewHostRegion {
        id: 1,
        logical_bounds: Region::new(10, 20, 400, 300),
        clip_bounds: Region::new(0, 0, 800, 600),
        visible: true,
        z_order: 2,
    };
    let u1 = sync.diff_region(reg, 1.5).expect("initial sync must emit");
    assert_eq!(u1.physical_rect, (15, 30, 600, 450));
    assert!(sync.diff_region(reg, 1.5).is_none(), "idle frame must emit no diff");
}

#[test]
fn test_p1_ai_native_mcp_protocol_and_deterministic_vlxrec_replay() {
    struct CounterView {
        count: u32,
    }
    impl View for CounterView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let on_inc = cx.listener(|this, _: ClickEvent, _| {
                this.count += 1;
            });
            div()
                .w(320)
                .h(180)
                .flex_col()
                .p(16)
                .gap(12)
                .child(
                    cx,
                    div()
                        .id(5001)
                        .w(140)
                        .h(36)
                        .rounded(6)
                        .bg(Color::rgb(59, 130, 246))
                        .focusable()
                        .on_click(cx, on_inc)
                        .child(cx, text("Deploy Agent").size(13).color(Color::WHITE)),
                )
                .child(
                    cx,
                    text(fmt!(cx, "Count: {}", self.count))
                        .size(13)
                        .color(Color::WHITE),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 180);
    let root = engine.mount_root(CounterView { count: 0 });
    let _ = engine.step_frame_dt(16);

    // 1. MCP JSON-RPC 协议闭环：initialize -> tools/list -> velox_snapshot_tree -> velox_click_label
    let mut session = AutomationSession::new();
    let init_resp = session.handle_mcp_request(
        &mut engine,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
    );
    assert!(init_resp.contains("velox-automation-mcp"));

    let list_resp = session.handle_mcp_request(
        &mut engine,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    );
    assert!(list_resp.contains("velox_click_label"));

    let snap_resp = session.handle_mcp_request(
        &mut engine,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"velox_snapshot_tree"}}"#,
    );
    assert!(snap_resp.contains("Deploy Agent"));

    let click_resp = session.handle_mcp_request(
        &mut engine,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"velox_click_label","arguments":{"label":"Deploy Agent"}}}"#,
    );
    assert!(click_resp.contains("\\\"clicked\\\":true"), "resp={click_resp}");
    assert_eq!(engine.view(root).unwrap().count, 1);

    // 2. 确定性事件录像 (.vlxrec) + 全新引擎实例逐帧回放校验和 100% 匹配
    let mut rec_engine = VeloxEngine::new(320, 180);
    let _ = rec_engine.mount_root(CounterView { count: 0 });
    let _ = rec_engine.step_frame_dt(16);
    let mut recorder = EventRecorder::new();
    recorder.record_dispatch(&mut rec_engine, 16, InputEvent::PointerDown { x: 40, y: 30 });
    recorder.record_dispatch(&mut rec_engine, 16, InputEvent::PointerUp { x: 40, y: 30 });
    recorder.record_tick(&mut rec_engine, 16);
    let vlxrec = recorder.to_vlxrec_text();
    assert!(vlxrec.starts_with("VLXREC v1"));

    let mut replay_engine = VeloxEngine::new(320, 180);
    let rroot = replay_engine.mount_root(CounterView { count: 0 });
    let _ = replay_engine.step_frame_dt(16);
    let mut replayer = EventReplayer::from_vlxrec_text(&vlxrec).expect("parse vlxrec");
    let report = replayer.replay_all(&mut replay_engine);
    assert!(report.is_deterministic(), "report={report:?}");
    assert_eq!(report.total_steps, 3);
    assert_eq!(replay_engine.view(rroot).unwrap().count, 1);
}

#[test]
fn test_p2_multi_window_dock_tearoff_os_integration_and_live_devtools() {
    // 1. 多窗口 Dock Tear-Off 共享 Signal 状态 + 拖回吸附合并
    let shared_counter = signal(10i32);
    struct SharedPanelView {
        sig: Signal<i32>,
    }
    impl View for SharedPanelView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let v = self.sig.get();
            div()
                .id(8001)
                .w(240)
                .h(120)
                .p(8)
                .child(cx, text(fmt!(cx, "Shared={v}")).size(12).color(Color::WHITE))
                .into_element(cx)
        }
    }

    let mut mgr = MultiWindowManager::new();
    let main_id = mgr.open_window(
        MultiWindowSpec::primary("Velox IDE", 960, 640),
        SharedPanelView {
            sig: shared_counter,
        },
    );
    let torn_id = mgr.tear_off_dock_panel(
        main_id,
        "inspector_panel",
        "Inspector (Detached)",
        DockZone::Right,
        360,
        480,
        SharedPanelView {
            sig: shared_counter,
        },
    );
    assert_eq!(mgr.window_count(), 2);
    assert_eq!(mgr.detached_dock_panels().len(), 1);
    shared_counter.set(42);
    let reports = mgr.step_all(16);
    assert_eq!(reports.len(), 2);
    let redocked = mgr.redock_panel(torn_id, DockZone::Bottom);
    assert_eq!(redocked, Some((String::from("inspector_panel"), DockZone::Bottom)));
    assert_eq!(mgr.window_count(), 1);

    // 2. 富格式剪贴板 + 原生菜单栏/托盘/文件对话框/拖拽幽灵预览
    assert!(clipboard_write_payload(ClipboardPayload::Image(ClipboardImage {
        width: 2,
        height: 2,
        rgba8: vec![255; 16],
    })));
    assert!(matches!(
        clipboard_read_payload(),
        Some(ClipboardPayload::Image(img)) if img.width == 2
    ));

    let menu = NativeMenuBar::new().add_section(
        "File",
        vec![
            NativeMenuItem::action(101, "Open Workspace...", Some("Ctrl+O")),
            NativeMenuItem::Separator,
            NativeMenuItem::checkbox(102, "Live Hot Reload", true),
        ],
    );
    assert_eq!(menu.find_action_label(101), Some("Open Workspace..."));
    let tray = SystemTraySpec::new("velox.tray", "Velox Pro")
        .with_menu(vec![NativeMenuItem::action(201, "Quit", Some("Ctrl+Q"))]);
    assert_eq!(tray.menu.len(), 1);

    set_mock_file_dialog_handler(Some(|req: &FileDialogRequest| {
        FileDialogResult::Selected(vec![std::path::PathBuf::from(
            req.default_name.as_deref().unwrap_or("workspace.vlx"),
        )])
    }));
    let dialog_res = run_file_dialog(
        &FileDialogRequest::save_file("Save Session", "bug_repro.vlxrec")
            .add_filter("Velox Recording", &["vlxrec"]),
    );
    assert_eq!(
        dialog_res,
        FileDialogResult::Selected(vec![std::path::PathBuf::from("bug_repro.vlxrec")])
    );
    set_mock_file_dialog_handler::<fn(&FileDialogRequest) -> FileDialogResult>(None);

    let ghost = DragPreviewGhost::new("Inspector Panel", "Drag to detach window", 780, 580);
    let gb = ghost.clamped_bounds(Region::new(0, 0, 800, 600));
    assert!(gb.right() <= 800 && gb.bottom() <= 600);

    // 3. 交互式 Live DevTools：64 帧耗时瀑布图 + 零编译样式热重载
    let mut inspector = LiveInspectorState::new();
    for us in [1200u32, 2400, 3600, 1800] {
        inspector.budget.push_us(us);
    }
    assert_eq!(inspector.budget.avg_us(), 2250);
    assert_eq!(inspector.budget.max_us(), 3600);
    let applied = inspector.overrides.load_hot_reload_sheet(
        "8001: pad=14 gap=10 rounded=12 bg=#1E293B border=#3B82F6\n",
    );
    assert_eq!(applied, 1);
    let mut st = Style::DEFAULT;
    assert!(inspector.overrides.apply_to_style(8001, &mut st));
    assert_eq!(st.padding.left, 14);
    assert_eq!(st.gap, 10);
    assert_eq!(st.bg, Color::rgb(0x1E, 0x29, 0x3B));
}
