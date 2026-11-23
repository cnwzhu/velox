//! 全局主题配色令牌 `Theme` / `CoreTheme`（现代 Shadcn Zinc 中性暗/亮色板）。

use crate::style::Color;

/// 核心主题配色令牌（供 `cx.theme()` 直接访问，包含亮色/暗色双模式）。
///
/// 色板采用现代 Shadcn / Zinc 中性桌面风格，提供完整的表面层级、交互状态、
/// 语义警示（`danger` / `success` / `warning` / `info`）、输入框边框（`input`）、
/// 键盘焦点环（`ring`）与侧边栏/列表高亮令牌。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CoreTheme {
    pub background: Color,
    pub foreground: Color,
    pub card: Color,
    pub card_foreground: Color,
    pub popover: Color,
    pub popover_foreground: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub secondary: Color,
    pub secondary_foreground: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub accent: Color,
    pub accent_foreground: Color,
    pub danger: Color,
    pub danger_foreground: Color,
    pub success: Color,
    pub success_foreground: Color,
    pub warning: Color,
    pub warning_foreground: Color,
    pub info: Color,
    pub info_foreground: Color,
    pub border: Color,
    pub input: Color,
    pub ring: Color,
    pub sidebar: Color,
    pub sidebar_foreground: Color,
    pub list_hover: Color,
    pub list_active: Color,
}

/// `CoreTheme` 的标准别名。
pub type Theme = CoreTheme;

impl CoreTheme {
    /// 默认暗色主题（Shadcn Zinc Dark 现代中性暗色色板）。
    pub const fn dark() -> Self {
        Self {
            background: Color::rgb(9, 9, 11),           // Zinc 950 (#09090B)
            foreground: Color::rgb(250, 250, 250),      // Zinc 50  (#FAFAFA)
            card: Color::rgb(20, 20, 23),               // Zinc 925 (#141417)
            card_foreground: Color::rgb(250, 250, 250), // Zinc 50
            popover: Color::rgb(24, 24, 27),            // Zinc 900 (#18181B)
            popover_foreground: Color::rgb(250, 250, 250),
            primary: Color::rgb(59, 130, 246),          // Blue 500 (#3B82F6)
            primary_foreground: Color::rgb(255, 255, 255),
            secondary: Color::rgb(39, 39, 42),          // Zinc 800 (#27272A)
            secondary_foreground: Color::rgb(244, 244, 245), // Zinc 100
            muted: Color::rgb(28, 28, 32),              // Zinc 875 (#1C1C20)
            muted_foreground: Color::rgb(161, 161, 170), // Zinc 400 (#A1A1AA)
            accent: Color::rgb(39, 39, 42),             // Zinc 800 (#27272A)
            accent_foreground: Color::rgb(250, 250, 250),
            danger: Color::rgb(239, 68, 68),            // Red 500  (#EF4444)
            danger_foreground: Color::rgb(255, 255, 255),
            success: Color::rgb(34, 197, 94),           // Green 500 (#22C55E)
            success_foreground: Color::rgb(255, 255, 255),
            warning: Color::rgb(245, 158, 11),          // Amber 500 (#F59E0B)
            warning_foreground: Color::rgb(24, 24, 27),
            info: Color::rgb(59, 130, 246),             // Blue 500 (#3B82F6)
            info_foreground: Color::rgb(255, 255, 255),
            border: Color::rgb(39, 39, 42),             // Zinc 800 (#27272A)
            input: Color::rgb(63, 63, 70),              // Zinc 700 (#3F3F46)
            ring: Color::rgb(91, 139, 217),             // 柔和低饱和钢蓝焦环（与暗色 Zinc 表面自然融合）
            sidebar: Color::rgb(15, 15, 18),            // Zinc 940 (#0F0F12)
            sidebar_foreground: Color::rgb(228, 228, 231), // Zinc 200
            list_hover: Color::rgb(34, 34, 38),         // Zinc 825
            list_active: Color::rgb(46, 46, 52),        // Zinc 775
        }
    }

    /// 明亮日间主题（Shadcn Zinc Light 清爽日间色板）。
    pub const fn light() -> Self {
        Self {
            background: Color::rgb(255, 255, 255),      // Pure White (#FFFFFF)
            foreground: Color::rgb(9, 9, 11),           // Zinc 950 (#09090B)
            card: Color::rgb(255, 255, 255),            // Pure White
            card_foreground: Color::rgb(9, 9, 11),      // Zinc 950
            popover: Color::rgb(255, 255, 255),         // Pure White
            popover_foreground: Color::rgb(9, 9, 11),
            primary: Color::rgb(37, 99, 235),           // Blue 600 (#2563EB)
            primary_foreground: Color::rgb(255, 255, 255),
            secondary: Color::rgb(244, 244, 245),       // Zinc 100 (#F4F4F5)
            secondary_foreground: Color::rgb(24, 24, 27), // Zinc 900
            muted: Color::rgb(244, 244, 245),           // Zinc 100 (#F4F4F5)
            muted_foreground: Color::rgb(113, 113, 122), // Zinc 500 (#71717A)
            accent: Color::rgb(244, 244, 245),          // Zinc 100
            accent_foreground: Color::rgb(24, 24, 27),
            danger: Color::rgb(220, 38, 38),            // Red 600  (#DC2626)
            danger_foreground: Color::rgb(255, 255, 255),
            success: Color::rgb(22, 163, 74),           // Green 600 (#16A34A)
            success_foreground: Color::rgb(255, 255, 255),
            warning: Color::rgb(217, 119, 6),           // Amber 600 (#D97706)
            warning_foreground: Color::rgb(255, 255, 255),
            info: Color::rgb(37, 99, 235),              // Blue 600 (#2563EB)
            info_foreground: Color::rgb(255, 255, 255),
            border: Color::rgb(228, 228, 231),          // Zinc 200 (#E4E4E7)
            input: Color::rgb(212, 212, 216),           // Zinc 300 (#D4D4D8)
            ring: Color::rgb(96, 145, 232),             // 柔和清爽天蓝焦环（与亮色 Zinc 表面自然融合）
            sidebar: Color::rgb(250, 250, 250),         // Zinc 50  (#FAFAFA)
            sidebar_foreground: Color::rgb(39, 39, 42), // Zinc 800
            list_hover: Color::rgb(244, 244, 245),      // Zinc 100
            list_active: Color::rgb(228, 228, 231),     // Zinc 200
        }
    }
}

impl Default for CoreTheme {
    fn default() -> Self {
        Self::dark()
    }
}
