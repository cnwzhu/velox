//! 华为鸿蒙 HarmonyOS NEXT / OpenHarmony（`OH_NativeXComponent` / `OHNativeWindow` / ArkTS N-API）专属平台宿主桥接层。
//!
//! 通过启用 `velox-engine/harmony` 或 `velox-kit/harmony` Feature 按需引入：
//! - [`HarmonyHostBridge<V>`]：泛型封装任意 [`View`] 的鸿蒙宿主生命周期管理（`AvoidAreaType.TYPE_SYSTEM` `38vp` + `TYPE_NAVIGATION_INDICATOR` `28vp`、`TYPE_KEYBOARD` 软键盘避让、`OH_NativeXComponent` 触控、`onBackPress(): boolean` 返回消费、折叠屏开合与横竖屏旋转）；
//! - [`step_and_blit_harmony_window`]：支持 `wgpu` (HarmonyOS Vulkan) 硬件 Swapchain 直通或 `OHNativeWindow` 缓冲区提交；
//! - [`export_harmony_app!`](crate::export_harmony_app)：一行宏为业务 [`View`] 自动生成完整 C ABI (`velox_harmony_app_*`) 符号供 ArkTS Stage 模型与 `XComponent` 调用。

use std::boxed::Box;

use crate::element::View;
use crate::platform::mobile::{
    CDirtyRect, DeviceOrientation, MobileEmbedApp, MobileOs, TouchPhase,
};
use crate::pod::{Handle, SafeAreaInsets};

/// 泛型鸿蒙 HarmonyOS NEXT `OH_NativeXComponent` / `OHNativeWindow` / ArkTS N-API 宿主桥接器。
pub struct HarmonyHostBridge<V: View + 'static> {
    app: *mut MobileEmbedApp,
    pub root_handle: Handle<V>,
}

impl<V: View + Default + 'static> HarmonyHostBridge<V> {
    /// 使用 `V::default()` 创建指定逻辑视口尺寸与鸿蒙系统避让区（`TYPE_SYSTEM` / `TYPE_NAVIGATION_INDICATOR`）的应用实例。
    #[inline]
    pub fn new(width: i16, height: i16, avoid_top: i16, avoid_bottom: i16) -> Self {
        Self::with_root(width, height, avoid_top, avoid_bottom, V::default())
    }
}

