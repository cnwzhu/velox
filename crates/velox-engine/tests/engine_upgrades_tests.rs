//! 引擎全栈架构升级端到端验证测试套件：
//! 1. 核心热结构零膨胀红线；
//! 2. 多字体/多字重/斜体/BiDi/连字/彩色 Emoji/`RichText` 双后端渲染；
//! 3. 纯 Rust PNG & BMP 解码器、`LruImageCache`、异步解码、`ObjectFit` 与 `NineSlice`；
//! 4. 高级布局（`aspect_ratio`、Grid `fr` & `col_span`、RTL 镜像、`sticky_top`、`overflow_auto`、嵌套滚动链）；
//! 5. `Spring` 物理弹簧、`Keymap` 作用域弦键命令路由、`PointerCapture`、`DragPayload` 与多点手势；
//! 6. 多窗口管理、`CsdConfig` 命中测试、`AccessKitBridge` 增量 Diff 与逆向动作、WebGPU 宿主与桌面框架模板；
//! 7. 原生 2D 矢量画布 `VectorCanvas`、GPU 脏区 Scissor 与内置 DevTools Inspector HUD。

use core::mem::size_of;

use velox_engine::element::{DivNode, DivVisual};
use velox_engine::motion::DrawOpSnapshot;
use velox_engine::prelude::*;

