//! 电子表格核心坐标、选区范围、单元格值类型与 Excel A1 地址编解码。

/// 单元格行列坐标（0-based）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct CellCoord {
    pub row: u32,
    pub col: u32,
}

impl CellCoord {
    #[inline(always)]
    pub const fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }
}

/// 矩形选区范围（支持任意反向拖拽，自动归一化为左上→右下）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct CellRange {
    pub anchor: CellCoord,
    pub focus: CellCoord,
}

impl CellRange {
    #[inline(always)]
    pub const fn single(c: CellCoord) -> Self {
        Self {
            anchor: c,
            focus: c,
        }
    }

    #[inline(always)]
    pub const fn new(anchor: CellCoord, focus: CellCoord) -> Self {
        Self { anchor, focus }
    }

    #[inline]
    pub fn normalized(&self) -> (u32, u32, u32, u32) {
        (
            self.anchor.row.min(self.focus.row),
            self.anchor.col.min(self.focus.col),
            self.anchor.row.max(self.focus.row),
            self.anchor.col.max(self.focus.col),
        )
    }

    #[inline]
    pub fn contains(&self, row: u32, col: u32) -> bool {
        let (r0, c0, r1, c1) = self.normalized();
        row >= r0 && row <= r1 && col >= c0 && col <= c1
    }

    #[inline]
    pub fn cell_count(&self) -> u64 {
        let (r0, c0, r1, c1) = self.normalized();
        (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64
    }
}

/// 单元格原始存储值。
#[derive(Clone, PartialEq, Debug, Default)]
pub enum CellValue {
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Formula(String),
}

/// 将 0-based 列索引转为 Excel 列标（`0 -> "A"`, `25 -> "Z"`, `26 -> "AA"`）。
pub fn col_index_to_label(mut col: u32) -> String {
    let mut chars = Vec::new();
    loop {
        let rem = (col % 26) as u8;
        chars.push((b'A' + rem) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    chars.reverse();
    chars.into_iter().collect()
}

/// 解析 Excel A1 单元格地址（如 `"B12"` → `CellCoord { row: 11, col: 1 }`）。
pub fn parse_a1_ref(s: &str) -> Option<CellCoord> {
    let s = s.trim();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut col: u32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        let ch = bytes[i].to_ascii_uppercase();
        col = col.checked_mul(26)?.checked_add((ch - b'A') as u32 + 1)?;
        i += 1;
    }
    if i == 0 || i >= bytes.len() {
        return None;
    }
    let row_1based: u32 = s[i..].parse().ok()?;
    if row_1based == 0 {
        return None;
    }
    Some(CellCoord {
        row: row_1based - 1,
        col: col - 1,
    })
}
