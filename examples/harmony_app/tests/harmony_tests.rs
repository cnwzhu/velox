//! `harmony_app` 端到端集成测试（验证仅依赖 `velox-kit` 的鸿蒙 HarmonyOS NEXT 应用生命周期、`AvoidArea` 避让区、`TYPE_KEYBOARD` 软键盘避让、左滑返回、`onBackPress` 消费与折叠屏展开旋转）。

use harmony_app::{HarmonyHostBridge, HarmonyMobileApp, HarmonyRoute};
use velox_kit::platform::mobile::{DeviceOrientation, MobileOs};
use velox_kit::prelude::*;

#[test]
fn test_harmony_avoid_area_touch_navigation_and_swipe_back() {
    // 1. 初始化 384x832 鸿蒙手机视口（顶部状态栏 TYPE_SYSTEM 38vp，底部导航条 TYPE_NAVIGATION_INDICATOR 28vp）
    let mut engine = VeloxEngine::new(384, 832);
    engine.set_safe_area(SafeAreaInsets::harmony_portrait());
    let root = engine.mount_root(HarmonyMobileApp::new());

    let f1 = engine.step_frame_dt(8);
    assert_eq!(f1, &[Region::new(0, 0, 384, 832)]);
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        HarmonyRoute::Dashboard
    );

    // 2. 点击底部 TabBar 第 2 项 "SoftBus" (x=192, y=832 - 28 - 24 = 780) 推入分布式软总线二级页
    engine.dispatch_pointer_down(192, 780);
    engine.dispatch_pointer_up(192, 780);
    let f2 = engine.step_frame_dt(8);
    assert!(!f2.is_empty());
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        HarmonyRoute::DistributedBus
    );
    assert_eq!(engine.view(root).unwrap().nav.depth(), 2);

    // 3. 在二级页模拟鸿蒙左边缘右滑返回手势: (8, 416) -> (112, 416)
    engine.dispatch_pointer_down(8, 416);
    engine.dispatch_pointer_move(60, 416);
    engine.dispatch_pointer_up(112, 416);
    let _ = engine.step_frame_dt(8);
    assert_eq!(
        engine.view(root).unwrap().nav.current(),
        HarmonyRoute::Dashboard,
        "HarmonyOS left-edge swipe back should pop DistributedBus back to Dashboard"
    );
    assert_eq!(engine.view(root).unwrap().back_count, 1);
}

#[test]
fn test_harmony_atomic_service_sheet_keyboard_and_on_back_press() {
    let mut bridge = HarmonyHostBridge::new(384, 832, 38, 28);
    assert_eq!(bridge.embed_app().os(), MobileOs::HarmonyOs);
    assert_eq!(bridge.embed_app().os().gpu_api_name(), "Vulkan");
    assert_eq!(bridge.on_frame_callback(8), 1);
    assert_eq!(bridge.rgba_slice().len(), 384 * 832 * 4);
    assert!(bridge.rgba_slice().chunks_exact(4).any(|px| px[3] == 255));

    // 点击右上角 "Sheet" 按钮 (x=350, y=60) 打开底部元服务抽屉
    bridge.on_touch_event(0, HarmonyHostBridge::OH_TOUCH_DOWN, 350, 60);
    bridge.on_touch_event(0, HarmonyHostBridge::OH_TOUCH_UP, 350, 60);
    assert!(bridge.on_frame_callback(8) >= 1);
    assert!(
        bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 模拟鸿蒙 AvoidAreaType.TYPE_KEYBOARD 软键盘弹出 (276vp)
    bridge.on_keyboard_avoid_area(276);
    assert!(bridge.on_frame_callback(8) >= 1);
    assert_eq!(bridge.embed_app().engine.keyboard_inset(), 276);

    // 第一次触发鸿蒙 onBackPress()：应消费事件 (返回 true) 并关闭元服务抽屉
    assert!(
        bridge.on_back_press(),
        "First HarmonyOS onBackPress should be consumed to close BottomSheet"
    );
    assert!(bridge.on_frame_callback(8) >= 1);
    assert!(
        !bridge
            .embed_app()
            .engine
            .view(bridge.root_handle)
            .unwrap()
            .sheet_open
    );

    // 第二次触发鸿蒙 onBackPress()：抽屉已关闭且位于根路由，返回 false 交由宿主 UIAbility 处理
    assert!(!bridge.on_back_press());
}

#[test]
fn test_harmony_foldable_expand_and_landscape_rotation() {
    let mut bridge = HarmonyHostBridge::new(384, 832, 38, 28);
    assert_eq!(bridge.on_frame_callback(8), 1);

    // 模拟鸿蒙双折叠/三折叠展开或横屏旋转至 832x384，应用鸿蒙横屏标准避让区域
    bridge.on_surface_changed(832, 384);
    bridge.set_orientation(DeviceOrientation::LandscapeLeft);
    assert_eq!(bridge.on_frame_callback(8), 1);
    assert_eq!(
        bridge.embed_app().engine.safe_area(),
        SafeAreaInsets::harmony_landscape()
    );
    assert_eq!(bridge.rgba_slice().len(), 832 * 384 * 4);
    let rect = bridge.dirty_rects()[0];
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0, 0, 832, 384));
}
