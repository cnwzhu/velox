//! Velox Android 移动应用示例可执行入口。
//!
//! 支持多种运行模式：
//! - 默认 / `--headless`：在宿主环境中验证 Android JNI/NDK `ANativeWindow` 生命周期、安全区、软键盘避让与物理返回键；
//! - `--window`（需 `--features winit`）：启动 `390x844` 手机竖屏模拟器窗口进行交互体验；
//! - `--save-png <path>`（需 `--features winit`）：导出 `390x844` 手机界面 PNG 渲染快照。

use android_app::AndroidHostBridge;
#[cfg(feature = "winit")]
use android_app::AndroidMobileApp;
#[cfg(feature = "winit")]
use velox_kit::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--emit-host") {
        if let Some(dir) = args.get(pos + 1) {
            velox_kit::platform::android::emit_android_host_project(
                std::path::Path::new(dir),
                "com.velox.android",
                "android_app",
                "Velox Android",
            )
            .expect("Failed to emit Android host glue files");
            println!("[Android] 已由框架自动生成 Android 宿主工程文件至: {dir}");
            return;
        }
    }

    #[cfg(feature = "winit")]
    {
        if let Some(pos) = args.iter().position(|a| a == "--save-png") {
            if let Some(path) = args.get(pos + 1) {
                let mut engine = VeloxEngine::new(390, 844);
                engine.set_safe_area(SafeAreaInsets::new(48, 0, 34, 0));
                engine.mount_root(AndroidMobileApp::new());
                let runner = velox_kit::platform::winit::WinitPlatform::new(
                    "Velox Android App (390x844)",
                    390,
                    844,
                );
                runner
                    .render_to_png(&mut engine, path)
                    .expect("Failed to write Android PNG snapshot");
                println!("[Android] 已导出 390x844 手机界面 PNG 快照至: {path}");
                return;
            }
        }

        if args.iter().any(|a| a == "--window") {
            let mut engine = VeloxEngine::new(390, 844);
            engine.set_safe_area(SafeAreaInsets::new(48, 0, 34, 0));
            engine.mount_root(AndroidMobileApp::new());
            println!("[Android] 正在启动 390x844 手机竖屏交互窗口（支持左边缘右滑返回）...");
            velox_kit::platform::winit::WinitPlatform::new(
                "Velox Android Simulator (390x844, Notch=48px, GestureBar=34px)",
                390,
                844,
            )
            .run_event_loop(engine);
            return;
        }
    }

    let _ = &args;
    println!("=== Velox Android Application (JNI / NDK ANativeWindow) ===");
    let mut bridge = AndroidHostBridge::new(390, 844, 48, 34);

    // 1. 首帧整屏渲染（包含顶部 48px 挖孔状态栏与底部 34px 手势导航条安全区）
    let dirty_f1 = bridge.do_frame(16);
    println!(
        "[Android] 首帧渲染完成: 脏矩形数 = {}, RGBA8888 缓冲区 = {} bytes, 安全区 = {:?}",
        dirty_f1,
        bridge.rgba_slice().len(),
        bridge.embed_app().engine.safe_area()
    );

    // 2. 模拟点击右上角 "Sheet" 按钮 (x=355, y=70) 打开底部抽屉 BottomSheet
    bridge.on_motion_event(0, AndroidHostBridge::ACTION_DOWN, 355, 70);
    bridge.on_motion_event(0, AndroidHostBridge::ACTION_UP, 355, 70);
    let dirty_f2 = bridge.do_frame(16);
    println!(
        "[Android] 打开 BottomSheet 快捷抽屉: 脏矩形数 = {}, sheet_open = {}",
        dirty_f2,
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 3. 模拟软键盘 IME 弹出 (280px)
    bridge.on_ime_inset(280);
    let dirty_f3 = bridge.do_frame(16);
    println!(
        "[Android] 软键盘弹出 (IME=280px) 自动抬升抽屉: 脏矩形数 = {}, keyboard_inset = {}px",
        dirty_f3,
        bridge.embed_app().engine.keyboard_inset()
    );

    // 4. 模拟按下 Android 系统返回键关闭 BottomSheet
    let consumed = bridge.on_back_pressed();
    let _ = bridge.do_frame(16);
    println!(
        "[Android] 按下系统返回键: 内部消费 = {}, sheet_open = {}",
        consumed,
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );
}
