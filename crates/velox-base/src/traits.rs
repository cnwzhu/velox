//! 统一组件修饰符 Trait：`Sizable`（尺寸）、`Variants`（语义变体）与 `Disableable`（禁用状态）。

/// 组件尺寸档位枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ComponentSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// 组件语义变体枚举（完整 8 档语义变体）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ComponentVariant {
    #[default]
    Primary,
    Secondary,
    Outline,
    Warning,
    Danger,
    Success,
    Info,
    Ghost,
}

/// 统一尺寸修饰符 Trait。
pub trait Sizable: Sized {
    fn with_size(self, size: ComponentSize) -> Self;

    #[inline(always)]
    fn small(self) -> Self {
        self.with_size(ComponentSize::Small)
    }

    #[inline(always)]
    fn medium(self) -> Self {
        self.with_size(ComponentSize::Medium)
    }

    #[inline(always)]
    fn large(self) -> Self {
        self.with_size(ComponentSize::Large)
    }
}

/// 统一语义变体修饰符 Trait。
pub trait Variants: Sized {
    fn with_variant(self, variant: ComponentVariant) -> Self;

    #[inline(always)]
    fn primary(self) -> Self {
        self.with_variant(ComponentVariant::Primary)
    }

    #[inline(always)]
    fn secondary(self) -> Self {
        self.with_variant(ComponentVariant::Secondary)
    }

    #[inline(always)]
    fn outline(self) -> Self {
        self.with_variant(ComponentVariant::Outline)
    }

    #[inline(always)]
    fn warning(self) -> Self {
        self.with_variant(ComponentVariant::Warning)
    }

    #[inline(always)]
    fn danger(self) -> Self {
        self.with_variant(ComponentVariant::Danger)
    }

    #[inline(always)]
    fn success(self) -> Self {
        self.with_variant(ComponentVariant::Success)
    }

    #[inline(always)]
    fn info(self) -> Self {
        self.with_variant(ComponentVariant::Info)
    }

    #[inline(always)]
    fn ghost(self) -> Self {
        self.with_variant(ComponentVariant::Ghost)
    }
}

/// 统一禁用状态修饰符 Trait。
pub trait Disableable: Sized {
    fn disabled(self, is_disabled: bool) -> Self;
}
