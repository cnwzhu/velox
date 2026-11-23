//! `Velox Base Gallery` 应用层手写样式助手（贴近 `gpui-base` 极简高对比黑白直角风格）。
//!
//! `velox-base` 本身是 **完全无样式（Headless / Unstyled）** 的行为原语层，默认零背景、零圆角、
//! 零边框、零阴影。本模块演示开发者如何在应用层用简洁的辅助函数，为无样式的 `velox_kit::base::*`
//! 原语按需赋予自定义外观（此处采用 0px 锐利直角 + 高对比单色线框风格，与 `velox-component`
//! 的圆角蓝调 Shadcn 主题形成清晰对比）。

use velox_kit::base::{
    Accordion, Alert, AlertDialog, AlertVariant, ArcGauge, Avatar, Badge, Button, Checkbox,
    CodeEditor, ContextMenu, DataTable, Dialog, Dropdown, Kbd, List, Modal, Notification,
    NotificationVariant, NumberInput, Pagination, Popover, ProgressBar, Radio, RadioGroup, Select,
    Settings, Sheet, Sidebar, Slider, Stepper, Switch, Tabs, TabsVariant, Tag, TagVariant,
    TextInput, Toggle, ToggleGroup, Tooltip, Tree,
};
use velox_kit::prelude::*;

/// 应用层极简高对比单色调色板（严格区分暗色与亮色模式，0px 锐利直角）。
#[derive(Copy, Clone)]
pub(crate) struct BasePalette {
    pub is_dark: bool,
    pub canvas: Color,
    pub surface: Color,
    pub surface_subtle: Color,
    pub border: Color,
    pub border_strong: Color,
    pub fg: Color,
    pub muted_fg: Color,
    pub solid_bg: Color,
    pub solid_fg: Color,
    pub solid_hover: Color,
    pub solid_press: Color,
    pub outline_bg: Color,
    pub outline_fg: Color,
    pub outline_hover: Color,
    pub outline_press: Color,
    pub hover: Color,
    pub active: Color,
    pub danger_bg: Color,
    pub danger_fg: Color,
}

impl BasePalette {
    pub(crate) fn from_theme(theme: CoreTheme) -> Self {
        let is_dark = theme.background.r() < 128;
        if is_dark {
            Self {
                is_dark: true,
                canvas: theme.background,                 // #09090B
                surface: Color::rgb(17, 17, 20),          // #111114
                surface_subtle: Color::rgb(24, 24, 27),   // #18181B
                border: Color::rgb(46, 46, 50),           // #2E2E32
                border_strong: Color::rgb(68, 68, 74),    // #44444A
                fg: Color::rgb(250, 250, 250),            // #FAFAFA
                muted_fg: Color::rgb(161, 161, 170),      // #A1A1AA
                solid_bg: Color::rgb(255, 255, 255),      // 纯白底（gpui-base Save changes 同款）
                solid_fg: Color::rgb(9, 9, 11),           // 深黑字
                solid_hover: Color::rgb(228, 228, 231),   // Zinc 200
                solid_press: Color::rgb(212, 212, 216),   // Zinc 300
                outline_bg: Color::rgb(20, 20, 23),       // 暗底（gpui-base Cancel 同款）
                outline_fg: Color::rgb(250, 250, 250),    // 纯白字
                outline_hover: Color::rgb(31, 31, 36),    // 微妙提亮
                outline_press: Color::rgb(39, 39, 42),
                hover: Color::rgb(28, 28, 32),
                active: Color::rgb(39, 39, 42),
                danger_bg: Color::rgb(220, 38, 38),
                danger_fg: Color::WHITE,
            }
        } else {
            Self {
                is_dark: false,
                canvas: theme.background,                 // #FFFFFF
                surface: Color::rgb(250, 250, 250),       // #FAFAFA
                surface_subtle: Color::rgb(244, 244, 245), // #F4F4F5
                border: Color::rgb(228, 228, 231),        // #E4E4E7
                border_strong: Color::rgb(212, 212, 216), // #D4D4D8
                fg: Color::rgb(9, 9, 11),                 // #09090B
                muted_fg: Color::rgb(113, 113, 122),      // #71717A
                solid_bg: Color::rgb(9, 9, 11),           // 纯黑底
                solid_fg: Color::rgb(255, 255, 255),      // 纯白字
                solid_hover: Color::rgb(39, 39, 42),
                solid_press: Color::rgb(63, 63, 70),
                outline_bg: Color::rgb(255, 255, 255),
                outline_fg: Color::rgb(9, 9, 11),
                outline_hover: Color::rgb(244, 244, 245),
                outline_press: Color::rgb(228, 228, 231),
                hover: Color::rgb(244, 244, 245),
                active: Color::rgb(228, 228, 231),
                danger_bg: Color::rgb(220, 38, 38),
                danger_fg: Color::WHITE,
            }
        }
    }
}

