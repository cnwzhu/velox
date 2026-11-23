//! 文本样式属性（[`Attrs`]）与单行稀疏属性区间表（[`AttrsList`]）。
//!
//! 参照 `pop-os/cosmic-text/src/attrs.rs` 设计：每行 `BufferLine` 维护默认属性与按字节区间覆盖的稀疏样式表，
//! 支持行拆分（`split_off`）与行合并（`append`）时自动保真迁移语法高亮与富文本属性。

use core::ops::Range;
use velox_engine::arena::SmallVec;
use velox_engine::style::Color;

/// 字体字重（对应 `cosmic-text` `Weight`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const THIN: Self = Self(100);
    pub const LIGHT: Self = Self(300);
    pub const NORMAL: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const SEMIBOLD: Self = Self(600);
    pub const BOLD: Self = Self(700);
    pub const BLACK: Self = Self(900);
}

impl Default for FontWeight {
    #[inline(always)]
    fn default() -> Self {
        Self::NORMAL
    }
}

/// 字体斜体风格（对应 `cosmic-text` `Style`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// 文本装饰线类型（下划线、删除线、诊断波浪线）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
    Squiggle,
}

/// 单个文本片段的富文本/语法样式属性（对应 `cosmic-text` `Attrs`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Attrs {
    /// 可选前景色覆盖（`None` 时继承编辑器/组件默认前景色）。
    pub color_opt: Option<Color>,
    /// 可选背景色覆盖（如内联高亮徽标或搜索词标记）。
    pub bg_opt: Option<Color>,
    /// 字体字重。
    pub weight: FontWeight,
    /// 字体倾斜风格。
    pub style: FontStyle,
    /// 文本装饰线。
    pub decoration: TextDecoration,
    /// 自定义业务元数据（例如语法高亮 Token 类别、诊断序号或超链接 ID）。
    pub metadata: usize,
}

impl Attrs {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            color_opt: None,
            bg_opt: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            metadata: 0,
        }
    }

    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.color_opt = Some(color);
        self
    }

    #[inline(always)]
    pub const fn bg(mut self, bg: Color) -> Self {
        self.bg_opt = Some(bg);
        self
    }

    #[inline(always)]
    pub const fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    #[inline(always)]
    pub const fn style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }

    #[inline(always)]
    pub const fn decoration(mut self, decoration: TextDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    #[inline(always)]
    pub const fn metadata(mut self, metadata: usize) -> Self {
        self.metadata = metadata;
        self
    }
}

/// 单行稀疏属性区间条目（左闭右开 `[start, end)`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AttrsSpan {
    pub start: usize,
    pub end: usize,
    pub attrs: Attrs,
}

/// 单行属性区间列表（对应 `cosmic-text` `AttrsList`）。
///
/// 维护行默认属性 `defaults` 以及按起始字节偏移升序排列、互不重叠的覆盖区间集合 `spans`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrsList {
    defaults: Attrs,
    spans: SmallVec<AttrsSpan, 16>,
}

impl Default for AttrsList {
    #[inline]
    fn default() -> Self {
        Self::new(Attrs::new())
    }
}

impl AttrsList {
    /// 以指定默认属性创建空区间列表。
    #[inline]
    pub const fn new(defaults: Attrs) -> Self {
        Self {
            defaults,
            spans: SmallVec::new(),
        }
    }

    /// 返回行默认属性。
    #[inline(always)]
    pub const fn defaults(&self) -> Attrs {
        self.defaults
    }

    /// 清除所有覆盖区间，保留默认属性。
    #[inline]
    pub fn clear_spans(&mut self) {
        self.spans.clear();
    }

    /// 重置默认属性并清除所有覆盖区间。
    #[inline]
    pub fn reset(&mut self, defaults: Attrs) {
        self.defaults = defaults;
        self.spans.clear();
    }

    /// 只读查看所有覆盖区间切片。
    #[inline]
    pub fn spans(&self) -> &[AttrsSpan] {
        self.spans.as_slice()
    }

