//! `Velox Base Gallery` 交互式组件画廊故事页面集合（按功能域分组组织）。

mod buttons;
mod data;
mod editor;
mod feedback;
mod icons_settings;
mod inputs;
mod navigation;
mod overlays;
mod overview;
mod selection;
mod sliders;

use crate::style::BasePalette;
use velox_kit::prelude::*;

/// 统一渲染 Story 页头（标题 + 副标题说明 + 对应的 `velox_base` 模块标识徽标，极简直角）。
pub(crate) fn story_header<'a>(
    cx: &'a FrameScope,
    theme: CoreTheme,
    title: impl Into<StrRef<'a>>,
    subtitle: impl Into<StrRef<'a>>,
    api_tag: &'a str,
) -> Div<'a> {
    let p = BasePalette::from_theme(theme);
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
                .child(cx, text(title).size(18).color(p.fg))
                .child(cx, text(subtitle).size(12).color(p.muted_fg)),
        )
        .child(
            cx,
            div()
                .px(8)
                .py(4)
                .bg(p.surface_subtle)
                .border(1, p.border_strong)
                .child(cx, text(api_tag).size(11).color(p.fg).text_center().nowrap()),
        )
}

/// 统一渲染 Story 分组卡片容器（极简 0px 直角 + 1px 细线边框）。
pub(crate) fn story_section<'a>(
    cx: &'a FrameScope,
    theme: CoreTheme,
    title: &'a str,
    hint: &'a str,
) -> Div<'a> {
    let p = BasePalette::from_theme(theme);
    let mut header = div()
        .flex_row()
        .w_full()
        .gap(12)
        .items_center()
        .justify_between()
        .child(cx, text(title).size(13).color(p.fg).nowrap());
    if !hint.is_empty() {
        header = header.child(cx, text(hint).size(11).color(p.muted_fg).ellipsis());
    }
    div()
        .flex_col()
        .w_full()
        .p(16)
        .gap(12)
        .bg(p.surface)
        .border(1, p.border)
        .child(cx, header)
        .child(cx, div().w_full().h(1).bg(p.border))
}

/// 渲染紧凑的状态回显标签（实时展示交互结果，0px 直角）。
pub(crate) fn state_pill<'a>(
    cx: &'a FrameScope,
    theme: CoreTheme,
    label: impl Into<StrRef<'a>>,
) -> Div<'a> {
    let p = BasePalette::from_theme(theme);
    div()
        .px(8)
        .py(3)
        .bg(p.surface_subtle)
        .border(1, p.border)
        .child(cx, text(label).size(11).color(p.muted_fg))
}
