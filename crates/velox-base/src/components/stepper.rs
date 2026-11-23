//! 分步向导与进度指示器基础组件 `Stepper`（默认无样式）。

use velox_engine::prelude::*;

/// 分步流程状态（`StepStatus`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StepStatus {
    #[default]
    Pending,
    Active,
    Completed,
    Error,
}

/// 单个步骤条目描述（`StepItem`）。
#[derive(Clone, Copy, Debug)]
pub struct StepItem<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub status: StepStatus,
}

impl<'a> StepItem<'a> {
    /// 创建一个步骤条目。
    #[inline(always)]
    pub const fn new(title: &'a str, description: &'a str, status: StepStatus) -> Self {
        Self {
            title,
            description,
            status,
        }
    }
}

/// 面向 [`Stepper::item`] 插槽回调的单步向导状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepperItemState {
    pub index: usize,
    pub status: StepStatus,
    pub is_last: bool,
}

impl StepperItemState {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn status(&self) -> StepStatus { self.status }
    #[inline(always)] pub const fn is_last(&self) -> bool { self.is_last }
}

/// 分步向导与进度指示器基础组件（`Stepper`，默认无样式）。
pub struct Stepper<'a> {
    pub id: u32,
    pub steps: &'a [StepItem<'a>],
    pub vertical: bool,
    pub badge_rounded: u8,
    pub active_bg: Color,
    pub active_border: Color,
    pub active_fg: Color,
    pub completed_bg: Color,
    pub completed_border: Color,
    pub completed_fg: Color,
    pub error_bg: Color,
    pub error_border: Color,
    pub error_fg: Color,
    pub pending_bg: Color,
    pub pending_border: Color,
    pub pending_fg: Color,
    pub title_color: Color,
    pub desc_color: Color,
    pub line_color: Color,
    pub completed_line_color: Color,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_step_click: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub item_renderer:
        Option<&'a dyn Fn(Div<'a>, StepperItemState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Stepper<'a> {
    /// 创建一个默认无样式的水平或垂直分步指示器。
    #[inline(always)]
    pub const fn new(id: u32, steps: &'a [StepItem<'a>]) -> Self {
        Self {
            id,
            steps,
            vertical: false,
            badge_rounded: 0,
            active_bg: Color::TRANSPARENT,
            active_border: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            completed_bg: Color::TRANSPARENT,
            completed_border: Color::TRANSPARENT,
            completed_fg: Color::TRANSPARENT,
            error_bg: Color::TRANSPARENT,
            error_border: Color::TRANSPARENT,
            error_fg: Color::TRANSPARENT,
            pending_bg: Color::TRANSPARENT,
            pending_border: Color::TRANSPARENT,
            pending_fg: Color::TRANSPARENT,
            title_color: Color::TRANSPARENT,
            desc_color: Color::TRANSPARENT,
            line_color: Color::TRANSPARENT,
            completed_line_color: Color::TRANSPARENT,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_step_click: None,
            item_renderer: None,
        }
    }

    /// 切换为垂直步骤条布局。
    #[inline(always)]
    pub const fn vertical(mut self, vertical: bool) -> Self {
        self.vertical = vertical;
        self
    }

    /// 绑定点击某个步骤节点的回调。
    #[inline(always)]
    pub fn on_step_click(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_step_click = Some(handler);
        self
    }

    /// `gpui-base` 风格步骤项插槽回调：自定义每一个步骤节点外观。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, StepperItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: StepperItemState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Stepper<'a>);

