//! `velox_engine::platform` 统一多后端测试套件（覆盖 `testing`、`wayland`、`winit`、`mobile`、`web` 平台适配）。

use velox_engine::platform::mobile::{DeviceOrientation, MobilePlatform, TouchPhase};
use velox_engine::platform::testing::*;
use velox_engine::platform::web::WebPlatform;
use velox_engine::platform::*;
use velox_engine::prelude::*;

use velox_engine::platform::mobile::MobileEmbedApp;
use velox_engine::platform::shm_driver::{WaylandSurfaceDriver, WlSurface};
use velox_engine::platform::wayland::WaylandPlatform;
use velox_engine::platform::web::WebCanvasApp;
use velox_engine::platform::winit::WinitPlatform;

struct TestPanel {
    active: bool,
}

impl View for TestPanel {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = *cx.theme();
        div()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .p(10)
            .child(
                cx,
                div()
                    .w(120)
                    .h(60)
                    .rounded(10)
                    .bg(if self.active {
                        theme.warning
                    } else {
                        theme.card
                    })
                    .border(2, theme.primary)
                    .child(cx, text("Velox Panel").size(14).color(theme.foreground)),
            )
            .into_element(cx)
    }
}

struct ButtonDemo {
    toggled: bool,
}

impl View for ButtonDemo {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = *cx.theme();
        let btn_bg = if self.toggled {
            theme.warning
        } else {
            theme.primary
        };
        let cb = cx.listener(|this, _: (), cx| {
            this.toggled = !this.toggled;
            cx.notify();
        });
        let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| cb((), acx));

        div()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .p(20)
            .child(
                cx,
                div()
                    .w(80)
                    .h(40)
                    .rounded(8)
                    .bg(btn_bg)
                    .on_click_raw(raw_cb),
            )
            .into_element(cx)
    }
}

#[test]
fn test_headless_testing_platform() {
    let mut engine = VeloxEngine::new(320, 240);
    let handle = engine.mount_root(ButtonDemo { toggled: false });
    let mut test_plat: TestingPlatform<320, 240> = TestingPlatform::new();

    // 首帧渲染
    assert_eq!(
        test_plat.step_platform(&mut engine, 16),
        PlatformControlFlow::Poll
    );
    assert_eq!(test_plat.total_frames_rendered, 1);
    let initial_btn_color = test_plat.framebuffer[30][30];

    // 模拟点击并推进一帧
    test_plat.send_click(30, 30);
    assert_eq!(
        test_plat.step_platform(&mut engine, 16),
        PlatformControlFlow::Poll
    );
    assert!(engine.view(handle).unwrap().toggled);
    let toggled_btn_color = test_plat.framebuffer[30][30];
    assert_ne!(initial_btn_color, toggled_btn_color);
}

