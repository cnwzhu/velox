//! `Table & List` 故事页：全面采用 `velox-component` 展示 `DataTable`、`List`、`Tree`、
//! 以及新增的 `Chart`（折线/柱状图）、`DescriptionList` 与 `Diff` 代码差异对比视图！

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{
    Button, CandlestickBar, Chart, ChartKind, DataTable, DescriptionItem, DescriptionList, Diff,
    List, ListItem, SortDirection, TableColumn, Tree, TreeNode,
};
use velox_kit::prelude::*;

const FPS_SERIES_A: &[i16] = &[60, 78, 95, 118, 120, 114, 120, 120, 108, 120];
const FPS_SERIES_B: &[i16] = &[88, 112, 104, 120, 96, 118, 120, 115, 120, 119];
const FPS_LABELS: &[&str] = &["T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8", "T9", "T10"];
const MEM_SERIES_A: &[i16] = &[32, 36, 40, 38, 42, 44, 41, 45, 43, 46];
const MEM_SERIES_B: &[i16] = &[48, 30, 52, 26, 55, 34, 49, 28, 54, 40];
const PIE_SERIES_A: &[i16] = &[42, 28, 18, 12];
const PIE_SERIES_B: &[i16] = &[24, 36, 26, 14];
const PIE_LABELS: &[&str] = &["GPU", "Layout", "Text", "FX"];
const CANDLE_LABELS: &[&str] = &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const CANDLES_A: &[CandlestickBar] = &[
    CandlestickBar::new(50, 66, 46, 62),
    CandlestickBar::new(62, 74, 58, 70),
    CandlestickBar::new(70, 72, 54, 58),
    CandlestickBar::new(58, 80, 56, 76),
    CandlestickBar::new(76, 90, 72, 86),
    CandlestickBar::new(86, 88, 68, 74),
];
const CANDLES_B: &[CandlestickBar] = &[
    CandlestickBar::new(62, 76, 58, 72),
    CandlestickBar::new(72, 75, 52, 56),
    CandlestickBar::new(56, 70, 50, 68),
    CandlestickBar::new(68, 88, 64, 84),
    CandlestickBar::new(84, 94, 78, 92),
    CandlestickBar::new(92, 95, 76, 82),
];
const ENGINE_SPECS: &[DescriptionItem<'static>] = &[
    DescriptionItem::new("Hot Structs", "Style 24B • DivNode 64B • DrawOpSnapshot 12B"),
    DescriptionItem::new("Renderers", "wgpu 2D SDF Single-Pass + Built-in Software SDF"),
    DescriptionItem::new("Animations", "std SpringAnimator + Easing::Spring / EaseOutBack"),
    DescriptionItem::new("Zero-Copy", "DX12 NT Handle • Vulkan dma_buf • Metal IOSurface"),
];
const SAMPLE_PATCH: &str = "\
@@ -12,4 +12,5 @@ fn render_button() {
-    Button::new(1, \"Save\")
+    Button::new(1, \"Save\").primary().animated(true)
     .on_click(handler)
 }";

const PKG_COLS: &[TableColumn<'static>] = &[
    TableColumn::new("Crate", 150),
    TableColumn::new("Layer", 130),
    TableColumn::new("Mode", 110),
    TableColumn::new("Role", 330),
];

const PKG_ROWS_ASC: &[&[&str]] = &[
    &["desktop_app", "Gallery App", "std + gpu", "Interactive Velox Component Gallery & Showcase"],
    &["velox-base", "Headless UI", "headless", "70+ Composable Primitives & CodeEditor"],
    &["velox-component", "Themed UI", "themed", "75+ Shadcn-Style Widgets & Spring Motion"],
    &["velox-engine", "GUI Kernel", "2D SDF GPU", "Layout, Effects, VectorCanvas & Dual Renderers"],
    &["velox-kit", "Facade", "cross-plat", "Unified Cross-Platform Application Entry"],
];

const PKG_ROWS_DESC: &[&[&str]] = &[
    &["velox-kit", "Facade", "cross-plat", "Unified Cross-Platform Application Entry"],
    &["velox-engine", "GUI Kernel", "2D SDF GPU", "Layout, Effects, VectorCanvas & Dual Renderers"],
    &["velox-component", "Themed UI", "themed", "75+ Shadcn-Style Widgets & Spring Motion"],
    &["velox-base", "Headless UI", "headless", "70+ Composable Primitives & CodeEditor"],
    &["desktop_app", "Gallery App", "std + gpu", "Interactive Velox Component Gallery & Showcase"],
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
            "DataTable (velox-component: Sortable Headers & Row Hover)",
            "Click any column header to toggle sort direction, or click rows to select",
        )
        .child(
            cx,
            DataTable::new(410, PKG_COLS, rows)
                .selected_row(Some(sel_row))
                .sort_by(self.table_sort_col, self.table_sort_dir)
                .on_row_click(on_row_click)
                .on_sort_column(on_sort),
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
                    story_section(cx, theme, "List (velox-component)", "Two-line items with trailing metrics")
                        .child(
                            cx,
                            List::new(1810, LIST_ITEMS)
                                .width(380)
                                .selected(Some(self.list_selected))
                                .on_select(on_list_sel),
                        ),
                ),
            )
            .child(
                cx,
                div().flex_grow(1).child(
                    cx,
                    story_section(cx, theme, "Tree (velox-component)", "Click vector chevrons to expand/collapse")
                        .child(
                            cx,
                            Tree::new(1820, tree_slice)
                                .selected(self.tree_state.selected_id)
                                .on_select(on_tree_sel)
                                .on_toggle_expand(on_tree_toggle),
                        ),
                ),
            );

        let on_toggle_dataset = cx.listener(|this, _: ClickEvent, _acx| {
            this.chart_dataset_b = !this.chart_dataset_b;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_replay_charts = cx.listener(|this, _: ClickEvent, _acx| {
            this.chart_anim_epoch = this.chart_anim_epoch.wrapping_add(1);
            this.event_count = this.event_count.saturating_add(1);
        });

        let fps_data = if self.chart_dataset_b {
            FPS_SERIES_B
        } else {
            FPS_SERIES_A
        };
        let mem_data = if self.chart_dataset_b {
            MEM_SERIES_B
        } else {
            MEM_SERIES_A
        };
        let pie_data = if self.chart_dataset_b {
            PIE_SERIES_B
        } else {
            PIE_SERIES_A
        };
        let candle_data = if self.chart_dataset_b {
            CANDLES_B
        } else {
            CANDLES_A
        };

        let charts_diff_section = story_section(
            cx,
            theme,
            "Animated Charts (gpui-kit Reveal & Morphing), DescriptionList & Diff",
            "Left-to-right Area reveal • Staggered rounded Bars • Sweep Donut • Morphing datasets",
        )
        .child(
            cx,
            div()
                .flex_row()
                .w_full()
                .gap(8)
                .items_center()
                .justify_between()
                .child(
                    cx,
                    div()
                        .flex_row()
                        .gap(8)
                        .items_center()
                        .child(
                            cx,
                            Button::new(
                                1828,
                                if self.chart_dataset_b {
                                    "Dataset B (Morph)"
                                } else {
                                    "Dataset A (Morph)"
                                },
                            )
                            .primary()
                            .small()
                            .on_click(on_toggle_dataset),
                        )
                        .child(
                            cx,
                            Button::new(1829, "Replay Entrance")
                                .outline()
                                .small()
                                .on_click(on_replay_charts),
                        ),
                )
                .child(
                    cx,
                    state_pill(
                        cx,
                        theme,
                        fmt!(
                            cx,
                            "Series: {} | Epoch #{}",
                            if self.chart_dataset_b { "B" } else { "A" },
                            self.chart_anim_epoch
                        ),
                    ),
                ),
        )
        .child(
            cx,
            div()
                .flex_row()
                .flex_wrap()
                .gap(14)
                .items_start()
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("Frame Rate (Hover / Scrub Area)").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            Chart::new(1830, 184, 96, fps_data, 40, 130)
                                .kind(ChartKind::Area)
                                .labels(FPS_LABELS)
                                .unit(" FPS")
                                .color(theme.primary)
                                .anim_epoch(self.chart_anim_epoch),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("Arena KB (Hover / Click Bar)").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            Chart::new(1831, 184, 96, mem_data, 0, 60)
                                .kind(ChartKind::Bar)
                                .labels(FPS_LABELS)
                                .unit(" KB")
                                .color(theme.success)
                                .anim_epoch(self.chart_anim_epoch),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("Donut Mix (Click Slice)").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            Chart::new(1834, 112, 96, pie_data, 0, 100)
                                .kind(ChartKind::Pie)
                                .labels(PIE_LABELS)
                                .donut_ratio(54)
                                .color(theme.primary)
                                .anim_epoch(self.chart_anim_epoch),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("OHLC Candles (Scrub)").size(11).color(theme.muted_foreground))
                        .child(
                            cx,
                            Chart::new(1835, 160, 96, &[], 40, 100)
                                .candles(candle_data)
                                .labels(CANDLE_LABELS)
                                .anim_epoch(self.chart_anim_epoch),
                        ),
                ),
        )
        .child(
            cx,
            Diff::from_patch(1832, "button.rs (Unified Diff)", SAMPLE_PATCH).width(740),
        )
        .child(
            cx,
            DescriptionList::new(1833, ENGINE_SPECS)
                .title("Engine Architecture Telemetry")
                .widths(740, 150)
                .bordered(true),
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
                    "Table, List, Tree, Charts & Diff",
                    "High-density velox-component data views: DataTable, List, Tree, Chart, Diff & DescriptionList.",
                    "velox_component::{DataTable, List, Tree, Chart, Diff}",
                ),
            )
            .child(cx, table_section)
            .child(cx, bottom_row)
            .child(cx, charts_diff_section)
    }
}