/// 实心高对比主按钮（暗色：纯白底黑字直角 `Save changes`；亮色：纯黑底白字直角）。
pub(crate) fn btn_solid<'a>(mut b: Button<'a>, theme: CoreTheme) -> Button<'a> {
    let p = BasePalette::from_theme(theme);
    if matches!(b.style.height, Length::Auto) {
        b = b.h(34);
    }
    if b.style.padding.left == 0 && b.style.padding.right == 0 {
        b = b.px(16);
    }
    b.bg(p.solid_bg)
        .text_color(p.solid_fg)
        .hover_bg(p.solid_hover)
        .active_bg(p.solid_press)
        .focus_ring(true)
        .rounded(0)
}

/// 1px 线框次按钮（暗色：暗底灰框白字直角 `Cancel`；亮色：白底灰框黑字直角）。
pub(crate) fn btn_outline<'a>(mut b: Button<'a>, theme: CoreTheme) -> Button<'a> {
    let p = BasePalette::from_theme(theme);
    if matches!(b.style.height, Length::Auto) {
        b = b.h(34);
    }
    if b.style.padding.left == 0 && b.style.padding.right == 0 {
        b = b.px(16);
    }
    b.bg(p.outline_bg)
        .border(1, p.border_strong)
        .text_color(p.outline_fg)
        .hover_bg(p.outline_hover)
        .active_bg(p.outline_press)
        .focus_ring(true)
        .rounded(0)
}

/// 柔和填充次级按钮（无强烈轮廓，直角）。
pub(crate) fn btn_subtle<'a>(mut b: Button<'a>, theme: CoreTheme) -> Button<'a> {
    let p = BasePalette::from_theme(theme);
    if matches!(b.style.height, Length::Auto) {
        b = b.h(34);
    }
    if b.style.padding.left == 0 && b.style.padding.right == 0 {
        b = b.px(14);
    }
    b.bg(p.surface_subtle)
        .border(1, p.border)
        .text_color(p.fg)
        .hover_bg(p.hover)
        .active_bg(p.active)
        .focus_ring(true)
        .rounded(0)
}

/// 透明幽灵按钮（悬停显背景，直角）。
pub(crate) fn btn_ghost<'a>(mut b: Button<'a>, theme: CoreTheme) -> Button<'a> {
    let p = BasePalette::from_theme(theme);
    if matches!(b.style.height, Length::Auto) {
        b = b.h(32);
    }
    if b.style.padding.left == 0 && b.style.padding.right == 0 {
        b = b.px(12);
    }
    b.bg(Color::TRANSPARENT)
        .text_color(p.muted_fg)
        .hover_bg(p.hover)
        .active_bg(p.active)
        .focus_ring(true)
        .rounded(0)
}

/// 危险操作按钮（直角高对比红底白字）。
pub(crate) fn btn_danger<'a>(mut b: Button<'a>, theme: CoreTheme) -> Button<'a> {
    let p = BasePalette::from_theme(theme);
    if matches!(b.style.height, Length::Auto) {
        b = b.h(34);
    }
    if b.style.padding.left == 0 && b.style.padding.right == 0 {
        b = b.px(16);
    }
    b.bg(p.danger_bg)
        .text_color(p.danger_fg)
        .hover_bg(p.danger_bg.lerp(Color::WHITE, 26))
        .active_bg(p.danger_bg.lerp(Color::BLACK, 32))
        .focus_ring(true)
        .rounded(0)
}

/// 工具栏紧凑型按钮（28px 高，12px 字）。
pub(crate) fn btn_toolbar<'a>(b: Button<'a>, solid: bool, theme: CoreTheme) -> Button<'a> {
    let sized = b.font_size(12).h(28).px(10);
    if solid {
        btn_solid(sized, theme)
    } else {
        btn_outline(sized, theme)
    }
}

/// 极简直角徽章：实心反转 / 线框 / 柔和 / 语义着色。
pub(crate) fn badge_solid<'a>(b: Badge<'a>, theme: CoreTheme) -> Badge<'a> {
    let p = BasePalette::from_theme(theme);
    b.font_size(11)
        .px(8)
        .py(3)
        .bg(p.solid_bg)
        .text_color(p.solid_fg)
        .rounded(0)
}

