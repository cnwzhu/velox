//! Velox WebAssembly (`wasm32` HTML5 `<canvas>`) 网页端应用示例（基于 `velox-kit`）。
//!
//! 展示如何使用 `velox-kit` 构建零 DOM 节点开销的 `<canvas>` Web 应用：
//! - 依托 `Rgba8888` 像素格式与 WebAssembly 线性内存 `Uint8ClampedArray` 零拷贝视图；
//! - 依托 8 槽位脏矩形追踪器实现 `ctx.putImageData(img, 0, 0, rx, ry, rw, rh)` 局部重绘；
//! - 完整支持浏览器 `PointerEvent`（点击/拖拽/悬停）、`WheelEvent`（滚轮平滑滚动）、`ResizeObserver` 与 `popstate` 浏览器后退路由。

use velox_kit::platform::web::{WebCanvasApp, generate_web_index_html};
use velox_kit::prelude::*;

/// Web 应用的页面路由枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WebRoute {
    /// 仪表盘主页。
    Overview,
    /// 实时性能分析二级页（支持浏览器 `popstate` 后退返回）。
    Analytics,
    /// 偏好设置二级页。
    Preferences,
}

impl WebRoute {
    #[inline(always)]
    pub const fn tab_index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Analytics => 1,
            Self::Preferences => 2,
        }
    }

    #[inline(always)]
    pub const fn from_tab_index(idx: usize) -> Self {
        match idx {
            0 => Self::Overview,
            1 => Self::Analytics,
            _ => Self::Preferences,
        }
    }

    #[inline(always)]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Overview => "Velox WebAssembly Studio",
            Self::Analytics => "Web Canvas Telemetry",
            Self::Preferences => "Browser Preferences",
        }
    }
}

/// Web `<canvas>` 应用主视图状态。
pub struct WebDashboardApp {
    /// 页面路由栈（深度 4，对接浏览器 History `popstate` 后退）。
    pub nav: Navigator<WebRoute, 4>,
    /// 吞吐率指标百分比（`0..=100`）。
    pub throughput_pct: u8,
    /// 高帧率模式开关。
    pub high_fps_mode: bool,
    /// 底部抽屉弹窗是否打开。
    pub drawer_open: bool,
    /// 累计浏览器后退触发次数。
    pub popstate_count: u32,
}

impl Default for WebDashboardApp {
    fn default() -> Self {
        Self::new()
    }
}

impl WebDashboardApp {
    /// 创建默认 Web 应用状态。
    pub const fn new() -> Self {
        Self {
            nav: Navigator::new(WebRoute::Overview),
            throughput_pct: 58,
            high_fps_mode: true,
            drawer_open: false,
            popstate_count: 0,
        }
    }
}

