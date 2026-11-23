//! Google Android（`ANativeWindow` / `Choreographer` / `WindowInsetsCompat` / JNI）专属平台宿主桥接层。
//!
//! 通过启用 `velox-core/android` 或 `velox-kit/android` Feature 按需引入：
//! - [`AndroidHostBridge<V>`]：泛型封装任意 [`View`] 的 Android 宿主生命周期管理（`Surface` 尺寸、系统状态栏/挖孔屏/手势横条 `WindowInsets`、软键盘 `IME` 高度、`MotionEvent` 触控、`OnBackInvokedCallback` 返回键消费）；
//! - [`step_and_blit_android_surface`]：支持 `wgpu` (Vulkan) 硬件 Swapchain 直通或 `ANativeWindow_lock` (`WINDOW_FORMAT_RGBA_8888`) 步长对齐拷贝；
//! - [`export_android_app!`](crate::export_android_app)：一行宏为业务 [`View`] 自动生成完整 C ABI (`velox_android_app_*` / `velox_mobile_app_*`) 与 `com.velox.android.MainActivity` JNI 导出入口。

use std::boxed::Box;

use crate::element::View;
use crate::platform::mobile::{CDirtyRect, MobileEmbedApp, MobileOs, TouchPhase};
use crate::pod::{Handle, SafeAreaInsets};

/// 泛型 Android JNI / NDK 宿主桥接器（将任意业务 [`View`] 挂载至 [`MobileEmbedApp`] 并桥接 Android 视图生命周期）。
pub struct AndroidHostBridge<V: View + 'static> {
    app: *mut MobileEmbedApp,
    pub root_handle: Handle<V>,
}

impl<V: View + Default + 'static> AndroidHostBridge<V> {
    /// 使用 `V::default()` 创建指定视口尺寸与系统状态栏/手势导航条安全区的 Android 应用宿主。
    #[inline]
    pub fn new(width: i16, height: i16, status_bar_top: i16, gesture_bar_bottom: i16) -> Self {
        Self::with_root(
            width,
            height,
            status_bar_top,
            gesture_bar_bottom,
            V::default(),
        )
    }
}

