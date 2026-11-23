//! Apple iOS / iPadOS（`UIKit` / `CADisplayLink` / `CAMetalLayer` / Swift `xcframework`）专属平台宿主桥接层。
//!
//! 通过启用 `velox-engine/ios` 或 `velox-kit/ios` Feature 按需引入：
//! - [`IosHostBridge<V>`]：泛型封装任意 [`View`] 的 iOS 宿主生命周期管理（灵动岛 `59pt` + Home Indicator `34pt` `safeAreaInsets`、`UIKeyboardWillChangeFrameNotification` 软键盘高度、`UITouchPhase` 触控、Retina `@2x`/`@3x` 缩放、左边缘右滑返回消费）；
//! - [`export_ios_app!`](crate::export_ios_app)：一行宏为业务 [`View`] 自动生成完整 C ABI (`velox_ios_app_*`) 符号供 Xcode / Swift / Objective-C 调用。

use std::boxed::Box;

use crate::element::View;
use crate::platform::mobile::{
    CDirtyRect, DeviceOrientation, MobileEmbedApp, MobileOs, TouchPhase,
};
use crate::pod::{Handle, SafeAreaInsets};

/// 泛型 iOS `UIKit` / `CADisplayLink` / `CAMetalLayer` 宿主桥接器（将任意业务 [`View`] 挂载至 [`MobileEmbedApp`]）。
pub struct IosHostBridge<V: View + 'static> {
    app: *mut MobileEmbedApp,
    pub root_handle: Handle<V>,
}

impl<V: View + Default + 'static> IosHostBridge<V> {
    /// 使用 `V::default()` 创建指定逻辑点尺寸（Points）与灵动岛/Home Indicator 安全区的 iOS 应用实例。
    #[inline]
    pub fn new(width: i16, height: i16, safe_top: i16, safe_bottom: i16) -> Self {
        Self::with_root(width, height, safe_top, safe_bottom, V::default())
    }
}

