//! 聊天消息气泡组件 `Bubble`。

use velox_engine::prelude::*;

/// 聊天气泡水平对齐方式（`BubbleAlign`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BubbleAlign {
    #[default]
    Start,
    End,
}

/// 消息气泡表情回应条目（`BubbleReaction`）。
#[derive(Clone, Copy, Debug)]
pub struct BubbleReaction<'a> {
    pub label: &'a str,
    pub count: u16,
    pub active: bool,
}

impl<'a> BubbleReaction<'a> {
    /// 创建一个消息表情回应条目（例如 `BubbleReaction::new("👍 3", 3, true)`）。
    #[inline(always)]
    pub const fn new(label: &'a str, count: u16, active: bool) -> Self {
        Self {
            label,
            count,
            active,
        }
    }
}

/// 聊天消息气泡表面基础组件（`Bubble`，默认无样式，支持左/右对齐、表情回应、入场弹簧动画、AI 思考波浪三点与流式输出光标）。
pub struct Bubble<'a> {
    pub id: u32,
    pub content: &'a str,
    pub align: BubbleAlign,
    pub reactions: &'a [BubbleReaction<'a>],
    pub max_width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub reaction_bg: Color,
    pub reaction_active_bg: Color,
    pub reaction_border_color: Color,
    pub reaction_active_border_color: Color,
    pub reaction_text_color: Color,
    pub reaction_active_text_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    /// 是否启用消息入场侧向弹簧滑入与表情胶囊微动效（无样式原语默认关闭，`velox-component` `std` 下默认开启）。
    pub animated: bool,
    /// 是否处于 AI 思考中状态（渲染 3 颗相位错峰波浪跳动圆点动画）。
    pub typing: bool,
    /// 是否处于流式打字机生成态（在消息末尾渲染脉冲流式光标并驱动持续刷新，结束后自动停帧休眠）。
    pub streaming: bool,
    /// 是否采用非对称聊天气泡尾角圆角（左下/右下微收角）。
    pub corner_tail: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_reaction_click: Option<&'a dyn Fn(usize, &mut ActionScope)>,
}

impl<'a> Bubble<'a> {
    /// 创建一个默认无样式的聊天消息气泡。
    #[inline(always)]
    pub const fn new(id: u32, content: &'a str) -> Self {
        Self {
            id,
            content,
            align: BubbleAlign::Start,
            reactions: &[],
            max_width: 280,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            reaction_bg: Color::TRANSPARENT,
            reaction_active_bg: Color::TRANSPARENT,
            reaction_border_color: Color::TRANSPARENT,
            reaction_active_border_color: Color::TRANSPARENT,
            reaction_text_color: Color::TRANSPARENT,
            reaction_active_text_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            animated: false,
            typing: false,
            streaming: false,
            corner_tail: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_reaction_click: None,
        }
    }

    /// 设置气泡对齐方向（`Start` 左侧他人/AI 消息，`End` 右侧当前用户消息）。
    #[inline(always)]
    pub const fn align(mut self, align: BubbleAlign) -> Self {
        self.align = align;
        self
    }

