//! 百万行稀疏电子表格数据模型、视口布局唯一真源、像素级拾取与键盘直输编辑状态机。

use std::collections::HashMap;

use velox_kit::pod::KeyCode;

use super::formula;
use super::types::{CellCoord, CellRange, CellValue};

/// 百万行稀疏电子表格数据模型。
pub struct SheetGridModel {
    pub row_count: u32,
    pub col_count: u32,
    pub default_row_h: i16,
    pub default_col_w: i16,
    pub header_w: i16,
    pub header_h: i16,
    pub frozen_rows: u32,
    pub frozen_cols: u32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub selection: CellRange,
    /// 当前正在直接键盘编辑的单元格草稿缓冲（敲字即显，回车提交并实时触发公式重算）。
    pub editing_buffer: Option<String>,
    cells: HashMap<(u32, u32), CellValue>,
    col_widths: HashMap<u32, i16>,
    revision: u64,
}

impl Default for SheetGridModel {
    fn default() -> Self {
        Self::new(1_000_000, 1_000)
    }
}

impl SheetGridModel {
    /// 创建指定规模的百万级电子表格模型（默认 O(1) 初始内存）。
    pub fn new(row_count: u32, col_count: u32) -> Self {
        Self {
            row_count: row_count.max(1),
            col_count: col_count.max(1),
            default_row_h: 24,
            default_col_w: 96,
            header_w: 62,
            header_h: 26,
            frozen_rows: 1,
            frozen_cols: 1,
            scroll_x: 0.0,
            scroll_y: 0.0,
            selection: CellRange::single(CellCoord::new(1, 1)),
            editing_buffer: None,
            cells: HashMap::new(),
            col_widths: HashMap::new(),
            revision: 1,
        }
    }

    /// 创建内置财务/量化示例数据集（1,000,000 行容量 + 真实聚合公式）。
    pub fn demo_million_rows() -> Self {
        let mut m = Self::new(1_000_000, 128);
        m.set_col_width(0, 118);
        m.set_col_width(1, 92);
        m.set_col_width(2, 92);
        m.set_col_width(3, 108);
        m.set_col_width(4, 112);

        m.set_text(0, 0, "Ticker / KPI");
        m.set_text(0, 1, "Qty");
        m.set_text(0, 2, "Price");
        m.set_text(0, 3, "Notional");
        m.set_text(0, 4, "Margin 12%");

        let rows = [
            ("NVDA", 120.0, 138.4),
            ("AAPL", 240.0, 226.5),
            ("MSFT", 85.0, 418.2),
            ("TSLA", 150.0, 254.0),
            ("AMZN", 180.0, 198.6),
            ("GOOG", 110.0, 176.8),
        ];
        for (i, &(sym, qty, px)) in rows.iter().enumerate() {
            let r = (i + 1) as u32;
            m.set_text(r, 0, sym);
            m.set_number(r, 1, qty);
            m.set_number(r, 2, px);
            m.set_formula(r, 3, format!("=B{}*C{}", r + 1, r + 1));
            m.set_formula(r, 4, format!("=D{}*0.12", r + 1));
        }
        m.set_text(7, 0, "TOTAL (SUM)");
        m.set_formula(7, 1, "=SUM(B2:B7)");
        m.set_formula(7, 2, "=AVG(C2:C7)");
        m.set_formula(7, 3, "=SUM(D2:D7)");
        m.set_formula(7, 4, "=SUM(E2:E7)");
        m.selection = CellRange::new(CellCoord::new(1, 1), CellCoord::new(7, 4));
        m
    }