impl<V: View + 'static> IosHostBridge<V> {
    /// `UITouchPhase.began`。
    pub const UITOUCH_PHASE_BEGAN: u8 = 0;
    /// `UITouchPhase.moved`。
    pub const UITOUCH_PHASE_MOVED: u8 = 1;
    /// `UITouchPhase.stationary`。
    pub const UITOUCH_PHASE_STATIONARY: u8 = 2;
    /// `UITouchPhase.ended`。
    pub const UITOUCH_PHASE_ENDED: u8 = 3;
    /// `UITouchPhase.cancelled`。
    pub const UITOUCH_PHASE_CANCELLED: u8 = 4;

    /// 使用给定根视图 `root_view` 创建指定逻辑点尺寸（Points）与安全区的 iOS 应用实例。
    pub fn with_root(
        width: i16,
        height: i16,
        safe_top: i16,
        safe_bottom: i16,
        root_view: V,
    ) -> Self {
        let safe_area = SafeAreaInsets::new(safe_top, 0, safe_bottom, 0);
        let mut embed = Box::new(MobileEmbedApp::new_for_os(
            MobileOs::Ios,
            width,
            height,
            safe_area,
        ));
        let root_handle = embed.engine.mount_root(root_view);
        Self {
            app: Box::into_raw(embed),
            root_handle,
        }
    }

    /// 设置 iOS Retina 屏幕像素缩放倍率（`@2x` / `@3x`，对应 `UIScreen.main.scale`）。
    #[inline]
    pub fn set_retina_scale(&mut self, scale: i16) {
        unsafe { &mut *self.app }.set_dpi_scale(scale);
    }

    /// 响应 `UIView.layoutSubviews` 或屏幕旋转后的逻辑尺寸变化。
    #[inline]
    pub fn on_drawable_resized(&mut self, width: i16, height: i16) {
        unsafe { &mut *self.app }.resize(width, height);
    }

    /// 同步 `UIView.safeAreaInsetsDidChange` 安全区内边距。
    #[inline]
    pub fn on_safe_area_insets_changed(&mut self, top: i16, right: i16, bottom: i16, left: i16) {
        unsafe { &mut *self.app }.set_safe_area(SafeAreaInsets::new(top, right, bottom, left));
    }

    /// 切换 iOS 设备屏幕朝向并应用对应朝向的标准灵动岛/Home Indicator 安全区。
    #[inline]
    pub fn set_orientation(&mut self, orientation: DeviceOrientation) {
        unsafe { &mut *self.app }.set_orientation(orientation);
    }

    /// 同步 `UIKeyboardWillChangeFrameNotification` 软键盘底部遮挡高度（Points）。
    #[inline]
    pub fn on_keyboard_frame_changed(&mut self, keyboard_height_pt: i16) {
        unsafe { &mut *self.app }.set_keyboard_inset(keyboard_height_pt);
    }

    /// 分发 iOS `UITouch` 事件（自动将 `UITouchPhase` 映射为 [`TouchPhase`]）。
    #[inline]
    pub fn on_ui_touch(&mut self, touch_id: u32, ui_touch_phase: u8, x: i16, y: i16) {
        let phase = TouchPhase::from_ios_phase(ui_touch_phase);
        unsafe { &mut *self.app }.push_touch(touch_id, phase, x, y);
    }

    /// 分发 iOS `UIScreenEdgePanGestureRecognizer` 左边缘返回事件：
    /// 返回 `true` 表示应用内部已消费（关闭 Action Sheet 或 pop 二级视图）。
    #[inline]
    pub fn on_interactive_pop_gesture(&mut self) -> bool {
        unsafe { &mut *self.app }.handle_back()
    }

    /// 响应 `CADisplayLink` 垂直同步回调推进一帧，返回本帧脏矩形个数。
    #[inline]
    pub fn on_display_link(&mut self, dt_ms: u16) -> usize {
        unsafe { &mut *self.app }.step_frame(dt_ms)
    }

    /// 获取 `RGBA8888` 像素缓冲区的只读字节切片（可直接上传至 `MTLTexture` 或 `CGDataProvider`）。
    #[inline]
    pub fn rgba_slice(&self) -> &[u8] {
        unsafe { &*self.app }.rgba_bytes()
    }

    /// 获取本帧脏矩形切片（供 `MTLTexture.replaceRegion` 局部纹理更新使用）。
    #[inline]
    pub fn dirty_rects(&self) -> &[CDirtyRect] {
        unsafe { &*self.app }.dirty_rects()
    }

    /// 访问底层 [`MobileEmbedApp`] 只读引用。
    #[inline]
    pub fn embed_app(&self) -> &MobileEmbedApp {
        unsafe { &*self.app }
    }

    /// 访问底层 [`MobileEmbedApp`] 可变引用。
    #[inline]
    pub fn embed_app_mut(&mut self) -> &mut MobileEmbedApp {
        unsafe { &mut *self.app }
    }

    /// 查询当前 iOS 宿主实例是否启用了基于轻量 `wgpu` (Apple Metal) 2D SDF 的硬件 GPU 渲染后端。
    #[inline]
    pub fn is_gpu_backend(&self) -> bool {
        unsafe { &*self.app }.backend().is_gpu()
    }

    /// 取出内部裸指针供 FFI 使用（调用后不再由 `Drop` 自动释放）。
    #[inline]
    pub fn into_raw(mut self) -> *mut MobileEmbedApp {
        let ptr = self.app;
        self.app = core::ptr::null_mut();
        ptr
    }
}

impl<V: View + 'static> Drop for IosHostBridge<V> {
    fn drop(&mut self) {
        if !self.app.is_null() {
            unsafe {
                let _ = Box::from_raw(self.app);
            }
        }
    }
}

/// 框架内置的标准 iOS C 桥接头文件（`velox_ios.h`）模板。
pub const IOS_VELOX_HEADER_H: &str = r#"#ifndef VELOX_IOS_H
#define VELOX_IOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MobileEmbedApp MobileEmbedApp;

typedef struct {
    int16_t x;
    int16_t y;
    int16_t w;
    int16_t h;
} CDirtyRect;

MobileEmbedApp* velox_ios_app_create(
    int16_t width,
    int16_t height,
    int16_t safe_top,
    int16_t safe_bottom
);