    /// 设置表情回应列表。
    #[inline(always)]
    pub const fn reactions(mut self, reactions: &'a [BubbleReaction<'a>]) -> Self {
        self.reactions = reactions;
        self
    }

    /// 设置最大宽度。
    #[inline(always)]
    pub const fn max_width(mut self, max_width: i16) -> Self {
        self.max_width = max_width;
        self
    }

    /// 启用或禁用消息气泡入场弹簧滑入与表情微动效。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 设置 AI 思考中波浪三点动画状态。
    #[inline(always)]
    pub const fn typing(mut self, typing: bool) -> Self {
        self.typing = typing;
        self
    }

    /// 设置流式打字机生成状态（末尾附加呼吸脉冲光标）。
    #[inline(always)]
    pub const fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// 绑定点击表情回应胶囊回调。
    #[inline(always)]
    pub fn on_reaction_click(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_reaction_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Bubble<'a>);

impl<'a> Widget<'a> for Bubble<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let fg = theme.foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let reaction_text_color = if self.reaction_text_color.a() > 0 {
            self.reaction_text_color
        } else {
            fg.scale_alpha(150)
        };
        let reaction_active_text_color = if self.reaction_active_text_color.a() > 0 {
            self.reaction_active_text_color
        } else {
            fg
        };

        let mut surface = div()
            .id(crate::scoped_child_id(self.id, 0x943F_DA6E, 1))
            .w(self.max_width)
            .px(12)
            .py(9)
            .flex_col()
            .gap(6)
            .bg(self.bg);
        if self.corner_tail && self.rounded >= 6 {
            let r = self.rounded;
            let tail = (r / 4).max(3);
            let radii = if self.align == BubbleAlign::End {
                CornerRadii::new(r, r, tail, r)
            } else {
                CornerRadii::new(r, r, r, tail)
            };
            surface = surface.rounded_corners(radii);
        } else {
            surface = surface.rounded(self.rounded);
        }
        if self.border_color.a() > 0 {
            surface = surface.border(1, self.border_color);
        }

        if self.typing {
            // gpui-kit 风格三颗相位错峰波浪跳动思考圆点动画（周期 960ms）
            let now = cx.now_ms();
            cx.request_frame();
            let mut dots_row = div().flex_row().h(18).gap(5).items_center();
            for i in 0u32..3 {
                let phase = ((now.wrapping_add(960 - i * 160)) % 960) as i32;
                // 仅在前 480ms 执行单峰平滑抛物线弹跳，后 480ms 停靠基线，形成清晰波浪接力感
                let (lift_px, alpha) = if phase < 480 {
                    let u = if phase <= 240 { phase } else { 480 - phase }; // 0..=240
                    let norm_q8 = (u * 256) / 240; // 0..=256
                    let ease = 256 - (((256 - norm_q8) * (256 - norm_q8)) >> 8);
                    let lift = ((ease * 5) >> 8) as i8;
                    let a = (115 + ((ease * 140) >> 8)).clamp(90, 255) as u8;
                    (lift, a)
                } else {
                    (0i8, 115u8)
                };
                let dot_wrap = div()
                    .w(8)
                    .h(16)
                    .flex_col()
                    .justify_end()
                    .pb(lift_px.max(0))
                    .items_center()
                    .child(
                        cx,
                        div()
                            .w(6)
                            .h(6)
                            .rounded(3)
                            .bg(text_color.scale_alpha(alpha)),
                    );
                dots_row = dots_row.child(cx, dot_wrap);
            }
            if !self.content.is_empty() {
                dots_row = dots_row.child(
                    cx,
                    div().pl(6).child(
                        cx,
                        text(self.content)
                            .size(11)
                            .color(text_color.scale_alpha(175)),
                    ),
                );
            }
            surface = surface.child(cx, dots_row);
        } else if self.streaming {
            // 流式打字机态：正文 + 末尾平滑脉冲流式光标指示条
            let now = cx.now_ms();
            cx.request_frame();
            let half_p = 320i32;
            let ph = (now % (half_p as u32 * 2)) as i32;
            let tri = if ph < half_p { ph } else { half_p * 2 - ph };
            let cursor_alpha = (70 + (tri * 185) / half_p).clamp(70, 255) as u8;
            let caret_color = if self.align == BubbleAlign::End {
                text_color.scale_alpha(cursor_alpha)
            } else {
                theme.primary.scale_alpha(cursor_alpha)
            };
            surface = surface
                .child(cx, text(self.content).size(13).color(text_color))
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(6)
                        .items_center()
                        .child(cx, div().w(7).h(12).rounded(2).bg(caret_color))
                        .child(
                            cx,
                            text("Streaming...")
                                .size(10)
                                .color(text_color.scale_alpha(150)),
                        ),
                );
        } else {
            surface = surface.child(cx, text(self.content).size(13).color(text_color));
        }

        let mut col = div().id(self.id).flex_col().gap(4);
        if self.align == BubbleAlign::End {
            col = col.items_end();
        }
        if self.animated {
            let enter_dx = if self.align == BubbleAlign::End { 14 } else { -14 };
            col = col
                .enter_from(0, enter_dx, 6, Duration::from_millis(240))
                .ease(Easing::Spring);
        }
        col = col.child(cx, surface);

        if !self.reactions.is_empty() {
            let mut react_row = div().flex_row().flex_wrap().gap(4).items_center();
            let on_react = self.on_reaction_click;
            for (idx, r) in self.reactions.iter().enumerate() {
                let r_bcol = if r.active {
                    self.reaction_active_border_color
                } else {
                    self.reaction_border_color
                };
                let r_bg = if r.active {
                    self.reaction_active_bg
                } else {
                    self.reaction_bg
                };
                let mut pill = div()
                    .id(crate::scoped_child_id(self.id, 0x943F_DA6F, idx as u32 + 1))
                    .h(22)
                    .px(7)
                    .rounded(11)
                    .bg(r_bg)
                    .hover_bg(if r.active {
                        self.reaction_active_bg
                    } else {
                        theme.list_hover
                    })
                    .items_center()
                    .justify_center()
                    .focusable();
                if self.animated {
                    pill = pill.animate_color(Duration::from_millis(120));
                }
                if r_bcol.a() > 0 {
                    pill = pill.border(1, r_bcol);
                }
                if !self.focus_ring {
                    pill = pill.no_focus_border();
                }
                pill = pill.child(
                    cx,
                    text(r.label).size(11).color(if r.active {
                        reaction_active_text_color
                    } else {
                        reaction_text_color
                    }),
                );
                if let Some(cb) = on_react {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    pill = pill.on_click_raw(click_cb);
                }
                react_row = react_row.child(cx, pill);
            }
            col = col.child(cx, react_row);
        }

        col = self.style_ext.apply_to_div(col, self.style);
        col.into_element(cx)
    }
}