pub(crate) fn badge_outline<'a>(b: Badge<'a>, theme: CoreTheme) -> Badge<'a> {
    let p = BasePalette::from_theme(theme);
    b.font_size(11)
        .px(8)
        .py(3)
        .bg(p.outline_bg)
        .border(1, p.border_strong)
        .text_color(p.fg)
        .rounded(0)
}

pub(crate) fn badge_subtle<'a>(b: Badge<'a>, theme: CoreTheme) -> Badge<'a> {
    let p = BasePalette::from_theme(theme);
    b.font_size(11)
        .px(8)
        .py(3)
        .bg(p.surface_subtle)
        .border(1, p.border)
        .text_color(p.muted_fg)
        .rounded(0)
}

pub(crate) fn badge_tint<'a>(b: Badge<'a>, accent: Color, theme: CoreTheme) -> Badge<'a> {
    let p = BasePalette::from_theme(theme);
    let bg = p.surface.lerp(accent, if p.is_dark { 36 } else { 24 });
    b.font_size(11)
        .px(8)
        .py(3)
        .bg(bg)
        .border(1, accent)
        .text_color(accent)
        .rounded(0)
}

/// 手动样式化的直角 Toggle。
pub(crate) fn toggle_outline<'a>(mut t: Toggle<'a>, theme: CoreTheme) -> Toggle<'a> {
    let p = BasePalette::from_theme(theme);
    t.active_bg = p.solid_bg;
    t.active_fg = p.solid_fg;
    t.inactive_bg = p.outline_bg;
    t.inactive_fg = p.fg;
    t.border_color = p.border_strong;
    t.border_width = 1;
    t.hover_bg = Some(p.outline_hover);
    t.active_press_bg = Some(p.outline_press);
    t.focus_ring = true;
    t.rounded = 0;
    t.h(32).px(12)
}

pub(crate) fn toggle_ghost<'a>(mut t: Toggle<'a>, theme: CoreTheme) -> Toggle<'a> {
    let p = BasePalette::from_theme(theme);
    t.active_bg = p.active;
    t.active_fg = p.fg;
    t.inactive_bg = Color::TRANSPARENT;
    t.inactive_fg = p.muted_fg;
    t.border_color = Color::TRANSPARENT;
    t.border_width = 0;
    t.hover_bg = Some(p.hover);
    t.active_press_bg = Some(p.active);
    t.focus_ring = true;
    t.rounded = 0;
    t.h(32).px(12)
}

pub(crate) fn toggle_solid<'a>(mut t: Toggle<'a>, theme: CoreTheme) -> Toggle<'a> {
    let p = BasePalette::from_theme(theme);
    t.active_bg = p.solid_bg;
    t.active_fg = p.solid_fg;
    t.inactive_bg = p.surface_subtle;
    t.inactive_fg = p.muted_fg;
    t.border_color = p.border;
    t.border_width = 1;
    t.hover_bg = Some(p.hover);
    t.active_press_bg = Some(p.active);
    t.focus_ring = true;
    t.rounded = 0;
    t.h(32).px(12)
}

pub(crate) fn toggle_group<'a>(mut g: ToggleGroup<'a>, theme: CoreTheme) -> ToggleGroup<'a> {
    let p = BasePalette::from_theme(theme);
    g.item_height = 32;
    g.item_px = 12;
    g.active_bg = p.solid_bg;
    g.active_fg = p.solid_fg;
    g.inactive_bg = p.outline_bg;
    g.inactive_fg = p.muted_fg;
    g.border_color = p.border_strong;
    g.border_width = 1;
    g.hover_bg = Some(p.outline_hover);
    g.active_press_bg = Some(p.outline_press);
    g.focus_ring = true;
    g.rounded = 0;
    g.gap = 4;
    g
}

/// 手动样式化的直角 Checkbox（勾选时高对比实心反转）。
pub(crate) fn checkbox<'a>(mut c: Checkbox<'a>, theme: CoreTheme) -> Checkbox<'a> {
    let p = BasePalette::from_theme(theme);
    let filled = c.checked || c.indeterminate;
    c.box_rounded = 0;
    c.box_bg = if filled { p.solid_bg } else { p.outline_bg };
    c.border_color = if filled { p.solid_bg } else { p.border_strong };
    c.check_color = p.solid_fg;
    c.label_color = if c.disabled { p.muted_fg } else { p.fg };
    c.hover_bg = Some(if filled { p.solid_hover } else { p.outline_hover });
    c.active_bg = Some(if filled { p.solid_press } else { p.outline_press });
    c.focus_ring = true;
    c
}