void velox_ios_app_destroy(MobileEmbedApp* app);
bool velox_ios_app_backend_is_gpu(const MobileEmbedApp* app);
void velox_ios_app_resize(MobileEmbedApp* app, int16_t width, int16_t height);
void velox_ios_app_set_dpi_scale(MobileEmbedApp* app, int16_t scale);
void velox_ios_app_set_safe_area(
    MobileEmbedApp* app,
    int16_t top,
    int16_t right,
    int16_t bottom,
    int16_t left
);
void velox_ios_app_set_keyboard_inset(MobileEmbedApp* app, int16_t height_pt);
void velox_ios_app_push_touch(
    MobileEmbedApp* app,
    uint32_t touch_id,
    uint8_t ui_touch_phase,
    int16_t x,
    int16_t y
);
bool velox_ios_app_handle_back(MobileEmbedApp* app);
size_t velox_ios_app_step_frame(MobileEmbedApp* app, uint16_t dt_ms);
const uint8_t* velox_ios_app_framebuffer_ptr(const MobileEmbedApp* app);
const CDirtyRect* velox_ios_app_dirty_rects_ptr(const MobileEmbedApp* app);
size_t velox_ios_app_dirty_rects_count(const MobileEmbedApp* app);

#ifdef __cplusplus
}
#endif

#endif /* VELOX_IOS_H */
"#;

/// 框架内置的标准 iOS Swift `UIView` + `CAMetalLayer` + 120Hz `CADisplayLink` 宿主视图（`VeloxMetalView.swift`）源码模板。
pub const IOS_METAL_VIEW_SWIFT: &str = r#"import UIKit
import QuartzCore

/// iOS 原生宿主视图封装（由 `velox-engine::platform::ios` 自动生成，对接 `velox-kit` C ABI 接口）。
final class VeloxMetalView: UIView {
    private var appHandle: OpaquePointer?
    private var displayLink: CADisplayLink?

    override class var layerClass: AnyClass {
        return CAMetalLayer.self
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        setupView()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        setupView()
    }

    private func setupView() {
        isMultipleTouchEnabled = true
        contentScaleFactor = UIScreen.main.scale

        let w = Int16(max(1, bounds.width))
        let h = Int16(max(1, bounds.height))
        appHandle = velox_ios_app_create(w, h, 59, 34)
        if let handle = appHandle {
            velox_ios_app_set_dpi_scale(handle, Int16(max(1, Int(UIScreen.main.scale.rounded()))))
        }

        let link = CADisplayLink(target: self, selector: #selector(onDisplayLink(_:)))
        if #available(iOS 15.0, *) {
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, __preferred: 120)
        }
        link.add(to: .main, forMode: .common)
        displayLink = link

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(onKeyboardFrameChanged(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification,
            object: nil
        )
    }

    deinit {
        displayLink?.invalidate()
        if let handle = appHandle {
            velox_ios_app_destroy(handle)
            appHandle = nil
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard let handle = appHandle else { return }
        velox_ios_app_resize(handle, Int16(max(1, bounds.width)), Int16(max(1, bounds.height)))
    }

    override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        guard let handle = appHandle else { return }
        let insets = safeAreaInsets
        velox_ios_app_set_safe_area(
            handle,
            Int16(insets.top.rounded()),
            Int16(insets.right.rounded()),
            Int16(insets.bottom.rounded()),
            Int16(insets.left.rounded())
        )
    }

    @objc private func onKeyboardFrameChanged(_ notification: Notification) {
        guard let handle = appHandle,
              let endFrame = notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else {
            return
        }
        let converted = convert(endFrame, from: window)
        let overlap = max(0, bounds.maxY - converted.minY)
        velox_ios_app_set_keyboard_inset(handle, Int16(overlap.rounded()))
    }

    @objc private func onDisplayLink(_ link: CADisplayLink) {
        guard let handle = appHandle else { return }
        let dtMs = UInt16(max(1, min(100, Int((link.targetTimestamp - link.timestamp) * 1000.0))))
        let dirtyCount = velox_ios_app_step_frame(handle, dtMs)
        if dirtyCount > 0 {
            setNeedsDisplay()
        }
    }

