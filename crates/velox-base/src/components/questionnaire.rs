//! 可组合多步问卷调查组件 `Questionnaire`。

use velox_engine::prelude::*;

/// 问卷题目作答类型（`QuestionKind`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum QuestionKind {
    #[default]
    SingleChoice,
    MultiChoice,
}

/// 单个问卷步骤题目定义（`QuestionItem`）。
#[derive(Clone, Copy, Debug)]
pub struct QuestionItem<'a> {
    pub prompt: &'a str,
    pub options: &'a [&'a str],
    pub kind: QuestionKind,
}

impl<'a> QuestionItem<'a> {
    /// 创建一道单选题或多选题。
    #[inline(always)]
    pub const fn new(prompt: &'a str, options: &'a [&'a str], kind: QuestionKind) -> Self {
        Self {
            prompt,
            options,
            kind,
        }
    }
}

/// 多步问卷调查状态机（`QuestionnaireState`，零堆分配记录每道题的选项位掩码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuestionnaireState<const MAX_QUESTIONS: usize = 8> {
    pub current_step: usize,
    pub answers_mask: [u32; MAX_QUESTIONS],
    pub submitted: bool,
}

impl<const MAX_QUESTIONS: usize> Default for QuestionnaireState<MAX_QUESTIONS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_QUESTIONS: usize> QuestionnaireState<MAX_QUESTIONS> {
    /// 创建一个初始处于第 `0` 步的多步问卷状态。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            current_step: 0,
            answers_mask: [0u32; MAX_QUESTIONS],
            submitted: false,
        }
    }

    /// 切换或选中当前题目的某个选项索引。
    pub fn select_option(&mut self, option_idx: usize, kind: QuestionKind) {
        if self.current_step >= MAX_QUESTIONS || option_idx >= 32 {
            return;
        }
        let bit = 1u32 << (option_idx as u32);
        match kind {
            QuestionKind::SingleChoice => {
                self.answers_mask[self.current_step] = bit;
            }
            QuestionKind::MultiChoice => {
                self.answers_mask[self.current_step] ^= bit;
            }
        }
    }

    /// 判断当前题目某个选项是否已选中。
    #[inline]
    pub const fn is_selected(&self, step: usize, option_idx: usize) -> bool {
        if step >= MAX_QUESTIONS || option_idx >= 32 {
            return false;
        }
        (self.answers_mask[step] & (1u32 << (option_idx as u32))) != 0
    }

    /// 前往下一题。
    #[inline]
    pub fn next_step(&mut self, total_questions: usize) {
        if self.current_step + 1 < total_questions && self.current_step + 1 < MAX_QUESTIONS {
            self.current_step += 1;
        } else {
            self.submitted = true;
        }
    }

    /// 返回上一题。
    #[inline]
    pub fn prev_step(&mut self) {
        if self.current_step > 0 {
            self.current_step -= 1;
        }
    }
}

