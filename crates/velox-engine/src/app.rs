//! 极简桌面应用启动层 `velox_engine::app`（参照 `gpui-kit` 启动范式）。
//!
//! 让桌面端启动代码精简到极致（仅需数行）：
//! ```no_run
//! use velox_engine::prelude::*;
//!
//! struct HelloWorld;
//! impl View for HelloWorld {
//!     fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
//!         div().size_full().items_center().justify_center().child(cx, text("Hello, World!")).into_element(cx)
//!     }
//! }
//!
//! fn main() {
//!     velox_engine::app::application().run(|cx| {
//!         velox_engine::app::init(cx);
//!         velox_engine::app::open_window(
//!             WindowOptions::new("Hello Velox").size(480, 320),
//!             cx,
//!             |cx| cx.new(|_| HelloWorld),
//!         )
//!         .expect("Failed to open window");
//!     });
//! }
//! ```

use core::fmt;
use std::string::String;
use std::sync::Arc;

use crate::async_rt::{NativeStdScheduler, SchedulerBackend, Task};
use crate::element::View;
use crate::engine::VeloxEngine;
use crate::i18n::{DynamicCatalog, Locale, StaticCatalog};
use crate::platform::winit::WinitPlatform;
use crate::platform::{DesktopRenderBackend, Platform};
use crate::pod::{CoreTheme, Handle};

/// 窗口打开与初始化过程中的错误类型。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AppError {
    /// 单窗口桌面运行器中重复打开主窗口。
    WindowAlreadyOpen,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowAlreadyOpen => {
                write!(f, "primary window has already been opened in this AppContext")
            }
        }
    }
}

impl std::error::Error for AppError {}

/// 桌面窗口创建配置项（参照 `gpui-kit` `WindowOptions`）。
#[derive(Clone, Debug)]
pub struct WindowOptions {
    /// 窗口标题。
    pub title: String,
    /// 初始逻辑宽度（像素，默认 `960`）。
    pub width: i16,
    /// 初始逻辑高度（像素，默认 `640`）。
    pub height: i16,
    /// 可选窗口级主题覆盖（未指定时继承 [`Application`] 全局主题，默认暗色）。
    pub theme: Option<CoreTheme>,
    /// 可选窗口级语言区域覆盖（未指定时继承 [`Application`] 全局语言）。
    pub locale: Option<Locale>,
    /// 可选固定 HiDPI 缩放倍率（`None` 时自动跟随操作系统缩放）。
    pub dpi_scale: Option<usize>,
    /// 桌面渲染后端（默认 [`DesktopRenderBackend::default_desktop`]）。
    pub backend: Option<DesktopRenderBackend>,
    /// 可选默认滚轮回退目标列表 ID。
    pub scroll_list_id: Option<u32>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: String::from("Velox Application"),
            width: 960,
            height: 640,
            theme: None,
            locale: None,
            dpi_scale: None,
            backend: None,
            scroll_list_id: None,
        }
    }
}

impl WindowOptions {
    /// 使用指定窗口标题创建默认尺寸（`960x640`）的窗口配置。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// 设置窗口标题。
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// 设置窗口初始逻辑宽高 `(width, height)`。
    #[inline]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 设置窗口初始主题。
    #[inline]
    pub const fn theme(mut self, theme: CoreTheme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// 设置窗口初始语言区域。
    #[inline]
    pub const fn locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// 显式锁定 HiDPI 缩放倍率（如 `1` / `2`）。
    #[inline]
    pub const fn dpi_scale(mut self, scale: usize) -> Self {
        self.dpi_scale = Some(scale);
        self
    }

    /// 配置渲染后端（GPU 或 Software）。
    #[inline]
    pub const fn backend(mut self, backend: DesktopRenderBackend) -> Self {
        self.backend = Some(backend);
        self
    }

    /// 使用内置多线程软件光栅渲染器。
    #[inline]
    pub const fn software(mut self) -> Self {
        self.backend = Some(DesktopRenderBackend::Software);
        self
    }

    /// 使用硬件加速 `wgpu` 2D SDF 渲染器。
    #[inline]
    pub const fn gpu(mut self) -> Self {
        self.backend = Some(DesktopRenderBackend::Wgpu);
        self
    }

    /// 绑定未命中滚动容器时的回退滚动列表 ID。
    #[inline]
    pub const fn scroll_list_id(mut self, id: u32) -> Self {
        self.scroll_list_id = Some(id);
        self
    }
}

/// 已打开窗口的类型化根视图句柄。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WindowHandle<V> {
    root: Handle<V>,
}

impl<V> WindowHandle<V> {
    /// 获取窗口根视图的强类型 [`Handle<V>`]。
    #[inline(always)]
    pub const fn root(&self) -> Handle<V> {
        self.root
    }
}

/// 窗口初始化闭包上下文（在 [`open_window`] 回调中提供 `cx.new(|cx| ...)` 与子视图挂载能力）。
pub struct WindowContext<'a> {
    engine: &'a mut VeloxEngine,
}

