//! 数据表格基础组件 `Table`、`DataTable` 与列定义 `TableColumn`（默认无样式）。

use velox_engine::prelude::*;

/// 列排序方向枚举（`SortDirection`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

/// 表格列定义（`TableColumn`）。
#[derive(Clone, Copy, Debug)]
pub struct TableColumn<'a> {
    pub title: &'a str,
    pub width: i16,
}

impl<'a> TableColumn<'a> {
    /// 创建一个固定宽度的数据列头。
    #[inline(always)]
    pub const fn new(title: &'a str, width: i16) -> Self {
        Self { title, width }
    }
}

/// 面向 [`Table::header_cell`] / [`DataTable::header_cell`] 插槽回调的表头单元状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableHeaderState {
    pub column_index: usize,
    pub width: i16,
    pub sort_direction: Option<SortDirection>,
}

impl TableHeaderState {
    #[inline(always)] pub const fn column_index(&self) -> usize { self.column_index }
    #[inline(always)] pub const fn width(&self) -> i16 { self.width }
    #[inline(always)] pub const fn sort_direction(&self) -> Option<SortDirection> { self.sort_direction }
}

/// 面向 [`Table::cell`] / [`DataTable::cell`] 插槽回调的数据单元状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCellState {
    pub row_index: usize,
    pub column_index: usize,
    pub width: i16,
    pub is_selected: bool,
    pub is_striped: bool,
}

impl TableCellState {
    #[inline(always)] pub const fn row_index(&self) -> usize { self.row_index }
    #[inline(always)] pub const fn column_index(&self) -> usize { self.column_index }
    #[inline(always)] pub const fn width(&self) -> i16 { self.width }
    #[inline(always)] pub const fn is_selected(&self) -> bool { self.is_selected }
    #[inline(always)] pub const fn is_striped(&self) -> bool { self.is_striped }
}