/// 手动样式化的 Radio 与 RadioGroup。
pub(crate) fn radio<'a>(mut r: Radio<'a>, theme: CoreTheme) -> Radio<'a> {
    let p = BasePalette::from_theme(theme);
    r.accent_color = p.solid_bg;
    r.unchecked_bg = p.outline_bg;
    r.border_color = p.border_strong;
    r.dot_color = p.solid_fg;
    r.text_color = p.fg;
    r.disabled_color = p.muted_fg;
    r.hover_bg = Some(p.outline_hover);
    r.active_bg = Some(p.outline_press);
    r.fill_on_checked = true;
    r
}

pub(crate) fn radio_group<'a>(mut g: RadioGroup<'a>, theme: CoreTheme) -> RadioGroup<'a> {
    let p = BasePalette::from_theme(theme);
    g.accent_color = p.solid_bg;
    g.unchecked_bg = p.outline_bg;
    g.border_color = p.border_strong;
    g.dot_color = p.solid_fg;
    g.text_color = p.fg;
    g.disabled_color = p.muted_fg;
    g.hover_bg = Some(p.outline_hover);
    g.active_bg = Some(p.outline_press);
    g.checked_hover_bg = Some(p.solid_hover);
    g.checked_active_bg = Some(p.solid_press);
    g.fill_on_checked = true;
    g
}

/// 手动样式化的直角高对比 Switch。
pub(crate) fn switch<'a>(mut s: Switch<'a>, theme: CoreTheme) -> Switch<'a> {
    let p = BasePalette::from_theme(theme);
    if s.active_color == Color::TRANSPARENT {
        s.active_color = p.solid_bg;
    }
    s.track_bg = if s.checked {
        s.active_color
    } else {
        p.surface_subtle
    };
    s.thumb_bg = if s.checked { p.solid_fg } else { p.fg };
    s.border_width = 1;
    s.border_color = if s.checked {
        s.active_color
    } else {
        p.border_strong
    };
    s.label_color = if s.disabled { p.muted_fg } else { p.fg };
    s.track_rounded = Some(0);
    s.thumb_rounded = Some(0);
    s.hover_bg = Some(if s.checked {
        s.active_color.lerp(p.solid_fg, 28)
    } else {
        p.hover
    });
    s.focus_ring = true;
    s
}

/// 手动样式化的直角高对比 Slider。
pub(crate) fn slider<'a>(mut s: Slider<'a>, theme: CoreTheme) -> Slider<'a> {
    let p = BasePalette::from_theme(theme);
    s.track_bg = p.surface_subtle;
    s.track_border_color = p.border;
    if s.fill_bg == Color::TRANSPARENT {
        s.fill_bg = p.solid_bg;
    }
    s.thumb_bg = p.solid_bg;
    s.thumb_border_color = p.canvas;
    s.thumb_border_width = 1;
    s.track_rounded = Some(0);
    s.thumb_rounded = Some(0);
    s.thumb_hover_bg = Some(p.solid_hover);
    s.thumb_active_bg = Some(p.solid_press);
    s
}

/// 手动样式化的直角 ProgressBar（显式开启 `animated = true` 以演示平滑过渡与收敛）。
pub(crate) fn progress_bar(mut bar: ProgressBar, theme: CoreTheme) -> ProgressBar {
    let p = BasePalette::from_theme(theme);
    bar.track_color = p.surface_subtle;
    bar.fill_color = Some(p.solid_bg);
    bar.rounded = Some(0);
    bar.animated = true;
    bar
}

/// 手动样式化的高对比 ArcGauge。
pub(crate) fn arc_gauge<'a>(mut g: ArcGauge<'a>, theme: CoreTheme) -> ArcGauge<'a> {
    let p = BasePalette::from_theme(theme);
    g.track_color = Some(p.border);
    g.fill_color = Some(p.solid_bg);
    g.label_color = p.fg;
    g
}

/// 手动样式化的直角 TextInput（保留 `theme.primary` 清晰光标与选区色）。
pub(crate) fn text_input<'a>(mut t: TextInput<'a>, theme: CoreTheme) -> TextInput<'a> {
    let p = BasePalette::from_theme(theme);
    t.bg = if t.disabled { p.surface_subtle } else { p.outline_bg };
    t.border_color = p.border_strong;
    t.focus_border_color = p.solid_bg;
    t.text_color = if t.disabled { p.muted_fg } else { p.fg };
    t.placeholder_color = p.muted_fg;
    // 显式指定高可见度主色光标（同时满足 150% DPI 光标亚像素精度检测）
    t.cursor_color = theme.primary;
    t.selection_bg = theme.primary.scale_alpha(78);
    t.selection_rounded = 0;
    t.clear_icon_color = p.muted_fg;
    t.clear_hover_bg = Some(p.hover);
    t.clear_active_bg = Some(p.active);
    t.rounded = 0;
    t.focus_ring = true;
    t
}

