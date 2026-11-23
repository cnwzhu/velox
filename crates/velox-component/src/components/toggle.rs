//! 主题化双态切换按钮与切换组组件 `Toggle`、`ToggleGroup`。

use crate::styled::{fill_color, fill_rounded};
use velox_base::{ComponentSize, ComponentVariant, Disableable, Sizable, Variants};
use velox_engine::prelude::*;

pub use velox_base::components::toggle::ToggleItemState;

/// 建立在 [`velox_base::Toggle`] 之上的主题化双态切换按钮组件。
pub struct Toggle<'a> {
    pub inner: velox_base::Toggle<'a>,
    pub variant: ComponentVariant,
    pub size: ComponentSize,
}

impl<'a> Toggle<'a> {
    #[inline(always)]
    pub fn new(id: u32, label: &'a str, checked: bool) -> Self {
        let mut inner = velox_base::Toggle::new(id, label, checked);
        inner.focus_ring = true;
        Self {
            inner,
            variant: ComponentVariant::Primary,
            size: ComponentSize::Medium,
        }
    }

    #[inline(always)]
    pub fn from_base(mut inner: velox_base::Toggle<'a>) -> Self {
        inner.focus_ring = true;
        Self {
            inner,
            variant: ComponentVariant::Primary,
            size: ComponentSize::Medium,
        }
    }

    #[inline(always)]
    pub fn into_base(self) -> velox_base::Toggle<'a> {
        self.inner
    }

    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.inner = self.inner.child(cx, child);
        self
    }

    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(bool, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_change(handler);
        self
    }
}

impl<'a> core::ops::Deref for Toggle<'a> {
    type Target = velox_base::Toggle<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> core::ops::DerefMut for Toggle<'a> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a> From<velox_base::Toggle<'a>> for Toggle<'a> {
    #[inline(always)]
    fn from(inner: velox_base::Toggle<'a>) -> Self {
        Self::from_base(inner)
    }
}

impl<'a> Styled for Toggle<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a> StyledExt for Toggle<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a> Variants for Toggle<'a> {
    #[inline(always)]
    fn with_variant(mut self, variant: ComponentVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl<'a> Sizable for Toggle<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl<'a> Disableable for Toggle<'a> {
    #[inline(always)]
    fn disabled(mut self, is_disabled: bool) -> Self {
        self.inner = Disableable::disabled(self.inner, is_disabled);
        self
    }
}

impl<'a> Widget<'a> for Toggle<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let (active_bg, active_fg) = match self.variant {
            ComponentVariant::Primary => (theme.primary, theme.primary_foreground),
            ComponentVariant::Secondary => (theme.secondary, theme.secondary_foreground),
            ComponentVariant::Outline => (theme.list_active, theme.foreground),
            ComponentVariant::Warning => (theme.warning, theme.warning_foreground),
            ComponentVariant::Danger => (theme.danger, theme.danger_foreground),
            ComponentVariant::Success => (theme.success, theme.success_foreground),
            ComponentVariant::Info => (theme.info, theme.info_foreground),
            ComponentVariant::Ghost => (theme.accent, theme.foreground),
        };
        let (h, px, fsz) = match self.size {
            ComponentSize::Small => (26i16, 8i8, 12u8),
            ComponentSize::Medium => (32i16, 12i8, 13u8),
            ComponentSize::Large => (38i16, 16i8, 14u8),
        };

        fill_color(&mut self.inner.active_bg, active_bg);
        fill_color(&mut self.inner.inactive_bg, theme.secondary);
        fill_color(&mut self.inner.active_fg, active_fg);
        fill_color(&mut self.inner.inactive_fg, theme.foreground);
        fill_color(&mut self.inner.border_color, theme.border);
        if self.inner.border_width == 0 {
            self.inner.border_width = 1;
        }
        fill_rounded(&mut self.inner.rounded, 6);
        if self.inner.font_size == 0 {
            self.inner.font_size = fsz;
        }
        if matches!(self.inner.style.height, Length::Auto) {
            self.inner.style.height = Length::Px(h);
        }
        if self.inner.style.padding.left == 0 && self.inner.style.padding.right == 0 {
            self.inner.style.padding.left = px;
            self.inner.style.padding.right = px;
        }
        if self.inner.hover_bg.is_none() {
            self.inner.hover_bg = Some(if self.inner.checked {
                self.inner.active_bg.lerp(Color::WHITE, 24)
            } else {
                theme.accent
            });
        }
        if self.inner.active_press_bg.is_none() {
            self.inner.active_press_bg = Some(if self.inner.checked {
                self.inner.active_bg.lerp(Color::BLACK, 32)
            } else {
                theme.list_active
            });
        }

