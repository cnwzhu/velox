//! 基础无样式按钮行为原语 `Button`（贴近 `gpui-base` 设计：默认零背景、零边框、零圆角、零预设尺寸与变体色，
//! 专注点击/长按/连发交互、键盘 `Tab`/`Enter`/`Space` 激活、`disabled` / `loading` / `selected` 状态管理与完整 `Styled` 链式定制）。

use velox_engine::prelude::*;

const AUTO_COLOR: Color = Color(0x0000_0001);
/// 按钮加载旋转图标周期（毫秒）：900ms 顺时针旋转一周。
const BUTTON_SPINNER_PERIOD_MS: u32 = 900;

/// 无样式基础按钮行为原语（`Button`）。
///
/// 默认不注入任何背景色、边框、圆角、内边距、高度或悬停渐变，完全交由调用方通过 [`Styled`]
/// 或上层 `velox-component` 主题样式层决定视觉呈现。
pub struct Button<'a> {
    pub id: u32,
    pub label: StrRef<'a>,
    pub disabled: bool,
    pub selected: bool,
    pub loading: bool,
    pub auto_repeat: bool,
    pub font_size: u8,
    pub text_color: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    /// 键盘聚焦态自定义边框 `(width, color)`；无样式原语默认为 `None`。
    pub focus_border: Option<(u8, Color)>,
    /// 是否使用引擎默认主题焦点环（`theme.ring`）；无样式原语默认为 `false`。
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub custom_child: Option<Element<'a>>,
    pub on_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
    pub on_long_press: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
}

impl<'a> Button<'a> {
    /// 创建一个带有唯一元素 ID 与文本标签的无样式按钮原语。
    #[inline(always)]
    pub fn new(id: u32, label: impl Into<StrRef<'a>>) -> Self {
        let style = crate::base_style! { row, items_center, justify_center };
        Self {
            id,
            label: label.into(),
            disabled: false,
            selected: false,
            loading: false,
            auto_repeat: false,
            font_size: 0,
            text_color: AUTO_COLOR,
            hover_bg: None,
            active_bg: None,
            focus_border: None,
            focus_ring: false,
            style,
            style_ext: StyleExt::DEFAULT,
            custom_child: None,
            on_click: None,
            on_long_press: None,
        }
    }

    /// 设置文本字号（默认 `13`）。
    #[inline(always)]
    pub const fn font_size(mut self, size: u8) -> Self {
        self.font_size = size;
        self
    }

    /// 设置文本颜色（未设置时仅继承 `cx.theme().foreground`）。
    #[inline(always)]
    pub const fn text_color(mut self, color: Color) -> Self {
        self.text_color = color;
        self
    }

    /// 设置悬停态背景色。
    #[inline(always)]
    pub const fn hover_bg(mut self, color: Color) -> Self {
        self.hover_bg = Some(color);
        self
    }

    /// 设置按下态背景色。
    #[inline(always)]
    pub const fn active_bg(mut self, color: Color) -> Self {
        self.active_bg = Some(color);
        self
    }

