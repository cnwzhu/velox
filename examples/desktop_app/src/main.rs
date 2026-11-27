//! Velox 跨平台桌面组件画廊示例可执行入口（`desktop_app` — **Velox Base Gallery**）。
//!
//! 支持多种运行模式：
//! - 默认 / `--window`：启动原生桌面画廊窗口（Windows / macOS / Linux Pure Wayland，默认启用 `wgpu` 2D SDF GPU 硬件加速）；
//! - `--software`：运行时切换至内置软件光栅渲染器（`DesktopRenderBackend::Software`）；
//! - `--light`：以亮色主题（`ThemeMode::Light`）启动画廊；
//! - `--story <name>`：直接打开指定故事页（`overview` / `buttons` / `selection` / `sliders` / `inputs` / `navigation` / `editor` / `overlays` / `feedback` / `data` / `icons` / `effects` / `motion` / `engine` / `chat`）；
//! - `--lang <en|zh|ja>`：指定初始显示语言；
//! - `--scale <1|2>`：设置 HiDPI 缩放倍率（默认 `2`）；
//! - `--save-png <path>`：渲染首帧并导出无损 PNG 画廊快照；
//! - `--native-scheduler`：切换回 Velox 内置零外部依赖调度器（默认接入 [`TokioScheduler`]）；
//! - `--http-url <url>`：指定 `"HTTP GET"` 按钮的测试目标 URL（默认 `https://httpbin.org/get`）；
//! - `--async-demo`：运行 Tokio 调度器多阶段异步同步 + 本地回环 `reqwest` HTTP GET 端到端演示并退出；
//! - `--headless`：在无显示服务器环境下验证 `WinitPlatform`、`WaylandPlatform`、`AsyncPlatform` 与 `Tokio + reqwest` 闭环。

