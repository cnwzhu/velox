//! 主题化多步问卷调查组件 `Questionnaire`。

use crate::styled::{fill_color, fill_rounded};
use velox_engine::prelude::*;

pub use velox_base::components::questionnaire::{QuestionItem, QuestionKind, QuestionnaireState};

/// 建立在 [`velox_base::Questionnaire`] 之上的主题化多步问卷调查组件。
pub struct Questionnaire<'a, const MAX_QUESTIONS: usize = 8> {
    pub inner: velox_base::Questionnaire<'a, MAX_QUESTIONS>,
}

impl<'a, const MAX_QUESTIONS: usize> Questionnaire<'a, MAX_QUESTIONS> {
    #[inline(always)]
    pub fn new(
        id: u32,
        questions: &'a [QuestionItem<'a>],
        state: QuestionnaireState<MAX_QUESTIONS>,
    ) -> Self {
        let mut inner = velox_base::Questionnaire::new(id, questions, state);
        inner.focus_ring = true;
        Self { inner }
    }

    #[inline(always)]
    pub fn from_base(mut inner: velox_base::Questionnaire<'a, MAX_QUESTIONS>) -> Self {
        inner.focus_ring = true;
        Self { inner }
    }

    #[inline(always)]
    pub fn into_base(self) -> velox_base::Questionnaire<'a, MAX_QUESTIONS> {
        self.inner
    }

    #[inline(always)]
    pub fn on_select_option(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.inner = self.inner.on_select_option(handler);
        self
    }

    #[inline(always)]
    pub fn on_prev(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.inner = self.inner.on_prev(handler);
        self
    }

    #[inline(always)]
    pub fn on_next(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.inner = self.inner.on_next(handler);
        self
    }
}

impl<'a, const MAX_QUESTIONS: usize> core::ops::Deref for Questionnaire<'a, MAX_QUESTIONS> {
    type Target = velox_base::Questionnaire<'a, MAX_QUESTIONS>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, const MAX_QUESTIONS: usize> core::ops::DerefMut for Questionnaire<'a, MAX_QUESTIONS> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a, const MAX_QUESTIONS: usize> From<velox_base::Questionnaire<'a, MAX_QUESTIONS>>
    for Questionnaire<'a, MAX_QUESTIONS>
{
    #[inline(always)]
    fn from(inner: velox_base::Questionnaire<'a, MAX_QUESTIONS>) -> Self {
        Self::from_base(inner)
    }
}

impl<'a, const MAX_QUESTIONS: usize> Styled for Questionnaire<'a, MAX_QUESTIONS> {
    #[inline(always)]
    fn style_mut(&mut self) -> &mut Style {
        self.inner.style_mut()
    }
}

impl<'a, const MAX_QUESTIONS: usize> StyledExt for Questionnaire<'a, MAX_QUESTIONS> {
    #[inline(always)]
    fn style_ext_mut(&mut self) -> &mut StyleExt {
        self.inner.style_ext_mut()
    }
}

impl<'a, const MAX_QUESTIONS: usize> Widget<'a> for Questionnaire<'a, MAX_QUESTIONS> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        fill_color(&mut self.inner.bg, theme.card);
        fill_color(&mut self.inner.border_color, theme.border);
        fill_color(&mut self.inner.prompt_color, theme.foreground);
        fill_color(&mut self.inner.completed_color, theme.success);
        fill_color(&mut self.inner.option_bg, theme.secondary);
        fill_color(&mut self.inner.option_selected_bg, theme.secondary);
        fill_color(&mut self.inner.option_border_color, theme.border);
        fill_color(&mut self.inner.option_selected_border_color, theme.primary);
        fill_color(&mut self.inner.option_text_color, theme.secondary_foreground);
        fill_color(&mut self.inner.option_selected_text_color, theme.foreground);
        fill_color(&mut self.inner.marker_color, theme.muted_foreground);
        fill_color(&mut self.inner.marker_selected_color, theme.primary);
        fill_color(&mut self.inner.prev_bg, theme.secondary);
        fill_color(&mut self.inner.prev_border_color, theme.border);
        fill_color(&mut self.inner.prev_text_color, theme.secondary_foreground);
        fill_color(&mut self.inner.next_bg, theme.primary);
        fill_color(&mut self.inner.next_text_color, theme.primary_foreground);
        fill_rounded(&mut self.inner.rounded, 8);
        self.inner.render_once(cx)
    }
}
