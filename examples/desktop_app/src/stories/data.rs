//! `Table & List` 故事页：`DataTable`（表头 SVG 排序箭头 + 行选中 + 悬停高亮）、`List` 与 `Tree`。

use super::{state_pill, story_header, story_section};
use crate::style;
use crate::GalleryApp;
use velox_kit::base::{DataTable, List, ListItem, SortDirection, TableColumn, Tree, TreeNode};
use velox_kit::prelude::*;

const PKG_COLS: &[TableColumn<'static>] = &[
    TableColumn::new("Crate", 150),
    TableColumn::new("Layer", 130),
    TableColumn::new("Mode", 110),
    TableColumn::new("Role", 330),
];

const PKG_ROWS_ASC: &[&[&str]] = &[
    &["desktop_app", "Gallery App", "std + gpu", "Interactive Velox Base Gallery & Benchmarks"],
    &["velox-base", "Base Widgets", "headless", "75+ Headless Composable Primitives & CodeEditor"],
    &["velox-component", "Themed UI", "themed", "Shadcn Zinc + Blue Themed Component Wrappers"],
    &["velox-engine", "GUI Kernel", "std + sdf", "ScratchArena, 1D Solver, Damage & SDF Renderers"],
    &["velox-kit", "Facade", "cross-plat", "Unified Cross-Platform Application Entry"],
];

const PKG_ROWS_DESC: &[&[&str]] = &[
    &["velox-kit", "Facade", "cross-plat", "Unified Cross-Platform Application Entry"],
    &["velox-engine", "GUI Kernel", "std + sdf", "ScratchArena, 1D Solver, Damage & SDF Renderers"],
    &["velox-component", "Themed UI", "themed", "Shadcn Zinc + Blue Themed Component Wrappers"],
    &["velox-base", "Base Widgets", "headless", "75+ Headless Composable Primitives & CodeEditor"],
    &["desktop_app", "Gallery App", "std + gpu", "Interactive Velox Base Gallery & Benchmarks"],
];

const LIST_ITEMS: &[ListItem<'static>] = &[
    ListItem::new("First-Frame Zero White Flash", "Software GDI instant paint -> async GPU upgrade", "61 ms"),
    ListItem::new("Static Idle Sleep", "Zero polling timers; ControlFlow::Wait", "0.0% CPU"),
    ListItem::new("HiDPI SVG Vector Atlas", "Physical DPI supersampling + hash cache", "1.5x / 2x"),
];

impl GalleryApp {
    pub(crate) fn render_story_data<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let on_row_click = cx.listener(|this, row: usize, _acx| {
            this.selected_pkg_row = row.min(PKG_ROWS_ASC.len().saturating_sub(1));
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_sort = cx.listener(|this, col: usize, _acx| {
            if this.table_sort_col == col {
                this.table_sort_dir = match this.table_sort_dir {
                    SortDirection::Asc => SortDirection::Desc,
                    SortDirection::Desc => SortDirection::Asc,
                };
            } else {
                this.table_sort_col = col;
                this.table_sort_dir = SortDirection::Asc;
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_list_sel = cx.listener(|this, idx: usize, _acx| {
            this.list_selected = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_tree_sel = cx.listener(|this, nid: u32, _acx| {
            this.tree_state.select(nid);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_tree_toggle = cx.listener(|this, nid: u32, _acx| {
            this.tree_state.toggle_expand(nid);
            this.event_count = this.event_count.saturating_add(1);
        });

        let rows = match self.table_sort_dir {
            SortDirection::Asc => PKG_ROWS_ASC,
            SortDirection::Desc => PKG_ROWS_DESC,
        };
        let sel_row = self.selected_pkg_row.min(rows.len() - 1);

        let table_section = story_section(
            cx,
            theme,
            "DataTable (Sortable Headers with SVG Chevrons & Row Hover)",
            "Click any column header to toggle sort direction, or click rows to select",
        )
        .child(
            cx,
            style::data_table(
                DataTable::new(410, PKG_COLS, rows)
                    .selected_row(Some(sel_row))
                    .sort_by(self.table_sort_col, self.table_sort_dir)
                    .on_row_click(on_row_click)
                    .on_sort_column(on_sort),
                theme,
            ),
        )
        .child(
            cx,
            div()
                .flex_row()
                .gap(10)
                .child(cx, state_pill(cx, theme, fmt!(cx, "Selected: {}", rows[sel_row][0])))
                .child(
                    cx,
                    state_pill(
                        cx,
                        theme,
                        fmt!(
                            cx,
                            "Sort: Col {} ({:?})",
                            self.table_sort_col,
                            self.table_sort_dir
                        ),
                    ),
                ),
        );

        let crates_expanded = self.tree_state.is_expanded(1);
        let base_expanded = self.tree_state.is_expanded(2);
        let mut tree_nodes = SmallVec::<TreeNode<'static>, 8>::new();
        let _ = tree_nodes.push(TreeNode::new(1, "crates/", 0, true, crates_expanded));
        if crates_expanded {
            let _ = tree_nodes.push(TreeNode::new(2, "velox-base/", 1, true, base_expanded));
            if base_expanded {
                let _ = tree_nodes.push(TreeNode::new(3, "button.rs", 2, false, false));
                let _ = tree_nodes.push(TreeNode::new(4, "switch.rs", 2, false, false));
                let _ = tree_nodes.push(TreeNode::new(5, "checkbox.rs", 2, false, false));
            }
            let _ = tree_nodes.push(TreeNode::new(6, "velox-engine/", 1, false, false));
        }
        let tree_slice = cx.arena().alloc_iter(tree_nodes.as_slice().iter().copied());

        let bottom_row = div()
            .flex_row()
            .w_full()
            .gap(16)
            .items_start()
            .child(
                cx,
                div().flex_grow(1).child(
                    cx,
                    story_section(cx, theme, "List", "Two-line items with trailing metrics")
                        .child(
                            cx,
                            style::list(
                                List::new(1810, LIST_ITEMS)
                                    .width(380)
                                    .selected(Some(self.list_selected))
                                    .on_select(on_list_sel),
                                theme,
                            ),
                        ),
                ),
            )
            .child(
                cx,
                div().flex_grow(1).child(
                    cx,
                    story_section(cx, theme, "Tree", "Click vector chevrons to expand/collapse")
                        .child(
                            cx,
                            style::tree(
                                Tree::new(1820, tree_slice)
                                    .selected(self.tree_state.selected_id)
                                    .on_select(on_tree_sel)
                                    .on_toggle_expand(on_tree_toggle),
                                theme,
                            ),
                        ),
                ),
            );

        div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "Table, List & Tree",
                    "High-density headless data views styled with crisp 1px borders, hover rows & vector tree chevrons.",
                    "velox_base::{DataTable, List, Tree}",
                ),
            )
            .child(cx, table_section)
            .child(cx, bottom_row)
    }
}