/// 可组合多步问卷调查基础组件（`Questionnaire`，默认无样式）。
pub struct Questionnaire<'a, const MAX_QUESTIONS: usize = 8> {
    pub id: u32,
    pub questions: &'a [QuestionItem<'a>],
    pub state: QuestionnaireState<MAX_QUESTIONS>,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub prompt_color: Color,
    pub completed_color: Color,
    pub option_bg: Color,
    pub option_selected_bg: Color,
    pub option_border_color: Color,
    pub option_selected_border_color: Color,
    pub option_text_color: Color,
    pub option_selected_text_color: Color,
    pub marker_color: Color,
    pub marker_selected_color: Color,
    pub prev_bg: Color,
    pub prev_border_color: Color,
    pub prev_text_color: Color,
    pub next_bg: Color,
    pub next_text_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_select_option: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub on_prev: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_next: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a, const MAX_QUESTIONS: usize> Questionnaire<'a, MAX_QUESTIONS> {
    /// 创建一个默认无样式的多步问答组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        questions: &'a [QuestionItem<'a>],
        state: QuestionnaireState<MAX_QUESTIONS>,
    ) -> Self {
        Self {
            id,
            questions,
            state,
            width: 320,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            prompt_color: Color::TRANSPARENT,
            completed_color: Color::TRANSPARENT,
            option_bg: Color::TRANSPARENT,
            option_selected_bg: Color::TRANSPARENT,
            option_border_color: Color::TRANSPARENT,
            option_selected_border_color: Color::TRANSPARENT,
            option_text_color: Color::TRANSPARENT,
            option_selected_text_color: Color::TRANSPARENT,
            marker_color: Color::TRANSPARENT,
            marker_selected_color: Color::TRANSPARENT,
            prev_bg: Color::TRANSPARENT,
            prev_border_color: Color::TRANSPARENT,
            prev_text_color: Color::TRANSPARENT,
            next_bg: Color::TRANSPARENT,
            next_text_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_select_option: None,
            on_prev: None,
            on_next: None,
        }
    }

    /// 绑定点击某个答案选项的回调。
    #[inline(always)]
    pub fn on_select_option(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select_option = Some(handler);
        self
    }

    /// 绑定点击上一步按钮回调。
    #[inline(always)]
    pub fn on_prev(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_prev = Some(handler);
        self
    }

    /// 绑定点击下一步/提交按钮回调。
    #[inline(always)]
    pub fn on_next(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_next = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Questionnaire<'a, const MAX_QUESTIONS: usize>);

impl<'a, const MAX_QUESTIONS: usize> Widget<'a> for Questionnaire<'a, MAX_QUESTIONS> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let muted = fg.scale_alpha(150);
        let prompt_color = if self.prompt_color.a() > 0 { self.prompt_color } else { fg };
        let completed_color = if self.completed_color.a() > 0 { self.completed_color } else { fg };
        let option_text_color = if self.option_text_color.a() > 0 { self.option_text_color } else { fg };
        let option_selected_text_color = if self.option_selected_text_color.a() > 0 {
            self.option_selected_text_color
        } else {
            fg
        };
        let marker_color = if self.marker_color.a() > 0 { self.marker_color } else { muted };
        let marker_selected_color = if self.marker_selected_color.a() > 0 {
            self.marker_selected_color
        } else {
            fg
        };
        let prev_text_color = if self.prev_text_color.a() > 0 { self.prev_text_color } else { fg };
        let next_text_color = if self.next_text_color.a() > 0 { self.next_text_color } else { fg };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut card = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .p(14)
            .gap(10)
            .rounded(self.rounded)
            .bg(self.bg);
        if self.border_color.a() > 0 {
            card = card.border(1, self.border_color);
        }

        if self.questions.is_empty() || self.state.submitted {
            card = card.child(
                cx,
                text("Questionnaire Completed")
                    .size(14)
                    .color(completed_color),
            );
            return self.style_ext.apply_to_div(card, self.style).into_element(cx);
        }

        let step = self.state.current_step.min(self.questions.len() - 1);
        let q = self.questions[step];

        card = card.child(cx, text(q.prompt).size(14).color(prompt_color));

        let on_opt = self.on_select_option;
        let sub_r = self.rounded.saturating_sub(2);
        for (idx, &opt) in q.options.iter().enumerate() {
            let sel = self.state.is_selected(step, idx);
            let marker = match (q.kind, sel) {
                (QuestionKind::SingleChoice, true) => "(•)",
                (QuestionKind::SingleChoice, false) => "( )",
                (QuestionKind::MultiChoice, true) => "[✓]",
                (QuestionKind::MultiChoice, false) => "[ ]",
            };
            let opt_bcol = if sel {
                self.option_selected_border_color
            } else {
                self.option_border_color
            };
            let mut opt_row = div()
                .id(crate::scoped_child_id(self.id, 0x37E9_6DAA, idx as u32 + 1))
                .flex_row()
                .w(width - 28)
                .h(30)
                .px(10)
                .gap(8)
                .rounded(sub_r)
                .bg(if sel {
                    self.option_selected_bg
                } else {
                    self.option_bg
                })
                .items_center()
                .focusable();
            if opt_bcol.a() > 0 {
                opt_row = opt_row.border(1, opt_bcol);
            }
            if !self.focus_ring {
                opt_row = opt_row.no_focus_border();
            }
            opt_row = opt_row
                .child(
                    cx,
                    text(marker).size(12).color(if sel {
                        marker_selected_color
                    } else {
                        marker_color
                    }),
                )
                .child(
                    cx,
                    text(opt).size(12).color(if sel {
                        option_selected_text_color
                    } else {
                        option_text_color
                    }),
                );

            if let Some(cb) = on_opt {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(idx, scope);
                });
                opt_row = opt_row.on_click_raw(click_cb);
            }
            card = card.child(cx, opt_row);
        }

        let mut prev_btn = div()
            .id(crate::scoped_child_id(self.id, 0x37E9_6DAA, 100))
            .px(10)
            .py(5)
            .rounded(sub_r)
            .bg(self.prev_bg)
            .focusable()
            .child(cx, text("Previous").size(11).color(prev_text_color));
        if self.prev_border_color.a() > 0 {
            prev_btn = prev_btn.border(1, self.prev_border_color);
        }
        if !self.focus_ring {
            prev_btn = prev_btn.no_focus_border();
        }
        if let Some(cb) = self.on_prev {
            prev_btn = prev_btn.on_click_raw(cb);
        }

        let next_lbl = if step + 1 >= self.questions.len() {
            "Submit"
        } else {
            "Next"
        };
        let mut next_btn = div()
            .id(crate::scoped_child_id(self.id, 0x37E9_6DAA, 101))
            .px(12)
            .py(5)
            .rounded(sub_r)
            .bg(self.next_bg)
            .focusable()
            .child(cx, text(next_lbl).size(11).color(next_text_color));
        if !self.focus_ring {
            next_btn = next_btn.no_focus_border();
        }
        if let Some(cb) = self.on_next {
            next_btn = next_btn.on_click_raw(cb);
        }

        card = card.child(
            cx,
            div()
                .flex_row()
                .w(width - 28)
                .justify_between()
                .items_center()
                .child(cx, prev_btn)
                .child(cx, next_btn),
        );
        self.style_ext.apply_to_div(card, self.style).into_element(cx)
    }
}
