//! Velox iOS / iPadOS 原生移动应用示例（基于 `velox-kit/ios` 平台特性）。
//!
//! 展示如何使用 `velox-kit` 构建完整的 Apple iOS 手机端应用，仅需编写纯声明式 [`View`] 业务代码，
//! 底层 `CADisplayLink`、`UITouchPhase`、`UIKeyboard` 与 C ABI 导出全部由框架层 [`velox_kit::platform::ios`] 提供：
//! - iPhone 灵动岛（Dynamic Island `59pt`）与底部 Home Indicator（`34pt`）[`SafeAreaInsets::ios_portrait`] 自动避让；
//! - `UIKeyboardWillChangeFrameNotification` 软键盘弹出与收起时的底部高度动态避让（`keyboard_inset`）；
//! - 移动端专属导航组件 [`NavBar`]、[`TabBar`]、[`BottomSheet`] 与 [`Navigator`] 页面栈；
//! - 左边缘右滑返回手势（`UIScreenEdgePanGestureRecognizer` / Velox 原生左边缘滑动识别）消费判定；
//! - 通过 [`velox_kit::export_ios_app!`] 一键生成供 Xcode / Swift `VeloxMetalView` 调用的 C ABI 入口。

use velox_kit::prelude::*;

/// iOS 示例应用的页面路由枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IosRoute {
    /// 首页灵动岛状态与仪表盘。
    Dashboard,
    /// Metal GPU 性能遥测二级页（支持左边缘右滑返回）。
    Telemetry,
    /// iOS 系统设置页（包含开关与软键盘输入避让验证）。
    Settings,
}

impl IosRoute {
    #[inline(always)]
    pub const fn tab_index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Telemetry => 1,
            Self::Settings => 2,
        }
    }

    #[inline(always)]
    pub const fn from_tab_index(idx: usize) -> Self {
        match idx {
            0 => Self::Dashboard,
            1 => Self::Telemetry,
            _ => Self::Settings,
        }
    }

    #[inline(always)]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Velox iOS",
            Self::Telemetry => "Metal Telemetry",
            Self::Settings => "iOS Settings",
        }
    }
}

/// iOS 移动应用主视图状态。
pub struct IosMobileApp {
    /// 页面路由栈（最大深度 4）。
    pub nav: Navigator<IosRoute, 4>,
    /// 电池/性能百分比（`0..=100`）。
    pub battery_pct: u8,
    /// ProMotion 120Hz 自适应刷新率开关。
    pub promotion_120hz: bool,
    /// 底部快捷操作抽屉（`BottomSheet`）是否打开。
    pub sheet_open: bool,
    /// 累计返回导航触发次数。
    pub back_count: u32,
}

impl Default for IosMobileApp {
    fn default() -> Self {
        Self::new()
    }
}

impl IosMobileApp {
    /// 创建默认 iOS 应用状态。
    pub const fn new() -> Self {
        Self {
            nav: Navigator::new(IosRoute::Dashboard),
            battery_pct: 78,
            promotion_120hz: true,
            sheet_open: false,
            back_count: 0,
        }
    }
}

impl View for IosMobileApp {
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
            let target = IosRoute::from_tab_index(idx);
            if target != this.nav.current() {
                if target == IosRoute::Dashboard && this.nav.can_pop() {
                    this.nav.pop(180);
                } else {
                    this.nav.push(target, PageTransition::SlideLeft, 180);
                }
            }
        });

        // 1. 顶部导航栏（自动叠加 iOS 灵动岛 safe_area.top = 59pt）
        let mut navbar =
            NavBar::new(100, current_route.title()).action("Sheet", raw_toggle_sheet);
        if can_pop {
            navbar = navbar.back("< Back", raw_nav_back);
        }

        // 2. 页面主体内容区
        let body = match current_route {
            IosRoute::Dashboard => {
                let bat = self.battery_pct;
                let on_boost = cx.listener(|this, _: ClickEvent, _acx| {
                    this.battery_pct = (this.battery_pct + 10).min(100);
                });
                let on_open_telemetry = cx.listener(|this, _: ClickEvent, _acx| {
                    this.nav
                        .push(IosRoute::Telemetry, PageTransition::SlideLeft, 200);
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
                            .on_click(on_boost),
                    )
                    .child(
                        cx,
                        Button::new(112, "Inspect Metal")
                            .secondary()
                            .w(150)
                            .h(38)
                            .on_click(on_open_telemetry),
                    )
            }
            IosRoute::Telemetry => {
                let on_drain = cx.listener(|this, _: ClickEvent, _acx| {
                    this.battery_pct = this.battery_pct.saturating_sub(15);
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(cx, text("Swipe from left edge to pop view controller"))
                    .child(
                        cx,
                        Button::new(121, "Drain -15%")
                            .primary()
                            .w(150)
                            .h(38)
                            .on_click(on_drain),
                    )
            }
            IosRoute::Settings => {
                let promotion = self.promotion_120hz;
                let on_toggle_promotion = cx.listener(|this, val: bool, _acx| {
                    this.promotion_120hz = val;
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(cx, text("ProMotion 120Hz CADisplayLink"))
                    .child(
                        cx,
                        Switch::new(131, promotion).on_change(on_toggle_promotion),
                    )
            }
        };

        // 3. 底部标签栏（自动叠加 iOS Home Indicator safe_area.bottom = 34pt）
        let tab_bar = TabBar::new(
            200,
            &["Home", "Telemetry", "Settings"],
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

        // 4. 模态底部抽屉（自动避让 safe_area.bottom + UIKeyboard 高度 keyboard_inset）
        if sheet_open {
            root = root.child(
                cx,
                BottomSheet::new(300, "iOS Action Sheet", 130)
                    .content(cx, text("Swipe down, tap backdrop, or edge-swipe"))
                    .on_dismiss(raw_dismiss_sheet),
            );
        }

        root.into_element(cx)
    }
}

/// 复用框架层泛型 [`velox_kit::platform::ios::IosHostBridge`] 绑定 [`IosMobileApp`]。
pub type IosHostBridge = velox_kit::platform::ios::IosHostBridge<IosMobileApp>;

// 一行宏导出完整 iOS C ABI (`velox_ios_app_*`) 符号
velox_kit::export_ios_app!(IosMobileApp);