    private func forwardTouches(_ touches: Set<UITouch>, phase: UInt8) {
        guard let handle = appHandle else { return }
        for touch in touches {
            let pt = touch.location(in: self)
            let id = UInt32(truncatingIfNeeded: touch.hash)
            velox_ios_app_push_touch(handle, id, phase, Int16(pt.x.rounded()), Int16(pt.y.rounded()))
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        forwardTouches(touches, phase: 0)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        forwardTouches(touches, phase: 1)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        forwardTouches(touches, phase: 3)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        forwardTouches(touches, phase: 4)
    }
}
"#;

/// 生成 iOS `Info.plist` 宿主配置清单。
pub fn generate_ios_info_plist(bundle_id: &str, app_name: &str) -> std::string::String {
    std::format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>VeloxIosApp</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{app_name}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CADisableMinimumFrameDurationOnPhone</key>
    <true/>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
</dict>
</plist>
"#
    )
}

/// 将框架内置的 iOS 宿主胶水文件（`include/velox_ios.h`、`VeloxMetalView.swift`、`Info.plist`）一键输出至目标目录。
pub fn emit_ios_host_project(
    out_dir: &std::path::Path,
    bundle_id: &str,
    app_name: &str,
) -> std::io::Result<()> {
    let inc_dir = out_dir.join("include");
    std::fs::create_dir_all(&inc_dir)?;
    std::fs::write(inc_dir.join("velox_ios.h"), IOS_VELOX_HEADER_H)?;
    std::fs::write(out_dir.join("VeloxMetalView.swift"), IOS_METAL_VIEW_SWIFT)?;
    std::fs::write(
        out_dir.join("Info.plist"),
        generate_ios_info_plist(bundle_id, app_name),
    )?;
    Ok(())
}

/// 为指定业务视图类型一键导出完整的 iOS C ABI (`velox_ios_app_*`) 符号（对接 `VeloxMetalView.swift` / `velox_ios.h`）。
#[macro_export]
macro_rules! export_ios_app {
    ($app_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn velox_ios_app_create(
            width: i16,
            height: i16,
            safe_top: i16,
            safe_bottom: i16,
        ) -> *mut $crate::platform::mobile::MobileEmbedApp {
            $crate::platform::ios::IosHostBridge::<$app_ty>::new(
                width,
                height,
                safe_top,
                safe_bottom,
            )
            .into_raw()
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_destroy(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::destroy(app);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_backend_is_gpu(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> bool {
            unsafe { $crate::platform::mobile::ffi::backend_is_gpu(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_resize(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            width: i16,
            height: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::resize(app, width, height);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_set_dpi_scale(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            scale: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_dpi_scale(app, scale);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_set_safe_area(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            top: i16,
            right: i16,
            bottom: i16,
            left: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_safe_area(app, top, right, bottom, left);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_set_keyboard_inset(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            height_pt: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_keyboard_inset(app, height_pt);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_push_touch(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            touch_id: u32,
            ui_touch_phase: u8,
            x: i16,
            y: i16,
        ) {
            let phase = $crate::platform::mobile::TouchPhase::from_ios_phase(ui_touch_phase);
            unsafe {
                $crate::platform::mobile::ffi::push_touch(app, touch_id, phase, x, y);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_handle_back(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
        ) -> bool {
            unsafe { $crate::platform::mobile::ffi::handle_back(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_step_frame(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            dt_ms: u16,
        ) -> usize {
            unsafe { $crate::platform::mobile::ffi::step_frame(app, dt_ms) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_framebuffer_ptr(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> *const u8 {
            unsafe { $crate::platform::mobile::ffi::framebuffer_ptr(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_dirty_rects_ptr(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> *const $crate::platform::mobile::CDirtyRect {
            unsafe { $crate::platform::mobile::ffi::dirty_rects_ptr(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_ios_app_dirty_rects_count(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> usize {
            unsafe { $crate::platform::mobile::ffi::dirty_rects_count(app) }
        }
    };
}
