//! 层级树视图组件 `Tree` 与树状态管理 `TreeState`（支持按需回退 `Theme` 样式）。

use velox_core::prelude::*;

/// 扁平化层级树节点描述（`TreeNode`）。
#[derive(Clone, Copy, Debug)]
pub struct TreeNode<'a> {
    pub id: u32,
    pub label: &'a str,
    pub depth: u8,
    pub has_children: bool,
    pub expanded: bool,
}

impl<'a> TreeNode<'a> {
    /// 创建一个树节点条目。
    #[inline(always)]
    pub const fn new(
        id: u32,
        label: &'a str,
        depth: u8,
        has_children: bool,
        expanded: bool,
    ) -> Self {
        Self {
            id,
            label,
            depth,
            has_children,
            expanded,
        }
    }
}

/// 树组件展开与选中状态管理（`TreeState`）。
#[derive(Clone, Debug)]
pub struct TreeState<const CAP: usize = 32> {
    expanded_ids: SmallVec<u32, CAP>,
    pub selected_id: Option<u32>,
}

impl<const CAP: usize> Default for TreeState<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> TreeState<CAP> {
    /// 创建空的树状态。
    pub const fn new() -> Self {
        Self {
            expanded_ids: SmallVec::new(),
            selected_id: None,
        }
    }

    /// 判断节点 `id` 是否已展开。
    pub fn is_expanded(&self, id: u32) -> bool {
        self.expanded_ids.as_slice().contains(&id)
    }

    /// 切换节点 `id` 的展开/收起状态。
    pub fn toggle_expand(&mut self, id: u32) -> bool {
        if let Some(pos) = self.expanded_ids.as_slice().iter().position(|&x| x == id) {
            self.expanded_ids.remove(pos);
            false
        } else {
            let _ = self.expanded_ids.push(id);
            true
        }
    }

    /// 选中指定节点。
    #[inline]
    pub fn select(&mut self, id: u32) {
        self.selected_id = Some(id);
    }
}

/// 面向 [`Tree::item`] 插槽回调的层级树节点状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeItemState {
    pub id: u32,
    pub depth: u8,
    pub has_children: bool,
    pub is_expanded: bool,
    pub is_selected: bool,
}

impl TreeItemState {
    #[inline(always)] pub const fn id(&self) -> u32 { self.id }
    #[inline(always)] pub const fn depth(&self) -> u8 { self.depth }
    #[inline(always)] pub const fn has_children(&self) -> bool { self.has_children }
    #[inline(always)] pub const fn is_expanded(&self) -> bool { self.is_expanded }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.is_selected }
}

/// 层级树视图基础组件（`Tree`，默认无样式）。
pub struct Tree<'a> {
    pub id: u32,
    pub nodes: &'a [TreeNode<'a>],
    pub selected_id: Option<u32>,
    pub width: i16,
    pub row_height: i16,
    pub indent_px: i16,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_select: Option<&'a dyn Fn(u32, &mut ActionScope)>,
    pub on_toggle_expand: Option<&'a dyn Fn(u32, &mut ActionScope)>,
    pub item_renderer: Option<&'a dyn Fn(Div<'a>, TreeItemState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Tree<'a> {
    /// 创建一个默认无样式的树视图组件。
    #[inline(always)]
    pub const fn new(id: u32, nodes: &'a [TreeNode<'a>]) -> Self {
        Self {
            id,
            nodes,
            selected_id: None,
            width: 240,
            row_height: 26,
            indent_px: 14,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_select: None,
            on_toggle_expand: None,
            item_renderer: None,
        }
    }

    /// 设置当前选中的节点 ID。
    #[inline(always)]
    pub const fn selected(mut self, selected_id: Option<u32>) -> Self {
        self.selected_id = selected_id;
        self
    }

    /// 绑定点击选中节点回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(u32, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// 绑定点击展开/折叠箭头回调。
    #[inline(always)]
    pub fn on_toggle_expand(mut self, handler: &'a dyn Fn(u32, &mut ActionScope)) -> Self {
        self.on_toggle_expand = Some(handler);
        self
    }

    /// `gpui-base` 风格树节点插槽回调：自定义每一行树节点外观。
    #[inline(always)]
    pub fn item<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TreeItemState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: TreeItemState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.item_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Tree<'a>);

impl<'a> Widget<'a> for Tree<'a> {
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
        let mut root = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .p(4)
            .rounded(self.rounded)
            .bg(self.bg);
        if self.border_color.a() > 0 {
            root = root.border(1, self.border_color);
        }

        let item_rounded = self.rounded.saturating_sub(2);
        let on_sel = self.on_select;
        let on_tog = self.on_toggle_expand;

        for node in self.nodes {
            let is_sel = self.selected_id == Some(node.id);
            let row_bg = if is_sel {
                self.selected_bg
            } else {
                Color::TRANSPARENT
            };
            let row_fg = if is_sel { selected_fg } else { text_color };

            let indent_w = (node.depth as i16) * self.indent_px;

            let mut arrow_box = div()
                .id(node.id ^ 0x5452_4545)
                .w(16)
                .h(self.row_height)
                .items_center()
                .justify_center();

            if node.has_children {
                let arrow_d = if node.expanded {
                    "M6 9l6 6 6-6"
                } else {
                    "M9 18l6-6-6-6"
                };
                arrow_box = arrow_box.child(
                    cx,
                    svg_icon(12, 12, arrow_d)
                        .tint(muted_color)
                        .stroke_width(20),
                );
                if let Some(tcb) = on_tog {
                    let nid = node.id;
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        tcb(nid, scope);
                    });
                    arrow_box = arrow_box.on_click_raw(click_cb);
                }
            }

            if let Some(renderer) = self.item_renderer {
                let mut row = div()
                    .id(node.id)
                    .flex_row()
                    .items_center()
                    .focusable()
                    .no_focus_border()
                    .child(cx, div().w(indent_w).h(1))
                    .child(cx, arrow_box)
                    .child(cx, text(node.label).ellipsis());
                if let Some(scb) = on_sel {
                    let nid = node.id;
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        scb(nid, scope);
                    });
                    row = row.on_click_raw(click_cb);
                }
                let state = TreeItemState {
                    id: node.id,
                    depth: node.depth,
                    has_children: node.has_children,
                    is_expanded: node.expanded,
                    is_selected: is_sel,
                };
                root = root.child(cx, renderer(row, state, cx));
                continue;
            }

            let mut row = div()
                .id(node.id)
                .flex_row()
                .w(width - 8)
                .h(self.row_height)
                .px(4)
                .rounded(item_rounded)
                .bg(row_bg)
                .items_center()
                .focusable();
            if let Some(h) = self.hover_bg {
                row = row.hover_bg(if is_sel { self.selected_bg } else { h });
            }
            if let Some(a) = self.active_bg {
                row = row.active_bg(a);
            }
            if !self.focus_ring {
                row = row.no_focus_border();
            }
            row = row
                .child(cx, div().w(indent_w).h(1))
                .child(cx, arrow_box)
                .child(cx, text(node.label).size(12).color(row_fg).ellipsis());

            if let Some(scb) = on_sel {
                let nid = node.id;
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    scb(nid, scope);
                });
                row = row.on_click_raw(click_cb);
            }

            root = root.child(cx, row);
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