        self.inner.render_once(cx)
    }
}

/// 建立在 [`velox_base::ToggleGroup`] 之上的主题化多项切换按钮组组件。
pub struct ToggleGroup<'a> {
    pub inner: velox_base::ToggleGroup<'a>,
    pub size: ComponentSize,
}

impl<'a> ToggleGroup<'a> {
    #[inline(always)]
    pub fn new(id: u32, labels: &'a [&'a str], checked_mask: u32) -> Self {
        let mut inner = velox_base::ToggleGroup::new(id, labels, checked_mask);
        inner.focus_ring = true;
        Self {
            inner,
            size: ComponentSize::Medium,
        }
    }

    #[inline(always)]
    pub fn from_base(mut inner: velox_base::ToggleGroup<'a>) -> Self {
        inner.focus_ring = true;
        Self {
            inner,
            size: ComponentSize::Medium,
        }
    }

    #[inline(always)]
    pub fn into_base(self) -> velox_base::ToggleGroup<'a> {
        self.inner
    }

    #[inline(always)]
    pub fn multiple(mut self, multiple: bool) -> Self {
        self.inner = self.inner.multiple(multiple);
        self
    }

    #[inline(always)]
    pub fn on_toggle(mut self, handler: &'a dyn Fn(usize, bool, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_toggle(handler);
        self
    }
}

impl<'a> core::ops::Deref for ToggleGroup<'a> {
    type Target = velox_base::ToggleGroup<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> core::ops::DerefMut for ToggleGroup<'a> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a> From<velox_base::ToggleGroup<'a>> for ToggleGroup<'a> {
    #[inline(always)]
    fn from(inner: velox_base::ToggleGroup<'a>) -> Self {
        Self::from_base(inner)
    }
}

impl<'a> Styled for ToggleGroup<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a> StyledExt for ToggleGroup<'a> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a> Sizable for ToggleGroup<'a> {
    #[inline(always)]
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl<'a> Disableable for ToggleGroup<'a> {
    #[inline(always)]
    fn disabled(mut self, is_disabled: bool) -> Self {
        self.inner = Disableable::disabled(self.inner, is_disabled);
        self
    }
}

impl<'a> Widget<'a> for ToggleGroup<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let (h, px, fsz) = match self.size {
            ComponentSize::Small => (26i16, 8i8, 12u8),
            ComponentSize::Medium => (32i16, 12i8, 13u8),
            ComponentSize::Large => (38i16, 16i8, 14u8),
        };
        if self.inner.item_height == 0 {
            self.inner.item_height = h;
        }
        if self.inner.item_px == 0 {
            self.inner.item_px = px;
        }
        if self.inner.font_size == 0 {
            self.inner.font_size = fsz;
        }
        fill_color(&mut self.inner.active_bg, theme.primary);
        fill_color(&mut self.inner.inactive_bg, theme.secondary);
        fill_color(&mut self.inner.active_fg, theme.primary_foreground);
        fill_color(&mut self.inner.inactive_fg, theme.foreground);
        fill_color(&mut self.inner.border_color, theme.border);
        if self.inner.border_width == 0 {
            self.inner.border_width = 1;
        }
        fill_rounded(&mut self.inner.rounded, 6);
        if self.inner.hover_bg.is_none() {
            self.inner.hover_bg = Some(theme.accent);
        }
        if self.inner.active_press_bg.is_none() {
            self.inner.active_press_bg = Some(theme.list_active);
        }
        self.inner.render_once(cx)
    }
}

crate::forward_slot_el!(ToggleGroup, item, (Div<'a>, ToggleItemState<'a>));

crate::forward_slot_str!(ToggleGroup, label, (usize, &'a str));

impl<'a> Toggle<'a> {
    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，切换时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, label: &'a str, sig: Signal<bool>) -> Self {
        Self::from_base(velox_base::Toggle::bind(cx, id, label, sig))
    }
}
