//! `Tabs & Accordion` 故事页：`Tabs` 三变体（Segmented / Underline / Pill）+ `Accordion` + `Stepper` + `Pagination`。

use super::{state_pill, story_header, story_section};
use crate::style::{self, BasePalette};
use crate::GalleryApp;
use velox_kit::base::{Accordion, AccordionItem, Pagination, StepItem, StepStatus, Stepper, Tabs};
use velox_kit::prelude::*;

const ACCORDION_ITEMS: &[AccordionItem<'static>] = &[
    AccordionItem::new(
        "How does Velox handle focus & keyboard nav?",
        "Every control implements controlled state, optional focus rings, Escape dismissal, and arrow-key roving navigation.",
    ),
    AccordionItem::new(
        "Is the runtime really zero-heap per frame?",
        "Yes. Transient layout trees and draw lists live in a fixed ScratchArena reset cleanly every frame.",
    ),
    AccordionItem::new(
        "How do icons scale at 150% / 200% DPI?",
        "Built-in icons use native 24x24 Lucide SVG vector paths rasterized at physical DPI with GPU atlas caching.",
    ),
];

impl GalleryApp {
    pub(crate) fn render_story_navigation<'a>(
        &self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let p = BasePalette::from_theme(theme);
        let on_tab_seg = cx.listener(|this, idx: usize, _acx| {
            this.tabs_segmented_idx = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_tab_ul = cx.listener(|this, idx: usize, _acx| {
            this.tabs_underline_idx = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_tab_pill = cx.listener(|this, idx: usize, _acx| {
            this.tabs_pill_idx = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_acc_inner = cx.listener(|this, (idx, open): (usize, bool), _acx| {
            if open {
                this.accordion_mask |= 1u32 << idx;
            } else {
                this.accordion_mask &= !(1u32 << idx);
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_acc = cx.arena().alloc(move |idx: usize, open: bool, acx: &mut ActionScope| {
            on_acc_inner((idx, open), acx);
        });
        let on_step = cx.listener(|this, idx: usize, _acx| {
            this.stepper_active = idx;
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_page = cx.listener(|this, page: usize, _acx| {
            this.pagination_page = page;
            this.event_count = this.event_count.saturating_add(1);
        });

        let tabs_section = story_section(
            cx,
            theme,
            "Tabs (Segmented • Underline • Pill)",
            "Focus any tab and use Arrow Left/Right or Home/End to traverse",
        )
        .child(
            cx,
            div()
                .flex_col()
                .w_full()
                .gap(12)
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("Segmented (Default)").size(11).color(p.muted_fg))
                        .child(
                            cx,
                            div().w(420).child(
                                cx,
                                style::tabs(
                                    Tabs::new(
                                        1501,
                                        &["Account", "Security", "Billing", "API Keys"],
                                        self.tabs_segmented_idx,
                                    )
                                    .segmented()
                                    .on_select(on_tab_seg),
                                    theme,
                                ),
                            ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("Underline").size(11).color(p.muted_fg))
                        .child(
                            cx,
                            div().w(420).child(
                                cx,
                                style::tabs(
                                    Tabs::new(
                                        1502,
                                        &["Overview", "Performance", "Diagnostics"],
                                        self.tabs_underline_idx,
                                    )
                                    .underline()
                                    .on_select(on_tab_ul),
                                    theme,
                                ),
                            ),
                        ),
                )
                .child(
                    cx,
                    div()
                        .flex_col()
                        .gap(4)
                        .child(cx, text("Block / Pill").size(11).color(p.muted_fg))
                        .child(
                            cx,
                            div().w(360).child(
                                cx,
                                style::tabs(
                                    Tabs::new(
                                        1503,
                                        &["Daily", "Weekly", "Monthly"],
                                        self.tabs_pill_idx,
                                    )
                                    .pill()
                                    .on_select(on_tab_pill),
                                    theme,
                                ),
                            ),
                        ),
                ),
        );

        let steps = cx.arena().alloc_iter((0..3usize).map(|i| {
            let status = if i < self.stepper_active {
                StepStatus::Completed
            } else if i == self.stepper_active {
                StepStatus::Active
            } else {
                StepStatus::Pending
            };
            match i {
                0 => StepItem::new("Configure", "Theme & Locale", status),
                1 => StepItem::new("Build", "Compile SDF Shaders", status),
                _ => StepItem::new("Ship", "Zero-Heap Release", status),
            }
        }));

        let bottom_row = div()
            .flex_row()
            .w_full()
            .gap(16)
            .items_start()
            .child(
                cx,
                div().flex_grow(1).child(
                    cx,
                    story_section(cx, theme, "Accordion", "SVG chevrons & crisp dividers")
                        .child(
                            cx,
                            style::accordion(
                                Accordion::new(1510, ACCORDION_ITEMS, self.accordion_mask)
                                    .width(380)
                                    .on_toggle_item(on_acc),
                                theme,
                            ),
                        ),
                ),
            )
            .child(
                cx,
                div().flex_grow(1).flex_col().gap(16).child(
                    cx,
                    story_section(cx, theme, "Stepper & Pagination", "Click steps or page numbers")
                        .child(
                            cx,
                            style::stepper(
                                Stepper::new(1520, steps).on_step_click(on_step),
                                theme,
                            ),
                        )
                        .child(
                            cx,
                            div()
                                .flex_row()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .child(
                                    cx,
                                    style::pagination(
                                        Pagination::new(1521, self.pagination_page, 9)
                                            .visible_pages(5)
                                            .on_change(on_page),
                                        theme,
                                    ),
                                )
                                .child(
                                    cx,
                                    state_pill(
                                        cx,
                                        theme,
                                        fmt!(cx, "Page {} / 9", self.pagination_page),
                                    ),
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
                    "Tabs, Accordion & Navigation",
                    "Headless tabs, accordions, steppers & pagination styled with 0px monochrome geometry.",
                    "velox_base::{Tabs, Accordion, Stepper, Pagination}",
                ),
            )
            .child(cx, tabs_section)
            .child(cx, bottom_row)
    }
}