impl View for WebDashboardApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let current_route = self.nav.current();
        let can_pop = self.nav.can_pop();
        let drawer_open = self.drawer_open;
        let can_intercept_back = drawer_open || can_pop;

        let on_popstate_back = cx.listener(|this, dir: SwipeDir, _acx| {
            if dir == SwipeDir::Right {
                if this.drawer_open {
                    this.drawer_open = false;
                    this.popstate_count += 1;
                } else if this.nav.can_pop() {
                    this.nav.pop(180);
                    this.popstate_count += 1;
                }
            }
        });

        let raw_nav_back = cx.action(|this, _acx| {
            if this.nav.can_pop() {
                this.nav.pop(180);
                this.popstate_count += 1;
            }
        });

        let raw_toggle_drawer = cx.action(|this, _acx| {
            this.drawer_open = !this.drawer_open;
        });

        let raw_dismiss_drawer = cx.action(|this, _acx| {
            this.drawer_open = false;
            this.popstate_count += 1;
        });

        let on_tab_select = cx.listener(|this, idx: usize, _acx| {
            let target = WebRoute::from_tab_index(idx);
            if target != this.nav.current() {
                if target == WebRoute::Overview && this.nav.can_pop() {
                    this.nav.pop(180);
                } else {
                    this.nav.push(target, PageTransition::Fade, 180);
                }
            }
        });

        // 1. 顶部 Web 应用导航栏（Web 端默认 safe_area 为 0）
        let mut navbar = NavBar::new(100, current_route.title())
            .safe_top(false)
            .action("Inspector", raw_toggle_drawer);
        if can_pop {
            navbar = navbar.back("< Back", raw_nav_back);
        }

        // 2. 主体内容区
        let body = match current_route {
            WebRoute::Overview => {
                let throughput = self.throughput_pct;
                let on_boost = cx.listener(|this, _: ClickEvent, _acx| {
                    this.throughput_pct = (this.throughput_pct + 12).min(100);
                });
                let on_open_analytics = cx.listener(|this, _: ClickEvent, _acx| {
                    this.nav
                        .push(WebRoute::Analytics, PageTransition::SlideLeft, 180);
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(20)
                    .gap(14)
                    .items_center()
                    .child(
                        cx,
                        ArcGauge::new(110, 120, throughput).thickness(12),
                    )
                    .child(
                        cx,
                        Button::new(111, "Scale Throughput +12%")
                            .primary()
                            .w(180)
                            .h(38)
                            .on_click(on_boost),
                    )
                    .child(
                        cx,
                        Button::new(112, "Open Analytics")
                            .secondary()
                            .w(180)
                            .h(38)
                            .on_click(on_open_analytics),
                    )
            }
            WebRoute::Analytics => {
                let on_throttle = cx.listener(|this, _: ClickEvent, _acx| {
                    this.throughput_pct = this.throughput_pct.saturating_sub(10);
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(20)
                    .gap(14)
                    .items_center()
                    .child(cx, text("Zero-Copy Uint8ClampedArray + putImageData Dirty Rects"))
                    .child(
                        cx,
                        Button::new(121, "Throttle -10%")
                            .primary()
                            .w(180)
                            .h(38)
                            .on_click(on_throttle),
                    )
            }
            WebRoute::Preferences => {
                let high_fps = self.high_fps_mode;
                let on_toggle_fps = cx.listener(|this, val: bool, _acx| {
                    this.high_fps_mode = val;
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(20)
                    .gap(14)
                    .items_center()
                    .child(cx, text("120Hz requestAnimationFrame Sync"))
                    .child(
                        cx,
                        Switch::new(131, high_fps).on_change(on_toggle_fps),
                    )
            }
        };

        // 3. 底部导航标签栏
        let tab_bar = TabBar::new(
            200,
            &["Overview", "Analytics", "Preferences"],
            current_route.tab_index(),
        )
        .safe_bottom(false)
        .on_select(on_tab_select);

        let mut root = div()
            .size_full()
            .flex_col()
            .justify_between()
            .bg(cx.theme().background)
            .child(cx, navbar)
            .child(cx, body)
            .child(cx, tab_bar);

        if can_intercept_back {
            root = root.on_swipe(cx, on_popstate_back);
        }

        if drawer_open {
            root = root.child(
                cx,
                BottomSheet::new(300, "WASM Memory & Damage Inspector", 130)
                    .content(cx, text("Click backdrop or trigger browser Back to close"))
                    .on_dismiss(raw_dismiss_drawer),
            );
        }

        root.into_element(cx)
    }
}

/// 创建挂载了 [`WebDashboardApp`] 的 [`WebCanvasApp`] 实例。
pub fn create_web_app(width: i16, height: i16) -> (WebCanvasApp, Handle<WebDashboardApp>) {
    let mut app = WebCanvasApp::new(width, height);
    let handle = app.engine.mount_root(WebDashboardApp::new());
    (app, handle)
}

/// 生成可直接由浏览器加载的 HTML5 `<canvas>` 宿主页面代码。
pub fn generate_index_html(wasm_url: &str, width: u16, height: u16) -> String {
    generate_web_index_html(wasm_url, width, height)
}

// 一行宏导出完整 WebAssembly `<canvas>` C ABI (`velox_wasm_app_init` / `velox_web_app_*`) 符号
velox_kit::export_web_app!(WebDashboardApp);
