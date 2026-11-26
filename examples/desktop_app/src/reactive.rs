//! `desktop_app` 细粒度响应式信号图与独立隔离边界子视图（`GallerySignals`、`StatusBarView`、`OverviewPlaygroundView`）。
//!
//! 展示 Velox 最佳实践：
//! - 将跨区块共享的高频标量状态托管于零堆 `Signal<T>` / `Computed<T>`（带 Early Cutoff）；
//! - 将高频变动区域与固定外框状态栏拆分为独立子 `View`（固定几何边界），
//!   交互时根 `GalleryApp` 与静态侧边栏/工具栏 **0 次重跑 `render()`**，直接命中引擎 **L2 原地局部重排**；
//! - 使用 `Slider::bind` / `Switch::bind` / `Checkbox::bind` 零堆双向绑定语法糖与 `batch` 事务合并更新。

use crate::i18n::DESKTOP_CATALOG;
use crate::stories::state_pill;
use crate::GalleryStory;
use velox_kit::component::{ArcGauge, Button, Checkbox, ProgressBar, Slider, StatusBarItem, Switch};
use velox_kit::prelude::*;

/// 画廊跨区块细粒度响应式共享信号集合（纯 `Copy` 轻量句柄集合）。
#[derive(Copy, Clone)]
pub struct GallerySignals {
    pub theme_mode: Signal<ThemeMode>,
    pub locale: Signal<Locale>,
    pub active_story: Signal<GalleryStory>,
    pub event_count: Signal<u32>,
    pub cpu_load_pct: Signal<u8>,
    pub frame_budget_pct: Signal<i32>,
    pub vsync_enabled: Signal<bool>,
    pub damage_overlay_hint: Signal<bool>,
    /// 派生高负载标记（`cpu_load_pct >= 80`），具备 Memoization 与 Early Cutoff 剪枝。
    pub high_load_alert: Computed<bool>,
}

impl GallerySignals {
    /// 创建并初始化画廊响应式信号图与派生计算节点。
    pub fn new(theme_mode: ThemeMode, locale: Locale, active_story: GalleryStory) -> Self {
        let cpu_load_pct = signal(42u8);
        let high_load_alert = computed(move || cpu_load_pct.get() >= 80);
        Self {
            theme_mode: signal(theme_mode),
            locale: signal(locale),
            active_story: signal(active_story),
            event_count: signal(0u32),
            cpu_load_pct,
            frame_budget_pct: signal(65i32),
            vsync_enabled: signal(true),
            damage_overlay_hint: signal(true),
            high_load_alert,
        }
    }
}

/// 底部固定高度（`26px`）独立状态栏子视图：
/// 仅订阅 `theme_mode` / `locale` / `active_story` / `event_count`，
/// 更新事件计数时直接命中 L2 原地局部重排，不触发根视图或画布重建。
pub struct StatusBarView {
    pub signals: GallerySignals,
}

impl StatusBarView {
    #[inline]
    pub const fn new(signals: GallerySignals) -> Self {
        Self { signals }
    }
}

impl View for StatusBarView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme_mode = self.signals.theme_mode.get();
        let locale = self.signals.locale.get();
        let active_story = self.signals.active_story.get();
        let events = self.signals.event_count.get();

        let theme = theme_mode.to_theme();
        cx.override_theme(theme);
        cx.override_locale(locale);
        cx.override_i18n_catalog(&DESKTOP_CATALOG);

        let active_story_label = cx.t(active_story.i18n_key());
        let theme_name = if theme_mode.is_dark() {
            cx.t("theme.dark")
        } else {
            cx.t("theme.light")
        };
        let left_item = StatusBarItem::new(cx.t("status.left").as_str());
        let center_item = StatusBarItem::new(
            cx.t_args("status.center", &[("story", active_story_label.into())])
                .as_str(),
        )
        .color(theme.primary);
        let right_item = StatusBarItem::new(
            cx.t_args(
                "status.right",
                &[("events", events.into()), ("theme", theme_name.into())],
            )
            .as_str(),
        );

        div()
            .id(600)
            .flex_row()
            .w_full()
            .h(26)
            .px(12)
            .bg(theme.card)
            .border(1, theme.border)
            .items_center()
            .justify_between()
            .child(cx, text(left_item.label).size(11).color(theme.muted_foreground))
            .child(cx, text(center_item.label).size(11).color(theme.foreground))
            .child(cx, text(right_item.label).size(11).color(theme.muted_foreground))
            .into_element(cx)
    }
}

