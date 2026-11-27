//! 颜色选择器组件 `ColorPicker` 与色块组件 `ColorSwatch`。

use velox_engine::prelude::*;

/// 颜色选择器预置调色板状态（提供 12 种常用色板与自定义 RGB 状态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPickerState {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub open: bool,
}

impl Default for ColorPickerState {
    fn default() -> Self {
        Self::from_color(Color::rgb(59, 130, 246))
    }
}

impl ColorPickerState {
    /// 默认预置调色板。
    pub const PRESET_PALETTE: [Color; 12] = [
        Color::rgb(239, 68, 68),
        Color::rgb(249, 115, 22),
        Color::rgb(245, 158, 11),
        Color::rgb(234, 179, 8),
        Color::rgb(132, 204, 22),
        Color::rgb(34, 197, 94),
        Color::rgb(16, 185, 129),
        Color::rgb(6, 182, 212),
        Color::rgb(59, 130, 246),
        Color::rgb(99, 102, 241),
        Color::rgb(168, 85, 247),
        Color::rgb(236, 72, 153),
    ];

    /// 从 [`Color`] 创建颜色状态。
    #[inline]
    pub const fn from_color(c: Color) -> Self {
        Self {
            r: c.r(),
            g: c.g(),
            b: c.b(),
            open: false,
        }
    }

    /// 转换为 [`Color`]。
    #[inline]
    pub const fn color(&self) -> Color {
        Color::rgb(self.r, self.g, self.b)
    }

    /// 切换弹出调色板开关。
    #[inline]
    pub fn toggle_open(&mut self) {
        self.open = !self.open;
    }

    /// 设置当前选中的 [`Color`]。
    #[inline]
    pub fn set_color(&mut self, c: Color) {
        self.r = c.r();
        self.g = c.g();
        self.b = c.b();
    }
}

/// 传递给 `ColorPicker::swatch(...)` 插槽回调的色块状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorSwatchState {
    pub index: usize,
    pub color: Color,
    pub selected: bool,
}

impl ColorSwatchState {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn color(&self) -> Color { self.color }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.selected }
}

type ColorSwatchRenderer<'a> =
    &'a dyn Fn(Div<'a>, ColorSwatchState, &'a FrameScope) -> Element<'a>;
type ColorTriggerRenderer<'a> = &'a dyn Fn(Div<'a>, Color, bool, &'a FrameScope) -> Element<'a>;

/// 单个可点击色块预览基础组件（`ColorSwatch`）。
pub struct ColorSwatch<'a> {
    pub id: u32,
    pub color: Color,
    pub size: i16,
    pub selected: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub border_color: Color,
    pub selected_border_color: Color,
    pub rounded: u8,
    pub on_click: Option<&'a dyn Fn(Color, &mut ActionScope)>,
}

impl<'a> ColorSwatch<'a> {
    /// 创建一个色块预览组件（未显式设置边框色与圆角时回退 `Theme` 默认样式）。
    #[inline(always)]
    pub const fn new(id: u32, color: Color) -> Self {
        Self {
            id,
            color,
            size: 22,
            selected: false,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            border_color: Color::TRANSPARENT,
            selected_border_color: Color::TRANSPARENT,
            rounded: 0,
            on_click: None,
        }
    }

    /// 设置是否处于选中态。
    #[inline(always)]
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// 绑定点击回调。
    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(Color, &mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(ColorSwatch<'a>);

impl<'a> Widget<'a> for ColorSwatch<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let border_color = self.border_color;
        let selected_border_color = if self.selected_border_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.selected_border_color
        };
        let rounded = self.rounded;

        let border_col = if self.selected {
            selected_border_color
        } else {
            border_color
        };
        let mut d = div()
            .id(self.id)
            .w(self.size)
            .h(self.size)
            .rounded(rounded)
            .bg(self.color)
            .focusable()
            .no_focus_border();
        if border_col != Color::TRANSPARENT {
            d = d.border(if self.selected { 2 } else { 1 }, border_col);
        }

        if let Some(cb) = self.on_click {
            let c = self.color;
            let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                cb(c, scope);
            });
            d = d.on_click_raw(click_cb);
        }
        self.style_ext.apply_to_div(d, self.style).into_element(cx)
    }
}

/// 颜色选择器组件（`ColorPicker`）。
pub struct ColorPicker<'a> {
    pub id: u32,
    pub state: ColorPickerState,
    pub palette: &'a [Color],
    pub style: Style,
    pub style_ext: StyleExt,
    pub bg: Color,
    pub popup_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub swatch_border_color: Color,
    pub rounded: u8,
    pub animated: bool,
    pub on_toggle_open: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_select_color: Option<&'a dyn Fn(Color, &mut ActionScope)>,
    trigger_renderer: Option<ColorTriggerRenderer<'a>>,
    swatch_renderer: Option<ColorSwatchRenderer<'a>>,
}

impl<'a> ColorPicker<'a> {
    /// 创建一个颜色选择器（未显式设置配色与圆角时回退 `Theme` 默认样式）。
    #[inline(always)]
    pub const fn new(id: u32, state: ColorPickerState) -> Self {
        Self {
            id,
            state,
            palette: &ColorPickerState::PRESET_PALETTE,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            bg: Color::TRANSPARENT,
            popup_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            swatch_border_color: Color::TRANSPARENT,
            rounded: 0,
            animated: false,
            on_toggle_open: None,
            on_select_color: None,
            trigger_renderer: None,
            swatch_renderer: None,
        }
    }