    /// 查询字节偏移 `index` 处生效的最终属性（未命中任何覆盖区间时返回 `defaults`）。
    #[inline]
    pub fn get_span(&self, index: usize) -> Attrs {
        let slice = self.spans.as_slice();
        if slice.is_empty() {
            return self.defaults;
        }
        // 二分查找第一个 `end > index` 的区间
        let mut lo = 0usize;
        let mut hi = slice.len();
        while lo < hi {
            let mid = lo + ((hi - lo) >> 1);
            if slice[mid].end <= index {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if let Some(sp) = slice.get(lo) {
            if sp.start <= index && index < sp.end {
                return sp.attrs;
            }
        }
        self.defaults
    }

    /// 添加或覆写一个字节区间 `range` 的样式属性（自动裁剪/分裂与之重叠的旧区间并保持升序）。
    pub fn add_span(&mut self, range: Range<usize>, attrs: Attrs) {
        if range.start >= range.end {
            return;
        }
        let mut next: SmallVec<AttrsSpan, 16> = SmallVec::new();
        let mut inserted = false;

        for &sp in self.spans.as_slice() {
            if sp.end <= range.start {
                let _ = next.push(sp);
            } else if sp.start >= range.end {
                if !inserted {
                    let _ = next.push(AttrsSpan {
                        start: range.start,
                        end: range.end,
                        attrs,
                    });
                    inserted = true;
                }
                let _ = next.push(sp);
            } else {
                // 与新区间存在重叠：保留左侧未重叠片段
                if sp.start < range.start {
                    let _ = next.push(AttrsSpan {
                        start: sp.start,
                        end: range.start,
                        attrs: sp.attrs,
                    });
                }
                if !inserted {
                    let _ = next.push(AttrsSpan {
                        start: range.start,
                        end: range.end,
                        attrs,
                    });
                    inserted = true;
                }
                // 保留右侧未重叠片段
                if sp.end > range.end {
                    let _ = next.push(AttrsSpan {
                        start: range.end,
                        end: sp.end,
                        attrs: sp.attrs,
                    });
                }
            }
        }
        if !inserted {
            let _ = next.push(AttrsSpan {
                start: range.start,
                end: range.end,
                attrs,
            });
        }
        self.spans = next;
    }

    /// 在字节偏移 `index` 处拆分属性列表：当前列表保留 `[0..index)`，返回 `[index..)` 并将其偏移平移回从 `0` 开始。
    pub fn split_off(&mut self, index: usize) -> Self {
        let mut left: SmallVec<AttrsSpan, 16> = SmallVec::new();
        let mut right: SmallVec<AttrsSpan, 16> = SmallVec::new();

        for &sp in self.spans.as_slice() {
            if sp.end <= index {
                let _ = left.push(sp);
            } else if sp.start >= index {
                let _ = right.push(AttrsSpan {
                    start: sp.start - index,
                    end: sp.end - index,
                    attrs: sp.attrs,
                });
            } else {
                // 跨切分点的区间一分为二
                let _ = left.push(AttrsSpan {
                    start: sp.start,
                    end: index,
                    attrs: sp.attrs,
                });
                let _ = right.push(AttrsSpan {
                    start: 0,
                    end: sp.end - index,
                    attrs: sp.attrs,
                });
            }
        }
        self.spans = left;
        Self {
            defaults: self.defaults,
            spans: right,
        }
    }

    /// 将另一行的属性区间列表追加到当前行尾（以 `base_len` 为起始字节偏移）。
    pub fn append(&mut self, other: &Self, base_len: usize) {
        if other.defaults != self.defaults && other.spans.is_empty() {
            return;
        }
        for &sp in other.spans.as_slice() {
            let _ = self.spans.push(AttrsSpan {
                start: sp.start.saturating_add(base_len),
                end: sp.end.saturating_add(base_len),
                attrs: sp.attrs,
            });
        }
    }
}
