//! `ios_app` 端到端集成测试（验证仅依赖 `velox-kit` 的 iOS 应用生命周期、灵动岛/Home Indicator 安全区、`UIKeyboard` 软键盘避让、左边缘右滑返回与 Retina `@3x` 缩放）。

use ios_app::{IosHostBridge, IosMobileApp, IosRoute};
use velox_kit::platform::mobile::{DeviceOrientation, MobileOs};
use velox_kit::prelude::*;

#[test]
fn test_ios_dynamic_island_safe_area_and_swipe_back() {
    // 1. 初始化 393x852 iPhone 15 Pro 视口（顶部灵动岛 59pt，底部 Home Indicator 34pt）
    let mut engine = VeloxEngine::new(393, 852);
    engine.set_safe_area(SafeAreaInsets::ios_portrait());
    let root = engine.mount_root(IosMobileApp::new());

    let f1 = engine.step_frame_dt(8);
    assert_eq!(f1, &[Region::new(0, 0, 393, 852)]);
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        IosRoute::Dashboard
    );

    // 2. 点击底部 TabBar 第 2 项 "Telemetry" (x=196, y=852 - 34 - 24 = 794) 推入二级页
    engine.dispatch_pointer_down(196, 794);
    engine.dispatch_pointer_up(196, 794);
    let f2 = engine.step_frame_dt(8);
    assert!(!f2.is_empty());
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        IosRoute::Telemetry
    );
    assert_eq!(engine.view(root).unwrap().nav.depth(), 2);

    // 3. 在二级页模拟 iOS 左边缘右滑返回手势: (6, 420) -> (120, 420)
    engine.dispatch_pointer_down(6, 420);
    engine.dispatch_pointer_move(64, 420);
    engine.dispatch_pointer_up(120, 420);
    let _ = engine.step_frame_dt(8);
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        IosRoute::Dashboard,
        "iOS left-edge swipe back should pop Telemetry back to Dashboard"
    );
    assert_eq!(engine.view(root).unwrap().back_count, 1);
}

#[test]
fn test_ios_action_sheet_uikeyboard_and_interactive_pop_consumption() {
    let mut bridge = IosHostBridge::new(393, 852, 59, 34);
    assert_eq!(bridge.embed_app().os(), MobileOs::Ios);
    assert_eq!(bridge.embed_app().os().gpu_api_name(), "Metal");
    assert_eq!(bridge.on_display_link(8), 1);
    assert_eq!(bridge.rgba_slice().len(), 393 * 852 * 4);
    assert!(bridge.rgba_slice().chunks_exact(4).any(|px| px[3] == 255));

    // 点击右上角 "Sheet" 按钮 (x=358, y=80) 打开底部抽屉
    bridge.on_ui_touch(1, IosHostBridge::UITOUCH_PHASE_BEGAN, 358, 80);
    bridge.on_ui_touch(1, IosHostBridge::UITOUCH_PHASE_ENDED, 358, 80);
    assert!(bridge.on_display_link(8) >= 1);
    assert!(
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 模拟 UIKeyboard 弹出 (291pt)
    bridge.on_keyboard_frame_changed(291);
    assert!(bridge.on_display_link(8) >= 1);
    assert_eq!(bridge.embed_app().engine.keyboard_inset(), 291);

    // 第一次触发 iOS 返回手势：应消费事件 (返回 true) 并关闭 Action Sheet
    assert!(
        bridge.on_interactive_pop_gesture(),
        "First iOS interactive pop gesture should close Action Sheet"
    );
    assert!(bridge.on_display_link(8) >= 1);
    assert!(
        !bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 第二次触发返回手势：处于根页面且无弹窗，应返回 false
    assert!(!bridge.on_interactive_pop_gesture());
}

#[test]
fn test_ios_retina_3x_scale_and_landscape_rotation() {
    let mut bridge = IosHostBridge::new(393, 852, 59, 34);
    bridge.set_retina_scale(3);
    assert_eq!(bridge.on_display_link(8), 1);
    assert_eq!(bridge.embed_app().physical_width(), 393 * 3);
    assert_eq!(bridge.embed_app().physical_height(), 852 * 3);
    assert_eq!(bridge.rgba_slice().len(), (393 * 3) * (852 * 3) * 4);

    // 旋转至横屏 852x393，并应用 iOS 横屏标准安全区 (left=59, right=59, bottom=21)
    bridge.set_retina_scale(1);
    bridge.on_drawable_resized(852, 393);
    bridge.set_orientation(DeviceOrientation::LandscapeLeft);
    assert_eq!(bridge.on_display_link(8), 1);
    assert_eq!(
        bridge.embed_app().engine.safe_area(),
        SafeAreaInsets::ios_landscape()
    );
    let rect = bridge.dirty_rects()[0];
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0, 0, 852, 393));
}