    /// 启用或禁用调色板弹层开关过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定点击触发按钮开关面板的回调。
    #[inline(always)]
    pub fn on_toggle_open(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_open = Some(handler);
        self
    }

    /// 绑定选中某种颜色的回调。
    #[inline(always)]
    pub fn on_select_color(mut self, handler: &'a dyn Fn(Color, &mut ActionScope)) -> Self {
        self.on_select_color = Some(handler);
        self
    }

    /// `gpui-base` 风格自定义触发器插槽回调（零堆分配）。
    #[inline]
    pub fn trigger<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, Color, bool) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, color: Color, open: bool, frame_cx: &'a FrameScope| {
                f(d, color, open).into_element(frame_cx)
            },
        );
        self.trigger_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格调色板色块插槽回调（零堆分配）。
    #[inline]
    pub fn swatch<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, ColorSwatchState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |d: Div<'a>, state: ColorSwatchState, frame_cx: &'a FrameScope| {
                f(d, state).into_element(frame_cx)
            },
        );
        self.swatch_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(ColorPicker<'a>);

impl<'a> Widget<'a> for ColorPicker<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let popup_bg = self.popup_bg;
        let border_color = self.border_color;
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let swatch_border_color = self.swatch_border_color;
        let rounded = self.rounded;

        let cur = self.state.color();
        let hex_label = fmt!(cx, "#{:02X}{:02X}{:02X}", self.state.r, self.state.g, self.state.b);
        let swatch_r = rounded.saturating_sub(2);

        let mut cur_swatch = div().w(16).h(16).rounded(swatch_r).bg(cur);
        if swatch_border_color != Color::TRANSPARENT {
            cur_swatch = cur_swatch.border(1, swatch_border_color);
        }

        let mut trigger = div()
            .id(self.id)
            .flex_row()
            .w(124)
            .h(30)
            .px(8)
            .gap(8)
            .rounded(rounded)
            .bg(bg)
            .items_center()
            .focusable()
            .no_focus_border()
            .child(cx, cur_swatch)
            .child(cx, text(hex_label).size(12).color(text_color));
        if border_color != Color::TRANSPARENT {
            trigger = trigger.border(1, border_color);
        }

        if let Some(cb) = self.on_toggle_open {
            trigger = trigger.on_click_raw(cb);
        }

        let trigger_el = match self.trigger_renderer {
            Some(f) => f(trigger, cur, self.state.open, cx),
            None => trigger.into_element(cx),
        };
        let mut root = div().flex_col().w(124).child(cx, trigger_el);

        if !self.state.open {
            return self.style_ext.apply_to_div(root, self.style).into_element(cx);
        }

        let num_rows = ((self.palette.len() + 5) / 6) as i16;
        let panel_h = if num_rows > 0 {
            num_rows.saturating_mul(22).saturating_add((num_rows - 1).saturating_mul(6)).saturating_add(16)
        } else {
            16
        };

        let mut panel = div()
            .id(crate::scoped_child_id(self.id, 0x1FD7_200F, 100))
            .pos(0, 34)
            .flex_col()
            .w(172)
            .h(panel_h)
            .p(8)
            .gap(6)
            .rounded(rounded)
            .bg(popup_bg)
            .overlay()
            .occlude_hit();
        if border_color != Color::TRANSPARENT {
            panel = panel.border(1, border_color);
        }
        if self.animated {
            panel = panel
                .slide_in(0, -6, Duration::from_millis(140))
                .slide_out(0, -6, Duration::from_millis(120));
        }

        let mut row = div().flex_row().gap(6).items_center();
        let mut count_in_row = 0usize;
        for (idx, &c) in self.palette.iter().enumerate() {
            let is_sel = c.0 == cur.0;
            if let Some(sw_fn) = self.swatch_renderer {
                let mut cell = div()
                    .id(crate::scoped_child_id(self.id, 0x1FD7_200F, idx as u32 + 1))
                    .focusable()
                    .no_focus_border();
                if let Some(cb) = self.on_select_color {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(c, scope);
                    });
                    cell = cell.on_click_raw(click_cb);
                }
                row = row.child(
                    cx,
                    sw_fn(
                        cell,
                        ColorSwatchState {
                            index: idx,
                            color: c,
                            selected: is_sel,
                        },
                        cx,
                    ),
                );
            } else {
                let mut swatch =
                    ColorSwatch::new(crate::scoped_child_id(self.id, 0x1FD7_200F, idx as u32 + 1), c).selected(is_sel);
                swatch.border_color = border_color;
                swatch.selected_border_color = text_color;
                swatch.rounded = swatch_r;
                if let Some(cb) = self.on_select_color {
                    swatch = swatch.on_click(cb);
                }
                row = row.child(cx, swatch);
            }
            count_in_row += 1;
            if count_in_row == 6 {
                panel = panel.child(cx, row);
                row = div().flex_row().gap(6).items_center();
                count_in_row = 0;
            }
        }
        if count_in_row > 0 {
            panel = panel.child(cx, row);
        }

        root = root.child(cx, panel);
        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
