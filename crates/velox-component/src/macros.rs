//! `velox-component` 主题化组件包装声明宏。
//!
//! 参照 `base` / `component` 分层模式：
//! - `velox-base` 提供默认无样式的交互行为原语与状态机；
//! - `velox-component` 通过宏生成独立的主题化包装组件，在构造阶段设置主题默认行为开关（`init`），
//!   在渲染前注入 `Theme` 视觉皮肤（`apply_tw`），绝不覆盖调用方显式定制的样式。

/// 为无生命周期（或带 `'a` 生命周期）的 `velox_base` 组件生成独立的 `velox_component` 主题化包装类型。
#[macro_export]
macro_rules! define_themed_component {
    (
        name: $name:ident <'a>,
        base: $base:path,
        new: ($($arg:ident : $arg_ty:ty),* $(,)?),
        styled: $styled:tt,
        variants: $variants:tt,
        sizable: $sizable:tt,
        disableable: $disableable:tt,
        $(init: $init:expr,)?
        apply_tw: $apply_tw:expr
        $(, forward: [ $( $fn_name:ident ( $($f_arg:ident : $f_ty:ty),* $(,)? ) ),* $(,)? ] )?
        $(,)?
    ) => {
        /// 建立在 [`velox_base`] 无样式基础原语之上的主题化视觉组件。
        pub struct $name<'a> {
            pub inner: $base,
            _marker: core::marker::PhantomData<&'a ()>,
        }

        impl<'a> $name<'a> {
            /// 构造主题化组件实例。
            #[inline(always)]
            pub fn new($($arg: $arg_ty),*) -> Self {
                #[allow(unused_mut)]
                let mut inner = <$base>::new($($arg),*);
                $(
                    let init_fn: fn(&mut $base) = $init;
                    init_fn(&mut inner);
                )?
                Self {
                    inner,
                    _marker: core::marker::PhantomData,
                }
            }

            /// 从底层 [`velox_base`] 组件包装为主题化组件。
            #[inline(always)]
            pub fn from_base(mut inner: $base) -> Self {
                $(
                    let init_fn: fn(&mut $base) = $init;
                    init_fn(&mut inner);
                )?
                let _ = &mut inner;
                Self {
                    inner,
                    _marker: core::marker::PhantomData,
                }
            }

            /// 剥离主题包装层，退化为底层 [`velox_base`] 基础组件。
            #[inline(always)]
            pub fn into_base(self) -> $base {
                self.inner
            }

            $(
                $(
                    #[inline(always)]
                    pub fn $fn_name(mut self, $($f_arg: $f_ty),*) -> Self {
                        self.inner = self.inner.$fn_name($($f_arg),*);
                        self
                    }
                )*
            )?
        }

        impl<'a> core::ops::Deref for $name<'a> {
            type Target = $base;

            #[inline(always)]
            fn deref(&self) -> &Self::Target {
                &self.inner
            }
        }

        impl<'a> core::ops::DerefMut for $name<'a> {
            #[inline(always)]
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.inner
            }
        }

        impl<'a> From<$base> for $name<'a> {
            #[inline(always)]
            fn from(inner: $base) -> Self {
                Self::from_base(inner)
            }
        }

        $crate::define_themed_component!(@impl_styled $styled, $name <'a>);
        $crate::define_themed_component!(@impl_variants $variants, $name <'a>);
        $crate::define_themed_component!(@impl_sizable $sizable, $name <'a>);
        $crate::define_themed_component!(@impl_disableable $disableable, $name <'a>);

        impl<'a> velox_core::prelude::Widget<'a> for $name<'a> {
            fn render_once(mut self, cx: &'a velox_core::prelude::FrameScope) -> velox_core::prelude::Element<'a> {
                let apply_fn: fn(&mut $base, &crate::theme::Theme) = $apply_tw;
                apply_fn(&mut self.inner, cx.theme());
                self.inner.render_once(cx)
            }
        }
    };

    (@impl_styled true, $name:ident <'a>) => {
        impl<'a> velox_core::style::Styled for $name<'a> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut velox_core::style::Style {
                velox_core::style::Styled::style_mut(&mut self.inner)
            }
        }
        impl<'a> velox_core::style::StyledExt for $name<'a> {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut velox_core::style::StyleExt {
                velox_core::style::StyledExt::style_ext_mut(&mut self.inner)
            }
        }
    };
    (@impl_styled false, $name:ident <'a>) => {
        impl<'a> velox_core::style::Styled for $name<'a> {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut velox_core::style::Style {
                velox_core::style::Styled::style_mut(&mut self.inner)
            }
        }
        impl<'a> velox_core::style::StyledExt for $name<'a> {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut velox_core::style::StyleExt {
                velox_core::style::StyledExt::style_ext_mut(&mut self.inner)
            }
        }
    };

    (@impl_variants true, $name:ident <'a>) => {
        impl<'a> velox_base::Variants for $name<'a> {
            #[inline(always)]
            fn with_variant(mut self, variant: velox_base::ComponentVariant) -> Self {
                self.inner = velox_base::Variants::with_variant(self.inner, variant);
                self
            }
        }
    };
    (@impl_variants false, $name:ident <'a>) => {};

    (@impl_sizable true, $name:ident <'a>) => {
        impl<'a> velox_base::Sizable for $name<'a> {
            #[inline(always)]
            fn with_size(mut self, size: velox_base::ComponentSize) -> Self {
                self.inner = velox_base::Sizable::with_size(self.inner, size);
                self
            }
        }
    };
    (@impl_sizable false, $name:ident <'a>) => {};

    (@impl_disableable true, $name:ident <'a>) => {
        impl<'a> velox_base::Disableable for $name<'a> {
            #[inline(always)]
            fn disabled(mut self, is_disabled: bool) -> Self {
                self.inner = velox_base::Disableable::disabled(self.inner, is_disabled);
                self
            }
        }
    };
    (@impl_disableable false, $name:ident <'a>) => {};

    // 无生命周期组件分支（如 ProgressBar / Scrollbar / Skeleton / Spinner）
    (
        name: $name:ident,
        base: $base:path,
        new: ($($arg:ident : $arg_ty:ty),* $(,)?),
        styled: $styled:tt,
        variants: $variants:tt,
        sizable: $sizable:tt,
        disableable: $disableable:tt,
        $(init: $init:expr,)?
        apply_tw: $apply_tw:expr
        $(, forward: [ $( $fn_name:ident ( $($f_arg:ident : $f_ty:ty),* $(,)? ) ),* $(,)? ] )?
        $(,)?
    ) => {
        /// 建立在 [`velox_base`] 无样式基础原语之上的主题化视觉组件。
        pub struct $name {
            pub inner: $base,
        }

        impl $name {
            /// 构造主题化组件实例。
            #[inline(always)]
            pub fn new($($arg: $arg_ty),*) -> Self {
                #[allow(unused_mut)]
                let mut inner = <$base>::new($($arg),*);
                $(
                    let init_fn: fn(&mut $base) = $init;
                    init_fn(&mut inner);
                )?
                Self { inner }
            }

            /// 从底层 [`velox_base`] 组件包装为主题化组件。
            #[inline(always)]
            pub fn from_base(mut inner: $base) -> Self {
                $(
                    let init_fn: fn(&mut $base) = $init;
                    init_fn(&mut inner);
                )?
                let _ = &mut inner;
                Self { inner }
            }

            /// 剥离主题包装层，退化为底层 [`velox_base`] 基础组件。
            #[inline(always)]
            pub fn into_base(self) -> $base {
                self.inner
            }

            $(
                $(
                    #[inline(always)]
                    pub fn $fn_name(mut self, $($f_arg: $f_ty),*) -> Self {
                        self.inner = self.inner.$fn_name($($f_arg),*);
                        self
                    }
                )*
            )?
        }

        impl core::ops::Deref for $name {
            type Target = $base;

            #[inline(always)]
            fn deref(&self) -> &Self::Target {
                &self.inner
            }
        }

        impl core::ops::DerefMut for $name {
            #[inline(always)]
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.inner
            }
        }

        impl From<$base> for $name {
            #[inline(always)]
            fn from(inner: $base) -> Self {
                Self::from_base(inner)
            }
        }

        impl velox_core::style::Styled for $name {
            #[inline(always)]
            fn style_mut(&mut self) -> &mut velox_core::style::Style {
                velox_core::style::Styled::style_mut(&mut self.inner)
            }
        }

        impl velox_core::style::StyledExt for $name {
            #[inline(always)]
            fn style_ext_mut(&mut self) -> &mut velox_core::style::StyleExt {
                velox_core::style::StyledExt::style_ext_mut(&mut self.inner)
            }
        }

        impl<'a> velox_core::prelude::Widget<'a> for $name {
            fn render_once(mut self, cx: &'a velox_core::prelude::FrameScope) -> velox_core::prelude::Element<'a> {
                let apply_fn: fn(&mut $base, &crate::theme::Theme) = $apply_tw;
                apply_fn(&mut self.inner, cx.theme());
                self.inner.render_once(cx)
            }
        }
    };
}

/// 为主题化组件转发底层 `velox-base` 的零堆分配元素插槽定制回调（返回任意 `IntoElement`）。
#[macro_export]
macro_rules! forward_slot_el {
    ($ty:ident, $method:ident, ($($arg:ty),* $(,)?)) => {
        impl<'a> $ty<'a> {
            /// 转发底层 `velox-base` 的零堆分配插槽定制回调。
            #[inline]
            pub fn $method<F, R>(mut self, cx: &'a velox_core::prelude::FrameScope, f: F) -> Self
            where
                F: Fn($($arg),*) -> R + 'a,
                R: velox_core::prelude::IntoElement<'a>,
            {
                self.inner = self.inner.$method(cx, f);
                self
            }
        }
    };
}

/// 为主题化组件转发底层 `velox-base` 的容器样式定制回调（返回 `Div`）。
#[macro_export]
macro_rules! forward_slot_div {
    ($ty:ident, $method:ident, ($($arg:ty),* $(,)?)) => {
        impl<'a> $ty<'a> {
            /// 转发底层 `velox-base` 的容器样式定制回调。
            #[inline]
            pub fn $method<F>(mut self, cx: &'a velox_core::prelude::FrameScope, f: F) -> Self
            where
                F: Fn($($arg),*) -> velox_core::prelude::Div<'a> + 'a,
            {
                self.inner = self.inner.$method(cx, f);
                self
            }
        }
    };
}

/// 为主题化组件转发底层 `velox-base` 的文案格式化插槽回调（返回任意 `Into<StrRef>`）。
#[macro_export]
macro_rules! forward_slot_str {
    ($ty:ident, $method:ident, ($($arg:ty),* $(,)?)) => {
        impl<'a> $ty<'a> {
            /// 转发底层 `velox-base` 的文案格式化插槽回调。
            #[inline]
            pub fn $method<F, S>(mut self, cx: &'a velox_core::prelude::FrameScope, f: F) -> Self
            where
                F: Fn($($arg),*) -> S + 'a,
                S: Into<velox_core::prelude::StrRef<'a>>,
            {
                self.inner = self.inner.$method(cx, f);
                self
            }
        }
    };
}
