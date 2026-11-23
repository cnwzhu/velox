//! `web_app` 端到端集成测试（验证仅依赖 `velox-kit` 的 WebAssembly `<canvas>` 应用、线性内存 `RGBA8888` 视图、PointerEvent 与 `popstate` 路由回退）。

use web_app::{
    WebRoute, create_web_app, generate_index_html, velox_wasm_app_init, velox_web_app_destroy,
    velox_web_app_dirty_rects_ptr, velox_web_app_pointer_event, velox_web_app_popstate_back,
    velox_web_app_resize, velox_web_app_rgba_byte_len, velox_web_app_rgba_ptr,
    velox_web_app_step_frame,
};

#[test]
fn test_web_wasm_canvas_init_imagedata_and_dirty_rects() {
    // 1. 通过 WebAssembly C ABI 创建 800x600 Canvas 应用实例
    let app_ptr = velox_wasm_app_init(800, 600);
    assert!(!app_ptr.is_null());

    unsafe {
        // 首帧整屏渲染
        let dirty_count = velox_web_app_step_frame(app_ptr, 16);
        assert_eq!(dirty_count, 1);

        // 校验供浏览器 `new Uint8ClampedArray(memory.buffer, ptr, len)` 使用的 RGBA8888 缓冲区
        let rgba_len = velox_web_app_rgba_byte_len(app_ptr);
        assert_eq!(rgba_len, 800 * 600 * 4);
        let rgba_ptr = velox_web_app_rgba_ptr(app_ptr);
        let rgba_slice = core::slice::from_raw_parts(rgba_ptr, rgba_len);
        assert!(rgba_slice.chunks_exact(4).any(|px| px[3] == 255));

        let rect_ptr = velox_web_app_dirty_rects_ptr(app_ptr);
        let rect = *rect_ptr;
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (0, 0, 800, 600));

        // 第二帧无交互时应保持 0 脏矩形（0% CPU 静止待机）
        assert_eq!(velox_web_app_step_frame(app_ptr, 16), 0);

        velox_web_app_destroy(app_ptr);
    }
}

#[test]
fn test_web_pointer_wheel_popstate_back_and_resize() {
    let (mut web, root) = create_web_app(800, 600);
    assert_eq!(web.step_frame(16), 1);
    assert_eq!(
        web.engine.view(root).unwrap().nav.current(),
        WebRoute::Overview
    );

    let app_ptr = &mut web as *mut _;
    unsafe {
        // 1. 模拟浏览器 pointerdown (0) / pointerup (2) 点击底部 TabBar 第 3 项 "Preferences" (x=660, y=575)
        velox_web_app_pointer_event(app_ptr, 0, 660, 575);
        velox_web_app_pointer_event(app_ptr, 2, 660, 575);
        assert!(velox_web_app_step_frame(app_ptr, 16) >= 1);
    }
    assert_eq!(
        web.engine.view(root).unwrap().nav.current(),
        WebRoute::Preferences
    );

    unsafe {
        // 2. 模拟点击右上角 "Inspector" 按钮 (x=760, y=22) 打开底部抽屉 BottomSheet
        velox_web_app_pointer_event(app_ptr, 0, 760, 22);
        velox_web_app_pointer_event(app_ptr, 2, 760, 22);
        assert!(velox_web_app_step_frame(app_ptr, 16) >= 1);
    }
    assert!(web.engine.view(root).unwrap().drawer_open);

    unsafe {
        // 3. 第一次触发浏览器 popstate 后退：优先关闭抽屉，返回 true
        assert!(velox_web_app_popstate_back(app_ptr));
        assert!(velox_web_app_step_frame(app_ptr, 16) >= 1);
    }
    assert!(!web.engine.view(root).unwrap().drawer_open);

    unsafe {
        // 4. 第二次触发浏览器 popstate 后退：从 Preferences 回退至根路由 Overview，返回 true
        assert!(velox_web_app_popstate_back(app_ptr));
        assert!(velox_web_app_step_frame(app_ptr, 16) >= 1);
    }
    assert_eq!(
        web.engine.view(root).unwrap().nav.current(),
        WebRoute::Overview
    );

    unsafe {
        // 5. 模拟浏览器窗口 ResizeObserver 调整画布尺寸为 1024x768
        velox_web_app_resize(app_ptr, 1024, 768);
        assert_eq!(velox_web_app_step_frame(app_ptr, 16), 1);
        assert_eq!(velox_web_app_rgba_byte_len(app_ptr), 1024 * 768 * 4);
    }

    let html = generate_index_html("./web_app.wasm", 800, 600);
    assert!(html.contains("mountVeloxCanvas"));
    assert!(html.contains("velox_wasm_app_init"));
}
