//! Velox WebAssembly (`<canvas>`) 应用示例可执行入口。
//!
//! 支持多种运行模式：
//! - 默认 / `--headless`：验证 WebAssembly `<canvas>` `Uint8ClampedArray` 零拷贝视图与脏矩形管线；
//! - `--emit-html <path>`：生成独立自包含的 HTML5 `<canvas>` 宿主页面文件；
//! - `--window`（需 `--features winit`）：启动 `800x600` 桌面预览窗口交互运行 `WebDashboardApp`；
//! - `--save-png <path>`（需 `--features winit`）：导出 `800x600` Web 界面 PNG 渲染快照。

use velox_kit::prelude::InputEvent;
use web_app::{create_web_app, generate_index_html};
#[cfg(feature = "winit")]
use velox_kit::prelude::{Platform, VeloxEngine};
#[cfg(feature = "winit")]
use web_app::WebDashboardApp;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--emit-html") {
        if let Some(out_path) = args.get(pos + 1) {
            let html = generate_index_html("./web_app.wasm", 800, 600);
            std::fs::write(out_path, html).expect("Failed to write HTML file");
            println!("[Web/WASM] 已导出 HTML5 <canvas> 引导页至: {out_path}");
            return;
        }
    }

    #[cfg(feature = "winit")]
    {
        if let Some(pos) = args.iter().position(|a| a == "--save-png") {
            if let Some(path) = args.get(pos + 1) {
                let mut engine = VeloxEngine::new(800, 600);
                engine.mount_root(WebDashboardApp::new());
                let runner = velox_kit::platform::winit::WinitPlatform::new(
                    "Velox Web App (800x600)",
                    800,
                    600,
                );
                runner
                    .render_to_png(&mut engine, path)
                    .expect("Failed to write Web PNG snapshot");
                println!("[Web/WASM] 已导出 800x600 Web 界面 PNG 快照至: {path}");
                return;
            }
        }

        if args.iter().any(|a| a == "--window") {
            let mut engine = VeloxEngine::new(800, 600);
            engine.mount_root(WebDashboardApp::new());
            println!("[Web/WASM] 正在启动 800x600 Web 应用交互预览窗口...");
            velox_kit::platform::winit::WinitPlatform::new(
                "Velox WebAssembly Studio (800x600 Canvas Preview)",
                800,
                600,
            )
            .run_event_loop(engine);
            return;
        }
    }

    println!("=== Velox WebAssembly Application (HTML5 <canvas> ImageData) ===");
    let (mut web, root) = create_web_app(800, 600);

    // 1. 首帧整屏渲染
    let dirty_f1 = web.step_frame(16);
    println!(
        "[Web/WASM] 首帧渲染完成: 脏矩形数 = {}, Uint8ClampedArray 视图长度 = {} bytes",
        dirty_f1,
        web.rgba_bytes().len()
    );

    // 2. 模拟浏览器 PointerEvent 点击底部 TabBar 第 2 项 "Analytics" (x=400, y=575)
    web.engine
        .dispatch_event(InputEvent::PointerDown { x: 400, y: 575 });
    web.engine
        .dispatch_event(InputEvent::PointerUp { x: 400, y: 575 });
    let dirty_f2 = web.step_frame(16);
    println!(
        "[Web/WASM] 切换至 {:?} 二级页: 脏矩形数 = {}",
        web.engine.view(root).unwrap().nav.current(),
        dirty_f2
    );

    // 3. 模拟浏览器 History popstate 后退事件
    let consumed = web.engine.dispatch_navigate_back();
    let dirty_f3 = web.step_frame(16);
    println!(
        "[Web/WASM] 触发浏览器 popstate 后退: 消费 = {}, 当前路由 = {:?}, 脏矩形数 = {}",
        consumed,
        web.engine.view(root).unwrap().nav.current(),
        dirty_f3
    );

    let html = generate_index_html("./web_app.wasm", 800, 600);
    println!(
        "[Web/WASM] HTML5 <canvas> 宿主页面就绪 ({} bytes)",
        html.len()
    );
}
