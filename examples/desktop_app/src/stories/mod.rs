//! `Velox Component Gallery` 交互式组件画廊故事页面集合（按功能域分组组织）。

mod buttons;
pub(crate) mod chat;
mod data;
mod editor;
mod effects;
mod engine_lab;
mod feedback;
mod icons_settings;
mod inputs;
pub(crate) mod motion;
mod navigation;
mod overlays;
mod overview;
mod selection;
mod sliders;

use velox_kit::prelude::*;

/// 统一渲染 Story 页头（标题 + 副标题说明 + 模块标识徽标）。
pub(crate) fn story_header<'a>(
    cx: &'a FrameScope,
    theme: CoreTheme,
    title: impl Into<StrRef<'a>>,
    subtitle: impl Into<StrRef<'a>>,
    api_tag: &'a str,
) -> Div<'a> {
    div()
        .flex_row()
        .w_full()
        .gap(12)
        .items_center()
        .justify_between()
        .child(
            cx,
            div()
                .flex_col()
                .flex_grow(1)
                .gap(4)
                .child(cx, text(title).size(18).color(theme.foreground))
                .child(cx, text(subtitle).size(12).color(theme.muted_foreground)),
        )
        .child(
            cx,
            div()
                .px(10)
                .py(4)
                .rounded(6)
                .bg(theme.secondary)
                .border(1, theme.border)
                .child(
                    cx,
                    text(api_tag)
                        .size(11)
                        .color(theme.secondary_foreground)
                        .text_center()
                        .nowrap(),
                ),
        )
}

/// 统一渲染 Story 分组卡片容器（Shadcn 风格圆角卡片）。
pub(crate) fn story_section<'a>(
    cx: &'a FrameScope,
    theme: CoreTheme,
    title: &'a str,
    hint: &'a str,
) -> Div<'a> {
    let mut header = div()
        .flex_row()
        .w_full()
        .gap(12)
        .items_center()
        .justify_between()
        .child(cx, text(title).size(13).color(theme.foreground).nowrap());
    if !hint.is_empty() {
        header = header.child(
            cx,
            text(hint)
                .size(11)
                .color(theme.muted_foreground)
                .ellipsis(),
        );
    }
    div()
        .flex_col()
        .w_full()
        .p(16)
        .gap(12)
        .rounded(8)
        .bg(theme.card)
        .border(1, theme.border)
        .child(cx, header)
        .child(cx, div().w_full().h(1).bg(theme.border))
}

/// 渲染紧凑的状态回显标签（实时展示交互结果）。
pub(crate) fn state_pill<'a>(
    cx: &'a FrameScope,
    theme: CoreTheme,
    label: impl Into<StrRef<'a>>,
) -> Div<'a> {
    div()
        .px(8)
        .py(3)
        .rounded(6)
        .bg(theme.secondary)
        .border(1, theme.border)
        .child(cx, text(label).size(11).color(theme.muted_foreground))
}
