//! 弹性盒容器构造器 `Div<'a>` 与链式样式、交互及动画配置 API。

use core::cell::Cell;
use core::time::Duration;

use crate::pod::{ActionScope, CursorShape, DragEvent, FrameScope};
use crate::style::{AlignItems, Color, Easing, Edges, LoopMode, Style, StyleRefinement, Styled};

use super::div_node::{
    duration_to_ms, pack_visual_flags, DivInteractivity, DivNode, DivRareHandlers, DivStateStyles,
    DivVisual, EnterFrom, TransitionSpec, FOCUS_BORDER_NONE, NO_FOCUS_BORDER_DIV_VISUAL,
};
use super::{Element, ElementNode, IntoElement};

impl<'a> IntoElement<'a> for Div<'a> {
    #[inline(always)]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        let has_state_styles = self.text_color.is_some()
            || self.text_size.is_some()
            || self.hover_style.is_some()
            || self.active_style.is_some()
            || self.focus_style.is_some();

        let alloc_visual = |state_styles: Option<&'a DivStateStyles<'a>>,
                            transition: Option<&'a TransitionSpec>|
         -> &'a DivVisual<'a> {
            let (pos_x, pos_y) = self.pos.unwrap_or((0, 0));
            let (bg_grad_to, bg_grad_horiz) = self.bg_grad.unwrap_or((Color::TRANSPARENT, false));
            let (focus_border_w, focus_border_color) = match self.focus_border {
                Some((w, c)) => (w.min(254), c),
                None => (FOCUS_BORDER_NONE, Color::TRANSPARENT),
            };
            let (shadow_blur, shadow_ox, shadow_oy, shadow_color) =
                self.shadow.unwrap_or((0, 0, 0, Color::TRANSPARENT));
            let extra_flags = pack_visual_flags(
                self.pos.is_some(),
                self.scrollable_max.is_some(),
                self.bg_grad.is_some(),
                bg_grad_horiz,
                self.items_baseline,
                self.align_self,
            );
            &*cx.arena().alloc_temp(DivVisual {
                state_styles,
                transition,
                margin: self.margin,
                extra_padding: self.extra_padding,
                bg_grad_to,
                focus_border_color,
                shadow_color,
                min_w: self.min_w,
                max_w: self.max_w,
                min_h: self.min_h,
                max_h: self.max_h,
                pos_x,
                pos_y,
                raw_scrollable_max: self.scrollable_max.unwrap_or(0),
                extra_flags,
                focus_border_w,
                shadow_blur,
                shadow_ox,
                shadow_oy,
                cursor: self.cursor,
            })
        };

        let visual = if self.has_non_focus_visual_extra() {
            let state_styles = if has_state_styles {
                let hover_style = self.hover_style.map(|r| &*cx.arena().alloc_temp(r));
                let active_style = self.active_style.map(|r| &*cx.arena().alloc_temp(r));
                let focus_style = self.focus_style.map(|r| &*cx.arena().alloc_temp(r));
                Some(&*cx.arena().alloc_temp(DivStateStyles {
                    hover_style,
                    active_style,
                    focus_style,
                    text_color: self.text_color.unwrap_or(Color::TRANSPARENT),
                    text_size: self.text_size.unwrap_or(0),
                    flags: if self.text_color.is_some() { 1 } else { 0 },
                }))
            } else {
                None
            };
            let transition = self.transition.map(|t| &*cx.arena().alloc_temp(t));
            Some(alloc_visual(state_styles, transition))
        } else if let Some((fw, fc)) = self.focus_border {
            if fw == 0 && fc == Color::TRANSPARENT {
                Some(&NO_FOCUS_BORDER_DIV_VISUAL)
            } else {
                Some(alloc_visual(None, None))
            }
        } else {
            None
        };

        let interact = if self.has_interactivity() {
            let rare = if self.has_rare_handlers() {
                Some(&*cx.arena().alloc(DivRareHandlers {
                    on_long_press: self.on_long_press,
                    on_drag: self.on_drag,
                    on_swipe: self.on_swipe,
                    on_key_input: self.on_key_input,
                    on_key_nav: self.on_key_nav,
                    on_navigate_back: self.on_navigate_back,
                    on_secondary_click: self.on_secondary_click,
                }))
            } else {
                None
            };
            Some(&*cx.arena().alloc(DivInteractivity {
                on_click: self.on_click,
                rare,
            }))
        } else {
            None
        };

        let mut flags = 0u8;
        if self.focusable {
            flags |= 1 << 0;
        }
        if self.is_overlay {
            flags |= 1 << 1;
        }
        if self.occlude_hit {
            flags |= 1 << 2;
        }
        if self.auto_repeat {
            flags |= 1 << 3;
        }
        if self.clip_children {
            flags |= 1 << 4;
        }
        if self.id.is_some() {
            flags |= 1 << 5;
        }
        if self.active_bg.is_some() {
            flags |= 1 << 6;
        }
        if self.hover_bg.is_some() {
            flags |= 1 << 7;
        }

        Element::Div(DivNode {
            style: self.style,
            first_child: self.first_child,
            visual,
            interact,
            raw_id: self.id.unwrap_or(0),
            raw_active_bg: self.active_bg.unwrap_or(Color::TRANSPARENT),
            raw_hover_bg: self.hover_bg.unwrap_or(Color::TRANSPARENT),
            opacity: self.opacity,
            z_index: self.z_index,
            grid_cols: self.grid_cols,
            flags,
        })
    }
}