/// 结构化数据表格基础组件（`Table`，默认无样式）。
pub struct Table<'a> {
    pub id: u32,
    pub columns: &'a [TableColumn<'a>],
    pub rows: &'a [&'a [&'a str]],
    pub selected_row: Option<usize>,
    pub striped: bool,
    pub row_height: i16,
    pub bg: Color,
    pub stripe_bg: Color,
    pub header_bg: Color,
    pub header_fg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_row_click: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub header_cell_renderer:
        Option<&'a dyn Fn(Div<'a>, TableHeaderState, &'a FrameScope) -> Element<'a>>,
    pub cell_renderer: Option<&'a dyn Fn(Div<'a>, TableCellState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> Table<'a> {
    /// 创建一个默认无样式的数据表格。
    #[inline(always)]
    pub const fn new(
        id: u32,
        columns: &'a [TableColumn<'a>],
        rows: &'a [&'a [&'a str]],
    ) -> Self {
        Self {
            id,
            columns,
            rows,
            selected_row: None,
            striped: true,
            row_height: 28,
            bg: Color::TRANSPARENT,
            stripe_bg: Color::TRANSPARENT,
            header_bg: Color::TRANSPARENT,
            header_fg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: Color::TRANSPARENT,
            hover_bg: None,
            active_bg: None,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_row_click: None,
            header_cell_renderer: None,
            cell_renderer: None,
        }
    }

    /// 设置当前选中的行索引。
    #[inline(always)]
    pub const fn selected_row(mut self, row: Option<usize>) -> Self {
        self.selected_row = row;
        self
    }

    /// 绑定点击某一行的回调。
    #[inline(always)]
    pub fn on_row_click(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_row_click = Some(handler);
        self
    }

    /// `gpui-base` 风格表头单元插槽回调。
    #[inline(always)]
    pub fn header_cell<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TableHeaderState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: TableHeaderState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.header_cell_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格数据单元格插槽回调。
    #[inline(always)]
    pub fn cell<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TableCellState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: TableCellState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.cell_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(Table<'a>);

impl<'a> Widget<'a> for Table<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let header_fg = if self.header_fg.a() > 0 {
            self.header_fg
        } else {
            fg
        };
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let selected_fg = if self.selected_fg.a() > 0 {
            self.selected_fg
        } else {
            fg
        };

        let total_w: i16 = self.columns.iter().map(|c| c.width).sum();

        let mut header_row = div()
            .flex_row()
            .w(total_w)
            .h(self.row_height + 2)
            .bg(self.header_bg)
            .items_center();

        for (c_idx, col) in self.columns.iter().enumerate() {
            if let Some(renderer) = self.header_cell_renderer {
                let hcell = div()
                    .w(col.width)
                    .items_center()
                    .child(cx, text(col.title).ellipsis());
                let state = TableHeaderState {
                    column_index: c_idx,
                    width: col.width,
                    sort_direction: None,
                };
                header_row = header_row.child(cx, renderer(hcell, state, cx));
            } else {
                header_row = header_row.child(
                    cx,
                    div()
                        .w(col.width)
                        .px(8)
                        .items_center()
                        .child(cx, text(col.title).size(12).color(header_fg).ellipsis()),
                );
            }
        }

        let mut table_box = div()
            .id(self.id)
            .flex_col()
            .w(total_w)
            .rounded(self.rounded)
            .overflow_hidden()
            .child(cx, header_row);
        if self.border_color.a() > 0 {
            table_box = table_box
                .border(1, self.border_color)
                .child(cx, div().w(total_w).h(1).bg(self.border_color));
        }

        let on_row_click = self.on_row_click;
        for (r_idx, &row_cells) in self.rows.iter().enumerate() {
            let is_sel = self.selected_row == Some(r_idx);
            let is_striped = self.striped && r_idx % 2 == 1;
            let row_bg = if is_sel {
                self.selected_bg
            } else if is_striped {
                self.stripe_bg
            } else {
                self.bg
            };
            let row_fg = if is_sel { selected_fg } else { text_color };

            let mut tr = div()
                .id(crate::scoped_child_id(self.id, 0x4A9C_9BDF, r_idx as u32 + 1))
                .flex_row()
                .w(total_w)
                .h(self.row_height)
                .bg(row_bg)
                .items_center();
            if let Some(h) = self.hover_bg {
                tr = tr.hover_bg(if is_sel { self.selected_bg } else { h });
            }
            if let Some(a) = self.active_bg {
                tr = tr.active_bg(a);
            }

            for (c_idx, col) in self.columns.iter().enumerate() {
                let cell_str = row_cells.get(c_idx).copied().unwrap_or("");
                if let Some(renderer) = self.cell_renderer {
                    let cell = div()
                        .w(col.width)
                        .items_center()
                        .child(cx, text(cell_str).ellipsis());
                    let state = TableCellState {
                        row_index: r_idx,
                        column_index: c_idx,
                        width: col.width,
                        is_selected: is_sel,
                        is_striped,
                    };
                    tr = tr.child(cx, renderer(cell, state, cx));
                } else {
                    tr = tr.child(
                        cx,
                        div()
                            .w(col.width)
                            .px(8)
                            .items_center()
                            .child(cx, text(cell_str).size(12).color(row_fg).ellipsis()),
                    );
                }
            }

            if let Some(cb) = on_row_click {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(r_idx, scope);
                });
                tr = tr.on_click_raw(click_cb);
            }

            table_box = table_box.child(cx, tr);
        }

        table_box = self.style_ext.apply_to_div(table_box, self.style);
        table_box.into_element(cx)
    }
}

