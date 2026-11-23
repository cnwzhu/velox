//! Tailwind 风格主题化样式扩展（`TailwindStyled`）与共享主题着色辅助函数。
//!
//! 将 `velox_core::style::Styled` 的原子链式能力与 `Theme`、`ComponentVariant`、`ComponentSize`
//! 结合，为 `velox-component` 中的全部主题化包装组件提供一致的 Shadcn Zinc + Blue 视觉皮肤。

use crate::theme::Theme;
use velox_base::{ComponentSize, ComponentVariant};
use velox_core::prelude::*;

pub(crate) const AUTO_COLOR: Color = Color(0x0000_0001);

/// 判断颜色槽位是否处于“未显式定制”状态（`Color::TRANSPARENT` 或 `AUTO_COLOR` 哨兵）。
#[inline(always)]
pub(crate) fn is_unset(c: Color) -> bool {
    c == Color::TRANSPARENT || c == AUTO_COLOR
}

/// 仅在目标颜色槽位尚未被调用方显式定制时，写入主题默认色。
#[inline(always)]
pub(crate) fn fill_color(slot: &mut Color, default: Color) {
    if is_unset(*slot) {
        *slot = default;
    }
}

/// 仅在目标圆角尚未被调用方显式设置（为 `0`）时，写入主题默认圆角。
#[inline(always)]
pub(crate) fn fill_rounded(slot: &mut u8, default: u8) {
    if *slot == 0 {
        *slot = default;
    }
}

/// 对底层 [`Style`] 直接应用 Tailwind 风格的卡片表面样式（`rounded-lg border bg-card`）。
#[inline]
pub fn apply_tw_card_surface(style: &mut Style, theme: &Theme) {
    if style.bg == Color::TRANSPARENT {
        style.bg = theme.card;
    }
    if style.border_px() == 0 {
        style.set_border_px(1);
        style.border_color = theme.border;
    }
    if (style.rounded & 0x7F) == 0 {
        style.set_rounded(8);
    }
}

/// 对底层 [`Style`] 直接应用 Tailwind 风格的按钮尺寸档位（标准桌面高度档位）。
#[inline]
pub fn apply_tw_button_size(style: &mut Style, size: ComponentSize) {
    let (h, px) = match size {
        ComponentSize::Small => (26i16, 10i8),
        ComponentSize::Medium => (32i16, 14i8),
        ComponentSize::Large => (38i16, 18i8),
    };
    if matches!(style.height, Length::Auto) {
        style.height = Length::Px(h);
    }
    if style.padding.left == 0 && style.padding.right == 0 {
        style.padding.left = px;
        style.padding.right = px;
    }
}

/// 返回指定 [`ComponentVariant`] 在当前主题下的 `(bg, border_w, border_col, fg, hover_bg, active_bg, is_solid)`。
#[inline]
pub(crate) fn button_variant_palette(
    theme: &Theme,
    variant: ComponentVariant,
) -> (Color, u8, Color, Color, Color, Color, bool) {
    match variant {
        ComponentVariant::Primary => (
            theme.primary,
            0,
            Color::TRANSPARENT,
            theme.primary_foreground,
            theme.primary.lerp(Color::WHITE, 26),
            theme.primary.lerp(Color::BLACK, 38),
            true,
        ),
        ComponentVariant::Secondary => (
            theme.secondary,
            1,
            theme.border,
            theme.secondary_foreground,
            theme.list_active,
            theme.secondary.lerp(Color::BLACK, 24),
            false,
        ),
        ComponentVariant::Outline => (
            Color::TRANSPARENT,
            1,
            theme.border,
            theme.foreground,
            theme.accent,
            theme.list_active,
            false,
        ),
        ComponentVariant::Warning => (
            theme.warning,
            0,
            Color::TRANSPARENT,
            theme.warning_foreground,
            theme.warning.lerp(Color::WHITE, 26),
            theme.warning.lerp(Color::BLACK, 38),
            true,
        ),
        ComponentVariant::Danger => (
            theme.danger,
            0,
            Color::TRANSPARENT,
            theme.danger_foreground,
            theme.danger.lerp(Color::WHITE, 26),
            theme.danger.lerp(Color::BLACK, 38),
            true,
        ),
        ComponentVariant::Success => (
            theme.success,
            0,
            Color::TRANSPARENT,
            theme.success_foreground,
            theme.success.lerp(Color::WHITE, 26),
            theme.success.lerp(Color::BLACK, 38),
            true,
        ),
        ComponentVariant::Info => (
            theme.info,
            0,
            Color::TRANSPARENT,
            theme.info_foreground,
            theme.info.lerp(Color::WHITE, 26),
            theme.info.lerp(Color::BLACK, 38),
            true,
        ),
        ComponentVariant::Ghost => (
            Color::TRANSPARENT,
            0,
            Color::TRANSPARENT,
            theme.foreground,
            theme.accent,
            theme.list_active,
            false,
        ),
    }
}

/// 对底层 [`Style`] 直接应用 Tailwind 风格的按钮变体表面色（覆盖完整 8 档语义变体）。
#[inline]
pub fn apply_tw_button_variant(style: &mut Style, theme: &Theme, variant: ComponentVariant) {
    let (bg, border_w, border_col, _, _, _, _) = button_variant_palette(theme, variant);
    if style.bg == Color::TRANSPARENT && bg != Color::TRANSPARENT {
        style.bg = bg;
    }
    if style.border_px() == 0 && border_w > 0 {
        style.set_border_px(border_w);
        style.border_color = border_col;
    }
    if (style.rounded & 0x7F) == 0 {
        style.set_rounded(6);
    }
}

/// Tailwind 风格主题化样式扩展 Trait。
///
/// 所有方法均在“尚未显式自定义对应属性”时回退到 `Theme` / 设计系统默认值，
/// 绝不覆盖调用方通过 [`Styled`] 显式设置的样式。
pub trait TailwindStyled: Styled + Sized {
    /// 应用 Tailwind 风格的卡片表面样式（`rounded-lg border bg-card`）。
    #[inline]
    fn tw_card_surface(mut self, theme: &Theme) -> Self {
        apply_tw_card_surface(self.style_mut(), theme);
        self
    }

    /// 应用 Tailwind 风格的按钮尺寸档位（`h-7/9/11 px-2.5/3.5/4.5`）。
    #[inline]
    fn tw_button_size(mut self, size: ComponentSize) -> Self {
        apply_tw_button_size(self.style_mut(), size);
        self
    }

    /// 应用 Tailwind 风格的按钮变体表面色（`bg-* border-*`）。
    #[inline]
    fn tw_button_variant(mut self, theme: &Theme, variant: ComponentVariant) -> Self {
        apply_tw_button_variant(self.style_mut(), theme, variant);
        self
    }
}

impl<T: Styled> TailwindStyled for T {}
