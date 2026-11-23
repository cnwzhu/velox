//! 主题化状态徽章组件 `Badge`。

use crate::styled::is_unset;
use velox_base::{ComponentSize, ComponentVariant, Sizable, Variants};
use velox_core::prelude::*;

/// 建立在 [`velox_base::Badge`] 之上的主题化状态徽章组件（完整 8 档语义变体与 3 档尺寸）。
pub struct Badge<'a> {
    pub inner: velox_base::components::badge::Badge<'a>,
    pub variant: ComponentVariant,
    pub size: ComponentSize,
}

impl<'a> Badge<'a> {
    #[inline(always)]
    pub fn new(label: impl Into<StrRef<'a>>) -> Self {
        Self {
            inner: velox_base::components::badge::Badge::new(label),
            variant: ComponentVariant::Primary,
            size: ComponentSize::Medium,
        }
    }

    #[inline(always)]
    pub const fn from_base(inner: velox_base::components::badge::Badge<'a>) -> Self {
        Self {
            inner,
            variant: ComponentVariant::Primary,
            size: ComponentSize::Medium,
        }
    }

    #[inline(always)]
    pub fn into_base(self) -> velox_base::components::badge::Badge<'a> {
        self.inner
    }

    #[inline(always)]
    pub fn font_size(mut self, size: u8) -> Self {
        self.inner = self.inner.font_size(size);
        self
    }

    #[inline(always)]
    pub fn text_color(mut self, color: Color) -> Self {
        self.inner = self.inner.text_color(color);
        self
    }
}

impl<'a> core::ops::Deref for Badge<'a> {
    type Target = velox_base::components::badge::Badge<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> core::ops::DerefMut for Badge<'a> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a> From<velox_base::components::badge::Badge<'a>> for Badge<'a> {
    #[inline(always)]
    fn from(inner: velox_base::components::badge::Badge<'a>) -> Self {
        Self::from_base(inner)
    }
}

impl<'a> Styled for Badge<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a> StyledExt for Badge<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a> Variants for Badge<'a> {
    #[inline(always)]
    fn with_variant(mut self, variant: ComponentVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl<'a> Sizable for Badge<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl<'a> Widget<'a> for Badge<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let (bg, bw, bcol, fg) = match self.variant {
            ComponentVariant::Primary => (theme.primary, 0, Color::TRANSPARENT, theme.primary_foreground),
            ComponentVariant::Secondary => (theme.secondary, 1, theme.border, theme.secondary_foreground),
            ComponentVariant::Outline => (Color::TRANSPARENT, 1, theme.border, theme.foreground),
            ComponentVariant::Warning => (theme.warning, 0, Color::TRANSPARENT, theme.warning_foreground),
            ComponentVariant::Danger => (theme.danger, 0, Color::TRANSPARENT, theme.danger_foreground),
            ComponentVariant::Success => (theme.success, 0, Color::TRANSPARENT, theme.success_foreground),
            ComponentVariant::Info => (theme.info, 0, Color::TRANSPARENT, theme.info_foreground),
            ComponentVariant::Ghost => (theme.muted, 0, Color::TRANSPARENT, theme.muted_foreground),
        };
        let (h, px, fsz) = match self.size {
            ComponentSize::Small => (18i16, 6i8, 10u8),
            ComponentSize::Medium => (22i16, 8i8, 11u8),
            ComponentSize::Large => (26i16, 10i8, 12u8),
        };

        if self.inner.style.bg == Color::TRANSPARENT && bg != Color::TRANSPARENT {
            self.inner.style.bg = bg;
        }
        if self.inner.style.border_px() == 0 && bw > 0 {
            self.inner.style.set_border_px(bw);
            self.inner.style.border_color = bcol;
        }
        if (self.inner.style.rounded & 0x7F) == 0 {
            self.inner.style.set_rounded(6);
        }
        if matches!(self.inner.style.height, Length::Auto) {
            self.inner.style.height = Length::Px(h);
        }
        if self.inner.style.padding.left == 0 && self.inner.style.padding.right == 0 {
            self.inner.style.padding.left = px;
            self.inner.style.padding.right = px;
        }
        if self.inner.font_size == 0 {
            self.inner.font_size = fsz;
        }
        if is_unset(self.inner.text_color) {
            self.inner.text_color = fg;
        }

        self.inner.render_once(cx)
    }
}