#[test]
fn test_wayland_and_winit_platforms() {
    let mut core = VeloxEngine::new(320, 240);
    core.mount_root(ButtonDemo { toggled: false });

    let mut driver = WaylandSurfaceDriver::new(320, 240);
    let mut surface = WlSurface::new();

    // 第 1 帧：首帧全屏提交 (slot 0, age=0 -> 1)
    assert!(driver.flush_frame(&mut core, &mut surface));
    assert_eq!(surface.committed_damage, vec![Region::new(0, 0, 320, 240)]);

    // 第 2 帧：静止帧，0% CPU 挂起
    assert!(!driver.flush_frame(&mut core, &mut surface));

    // 第 3 帧：点击按钮 (x=30, y=30)，仅提交按钮局部矩形 (80x40) 到 slot 1
    assert!(core.dispatch_click(30, 30));
    let prev_buf = surface.attached_buffer_id.unwrap();
    assert!(driver.flush_frame(&mut core, &mut surface));
    driver.release_buffer(prev_buf);
    assert_eq!(surface.committed_damage, vec![Region::new(20, 20, 80, 40)]);
    assert_eq!(driver.slots[0].age, 2);

    // 验证纯 Wayland 客户端平台 WaylandPlatform (LayerShell 与 XdgToplevel)
    {
        let mut core_wl = VeloxEngine::new(680, 32);
        core_wl.mount_root(ButtonDemo { toggled: false });
        let mut wl_plat = WaylandPlatform::new_layer_shell("velox.titlebar", 680, 32, 32);
        assert_eq!(wl_plat.surface_size(), (680, 32));
        assert_eq!(
            wl_plat.step_platform(&mut core_wl, 16),
            PlatformControlFlow::Poll
        );
        assert_eq!(
            wl_plat.step_platform(&mut core_wl, 16),
            PlatformControlFlow::Wait
        );
    }

    // 验证 WinitPlatform
    let mut core_plat = VeloxEngine::new(320, 240);
    core_plat.mount_root(ButtonDemo { toggled: false });
    let mut winit_plat = WinitPlatform::new("Winit Platform Test", 320, 240).with_dpi_scale(1);
    assert_eq!(winit_plat.surface_size(), (320, 240));
    assert_eq!(
        winit_plat.step_platform(&mut core_plat, 16),
        PlatformControlFlow::Poll
    );
    assert_eq!(
        winit_plat.step_platform(&mut core_plat, 16),
        PlatformControlFlow::Wait
    );

    // 验证 Wayland 二进制线协议 (WaylandWireState) 握手、ping/pong、configure/ack 与指针点击派发
    {
        use velox_engine::platform::wayland::WaylandWireState;
        let mut wl_wire_test =
            WaylandPlatform::new_xdg_toplevel("Velox Wire", "dev.velox.app", 320, 240);
        let mut core_wire = VeloxEngine::new(320, 240);
        let wire_btn = core_wire.mount_root(ButtonDemo { toggled: false });
        let _ = wl_wire_test.step_platform(&mut core_wire, 16);

        let mut compositor_pkt = Vec::new();
        // 1. xdg_wm_base.ping(serial = 42) -> object 6, opcode 0, size 12
        WaylandWireState::write_header(&mut compositor_pkt, 6, 0, 12);
        WaylandWireState::write_u32(&mut compositor_pkt, 42);
        // 2. xdg_toplevel.configure(360, 260, empty_array) -> object 14, opcode 0, size 20
        WaylandWireState::write_header(&mut compositor_pkt, 14, 0, 20);
        WaylandWireState::write_i32(&mut compositor_pkt, 360);
        WaylandWireState::write_i32(&mut compositor_pkt, 260);
        WaylandWireState::write_u32(&mut compositor_pkt, 0);
        // 3. xdg_surface.configure(serial = 99) -> object 13, opcode 0, size 12
        WaylandWireState::write_header(&mut compositor_pkt, 13, 0, 12);
        WaylandWireState::write_u32(&mut compositor_pkt, 99);
        // 4. wl_pointer.enter(serial=1, surface=12, sx=30.0 (30<<8), sy=30.0 (30<<8)) -> object 16, opcode 0, size 24
        WaylandWireState::write_header(&mut compositor_pkt, 16, 0, 24);
        WaylandWireState::write_u32(&mut compositor_pkt, 1);
        WaylandWireState::write_u32(&mut compositor_pkt, 12);
        WaylandWireState::write_i32(&mut compositor_pkt, 30 << 8);
        WaylandWireState::write_i32(&mut compositor_pkt, 30 << 8);
        // 5. wl_pointer.button(serial=2, time=10, button=0x110, state=1) -> object 16, opcode 3, size 24
        WaylandWireState::write_header(&mut compositor_pkt, 16, 3, 24);
        WaylandWireState::write_u32(&mut compositor_pkt, 2);
        WaylandWireState::write_u32(&mut compositor_pkt, 10);
        WaylandWireState::write_u32(&mut compositor_pkt, 0x110);
        WaylandWireState::write_u32(&mut compositor_pkt, 1);
        // 6. wl_pointer.button(serial=3, time=20, button=0x110, state=0) -> object 16, opcode 3, size 24
        WaylandWireState::write_header(&mut compositor_pkt, 16, 3, 24);
        WaylandWireState::write_u32(&mut compositor_pkt, 3);
        WaylandWireState::write_u32(&mut compositor_pkt, 20);
        WaylandWireState::write_u32(&mut compositor_pkt, 0x110);
        WaylandWireState::write_u32(&mut compositor_pkt, 0);

        wl_wire_test.feed_wire_bytes(&mut core_wire, &compositor_pkt);
        assert_eq!(wl_wire_test.surface_size(), (360, 260));
        assert!(wl_wire_test.wire_state.configured);
        assert!(core_wire.view(wire_btn).unwrap().toggled);
        let auto_replies = wl_wire_test.wire_state.take_outbound_bytes();
        // pong (12B) + ack_configure (12B) + destroy_double_buffers (16B) + create_double_buffers (64B)
        assert!(auto_replies.len() >= 104);

        // 验证 configure 触发的动态 resize 后下一帧产出完整新尺寸 (360x260) 脏区
        let resize_report = wl_wire_test.flush_frame_dt(&mut core_wire, 16);
        assert!(resize_report.has_updates());
        assert_eq!(
            wl_wire_test.committed_damage(),
            &[Region::new(0, 0, 360, 260)]
        );
        assert_eq!(wl_wire_test.front_pixels().len(), 360 * 260);

        // 验证 wl_pointer.leave (opcode 1) 转换生成 InputEvent::PointerLeave
        let mut leave_pkt = Vec::new();
        WaylandWireState::write_header(&mut leave_pkt, 16, 1, 16);
        WaylandWireState::write_u32(&mut leave_pkt, 4);
        WaylandWireState::write_u32(&mut leave_pkt, 12);
        wl_wire_test.feed_wire_bytes(&mut core_wire, &leave_pkt);
        assert!(wl_wire_test
            .pending_events
            .contains(&velox_engine::platform::wayland::WaylandWireEvent::SeatInput(
                InputEvent::PointerLeave
            )));
    }
}