#[test]
fn test_upgrade_zero_bloat_invariants_and_spring_keymap_gestures() {
    // 1. 严守核心热数据结构零体积膨胀红线
    assert_eq!(size_of::<Style>(), 24);
    assert_eq!(size_of::<DivNode<'static>>(), 64);
    assert_eq!(size_of::<DivVisual<'static>>(), 64);
    assert_eq!(size_of::<DrawOpSnapshot>(), 12);

    // 2. Spring 二阶物理弹簧：打断反向切换保持速度连续并最终收敛静止
    let mut sp = Spring::new(0.0, SpringConfig::SNAPPY);
    assert!(sp.is_settled());
    sp.animate_to(100.0);
    assert!(sp.is_running());
    let mut mid_v = 0.0;
    for _ in 0..8 {
        sp.step(16);
        mid_v = sp.velocity();
    }
    assert!(mid_v > 0.0, "spring must gain positive velocity toward 100");
    // 中途反向打断回 20，瞬时速度仍被保留不会突变为 0
    sp.animate_to(20.0);
    assert_eq!(sp.velocity(), mid_v);
    for _ in 0..180 {
        sp.step(16);
        if sp.is_settled() {
            break;
        }
    }
    assert!(sp.is_settled(), "spring must eventually settle to 0% CPU");
    assert_eq!(sp.value_i32(), 20);

    // 3. Keymap 作用域快捷键 + 两段弦键（Ctrl+K -> Ctrl+S）与超时复位
    let mut km = Keymap::<32>::new();
    const ACT_SAVE_GLOBAL: ActionId = ActionId(1);
    const ACT_SAVE_EDITOR: ActionId = ActionId(2);
    const ACT_CHORD_KEYS: ActionId = ActionId(3);
    assert!(km.bind(KeyBinding::global(
        KeyChord::single(KeyStroke::ctrl('s')),
        ACT_SAVE_GLOBAL,
    )));
    assert!(km.bind(KeyBinding::scoped(
        "CodeEditor",
        KeyChord::single(KeyStroke::ctrl('s')),
        ACT_SAVE_EDITOR,
    )));
    assert!(km.bind(KeyBinding::global(
        KeyChord::pair(KeyStroke::ctrl('k'), KeyStroke::ctrl('s')),
        ACT_CHORD_KEYS,
    )));

    // 作用域优先覆盖全局
    assert_eq!(
        km.resolve(&["CodeEditor", "Workspace"], KeyStroke::ctrl('s'), 100),
        KeymapMatch::Matched(ACT_SAVE_EDITOR)
    );
    assert_eq!(
        km.resolve(&["Sidebar"], KeyStroke::ctrl('s'), 100),
        KeymapMatch::Matched(ACT_SAVE_GLOBAL)
    );
    // 两段弦键匹配
    assert_eq!(
        km.resolve(&["Workspace"], KeyStroke::ctrl('k'), 200),
        KeymapMatch::PendingChord
    );
    assert_eq!(
        km.resolve(&["Workspace"], KeyStroke::ctrl('s'), 400),
        KeymapMatch::Matched(ACT_CHORD_KEYS)
    );
    // 弦键超时后回落普通单键
    assert_eq!(
        km.resolve(&["Workspace"], KeyStroke::ctrl('k'), 1000),
        KeymapMatch::PendingChord
    );
    assert_eq!(
        km.resolve(&["Workspace"], KeyStroke::ctrl('s'), 5000),
        KeymapMatch::Matched(ACT_SAVE_GLOBAL)
    );

    // 4. Unicode BiDi 视觉重排与 OpenType 编程连字塑形（无堆）
    let bidi = reorder_bidi_visual("مرحبا 123 (OK)", true);
    assert!(bidi.len() >= 2);
    assert_eq!(mirror_bidi_char('('), ')');
    assert_eq!(mirror_bidi_char('['), ']');
    let shaped = shape_text_run("a->b!=c<=d", true);
    let chars: SmallVec<char, 32> = {
        let mut v = SmallVec::new();
        for g in shaped.as_slice() {
            let _ = v.push(g.ch);
        }
        v
    };
    assert!(chars.as_slice().contains(&'→'));
    assert!(chars.as_slice().contains(&'≠'));
    assert!(chars.as_slice().contains(&'≤'));

    // 5. 引擎手势、指针捕获、DragPayload 与系统主题事件
    let mut engine = VeloxEngine::new(320, 240);
    engine.set_pointer_capture(77);
    assert_eq!(engine.pointer_capture_id(), Some(77));
    engine.release_pointer_capture();
    assert_eq!(engine.pointer_capture_id(), None);
    engine.begin_drag_payload(DragPayload {
        kind_tag: 2,
        payload_id: 99,
        source_id: 10,
    });
    assert_eq!(engine.take_drag_payload().unwrap().payload_id, 99);
    assert!(engine.dispatch_event(InputEvent::Pinch {
        x: 50,
        y: 50,
        delta_scale_x1000: 125
    }));
    assert_eq!(engine.last_pinch_delta_x1000(), 125);
    assert!(engine.dispatch_event(InputEvent::Rotate {
        x: 50,
        y: 50,
        delta_deg10: -45
    }));
    assert_eq!(engine.last_rotate_deg10(), -45);
    assert!(engine.dispatch_event(InputEvent::SystemThemeChanged { dark: false }));
    assert!(!engine.system_theme_is_dark());
}

#[test]
fn test_advanced_layout_aspect_ratio_grid_fr_span_rtl_sticky_and_scroll_chaining() {
    struct AdvancedLayoutView;
    impl View for AdvancedLayoutView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(400)
                .h(300)
                .flex_col()
                .child(
                    cx,
                    // 1. aspect_ratio = 2.0，宽 120 -> 自动高 60
                    div()
                        .id(10)
                        .w(120)
                        .aspect_ratio(2.0)
                        .bg(Color::rgb(20, 80, 140)),
                )
                .child(
                    cx,
                    // 2. Grid 1fr 2fr 1fr（总宽 400 -> 列宽 100 / 200 / 100），子项 21 跨前 2 列应宽 300
                    div()
                        .w(400)
                        .grid_template_fr(cx, &[1, 2, 1])
                        .child(cx, div().id(21).grid_col_span(2).h(20).bg(Color::rgb(50, 50, 50)))
                        .child(cx, div().id(22).h(20).bg(Color::rgb(80, 80, 80))),
                )
                .child(
                    cx,
                    // 3. RTL 行布局：子项 31 (宽 60) 应镜像至最右侧 x=340，子项 32 (宽 40) 在其左侧 x=300
                    div()
                        .w(400)
                        .h(20)
                        .flex_row()
                        .rtl(true)
                        .child(cx, div().id(31).w(60).h(20).bg(Color::rgb(200, 40, 40)))
                        .child(cx, div().id(32).w(40).h(20).bg(Color::rgb(40, 200, 40))),
                )
                .child(
                    cx,
                    // 4. 外层滚动容器 (id=40) + 内层滚动容器 (id=41) + sticky 吸顶表头 (id=42)
                    div()
                        .id(40)
                        .w(400)
                        .h(100)
                        .overflow_auto()
                        .flex_col()
                        .child(
                            cx,
                            div()
                                .id(42)
                                .sticky_top(0)
                                .w(400)
                                .h(20)
                                .bg(Color::rgb(255, 180, 0)),
                        )
                        .child(
                            cx,
                            div()
                                .id(41)
                                .w(400)
                                .h(60)
                                .scrollable(40)
                                .flex_col()
                                .child(cx, div().w(400).h(100).bg(Color::rgb(30, 30, 60))),
                        )
                        .child(cx, div().w(400).h(120).bg(Color::rgb(20, 40, 20))),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(400, 300);
    let _ = engine.mount_root(AdvancedLayoutView);
    let _ = engine.step_frame_dt(16);

    let find_bounds = |eng: &VeloxEngine, id: u32| -> Region {
        eng.cached_draw_ops()
            .iter()
            .find(|op| op.id == Some(id))
            .map(|op| op.bounds)
            .expect("missing DrawOp id")
    };

    // aspect_ratio
    let b10 = find_bounds(&engine, 10);
    assert_eq!(b10.w, 120);
    assert_eq!(b10.h, 60);

    // Grid 1fr 2fr 1fr + span 2
    let b21 = find_bounds(&engine, 21);
    let b22 = find_bounds(&engine, 22);
    assert_eq!(b21.x, 0);
    assert_eq!(b21.w, 300);
    assert_eq!(b22.x, 300);
    assert_eq!(b22.w, 100);

    // RTL 水平镜像
    let b31 = find_bounds(&engine, 31);
    let b32 = find_bounds(&engine, 32);
    assert_eq!(b31.x, 340);
    assert_eq!(b32.x, 300);

    // ScrollChaining 嵌套滚动链：在内层滚动容器 (y=130) 连续向下滚动，先滚满内层(40)，再冒泡驱动外层(40)
    let _ = engine.dispatch_event(InputEvent::Scroll {
        x: 50,
        y: 130,
        delta_y: 40,
    });
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.motion_mut().scroll_offset(41), 40);
    assert_eq!(engine.motion_mut().scroll_offset(40), 0);

    // 内层已触底，再次滚轮必须冒泡到外层容器 id=40
    let _ = engine.dispatch_event(InputEvent::Scroll {
        x: 50,
        y: 130,
        delta_y: 35,
    });
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.motion_mut().scroll_offset(40), 35);

    // sticky_top(0)：外层向下滚动 35px 后，吸顶表头 id=42 依然钉在滚动视口顶部 y=100 处！
    let b42 = find_bounds(&engine, 42);
    assert_eq!(b42.y, 100, "sticky header must stay pinned at scroll viewport top");
}

#[test]
fn test_vector_canvas_and_devtools_inspector_zero_heap() {
    static POLY_PTS: [VecPoint; 3] = [
        VecPoint::new(10.0, 50.0),
        VecPoint::new(50.0, 10.0),
        VecPoint::new(90.0, 50.0),
    ];

    struct VecAndInspectorView {
        show_inspector: bool,
        telemetry: FrameTelemetry,
    }
    impl View for VecAndInspectorView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut root = div()
                .w(120)
                .h(80)
                .bg(Color::rgb(10, 15, 25))
                .child(
                    cx,
                    vector_canvas(
                        cx,
                        100,
                        60,
                        &[
                            VectorPrimitive::Polygon {
                                points: &POLY_PTS,
                                fill: Color::rgb(30, 180, 120),
                                stroke: Color::rgb(255, 255, 255),
                                stroke_width: 2.0,
                            },
                            VectorPrimitive::Circle {
                                cx: 50.0,
                                cy: 35.0,
                                radius: 8.0,
                                fill: Color::rgb(240, 80, 60),
                                stroke: Color::TRANSPARENT,
                                stroke_width: 0.0,
                            },
                        ],
                    ),
                );
            if self.show_inspector {
                let dirty = [Region::new(0, 0, 40, 40)];
                root = root.child(
                    cx,
                    build_inspector_overlay(cx, &self.telemetry, &dirty, InspectorConfig::ALL),
                );
            }
            root.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(120, 80);
    let root = engine.mount_root(VecAndInspectorView {
        show_inspector: false,
        telemetry: FrameTelemetry::default(),
    });
    let _ = engine.step_frame_dt(16);

    // 验证 VectorCanvas 在无堆软件光栅下正确绘出红色圆心与绿色多边形像素
    let mut fb = [Rgba8888(0); 120 * 80];
    rasterize_region(
        engine.cached_draw_ops(),
        engine.viewport_region(),
        0,
        0,
        120,
        &mut fb,
    );
    let circle_px = fb[35 * 120 + 50].to_color();
    assert!(
        circle_px.r() > 200 && circle_px.g() < 110,
        "expected red circle center in VectorCanvas, got {circle_px:?}"
    );
    let poly_px = fb[45 * 120 + 30].to_color();
    assert!(
        poly_px.g() > 140 && poly_px.r() < 60,
        "expected green polygon fill in VectorCanvas, got {poly_px:?}"
    );

    // 采集性能遥测并挂载 DevTools Inspector HUD
    let tel = capture_telemetry(&engine, Some((20, 20)));
    assert!(tel.draw_op_count >= 2);
    engine.update_view(root, |v| {
        v.show_inspector = true;
        v.telemetry = tel;
    });
    let _ = engine.step_frame_dt(16);
    assert!(
        engine.cached_draw_ops().len() > tel.draw_op_count,
        "Inspector overlay must append HUD & dirty rect DrawOps"
    );
}

#[test]
fn test_png_bmp_decode_lru_cache_async_loader_and_richtext() {
    use velox_engine::platform::shm_driver::write_uncompressed_png;

    // 1. 生成标准 4x4 PNG 字节流并用内置纯 Rust PNG 解码器完整还原
    let mut png_pixels = Vec::with_capacity(4 * 4);
    for y in 0..4u8 {
        for x in 0..4u8 {
            png_pixels.push(Xrgb8888::from_color(Color::rgb(
                x * 60 + 20,
                y * 60 + 10,
                200,
            )));
        }
    }
    let tmp_png = std::env::temp_dir().join("velox_decode_test.png");
    write_uncompressed_png(tmp_png.to_str().unwrap(), 4, 4, &png_pixels)
        .expect("write_uncompressed_png");
    let png_bytes = std::fs::read(&tmp_png).expect("read png");
    let _ = std::fs::remove_file(&tmp_png);

    assert_eq!(detect_image_format(&png_bytes), ImageFormat::Png);
    let (pw, ph, rgba) = decode_image_bytes(&png_bytes).expect("pure-Rust PNG decode must succeed");
    assert_eq!((pw, ph), (4, 4));
    assert_eq!(&rgba[0..4], &[20, 10, 200, 255]);
    assert_eq!(&rgba[(3 * 4 + 3) * 4..(3 * 4 + 3) * 4 + 4], &[200, 190, 200, 255]);

    // 2. 构造标准 2x2 24-bit BMP 字节流并解码验证
    let mut bmp = vec![0u8; 54 + 16]; // 每行 6 字节对齐到 8 字节 -> 16 字节像素
    bmp[0..2].copy_from_slice(b"BM");
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&2i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&(-2i32).to_le_bytes()); // top-down
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    // (0,0) BGR = (10, 20, 250) -> RGB(250, 20, 10)
    bmp[54..57].copy_from_slice(&[10, 20, 250]);
    assert_eq!(detect_image_format(&bmp), ImageFormat::Bmp);
    let (bw, bh, brgba) = decode_image_bytes(&bmp).expect("pure-Rust BMP decode must succeed");
    assert_eq!((bw, bh), (2, 2));
    assert_eq!(&brgba[0..4], &[250, 20, 10, 255]);

    // 3. LruImageCache 字节预算淘汰验证
    let mut lru = LruImageCache::new(1500);
    let mk_px = |rev: u64| DecodedPixels {
        width: 16,
        height: 16,
        rgba: std::sync::Arc::from(vec![255u8; 1024].into_boxed_slice()),
        revision: rev,
    };
    lru.insert(ImageAssetId(1), mk_px(1));
    assert_eq!(lru.len(), 1);
    lru.insert(ImageAssetId(2), mk_px(2)); // 2048 > 1500 -> 淘汰最旧的 id=1
    assert_eq!(lru.len(), 1);
    assert!(lru.get(ImageAssetId(1)).is_none());
    assert!(lru.get(ImageAssetId(2)).is_some());

    // 4. 异步加载队列 + DecodedImage + RichText（粗体/斜体/等宽/高亮/下划线/彩色 Emoji）完整渲染验证
    let (tx, rx) = std::sync::mpsc::channel();
    load_image_bytes_async(
        ImageAssetId(500),
        png_bytes,
        Some(std::sync::Arc::new(move |id, ok| {
            let _ = tx.send((id, ok));
        })),
    );
    let (loaded_id, ok) = rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("async image decode timeout");
    assert_eq!(loaded_id, ImageAssetId(500));
    assert!(ok);
    assert!(matches!(
        image_load_state(ImageAssetId(500)),
        Some(ImageLoadState::Ready(_))
    ));

    struct RichAndImgView;
    impl View for RichAndImgView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(240)
                .h(120)
                .bg(Color::rgb(12, 16, 24))
                .flex_col()
                .gap(8)
                .child(
                    cx,
                    rich_text(
                        cx,
                        &[
                            TextSpan::new("Velox ")
                                .bold()
                                .color(Color::rgb(255, 255, 255)),
                            TextSpan::new("a->b ")
                                .mono()
                                .ligatures(true)
                                .underline()
                                .bg(Color::rgb(30, 64, 175))
                                .color(Color::rgb(250, 204, 21)),
                            TextSpan::new("🚀").size(16),
                        ],
                    ),
                )
                .child(
                    cx,
                    decoded_image(ImageAssetId(500), 64, 48)
                        .fit(ObjectFit::Cover)
                        .rounded(8),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 120);
    let _ = engine.mount_root(RichAndImgView);
    let _ = engine.step_frame_dt(16);

    let mut fb = vec![Rgba8888(0); 240 * 120];
    rasterize_region(
        engine.cached_draw_ops(),
        engine.viewport_region(),
        0,
        0,
        240,
        &mut fb,
    );
    // 验证 RichText 与 DecodedImage 都渲染出了非背景色像素
    let non_bg_count = fb
        .iter()
        .filter(|p| {
            let c = p.to_color();
            c != Color::rgb(12, 16, 24)
        })
        .count();
    assert!(
        non_bg_count > 1000,
        "RichText + DecodedImage must paint rich pixels, got {non_bg_count}"
    );
}

