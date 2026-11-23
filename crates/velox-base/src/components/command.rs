//! 搜索与快捷操作命令面板基础组件 `Command`（默认无样式）。

use crate::components::Combobox;
use velox_core::prelude::*;

/// 命令面板动作条目定义（`CommandItem`）。
#[derive(Clone, Copy, Debug)]
pub struct CommandItem<'a> {
    pub label: &'a str,
    pub shortcut: &'a str,
    pub group: &'a str,
}

impl<'a> CommandItem<'a> {
    /// 创建一个命令面板动作条目。
    #[inline(always)]
    pub const fn new(label: &'a str, shortcut: &'a str, group: &'a str) -> Self {
        Self {
            label,
            shortcut,
            group,
        }
    }
}

/// 命令面板分组定义（`CommandGroup`）。
#[derive(Clone, Copy, Debug)]
pub struct CommandGroup<'a> {
    pub title: &'a str,
    pub items: &'a [CommandItem<'a>],
}

/// 面向 [`Command::item`] 插槽回调的命令条目状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandItemState {
    pub index: usize,
    pub is_selected: bool,
}

impl CommandItemState {
    #[inline(always)] pub const fn index(&self) -> usize { self.index }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.is_selected }
}

/// 搜索与快捷操作命令面板基础组件（`Command`，默认无样式）。
pub struct Command<'a> {
    pub id: u32,
    pub query: &'a str,
    pub items: &'a [CommandItem<'a>],
    pub selected_index: usize,
    pub width: i16,
    pub bg: Color,
    pub search_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    pub item_renderer:
        Option<&'a dyn Fn(Div<'a>, CommandItemState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Command<'a> {
    /// 创建一个默认无样式的命令面板组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        query: &'a str,
        items: &'a [CommandItem<'a>],
        selected_index: usize,
    ) -> Self {
        Self {
            id,
            query,
            items,
            selected_index,
            width: 340,
            bg: Color::TRANSPARENT,
            search_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_select: None,
            on_key_input: None,
            item_renderer: None,
        }
    }

    /// 绑定选中并执行某条命令的回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// 绑定搜索输入框按键回调。
    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// `gpui-base` 风格命令项插槽回调：自定义每一条命令结果外观。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, CommandItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: CommandItemState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Command<'a>);

impl<'a> Widget<'a> for Command<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let muted_color = if self.muted_color.a() > 0 {
            self.muted_color
        } else {
            fg.scale_alpha(150)
        };
        let selected_fg = if self.selected_fg.a() > 0 {
            self.selected_fg
        } else {
            fg
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let q_txt = if self.query.is_empty() {
            "Type a command or search..."
        } else {
            self.query
        };
        let item_rounded = self.rounded.saturating_sub(2);

        let mut search_bar = div()
            .id(crate::scoped_child_id(self.id, 0x9359_4AB2, 1))
            .flex_row()
            .w(width - 16)
            .h(32)
            .px(10)
            .gap(8)
            .rounded(item_rounded)
            .bg(self.search_bg)
            .items_center()
            .focusable();
        if !self.focus_ring {
            search_bar = search_bar.no_focus_border();
        }
        search_bar = search_bar
            .child(cx, text("⌕").size(12).color(muted_color))
            .child(
                cx,
                text(q_txt).size(12).color(if self.query.is_empty() {
                    muted_color
                } else {
                    text_color
                }),
            );

        if let Some(kcb) = self.on_key_input {
            search_bar = search_bar.on_key_input(kcb);
        }

        let mut panel = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .p(8)
            .gap(4)
            .rounded(self.rounded)
            .bg(self.bg)
            .child(cx, search_bar);
        if self.border_color.a() > 0 {
            panel = panel.border(1, self.border_color);
        }

        let on_sel = self.on_select;
        for (idx, item) in self.items.iter().enumerate() {
            if !Combobox::matches_query(item.label, self.query)
                && !Combobox::matches_query(item.group, self.query)
            {
                continue;
            }
            let is_sel = idx == self.selected_index;
            if let Some(renderer) = self.item_renderer {
                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x9359_4AB2, 10 + idx as u32))
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .focusable()
                    .no_focus_border()
                    .child(cx, text(item.label));
                if !item.shortcut.is_empty() {
                    row = row.child(cx, text(item.shortcut));
                }
                if let Some(cb) = on_sel {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(idx, scope);
                    });
                    row = row.on_click_raw(click_cb);
                }
                let state = CommandItemState {
                    index: idx,
                    is_selected: is_sel,
                };
                panel = panel.child(cx, renderer(row, state, cx));
                continue;
            }

            let mut row = div()
                .id(crate::scoped_child_id(self.id, 0x9359_4AB2, 10 + idx as u32))
                .flex_row()
                .w(width - 16)
                .h(28)
                .px(8)
                .rounded(item_rounded)
                .bg(if is_sel {
                    self.selected_bg
                } else {
                    Color::TRANSPARENT
                })
                .items_center()
                .justify_between()
                .focusable();
            if !self.focus_ring {
                row = row.no_focus_border();
            }
            row = row.child(
                cx,
                text(item.label)
                    .size(12)
                    .color(if is_sel { selected_fg } else { text_color }),
            );

            if !item.shortcut.is_empty() {
                row = row.child(
                    cx,
                    text(item.shortcut).size(11).color(if is_sel {
                        selected_fg
                    } else {
                        muted_color
                    }),
                );
            }

            if let Some(cb) = on_sel {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(idx, scope);
                });
                row = row.on_click_raw(click_cb);
            }
            panel = panel.child(cx, row);
        }

        panel = self.style_ext.apply_to_div(panel, self.style);
        panel.into_element(cx)
    }
}