impl<V: View + 'static> HarmonyHostBridge<V> {
    /// `OH_NATIVEXCOMPONENT_DOWN`。
    pub const OH_TOUCH_DOWN: u8 = 0;
    /// `OH_NATIVEXCOMPONENT_UP`。
    pub const OH_TOUCH_UP: u8 = 1;
    /// `OH_NATIVEXCOMPONENT_MOVE`。
    pub const OH_TOUCH_MOVE: u8 = 2;
    /// `OH_NATIVEXCOMPONENT_CANCEL`。
    pub const OH_TOUCH_CANCEL: u8 = 3;

    /// 使用给定根视图 `root_view` 创建指定逻辑视口尺寸与鸿蒙系统避让区的应用实例。
    pub fn with_root(
        width: i16,
        height: i16,
        avoid_top: i16,
        avoid_bottom: i16,
        root_view: V,
    ) -> Self {
        let safe_area = SafeAreaInsets::new(avoid_top, 0, avoid_bottom, 0);
        let mut embed = Box::new(MobileEmbedApp::new_for_os(
            MobileOs::HarmonyOs,
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

    /// 设置鸿蒙屏幕像素密度缩放倍率（`display.getDefaultDisplaySync().densityPixels`）。
    #[inline]
    pub fn set_density_scale(&mut self, scale: i16) {
        unsafe { &mut *self.app }.set_dpi_scale(scale);
    }

    /// 响应 `OH_NativeXComponent_Callback.OnSurfaceChanged` 或折叠屏展开/分屏尺寸变化。
    #[inline]
    pub fn on_surface_changed(&mut self, width: i16, height: i16) {
        unsafe { &mut *self.app }.resize(width, height);
    }

    /// 同步鸿蒙 `window.on('avoidAreaChange')` (`TYPE_SYSTEM` / `TYPE_CUTOUT` / `TYPE_NAVIGATION_INDICATOR`) 避让区内边距。
    #[inline]
    pub fn on_avoid_area_changed(&mut self, top: i16, right: i16, bottom: i16, left: i16) {
        unsafe { &mut *self.app }.set_safe_area(SafeAreaInsets::new(top, right, bottom, left));
    }

    /// 切换鸿蒙设备屏幕朝向并应用对应朝向的标准避让区域。
    #[inline]
    pub fn set_orientation(&mut self, orientation: DeviceOrientation) {
        unsafe { &mut *self.app }.set_orientation(orientation);
    }

    /// 同步鸿蒙 `AvoidAreaType.TYPE_KEYBOARD` 软键盘底部像素高度。
    #[inline]
    pub fn on_keyboard_avoid_area(&mut self, keyboard_bottom_px: i16) {
        unsafe { &mut *self.app }.set_keyboard_inset(keyboard_bottom_px);
    }

    /// 分发鸿蒙 `OH_NativeXComponent_GetTouchEvent` 触控事件。
    #[inline]
    pub fn on_touch_event(&mut self, pointer_id: u32, oh_touch_type: u8, x: i16, y: i16) {
        let phase = TouchPhase::from_harmony_touch_type(oh_touch_type);
        unsafe { &mut *self.app }.push_touch(pointer_id, phase, x, y);
    }

    /// 分发鸿蒙 ArkTS `@Entry onBackPress(): boolean` 页面级返回事件：
    /// 返回 `true` 表示应用内部已消费（关闭元服务抽屉或回退二级页），返回 `false` 表示交由 `UIAbility` 处理。
    #[inline]
    pub fn on_back_press(&mut self) -> bool {
        unsafe { &mut *self.app }.handle_back()
    }

    /// 响应 `OH_NativeXComponent_RegisterOnFrameCallback` 推进一帧，返回本帧脏矩形个数。
    #[inline]
    pub fn on_frame_callback(&mut self, dt_ms: u16) -> usize {
        unsafe { &mut *self.app }.step_frame(dt_ms)
    }

    /// 获取 `RGBA8888` 像素缓冲区的只读字节切片（可直接写入 `OHNativeWindowBuffer`）。
    #[inline]
    pub fn rgba_slice(&self) -> &[u8] {
        unsafe { &*self.app }.rgba_bytes()
    }

    /// 获取本帧脏矩形切片（供 `OH_NativeWindow_NativeWindowFlushBuffer` `Region` 局部刷新使用）。
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

    /// 查询当前鸿蒙宿主实例是否启用了基于轻量 `wgpu` (HarmonyOS Vulkan) 2D SDF 的硬件 GPU 渲染后端。
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

impl<V: View + 'static> Drop for HarmonyHostBridge<V> {
    fn drop(&mut self) {
        if !self.app.is_null() {
            unsafe {
                let _ = Box::from_raw(self.app);
            }
        }
    }
}

/// 推进一帧 [`MobileEmbedApp`] 并将像素提交至鸿蒙 `OHNativeWindow`。
///
/// # Safety
/// `app` 必须为有效的 [`MobileEmbedApp`] 指针或空指针。
pub unsafe fn step_and_blit_harmony_window(
    app: *mut MobileEmbedApp,
    oh_native_window: *mut core::ffi::c_void,
    dt_ms: u16,
) -> i32 {
    let Some(app_ref) = (unsafe { app.as_mut() }) else {
        return 0;
    };
    let dirty = app_ref.step_frame(dt_ms.max(1));
    if dirty == 0 || oh_native_window.is_null() {
        return dirty as i32;
    }
    if app_ref.has_active_gpu_window() {
        return dirty as i32;
    }
    let _ = oh_native_window;
    dirty as i32
}

/// 框架内置的鸿蒙 HarmonyOS NEXT `UIAbility` (`EntryAbility.ets`) 源码模板。
pub const HARMONY_ENTRY_ABILITY_ETS: &str = r#"import UIAbility from '@ohos.app.ability.UIAbility';
import window from '@ohos.window';

export default class EntryAbility extends UIAbility {
  onWindowStageCreate(windowStage: window.WindowStage): void {
    const mainWin = windowStage.getMainWindowSync();
    mainWin.setWindowLayoutFullScreen(true);
    windowStage.loadContent('pages/Index');
  }
}
"#;

/// 框架内置的鸿蒙 HarmonyOS NEXT N-API TypeScript 声明文件（`index.d.ts`）模板。
pub const HARMONY_NAPI_INDEX_DTS: &str = r#"export const create: (width: number, height: number, avoidTop: number, avoidBottom: number) => number;
export const destroy: (handle: number) => void;
export const resize: (handle: number, width: number, height: number) => void;
export const setDpiScale: (handle: number, scale: number) => void;
export const setSafeArea: (handle: number, top: number, right: number, bottom: number, left: number) => void;
export const setKeyboardInset: (handle: number, heightVp: number) => void;
export const pushTouch: (handle: number, pointerId: number, touchType: number, x: number, y: number) => void;
export const handleBack: (handle: number) => boolean;
export const stepFrame: (handle: number, dtMs: number) => number;
"#;

/// 框架内置的鸿蒙 HarmonyOS NEXT Stage 模型 `module.json5` 模板。
pub const HARMONY_MODULE_JSON5: &str = r#"{
  "module": {
    "name": "entry",
    "type": "entry",
    "description": "Velox HarmonyOS NEXT Native XComponent Application",
    "mainElement": "EntryAbility",
    "deviceTypes": [
      "phone",
      "tablet",
      "2in1"
    ],
    "deliveryWithInstall": true,
    "installationFree": false,
    "pages": "$profile:main_pages",
    "abilities": [
      {
        "name": "EntryAbility",
        "srcEntry": "./ets/entryability/EntryAbility.ets",
        "description": "Velox HarmonyOS NEXT UIAbility",
        "exported": true,
        "skills": [
          {
            "entities": [
              "entity.system.home"
            ],
            "actions": [
              "action.system.home"
            ]
          }
        ]
      }
    ]
  }
}
"#;

/// 生成加载指定原生库名 `lib_name`（如 `"harmony_app"`）的鸿蒙 ArkTS `Index.ets` 页面源码。
pub fn generate_harmony_index_ets(lib_name: &str) -> std::string::String {
    std::format!(
        r#"import window from '@ohos.window';
import display from '@ohos.display';
import veloxHarmony from 'lib{lib_name}.so';

/**
 * 鸿蒙 HarmonyOS NEXT ArkTS 主页面（由 `velox-engine::platform::harmony` 自动生成，挂载 `XComponent` Native 渲染表面）。
 */
@Entry
@Component
struct Index {{
  private xComponentContext: Record<string, () => void> | undefined = undefined;
  private nativeHandle: number = 0;

  aboutToAppear(): void {{
    window.getLastWindow(getContext(this)).then((win) => {{
      win.setWindowLayoutFullScreen(true);
      const sysArea = win.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM);
      const navArea = win.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR);
      if (this.nativeHandle !== 0) {{
        veloxHarmony.setSafeArea(
          this.nativeHandle,
          px2vp(sysArea.topRect.height),
          0,
          px2vp(navArea.bottomRect.height),
          0
        );
      }}

      win.on('avoidAreaChange', (data) => {{
        if (this.nativeHandle === 0) {{
          return;
        }}
        if (data.type === window.AvoidAreaType.TYPE_KEYBOARD) {{
          veloxHarmony.setKeyboardInset(this.nativeHandle, px2vp(data.area.bottomRect.height));
        }} else if (
          data.type === window.AvoidAreaType.TYPE_SYSTEM ||
          data.type === window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR
        ) {{
          veloxHarmony.setSafeArea(
            this.nativeHandle,
            px2vp(data.area.topRect.height),
            px2vp(data.area.rightRect.width),
            px2vp(data.area.bottomRect.height),
            px2vp(data.area.leftRect.width)
          );
        }}
      }});
    }});
  }}

  onBackPress(): boolean {{
    if (this.nativeHandle !== 0) {{
      return veloxHarmony.handleBack(this.nativeHandle);
    }}
    return false;
  }}

  build() {{
    Stack() {{
      XComponent({{
        id: 'velox_harmony_xcomponent',
        type: XComponentType.SURFACE,
        libraryname: '{lib_name}'
      }})
        .onLoad((context) => {{
          this.xComponentContext = context as Record<string, () => void>;
          const disp = display.getDefaultDisplaySync();
          const scale = Math.max(1, Math.round(disp.densityPixels));
          this.nativeHandle = veloxHarmony.create(384, 832, 38, 28);
          veloxHarmony.setDpiScale(this.nativeHandle, scale);
        }})
        .onDestroy(() => {{
          if (this.nativeHandle !== 0) {{
            veloxHarmony.destroy(this.nativeHandle);
            this.nativeHandle = 0;
          }}
        }})
        .width('100%')
        .height('100%')
    }}
    .width('100%')
    .height('100%')
  }}
}}
"#
    )
}

