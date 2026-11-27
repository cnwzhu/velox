//! 高频时序环形缓冲（`RingSeries`）与金融 OHLC 蜡烛数据结构（`OhlcCandle`）。

use velox_kit::style::Color;

/// 固定容量高频时序环形缓冲（零重分配滑动窗口）。
#[derive(Clone, Debug)]
pub struct RingSeries {
    pub name: String,
    pub color: Color,
    cap: usize,
    buf: Vec<(f64, f64)>,
    head: usize,
    len: usize,
}

impl RingSeries {
    pub fn new(name: impl Into<String>, color: Color, capacity: usize) -> Self {
        let cap = capacity.max(16);
        Self {
            name: name.into(),
            color,
            cap,
            buf: vec![(0.0, 0.0); cap],
            head: 0,
            len: 0,
        }
    }

    /// 推入一个新采样点 `(x, y)`，超容时 O(1) 覆写最旧数据。
    #[inline]
    pub fn push(&mut self, x: f64, y: f64) {
        let write_idx = (self.head + self.len) % self.cap;
        self.buf[write_idx] = (x, y);
        if self.len < self.cap {
            self.len += 1;
        } else {
            self.head = (self.head + 1) % self.cap;
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按时间顺序导出当前窗口内的有序切片到目标缓冲 `out`。
    pub fn copy_ordered_into(&self, out: &mut Vec<(f64, f64)>) {
        out.clear();
        if self.len == 0 {
            return;
        }
        out.reserve(self.len);
        let first_part = (self.cap - self.head).min(self.len);
        out.extend_from_slice(&self.buf[self.head..self.head + first_part]);
        if self.len > first_part {
            out.extend_from_slice(&self.buf[0..self.len - first_part]);
        }
    }
}

/// 金融 OHLCV 蜡烛单根数据结构。
#[derive(Copy, Clone, Debug, Default)]
pub struct OhlcCandle {
    pub t: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}
