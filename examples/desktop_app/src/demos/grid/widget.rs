//! 声明式百万行虚拟电子表格组件（`SheetGrid`）：每帧 O(可见格数) 渲染，彻底零 Widget 树膨胀。

use velox_kit::prelude::*;

use super::model::SheetGridModel;
use super::types::{col_index_to_label, CellCoord, CellValue};

/// 声明式百万行虚拟电子表格组件（`SheetGrid`）。
pub struct SheetGrid<'s, 'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub model: &'s SheetGridModel,
    pub on_wheel: Option<&'a dyn Fn(WheelEvent, &mut ActionScope)>,
    pub on_drag: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    pub on_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
    pub on_key_nav: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
}

impl<'s, 'a> SheetGrid<'s, 'a> {
    pub fn new(id: u32, width: i16, height: i16, model: &'s SheetGridModel) -> Self {
        Self {
            id,
            width,
            height,
            model,
            on_wheel: None,
            on_drag: None,
            on_click: None,
            on_key_nav: None,
            on_key_input: None,
        }
    }

    #[inline]
    pub fn on_wheel(mut self, cb: &'a dyn Fn(WheelEvent, &mut ActionScope)) -> Self {
        self.on_wheel = Some(cb);
        self
    }

    /// 绑定精确局部坐标点击/框选回调（`DragEvent::local_x/local_y` 严格相对 `SheetGrid` 左上角，
    /// 按下瞬间以 `delta_x == 0 && delta_y == 0` 触发单格选中，按住拖动实时扩展矩形选区）。
    #[inline]
    pub fn on_drag(mut self, cb: &'a dyn Fn(DragEvent, &mut ActionScope)) -> Self {
        self.on_drag = Some(cb);
        self
    }

    #[inline]
    pub fn on_click(mut self, cb: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.on_click = Some(cb);
        self
    }

    #[inline]
    pub fn on_key_nav(mut self, cb: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_nav = Some(cb);
        self
    }

    /// 绑定直接键盘字符/公式编辑回调。
    #[inline]
    pub fn on_key_input(mut self, cb: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(cb);
        self
    }
}

impl<'s, 'a> Widget<'a> for SheetGrid<'s, 'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = *cx.theme();
        let arena = cx.arena();
        let m = self.model;

        let hh = m.header_h.max(20);
        let hw = m.header_w.max(40);
        let vis_cols = m.visible_cols_layout(self.width);
        let vis_rows = m.visible_rows_layout(self.height);

        let mut root = div()
            .id(self.id)
            .w(self.width)
            .h(self.height)
            .rounded(8)
            .bg(theme.background)
            .border(1, theme.border)
            .overflow_hidden()
            .cursor(CursorShape::Default)
            .focusable();

        // 左上角全选标头
        root = root.child(
            cx,
            div()
                .pos(0, 0)
                .w(hw)
                .h(hh)
                .bg(theme.muted)
                .border(1, theme.border)
                .items_center()
                .justify_center()
                .child(cx, text("#").size(10).color(theme.muted_foreground)),
        );

        // 列标头 A, B, C...
        for &(col, x, cw) in &vis_cols {
            let lbl = arena.alloc_str(&col_index_to_label(col)).as_str();
            let col_sel = {
                let (_, c0, _, c1) = m.selection.normalized();
                col >= c0 && col <= c1
            };
            root = root.child(
                cx,
                div()
                    .pos(x, 0)
                    .w(cw.min(self.width - x).max(1))
                    .h(hh)
                    .bg(if col_sel {
                        theme.primary.scale_alpha(44)
                    } else {
                        theme.muted
                    })
                    .border(1, theme.border)
                    .items_center()
                    .justify_center()
                    .child(
                        cx,
                        text(lbl)
                            .size(10)
                            .color(if col_sel {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            })
                            .nowrap(),
                    ),
            );
        }

        // 行号与可见单元格
        for &(row, y, rh) in &vis_rows {
            let is_frozen_row = row < m.frozen_rows;
            let row_sel = {
                let (r0, _, r1, _) = m.selection.normalized();
                row >= r0 && row <= r1
            };
            let rlbl = arena.alloc_str(&format!("{}", row + 1)).as_str();
            root = root.child(
                cx,
                div()
                    .pos(0, y)
                    .w(hw)
                    .h(rh)
                    .bg(if row_sel {
                        theme.primary.scale_alpha(44)
                    } else {
                        theme.muted
                    })
                    .border(1, theme.border)
                    .items_center()
                    .justify_center()
                    .child(
                        cx,
                        text(rlbl)
                            .size(10)
                            .color(if row_sel {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            })
                            .nowrap(),
                    ),
            );

            for &(col, x, cw) in &vis_cols {
                let in_sel = m.selection.contains(row, col);
                let is_focus = m.selection.focus == CellCoord::new(row, col);
                let cell_bg = if is_frozen_row {
                    theme.secondary.scale_alpha(200)
                } else if in_sel {
                    theme.primary.scale_alpha(38)
                } else if row % 2 == 1 {
                    theme.card.scale_alpha(130)
                } else {
                    theme.background
                };
                let editing_here = is_focus && m.editing_buffer.is_some();
                let val_owned = if editing_here {
                    format!("{}|", m.editing_buffer.as_deref().unwrap_or(""))
                } else {
                    m.display_text(row, col)
                };
                let val_str = arena.alloc_str(&val_owned).as_str();
                let is_formula = matches!(m.get_raw(row, col), Some(CellValue::Formula(_)));
                let txt_col = if editing_here {
                    Color::rgb(250, 204, 21)
                } else if is_frozen_row {
                    theme.foreground
                } else if is_formula {
                    theme.primary
                } else {
                    theme.foreground
                };

                let mut cell = div()
                    .pos(x, y)
                    .w(cw.min(self.width - x).max(1))
                    .h(rh)
                    .px(6)
                    .bg(cell_bg)
                    .border(
                        if is_focus { 2 } else { 1 },
                        if is_focus {
                            theme.primary
                        } else {
                            theme.border.scale_alpha(120)
                        },
                    )
                    .items_center()
                    .overflow_hidden();
                let t = text(val_str).size(11).color(txt_col).nowrap();
                cell = cell.child(cx, t);
                root = root.child(cx, cell);
            }
        }

        if let Some(cb) = self.on_wheel {
            root = root.on_wheel(cx, cb);
        }
        if let Some(cb) = self.on_drag {
            root = root.on_drag(cx, cb);
        }
        if let Some(cb) = self.on_click {
            root = root.on_click(cx, cb);
        }
        if let Some(cb) = self.on_key_nav {
            root = root.on_key_nav(cb);
        }
        if let Some(cb) = self.on_key_input {
            root = root.on_key_input(cb);
        }

        root.into_element(cx)
    }
}