    /// 自定义按钮内部子元素内容（对齐 `gpui-kit` `Button::new(...).child(...)` 组合写法）。
    #[inline(always)]
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.custom_child = Some(child.into_element(cx));
        self
    }

    /// 设置键盘 `:focus-visible` 聚焦态边框。
    #[inline(always)]
    pub const fn focus_border(mut self, width: u8, color: Color) -> Self {
        self.focus_border = Some((width, color));
        self
    }

    /// 启用或禁用引擎默认焦点环（无样式原语默认关闭）。
    #[inline(always)]
    pub const fn focus_ring(mut self, enabled: bool) -> Self {
        self.focus_ring = enabled;
        self
    }

    /// 设置选中态标记。
    #[inline(always)]
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// 设置加载中状态（显示矢量加载弧并自动阻断点击交互）。
    #[inline(always)]
    pub const fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// 显式重置为无样式状态（`velox-base` 默认即无样式，保留此方法以便通用组合代码调用）。
    #[inline(always)]
    pub fn unstyled(mut self) -> Self {
        self.style.set_rounded(0);
        self.hover_bg = None;
        self.active_bg = None;
        self.focus_border = None;
        self.focus_ring = false;
        self
    }

    /// 启用按住自动连发（按住 `>= 400ms` 后每 `100ms` 触发一次 `on_click`）。
    #[inline(always)]
    pub const fn auto_repeat(mut self, enabled: bool) -> Self {
        self.auto_repeat = enabled;
        self
    }

    /// 绑定点击回调闭包。
    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self
    }

    /// 绑定长按回调闭包（按住 `>= 500ms` 触发）。
    #[inline(always)]
    pub fn on_long_press(mut self, handler: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.on_long_press = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Button<'a>);
crate::impl_base_disableable!(Button<'a>);

impl<'a> Widget<'a> for Button<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let font_sz = self.style_ext.text_size.unwrap_or(if self.font_size == 0 {
            13
        } else {
            self.font_size
        });
        let fg = if let Some(c) = self.style_ext.text_color {
            c
        } else if self.text_color == AUTO_COLOR {
            cx.theme().foreground
        } else {
            self.text_color
        };

        let mut d = div().id(self.id);
        d.style = self.style;
        if self.loading && d.style.gap == 0 {
            d = d.gap(6);
        }

        let is_blocked = self.disabled || self.loading;
        let ext = self.style_ext;
        if is_blocked {
            d = d
                .opacity(if self.disabled { 115 } else { 185 })
                .cursor(if self.disabled {
                    CursorShape::NotAllowed
                } else {
                    CursorShape::Progress
                });
            // 禁用/加载态跳过 hover/active/focus 样式，保留其余外层布局/过渡/裁剪修饰
            let mut safe_ext = ext;
            safe_ext.hover_style = None;
            safe_ext.active_style = None;
            safe_ext.focus_style = None;
            d = safe_ext.apply_ext_only(d);
        } else {
            d = d.focusable();
            if let Some((bw, bcol)) = self.focus_border {
                d = d.focus_border(bw, bcol);
            } else if !self.focus_ring {
                d = d.no_focus_border();
            }
            if let Some(h_bg) = self.hover_bg {
                d = d.hover_bg(h_bg);
            }
            if let Some(a_bg) = self.active_bg {
                d = d.active_bg(a_bg);
            }
            d = ext.apply_ext_only(d);
            if self.auto_repeat {
                d = d.auto_repeat();
            }
            if let Some(cb) = self.on_click {
                let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let (x, y) = acx.click_position();
                    cb(ClickEvent { x, y }, acx);
                });
                d = d.on_click_raw(raw_cb);
            }
            if let Some(lp_cb) = self.on_long_press {
                let raw_lp = cx.arena().alloc(move |acx: &mut ActionScope| {
                    let (x, y) = acx.click_position();
                    lp_cb(ClickEvent { x, y }, acx);
                });
                d = d.on_long_press_raw(raw_lp);
            }
        }

        if self.loading {
            let spin_sz = (font_sz as i16 + 2).clamp(12, 18);
            let start_deg10 = (((cx.now_ms() % BUTTON_SPINNER_PERIOD_MS) as u64 * 3600)
                / BUTTON_SPINNER_PERIOD_MS as u64) as u16;
            cx.request_frame();
            d = d.child(
                cx,
                img_arc(
                    spin_sz,
                    spin_sz,
                    2,
                    start_deg10,
                    3590,
                    2400,
                    fg.scale_alpha(56),
                    fg,
                ),
            );
        }

        if let Some(custom) = self.custom_child {
            if d.text_color.is_none() {
                d = d.text_color(fg).text_size(font_sz);
            }
            return d.child(cx, custom).into_element(cx);
        }

        // 若配置了 hover/active/focus 文本色覆盖，则让内部 text 继承容器文本色
        let has_dynamic_text_color = ext.hover_style.is_some_and(|s| s.text_color.is_some())
            || ext.active_style.is_some_and(|s| s.text_color.is_some())
            || ext.focus_style.is_some_and(|s| s.text_color.is_some());
        if has_dynamic_text_color {
            d = d.text_color(fg).text_size(font_sz);
            d.child(cx, text(self.label).text_center().nowrap())
                .into_element(cx)
        } else {
            d.child(
                cx,
                text(self.label)
                    .size(font_sz)
                    .color(fg)
                    .text_center()
                    .nowrap(),
            )
            .into_element(cx)
        }
    }
}