#[test]
fn test_renderers_software_and_lowered_sink() {
    use velox_engine::renderers::software::SoftwareRenderer;
    use velox_engine::renderers::{lower_draw_ops, LoweredPrimitiveSink};

    // 1. SoftwareRenderer 与 PixelBufferRenderer 局部 Rgb565 光栅化
    let mut eng_sw = VeloxEngine::new(320, 240);
    eng_sw.mount_root(TestPanel { active: true });
    let mut sw = SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::SwappedBuffers);
    assert_eq!(sw.name(), "software");
    assert_eq!(sw.repaint_buffer_type(), RepaintBufferType::SwappedBuffers);
    let (rects, px) = sw.render_frame_dt(&mut eng_sw, 16);
    assert_eq!(rects, 1);
    assert_eq!(px, 320 * 240);

    let mut strip = [Rgb565(0); 80 * 40];
    sw.render_region_to_buffer(
        eng_sw.cached_draw_ops(),
        Region::new(10, 10, 80, 40),
        10,
        10,
        80,
        &mut strip,
    );
    assert!(strip.iter().any(|p| p.0 != 0));

    // 2. 跨后端统一降维管道 lower_draw_ops + LoweredPrimitiveSink（验证直角/圆角、边框、文本降维）
    #[derive(Default)]
    struct CountingSink {
        fill_rect_count: usize,
        stroke_rect_count: usize,
        text_run_count: usize,
        sharp_corner_rect_count: usize,
    }
    impl LoweredPrimitiveSink for CountingSink {
        fn begin_dirty_clip(&mut self, _scaled_dirty: Region) {}
        fn end_dirty_clip(&mut self) {}
        fn fill_rect(
            &mut self,
            _rect: Region,
            corner_radius: u8,
            _start_color: Color,
            _end_color: Color,
            _horizontal_grad: bool,
        ) {
            self.fill_rect_count += 1;
            if corner_radius == 0 {
                self.sharp_corner_rect_count += 1;
            }
        }
        fn stroke_rect(&mut self, _rect: Region, _corner_radius: u8, _stroke_width: u8, _color: Color) {
            self.stroke_rect_count += 1;
        }
        fn draw_text(
            &mut self,
            _bounds: Region,
            _clip: Region,
            _text: &str,
            _color: Color,
            _font_size: u8,
            _text_flags: u8,
        ) {
            self.text_run_count += 1;
        }
    }

    let mut sink = CountingSink::default();
    let dirty = [Region::new(0, 0, 320, 240)];
    let (l_rects, l_px) = lower_draw_ops(eng_sw.cached_draw_ops(), 1, &dirty, &mut sink);
    assert_eq!(l_rects, 1);
    assert_eq!(l_px, 320 * 240);
    assert!(sink.fill_rect_count >= 2);
    assert!(sink.stroke_rect_count >= 1);
    assert!(sink.text_run_count >= 1);
    assert!(sink.sharp_corner_rect_count >= 1);
}