/// 高性能增强数据表格基础组件（`DataTable`，默认无样式，支持列排序指示器、行选中与空状态提示）。
pub struct DataTable<'a> {
    pub id: u32,
    pub columns: &'a [TableColumn<'a>],
    pub rows: &'a [&'a [&'a str]],
    pub selected_row: Option<usize>,
    pub sort_column: Option<(usize, SortDirection)>,
    pub striped: bool,
    pub row_height: i16,
    pub empty_text: &'a str,
    pub bg: Color,
    pub stripe_bg: Color,
    pub header_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub accent_color: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub header_hover_bg: Option<Color>,
    pub header_active_bg: Option<Color>,
    pub row_hover_bg: Option<Color>,
    pub row_active_bg: Option<Color>,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_row_click: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub on_sort_column: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    pub header_cell_renderer:
        Option<&'a dyn Fn(Div<'a>, TableHeaderState, &'a FrameScope) -> Element<'a>>,
    pub cell_renderer: Option<&'a dyn Fn(Div<'a>, TableCellState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> DataTable<'a> {
    /// 创建一个默认无样式的高性能数据表格组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        columns: &'a [TableColumn<'a>],
        rows: &'a [&'a [&'a str]],
    ) -> Self {
        Self {
            id,
            columns,
            rows,
            selected_row: None,
            sort_column: None,
            striped: true,
            row_height: 28,
            empty_text: "No data available",
            bg: Color::TRANSPARENT,
            stripe_bg: Color::TRANSPARENT,
            header_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            accent_color: Color::TRANSPARENT,
            selected_bg: Color::TRANSPARENT,
            selected_fg: Color::TRANSPARENT,
            header_hover_bg: None,
            header_active_bg: None,
            row_hover_bg: None,
            row_active_bg: None,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_row_click: None,
            on_sort_column: None,
            header_cell_renderer: None,
            cell_renderer: None,
        }
    }

    /// 设置当前选中的行索引。
    #[inline(always)]
    pub const fn selected_row(mut self, row: Option<usize>) -> Self {
        self.selected_row = row;
        self
    }

    /// 设置当前排序列及排序方向。
    #[inline(always)]
    pub const fn sort_by(mut self, col_idx: usize, dir: SortDirection) -> Self {
        self.sort_column = Some((col_idx, dir));
        self
    }

    /// 绑定点击某一行的回调。
    #[inline(always)]
    pub fn on_row_click(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_row_click = Some(handler);
        self
    }

    /// 绑定点击表头列进行排序的回调。
    #[inline(always)]
    pub fn on_sort_column(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_sort_column = Some(handler);
        self
    }

    /// `gpui-base` 风格表头单元插槽回调。
    #[inline(always)]
    pub fn header_cell<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TableHeaderState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: TableHeaderState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.header_cell_renderer = Some(cb);
        self
    }

    /// `gpui-base` 风格数据单元格插槽回调。
    #[inline(always)]
    pub fn cell<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, TableCellState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |el: Div<'a>, state: TableCellState, scope: &'a FrameScope| {
                f(el, state, scope).into_element(scope)
            },
        );
        self.cell_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(DataTable<'a>);

impl<'a> Widget<'a> for DataTable<'a> {
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
            fg
        };
        let accent_color = if self.accent_color.a() > 0 {
            self.accent_color
        } else {
            fg
        };
        let selected_fg = if self.selected_fg.a() > 0 {
            self.selected_fg
        } else {
            fg
        };

        let total_w: i16 = self.columns.iter().map(|c| c.width).sum::<i16>().max(120);

        let mut header_row = div()
            .flex_row()
            .w(total_w)
            .h(self.row_height + 4)
            .bg(self.header_bg)
            .items_center();

        let on_sort = self.on_sort_column;
        for (c_idx, col) in self.columns.iter().enumerate() {
            let sort_dir = match self.sort_column {
                Some((sc, dir)) if sc == c_idx => Some(dir),
                _ => None,
            };
            let sort_d = match sort_dir {
                Some(SortDirection::Asc) => Some("M18 15l-6-6-6 6"),
                Some(SortDirection::Desc) => Some("M6 9l6 6 6-6"),
                None => None,
            };
            if let Some(renderer) = self.header_cell_renderer {
                let mut hcell = div()
                    .id(crate::scoped_child_id(self.id, 0x4A9C_9BDF, 1000 + c_idx as u32))
                    .flex_row()
                    .w(col.width)
                    .h(self.row_height)
                    .items_center()
                    .child(cx, text(col.title).ellipsis());
                if let Some(cb) = on_sort {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(c_idx, scope);
                    });
                    hcell = hcell.on_click_raw(click_cb);
                }
                let state = TableHeaderState {
                    column_index: c_idx,
                    width: col.width,
                    sort_direction: sort_dir,
                };
                header_row = header_row.child(cx, renderer(hcell, state, cx));
                continue;
            }
            let mut hcell = div()
                .id(crate::scoped_child_id(self.id, 0x4A9C_9BDF, 1000 + c_idx as u32))
                .flex_row()
                .w(col.width)
                .h(self.row_height)
                .px(8)
                .gap(4)
                .items_center()
                .child(cx, text(col.title).size(12).color(muted_color).ellipsis());
            if let Some(h) = self.header_hover_bg {
                hcell = hcell.hover_bg(h);
            }
            if let Some(a) = self.header_active_bg {
                hcell = hcell.active_bg(a);
            }
            if let Some(d) = sort_d {
                hcell = hcell.child(
                    cx,
                    svg_icon(12, 12, d).tint(accent_color).stroke_width(22),
                );
            }
            if let Some(cb) = on_sort {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(c_idx, scope);
                });
                hcell = hcell.on_click_raw(click_cb);
            }
            header_row = header_row.child(cx, hcell);
        }

        let mut table_box = div()
            .id(self.id)
            .flex_col()
            .w(total_w)
            .rounded(self.rounded)
            .overflow_hidden()
            .child(cx, header_row);
        if self.border_color.a() > 0 {
            table_box = table_box
                .border(1, self.border_color)
                .child(cx, div().w(total_w).h(1).bg(self.border_color));
        }

        if self.rows.is_empty() {
            table_box = table_box.child(
                cx,
                div()
                    .w(total_w)
                    .h(self.row_height * 2)
                    .bg(self.bg)
                    .items_center()
                    .justify_center()
                    .child(cx, text(self.empty_text).size(12).color(muted_color)),
            );
            table_box = self.style_ext.apply_to_div(table_box, self.style);
            return table_box.into_element(cx);
        }

        let on_row_click = self.on_row_click;
        for (r_idx, &row_cells) in self.rows.iter().enumerate() {
            let is_sel = self.selected_row == Some(r_idx);
            let is_striped = self.striped && r_idx % 2 == 1;
            let row_bg = if is_sel {
                self.selected_bg
            } else if is_striped {
                self.stripe_bg
            } else {
                self.bg
            };
            let row_fg = if is_sel { selected_fg } else { text_color };

            let mut tr = div()
                .id(crate::scoped_child_id(self.id, 0x4A9C_9BDF, r_idx as u32 + 1))
                .flex_row()
                .w(total_w)
                .h(self.row_height)
                .bg(row_bg)
                .items_center();
            if let Some(h) = self.row_hover_bg {
                tr = tr.hover_bg(if is_sel { self.selected_bg } else { h });
            }
            if let Some(a) = self.row_active_bg {
                tr = tr.active_bg(a);
            }

            for (c_idx, col) in self.columns.iter().enumerate() {
                let cell_str = row_cells.get(c_idx).copied().unwrap_or("");
                if let Some(renderer) = self.cell_renderer {
                    let cell = div()
                        .w(col.width)
                        .items_center()
                        .child(cx, text(cell_str).ellipsis());
                    let state = TableCellState {
                        row_index: r_idx,
                        column_index: c_idx,
                        width: col.width,
                        is_selected: is_sel,
                        is_striped,
                    };
                    tr = tr.child(cx, renderer(cell, state, cx));
                } else {
                    tr = tr.child(
                        cx,
                        div()
                            .w(col.width)
                            .px(8)
                            .items_center()
                            .child(cx, text(cell_str).size(12).color(row_fg).ellipsis()),
                    );
                }
            }

            if let Some(cb) = on_row_click {
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(r_idx, scope);
                });
                tr = tr.on_click_raw(click_cb);
            }

            table_box = table_box.child(cx, tr);
        }

        table_box = self.style_ext.apply_to_div(table_box, self.style);
        table_box.into_element(cx)
    }
}
