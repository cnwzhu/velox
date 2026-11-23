//! 主题配色与外观模式组件模块 `Theme` / `ThemeMode`（统一导出核心主题令牌表与明暗模式辅助方法）。

/// 导出核心主题令牌表（`Theme`，支持自定义颜色、排版与亮色/暗黑外观模式）。
pub use velox_engine::pod::CoreTheme as Theme;

/// 主题外观明暗模式枚举（`ThemeMode`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    /// 获取对应明暗模式的默认 [`Theme`] 配色表。
    #[inline(always)]
    pub const fn to_theme(self) -> Theme {
        match self {
            Self::Dark => Theme::dark(),
            Self::Light => Theme::light(),
        }
    }

    /// 判断当前是否为暗黑模式。
    #[inline(always)]
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    /// 在亮色与暗黑模式之间切换。
    #[inline(always)]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}