pub(crate) fn number_input<'a>(mut n: NumberInput<'a>, theme: CoreTheme) -> NumberInput<'a> {
    let p = BasePalette::from_theme(theme);
    n.bg = p.outline_bg;
    n.border_color = p.border_strong;
    n.text_color = p.fg;
    n.button_bg = p.surface_subtle;
    n.button_hover_bg = Some(p.hover);
    n.button_active_bg = Some(p.active);
    n.icon_color = p.fg;
    n.rounded = 0;
    n.focus_ring = true;
    n
}

pub(crate) fn select<'a>(mut s: Select<'a>, theme: CoreTheme) -> Select<'a> {
    let p = BasePalette::from_theme(theme);
    s.bg = p.outline_bg;
    s.border_color = p.border_strong;
    s.active_border_color = p.solid_bg;
    s.popup_bg = p.surface;
    s.popup_border_color = p.border_strong;
    s.selected_bg = p.active;
    s.selected_fg = p.fg;
    s.check_color = p.solid_bg;
    s.text_color = p.fg;
    s.placeholder_color = p.muted_fg;
    s.disabled_color = p.muted_fg.scale_alpha(120);
    s.arrow_color = p.muted_fg;
    s.hover_bg = Some(p.hover);
    s.active_bg = Some(p.active);
    s.rounded = 0;
    s.focus_ring = true;
    s
}

pub(crate) fn dropdown<'a>(mut d: Dropdown<'a>, theme: CoreTheme) -> Dropdown<'a> {
    let p = BasePalette::from_theme(theme);
    d.header_bg = p.outline_bg;
    d.border_color = p.border_strong;
    d.active_border_color = p.solid_bg;
    d.popup_bg = p.surface;
    d.popup_border_color = p.border_strong;
    d.selected_bg = p.active;
    d.text_color = p.fg;
    d.selected_text_color = p.fg;
    d.check_color = p.solid_bg;
    d.arrow_color = p.muted_fg;
    d.hover_bg = Some(p.hover);
    d.active_bg = Some(p.active);
    d.rounded = 0;
    d.focus_ring = true;
    d
}

pub(crate) fn code_editor<'a>(mut e: CodeEditor<'a>, theme: CoreTheme) -> CodeEditor<'a> {
    let p = BasePalette::from_theme(theme);
    e.bg = p.surface;
    e.gutter_bg = p.canvas;
    e.active_line_bg = p.hover;
    e.selection_bg = theme.primary.scale_alpha(78);
    e.match_bg = theme.warning.scale_alpha(72);
    e.border_color = p.border;
    e.focus_border_color = p.solid_bg;
    e.divider_color = p.border;
    e.text_color = p.fg;
    e.muted_color = p.muted_fg;
    e.active_line_num_color = p.fg;
    e.cursor_color = theme.primary;
    e.rounded = 0;
    e.focus_ring = true;
    e
}

pub(crate) fn sidebar<'a>(mut s: Sidebar<'a>, theme: CoreTheme) -> Sidebar<'a> {
    let p = BasePalette::from_theme(theme);
    s.bg = theme.sidebar;
    s.border_color = theme.sidebar;
    s.title_color = p.fg;
    s.section_title_color = p.muted_fg;
    s.text_color = p.muted_fg;
    s.muted_color = p.muted_fg;
    s.active_bg = p.active;
    s.active_fg = p.fg;
    s.active_icon_color = p.solid_bg;
    s.hover_bg = Some(p.hover);
    s.press_bg = Some(p.active);
    s.badge_bg = p.surface_subtle;
    s.badge_fg = p.muted_fg;
    s.collapse_btn_bg = p.surface_subtle;
    s.focus_ring = true;
    s.rounded = 0;
    s
}