/// 基础弹性盒容器原语（完全复刻 HTML/Tailwind 的 `div` 直觉）。
pub struct Div<'a> {
    /// 跨帧唯一 ID（驱动滚动位置记忆与 `.animate_xxx()` 过渡动画）。
    pub id: Option<u32>,
    /// 紧凑位域样式（严格 24 Bytes）。
    pub style: Style,
    /// 可选的四边独立外边距（配合 `style.margin_px()` 使用）。
    pub margin: Edges<i16>,
    /// 可选的大尺寸安全区或内边距扩展（像素，与 `style.padding` 叠加，避免移动端软键盘 > 127px 截断）。
    pub extra_padding: Edges<i16>,
    /// 最小宽度约束（0 表示不限）。
    pub min_w: i16,
    /// 最大宽度约束（0 表示不限）。
    pub max_w: i16,
    /// 最小高度约束（0 表示不限）。
    pub min_h: i16,
    /// 最大高度约束（0 表示不限）。
    pub max_h: i16,
    /// 容器与子树整体不透明度（`0..=255`，默认 `255`）。
    pub opacity: u8,
    /// 按下态（Active）背景色覆盖。
    pub active_bg: Option<Color>,
    /// 悬停态（Hover）背景色覆盖。
    pub hover_bg: Option<Color>,
    /// 线性渐变终点颜色与方向 `(to_color, is_horizontal)`。
    pub bg_grad: Option<(Color, bool)>,
    /// 键盘/旋钮聚焦态（Focus）边框 `(width, color)` 覆盖。
    pub focus_border: Option<(u8, Color)>,
    /// 是否可纳入物理按键/编码器 `Tab`/`Up`/`Down` 焦点链。
    pub focusable: bool,
    /// 是否将该子树提升至顶层 Z-Order 延迟渲染队列（用于 `Modal` / `Dropdown` / `Tooltip` 越过父裁剪框）。
    pub is_overlay: bool,
    /// 是否拦截并吞噬落在该容器内的指针命中事件（防止点击穿透 `Modal` 遮罩层）。
    pub occlude_hit: bool,
    /// 按住不放时是否自动连发 `on_click`（用于 `+/-` 连续调节按钮）。
    pub auto_repeat: bool,
    /// 可触摸拖拽滚动的最大垂直偏移量（`Some(max_px)` 表示启用触摸惯性滚动容器）。
    pub scrollable_max: Option<i16>,
    /// 可选的相对于父容器内容区左上角的叠层偏移坐标 `(x, y)`（用于表盘/徽标/浮层叠放）。
    pub pos: Option<(i16, i16)>,
    /// 第一个子节点引用（侵入式单向链表头）。
    pub first_child: Option<&'a ElementNode<'a>>,
    /// 最后一个子节点引用（用于 $O(1)$ 安全尾插，消除裸指针 UB）。
    pub last_child: Option<&'a ElementNode<'a>>,
    /// 点击事件响应闭包（分配于 `ScratchArena`）。
    pub on_click: Option<&'a dyn Fn(&mut ActionScope)>,
    /// 长按事件响应闭包（按住超过 `500ms` 触发，分配于 `ScratchArena`）。
    pub on_long_press: Option<&'a dyn Fn(&mut ActionScope)>,
    /// 拖拽事件响应闭包（分配于 `ScratchArena`）。
    pub on_drag: Option<&'a dyn Fn(&mut ActionScope)>,
    /// 四向快速滑动手势回调闭包（分配于 `ScratchArena`）。
    pub on_swipe: Option<&'a dyn Fn(crate::pod::SwipeDir, &mut ActionScope)>,
    /// 字符与文本输入事件响应闭包（分配于 `ScratchArena`，供聚焦态 `TextInput` 编辑使用）。
    pub on_key_input: Option<&'a dyn Fn(crate::pod::KeyCode, &mut ActionScope)>,
    /// 控件局部导航键（方向键、`Home`/`End`、`Enter`/`Space`）响应闭包（不抢占 `Tab`/`BackTab` 焦点切换，亦不触发文本 IME）。
    pub on_key_nav: Option<&'a dyn Fn(crate::pod::KeyCode, &mut ActionScope)>,
    /// 系统返回导航请求回调闭包（Android Back / 浏览器后退 / 左边缘返回手势 / 桌面 `Escape` 关闭浮层）。
    pub on_navigate_back: Option<&'a dyn Fn(&mut ActionScope)>,
    /// 鼠标右键（次键）点击回调闭包（分配于 `ScratchArena`）。
    pub on_secondary_click: Option<&'a dyn Fn(&mut ActionScope)>,
    /// 显式層级排序优先级（默认 `0`；用于控制同层或浮层间的绘制先后顺序）。
    pub z_index: i8,
    /// 解析式 SDF 软阴影 `(blur_radius, offset_x, offset_y, color)`（无需离屏模糊缓冲区）。
    pub shadow: Option<(u8, i8, i8, Color)>,
    /// 二维网格布局列数（`0` 表示常规 Flexbox，`>= 1` 表示按指定列数自动排列的二维 Grid）。
    pub grid_cols: u8,
    /// 声明式过渡动画配置。
    pub transition: Option<TransitionSpec>,
    /// 是否裁剪超出容器边界的子节点（如滚动列表视口）。
    pub clip_children: bool,
    /// 子节点独立交叉轴对齐方式覆盖（`align-self`）。
    pub align_self: Option<AlignItems>,
    /// 显式指定的鼠标光标形状（`None` 时按节点交互能力自动推断 `Pointer` / `Text` / `Grab` / `Default`）。
    pub cursor: Option<CursorShape>,
    /// 容器级可继承文本颜色（未显式设 `.color(...)` 的子 `Text` 自动继承）。
    pub text_color: Option<Color>,
    /// 容器级可继承字号（未显式设 `.size(...)` 的子 `Text` 自动继承）。
    pub text_size: Option<u8>,
    /// 悬停态样式覆写增量（由 `.hover(|s| ...)` 写入，支持背景/边框/文本色/不透明度/圆角）。
    pub hover_style: Option<StyleRefinement>,
    /// 按下态样式覆写增量（由 `.active(|s| ...)` 写入）。
    pub active_style: Option<StyleRefinement>,
    /// 聚焦态样式覆写增量（由 `.focus(|s| ...)` 写入）。
    pub focus_style: Option<StyleRefinement>,
    /// 行容器内是否按子元素文本基线对齐（`items-baseline`）。
    pub items_baseline: bool,
}

