//! `velox-base` 基础样式与组件样板声明宏。
//!
//! 通过声明宏消除基础组件在 `Styled`、`Disableable`、`Sizable`、`Variants` 及基础骨架 `Style` 构建上的重复样板代码。

/// 快速构建基础骨架 [`velox_engine::style::Style`] 的声明宏。
///
/// 示例：
/// ```ignore
/// let style = base_style! { row, items_center, justify_center, rounded: 8 };
/// ```
#[macro_export]
macro_rules! base_style {
    ($($op:ident $(: $val:expr)?),* $(,)?) => {{
        let mut style = $crate::__private::Style::default();
        $(
            $crate::base_style!(@apply style, $op $(: $val)?);
        )*
        style
    }};
    (@apply $style:ident, row) => { $style.set_row(); };
    (@apply $style:ident, col) => { $style.set_col(); };
    (@apply $style:ident, items_center) => { $style.set_items_center(); };
    (@apply $style:ident, justify_center) => { $style.set_justify_center(); };
    (@apply $style:ident, rounded: $r:expr) => { $style.set_rounded($r); };
    (@apply $style:ident, border: $w:expr) => { $style.set_border_px($w); };
    (@apply $style:ident, bg: $c:expr) => { $style.bg = $c; };
}

/// 为拥有 `style: Style` 与 `style_ext: StyleExt` 的组件批量同时实现 [`velox_engine::style::Styled`] 与 [`velox_engine::style::StyledExt`]。
#[macro_export]
macro_rules! impl_base_styled_box {
    ($ty:ident <'a>) => {
        impl<'a> $crate::__private::Styled for $ty<'a> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                &mut self.style
            }
        }
        impl<'a> $crate::__private::StyledExt for $ty<'a> {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut $crate::__private::StyleExt {
                &mut self.style_ext
            }
        }
    };
    ($ty:ident <'a, const $n:ident : usize>) => {
        impl<'a, const $n: usize> $crate::__private::Styled for $ty<'a, $n> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                &mut self.style
            }
        }
        impl<'a, const $n: usize> $crate::__private::StyledExt for $ty<'a, $n> {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut $crate::__private::StyleExt {
                &mut self.style_ext
            }
        }
    };
    ($ty:ident <'a, $t:ident>) => {
        impl<'a, $t> $crate::__private::Styled for $ty<'a, $t> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                &mut self.style
            }
        }
        impl<'a, $t> $crate::__private::StyledExt for $ty<'a, $t> {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut $crate::__private::StyleExt {
                &mut self.style_ext
            }
        }
    };
    ($ty:ident <'a>, via $field:ident) => {
        impl<'a> $crate::__private::Styled for $ty<'a> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                self.$field.style_mut()
            }
        }
        impl<'a> $crate::__private::StyledExt for $ty<'a> {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut $crate::__private::StyleExt {
                &mut self.style_ext
            }
        }
    };
    ($ty:ident) => {
        impl $crate::__private::Styled for $ty {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                &mut self.style
            }
        }
        impl $crate::__private::StyledExt for $ty {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut $crate::__private::StyleExt {
                &mut self.style_ext
            }
        }
    };
}

/// 为拥有 `style: Style` 字段（或指定字段）的组件批量实现 [`velox_engine::style::Styled`]。
#[macro_export]
macro_rules! impl_base_styled {
    ($ty:ident <'a>) => {
        impl<'a> $crate::__private::Styled for $ty<'a> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                &mut self.style
            }
        }
    };
    ($ty:ident <'a>, via $field:ident) => {
        impl<'a> $crate::__private::Styled for $ty<'a> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                self.$field.style_mut()
            }
        }
    };
    ($ty:ident) => {
        impl $crate::__private::Styled for $ty {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut $crate::__private::Style {
                &mut self.style
            }
        }
    };
}

/// 为拥有 `disabled: bool` 字段的组件批量实现 [`crate::traits::Disableable`]。
#[macro_export]
macro_rules! impl_base_disableable {
    ($($ty:ident $(<$lt:lifetime>)?),* $(,)?) => {
        $(
            impl$(<$lt>)? $crate::traits::Disableable for $ty$(<$lt>)? {
                #[inline(always)]
                fn disabled(mut self, is_disabled: bool) -> Self {
                    self.disabled = is_disabled;
                    self
                }
            }
        )*
    };
}

/// 为拥有 `size: ComponentSize` 字段的组件批量实现 [`crate::traits::Sizable`]。
#[macro_export]
macro_rules! impl_base_sizable {
    ($($ty:ident $(<$lt:lifetime>)?),* $(,)?) => {
        $(
            impl$(<$lt>)? $crate::traits::Sizable for $ty$(<$lt>)? {
                #[inline(always)]
                fn with_size(mut self, size: $crate::traits::ComponentSize) -> Self {
                    self.size = size;
                    self
                }
            }
        )*
    };
}

/// 为拥有 `variant: ComponentVariant` 字段的组件批量实现 [`crate::traits::Variants`]。
#[macro_export]
macro_rules! impl_base_variants {
    ($($ty:ident $(<$lt:lifetime>)?),* $(,)?) => {
        $(
            impl$(<$lt>)? $crate::traits::Variants for $ty$(<$lt>)? {
                #[inline(always)]
                fn with_variant(mut self, variant: $crate::traits::ComponentVariant) -> Self {
                    self.variant = variant;
                    self
                }
            }
        )*
    };
}
