//! 百万行虚拟电子表格与公式引擎模块（`velox_base::grid`）。
//!
//! 模块职责划分：
//! - [`types`]：单元格坐标、矩形选区、值枚举与 Excel A1 地址编解码；
//! - [`formula`]：内置电子表格公式求值引擎（支持 `SUM`/`AVG`/`MIN`/`MAX` 与四则运算，便于扩展更多函数与 DAG 增量重算）；
//! - [`model`]：百万行稀疏数据模型、冻结行列、渲染/拾取唯一真源布局、填充柄与键盘直输编辑状态机；
//! - [`widget`]：零 Widget 树膨胀的声明式 [`SheetGrid`] 视图组件。

pub mod formula;
pub mod model;
pub mod types;
pub mod widget;

pub use model::SheetGridModel;
pub use types::{col_index_to_label, parse_a1_ref, CellCoord, CellRange, CellValue};
pub use widget::SheetGrid;

#[cfg(test)]
mod tests {
    use super::*;
    use velox_kit::pod::KeyCode;

    #[test]
    fn test_million_row_sheet_grid_formulas_fill_hit_parity_and_typing() {
        let mut m = SheetGridModel::demo_million_rows();
        assert_eq!(m.row_count, 1_000_000);
        assert_eq!(col_index_to_label(0), "A");
        assert_eq!(col_index_to_label(26), "AA");
        assert_eq!(parse_a1_ref("D8"), Some(CellCoord::new(7, 3)));

        let qty_sum = m.eval_number(7, 1).unwrap();
        assert!((qty_sum - 885.0).abs() < 1e-6, "got {qty_sum}");
        let margin_sum = m.eval_number(7, 4).unwrap();
        let notional_sum = m.eval_number(7, 3).unwrap();
        assert!((margin_sum - notional_sum * 0.12).abs() < 1e-3);

        m.set_number(20, 1, 100.0);
        m.selection = CellRange::new(CellCoord::new(20, 1), CellCoord::new(24, 1));
        assert_eq!(m.fill_selection_down(), 4);
        assert_eq!(m.eval_number(24, 1), Some(104.0));

        m.jump_to_row(500_000);
        assert_eq!(m.selection.focus.row, 500_000);
        assert_eq!(m.display_text(500_000, 0), "ROW-0500001");

        m.scroll_y = 317.0 * 24.0 + 13.5;
        m.scroll_x = 45.0;
        let cols = m.visible_cols_layout(780);
        let rows = m.visible_rows_layout(404);
        assert!(!cols.is_empty() && !rows.is_empty());
        for &(r, ry, rh) in &rows {
            for &(c, cx, cw) in &cols {
                let hit = m.hit_test_cell(cx + cw / 2, ry + rh / 2);
                assert_eq!(hit, Some(CellCoord::new(r, c)));
            }
        }

        m.selection = CellRange::single(CellCoord::new(10, 1));
        for ch in "=B2+10".chars() {
            assert!(m.handle_key_input(KeyCode::Char(ch)));
        }
        assert!(m.handle_key_input(KeyCode::Enter));
        assert_eq!(m.eval_number(10, 1), Some(130.0));
    }
}