/// 构造一个基础弹性盒容器（完全复刻 HTML/Tailwind 的 `div` 直觉）。
#[inline(always)]
pub fn div<'a>() -> Div<'a> {
    Div {
        id: None,
        style: Style::default(),
        margin: Edges::all(0),
        extra_padding: Edges::all(0),
        min_w: 0,
        max_w: 0,
        min_h: 0,
        max_h: 0,
        opacity: 255,
        active_bg: None,
        hover_bg: None,
        bg_grad: None,
        focus_border: None,
        focusable: false,
        is_overlay: false,
        occlude_hit: false,
        auto_repeat: false,
        scrollable_max: None,
        pos: None,
        first_child: None,
        last_child: None,
        on_click: None,
        on_long_press: None,
        on_drag: None,
        on_swipe: None,
        on_key_input: None,
        on_key_nav: None,
        on_navigate_back: None,
        on_secondary_click: None,
        z_index: 0,
        shadow: None,
        grid_cols: 0,
        transition: None,
        clip_children: false,
        align_self: None,
        cursor: None,
        text_color: None,
        text_size: None,
        hover_style: None,
        active_style: None,
        focus_style: None,
        items_baseline: false,
    }
}

impl<'a> Styled for Div<'a> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl<'a> Div<'a> {
    #[inline(always)]
    fn has_non_focus_visual_extra(&self) -> bool {
        self.margin != Edges::all(0)
            || self.extra_padding != Edges::all(0)
            || self.min_w != 0
            || self.max_w != 0
            || self.min_h != 0
            || self.max_h != 0
            || self.bg_grad.is_some()
            || self.scrollable_max.is_some()
            || self.pos.is_some()
            || self.shadow.is_some()
            || self.transition.is_some()
            || self.align_self.is_some()
            || self.cursor.is_some()
            || self.text_color.is_some()
            || self.text_size.is_some()
            || self.hover_style.is_some()
            || self.active_style.is_some()
            || self.focus_style.is_some()
            || self.items_baseline
    }

    #[inline(always)]
    fn has_interactivity(&self) -> bool {
        self.on_click.is_some() || self.has_rare_handlers()
    }

    #[inline(always)]
    fn has_rare_handlers(&self) -> bool {
        self.on_long_press.is_some()
            || self.on_drag.is_some()
            || self.on_swipe.is_some()
            || self.on_key_input.is_some()
            || self.on_key_nav.is_some()
            || self.on_navigate_back.is_some()
            || self.on_secondary_click.is_some()
    }

    /// 设置元素的跨帧唯一标识符（用于过渡动画与滚动位置记忆）。
    #[inline(always)]
    pub fn id(mut self, id: u32) -> Self {
        self.id = Some(id);
        self
    }

    /// 基于当前 [`FrameScope`] 作用域 ID 栈派生全局唯一的元素标识符（避免复用组件时 ID 撞车）。
    #[inline(always)]
    pub fn scoped_id(mut self, cx: &FrameScope, local_id: u32) -> Self {
        self.id = Some(cx.scoped_id(local_id));
        self
    }

    /// 设置相对于父容器内容区左上角的叠层偏移坐标 `(x, y)`（脱离常规线性流，用于叠层拼装）。
    #[inline(always)]
    pub fn pos(mut self, x: i16, y: i16) -> Self {
        self.pos = Some((x, y));
        self
    }

    /// 自动叠加移动端/异形屏安全区域内边距（状态栏、刘海屏、底部 Home Indicator 与软键盘高度，支持 >127px 软键盘不截断）。
    #[inline(always)]
    pub fn safe_area(mut self, cx: &FrameScope) -> Self {
        let sa = cx.safe_area();
        let kb = cx.keyboard_inset();
        self.extra_padding.top = self.extra_padding.top.saturating_add(sa.top.max(0));
        self.extra_padding.right = self.extra_padding.right.saturating_add(sa.right.max(0));
        self.extra_padding.bottom = self
            .extra_padding
            .bottom
            .saturating_add(sa.bottom.saturating_add(kb).max(0));
        self.extra_padding.left = self.extra_padding.left.saturating_add(sa.left.max(0));
        self
    }

    /// 自动叠加顶部安全区域内边距（用于顶部导航栏 `NavBar` 避开刘海屏/状态栏）。
    #[inline(always)]
    pub fn pt_safe(mut self, cx: &FrameScope) -> Self {
        let sa = cx.safe_area();
        self.extra_padding.top = self.extra_padding.top.saturating_add(sa.top.max(0));
        self
    }

    /// 自动叠加底部安全区域与软键盘内边距（用于底部 `TabBar` 或 `BottomSheet` 避开 Home Indicator 与软键盘）。
    #[inline(always)]
    pub fn pb_safe(mut self, cx: &FrameScope) -> Self {
        let sa = cx.safe_area();
        let kb = cx.keyboard_inset();
        self.extra_padding.bottom = self
            .extra_padding
            .bottom
            .saturating_add(sa.bottom.saturating_add(kb).max(0));
        self
    }

    /// 设置水平外边距（左右）。
    #[inline(always)]
    pub fn mx(mut self, px: i16) -> Self {
        self.margin.left = px;
        self.margin.right = px;
        self
    }

    /// 设置垂直外边距（上下）。
    #[inline(always)]
    pub fn my(mut self, px: i16) -> Self {
        self.margin.top = px;
        self.margin.bottom = px;
        self
    }

    /// 设置上外边距。
    #[inline(always)]
    pub fn mt(mut self, px: i16) -> Self {
        self.margin.top = px;
        self
    }

    /// 设置下外边距。
    #[inline(always)]
    pub fn mb(mut self, px: i16) -> Self {
        self.margin.bottom = px;
        self
    }

    /// 设置左外边距。
    #[inline(always)]
    pub fn ml(mut self, px: i16) -> Self {
        self.margin.left = px;
        self
    }

    /// 设置右外边距。
    #[inline(always)]
    pub fn mr(mut self, px: i16) -> Self {
        self.margin.right = px;
        self
    }

    /// 设置最小宽度像素约束。
    #[inline(always)]
    pub fn min_w(mut self, px: i16) -> Self {
        self.min_w = px.max(0);
        self
    }

    /// 设置最大宽度像素约束。
    #[inline(always)]
    pub fn max_w(mut self, px: i16) -> Self {
        self.max_w = px.max(0);
        self
    }

    /// 设置最小高度像素约束。
    #[inline(always)]
    pub fn min_h(mut self, px: i16) -> Self {
        self.min_h = px.max(0);
        self
    }

    /// 设置最大高度像素约束。
    #[inline(always)]
    pub fn max_h(mut self, px: i16) -> Self {
        self.max_h = px.max(0);
        self
    }