use desktop_app::{
    block_on, mount_desktop_app, run_async_platform_demo, run_tokio_http_demo, GalleryApp,
    GalleryStory, DEFAULT_DESKTOP_HEIGHT, DEFAULT_DESKTOP_WIDTH,
};
use velox_kit::platform::wayland::WaylandPlatform;
use velox_kit::platform::winit::WinitPlatform;
use velox_kit::platform::{
    DesktopRenderBackend, Platform, PlatformControlFlow, PlatformSurface,
};
use velox_kit::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let light_mode = args.iter().any(|a| a == "--light");
    let headless = args.iter().any(|a| a == "--headless");
    let async_demo = args.iter().any(|a| a == "--async-demo");
    let use_native_scheduler = args.iter().any(|a| a == "--native-scheduler");
    let use_software = args.iter().any(|a| a == "--software");
    let explicit_http_url = args
        .iter()
        .position(|a| a == "--http-url")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let backend = if use_software {
        DesktopRenderBackend::Software
    } else {
        DesktopRenderBackend::default_desktop()
    };
    let explicit_scale = args
        .iter()
        .position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .map(|s| s.clamp(1, 4));
    let headless_scale = explicit_scale.unwrap_or(2);

    let explicit_locale = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| Locale::parse(s));

    let explicit_story = args
        .iter()
        .position(|a| a == "--story")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| GalleryStory::parse_cli(s));

    let theme_mode = if light_mode {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };

    if async_demo {
        let report = block_on(run_async_platform_demo(Duration::from_millis(60)));
        println!(
            "[Gallery/AsyncPlatform] frames_polled={}, completed_runs={}, root_renders={}, final_flow={:?}",
            report.frames_polled,
            report.completed_runs,
            report.root_render_count,
            report.final_control_flow
        );
        let http_report = block_on(run_tokio_http_demo());
        println!(
            "[Gallery/Tokio+Reqwest] scheduler={}, status={}, bytes={}, elapsed_ms={}, root_renders={}, final_flow={:?}, body={:?}",
            http_report.scheduler_name,
            http_report.status_code,
            http_report.bytes,
            http_report.elapsed_ms,
            http_report.root_render_count,
            http_report.final_control_flow,
            http_report.preview
        );
        return;
    }

    let mut app_state = GalleryApp::new().with_theme_mode(theme_mode);
    if let Some(loc) = explicit_locale {
        app_state = app_state.with_locale(loc);
    }
    if let Some(story) = explicit_story {
        app_state = app_state.with_story(story);
    }

    if let Some(pos) = args.iter().position(|a| a == "--save-png") {
        if let Some(path) = args.get(pos + 1) {
            let mut engine = VeloxEngine::new(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
            if !use_native_scheduler {
                engine.use_tokio();
            }
            engine.set_theme(theme_mode.to_theme());
            if let Some(loc) = explicit_locale {
                engine.set_locale(loc);
            }
            let _ = mount_desktop_app(&mut engine, app_state);
            // 先步进入场动效（最多 48 帧 ≈ 768ms，提前收敛即停），确保图表/弹簧卡片等展示完整终态
            let _ = engine.step_frame_dt(16);
            for _ in 0..48 {
                if !engine.needs_redraw() {
                    break;
                }
                let _ = engine.step_frame_dt(16);
            }
            let runner = WinitPlatform::new(
                "Velox Base Gallery — Component Showcase",
                DEFAULT_DESKTOP_WIDTH as usize,
                DEFAULT_DESKTOP_HEIGHT as usize,
            )
            .with_backend(backend)
            .with_dpi_scale(headless_scale);
            runner
                .render_to_png(&mut engine, path)
                .expect("Failed to export gallery PNG snapshot");
            println!(
                "[Gallery] 已导出 {}x{} ({}x HiDPI) 组件画廊 PNG 快照至: {path}",
                DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT, headless_scale
            );
            return;
        }
    }

    if headless {
        let mut engine = VeloxEngine::new(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
        if !use_native_scheduler {
            engine.use_tokio();
        }
        engine.set_theme(theme_mode.to_theme());
        if let Some(loc) = explicit_locale {
            engine.set_locale(loc);
        }
        let root = mount_desktop_app(&mut engine, app_state);
        println!(
            "=== Velox Base Gallery Headless Verification ({}x{}, scale={}x) ===",
            DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT, headless_scale
        );
        let mut winit_plat = WinitPlatform::new(
            "Velox Base Gallery (Headless)",
            DEFAULT_DESKTOP_WIDTH as usize,
            DEFAULT_DESKTOP_HEIGHT as usize,
        )
        .with_dpi_scale(headless_scale);

        let r1 = winit_plat.flush_frame_dt(&mut engine, 16);
        assert!(r1.has_updates(), "Frame 1 must render initial gallery frame");

        // 点击顶部栏 "Toast" 按钮弹出右上角 Notification（1280px 宽窗口右侧按钮区坐标）
        let clicked = engine.dispatch_click(1044, 22);
        assert!(clicked, "Expected click at (1044, 22) to trigger Gallery Toast");
        let r2 = winit_plat.flush_frame_dt(&mut engine, 16);
        assert!(r2.has_updates(), "Frame 2 must repaint notification dirty region");
        assert!(engine.view(root).unwrap().notification_open);

        // 步进 Notification 的 210ms 物理弹簧入场动画直至完全收敛，验证随后立即归零进入 0% CPU 休眠
        let mut r_idle = winit_plat.flush_frame_dt(&mut engine, 16);
        for _ in 0..24 {
            if r_idle.dirty_regions == 0 {
                break;
            }
            r_idle = winit_plat.flush_frame_dt(&mut engine, 16);
        }
        assert_eq!(
            r_idle.dirty_regions, 0,
            "Notification spring enter animation must settle to 0 dirty regions"
        );

        println!(
            "[Gallery/Winit] Frame 1: dirty={}, pixels={} | Frame 2 (Toast): dirty={}, pixels={} | Settled Idle: dirty={}",
            r1.dirty_regions, r1.pixels_updated, r2.dirty_regions, r2.pixels_updated, r_idle.dirty_regions
        );

        {
            let mut wl_engine = VeloxEngine::new(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
            wl_engine.mount_root(GalleryApp::new().with_theme_mode(theme_mode));
            let mut wl_plat = WaylandPlatform::new_xdg_toplevel(
                "Velox Base Gallery",
                "dev.velox.gallery",
                DEFAULT_DESKTOP_WIDTH as usize,
                DEFAULT_DESKTOP_HEIGHT as usize,
            )
            .with_backend(backend);
            let f1 = wl_plat.step_platform(&mut wl_engine, 16);
            let f2 = wl_plat.step_platform(&mut wl_engine, 16);
            assert_eq!(f1, PlatformControlFlow::Poll);
            assert_eq!(f2, PlatformControlFlow::Wait);
            println!(
                "[Gallery/Wayland] xdg_toplevel surface {:?}: Frame 1={:?}, Frame 2={:?} (0% CPU Wait)",
                wl_plat.surface_size(),
                f1,
                f2
            );
        }

        {
            let async_report = block_on(run_async_platform_demo(Duration::from_millis(20)));
            assert_eq!(async_report.completed_runs, 1);
            assert_eq!(async_report.final_control_flow, PlatformControlFlow::Wait);
            println!(
                "[Gallery/AsyncPlatform] frames_polled={}, completed_runs={}, root_renders={}, final_flow={:?}",
                async_report.frames_polled,
                async_report.completed_runs,
                async_report.root_render_count,
                async_report.final_control_flow
            );

            let http_report = block_on(run_tokio_http_demo());
            assert_eq!(http_report.scheduler_name, "tokio");
            assert_eq!(http_report.status_code, 200);
            assert_eq!(http_report.root_render_count, 1);
            assert_eq!(http_report.final_control_flow, PlatformControlFlow::Wait);
            println!(
                "[Gallery/Tokio+Reqwest] scheduler={}, status={}, bytes={}, elapsed_ms={}, root_renders={}, final_flow={:?}",
                http_report.scheduler_name,
                http_report.status_code,
                http_report.bytes,
                http_report.elapsed_ms,
                http_report.root_render_count,
                http_report.final_control_flow
            );
        }
        return;
    }

    // 精简桌面应用启动（参照 gpui-kit application().run(|cx| open_window(...))）
    let mut app = velox_kit::application()
        .with_theme(theme_mode.to_theme())
        .with_backend(backend);
    if !use_native_scheduler {
        app = app.with_tokio();
    }
    if let Some(loc) = explicit_locale {
        app = app.with_locale(loc);
    }
    if let Some(s) = explicit_scale {
        app = app.with_dpi_scale(s);
    }

    app.run(move |cx| {
        velox_kit::init(cx);
        println!(
            "[Gallery] 正在启动 Velox Base Gallery ({}x{}, scale={}, theme={:?}, backend={:?}, scheduler={})...",
            DEFAULT_DESKTOP_WIDTH,
            DEFAULT_DESKTOP_HEIGHT,
            explicit_scale
                .map(|s| format!("{s}x"))
                .unwrap_or_else(|| "auto".to_string()),
            theme_mode,
            backend,
            cx.scheduler_name()
        );
        let opts = WindowOptions::new("Velox Base Gallery — Component Showcase")
            .size(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
        velox_kit::open_window(opts, cx, move |wcx| {
            let root = mount_desktop_app(wcx.engine_mut(), app_state);
            if let Some(url) = explicit_http_url {
                if let Some(async_h) = wcx.engine().view(root).and_then(|v| v.async_task_view) {
                    if let Some(v) = wcx.engine_mut().view_mut(async_h) {
                        v.set_http_url(url);
                    }
                }
            }
            root
        })
        .expect("Failed to open Velox Base Gallery window");
    });
}