impl<'a> Widget<'a> for Stepper<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let muted_fg = fg.scale_alpha(150);
        let faint_line = fg.scale_alpha(64);

        let active_border = if self.active_border.a() > 0 {
            self.active_border
        } else {
            fg
        };
        let active_fg = if self.active_fg.a() > 0 {
            self.active_fg
        } else {
            fg
        };
        let completed_border = if self.completed_border.a() > 0 {
            self.completed_border
        } else {
            fg
        };
        let completed_fg = if self.completed_fg.a() > 0 {
            self.completed_fg
        } else {
            fg
        };
        let error_border = if self.error_border.a() > 0 {
            self.error_border
        } else {
            fg
        };
        let error_fg = if self.error_fg.a() > 0 {
            self.error_fg
        } else {
            fg
        };
        let pending_border = if self.pending_border.a() > 0 {
            self.pending_border
        } else {
            faint_line
        };
        let pending_fg = if self.pending_fg.a() > 0 {
            self.pending_fg
        } else {
            muted_fg
        };
        let title_color = if self.title_color.a() > 0 {
            self.title_color
        } else {
            fg
        };
        let desc_color = if self.desc_color.a() > 0 {
            self.desc_color
        } else {
            muted_fg
        };
        let line_color = if self.line_color.a() > 0 {
            self.line_color
        } else {
            faint_line
        };
        let completed_line_color = if self.completed_line_color.a() > 0 {
            self.completed_line_color
        } else {
            fg
        };

        let mut root = if self.vertical {
            div().id(self.id).flex_col().gap(8)
        } else {
            div().id(self.id).flex_row().gap(12).items_center()
        };

        let on_click = self.on_step_click;
        const NUM_LABELS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8", "9"];

        for (idx, step) in self.steps.iter().enumerate() {
            let (badge_bg, badge_border, badge_fg, badge_txt) = match step.status {
                StepStatus::Completed => (self.completed_bg, completed_border, completed_fg, ""),
                StepStatus::Active => (
                    self.active_bg,
                    active_border,
                    active_fg,
                    NUM_LABELS.get(idx).copied().unwrap_or("•"),
                ),
                StepStatus::Error => (self.error_bg, error_border, error_fg, "!"),
                StepStatus::Pending => (
                    self.pending_bg,
                    pending_border,
                    pending_fg,
                    NUM_LABELS.get(idx).copied().unwrap_or("•"),
                ),
            };

            let mut badge = div()
                .w(24)
                .h(24)
                .rounded(self.badge_rounded)
                .bg(badge_bg)
                .border(1, badge_border)
                .items_center()
                .justify_center();
            badge = if step.status == StepStatus::Completed {
                badge.child(
                    cx,
                    svg_icon(13, 13, "M20 6 9 17l-5-5")
                        .tint(badge_fg)
                        .stroke_width(26),
                )
            } else {
                badge.child(cx, text(badge_txt).size(11).color(badge_fg))
            };

            let mut text_col = div()
                .flex_col()
                .gap(2)
                .child(cx, text(step.title).size(12).color(title_color));
            if !step.description.is_empty() {
                text_col = text_col.child(cx, text(step.description).size(11).color(desc_color));
            }

            let is_last = idx + 1 >= self.steps.len();
            if let Some(renderer) = self.item_renderer {
                let mut item_node = div()
                    .id(crate::scoped_child_id(self.id, 0xE29A_C5DE, idx as u32 + 1))
                    .flex_row()
                    .gap(8)
                    .items_center()
                    .child(cx, text(step.title));
                if let Some(cb) = on_click {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    item_node = item_node.focusable().no_focus_border().on_click_raw(click_cb);
                }
                let state = StepperItemState {
                    index: idx,
                    status: step.status,
                    is_last,
                };
                root = root.child(cx, renderer(item_node, state, cx));
                continue;
            }

            let mut item_node = div()
                .id(crate::scoped_child_id(self.id, 0xE29A_C5DE, idx as u32 + 1))
                .flex_row()
                .gap(8)
                .items_center()
                .child(cx, badge)
                .child(cx, text_col);

            if let Some(cb) = on_click {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(idx, scope);
                });
                item_node = item_node.focusable().no_focus_border().on_click_raw(click_cb);
            }

            root = root.child(cx, item_node);

            if !self.vertical && !is_last {
                root = root.child(
                    cx,
                    div()
                        .w(20)
                        .h(2)
                        .bg(if step.status == StepStatus::Completed {
                            completed_line_color
                        } else {
                            line_color
                        }),
                );
            }
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