/// `Overview` 故事页独立实时交互体验台子视图（固定高度边界 `210px`）：
/// - 使用 `Slider::bind` / `Switch::bind` / `Checkbox::bind` 零堆双向绑定；
/// - 使用 `batch` 原子更新 `cpu_load_pct` 与 `event_count`；
/// - 交互时根视图 `GalleryApp`、顶栏、侧边栏均 0 次重跑 `render()`，原地局部重排！
pub struct OverviewPlaygroundView {
    pub signals: GallerySignals,
}

impl OverviewPlaygroundView {
    #[inline]
    pub const fn new(signals: GallerySignals) -> Self {
        Self { signals }
    }
}

impl View for OverviewPlaygroundView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let theme = self.signals.theme_mode.get().to_theme();
        let locale = self.signals.locale.get();
        cx.override_theme(theme);
        cx.override_locale(locale);
        cx.override_i18n_catalog(&DESKTOP_CATALOG);

        let cpu_load = self.signals.cpu_load_pct.get();
        let frame_budget = self.signals.frame_budget_pct.get();
        let is_high_load = self.signals.high_load_alert.get();
        let signals = self.signals;

        let on_dec_load = cx.arena().alloc(move |_: ClickEvent, _: &mut ActionScope| {
            batch(|| {
                signals.cpu_load_pct.update(|v| *v = v.saturating_sub(10));
                signals.event_count.update(|n| *n = n.saturating_add(1));
            });
        });
        let on_inc_load = cx.arena().alloc(move |_: ClickEvent, _: &mut ActionScope| {
            batch(|| {
                signals.cpu_load_pct.update(|v| *v = (*v + 10).min(100));
                signals.event_count.update(|n| *n = n.saturating_add(1));
            });
        });

        let gauge_accent = if is_high_load {
            theme.danger
        } else {
            theme.primary
        };

        div()
            .id(300)
            .flex_row()
            .w_full()
            .h(210)
            .gap(16)
            .child(
                cx,
                div()
                    .flex_col()
                    .flex_grow(1)
                    .h_full()
                    .p(16)
                    .gap(12)
                    .rounded(8)
                    .bg(theme.card)
                    .border(1, theme.border)
                    .items_center()
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(6)
                            .items_center()
                            .child(
                                cx,
                                svg_icon(14, 14, "M22 12h-4l-3 9L9 3l-3 9H2").tint(gauge_accent),
                            )
                            .child(cx, text(cx.t("ov.load_title")).size(13).color(theme.foreground)),
                    )
                    .child(
                        cx,
                        ArcGauge::new(301, 108, cpu_load)
                            .thickness(10)
                            .color(gauge_accent)
                            .label(fmt!(cx, "{}%", cpu_load)),
                    )
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(8)
                            .child(
                                cx,
                                Button::new(304, "-10%")
                                    .outline()
                                    .font_size(12)
                                    .w(74)
                                    .h(28)
                                    .on_click(on_dec_load),
                            )
                            .child(
                                cx,
                                Button::new(305, "+10%")
                                    .primary()
                                    .font_size(12)
                                    .w(74)
                                    .h(28)
                                    .on_click(on_inc_load),
                            ),
                    ),
            )
            .child(
                cx,
                div()
                    .flex_col()
                    .flex_grow(1)
                    .h_full()
                    .p(16)
                    .gap(12)
                    .rounded(8)
                    .bg(theme.card)
                    .border(1, theme.border)
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                cx,
                                div()
                                    .flex_row()
                                    .gap(6)
                                    .items_center()
                                    .child(
                                        cx,
                                        svg_icon(
                                            14,
                                            14,
                                            "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM12 6v6l4 2",
                                        )
                                        .tint(theme.primary),
                                    )
                                    .child(
                                        cx,
                                        text(cx.t("ov.budget_title"))
                                            .size(13)
                                            .color(theme.foreground),
                                    ),
                            )
                            .child(
                                cx,
                                state_pill(cx, theme, fmt!(cx, "{} FPS", frame_budget)),
                            ),
                    )
                    .child(cx, ProgressBar::new(302, 280, cpu_load).height(8))
                    .child(
                        cx,
                        Slider::bind(cx, 303, signals.frame_budget_pct)
                            .range(30, 144)
                            .step(2)
                            .w(280),
                    )
                    .child(
                        cx,
                        div()
                            .flex_row()
                            .gap(16)
                            .items_center()
                            .child(cx, Switch::bind(cx, 306, signals.vsync_enabled).label("VSync"))
                            .child(
                                cx,
                                Checkbox::bind(cx, 307, signals.damage_overlay_hint)
                                    .label("Damage Tracking"),
                            ),
                    ),
            )
            .into_element(cx)
    }
}