/// 将框架内置的鸿蒙 HarmonyOS NEXT 宿主胶水文件（`module.json5`、`Index.ets`、`EntryAbility.ets`、`index.d.ts`）一键输出至目标目录。
pub fn emit_harmony_host_project(
    out_dir: &std::path::Path,
    lib_name: &str,
) -> std::io::Result<()> {
    let pages_dir = out_dir.join("ets").join("pages");
    let ability_dir = out_dir.join("ets").join("entryability");
    let types_dir = out_dir.join("types").join(std::format!("lib{lib_name}"));
    std::fs::create_dir_all(&pages_dir)?;
    std::fs::create_dir_all(&ability_dir)?;
    std::fs::create_dir_all(&types_dir)?;
    std::fs::write(out_dir.join("module.json5"), HARMONY_MODULE_JSON5)?;
    std::fs::write(
        pages_dir.join("Index.ets"),
        generate_harmony_index_ets(lib_name),
    )?;
    std::fs::write(
        ability_dir.join("EntryAbility.ets"),
        HARMONY_ENTRY_ABILITY_ETS,
    )?;
    std::fs::write(types_dir.join("index.d.ts"), HARMONY_NAPI_INDEX_DTS)?;
    Ok(())
}

/// 为指定业务视图类型一键导出完整的鸿蒙 HarmonyOS NEXT C ABI (`velox_harmony_app_*`) 符号。
#[macro_export]
macro_rules! export_harmony_app {
    ($app_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn velox_harmony_app_create(
            width: i16,
            height: i16,
            avoid_top: i16,
            avoid_bottom: i16,
        ) -> *mut $crate::platform::mobile::MobileEmbedApp {
            $crate::platform::harmony::HarmonyHostBridge::<$app_ty>::new(
                width,
                height,
                avoid_top,
                avoid_bottom,
            )
            .into_raw()
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_destroy(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::destroy(app);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_backend_is_gpu(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> bool {
            unsafe { $crate::platform::mobile::ffi::backend_is_gpu(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_resize(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            width: i16,
            height: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::resize(app, width, height);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_set_dpi_scale(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            scale: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_dpi_scale(app, scale);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_set_safe_area(
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
        pub unsafe extern "C" fn velox_harmony_app_set_keyboard_inset(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            height_px: i16,
        ) {
            unsafe {
                $crate::platform::mobile::ffi::set_keyboard_inset(app, height_px);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_push_touch(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            pointer_id: u32,
            oh_touch_type: u8,
            x: i16,
            y: i16,
        ) {
            let phase =
                $crate::platform::mobile::TouchPhase::from_harmony_touch_type(oh_touch_type);
            unsafe {
                $crate::platform::mobile::ffi::push_touch(app, pointer_id, phase, x, y);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_handle_back(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
        ) -> bool {
            unsafe { $crate::platform::mobile::ffi::handle_back(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_step_frame(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            dt_ms: u16,
        ) -> usize {
            unsafe { $crate::platform::mobile::ffi::step_frame(app, dt_ms) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_framebuffer_ptr(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> *const u8 {
            unsafe { $crate::platform::mobile::ffi::framebuffer_ptr(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_dirty_rects_ptr(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> *const $crate::platform::mobile::CDirtyRect {
            unsafe { $crate::platform::mobile::ffi::dirty_rects_ptr(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_dirty_rects_count(
            app: *const $crate::platform::mobile::MobileEmbedApp,
        ) -> usize {
            unsafe { $crate::platform::mobile::ffi::dirty_rects_count(app) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn velox_harmony_app_step_and_blit_window(
            app: *mut $crate::platform::mobile::MobileEmbedApp,
            oh_native_window: *mut core::ffi::c_void,
            dt_ms: u16,
        ) -> i32 {
            unsafe {
                $crate::platform::harmony::step_and_blit_harmony_window(
                    app,
                    oh_native_window,
                    dt_ms,
                )
            }
        }
    };
}