#[test]
fn test_mobile_and_web_wasm_platforms() {
    struct MobileBackApp {
        tapped: bool,
        went_back: bool,
        last_safe_top: i16,
    }

    impl View for MobileBackApp {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            self.last_safe_top = cx.safe_area().top;
            let on_tap = cx.listener(|this, _: ClickEvent, acx| {
                this.tapped = true;
                acx.notify();
            });
            let on_swipe = cx.listener(|this, dir: SwipeDir, acx| {
                if matches!(dir, SwipeDir::Right) {
                    this.went_back = true;
                    acx.notify();
                }
            });

            div()
                .size_full()
                .safe_area(cx)
                .on_swipe(cx, on_swipe)
                .child(
                    cx,
                    div()
                        .id(1)
                        .w(120)
                        .h(48)
                        .bg(Color::rgb(255, 64, 32))
                        .on_click(cx, on_tap),
                )
                .into_element(cx)
        }
    }

    // 1. 验证 MobilePlatform (iOS / Android 触控状态机、安全区同步与左边缘右滑返回)
    let mut mobile_eng = VeloxEngine::new(200, 300);
    let m_root = mobile_eng.mount_root(MobileBackApp {
        tapped: false,
        went_back: false,
        last_safe_top: 0,
    });
    let mut mobile_plat: MobilePlatform<200, 300> = MobilePlatform::new();
    assert_eq!(
        mobile_plat.step_platform(&mut mobile_eng, 16),
        PlatformControlFlow::Poll
    );
    assert_eq!(mobile_eng.view(m_root).unwrap().last_safe_top, 44);

    // 验证多指触控过滤：手指 1 按下后，手指 2 的干扰事件被忽略
    mobile_plat.on_touch(1, TouchPhase::Began, 30, 60);
    mobile_plat.on_touch(2, TouchPhase::Began, 180, 280);
    mobile_plat.on_touch(2, TouchPhase::Ended, 180, 280);
    mobile_plat.on_touch(1, TouchPhase::Ended, 30, 60);
    let _ = mobile_plat.step_platform(&mut mobile_eng, 16);
    assert!(mobile_eng.view(m_root).unwrap().tapped);

    // 验证横屏切换自动更新 SafeAreaInsets
    mobile_plat.set_orientation(DeviceOrientation::LandscapeLeft);
    let _ = mobile_plat.step_platform(&mut mobile_eng, 16);
    assert_eq!(mobile_eng.safe_area().left, 44);
    assert_eq!(mobile_eng.safe_area().top, 0);

    // 验证系统返回导航 handle_back
    assert!(mobile_plat.handle_back(&mut mobile_eng));
    assert!(mobile_eng.view(m_root).unwrap().went_back);

    // 2. 验证 WebPlatform (WebAssembly RGBA8888 字节序与脏矩形局部上报)
    let mut web_eng = VeloxEngine::new(200, 150);
    let w_root = web_eng.mount_root(ButtonDemo { toggled: false });
    let mut web_plat: WebPlatform<200, 150> = WebPlatform::new();
    assert_eq!(
        web_plat.step_platform(&mut web_eng, 16),
        PlatformControlFlow::Poll
    );
    assert_eq!(web_plat.dirty_rects().len(), 1);
    assert_eq!(web_plat.rgba_byte_len(), 200 * 150 * 4);

    web_plat.on_pointer_down(30, 30);
    web_plat.on_pointer_up(30, 30);
    let _ = web_plat.step_platform(&mut web_eng, 16);
    assert!(web_eng.view(w_root).unwrap().toggled);
    assert_eq!(web_plat.dirty_rects().len(), 1);
    assert_eq!(web_plat.dirty_rects()[0].w, 80);
    assert_eq!(web_plat.dirty_rects()[0].h, 40);

    {
        let mut embed_app = MobileEmbedApp::new(240, 400, SafeAreaInsets::phone_portrait());
        assert_eq!(embed_app.backend(), DesktopRenderBackend::default_desktop());
        let eroot = embed_app.engine.mount_root(ButtonDemo { toggled: false });
        assert_eq!(embed_app.step_frame(16), 1);
        embed_app.push_touch(7, TouchPhase::Began, 30, 30);
        embed_app.push_touch(7, TouchPhase::Ended, 30, 30);
        assert_eq!(embed_app.step_frame(16), 1);
        assert!(embed_app.engine.view(eroot).unwrap().toggled);

        let mut web_app = WebCanvasApp::new(240, 180);
        let wroot2 = web_app.engine.mount_root(ButtonDemo { toggled: false });
        assert_eq!(web_app.step_frame(16), 1);
        web_app.resize(320, 240);
        assert_eq!(web_app.step_frame(16), 1);
        assert!(!web_app.engine.view(wroot2).unwrap().toggled);
    }
}