#[test]
fn test_multi_window_csd_accesskit_bridge_webgpu_and_host_templates() {
    use core::sync::atomic::{AtomicU32, Ordering};
    use velox_engine::platform::a11y_bridge::{AccessKitActionRequest, AccessKitBridge};
    use velox_engine::platform::embed_ffi::{
        GTK4_DMABUF_WIDGET_C, QT6_QQUICK_RHI_ITEM_CPP, WINUI3_SURFACE_HOST_CPP,
        WPF_D3DIMAGE_HOST_CS,
    };
    use velox_engine::platform::web::{
        WebGpuBackendPreference, WebGpuCanvasHost, WEBGPU_HOST_JS_BRIDGE,
    };
    use velox_engine::platform::winit::{CsdConfig, CsdHitZone, MultiWindowManager, MultiWindowSpec};

    static A11Y_CLICKS: AtomicU32 = AtomicU32::new(0);
    A11Y_CLICKS.store(0, Ordering::Relaxed);

    struct MainWinView;
    impl View for MainWinView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(400)
                .h(300)
                .bg(Color::rgb(15, 20, 30))
                .child(
                    cx,
                    div()
                        .id(555)
                        .w(120)
                        .h(40)
                        .bg(Color::rgb(59, 130, 246))
                        .on_click(cx, &|_ev, _acx| {
                            A11Y_CLICKS.fetch_add(1, Ordering::Relaxed);
                        })
                        .child(cx, text("Action")),
                )
                .into_element(cx)
        }
    }

    struct PopoverView;
    impl View for PopoverView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div().w(160).h(120).bg(Color::rgb(30, 35, 45)).into_element(cx)
        }
    }

    // 1. MultiWindowManager + CSD 无边框命中测试 + 父子浮层级联关闭
    let mut mgr = MultiWindowManager::new();
    let csd = CsdConfig {
        enabled: true,
        titlebar_height: 36,
        resize_border_px: 6,
        controls_inset_right: 120,
        double_click_maximize: true,
    };
    let main_id = mgr.open_window(
        MultiWindowSpec::primary("Main", 400, 300).with_csd(csd),
        MainWinView,
    );
    let pop_id = mgr.open_window(MultiWindowSpec::popover(main_id, 160, 120), PopoverView);
    assert_eq!(mgr.window_count(), 2);

    assert_eq!(mgr.hit_test_csd(main_id, 2, 2), CsdHitZone::ResizeTopLeft);
    assert_eq!(mgr.hit_test_csd(main_id, 100, 18), CsdHitZone::Caption);
    assert_eq!(mgr.hit_test_csd(main_id, 350, 18), CsdHitZone::Client); // 右上角按钮排除区
    assert_eq!(mgr.hit_test_csd(main_id, 200, 150), CsdHitZone::Client);

    let reports = mgr.step_all(16);
    assert_eq!(reports.len(), 2);
    assert!(reports[0].2.is_some(), "initial AccessKit sync must emit full tree");

    // 静止帧增量同步返回 None（零屏幕阅读器 IPC 噪声）
    let mut bridge = AccessKitBridge::new();
    let main_eng = &mut mgr.window_mut(main_id).unwrap().engine;
    let init_upd = bridge.sync_engine(main_eng).expect("first sync");
    assert!(!init_upd.updated_nodes.is_empty());
    assert!(bridge.sync_engine(main_eng).is_none());

    // 逆向 AccessKit Click 动作精准触发按钮回调
    let btn_node = init_upd
        .updated_nodes
        .iter()
        .find(|n| n.id == 555)
        .expect("button node 555 must exist in AccessKit tree");
    assert!(bridge.apply_action_request(
        main_eng,
        AccessKitActionRequest::Click {
            target_id: btn_node.id
        }
    ));
    assert_eq!(A11Y_CLICKS.load(Ordering::Relaxed), 1);

    // 广播系统主题切换 + 关闭主窗自动级联关闭 Popover
    assert_eq!(
        mgr.broadcast_event(InputEvent::SystemThemeChanged { dark: false }),
        2
    );
    assert!(!mgr.window(pop_id).unwrap().engine.system_theme_is_dark());
    assert!(mgr.close_window(main_id));
    assert_eq!(mgr.window_count(), 0);

    // 2. WebGPU 宿主 + 四大桌面框架零拷贝集成模板完整性校验
    let mut webgpu = WebGpuCanvasHost::new(640, 480, 2.0, WebGpuBackendPreference::WebGpuFirst);
    assert_eq!(webgpu.physical_size(), (1280, 960));
    assert!(webgpu.is_gpu_active());
    let _ = webgpu.engine.mount_root(PopoverView);
    assert!(webgpu.step_needs_present(16));
    assert!(WEBGPU_HOST_JS_BRIDGE.contains("\"gpu\" in navigator"));
    assert!(QT6_QQUICK_RHI_ITEM_CPP.contains("QQuickRhiItem"));
    assert!(WPF_D3DIMAGE_HOST_CS.contains("D3DImage"));
    assert!(WINUI3_SURFACE_HOST_CPP.contains("OpenSharedHandle"));
    assert!(GTK4_DMABUF_WIDGET_C.contains("GdkDmabufTextureBuilder"));
}