impl<V: View + 'static> AndroidHostBridge<V> {
    /// Android `MotionEvent.ACTION_DOWN`。
    pub const ACTION_DOWN: u8 = 0;
    /// Android `MotionEvent.ACTION_UP`。
    pub const ACTION_UP: u8 = 1;
    /// Android `MotionEvent.ACTION_MOVE`。
    pub const ACTION_MOVE: u8 = 2;
    /// Android `MotionEvent.ACTION_CANCEL`。
    pub const ACTION_CANCEL: u8 = 3;

    /// 使用给定根视图实例 `root_view` 创建指定视口尺寸与安全区的 Android 应用宿主。
    pub fn with_root(
        width: i16,
        height: i16,
        status_bar_top: i16,
        gesture_bar_bottom: i16,
        root_view: V,
    ) -> Self {
        let safe_area = SafeAreaInsets::new(status_bar_top, 0, gesture_bar_bottom, 0);
        let mut embed = Box::new(MobileEmbedApp::new_for_os(
            MobileOs::Android,
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

    /// 设置 Android `DisplayMetrics.density` 整数倍 HiDPI 缩放因子。
    #[inline]
    pub fn set_dpi_scale(&mut self, scale: i16) {
        unsafe { &mut *self.app }.set_dpi_scale(scale);
    }

    /// 响应屏幕旋转或分屏窗口大小改变（如竖屏 `390x844` 旋转为横屏 `844x390`）。
    #[inline]
    pub fn on_surface_changed(&mut self, width: i16, height: i16) {
        unsafe { &mut *self.app }.resize(width, height);
    }

    /// 同步 Android `WindowInsetsCompat.Type.systemBars() | displayCutout()` 安全区内边距。
    #[inline]
    pub fn on_window_insets(&mut self, top: i16, right: i16, bottom: i16, left: i16) {
        unsafe { &mut *self.app }.set_safe_area(SafeAreaInsets::new(top, right, bottom, left));
    }

    /// 同步 Android `WindowInsetsCompat.Type.ime()` 软键盘底部像素高度。
    #[inline]
    pub fn on_ime_inset(&mut self, ime_bottom_px: i16) {
        unsafe { &mut *self.app }.set_keyboard_inset(ime_bottom_px);
    }

    /// 分发 Android `MotionEvent`（自动将 `ACTION_DOWN/MOVE/UP/CANCEL` 映射为 [`TouchPhase`]）。
    #[inline]
    pub fn on_motion_event(&mut self, pointer_id: u32, action: u8, x: i16, y: i16) {
        let phase = TouchPhase::from_android_action(action);
        unsafe { &mut *self.app }.push_touch(pointer_id, phase, x, y);
    }

    /// 分发 Android `OnBackInvokedCallback` / `onBackPressed` 系统返回事件：
    /// 返回 `true` 表示应用内部已消费（关闭抽屉或回退二级页），返回 `false` 表示交由宿主 `Activity` 处理。
    #[inline]
    pub fn on_back_pressed(&mut self) -> bool {
        unsafe { &mut *self.app }.handle_back()
    }

    /// 响应 Android `Choreographer.FrameCallback.doFrame` 推进一帧，返回本帧脏矩形个数。
    #[inline]
    pub fn do_frame(&mut self, dt_ms: u16) -> usize {
        unsafe { &mut *self.app }.step_frame(dt_ms)
    }

    /// 获取 `RGBA8888` 像素缓冲区的只读字节切片（可直接写入 `ANativeWindow_Buffer.bits`）。
    #[inline]
    pub fn rgba_slice(&self) -> &[u8] {
        unsafe { &*self.app }.rgba_bytes()
    }

    /// 获取本帧脏矩形切片（供 `ANativeWindow_lock` 局部脏区域刷新使用）。
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

    /// 查询当前 Android 宿主实例是否启用了基于轻量 `wgpu` (Vulkan) 2D SDF 的硬件 GPU 渲染后端。
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

impl<V: View + 'static> Drop for AndroidHostBridge<V> {
    fn drop(&mut self) {
        if !self.app.is_null() {
            unsafe {
                let _ = Box::from_raw(self.app);
            }
        }
    }
}

#[cfg(target_os = "android")]
#[repr(C)]
struct ANativeWindowBuffer {
    width: i32,
    height: i32,
    stride: i32,
    format: i32,
    bits: *mut core::ffi::c_void,
    reserved: [u32; 6],
}

#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(
        env: *mut core::ffi::c_void,
        surface: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn ANativeWindow_release(window: *mut core::ffi::c_void);
    fn ANativeWindow_setBuffersGeometry(
        window: *mut core::ffi::c_void,
        width: i32,
        height: i32,
        format: i32,
    ) -> i32;
    fn ANativeWindow_lock(
        window: *mut core::ffi::c_void,
        out_buffer: *mut ANativeWindowBuffer,
        in_out_dirty_bounds: *mut core::ffi::c_void,
    ) -> i32;
    fn ANativeWindow_unlockAndPost(window: *mut core::ffi::c_void) -> i32;
}

/// 推进一帧 [`MobileEmbedApp`] 并将像素提交至 Android `Surface` / `ANativeWindow`。
///
/// 若已挂载轻量 `wgpu` (Vulkan) 2D SDF Swapchain，则在 `step_frame` 内直接完成硬件零拷贝呈现；
/// 否则在 `aarch64` Android 环境中通过 `ANativeWindow_lock` 步长对齐写入 `RGBA_8888` 缓冲区。
///
/// # Safety
/// `app_ptr` 必须为有效的 [`MobileEmbedApp`] 指针或空指针；在 `aarch64` Android 下 `env` 与 `surface` 必须为合法 JNI `JNIEnv*` 与 `jobject Surface`。
pub unsafe fn step_and_blit_android_surface(
    env: *mut core::ffi::c_void,
    app_ptr: *mut MobileEmbedApp,
    surface: *mut core::ffi::c_void,
    dt_ms: i32,
) -> i32 {
    let Some(app_ref) = (unsafe { app_ptr.as_mut() }) else {
        return 0;
    };
    let dirty = app_ref.step_frame(dt_ms.max(1) as u16);
    if dirty == 0 || surface.is_null() {
        return dirty as i32;
    }
    if app_ref.has_active_gpu_window() {
        return dirty as i32;
    }

    #[cfg(target_os = "android")]
    unsafe {
        let win = ANativeWindow_fromSurface(env, surface);
        if !win.is_null() {
            let (w, h) = (
                app_ref.physical_width() as i32,
                app_ref.physical_height() as i32,
            );
            // WINDOW_FORMAT_RGBA_8888 = 1
            let _ = ANativeWindow_setBuffersGeometry(win, w, h, 1);
            let mut buf = ANativeWindowBuffer {
                width: 0,
                height: 0,
                stride: 0,
                format: 0,
                bits: core::ptr::null_mut(),
                reserved: [0; 6],
            };
            if ANativeWindow_lock(win, &mut buf, core::ptr::null_mut()) == 0 && !buf.bits.is_null()
            {
                let src = app_ref.rgba_bytes();
                let copy_w = (w.min(buf.width).max(0) as usize) * 4;
                let copy_h = h.min(buf.height).max(0) as usize;
                let dst_base = buf.bits as *mut u8;
                let dst_stride = (buf.stride.max(w) as usize) * 4;
                let src_stride = (w as usize) * 4;
                for row in 0..copy_h {
                    core::ptr::copy_nonoverlapping(
                        src.as_ptr().add(row * src_stride),
                        dst_base.add(row * dst_stride),
                        copy_w,
                    );
                }
                let _ = ANativeWindow_unlockAndPost(win);
            }
            ANativeWindow_release(win);
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = env;

    dirty as i32
}

/// 框架内置的标准 Android Kotlin 宿主视图组件（`VeloxSurfaceView.kt`）源码模板。
pub const ANDROID_SURFACE_VIEW_KT: &str = r#"package com.velox.android

import android.content.Context
import android.view.Choreographer
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Android 宿主视图封装（由 `velox-core::platform::android` 自动生成，对接 `velox-kit` C ABI / JNI 接口）。
 */
class VeloxSurfaceView(context: Context) : SurfaceView(context), SurfaceHolder.Callback, Choreographer.FrameCallback {
    private var nativeHandle: Long = 0L

    init {
        holder.addCallback(this)
        ViewCompat.setOnApplyWindowInsetsListener(this) { _, insets ->
            if (nativeHandle != 0L) {
                val sysBars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
                val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
                nativeSetSafeArea(nativeHandle, sysBars.top, sysBars.right, sysBars.bottom, sysBars.left)
                nativeSetKeyboardInset(nativeHandle, ime.bottom)
            }
            insets
        }
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        val frame = holder.surfaceFrame
        nativeHandle = nativeCreate(frame.width(), frame.height(), 48, 34)
        Choreographer.getInstance().postFrameCallback(this)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (nativeHandle != 0L) {
            nativeResize(nativeHandle, width, height)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (nativeHandle != 0L) {
            nativeDestroy(nativeHandle)
            nativeHandle = 0L
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (nativeHandle == 0L) return false
        val pointerId = event.getPointerId(0)
        nativePushTouch(nativeHandle, pointerId, event.actionMasked, event.x.toInt(), event.y.toInt())
        return true
    }

    fun handleBackPressed(): Boolean {
        return if (nativeHandle != 0L) nativeHandleBack(nativeHandle) else false
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (nativeHandle != 0L) {
            nativeStepAndBlitSurface(nativeHandle, holder.surface, 16)
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    private external fun nativeCreate(width: Int, height: Int, safeTop: Int, safeBottom: Int): Long
    private external fun nativeDestroy(handle: Long)
    private external fun nativeResize(handle: Long, width: Int, height: Int)
    private external fun nativeSetSafeArea(handle: Long, top: Int, right: Int, bottom: Int, left: Int)
    private external fun nativeSetKeyboardInset(handle: Long, height: Int)
    private external fun nativePushTouch(handle: Long, pointerId: Int, action: Int, x: Int, y: Int)
    private external fun nativeHandleBack(handle: Long): Boolean
    private external fun nativeStepAndBlitSurface(handle: Long, surface: Any, dtMs: Int): Int
}
"#;

/// 生成加载指定原生库名 `lib_name`（如 `"android_app"`）的 Android `MainActivity.java` 宿主源码。
pub fn generate_android_main_activity_java(lib_name: &str) -> std::string::String {
    std::format!(
        r#"package com.velox.android;

import android.app.Activity;
import android.os.Bundle;
import android.view.Choreographer;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.Window;

/**
 * Android 原生 Activity 宿主入口（由 `velox-core::platform::android` 自动生成，加载 `lib{lib_name}.so`）。
 */
public class MainActivity extends Activity
    implements SurfaceHolder.Callback,
               Choreographer.FrameCallback,
               View.OnTouchListener {{

    static {{
        System.loadLibrary("{lib_name}");
    }}

    private SurfaceView surfaceView;
    private long nativeHandle = 0L;
    private int dpiScale = 2;
    private int logicalWidth = 450;
    private int logicalHeight = 800;

    @Override
    protected void onCreate(Bundle savedInstanceState) {{
        super.onCreate(savedInstanceState);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        float density = getResources().getDisplayMetrics().density;
        dpiScale = Math.max(2, Math.round(density));
        surfaceView = new SurfaceView(this);
        surfaceView.getHolder().addCallback(this);
        surfaceView.setOnTouchListener(this);
        setContentView(surfaceView);
    }}

    @Override
    public boolean onTouch(View v, MotionEvent event) {{
        if (nativeHandle != 0L) {{
            int vw = Math.max(1, v.getWidth());
            int vh = Math.max(1, v.getHeight());
            int lx = Math.round(event.getX() * logicalWidth / (float) vw);
            int ly = Math.round(event.getY() * logicalHeight / (float) vh);
            int pointerId = event.getPointerId(0);
            nativePushTouch(
                nativeHandle,
                pointerId,
                event.getActionMasked(),
                lx,
                ly
            );
        }}
        return true;
    }}

    @Override
    public void surfaceCreated(SurfaceHolder holder) {{
        float density = getResources().getDisplayMetrics().density;
        dpiScale = Math.max(2, Math.round(density));
        int vw = surfaceView.getWidth() > 0 ? surfaceView.getWidth() : 900;
        int vh = surfaceView.getHeight() > 0 ? surfaceView.getHeight() : 1600;
        logicalWidth = Math.max(320, vw / dpiScale);
        logicalHeight = Math.max(320, vh / dpiScale);
        int physW = logicalWidth * dpiScale;
        int physH = logicalHeight * dpiScale;
        holder.setFixedSize(physW, physH);
        nativeHandle = nativeCreate(logicalWidth, logicalHeight, 0, 0);
        nativeSetDpiScale(nativeHandle, dpiScale);
        Choreographer.getInstance().postFrameCallback(this);
    }}

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {{
        if (nativeHandle != 0L) {{
            logicalWidth = Math.max(320, width / dpiScale);
            logicalHeight = Math.max(320, height / dpiScale);
            nativeSetDpiScale(nativeHandle, dpiScale);
            nativeResize(nativeHandle, logicalWidth, logicalHeight);
        }}
    }}

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {{
        if (nativeHandle != 0L) {{
            nativeDestroy(nativeHandle);
            nativeHandle = 0L;
        }}
    }}

    @Override
    public void onBackPressed() {{
        if (nativeHandle != 0L && nativeHandleBack(nativeHandle)) {{
            return;
        }}
        super.onBackPressed();
    }}

    @Override
    public void doFrame(long frameTimeNanos) {{
        if (nativeHandle != 0L && surfaceView.getHolder().getSurface().isValid()) {{
            nativeStepAndBlitSurface(nativeHandle, surfaceView.getHolder().getSurface(), 16);
            Choreographer.getInstance().postFrameCallback(this);
        }}
    }}

    public static native long nativeCreate(int width, int height, int safeTop, int safeBottom);
    public static native void nativeDestroy(long handle);
    public static native void nativeResize(long handle, int width, int height);
    public static native void nativeSetDpiScale(long handle, int scale);
    public static native void nativeSetSafeArea(long handle, int top, int right, int bottom, int left);
    public static native void nativeSetKeyboardInset(long handle, int height);
    public static native void nativePushTouch(long handle, int pointerId, int action, int x, int y);
    public static native boolean nativeHandleBack(long handle);
    public static native int nativeStepAndBlitSurface(long handle, Object surface, int dtMs);
}}
"#
    )
}

/// 生成 Android `AndroidManifest.xml` 宿主清单内容。
pub fn generate_android_manifest_xml(package_name: &str, app_label: &str) -> std::string::String {
    std::format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{package_name}"
    android:versionCode="1"
    android:versionName="0.1.0">

    <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="29" />

    <application
        android:label="{app_label}"
        android:hasCode="true">
        <activity
            android:name="com.velox.android.MainActivity"
            android:theme="@android:style/Theme.NoTitleBar"
            android:exported="true"
            android:configChanges="orientation|screenSize|keyboardHidden">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
    )
}

/// 将框架内置的 Android 宿主胶水文件（`AndroidManifest.xml`、`VeloxSurfaceView.kt`、`MainActivity.java`）一键输出至目标目录。
pub fn emit_android_host_project(
    out_dir: &std::path::Path,
    package_name: &str,
    lib_name: &str,
    app_label: &str,
) -> std::io::Result<()> {
    let java_dir = out_dir.join("src").join("com").join("velox").join("android");
    std::fs::create_dir_all(&java_dir)?;
    std::fs::write(
        out_dir.join("AndroidManifest.xml"),
        generate_android_manifest_xml(package_name, app_label),
    )?;
    std::fs::write(out_dir.join("VeloxSurfaceView.kt"), ANDROID_SURFACE_VIEW_KT)?;
    std::fs::write(
        java_dir.join("MainActivity.java"),
        generate_android_main_activity_java(lib_name),
    )?;
    Ok(())
}

/// 为指定业务视图类型一键导出完整的 Android C ABI (`velox_android_app_*` / `velox_mobile_app_*`) 与 `MainActivity` JNI 符号。
#[macro_export]
macro_rules! export_android_app {
    ($app_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn velox_android_app_create(
            width: i16,
            height: i16,
            safe_top: i16,
            safe_bottom: i16,
        ) -> *mut $crate::platform::mobile::MobileEmbedApp {
            $crate::platform::android::AndroidHostBridge::<$app_ty>::new(
                width,
                height,
                safe_top,
                safe_bottom,
            )
            .into_raw()
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_destroy(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::destroy(app);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_backend_is_gpu(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> bool {
            unsafe { $crate::platform::mobile::ffi::backend_is_gpu(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_resize(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            width: i16,
            height: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::resize(app, width, height);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_set_dpi_scale(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            scale: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_dpi_scale(app, scale);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_set_safe_area(
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
        pub unsafe extern "C" fn velox_mobile_app_set_keyboard_inset(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            height_px: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_keyboard_inset(app, height_px);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_push_touch(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            touch_id: u32,
            phase: u8,
            x: i16,
            y: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::push_touch_raw(app, touch_id, phase, x, y);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_handle_back(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
        ) -> bool {
            unsafe { $crate::platform::mobile::ffi::handle_back(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_step_frame(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            dt_ms: u16,
        ) -> usize {
            unsafe { $crate::platform::mobile::ffi::step_frame(app, dt_ms) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_framebuffer_ptr(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> *const u8 {
            unsafe { $crate::platform::mobile::ffi::framebuffer_ptr(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_dirty_rects_ptr(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> *const $crate::platform::mobile::CDirtyRect {
            unsafe { $crate::platform::mobile::ffi::dirty_rects_ptr(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_mobile_app_dirty_rects_count(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> usize {
            unsafe { $crate::platform::mobile::ffi::dirty_rects_count(app) }
        }

        #[unsafe(no_mangle)]
        pub extern "system" fn Java_com_velox_android_MainActivity_nativeCreate(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            width: i32,
            height: i32,
            safe_top: i32,
            safe_bottom: i32,
        ) -> i64 {
            velox_android_app_create(
                width as i16,
                height as i16,
                safe_top as i16,
                safe_bottom as i16,
            ) as usize as i64
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeDestroy(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::destroy(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeResize(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
            width: i32,
            height: i32,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::resize(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                    width as i16,
                    height as i16,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeSetDpiScale(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
            scale: i32,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_dpi_scale(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                    scale as i16,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeSetSafeArea(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
            top: i32,
            right: i32,
            bottom: i32,
            left: i32,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_safe_area(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                    top as i16,
                    right as i16,
                    bottom as i16,
                    left as i16,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeSetKeyboardInset(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
            height: i32,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_keyboard_inset(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                    height as i16,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativePushTouch(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
            pointer_id: i32,
            action: i32,
            x: i32,
            y: i32,
        ) {
            let phase = $crate::platform::mobile::TouchPhase::from_android_action(action as u8);
            unsafe {
                $crate::platform::mobile::ffi::push_touch(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                    pointer_id as u32,
                    phase,
                    x as i16,
                    y as i16,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeHandleBack(
            _env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
        ) -> u8 {
            unsafe {
                u8::from($crate::platform::mobile::ffi::handle_back(
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                ))
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_com_velox_android_MainActivity_nativeStepAndBlitSurface(
            env: *mut core::ffi::c_void,
            _cls: *mut core::ffi::c_void,
            handle: i64,
            surface: *mut core::ffi::c_void,
            dt_ms: i32,
        ) -> i32 {
            unsafe {
                $crate::platform::android::step_and_blit_android_surface(
                    env,
                    handle as usize as *mut $crate::platform::mobile::MobileEmbedApp,
                    surface,
                    dt_ms,
                )
            }
        }
    };
}