pub(crate) fn tabs<'a>(mut t: Tabs<'a>, theme: CoreTheme) -> Tabs<'a> {
    let p = BasePalette::from_theme(theme);
    match t.variant {
        TabsVariant::Segmented => {
            t.bar_bg = p.surface_subtle;
            t.bar_border_color = p.border;
            t.bar_pad = 3;
            t.active_bg = p.solid_bg;
            t.active_fg = p.solid_fg;
            t.inactive_bg = Color::TRANSPARENT;
            t.inactive_fg = p.muted_fg;
            t.hover_bg = Some(p.hover);
            t.active_press_bg = Some(p.active);
        }
        TabsVariant::Underline => {
            t.bar_bg = Color::TRANSPARENT;
            t.bar_border_color = Color::TRANSPARENT;
            t.bar_pad = 0;
            t.active_bg = Color::TRANSPARENT;
            t.active_fg = p.fg;
            t.inactive_bg = Color::TRANSPARENT;
            t.inactive_fg = p.muted_fg;
            t.hover_bg = Some(p.hover);
            t.underline_active_color = p.solid_bg;
            t.underline_inactive_color = p.border;
        }
        TabsVariant::Pill => {
            t.bar_bg = Color::TRANSPARENT;
            t.bar_border_color = Color::TRANSPARENT;
            t.bar_pad = 0;
            t.active_bg = p.solid_bg;
            t.active_fg = p.solid_fg;
            t.inactive_bg = p.outline_bg;
            t.inactive_fg = p.muted_fg;
            t.hover_bg = Some(p.hover);
            t.active_press_bg = Some(p.active);
        }
    }
    t.bar_rounded = 0;
    t.item_rounded = 0;
    t.focus_ring = true;
    t
}

pub(crate) fn accordion<'a>(mut a: Accordion<'a>, theme: CoreTheme) -> Accordion<'a> {
    let p = BasePalette::from_theme(theme);
    a.header_bg = p.surface;
    a.content_bg = p.canvas;
    a.border_color = p.border;
    a.text_color = p.fg;
    a.body_text_color = p.muted_fg;
    a.arrow_color = p.muted_fg;
    a.hover_bg = Some(p.hover);
    a.active_bg = Some(p.active);
    a.rounded = 0;
    a.focus_ring = true;
    a
}

pub(crate) fn stepper<'a>(mut s: Stepper<'a>, theme: CoreTheme) -> Stepper<'a> {
    let p = BasePalette::from_theme(theme);
    s.badge_rounded = 0;
    s.active_bg = p.solid_bg;
    s.active_border = p.solid_bg;
    s.active_fg = p.solid_fg;
    s.completed_bg = p.surface_subtle;
    s.completed_border = p.solid_bg;
    s.completed_fg = p.fg;
    s.error_bg = p.danger_bg;
    s.error_border = p.danger_bg;
    s.error_fg = p.danger_fg;
    s.pending_bg = p.outline_bg;
    s.pending_border = p.border_strong;
    s.pending_fg = p.muted_fg;
    s.title_color = p.fg;
    s.desc_color = p.muted_fg;
    s.line_color = p.border;
    s.completed_line_color = p.solid_bg;
    s
}

pub(crate) fn pagination<'a>(mut pg: Pagination<'a>, theme: CoreTheme) -> Pagination<'a> {
    let p = BasePalette::from_theme(theme);
    pg.item_bg = p.outline_bg;
    pg.active_bg = p.solid_bg;
    pg.border_color = p.border_strong;
    pg.text_color = p.fg;
    pg.active_text_color = p.solid_fg;
    pg.disabled_text_color = p.muted_fg.scale_alpha(120);
    pg.hover_bg = Some(p.outline_hover);
    pg.press_bg = Some(p.outline_press);
    pg.active_hover_bg = Some(p.solid_hover);
    pg.active_press_bg = Some(p.solid_press);
    pg.rounded = 0;
    pg.focus_ring = true;
    pg
}

pub(crate) fn dialog<'a>(mut d: Dialog<'a>, theme: CoreTheme) -> Dialog<'a> {
    let p = BasePalette::from_theme(theme);
    d.bg = p.surface;
    d.border_color = p.border_strong;
    d.title_color = p.fg;
    d.description_color = p.muted_fg;
    d.close_color = p.muted_fg;
    d.close_hover_bg = Some(p.hover);
    d.close_active_bg = Some(p.active);
    d.rounded = 0;
    d.focus_ring = true;
    d
}

