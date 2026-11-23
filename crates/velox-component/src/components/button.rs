//! 主题化按钮组件 `Button`（8 档语义变体、3 档尺寸、悬停/按压与焦点环）。

use crate::styled::{
    apply_tw_button_size, apply_tw_button_variant, button_variant_palette, is_unset,
};
use velox_base::{ComponentSize, ComponentVariant, Disableable, Sizable, Variants};
use velox_engine::prelude::*;

/// 建立在 [`velox_base::Button`] 之上的主题化按钮组件（完整 8 档语义变体、3 档尺寸、悬停/按压与焦点环）。
pub struct Button<'a> {
    pub inner: velox_base::components::button::Button<'a>,
    pub variant: ComponentVariant,
    pub size: ComponentSize,
    pub unstyled: bool,
}

impl<'a> Button<'a> {
    /// 构造一个主题化按钮组件（默认 `Primary` + `Medium`）。
    #[inline(always)]
    pub fn new(id: u32, label: impl Into<StrRef<'a>>) -> Self {
        Self {
            inner: velox_base::components::button::Button::new(id, label),
            variant: ComponentVariant::Primary,
            size: ComponentSize::Medium,
            unstyled: false,
        }
    }

    /// 从底层 [`velox_base::Button`] 包装为主题化按钮。
    #[inline(always)]
    pub const fn from_base(inner: velox_base::components::button::Button<'a>) -> Self {
        Self {
            inner,
            variant: ComponentVariant::Primary,
            size: ComponentSize::Medium,
            unstyled: false,
        }
    }

    /// 剥离主题包装层，退化为底层无样式 [`velox_base::Button`]。
    #[inline(always)]
    pub fn into_base(self) -> velox_base::components::button::Button<'a> {
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

    #[inline(always)]
    pub fn hover_bg(mut self, color: Color) -> Self {
        self.inner = self.inner.hover_bg(color);
        self
    }

    #[inline(always)]
    pub fn active_bg(mut self, color: Color) -> Self {
        self.inner = self.inner.active_bg(color);
        self
    }

    #[inline(always)]
    pub fn focus_border(mut self, width: u8, color: Color) -> Self {
        self.inner = self.inner.focus_border(width, color);
        self
    }

    #[inline(always)]
    pub fn focus_ring(mut self, enabled: bool) -> Self {
        self.inner = self.inner.focus_ring(enabled);
        self
    }

    #[inline(always)]
    pub fn selected(mut self, selected: bool) -> Self {
        self.inner = self.inner.selected(selected);
        self
    }

    #[inline(always)]
    pub fn loading(mut self, loading: bool) -> Self {
        self.inner = self.inner.loading(loading);
        self
    }

    /// 剥离本按钮上的全部预设主题样式，退化为纯无样式按钮。
    #[inline(always)]
    pub fn unstyled(mut self) -> Self {
        self.unstyled = true;
        self.inner = self.inner.unstyled();
        self
    }

    #[inline(always)]
    pub fn auto_repeat(mut self, enabled: bool) -> Self {
        self.inner = self.inner.auto_repeat(enabled);
        self
    }

    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.inner = self.inner.child(cx, child);
        self
    }

    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_click(handler);
        self
    }

    #[inline(always)]
    pub fn on_long_press(mut self, handler: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_long_press(handler);
        self
    }
}

impl<'a> core::ops::Deref for Button<'a> {
    type Target = velox_base::components::button::Button<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> core::ops::DerefMut for Button<'a> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a> From<velox_base::components::button::Button<'a>> for Button<'a> {
    #[inline(always)]
    fn from(inner: velox_base::components::button::Button<'a>) -> Self {
        Self::from_base(inner)
    }
}

impl<'a> Styled for Button<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a> StyledExt for Button<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a> Variants for Button<'a> {
    #[inline(always)]
    fn with_variant(mut self, variant: ComponentVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl<'a> Sizable for Button<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl<'a> Disableable for Button<'a> {
    #[inline(always)]
    fn disabled(mut self, is_disabled: bool) -> Self {
        self.inner = Disableable::disabled(self.inner, is_disabled);
        self
    }
}

impl<'a> Widget<'a> for Button<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        if !self.unstyled {
            let theme = cx.theme();
            let (def_bg, _bw, _bcol, def_fg, def_hover, def_active, is_solid) =
                button_variant_palette(theme, self.variant);

            apply_tw_button_variant(self.inner.style_mut(), theme, self.variant);
            apply_tw_button_size(self.inner.style_mut(), self.size);

            if self.inner.font_size == 0 {
                self.inner.font_size = match self.size {
                    ComponentSize::Small => 12,
                    ComponentSize::Medium => 13,
                    ComponentSize::Large => 14,
                };
            }
            if is_unset(self.inner.text_color) {
                self.inner.text_color = def_fg;
            }
            if self.inner.selected {
                if !is_solid
                    && (self.inner.style.bg == Color::TRANSPARENT || self.inner.style.bg == def_bg)
                {
                    self.inner.style.bg = theme.list_active;
                }
                if self.inner.style.border_px() == 0 {
                    self.inner.style.set_border_px(1);
                    self.inner.style.border_color = if is_solid {
                        def_bg.lerp(Color::WHITE, 128)
                    } else {
                        theme.primary
                    };
                }
            }
            if self.inner.hover_bg.is_none() {
                self.inner.hover_bg = Some(def_hover);
            }
            if self.inner.active_bg.is_none() {
                self.inner.active_bg = Some(def_active);
            }
            if self.inner.focus_border.is_none() && !self.inner.focus_ring {
                if is_solid {
                    let fill = if self.inner.style.bg != Color::TRANSPARENT {
                        self.inner.style.bg
                    } else {
                        def_bg
                    };
                    self.inner.focus_border = Some((1, fill.lerp(Color::WHITE, 128)));
                } else {
                    self.inner.focus_ring = true;
                }
            }
        }
        self.inner.render_once(cx)
    }
}
