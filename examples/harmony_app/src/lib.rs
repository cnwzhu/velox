//! Velox 鸿蒙 HarmonyOS NEXT / OpenHarmony 原生移动应用示例（基于 `velox-kit/harmony` 平台特性）。
//!
//! 展示如何使用 `velox-kit` 构建完整的华为鸿蒙（HarmonyOS NEXT Stage 模型 + ArkTS `XComponent`）手机与折叠屏应用，
//! 仅需编写纯声明式 [`View`] 业务代码，底层 `OH_NativeXComponent`、`OHNativeWindow` 与 N-API 导出全部由框架层 [`velox_kit::platform::harmony`] 提供：
//! - 鸿蒙系统状态栏/挖孔区（`AvoidAreaType.TYPE_SYSTEM` / `TYPE_CUTOUT` `38vp`）与底部导航指示条（`TYPE_NAVIGATION_INDICATOR` `28vp`）[`SafeAreaInsets::harmony_portrait`] 自动避让；
//! - `AvoidAreaType.TYPE_KEYBOARD` 软键盘弹出与收起时的底部高度动态避让（`keyboard_inset`）；
//! - 移动端专属导航组件 [`NavBar`]、[`TabBar`]、[`BottomSheet`] 与 [`Navigator`] 页面栈；
//! - 左边缘右滑返回手势与鸿蒙 `onBackPress(): boolean` 页面级返回拦截消费判定；
//! - 通过 [`velox_kit::export_harmony_app!`] 一键生成供 ArkTS `XComponent` 调用的 C ABI 入口。

use velox_kit::prelude::*;

/// 鸿蒙 HarmonyOS NEXT 示例应用的页面路由枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HarmonyRoute {
    /// 首页鸿蒙设备仪表盘。
    Dashboard,
    /// 分布式软总线（Distributed SoftBus）互联遥测二级页（支持左边缘右滑返回与 `onBackPress`）。
    DistributedBus,
    /// 鸿蒙系统设置页（包含开关与 `TYPE_KEYBOARD` 软键盘避让验证）。
    Settings,
}

impl HarmonyRoute {
    #[inline(always)]
    pub const fn tab_index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::DistributedBus => 1,
            Self::Settings => 2,
        }
    }

    #[inline(always)]
    pub const fn from_tab_index(idx: usize) -> Self {
        match idx {
            0 => Self::Dashboard,
            1 => Self::DistributedBus,
            _ => Self::Settings,
        }
    }

    #[inline(always)]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Velox HarmonyOS",
            Self::DistributedBus => "Distributed SoftBus",
            Self::Settings => "Harmony Settings",
        }
    }
}

/// 鸿蒙 HarmonyOS NEXT 移动应用主视图状态。
pub struct HarmonyMobileApp {
    /// 页面路由栈（最大深度 4）。
    pub nav: Navigator<HarmonyRoute, 4>,
    /// 设备电量/算力协同百分比（`0..=100`）。
    pub battery_pct: u8,
    /// 鸿蒙分布式流转（Super Device Hop）开关。
    pub super_device_hop: bool,
    /// 底部快捷服务卡片抽屉（`BottomSheet`）是否打开。
    pub sheet_open: bool,
    /// 累计返回导航触发次数。
    pub back_count: u32,
}

impl Default for HarmonyMobileApp {
    fn default() -> Self {
        Self::new()
    }
}

impl HarmonyMobileApp {
    /// 创建默认鸿蒙应用状态。
    pub const fn new() -> Self {
        Self {
            nav: Navigator::new(HarmonyRoute::Dashboard),
            battery_pct: 82,
            super_device_hop: true,
            sheet_open: false,
            back_count: 0,
        }
    }
}

impl View for HarmonyMobileApp {
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
            let target = HarmonyRoute::from_tab_index(idx);
            if target != this.nav.current() {
                if target == HarmonyRoute::Dashboard && this.nav.can_pop() {
                    this.nav.pop(180);
                } else {
                    this.nav.push(target, PageTransition::SlideLeft, 180);
                }
            }
        });

        // 1. 顶部导航栏（自动叠加鸿蒙 AvoidAreaType.TYPE_SYSTEM safe_area.top = 38vp）
        let mut navbar =
            NavBar::new(100, current_route.title()).action("Sheet", raw_toggle_sheet);
        if can_pop {
            navbar = navbar.back("< Back", raw_nav_back);
        }

        // 2. 页面主体内容区
        let body = match current_route {
            HarmonyRoute::Dashboard => {
                let bat = self.battery_pct;
                let on_charge = cx.listener(|this, _: ClickEvent, _acx| {
                    this.battery_pct = (this.battery_pct + 10).min(100);
                });
                let on_open_bus = cx.listener(|this, _: ClickEvent, _acx| {
                    this.nav.push(
                        HarmonyRoute::DistributedBus,
                        PageTransition::SlideLeft,
                        200,
                    );
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
                        Button::new(112, "Inspect SoftBus")
                            .secondary()
                            .w(150)
                            .h(38)
                            .on_click(on_open_bus),
                    )
            }
            HarmonyRoute::DistributedBus => {
                let on_drain = cx.listener(|this, _: ClickEvent, _acx| {
                    this.battery_pct = this.battery_pct.saturating_sub(15);
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(cx, text("Swipe from left edge or trigger Harmony onBackPress"))
                    .child(
                        cx,
                        Button::new(121, "Drain -15%")
                            .primary()
                            .w(150)
                            .h(38)
                            .on_click(on_drain),
                    )
            }
            HarmonyRoute::Settings => {
                let hop = self.super_device_hop;
                let on_toggle_hop = cx.listener(|this, val: bool, _acx| {
                    this.super_device_hop = val;
                });

                div()
                    .flex_col()
                    .w_full()
                    .p(16)
                    .gap(12)
                    .items_center()
                    .child(cx, text("HarmonyOS Super Device Hop"))
                    .child(cx, Switch::new(131, hop).on_change(on_toggle_hop))
            }
        };

        // 3. 底部标签栏（自动叠加鸿蒙 TYPE_NAVIGATION_INDICATOR safe_area.bottom = 28vp）
        let tab_bar = TabBar::new(
            200,
            &["Home", "SoftBus", "Settings"],
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

        // 4. 模态底部元服务卡片抽屉（自动避让 safe_area.bottom + TYPE_KEYBOARD 软键盘高度）
        if sheet_open {
            root = root.child(
                cx,
                BottomSheet::new(300, "Harmony Atomic Service", 130)
                    .content(cx, text("Swipe down, tap backdrop, or press Back"))
                    .on_dismiss(raw_dismiss_sheet),
            );
        }

        root.into_element(cx)
    }
}

/// 复用框架层泛型 [`velox_kit::platform::harmony::HarmonyHostBridge`] 绑定 [`HarmonyMobileApp`]。
pub type HarmonyHostBridge = velox_kit::platform::harmony::HarmonyHostBridge<HarmonyMobileApp>;

// 一行宏导出完整鸿蒙 HarmonyOS NEXT C ABI (`velox_harmony_app_*`) 符号
velox_kit::export_harmony_app!(HarmonyMobileApp);