    #[inline]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set_col_width(&mut self, col: u32, w: i16) {
        self.col_widths.insert(col, w.clamp(36, 480));
        self.revision = self.revision.wrapping_add(1);
    }

    #[inline]
    pub fn col_width(&self, col: u32) -> i16 {
        self.col_widths.get(&col).copied().unwrap_or(self.default_col_w)
    }

    pub fn set_cell(&mut self, row: u32, col: u32, val: CellValue) {
        if row < self.row_count && col < self.col_count {
            match val {
                CellValue::Empty => {
                    self.cells.remove(&(row, col));
                }
                v => {
                    self.cells.insert((row, col), v);
                }
            }
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn set_number(&mut self, row: u32, col: u32, v: f64) {
        self.set_cell(row, col, CellValue::Number(v));
    }

    pub fn set_text(&mut self, row: u32, col: u32, s: impl Into<String>) {
        self.set_cell(row, col, CellValue::Text(s.into()));
    }

    pub fn set_formula(&mut self, row: u32, col: u32, expr: impl Into<String>) {
        self.set_cell(row, col, CellValue::Formula(expr.into()));
    }

    pub fn get_raw(&self, row: u32, col: u32) -> Option<&CellValue> {
        self.cells.get(&(row, col))
    }

    /// 递归求值单元格数值（含循环引用保护深度 `depth <= 16`）。
    pub fn eval_number(&self, row: u32, col: u32) -> Option<f64> {
        self.eval_number_depth(row, col, 0)
    }

    pub(crate) fn eval_number_depth(&self, row: u32, col: u32, depth: u8) -> Option<f64> {
        if depth > 16 {
            return None;
        }
        match self.cells.get(&(row, col)) {
            Some(CellValue::Number(n)) => Some(*n),
            Some(CellValue::Formula(expr)) => formula::eval_formula(self, expr, depth + 1),
            Some(CellValue::Text(s)) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    /// 求值公式字符串（委托至 [`formula::eval_formula`]）。
    #[inline]
    pub fn eval_formula(&self, expr: &str, depth: u8) -> Option<f64> {
        formula::eval_formula(self, expr, depth)
    }

    /// 获取单元格格式化显示文本（无数据时支持合成百万行虚拟占位内容，便于海量滚动验证）。
    pub fn display_text(&self, row: u32, col: u32) -> String {
        match self.cells.get(&(row, col)) {
            Some(CellValue::Empty) | None => {
                if row >= 8 && col == 0 {
                    format!("ROW-{:07}", row + 1)
                } else if row >= 8 && col <= 4 {
                    let v = ((row as u64 * 37 + col as u64 * 17) % 9973) as f64 / 10.0;
                    format!("{v:.1}")
                } else {
                    String::new()
                }
            }
            Some(CellValue::Number(v)) => {
                if (v - v.round()).abs() < 1e-6 {
                    format!("{:.0}", v)
                } else {
                    format!("{:.2}", v)
                }
            }
            Some(CellValue::Text(s)) => s.clone(),
            Some(CellValue::Formula(_)) => match self.eval_number(row, col) {
                Some(v) => format!("{:.2}", v),
                None => String::from("#ERR"),
            },
        }
    }

    /// 填充柄操作：将选区首行内容向下复制填充至选区末行（数字自动等差递增）。
    pub fn fill_selection_down(&mut self) -> usize {
        let (r0, c0, r1, c1) = self.selection.normalized();
        if r1 <= r0 {
            return 0;
        }
        let mut filled = 0usize;
        for c in c0..=c1 {
            let src = self.cells.get(&(r0, c)).cloned();
            if let Some(val) = src {
                for r in (r0 + 1)..=r1 {
                    let next_val = match &val {
                        CellValue::Number(n) => CellValue::Number(*n + (r - r0) as f64),
                        other => other.clone(),
                    };
                    self.set_cell(r, c, next_val);
                    filled += 1;
                }
            }
        }
        filled
    }

    /// 滚动视口（自动限制在 1,000,000 行边界内）。
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        let max_y = (self.row_count.saturating_sub(1) as f32) * self.default_row_h as f32;
        let max_x = (self.col_count.saturating_sub(1) as f32) * self.default_col_w as f32;
        self.scroll_x = (self.scroll_x + dx).clamp(0.0, max_x);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max_y);
    }

    /// 瞬间跳转至指定行（如一键跳到第 500,000 行）。
    pub fn jump_to_row(&mut self, row: u32) {
        let target = row.min(self.row_count.saturating_sub(1));
        self.scroll_y = target.saturating_sub(self.frozen_rows) as f32 * self.default_row_h as f32;
        self.selection = CellRange::single(CellCoord::new(target, self.selection.focus.col));
    }

    /// 返回单元格的原始公式/文本（供顶部公式栏显示）。
    pub fn raw_formula_or_text(&self, row: u32, col: u32) -> String {
        if self.selection.focus == CellCoord::new(row, col) {
            if let Some(ref buf) = self.editing_buffer {
                return buf.clone();
            }
        }
        match self.cells.get(&(row, col)) {
            Some(CellValue::Formula(f)) => f.clone(),
            Some(CellValue::Number(n)) => format!("{n}"),
            Some(CellValue::Text(t)) => t.clone(),
            _ => self.display_text(row, col),
        }
    }

    /// 将草稿文本提交写入当前焦点单元格（自动识别 `=` 公式 / 数值 / 文本）。
    pub fn commit_edit(&mut self) -> bool {
        let Some(raw) = self.editing_buffer.take() else {
            return false;
        };
        let f = self.selection.focus;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            self.set_cell(f.row, f.col, CellValue::Empty);
        } else if trimmed.starts_with('=') {
            self.set_formula(f.row, f.col, trimmed);
        } else if let Ok(num) = trimmed.parse::<f64>() {
            self.set_number(f.row, f.col, num);
        } else {
            self.set_text(f.row, f.col, trimmed);
        }
        true
    }

    /// 处理直接键盘字符输入（支持直接往选中格敲数字、公式、退格、Delete 清空与 Enter 提交）。
    pub fn handle_key_input(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char(ch) if !ch.is_control() => {
                let buf = self.editing_buffer.get_or_insert_with(String::new);
                buf.push(ch);
                true
            }
            KeyCode::Backspace => {
                if let Some(ref mut buf) = self.editing_buffer {
                    buf.pop();
                } else {
                    let f = self.selection.focus;
                    let mut cur = self.raw_formula_or_text(f.row, f.col);
                    cur.pop();
                    self.editing_buffer = Some(cur);
                }
                true
            }
            KeyCode::Delete => {
                self.editing_buffer = None;
                let (r0, c0, r1, c1) = self.selection.normalized();
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        self.set_cell(r, c, CellValue::Empty);
                    }
                }
                true
            }
            KeyCode::Enter => {
                let _ = self.commit_edit();
                self.move_focus(1, 0, false);
                true
            }
            KeyCode::Escape => {
                self.editing_buffer = None;
                true
            }
            _ => false,
        }
    }

    /// 键盘方向键移动焦点格（`extend == true` 表示按住 Shift 扩展矩形选区）。
    pub fn move_focus(&mut self, dr: i32, dc: i32, extend: bool) {
        let _ = self.commit_edit();
        let nr = (self.selection.focus.row as i64 + dr as i64)
            .clamp(0, self.row_count.saturating_sub(1) as i64) as u32;
        let nc = (self.selection.focus.col as i64 + dc as i64)
            .clamp(0, self.col_count.saturating_sub(1) as i64) as u32;
        let next = CellCoord::new(nr, nc);
        if extend {
            self.selection.focus = next;
        } else {
            self.selection = CellRange::single(next);
        }
    }

    /// 计算当前视口宽度 `view_w` 下所有可见列的精确局部坐标 `(col_index, local_x, col_width)`。
    /// `render_once` 与 [`Self::hit_test_cell`] 共用此唯一真源，保证渲染与鼠标拾取 100% 像素级对齐。
    pub fn visible_cols_layout(&self, view_w: i16) -> Vec<(u32, i16, i16)> {
        let hw = self.header_w.max(40);
        let mut vis_cols = Vec::new();
        let mut cx_pos = hw;
        for c in 0..self.frozen_cols.min(self.col_count) {
            let cw = self.col_width(c);
            if cx_pos < view_w {
                vis_cols.push((c, cx_pos, cw));
                cx_pos = cx_pos.saturating_add(cw);
            }
        }
        let first_scroll_col = self.frozen_cols
            + (self.scroll_x / self.default_col_w.max(1) as f32)
                .floor()
                .max(0.0) as u32;
        let mut c_cur = first_scroll_col;
        while cx_pos < view_w && c_cur < self.col_count {
            let cw = self.col_width(c_cur);
            vis_cols.push((c_cur, cx_pos, cw));
            cx_pos = cx_pos.saturating_add(cw);
            c_cur += 1;
        }
        vis_cols
    }

    /// 计算当前视口高度 `view_h` 下所有可见行的精确局部坐标 `(row_index, local_y, row_height)`。
    /// `render_once` 与 [`Self::hit_test_cell`] 共用此唯一真源，彻底消除非整行 `scroll_y` 导致的错行偏差。
    pub fn visible_rows_layout(&self, view_h: i16) -> Vec<(u32, i16, i16)> {
        let rh = self.default_row_h.max(18);
        let hh = self.header_h.max(20);
        let mut vis_rows = Vec::new();
        let mut cy_pos = hh;
        for r in 0..self.frozen_rows.min(self.row_count) {
            if cy_pos < view_h {
                vis_rows.push((r, cy_pos, rh));
                cy_pos = cy_pos.saturating_add(rh);
            }
        }
        let first_scroll_row =
            self.frozen_rows + (self.scroll_y / rh as f32).floor().max(0.0) as u32;
        let mut r_cur = first_scroll_row;
        while cy_pos < view_h && r_cur < self.row_count {
            vis_rows.push((r_cur, cy_pos, rh));
            cy_pos = cy_pos.saturating_add(rh);
            r_cur += 1;
        }
        vis_rows
    }

    /// 屏幕局部像素坐标（相对于 `SheetGrid` 左上角 `(0, 0)`）精确拾取命中的单元格。
    pub fn hit_test_cell(&self, lx: i16, ly: i16) -> Option<CellCoord> {
        let hw = self.header_w.max(40);
        let hh = self.header_h.max(20);
        if lx < hw || ly < hh {
            return None;
        }
        let vis_rows =
            self.visible_rows_layout(ly.saturating_add(self.default_row_h.max(18)).saturating_add(1));
        let row = vis_rows
            .iter()
            .find(|&&(_, y, rh)| ly >= y && ly < y.saturating_add(rh))
            .map(|&(r, _, _)| r)?;

        let vis_cols = self.visible_cols_layout(lx.saturating_add(480));
        let col = vis_cols
            .iter()
            .find(|&&(_, x, cw)| lx >= x && lx < x.saturating_add(cw))
            .map(|&(c, _, _)| c)?;

        Some(CellCoord::new(row, col))
    }
}