pub(crate) fn alert_dialog<'a>(mut d: AlertDialog<'a>, theme: CoreTheme) -> AlertDialog<'a> {
    let p = BasePalette::from_theme(theme);
    d.bg = p.surface;
    d.border_color = p.border_strong;
    d.title_color = p.fg;
    d.description_color = p.muted_fg;
    d.cancel_bg = p.outline_bg;
    d.cancel_border_color = p.border_strong;
    d.cancel_fg = p.outline_fg;
    d.cancel_hover_bg = Some(p.outline_hover);
    d.cancel_active_bg = Some(p.outline_press);
    d.confirm_bg = if d.destructive { p.danger_bg } else { p.solid_bg };
    d.confirm_fg = if d.destructive { p.danger_fg } else { p.solid_fg };
    d.confirm_hover_bg = Some(if d.destructive {
        p.danger_bg.lerp(Color::WHITE, 26)
    } else {
        p.solid_hover
    });
    d.confirm_active_bg = Some(if d.destructive {
        p.danger_bg.lerp(Color::BLACK, 32)
    } else {
        p.solid_press
    });
    d.rounded = 0;
    d.btn_rounded = 0;
    d.focus_ring = true;
    d
}

pub(crate) fn modal<'a>(mut m: Modal<'a>, theme: CoreTheme) -> Modal<'a> {
    let p = BasePalette::from_theme(theme);
    m.backdrop_bg = Color::rgba(0, 0, 0, 150);
    m.card_bg = p.surface;
    m.border_color = p.border_strong;
    m.title_color = p.fg;
    m.close_color = p.muted_fg;
    m.close_hover_bg = Some(p.hover);
    m.close_active_bg = Some(p.active);
    m.rounded = 0;
    m.focus_ring = true;
    m
}

pub(crate) fn sheet<'a>(mut s: Sheet<'a>, theme: CoreTheme) -> Sheet<'a> {
    let p = BasePalette::from_theme(theme);
    s.bg = p.surface;
    s.border_color = p.border_strong;
    s.title_color = p.fg;
    s.close_color = p.muted_fg;
    s.close_rounded = 0;
    s.close_hover_bg = Some(p.hover);
    s.close_active_bg = Some(p.active);
    s.rounded = 0;
    s.focus_ring = true;
    s
}

pub(crate) fn notification<'a>(mut n: Notification<'a>, theme: CoreTheme) -> Notification<'a> {
    let p = BasePalette::from_theme(theme);
    let accent = match n.variant {
        NotificationVariant::Success => theme.success,
        NotificationVariant::Warning => theme.warning,
        NotificationVariant::Error => theme.danger,
        NotificationVariant::Info => p.solid_bg,
    };
    n.bg = p.surface;
    n.border_color = p.border_strong;
    n.title_color = p.fg;
    n.text_color = p.muted_fg;
    n.accent_color = accent;
    n.action_bg = p.surface_subtle;
    n.action_rounded = 0;
    n.action_hover_bg = Some(p.hover);
    n.action_active_bg = Some(p.active);
    n.close_color = p.muted_fg;
    n.close_rounded = 0;
    n.close_hover_bg = Some(p.hover);
    n.close_active_bg = Some(p.active);
    n.rounded = 0;
    n.focus_ring = true;
    n
}

pub(crate) fn context_menu<'a>(mut cm: ContextMenu<'a>, theme: CoreTheme) -> ContextMenu<'a> {
    let p = BasePalette::from_theme(theme);
    cm.menu.bg = p.surface;
    cm.menu.border_color = p.border_strong;
    cm.menu.separator_color = p.border;
    cm.menu.text_color = p.fg;
    cm.menu.muted_color = p.muted_fg;
    cm.menu.disabled_color = p.muted_fg.scale_alpha(110);
    cm.menu.destructive_color = p.danger_bg;
    cm.menu.hover_bg = Some(p.hover);
    cm.menu.active_bg = Some(p.active);
    cm.menu.rounded = 0;
    cm.menu.focus_ring = true;
    cm
}

pub(crate) fn alert<'a>(mut a: Alert<'a>, theme: CoreTheme) -> Alert<'a> {
    let p = BasePalette::from_theme(theme);
    let accent = match a.variant {
        AlertVariant::Info => p.solid_bg,
        AlertVariant::Success => theme.success,
        AlertVariant::Warning => theme.warning,
        AlertVariant::Error => theme.danger,
    };
    a.bg = p.surface;
    a.border_color = accent;
    a.title_color = p.fg;
    a.text_color = p.muted_fg;
    a.close_color = p.muted_fg;
    a.close_hover_bg = Some(p.hover);
    a.close_active_bg = Some(p.active);
    a.rounded = 0;
    a.focus_ring = true;
    a
}