#[test]
fn test_shm_surface_driver_odd_physical_resize_no_scanline_shear() {
    struct VerticalStripeView;
    impl View for VerticalStripeView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_row()
                .bg(Color::rgb(10, 20, 30))
                .child(cx, div().w(40).h_full().bg(Color::rgb(255, 0, 0)))
                .child(cx, div().flex_grow(1).h_full().bg(Color::rgb(0, 0, 255)))
                .into_element(cx)
        }
    }

    // 情况 1：ceil 逻辑尺寸 (641.div_ceil(2) = 321, 481.div_ceil(2) = 241)，即 viewport * scale (642) > dst_stride (641)
    let phys_w = 641usize;
    let phys_h = 481usize;
    let logical_w = phys_w.div_ceil(2) as i16;
    let logical_h = phys_h.div_ceil(2) as i16;

    let mut engine = VeloxEngine::new(320, 240);
    engine.mount_root(VerticalStripeView);
    engine.set_dpi_scale(2);
    engine.resize(logical_w, logical_h);

    let mut driver = WaylandSurfaceDriver::new(phys_w, phys_h);
    let mut wl = WlSurface::new();
    assert!(driver.flush_frame_dt(&mut engine, &mut wl, 16));
    assert_eq!(driver.width(), phys_w);
    assert_eq!(driver.height(), phys_h);

    let px = driver.front_pixels();
    let red = Xrgb8888::from_color(Color::rgb(255, 0, 0));
    let blue = Xrgb8888::from_color(Color::rgb(0, 0, 255));
    for y in [10usize, 200, 400, 480] {
        // 红色竖条（逻辑 x=0..40 => 物理 x=0..80）在所有扫描行必须垂直对齐、零斜切
        assert_eq!(px[y * phys_w + 20], red, "row {y} x=20 must stay red");
        assert_eq!(px[y * phys_w + 120], blue, "row {y} x=120 must stay blue");
        // 右侧奇数列 x=640 也应被蓝色背景完全覆盖，无 1px 未绘制黑边
        assert_eq!(px[y * phys_w + 640], blue, "row {y} rightmost col must be filled");
    }

    // 情况 2：非整除宽度下 viewport * scale (640) < dst_stride (641) 时，同样不得发生 45° 斜切错行
    engine.resize(320, 240);
    let mut wl2 = WlSurface::new();
    driver.resize(phys_w, phys_h);
    assert!(driver.flush_frame_dt(&mut engine, &mut wl2, 16));
    let px2 = driver.front_pixels();
    for y in [10usize, 200, 400] {
        assert_eq!(px2[y * phys_w + 20], red, "case2 row {y} x=20 must stay red");
        assert_eq!(px2[y * phys_w + 120], blue, "case2 row {y} x=120 must stay blue");
    }
}

