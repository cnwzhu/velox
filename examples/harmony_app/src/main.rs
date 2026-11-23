//! Velox 鸿蒙 HarmonyOS NEXT 移动应用示例可执行入口。
//!
//! 支持多种运行模式：
//! - 默认 / `--headless`：在宿主环境中验证鸿蒙 `OH_NativeXComponent` 生命周期、`AvoidArea` (`38vp` + `28vp`) 避让、`TYPE_KEYBOARD` 软键盘避让与 `onBackPress` 返回拦截；
//! - `--window`（需 `--features winit`）：启动 `384x832` 鸿蒙手机竖屏模拟器窗口进行交互体验；
//! - `--save-png <path>`（需 `--features winit`）：导出 `384x832` 鸿蒙手机界面 PNG 渲染快照。

use harmony_app::HarmonyHostBridge;
#[cfg(feature = "winit")]
use harmony_app::HarmonyMobileApp;
#[cfg(feature = "winit")]
use velox_kit::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--emit-host") {
        if let Some(dir) = args.get(pos + 1) {
            velox_kit::platform::harmony::emit_harmony_host_project(
                std::path::Path::new(dir),
                "harmony_app",
            )
            .expect("Failed to emit HarmonyOS host glue files");
            println!("[HarmonyOS] 已由框架自动生成鸿蒙 Stage 宿主工程文件至: {dir}");
            return;
        }
    }

    #[cfg(feature = "winit")]
    {
        if let Some(pos) = args.iter().position(|a| a == "--save-png") {
            if let Some(path) = args.get(pos + 1) {
                let mut engine = VeloxEngine::new(384, 832);
                engine.set_safe_area(SafeAreaInsets::harmony_portrait());
                engine.mount_root(HarmonyMobileApp::new());
                let runner = velox_kit::platform::winit::WinitPlatform::new(
                    "Velox HarmonyOS NEXT App (384x832)",
                    384,
                    832,
                );
                runner
                    .render_to_png(&mut engine, path)
                    .expect("Failed to write HarmonyOS PNG snapshot");
                println!("[HarmonyOS] 已导出 384x832 鸿蒙手机界面 PNG 快照至: {path}");
                return;
            }
        }

        if args.iter().any(|a| a == "--window") {
            let mut engine = VeloxEngine::new(384, 832);
            engine.set_safe_area(SafeAreaInsets::harmony_portrait());
            engine.mount_root(HarmonyMobileApp::new());
            println!("[HarmonyOS] 正在启动 384x832 鸿蒙竖屏交互窗口（支持左边缘右滑返回）...");
            velox_kit::platform::winit::WinitPlatform::new(
                "Velox HarmonyOS NEXT Simulator (384x832, AvoidTop=38vp, NavIndicator=28vp)",
                384,
                832,
            )
            .run_event_loop(engine);
            return;
        }
    }

    let _ = &args;
    println!("=== Velox HarmonyOS NEXT Application (ArkTS XComponent / OH_NativeXComponent) ===");
    let mut bridge = HarmonyHostBridge::new(384, 832, 38, 28);

    // 1. 首帧整屏渲染（包含顶部 38vp TYPE_SYSTEM 状态栏与底部 28vp TYPE_NAVIGATION_INDICATOR 避让区）
    let dirty_f1 = bridge.on_frame_callback(8);
    println!(
        "[HarmonyOS] 首帧渲染完成: 脏矩形数 = {}, RGBA8888 缓冲区 = {} bytes, 避让区 = {:?}",
        dirty_f1,
        bridge.rgba_slice().len(),
        bridge.embed_app().engine.safe_area()
    );

    // 2. 模拟点击右上角 "Sheet" 按钮 (x=350, y=60) 打开底部元服务卡片抽屉
    bridge.on_touch_event(0, HarmonyHostBridge::OH_TOUCH_DOWN, 350, 60);
    bridge.on_touch_event(0, HarmonyHostBridge::OH_TOUCH_UP, 350, 60);
    let dirty_f2 = bridge.on_frame_callback(8);
    println!(
        "[HarmonyOS] 打开元服务卡片抽屉: 脏矩形数 = {}, sheet_open = {}",
        dirty_f2,
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 3. 模拟 AvoidAreaType.TYPE_KEYBOARD 软键盘弹出 (276vp)
    bridge.on_keyboard_avoid_area(276);
    let dirty_f3 = bridge.on_frame_callback(8);
    println!(
        "[HarmonyOS] TYPE_KEYBOARD 软键盘弹出 (276vp) 自动抬升抽屉: 脏矩形数 = {}, keyboard_inset = {}vp",
        dirty_f3,
        bridge.embed_app().engine.keyboard_inset()
    );

    // 4. 模拟触发鸿蒙 ArkTS onBackPress() 关闭元服务抽屉
    let consumed = bridge.on_back_press();
    let _ = bridge.on_frame_callback(8);
    println!(
        "[HarmonyOS] 触发 onBackPress(): 内部消费 = {}, sheet_open = {}",
        consumed,
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );
}