pub(crate) fn tag<'a>(mut t: Tag<'a>, theme: CoreTheme) -> Tag<'a> {
    let p = BasePalette::from_theme(theme);
    let (bg, border, fg) = match t.variant {
        TagVariant::Primary => (p.solid_bg, p.solid_bg, p.solid_fg),
        TagVariant::Success => (p.surface, theme.success, theme.success),
        TagVariant::Warning => (p.surface, theme.warning, theme.warning),
        TagVariant::Danger => (p.surface, theme.danger, theme.danger),
        TagVariant::Outline | TagVariant::Default => (p.outline_bg, p.border_strong, p.fg),
    };
    t.bg = bg;
    t.border_color = border;
    t.text_color = fg;
    t.hover_bg = Some(p.hover);
    t.active_bg = Some(p.active);
    t.close_hover_bg = Some(p.hover);
    t.close_active_bg = Some(p.active);
    t.rounded = 0;
    t
}

pub(crate) fn kbd<'a>(mut k: Kbd<'a>, theme: CoreTheme) -> Kbd<'a> {
    let p = BasePalette::from_theme(theme);
    k.key_bg = p.surface_subtle;
    k.border_color = p.border_strong;
    k.text_color = p.fg;
    k.separator_color = p.muted_fg;
    k.rounded = 0;
    k
}

pub(crate) fn avatar<'a>(mut a: Avatar<'a>, theme: CoreTheme) -> Avatar<'a> {
    let p = BasePalette::from_theme(theme);
    a.bg = p.surface_subtle;
    a.fg = p.fg;
    a.border_color = p.border_strong;
    a.rounded = 0;
    a
}

pub(crate) fn tooltip<'a>(mut t: Tooltip<'a>, theme: CoreTheme) -> Tooltip<'a> {
    let p = BasePalette::from_theme(theme);
    t.bg = p.solid_bg;
    t.border_color = p.solid_bg;
    t.text_color = p.solid_fg;
    t.rounded = 0;
    t
}

pub(crate) fn popover<'a>(mut pop: Popover<'a>, theme: CoreTheme) -> Popover<'a> {
    let p = BasePalette::from_theme(theme);
    pop.bg = p.surface;
    pop.border_color = p.border_strong;
    pop.rounded = 0;
    pop
}

pub(crate) fn data_table<'a>(mut dt: DataTable<'a>, theme: CoreTheme) -> DataTable<'a> {
    let p = BasePalette::from_theme(theme);
    dt.bg = p.surface;
    dt.stripe_bg = p.canvas;
    dt.header_bg = p.surface_subtle;
    dt.border_color = p.border;
    dt.text_color = p.fg;
    dt.muted_color = p.muted_fg;
    dt.accent_color = p.solid_bg;
    dt.selected_bg = p.active;
    dt.selected_fg = p.fg;
    dt.header_hover_bg = Some(p.hover);
    dt.header_active_bg = Some(p.active);
    dt.row_hover_bg = Some(p.hover);
    dt.row_active_bg = Some(p.active);
    dt.rounded = 0;
    dt
}

pub(crate) fn list<'a>(mut l: List<'a>, theme: CoreTheme) -> List<'a> {
    let p = BasePalette::from_theme(theme);
    l.bg = p.surface;
    l.border_color = p.border;
    l.text_color = p.fg;
    l.muted_color = p.muted_fg;
    l.selected_bg = p.active;
    l.selected_fg = p.fg;
    l.hover_bg = Some(p.hover);
    l.active_bg = Some(p.active);
    l.rounded = 0;
    l.focus_ring = true;
    l
}

pub(crate) fn tree<'a>(mut t: Tree<'a>, theme: CoreTheme) -> Tree<'a> {
    let p = BasePalette::from_theme(theme);
    t.bg = p.surface;
    t.border_color = p.border;
    t.text_color = p.fg;
    t.muted_color = p.muted_fg;
    t.selected_bg = p.active;
    t.selected_fg = p.fg;
    t.hover_bg = Some(p.hover);
    t.active_bg = Some(p.active);
    t.rounded = 0;
    t.focus_ring = true;
    t
}

pub(crate) fn settings<'a>(mut s: Settings<'a>, theme: CoreTheme) -> Settings<'a> {
    let p = BasePalette::from_theme(theme);
    s.bg = p.surface;
    s.border_color = p.border;
    s.text_color = p.fg;
    s.muted_color = p.muted_fg;
    s.control_bg = p.surface_subtle;
    s.active_bg = p.solid_bg;
    s.active_fg = p.solid_fg;
    s.switch_off_bg = p.border_strong;
    s.switch_thumb_bg = p.solid_fg;
    s.switch_rounded = 0;
    s.row_hover_bg = Some(p.hover);
    s.row_active_bg = Some(p.active);
    s.action_hover_bg = Some(p.solid_hover);
    s.rounded = 0;
    s.focus_ring = true;
    s
}
