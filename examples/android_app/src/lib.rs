//! Velox Android 原生移动应用示例（基于 `velox-kit/android` 平台特性）。
//!
//! 展示如何使用 `velox-kit` 构建完整的 Android 手机端应用，仅需编写纯声明式 [`View`] 业务代码，
//! 底层 `ANativeWindow` 渲染、JNI 导出与系统生命周期全部由框架层 [`velox_kit::platform::android`] 提供：
//! - 异形屏状态栏（Status Bar / Display Cutout）与底部手势横条（Navigation Bar）[`SafeAreaInsets::android_portrait`] 自动避让；
//! - 软键盘（IME）弹出与收起时的底部高度动态避让（`keyboard_inset`）；
//! - 移动端专属导航组件 [`NavBar`]、[`TabBar`]、[`BottomSheet`] 与 [`Navigator`] 页面栈；
//! - 左边缘右滑返回手势与 Android 物理返回键（`OnBackInvokedCallback` / `onBackPressed`）消费判定；
//! - 通过 [`velox_kit::export_android_app!`] 一键生成 C ABI 与 `com.velox.android.MainActivity` JNI 桥接入口。

use velox_kit::prelude::*;

/// Android 示例应用的页面路由枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AndroidRoute {
    /// 首页仪表盘。
    Dashboard,
    /// 设备详情二级页（支持左边缘右滑返回与 Android 物理返回键回退）。
    DeviceDetail,
    /// 系统设置页（包含开关与软键盘输入避让验证）。
    Settings,
}

impl AndroidRoute {
    #[inline(always)]
    pub const fn tab_index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::DeviceDetail => 1,
            Self::Settings => 2,
        }
    }

    #[inline(always)]
    pub const fn from_tab_index(idx: usize) -> Self {
        match idx {
            0 => Self::Dashboard,
            1 => Self::DeviceDetail,
            _ => Self::Settings,
        }
    }

    #[inline(always)]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Velox Android",
            Self::DeviceDetail => "Device Telemetry",
            Self::Settings => "Android Settings",
        }
    }
}

/// Android 移动应用主视图状态。
pub struct AndroidMobileApp {
    /// 页面路由栈（最大深度 4）。
    pub nav: Navigator<AndroidRoute, 4>,
    /// 电池/性能百分比（`0..=100`）。
    pub battery_pct: u8,
    /// 省电模式开关。
    pub power_save: bool,
    /// 底部快捷操作抽屉（`BottomSheet`）是否打开。
    pub sheet_open: bool,
    /// 累计返回导航触发次数。
    pub back_count: u32,
}

impl Default for AndroidMobileApp {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidMobileApp {
    /// 创建默认 Android 应用状态。
    pub const fn new() -> Self {
        Self {
            nav: Navigator::new(AndroidRoute::Dashboard),
            battery_pct: 65,
            power_save: false,
            sheet_open: false,
            back_count: 0,
        }
    }
}

impl View for AndroidMobileApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let current_route = self.nav.current();
        let can_pop = self.nav.can_pop();
        let sheet_open = self.sheet_open;
        let can_intercept_back = sheet_open || can_pop;

        let on_swipe_back = cx.listener(|this, dir: SwipeDir, _acx| {
            if dir == SwipeDir::Right {
                if this.sheet_open {
                    this.sheet_open = false;
                    this.back_count += 1;
                } else if this.nav.can_pop() {
                    this.nav.pop(200);
                    this.back_count += 1;
                }
            }
        });

        let raw_nav_back = cx.action(|this, _acx| {
            if this.nav.can_pop() {
                this.nav.pop(200);
                this.back_count += 1;
            }
        });

        let raw_toggle_sheet = cx.action(|this, _acx| {
            this.sheet_open = !this.sheet_open;
        });

        let raw_dismiss_sheet = cx.action(|this, _acx| {
            this.sheet_open = false;
            this.back_count += 1;
        });

        let on_tab_select = cx.listener(|this, idx: usize, _acx| {
            let target = AndroidRoute::from_tab_index(idx);
            if target != this.nav.current() {
                if target == AndroidRoute::Dashboard && this.nav.can_pop() {
                    this.nav.pop(180);
                } else {
                    this.nav.push(target, PageTransition::SlideLeft, 180);
                }
            }
        });

        // 1. 顶部导航栏（自动叠加 Android 状态栏/挖孔屏 safe_area.top）
        let mut navbar =
            NavBar::new(100, current_route.title()).action("Sheet", raw_toggle_sheet);
        if can_pop {
            navbar = navbar.back("< Back", raw_nav_back);
        }

        // 2. 页面主体内容区
        let body = match current_route {
            AndroidRoute::Dashboard => {
                let bat = self.battery_pct;
                let on_charge = cx.listener(|this, _: ClickEvent, _acx| {
                    this.battery_pct = (this.battery_pct + 10).min(100);
                });
                let on_open_detail = cx.listener(|this, _: ClickEvent, _acx| {
                    this.nav
                        .push(AndroidRoute::DeviceDetail, PageTransition::SlideLeft, 200);
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(
                        cx,
                        ArcGauge::new(110, 120, bat)
                            .thickness(10)
                            .label(fmt!(cx, "{}%", bat)),
                    )
                    .child(
                        cx,
                        Button::new(111, "Charge +10%")
                            .primary()
                            .w(150)
                            .h(38)
                            .on_click(on_charge),
                    )
                    .child(
                        cx,
                        Button::new(112, "Inspect Detail")
                            .secondary()
                            .w(150)
                            .h(38)
                            .on_click(on_open_detail),
                    )
            }
            AndroidRoute::DeviceDetail => {
                let on_drain = cx.listener(|this, _: ClickEvent, _acx| {
                    this.battery_pct = this.battery_pct.saturating_sub(15);
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(cx, text("Swipe from left edge or press Android Back"))
                    .child(
                        cx,
                        Button::new(121, "Drain -15%")
                            .primary()
                            .w(150)
                            .h(38)
                            .on_click(on_drain),
                    )
            }
            AndroidRoute::Settings => {
                let power_save = self.power_save;
                let on_toggle_save = cx.listener(|this, val: bool, _acx| {
                    this.power_save = val;
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(cx, text("Battery Saver Mode"))
                    .child(
                        cx,
                        Switch::new(131, power_save).on_change(on_toggle_save),
                    )
            }
        };

        // 3. 底部标签栏（自动叠加 Android 底部手势导航条 safe_area.bottom）
        let tab_bar = TabBar::new(
            200,
            &["Home", "Detail", "Settings"],
            current_route.tab_index(),
        )
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
            root = root.on_swipe(cx, on_swipe_back);
        }

        // 4. 模态底部抽屉（自动避让 safe_area.bottom + 软键盘高度 keyboard_inset）
        if sheet_open {
            root = root.child(
                cx,
                BottomSheet::new(300, "Android Quick Actions", 130)
                    .content(cx, text("Swipe down, tap backdrop, or press Back"))
                    .on_dismiss(raw_dismiss_sheet),
            );
        }

        root.into_element(cx)
    }
}

/// 复用框架层泛型 [`velox_kit::platform::android::AndroidHostBridge`] 绑定 [`AndroidMobileApp`]。
pub type AndroidHostBridge = velox_kit::platform::android::AndroidHostBridge<AndroidMobileApp>;

// 一行宏导出完整 Android C ABI (`velox_android_app_*` / `velox_mobile_app_*`) 与 `MainActivity` JNI 接口
velox_kit::export_android_app!(AndroidMobileApp);