#[cfg(feature = "gpu")]
#[test]
fn test_gpu_richtext_decoded_image_vector_canvas_and_dirty_scissor() {
    use velox_engine::renderers::wgpu::{
        PreparedWgpuContext, WgpuOffscreenRenderer, WgpuRenderer, wgpu,
    };

    let Some(ctx) = PreparedWgpuContext::from_host_adapter_id(None, None) else {
        return;
    };

    // 注册一张纯红 8x8 纹理进全局 LRU 缓存
    let red_px = vec![240u8, 40, 60, 255].repeat(64);
    register_image_rgba(ImageAssetId(700), 8, 8, red_px).expect("register_image_rgba");

    static LINE_PTS: [VecPoint; 2] = [VecPoint::new(8.0, 20.0), VecPoint::new(92.0, 20.0)];

    struct GpuUpgradeView;
    impl View for GpuUpgradeView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(200)
                .h(120)
                .bg(Color::rgb(10, 14, 22))
                .flex_col()
                .gap(4)
                .child(
                    cx,
                    rich_text(
                        cx,
                        &[
                            TextSpan::new("GPU ").bold().color(Color::rgb(255, 255, 255)),
                            TextSpan::new("🚀").size(16),
                        ],
                    ),
                )
                .child(cx, decoded_image(ImageAssetId(700), 40, 40).fit(ObjectFit::Fill))
                .child(
                    cx,
                    vector_canvas(
                        cx,
                        100,
                        40,
                        &[VectorPrimitive::Polyline {
                            points: &LINE_PTS,
                            color: Color::rgb(40, 230, 120),
                            width: 4.0,
                            dash_len: 0.0,
                            gap_len: 0.0,
                            closed: false,
                        }],
                    ),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(200, 120);
    let _ = engine.mount_root(GpuUpgradeView);
    let _ = engine.step_frame_dt(16);

    let mut offscreen = WgpuOffscreenRenderer::from_shared_device(
        ctx.device().clone(),
        ctx.queue().clone(),
        200,
        120,
    );
    assert!(offscreen.render_draw_ops_scaled(
        engine.cached_draw_ops(),
        engine.viewport_region(),
        1.0,
    ));

    // 验证 GPU 端渲染出 DecodedImage 的红色像素与 VectorCanvas 的绿色折线像素
    let mut red_hits = 0usize;
    let mut green_hits = 0usize;
    for px in offscreen.rgba_buffer.chunks_exact(4) {
        if px[0] > 200 && px[1] < 80 && px[2] < 100 {
            red_hits += 1;
        }
        if px[1] > 180 && px[0] < 80 {
            green_hits += 1;
        }
    }
    assert!(red_hits > 500, "GPU DecodedImage must render red pixels, got {red_hits}");
    assert!(green_hits > 100, "GPU VectorCanvas must render green polyline, got {green_hits}");

    // 验证 WgpuRenderer::render_to_view_dirty GPU 硬件 Scissor 局部刷新零错误执行
    let tex = ctx.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("velox_dirty_scissor_target"),
        size: wgpu::Extent3d {
            width: 200,
            height: 120,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut renderer = WgpuRenderer::new(ctx.device(), wgpu::TextureFormat::Rgba8Unorm);
    renderer.render_to_view_dirty(
        ctx.device(),
        ctx.queue(),
        engine.cached_draw_ops(),
        engine.viewport_region(),
        1.0,
        &view,
        200,
        120,
        &[Region::new(0, 0, 100, 60)],
        wgpu::Color::BLACK,
    );
    let _ = ctx.device().poll(wgpu::PollType::wait_indefinitely());
}