#[test]
fn test_winit_single_slot_reuse_and_lazy_allocation() {
    let mut engine = VeloxEngine::new(320, 240);
    engine.mount_root(ButtonDemo { toggled: false });

    let mut winit_plat = WinitPlatform::new("Single Slot Reuse", 320, 240).with_dpi_scale(1);
    // 构造阶段尚未实际刷新前，双缓冲槽位均不应预先分配全屏像素数组
    assert!(winit_plat.driver().slots[0].pixels.is_empty());
    assert!(winit_plat.driver().slots[1].pixels.is_empty());

    // 第 1 帧：首帧全屏提交到 slots[0]
    let r1 = winit_plat.flush_frame_dt(&mut engine, 16);
    assert!(r1.has_updates());
    assert_eq!(winit_plat.driver().slots[0].pixels.len(), 320 * 240);
    assert!(winit_plat.driver().slots[1].pixels.is_empty());
    assert_eq!(
        winit_plat.driver().last_raster_regions,
        vec![Region::new(0, 0, 320, 240)]
    );

    // 第 2 帧：点击按钮后原位复用 slots[0]，不分配 slots[1]，且仅光栅化按钮局部脏矩形
    assert!(engine.dispatch_click(30, 30));
    let r2 = winit_plat.flush_frame_dt(&mut engine, 16);
    assert!(r2.has_updates());
    assert!(winit_plat.driver().slots[1].pixels.is_empty());
    assert_eq!(winit_plat.driver().slots[0].age, 1);
    assert_eq!(
        winit_plat.driver().last_raster_regions,
        vec![Region::new(20, 20, 80, 40)]
    );
}

#[test]
fn test_ttf_outline_rasterizer_ascii_and_cjk_non_blank() {
    struct TextSampleView {
        label: &'static str,
    }
    impl View for TextSampleView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .bg(Color::BLACK)
                .items_center()
                .justify_center()
                .child(cx, text(self.label).size(18).color(Color::WHITE))
                .into_element(cx)
        }
    }

    for sample in ["Velox Studio 123", "桌面性能测试"] {
        let mut engine = VeloxEngine::new(240, 80);
        engine.mount_root(TextSampleView { label: sample });
        let mut winit_plat = WinitPlatform::new("Text Render", 240, 80).with_dpi_scale(1);
        let rep = winit_plat.flush_frame_dt(&mut engine, 16);
        assert!(rep.has_updates());
        let lit_pixels = winit_plat
            .driver()
            .front_pixels()
            .iter()
            .filter(|px| (px.0 & 0x00FF_FFFF) != 0)
            .count();
        assert!(
            lit_pixels > 20,
            "Expected non-blank rasterized pixels for '{sample}', got {lit_pixels}"
        );
    }
}
