//! `android_app` 端到端集成测试（验证仅依赖 `velox-kit` 的 Android 应用生命周期、安全区、软键盘避让、左滑返回与物理返回键消费）。

use android_app::{AndroidHostBridge, AndroidMobileApp, AndroidRoute};
use velox_kit::prelude::*;

#[test]
fn test_android_safe_area_touch_navigation_and_swipe_back() {
    // 1. 初始化 390x844 Android 手机视口（状态栏 48px，底部手势导航栏 34px）
    let mut engine = VeloxEngine::new(390, 844);
    engine.set_safe_area(SafeAreaInsets::new(48, 0, 34, 0));
    let root = engine.mount_root(AndroidMobileApp::new());

    let f1 = engine.step_frame_dt(16);
    assert_eq!(f1, &[Region::new(0, 0, 390, 844)]);
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        AndroidRoute::Dashboard
    );

    // 2. 点击底部 TabBar 第 2 项 "Detail" (x=195, y=844 - 34 - 24 = 786) 推入二级详情页
    engine.dispatch_pointer_down(195, 786);
    engine.dispatch_pointer_up(195, 786);
    let f2 = engine.step_frame_dt(16);
    assert!(!f2.is_empty());
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        AndroidRoute::DeviceDetail
    );
    assert_eq!(engine.view(root).unwrap().nav.depth(), 2);

    // 3. 在二级详情页模拟手机左边缘右滑返回手势: (8, 420) -> (110, 420)
    engine.dispatch_pointer_down(8, 420);
    engine.dispatch_pointer_move(60, 420);
    engine.dispatch_pointer_up(110, 420);
    let _ = engine.step_frame_dt(16);
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        AndroidRoute::Dashboard,
        "Left-edge swipe back should pop DeviceDetail back to Dashboard"
    );
    assert_eq!(engine.view(root).unwrap().back_count, 1);
}

#[test]
fn test_android_bottom_sheet_ime_keyboard_and_back_button_consumption() {
    let mut bridge = AndroidHostBridge::new(390, 844, 48, 34);
    assert_eq!(bridge.do_frame(16), 1);
    assert_eq!(bridge.rgba_slice().len(), 390 * 844 * 4);
    assert!(bridge.rgba_slice().chunks_exact(4).any(|px| px[3] == 255));

    // 点击右上角 "Sheet" 按钮 (x=355, y=70) 打开底部抽屉
    bridge.on_motion_event(1, AndroidHostBridge::ACTION_DOWN, 355, 70);
    bridge.on_motion_event(1, AndroidHostBridge::ACTION_UP, 355, 70);
    assert!(bridge.do_frame(16) >= 1);
    assert!(
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 模拟软键盘弹出 (IME = 260px)
    bridge.on_ime_inset(260);
    assert!(bridge.do_frame(16) >= 1);
    assert_eq!(bridge.embed_app().engine.keyboard_inset(), 260);

    // 第一次按下 Android 物理返回键：应消费事件 (返回 true) 并关闭 BottomSheet
    assert!(
        bridge.on_back_pressed(),
        "First Android Back press should be consumed to close BottomSheet"
    );
    assert!(bridge.do_frame(16) >= 1);
    assert!(
        !bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 第二次按下 Android 物理返回键：此时抽屉已关且处于根页面 Dashboard，应返回 false 交由宿主 Activity 处理
    assert!(
        !bridge.on_back_pressed(),
        "Second Android Back press at root screen with sheet closed should return false"
    );
}

#[test]
fn test_android_screen_rotation_portrait_to_landscape() {
    let mut bridge = AndroidHostBridge::new(390, 844, 48, 34);
    assert_eq!(bridge.do_frame(16), 1);

    // 旋转至横屏 844x390，左侧挖孔安全区 48px，底部手势横条 24px
    bridge.on_surface_changed(844, 390);
    bridge.on_window_insets(0, 0, 24, 48);
    assert_eq!(bridge.do_frame(16), 1);
    assert_eq!(bridge.rgba_slice().len(), 844 * 390 * 4);
    let rect = bridge.dirty_rects()[0];
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0, 0, 844, 390));
}