impl<'a> WindowContext<'a> {
    /// 构造并挂载窗口根视图（参照 `gpui-kit` `|cx| cx.new(|_| Example)` 写法）。
    #[inline]
    pub fn new<V: View>(&mut self, build: impl FnOnce(&mut Self) -> V) -> Handle<V> {
        let view = build(self);
        self.engine.mount_root(view)
    }

    /// 注册一个独立的子视图（L2 局部重排边界），返回其 [`Handle<V>`]。
    #[inline]
    pub fn insert_view<V: View>(&mut self, view: V) -> Handle<V> {
        self.engine.insert_view(view)
    }

    /// 直接挂载已构造好的根视图实例。
    #[inline]
    pub fn mount_root<V: View>(&mut self, view: V) -> Handle<V> {
        self.engine.mount_root(view)
    }

    /// 获取当前窗口绑定的 [`VeloxEngine`] 只读引用。
    #[inline(always)]
    pub fn engine(&self) -> &VeloxEngine {
        self.engine
    }

    /// 获取当前窗口绑定的 [`VeloxEngine`] 可变引用（可用于挂载复合子视图、设置初始状态或派生启动任务）。
    #[inline(always)]
    pub fn engine_mut(&mut self) -> &mut VeloxEngine {
        self.engine
    }

    /// 返回当前引擎绑定的调度器名称。
    #[inline]
    pub fn scheduler_name(&self) -> &'static str {
        self.engine.scheduler_name()
    }

    /// 在窗口初始化阶段为指定视图派生启动期前景异步任务。
    #[inline]
    pub fn spawn_for<V, T, F, Fut>(&mut self, handle: Handle<V>, f: F) -> Task<T>
    where
        V: View,
        T: 'static,
        F: FnOnce(Handle<V>, crate::async_rt::AsyncApp) -> Fut,
        Fut: core::future::Future<Output = T> + 'static,
    {
        self.engine.spawn_for(handle, f)
    }
}

/// 无头构建完成后返回的已初始化窗口与引擎（供自动化测试、`--headless` 与 `--save-png` 零弹窗复用）。
pub struct BuiltWindow {
    pub options: WindowOptions,
    pub engine: VeloxEngine,
}

/// 应用级启动上下文（传入 [`Application::run`] 回调，参照 `gpui-kit` `|cx| { init(cx); open_window(..., cx, ...); }`）。
pub struct AppContext {
    scheduler: Arc<dyn SchedulerBackend>,
    theme: CoreTheme,
    locale: Option<Locale>,
    static_catalog: Option<&'static StaticCatalog>,
    dynamic_catalog: Option<Arc<DynamicCatalog>>,
    backend: DesktopRenderBackend,
    dpi_scale: Option<usize>,
    primary_window: Option<BuiltWindow>,
}

impl AppContext {
    /// 设置全局默认异步调度器后端（后续打开的窗口将自动继承）。
    pub fn set_scheduler(&mut self, scheduler: Arc<dyn SchedulerBackend>) {
        self.scheduler = scheduler;
    }

    /// 返回当前应用配置的异步调度器名称。
    #[inline]
    pub fn scheduler_name(&self) -> &'static str {
        self.scheduler.name()
    }

    /// 设置全局默认主题。
    #[inline]
    pub fn set_theme(&mut self, theme: CoreTheme) {
        self.theme = theme;
    }

    /// 设置全局默认语言区域。
    #[inline]
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = Some(locale);
    }

    /// 注册全局静态多语言翻译词典。
    #[inline]
    pub fn set_i18n_catalog(&mut self, catalog: &'static StaticCatalog) {
        self.static_catalog = Some(catalog);
    }

    /// 注册全局动态多语言翻译词典。
    #[inline]
    pub fn set_dynamic_catalog(&mut self, catalog: Arc<DynamicCatalog>) {
        self.dynamic_catalog = Some(catalog);
    }

    /// 设置默认桌面渲染后端。
    #[inline]
    pub fn set_backend(&mut self, backend: DesktopRenderBackend) {
        self.backend = backend;
    }

    /// 打开主桌面窗口并挂载根视图。
    pub fn open_window<V: View>(
        &mut self,
        options: WindowOptions,
        build_root: impl FnOnce(&mut WindowContext<'_>) -> Handle<V>,
    ) -> Result<WindowHandle<V>, AppError> {
        if self.primary_window.is_some() {
            return Err(AppError::WindowAlreadyOpen);
        }

        let w = options.width.max(1);
        let h = options.height.max(1);
        let mut engine = VeloxEngine::new(w, h).with_scheduler(Arc::clone(&self.scheduler));
        engine.set_theme(options.theme.unwrap_or(self.theme));
        if let Some(loc) = options.locale.or(self.locale) {
            engine.set_locale(loc);
        }
        if let Some(cat) = self.static_catalog {
            engine.set_i18n_catalog(cat);
        }
        if let Some(cat) = &self.dynamic_catalog {
            engine.set_dynamic_catalog(Arc::clone(cat));
        }

        let root = {
            let mut wcx = WindowContext {
                engine: &mut engine,
            };
            build_root(&mut wcx)
        };

        let mut merged_opts = options;
        if merged_opts.backend.is_none() {
            merged_opts.backend = Some(self.backend);
        }
        if merged_opts.dpi_scale.is_none() {
            merged_opts.dpi_scale = self.dpi_scale;
        }

        self.primary_window = Some(BuiltWindow {
            options: merged_opts,
            engine,
        });
        Ok(WindowHandle { root })
    }
}

