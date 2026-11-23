//! Velox iOS 移动应用示例可执行入口。
//!
//! 支持多种运行模式：
//! - 默认 / `--headless`：在宿主环境中验证 iOS `UIKit` / `CADisplayLink` 生命周期、灵动岛 `59pt` + Home Indicator `34pt` 安全区、`UIKeyboard` 避让与左边缘返回；
//! - `--window`（需 `--features winit`）：启动 `393x852` (iPhone 15 Pro 逻辑点尺寸) 手机竖屏模拟器窗口进行交互体验；
//! - `--save-png <path>`（需 `--features winit`）：导出 `393x852` iPhone 手机界面 PNG 渲染快照。

use ios_app::IosHostBridge;
#[cfg(feature = "winit")]
use ios_app::IosMobileApp;
#[cfg(feature = "winit")]
use velox_kit::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--emit-host") {
        if let Some(dir) = args.get(pos + 1) {
            velox_kit::platform::ios::emit_ios_host_project(
                std::path::Path::new(dir),
                "com.velox.ios.app",
                "Velox iOS",
            )
            .expect("Failed to emit iOS host glue files");
            println!("[iOS] 已由框架自动生成 iOS 宿主工程文件至: {dir}");
            return;
        }
    }

    #[cfg(feature = "winit")]
    {
        if let Some(pos) = args.iter().position(|a| a == "--save-png") {
            if let Some(path) = args.get(pos + 1) {
                let mut engine = VeloxEngine::new(393, 852);
                engine.set_safe_area(SafeAreaInsets::ios_portrait());
                engine.mount_root(IosMobileApp::new());
                let runner = velox_kit::platform::winit::WinitPlatform::new(
                    "Velox iOS App (393x852)",
                    393,
                    852,
                );
                runner
                    .render_to_png(&mut engine, path)
                    .expect("Failed to write iOS PNG snapshot");
                println!("[iOS] 已导出 393x852 iPhone 界面 PNG 快照至: {path}");
                return;
            }
        }

        if args.iter().any(|a| a == "--window") {
            let mut engine = VeloxEngine::new(393, 852);
            engine.set_safe_area(SafeAreaInsets::ios_portrait());
            engine.mount_root(IosMobileApp::new());
            println!("[iOS] 正在启动 393x852 iPhone 竖屏交互窗口（支持左边缘右滑返回）...");
            velox_kit::platform::winit::WinitPlatform::new(
                "Velox iOS Simulator (393x852, DynamicIsland=59pt, HomeIndicator=34pt)",
                393,
                852,
            )
            .run_event_loop(engine);
            return;
        }
    }

    let _ = &args;
    println!("=== Velox iOS Application (UIKit / CADisplayLink / CAMetalLayer) ===");
    let mut bridge = IosHostBridge::new(393, 852, 59, 34);

    // 1. 首帧整屏渲染（包含顶部 59pt 灵动岛与底部 34pt Home Indicator 安全区）
    let dirty_f1 = bridge.on_display_link(8);
    println!(
        "[iOS] 首帧渲染完成: 脏矩形数 = {}, RGBA8888 缓冲区 = {} bytes, 安全区 = {:?}",
        dirty_f1,
        bridge.rgba_slice().len(),
        bridge.embed_app().engine.safe_area()
    );

    // 2. 模拟点击右上角 "Sheet" 按钮 (x=358, y=80) 打开底部 Action Sheet
    bridge.on_ui_touch(1, IosHostBridge::UITOUCH_PHASE_BEGAN, 358, 80);
    bridge.on_ui_touch(1, IosHostBridge::UITOUCH_PHASE_ENDED, 358, 80);
    let dirty_f2 = bridge.on_display_link(8);
    println!(
        "[iOS] 打开 Action Sheet 抽屉: 脏矩形数 = {}, sheet_open = {}",
        dirty_f2,
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 3. 模拟 UIKeyboardWillChangeFrameNotification 软键盘弹出 (291pt)
    bridge.on_keyboard_frame_changed(291);
    let dirty_f3 = bridge.on_display_link(8);
    println!(
        "[iOS] UIKeyboard 弹出 (291pt) 自动抬升抽屉: 脏矩形数 = {}, keyboard_inset = {}pt",
        dirty_f3,
        bridge.embed_app().engine.keyboard_inset()
    );

    // 4. 模拟触发 iOS 左边缘返回手势关闭 Action Sheet
    let consumed = bridge.on_interactive_pop_gesture();
    let _ = bridge.on_display_link(8);
    println!(
        "[iOS] 触发左边缘返回手势: 内部消费 = {}, sheet_open = {}",
        consumed,
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );
}