    /// 设置容器及其子树的整体不透明度（`0..=255`）。
    #[inline(always)]
    pub fn opacity(mut self, alpha: u8) -> Self {
        self.opacity = alpha;
        self
    }

    /// 设置指针按下（Active）时的背景色。
    #[inline(always)]
    pub fn active_bg(mut self, color: Color) -> Self {
        self.active_bg = Some(color);
        self
    }

    /// 设置指针悬停（Hover）时的背景色。
    #[inline(always)]
    pub fn hover_bg(mut self, color: Color) -> Self {
        self.hover_bg = Some(color);
        self
    }

    /// GPUI 风格悬停态样式闭包（如 `.hover(|s| s.bg(rgb(0xf5f5f5)).border_color(rgb(0x171717)))`）。
    ///
    /// 若闭包仅覆写背景色，自动退化为零分配 `hover_bg` 快速路径；否则完整保留边框、文本色、不透明度与圆角覆写。
    #[inline(always)]
    pub fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        let base = self.hover_style.unwrap_or_else(|| {
            let mut r = StyleRefinement::new();
            r.bg = self.hover_bg;
            r
        });
        let refined = f(base);
        if refined.is_empty() {
            return self;
        }
        if refined.is_bg_only() {
            self.hover_bg = refined.bg;
            self.hover_style = None;
        } else {
            if let Some(bg) = refined.bg {
                self.hover_bg = Some(bg);
            }
            self.hover_style = Some(refined);
        }
        self
    }

    /// GPUI 风格按下态（Active）样式闭包（如 `.active(|s| s.bg(rgb(0xe5e5e5)))`）。
    #[inline(always)]
    pub fn active(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        let base = self.active_style.unwrap_or_else(|| {
            let mut r = StyleRefinement::new();
            r.bg = self.active_bg;
            r
        });
        let refined = f(base);
        if refined.is_empty() {
            return self;
        }
        if refined.is_bg_only() {
            self.active_bg = refined.bg;
            self.active_style = None;
        } else {
            if let Some(bg) = refined.bg {
                self.active_bg = Some(bg);
            }
            self.active_style = Some(refined);
        }
        self
    }

    /// GPUI 风格聚焦态（Focus）样式闭包（如 `.focus(|s| s.border_1().border_color(rgb(0x3b82f6)))`），自动开启 `focusable`。
    #[inline(always)]
    pub fn focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.focusable = true;
        let refined = f(self.focus_style.unwrap_or_default());
        if !refined.is_empty() {
            self.focus_style = Some(refined);
        }
        self
    }

    /// 设置容器级可继承文本颜色（子树中未显式调用 `.color(...)` 的 `text(...)` 自动继承）。
    #[inline(always)]
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    /// 设置容器级可继承字号像素（子树中未显式调用 `.size(...)` 的 `text(...)` 自动继承）。
    #[inline(always)]
    pub fn text_size(mut self, px: u8) -> Self {
        self.text_size = Some(px);
        self
    }

    /// 容器级可继承字号 Tailwind 简写（`11px` / `12px` / `14px` / `16px` / `18px` / `22px` / `28px`）。
    #[inline(always)] pub fn text_xs(self) -> Self { self.text_size(11) }
    #[inline(always)] pub fn text_sm(self) -> Self { self.text_size(12) }
    #[inline(always)] pub fn text_base(self) -> Self { self.text_size(14) }
    #[inline(always)] pub fn text_lg(self) -> Self { self.text_size(16) }
    #[inline(always)] pub fn text_xl(self) -> Self { self.text_size(18) }
    #[inline(always)] pub fn text_2xl(self) -> Self { self.text_size(22) }
    #[inline(always)] pub fn text_3xl(self) -> Self { self.text_size(28) }

    /// 浮点不透明度简写（`0.0..=1.0`，映射到 `0..=255`）。
    #[inline(always)]
    pub fn opacity_f(mut self, alpha: f32) -> Self {
        let c = if alpha <= 0.0 {
            0.0
        } else if alpha >= 1.0 {
            1.0
        } else {
            alpha
        };
        self.opacity = (c * 255.0 + 0.5) as u8;
        self
    }

    // --- 外边距 Tailwind 档位简写 ---
    #[inline(always)] pub fn mx_0(self) -> Self { self.mx(0) }
    #[inline(always)] pub fn mx_1(self) -> Self { self.mx(4) }
    #[inline(always)] pub fn mx_2(self) -> Self { self.mx(8) }
    #[inline(always)] pub fn mx_3(self) -> Self { self.mx(12) }
    #[inline(always)] pub fn mx_4(self) -> Self { self.mx(16) }
    #[inline(always)] pub fn my_0(self) -> Self { self.my(0) }
    #[inline(always)] pub fn my_1(self) -> Self { self.my(4) }
    #[inline(always)] pub fn my_2(self) -> Self { self.my(8) }
    #[inline(always)] pub fn my_3(self) -> Self { self.my(12) }
    #[inline(always)] pub fn my_4(self) -> Self { self.my(16) }
    #[inline(always)] pub fn mt_1(self) -> Self { self.mt(4) }
    #[inline(always)] pub fn mt_2(self) -> Self { self.mt(8) }
    #[inline(always)] pub fn mt_3(self) -> Self { self.mt(12) }
    #[inline(always)] pub fn mt_4(self) -> Self { self.mt(16) }
    #[inline(always)] pub fn mb_1(self) -> Self { self.mb(4) }
    #[inline(always)] pub fn mb_2(self) -> Self { self.mb(8) }
    #[inline(always)] pub fn mb_3(self) -> Self { self.mb(12) }
    #[inline(always)] pub fn mb_4(self) -> Self { self.mb(16) }
    #[inline(always)] pub fn ml_1(self) -> Self { self.ml(4) }
    #[inline(always)] pub fn ml_2(self) -> Self { self.ml(8) }
    #[inline(always)] pub fn ml_3(self) -> Self { self.ml(12) }
    #[inline(always)] pub fn ml_4(self) -> Self { self.ml(16) }
    #[inline(always)] pub fn mr_1(self) -> Self { self.mr(4) }
    #[inline(always)] pub fn mr_2(self) -> Self { self.mr(8) }
    #[inline(always)] pub fn mr_3(self) -> Self { self.mr(12) }
    #[inline(always)] pub fn mr_4(self) -> Self { self.mr(16) }

    // --- 定位、显示、光标与阴影 Tailwind / GPUI 简写 ---
    /// 语义化定位标记（Velox 子节点 `.pos(x, y)` 天然相对父容器左上角定位）。
    #[inline(always)] pub fn relative(self) -> Self { self }
    /// 块级垂直容器简写（等价 `.flex_col()`）。
    #[inline(always)] pub fn block(self) -> Self { self.flex_col() }
    /// 行内弹性容器简写（等价 `.flex_row()`）。
    #[inline(always)] pub fn inline_flex(self) -> Self { self.flex_row() }

    #[inline(always)] pub fn cursor_pointer(self) -> Self { self.cursor(CursorShape::Pointer) }
    #[inline(always)] pub fn cursor_default(self) -> Self { self.cursor(CursorShape::Default) }
    #[inline(always)] pub fn cursor_text(self) -> Self { self.cursor(CursorShape::Text) }
    #[inline(always)] pub fn cursor_not_allowed(self) -> Self { self.cursor(CursorShape::NotAllowed) }
    #[inline(always)] pub fn cursor_grab(self) -> Self { self.cursor(CursorShape::Grab) }
    #[inline(always)] pub fn cursor_grabbing(self) -> Self { self.cursor(CursorShape::Grabbing) }

    #[inline(always)] pub fn shadow_none(mut self) -> Self { self.shadow = None; self }
    #[inline(always)] pub fn shadow_sm(self) -> Self { self.shadow(4, 0, 1, Color::rgba(0, 0, 0, 48)) }
    #[inline(always)] pub fn shadow_md(self) -> Self { self.shadow(10, 0, 3, Color::rgba(0, 0, 0, 72)) }
    #[inline(always)] pub fn shadow_lg(self) -> Self { self.shadow(18, 0, 6, Color::rgba(0, 0, 0, 96)) }
    #[inline(always)] pub fn shadow_xl(self) -> Self { self.shadow(28, 0, 10, Color::rgba(0, 0, 0, 120)) }

    /// 设置自顶向下的垂直线性渐变终点颜色（起点颜色为 `.bg(...)`）。
    #[inline(always)]
    pub fn bg_gradient(mut self, to_color: Color) -> Self {
        self.bg_grad = Some((to_color, false));
        self
    }

    /// 设置自左向右的水平线性渐变终点颜色（起点颜色为 `.bg(...)`）。
    #[inline(always)]
    pub fn bg_gradient_x(mut self, to_color: Color) -> Self {
        self.bg_grad = Some((to_color, true));
        self
    }

    /// 将当前节点及子树提升至顶层 Z-Order 渲染队列（脱离父级裁剪并覆盖在普通层之上，用于 `Modal` / `Dropdown`）。
    #[inline(always)]
    pub fn overlay(mut self) -> Self {
        self.is_overlay = true;
        self
    }

    /// 拦截并吞噬落在该容器区域内的指针命中事件，阻断点击穿透至底层控件（如 `Modal` 遮罩层）。
    #[inline(always)]
    pub fn occlude_hit(mut self) -> Self {
        self.occlude_hit = true;
        self
    }

    /// 开启按住不放自动连发（按住超过 `400ms` 后每 `100ms` 自动触发一次 `on_click`）。
    #[inline(always)]
    pub fn auto_repeat(mut self) -> Self {
        self.auto_repeat = true;
        self
    }

    /// 绑定原始长按回调闭包（按住 `>= 500ms` 触发）。
    #[inline(always)]
    pub fn on_long_press_raw(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_long_press = Some(handler);
        self.focusable = true;
        self
    }

    /// 绑定与 `cx.listener(...)` 兼容的长按回调闭包（按住 `>= 500ms` 触发）。
    #[inline(always)]
    pub fn on_long_press(
        mut self,
        cx: &'a FrameScope,
        handler: &'a dyn Fn(crate::pod::ClickEvent, &mut ActionScope),
    ) -> Self {
        let wrapped = cx.arena().alloc(move |acx: &mut ActionScope| {
            let (x, y) = acx.click_position();
            handler(crate::pod::ClickEvent { x, y }, acx);
        });
        self.on_long_press = Some(wrapped);
        self.focusable = true;
        self
    }

    /// 设置物理按键/旋钮焦点命中时的边框高亮样式，并自动开启 `focusable`。
    #[inline(always)]
    pub fn focus_border(mut self, width: u8, color: Color) -> Self {
        self.focus_border = Some((width, color));
        self.focusable = true;
        self
    }

    /// 显式禁止在聚焦态绘制外围焦点边框（适用于 `Slider`、透明容器、内嵌滚动视口及 `.unstyled()` 控件）。
    #[inline(always)]
    pub fn no_focus_border(mut self) -> Self {
        self.focus_border = Some((0, Color::TRANSPARENT));
        self
    }

    /// 显式指定鼠标悬停在此节点时的光标形状（如 `CursorShape::Pointer` / `Grab` / `Text` / `NotAllowed` / `ResizeEW`）。
    #[inline(always)]
    pub fn cursor(mut self, cursor: CursorShape) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// 显式将该节点加入物理按键/旋钮焦点导航链。
    #[inline(always)]
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// 显式禁止该节点抢占键盘焦点（适用于右键上下文菜单项、清空按钮与点击外部关闭遮罩，点击后保持底层输入框焦点不丢失）。
    #[inline(always)]
    pub fn unfocusable(mut self) -> Self {
        self.focusable = false;
        self
    }

    /// 将当前容器标记为可触摸拖拽/滚轮滚动的视口，并指定最大可滚动偏移量 `max_scroll`（自动开启 `overflow_hidden`）。
    #[inline(always)]
    pub fn scrollable(mut self, max_scroll: i16) -> Self {
        self.scrollable_max = Some(max_scroll.max(0));
        self.clip_children = true;
        self
    }

    /// 将当前容器标记为按内容实际尺寸自动测量最大滚动范围的视口（自动开启 `overflow_hidden`，同时支持垂直与水平滚动）。
    #[inline(always)]
    pub fn scrollable_auto(mut self) -> Self {
        self.scrollable_max = Some(i16::MAX);
        self.clip_children = true;
        self
    }

    /// 独立覆盖当前节点在父容器交叉轴上的对齐方式（`align-self`）。
    #[inline(always)]
    pub fn align_self(mut self, align: AlignItems) -> Self {
        self.align_self = Some(align);
        self
    }

    /// 交叉轴自身起始对齐（`self-start`）。
    #[inline(always)]
    pub fn self_start(self) -> Self {
        self.align_self(AlignItems::Start)
    }

    /// 交叉轴自身居中对齐（`self-center`）。
    #[inline(always)]
    pub fn self_center(self) -> Self {
        self.align_self(AlignItems::Center)
    }

    /// 交叉轴自身末尾对齐（`self-end`）。
    #[inline(always)]
    pub fn self_end(self) -> Self {
        self.align_self(AlignItems::End)
    }

    /// 交叉轴自身拉伸对齐（`self-stretch`）。
    #[inline(always)]
    pub fn self_stretch(self) -> Self {
        self.align_self(AlignItems::Stretch)
    }

    /// 在水平行容器（`flex_row`）中启用子元素按文本基线对齐（`items-baseline`）。
    #[inline(always)]
    pub fn items_baseline(mut self) -> Self {
        self.items_baseline = true;
        self
    }

    /// 启用裁剪子元素（等价 CSS `overflow: hidden`）。
    #[inline(always)]
    pub fn overflow_hidden(mut self) -> Self {
        self.clip_children = true;
        self
    }

    /// 关闭子元素裁剪（等价 CSS `overflow: visible`）。
    #[inline(always)]
    pub fn overflow_visible(mut self) -> Self {
        self.clip_children = false;
        self
    }

    /// 启用自动测量边界的滚动视口（等价 CSS `overflow: auto/scroll`）。
    #[inline(always)]
    pub fn overflow_scroll(self) -> Self {
        self.scrollable_auto()
    }

    /// 启用垂直方向自动测量滚动视口（等价 CSS `overflow-y: auto/scroll`）。
    #[inline(always)]
    pub fn overflow_y_scroll(self) -> Self {
        self.flex_col().scrollable_auto()
    }

    /// 启用水平方向自动测量滚动视口（等价 CSS `overflow-x: auto/scroll`）。
    #[inline(always)]
    pub fn overflow_x_scroll(self) -> Self {
        self.flex_row().scrollable_auto()
    }

    /// Tailwind / GPUI 风格默认过渡动画（150ms 颜色 + 不透明度 + 几何边界，EaseOut）。
    #[inline(always)]
    pub fn transition(self) -> Self {
        self.animate_all(Duration::from_millis(150))
    }

    /// Tailwind / GPUI 风格颜色过渡简写（150ms 背景/边框/文本颜色平滑过渡）。
    #[inline(always)]
    pub fn transition_colors(self) -> Self {
        self.animate_color(Duration::from_millis(150))
    }

    /// Tailwind / GPUI 风格不透明度过渡简写（150ms 淡入淡出）。
    #[inline(always)]
    pub fn transition_opacity(self) -> Self {
        self.animate_opacity(Duration::from_millis(150))
    }

    /// Tailwind / GPUI 风格全属性过渡简写（150ms 颜色 + 边界 + 不透明度）。
    #[inline(always)]
    pub fn transition_all(self) -> Self {
        self.animate_all(Duration::from_millis(150))
    }

    /// 显式禁用该节点上的过渡动画（等价 CSS `transition: none`）。
    #[inline(always)]
    pub fn transition_none(mut self) -> Self {
        self.transition = None;
        self
    }

    #[inline(always)] pub fn duration_75(self) -> Self { self.duration(Duration::from_millis(75)) }
    #[inline(always)] pub fn duration_100(self) -> Self { self.duration(Duration::from_millis(100)) }
    #[inline(always)] pub fn duration_150(self) -> Self { self.duration(Duration::from_millis(150)) }
    #[inline(always)] pub fn duration_200(self) -> Self { self.duration(Duration::from_millis(200)) }
    #[inline(always)] pub fn duration_300(self) -> Self { self.duration(Duration::from_millis(300)) }
    #[inline(always)] pub fn duration_500(self) -> Self { self.duration(Duration::from_millis(500)) }

    #[inline(always)] pub fn ease_linear(self) -> Self { self.ease(Easing::Linear) }
    #[inline(always)] pub fn ease_in(self) -> Self { self.ease(Easing::EaseIn) }
    #[inline(always)] pub fn ease_out(self) -> Self { self.ease(Easing::EaseOut) }
    #[inline(always)] pub fn ease_in_out(self) -> Self { self.ease(Easing::EaseInOut) }
    #[inline(always)] pub fn ease_spring(self) -> Self { self.ease(Easing::Spring) }

    /// 声明背景色与边框/子文本颜色的跨帧平滑过渡动画。
    #[inline(always)]
    pub fn animate_color(mut self, duration: Duration) -> Self {
        let ms = duration.as_millis().min(u16::MAX as u128) as u16;
        let mut spec = self.transition.unwrap_or(TransitionSpec {
            duration_ms: ms,
            ..TransitionSpec::default()
        });
        spec.duration_ms = ms;
        spec.animate_color = true;
        self.transition = Some(spec);
        self
    }

    /// 声明背景色的跨帧平滑过渡动画。
    #[inline(always)]
    pub fn animate_bg(self, duration: Duration) -> Self {
        self.animate_color(duration)
    }

    /// 声明几何边界框（位置/尺寸）的跨帧平滑过渡动画。
    #[inline(always)]
    pub fn animate_bounds(mut self, duration: Duration) -> Self {
        let ms = duration.as_millis().min(u16::MAX as u128) as u16;
        let mut spec = self.transition.unwrap_or(TransitionSpec {
            duration_ms: ms,
            ..TransitionSpec::default()
        });
        spec.duration_ms = ms;
        spec.animate_bounds = true;
        self.transition = Some(spec);
        self
    }

    /// 设置过渡动画的缓动曲线（`Linear` / `EaseIn` / `EaseOut` / `EaseInOut` / `Spring`）。
    #[inline(always)]
    pub fn ease(mut self, easing: Easing) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.easing = easing;
        self.transition = Some(spec);
        self
    }

    /// 设置动画为无限循环模式。
    #[inline(always)]
    pub fn repeat_infinite(mut self) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.loop_mode = LoopMode::Infinite;
        self.transition = Some(spec);
        self
    }

    /// 设置动画为无限往复（`0 -> 1 -> 0`）循环模式。
    #[inline(always)]
    pub fn repeat_ping_pong(mut self) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.loop_mode = LoopMode::PingPong;
        self.transition = Some(spec);
        self
    }

    /// 修改当前过渡动画的时长（不改变已声明的插值属性）。
    #[inline(always)]
    pub fn duration(mut self, duration: Duration) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.duration_ms = duration_to_ms(duration);
        self.transition = Some(spec);
        self
    }

    /// 设置过渡/入场动画开始前的延迟（用于列表项交错 `stagger` 入场等编排）。
    #[inline(always)]
    pub fn delay(mut self, delay: Duration) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.delay_ms = duration_to_ms(delay);
        self.transition = Some(spec);
        self
    }

    /// 声明容器整体不透明度（`.opacity(...)`）的跨帧淡入淡出过渡，作用于整个子树。
    #[inline(always)]
    pub fn animate_opacity(mut self, duration: Duration) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.duration_ms = duration_to_ms(duration);
        spec.animate_opacity = true;
        self.transition = Some(spec);
        self
    }

    /// 同时声明颜色、几何边界与不透明度的跨帧过渡（等价 CSS `transition: all`）。
    #[inline(always)]
    pub fn animate_all(mut self, duration: Duration) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.duration_ms = duration_to_ms(duration);
        spec.animate_color = true;
        spec.animate_bounds = true;
        spec.animate_opacity = true;
        self.transition = Some(spec);
        self
    }

    /// 声明首次出现时的入场动画：从不透明度 `opacity` 与偏移 `(dx, dy)` 平滑过渡到布局目标状态。
    #[inline(always)]
    pub fn enter_from(mut self, opacity: u8, dx: i16, dy: i16, duration: Duration) -> Self {
        let mut spec = self.transition.unwrap_or_default();
        spec.duration_ms = duration_to_ms(duration);
        spec.enter = Some(EnterFrom { opacity, dx, dy });
        self.transition = Some(spec);
        self
    }

    /// 首次出现时从完全透明淡入（入场动画快捷方式）。
    #[inline(always)]
    pub fn fade_in(self, duration: Duration) -> Self {
        self.enter_from(0, 0, 0, duration)
    }

    /// 首次出现时从偏移 `(dx, dy)` 处滑入并同时淡入（入场动画快捷方式）。
    #[inline(always)]
    pub fn slide_in(self, dx: i16, dy: i16, duration: Duration) -> Self {
        self.enter_from(0, dx, dy, duration)
    }

    /// 声明在 `from_bg` 与当前 `.bg(...)` 之间无限往复呼吸的循环动画（如呼吸指示灯）。
    #[inline(always)]
    pub fn ping_pong_bg(mut self, from_bg: Color, duration: Duration) -> Self {
        let ms = duration.as_millis().min(u16::MAX as u128) as u16;
        let mut spec = self.transition.unwrap_or_default();
        spec.duration_ms = ms;
        spec.animate_color = true;
        spec.loop_mode = LoopMode::PingPong;
        spec.easing = Easing::EaseInOut;
        spec.pulse_from_bg = Some(from_bg);
        self.transition = Some(spec);
        self
    }

    /// 绑定原始点击回调闭包。
    #[inline(always)]
    pub fn on_click_raw(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self.focusable = true;
        self
    }

    /// 绑定与 `cx.listener(...)` 兼容的点击回调闭包。
    #[inline(always)]
    pub fn on_click(
        mut self,
        cx: &'a FrameScope,
        handler: &'a dyn Fn(crate::pod::ClickEvent, &mut ActionScope),
    ) -> Self {
        let wrapped = cx.arena().alloc(move |acx: &mut ActionScope| {
            let (x, y) = acx.click_position();
            handler(crate::pod::ClickEvent { x, y }, acx);
        });
        self.on_click = Some(wrapped);
        self.focusable = true;
        self
    }

    /// 绑定原始鼠标右键（次键）点击回调闭包。
    #[inline(always)]
    pub fn on_secondary_click_raw(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_secondary_click = Some(handler);
        self
    }

    /// 绑定与 `cx.listener(...)` 兼容的鼠标右键（次键）点击回调闭包。
    #[inline(always)]
    pub fn on_secondary_click(
        mut self,
        cx: &'a FrameScope,
        handler: &'a dyn Fn(crate::pod::ClickEvent, &mut ActionScope),
    ) -> Self {
        let wrapped = cx.arena().alloc(move |acx: &mut ActionScope| {
            let (x, y) = acx.click_position();
            handler(crate::pod::ClickEvent { x, y }, acx);
        });
        self.on_secondary_click = Some(wrapped);
        self
    }

    /// 绑定原始拖拽回调闭包。
    #[inline(always)]
    pub fn on_drag_raw(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_drag = Some(handler);
        self
    }

    /// 绑定与 `cx.listener(...)` 兼容的触摸/鼠标拖拽回调闭包（如 `Slider` 连续滑动）。
    #[inline(always)]
    pub fn on_drag(
        mut self,
        cx: &'a FrameScope,
        handler: &'a dyn Fn(DragEvent, &mut ActionScope),
    ) -> Self {
        let wrapped = cx.arena().alloc(move |acx: &mut ActionScope| {
            let ev = acx.drag_event();
            handler(ev, acx);
        });
        self.on_drag = Some(wrapped);
        self
    }

    /// 绑定与 `cx.listener(...)` 兼容的四向快速滑动手势（`SwipeDir`）回调闭包。
    #[inline(always)]
    pub fn on_swipe(
        mut self,
        _cx: &'a FrameScope,
        handler: &'a dyn Fn(crate::pod::SwipeDir, &mut ActionScope),
    ) -> Self {
        self.on_swipe = Some(handler);
        self
    }

    /// 设置列表项或动态节点的稳定键值（自动哈希为 `u32` 元素 ID，消除手动管理魔法数字冲突）。
    #[inline(always)]
    pub fn key(mut self, key: impl AsRef<[u8]>) -> Self {
        let mut h: u32 = 0x811c_9dc5;
        for &b in key.as_ref() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.id = Some(h | 1);
        self
    }

    /// 设置元素的 Z-Index 层级（用于在同父容器兄弟节点间调整前后绘制顺序；若需脱离父容器裁剪并提升至全屏最顶层，请显式调用 `.overlay()`）。
    #[inline(always)]
    pub fn z_index(mut self, z: i8) -> Self {
        self.z_index = z;
        self
    }

    /// 绑定键盘字符/编辑键回调闭包（用于聚焦态 `TextInput` 接收 `Char(c)`、`Backspace`、`Delete`、`Left`/`Right` 等）。
    #[inline(always)]
    pub fn on_key_input(
        mut self,
        handler: &'a dyn Fn(crate::pod::KeyCode, &mut ActionScope),
    ) -> Self {
        self.on_key_input = Some(handler);
        self.focusable = true;
        self
    }

    /// 绑定非文本控件的局部键盘导航回调闭包（供 `Slider` / `Tabs` / `RadioGroup` / `Select` 在聚焦或浮层打开时响应方向键、`Home`/`End`、`Enter`/`Space`，绝不吞掉 `Tab`/`BackTab` 焦点切换）。
    #[inline(always)]
    pub fn on_key_nav(
        mut self,
        handler: &'a dyn Fn(crate::pod::KeyCode, &mut ActionScope),
    ) -> Self {
        self.on_key_nav = Some(handler);
        self
    }

    /// 绑定系统级返回请求（Android Back / 浏览器后退 / 左边缘返回手势）回调闭包。
    #[inline(always)]
    pub fn on_navigate_back(
        mut self,
        handler: &'a dyn Fn(&mut ActionScope),
    ) -> Self {
        self.on_navigate_back = Some(handler);
        self
    }

    /// 设置解析式 SDF 外阴影（`blur`: 模糊扩散半径 `1..=64px`，`offset_x`/`offset_y`: 阴影偏移，`color`: 阴影颜色）。
    #[inline(always)]
    pub fn shadow(mut self, blur: u8, offset_x: i8, offset_y: i8, color: Color) -> Self {
        if blur > 0 && color.a() > 0 {
            self.shadow = Some((blur.min(64), offset_x, offset_y, color));
        }
        self
    }

    /// 将当前容器启用为指定列数 `cols` 的二维网格布局。
    #[inline(always)]
    pub fn grid(mut self, cols: u8) -> Self {
        self.grid_cols = cols.max(1);
        self
    }

    /// 链式追加单个子节点（通过 `ScratchArena` + `Cell` 侵入式链表连接，$O(1)$ 耗时，100% 遵循 Stacked Borrows 安全规范）。
    pub fn child(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        let elem = child.into_element(cx);
        let node: &'a ElementNode<'a> = cx.arena().alloc_temp(ElementNode {
            element: elem,
            next_sibling: Cell::new(None),
        });
        if let Some(last) = self.last_child {
            last.next_sibling.set(Some(node));
        } else {
            self.first_child = Some(node);
        }
        self.last_child = Some(node);
        self
    }

    /// 可选子节点追加（当 `child` 为 `Some(c)` 时追加，否则无操作）。
    #[inline(always)]
    pub fn child_opt(self, cx: &'a FrameScope, child: Option<impl IntoElement<'a>>) -> Self {
        match child {
            Some(c) => self.child(cx, c),
            None => self,
        }
    }

    /// 批量追加任意迭代器生成的子节点序列。
    pub fn children<I, E>(mut self, cx: &'a FrameScope, iter: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: IntoElement<'a>,
    {
        for item in iter {
            self = self.child(cx, item);
        }
        self
    }

    /// 按当前语言的书写方向设置主轴“逻辑起始端对齐”（LTR 为 `Start`，RTL 自动镜像为 `End`）。
    #[inline(always)]
    pub fn justify_start_localized(mut self, cx: &FrameScope) -> Self {
        if cx.is_rtl() {
            self.style.set_justify(crate::style::Justify::End);
        } else {
            self.style.set_justify(crate::style::Justify::Start);
        }
        self
    }

    /// 按当前语言的书写方向设置主轴“逻辑末端对齐”（LTR 为 `End`，RTL 自动镜像为 `Start`）。
    #[inline(always)]
    pub fn justify_end_localized(mut self, cx: &FrameScope) -> Self {
        if cx.is_rtl() {
            self.style.set_justify(crate::style::Justify::Start);
        } else {
            self.style.set_justify(crate::style::Justify::End);
        }
        self
    }

    /// 按当前语言方向设置逻辑起始端内边距（CSS `padding-inline-start`：LTR 为 `pl`，RTL 自动镜像为 `pr`）。
    #[inline(always)]
    pub fn ps(self, cx: &FrameScope, px: i8) -> Self {
        if cx.is_rtl() {
            self.pr(px)
        } else {
            self.pl(px)
        }
    }

    /// 按当前语言方向设置逻辑末端内边距（CSS `padding-inline-end`：LTR 为 `pr`，RTL 自动镜像为 `pl`）。
    #[inline(always)]
    pub fn pe(self, cx: &FrameScope, px: i8) -> Self {
        if cx.is_rtl() {
            self.pl(px)
        } else {
            self.pr(px)
        }
    }

    /// 按当前语言方向设置逻辑起始端外边距（CSS `margin-inline-start`：LTR 为 `ml`，RTL 自动镜像为 `mr`）。
    #[inline(always)]
    pub fn ms(self, cx: &FrameScope, px: i16) -> Self {
        if cx.is_rtl() {
            self.mr(px)
        } else {
            self.ml(px)
        }
    }

    /// 按当前语言方向设置逻辑末端外边距（CSS `margin-inline-end`：LTR 为 `mr`，RTL 自动镜像为 `ml`）。
    #[inline(always)]
    pub fn me(self, cx: &FrameScope, px: i16) -> Self {
        if cx.is_rtl() {
            self.ml(px)
        } else {
            self.mr(px)
        }
    }
}