/// 极简跨平台桌面应用启动构建器（由 [`application`] 创建，参照 `gpui-kit`）。
pub struct Application {
    scheduler: Arc<dyn SchedulerBackend>,
    theme: CoreTheme,
    locale: Option<Locale>,
    static_catalog: Option<&'static StaticCatalog>,
    dynamic_catalog: Option<Arc<DynamicCatalog>>,
    backend: DesktopRenderBackend,
    dpi_scale: Option<usize>,
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

impl Application {
    /// 创建一个使用内置零依赖 [`NativeStdScheduler`] 与默认暗色主题的 [`Application`] 构建器。
    pub fn new() -> Self {
        Self {
            scheduler: NativeStdScheduler::global(),
            theme: CoreTheme::dark(),
            locale: None,
            static_catalog: None,
            dynamic_catalog: None,
            backend: DesktopRenderBackend::default_desktop(),
            dpi_scale: None,
        }
    }

    /// 注入自定义可插拔异步调度器后端（如 `velox-async` 的 `TokioScheduler::global()` 或 `SmolScheduler::global()`）。
    pub fn with_scheduler(mut self, scheduler: Arc<dyn SchedulerBackend>) -> Self {
        self.scheduler = scheduler;
        self
    }

    /// 配置应用级默认主题。
    #[inline]
    pub const fn with_theme(mut self, theme: CoreTheme) -> Self {
        self.theme = theme;
        self
    }

    /// 配置应用级默认语言区域。
    #[inline]
    pub const fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// 注册应用级静态多语言翻译目录。
    #[inline]
    pub const fn with_i18n_catalog(mut self, catalog: &'static StaticCatalog) -> Self {
        self.static_catalog = Some(catalog);
        self
    }

    /// 注册应用级动态多语言翻译目录。
    pub fn with_dynamic_catalog(mut self, catalog: Arc<DynamicCatalog>) -> Self {
        self.dynamic_catalog = Some(catalog);
        self
    }

    /// 配置应用级默认桌面渲染后端（GPU / Software）。
    #[inline]
    pub const fn with_backend(mut self, backend: DesktopRenderBackend) -> Self {
        self.backend = backend;
        self
    }

    /// 显式锁定 HiDPI 缩放倍率。
    #[inline]
    pub const fn with_dpi_scale(mut self, scale: usize) -> Self {
        self.dpi_scale = Some(scale);
        self
    }

    fn into_context(self) -> AppContext {
        AppContext {
            scheduler: self.scheduler,
            theme: self.theme,
            locale: self.locale,
            static_catalog: self.static_catalog,
            dynamic_catalog: self.dynamic_catalog,
            backend: self.backend,
            dpi_scale: self.dpi_scale,
            primary_window: None,
        }
    }

    /// 以无头模式执行应用初始化回调并返回已配置好的 [`BuiltWindow`]（不弹出真实窗口，专供测试、CI 与 `--headless`）。
    pub fn build_headless(self, setup: impl FnOnce(&mut AppContext)) -> Option<BuiltWindow> {
        let mut cx = self.into_context();
        setup(&mut cx);
        cx.primary_window
    }

    /// 启动桌面应用主事件循环：执行 `setup` 打开窗口并自动接管原生 `winit` 事件循环。
    pub fn run(self, setup: impl FnOnce(&mut AppContext)) {
        let Some(built) = self.build_headless(setup) else {
            return;
        };
        let BuiltWindow { options, engine } = built;
        let mut plat = WinitPlatform::new(
            options.title,
            options.width.max(1) as usize,
            options.height.max(1) as usize,
        )
        .with_backend(
            options
                .backend
                .unwrap_or_else(DesktopRenderBackend::default_desktop),
        );
        if let Some(scale) = options.dpi_scale {
            plat = plat.with_dpi_scale(scale);
        }
        if let Some(scroll_id) = options.scroll_list_id {
            plat.run(engine, Some(scroll_id));
        } else {
            plat.run_event_loop(engine);
        }
    }
}

/// 创建桌面应用启动构建器（对齐 `gpui_kit::application()`）。
#[inline]
pub fn application() -> Application {
    Application::new()
}

/// 初始化 Velox 应用默认环境（对齐 `gpui_kit::init(cx)` 习惯；保留作为扩展初始化锚点）。
#[inline]
pub fn init(_cx: &mut AppContext) {}

/// 在应用启动回调中打开桌面窗口并挂载根视图（对齐 `gpui_kit::open_window(options, cx, |cx| cx.new(...))`）。
#[inline]
pub fn open_window<V: View>(
    options: WindowOptions,
    cx: &mut AppContext,
    build_root: impl FnOnce(&mut WindowContext<'_>) -> Handle<V>,
) -> Result<WindowHandle<V>, AppError> {
    cx.open_window(options, build_root)
}
