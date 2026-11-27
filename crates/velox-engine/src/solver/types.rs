//! 布局几何原语（`Region`）、交互热区（`HitRegion`、`InteractionState`）与瞬态 `LayoutNode` 定义。

use crate::element::{DivInteractivity, DivNode, Element, ElementNode};
use crate::pod::{ActionScope, CursorShape};
use crate::style::{AlignItems, Color, Edges, FlexDir, Justify, Length};

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Region {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

impl Region {
    /// 空矩形常量。
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    /// 构造矩形。
    #[inline(always)]
    pub const fn new(x: i16, y: i16, w: i16, h: i16) -> Self {
        Self { x, y, w, h }
    }

    /// 是否为空或无效区域。
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// 右边界坐标（`x + w`）。
    #[inline(always)]
    pub const fn right(self) -> i16 {
        self.x.saturating_add(self.w)
    }

    /// 下边界坐标（`y + h`）。
    #[inline(always)]
    pub const fn bottom(self) -> i16 {
        self.y.saturating_add(self.h)
    }

    /// 计算矩形面积（像素数）。
    #[inline(always)]
    pub const fn area(self) -> i32 {
        if self.w <= 0 || self.h <= 0 {
            0
        } else {
            (self.w as i32) * (self.h as i32)
        }
    }

    /// 判断点 `(px, py)` 是否落在该矩形内。
    #[inline(always)]
    pub const fn contains(self, px: i16, py: i16) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// 判断两个矩形是否相交或边缘接触。
    #[inline(always)]
    pub const fn overlaps_or_touches(self, other: Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.x <= other.right()
            && self.right() >= other.x
            && self.y <= other.bottom()
            && self.bottom() >= other.y
    }

    /// 求两个矩形的交集（裁剪框计算）。
    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 || y2 <= y1 {
            Self::EMPTY
        } else {
            Self {
                x: x1,
                y: y1,
                w: x2 - x1,
                h: y2 - y1,
            }
        }
    }

    /// 求两个矩形的最小包围并集（Union）。
    #[inline]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());
        Self {
            x: x1,
            y: y1,
            w: x2.saturating_sub(x1),
            h: y2.saturating_sub(y1),
        }
    }
}

/// 引擎交互伪类与焦点状态（用于驱动 `hover_bg`、`active_bg` 与 `:focus-visible` 主题融合焦点边框）。
#[derive(Copy, Clone, Debug)]
pub struct InteractionState {
    pub pointer_pos: Option<(i16, i16)>,
    pub is_pressed: bool,
    pub focused_index: Option<u16>,
    /// 是否由键盘导航（`Tab` / `BackTab` / 方向键）激活可见焦点环（模拟标准 `:focus-visible`，鼠标点击普通控件不弹出多余方框）。
    pub focus_visible: bool,
    /// 当前生效主题的默认焦点边框色（由引擎同步自 `theme().ring`，彻底消除硬编码青蓝框）。
    pub focus_ring_color: Color,
}

impl InteractionState {
    /// 编译期默认交互状态常量（支持 `VeloxEngine::new` 的 `#![no_std]` `const fn` 构造）。
    #[inline(always)]
    pub const fn const_default() -> Self {
        Self {
            pointer_pos: None,
            is_pressed: false,
            focused_index: None,
            focus_visible: false,
            focus_ring_color: Color::TRANSPARENT,
        }
    }
}

impl Default for InteractionState {
    #[inline(always)]
    fn default() -> Self {
        Self::const_default()
    }
}

/// 可交互热区条目（在 Pass 4 拍平时同步生成，用于 $O(N)$ 逆序命中测试、光标解析、触摸拖拽与按键焦点导航，体积仅 40B）。
#[derive(Copy, Clone)]
pub struct HitRegion<'a> {
    pub id: Option<u32>,
    pub bounds: Region,
    pub clip: Region,
    pub focus_index: Option<u16>,
    pub owner_pod: u16,
    pub scroll_target: Option<(u32, i16)>,
    pub interact: Option<&'a DivInteractivity<'a>>,
    pub occlude: bool,
    pub auto_repeat: bool,
    pub has_hover_bg: bool,
    pub has_active_bg: bool,
    pub scroll_horizontal: bool,
    pub cursor: Option<CursorShape>,
}

impl<'a> HitRegion<'a> {
    #[inline(always)]
    pub fn on_click(&self) -> Option<&'a dyn Fn(&mut ActionScope)> {
        self.interact.and_then(|i| i.on_click)
    }

    #[inline(always)]
    pub fn on_long_press(&self) -> Option<&'a dyn Fn(&mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_long_press)
    }

    #[inline(always)]
    pub fn on_drag(&self) -> Option<&'a dyn Fn(&mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_drag)
    }

    #[inline(always)]
    pub fn on_hover(&self) -> Option<&'a dyn Fn(bool, &mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_hover)
    }

    #[inline(always)]
    pub fn on_pointer_move(&self) -> Option<&'a dyn Fn(&mut ActionScope)> {
        self.interact
            .and_then(|i| i.rare)
            .and_then(|r| r.on_pointer_move)
    }

    #[inline(always)]
    pub fn on_wheel(&self) -> Option<&'a dyn Fn(crate::pod::WheelEvent, &mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_wheel)
    }

    #[inline(always)]
    pub fn on_pinch(&self) -> Option<&'a dyn Fn(crate::pod::PinchEvent, &mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_pinch)
    }

    #[inline(always)]
    pub fn on_swipe(&self) -> Option<&'a dyn Fn(crate::pod::SwipeDir, &mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_swipe)
    }

    #[inline(always)]
    pub fn on_key_input(&self) -> Option<&'a dyn Fn(crate::pod::KeyCode, &mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_key_input)
    }

    #[inline(always)]
    pub fn on_key_nav(&self) -> Option<&'a dyn Fn(crate::pod::KeyCode, &mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_key_nav)
    }

    #[inline(always)]
    pub fn on_navigate_back(&self) -> Option<&'a dyn Fn(&mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_navigate_back)
    }

    #[inline(always)]
    pub fn on_secondary_click(&self) -> Option<&'a dyn Fn(&mut ActionScope)> {
        self.interact.and_then(|i| i.rare).and_then(|r| r.on_secondary_click)
    }
}

pub(super) const SUBVIEW_FLAG_ROOT: u8 = 1 << 0;
pub(super) const SUBVIEW_FLAG_CACHED_PASS1: u8 = 1 << 1;
pub(super) const SUBVIEW_FLAG_SKIP_PASS3: u8 = 1 << 2;
pub(super) const SUBVIEW_FLAG_OWNER: u8 = 1 << 3;

/// 布局中间节点（分配在布局瞬态池中，利用水平/垂直求解生命周期不相交特性，复用 `min_size` / `pref_size` 字段）。
pub(crate) struct LayoutNode<'a, 'b> {
    pub(super) element: &'a Element<'a>,
    pub(super) min_size: i16,
    pub(super) pref_size: i16,
    pub(super) flex_grow: u8,
    pub(super) subview_flags: u8,
    pub(super) x: i16,
    pub(super) y: i16,
    pub(super) w: i16,
    pub(super) h: i16,
    pub(super) subview_pod: u16,
    pub(super) children: &'b mut [LayoutNode<'a, 'b>],
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) struct OuterBoxSpec {
    pub(super) kind: u8,
    pub(super) width: Length,
    pub(super) height: Length,
    pub(super) min_w: i16,
    pub(super) max_w: i16,
    pub(super) min_h: i16,
    pub(super) max_h: i16,
    pub(super) margin: Edges<i16>,
    pub(super) pos: Option<(i16, i16)>,
    pub(super) flex_grow: u8,
    pub(super) align_self: Option<AlignItems>,
    pub(super) flex_dir: FlexDir,
    pub(super) justify: Justify,
}

#[inline]
pub(super) fn outer_box_spec(elem: &Element<'_>) -> OuterBoxSpec {
    match elem {
        Element::Div(d) => OuterBoxSpec {
            kind: 1,
            width: d.style.width,
            height: d.style.height,
            min_w: d.min_w,
            max_w: d.max_w,
            min_h: d.min_h,
            max_h: d.max_h,
            margin: child_margin(elem),
            pos: d.pos(),
            flex_grow: d.style.flex_grow(),
            align_self: d.align_self(),
            flex_dir: d.style.flex_dir(),
            justify: d.style.justify(),
        },
        Element::Text(_) => OuterBoxSpec {
            kind: 2,
            width: Length::Auto,
            height: Length::Auto,
            min_w: 0,
            max_w: 0,
            min_h: 0,
            max_h: 0,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
        Element::Img(img) => OuterBoxSpec {
            kind: 3,
            width: Length::Px(img.width),
            height: Length::Px(img.height),
            min_w: img.width,
            max_w: img.width,
            min_h: img.height,
            max_h: img.height,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
        Element::RichText(_) => OuterBoxSpec {
            kind: 2,
            width: Length::Auto,
            height: Length::Auto,
            min_w: 0,
            max_w: 0,
            min_h: 0,
            max_h: 0,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
        Element::DecodedImage(img) => OuterBoxSpec {
            kind: 3,
            width: Length::Px(img.width),
            height: Length::Px(img.height),
            min_w: img.width,
            max_w: img.width,
            min_h: img.height,
            max_h: img.height,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
        Element::Vector(vec_spec) => OuterBoxSpec {
            kind: 3,
            width: Length::Px(vec_spec.width),
            height: Length::Px(vec_spec.height),
            min_w: vec_spec.width,
            max_w: vec_spec.width,
            min_h: vec_spec.height,
            max_h: vec_spec.height,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
        Element::SceneCanvas(spec) => OuterBoxSpec {
            kind: 3,
            width: Length::Px(spec.width),
            height: Length::Px(spec.height),
            min_w: spec.width,
            max_w: spec.width,
            min_h: spec.height,
            max_h: spec.height,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
        Element::SubView { .. } => OuterBoxSpec {
            kind: 4,
            width: Length::Auto,
            height: Length::Auto,
            min_w: 0,
            max_w: 0,
            min_h: 0,
            max_h: 0,
            margin: Edges::all(0),
            pos: None,
            flex_grow: 0,
            align_self: None,
            flex_dir: FlexDir::Col,
            justify: Justify::Start,
        },
    }
}


pub(super) fn count_children(mut cur: Option<&ElementNode<'_>>) -> usize {
    let mut n = 0;
    while let Some(node) = cur {
        n += 1;
        cur = node.next_sibling.get();
    }
    n
}

#[inline(always)]
pub(super) fn child_pos(elem: &Element<'_>) -> Option<(i16, i16)> {
    match elem {
        Element::Div(d) => d.pos(),
        _ => None,
    }
}

#[inline(always)]
pub(super) fn child_is_overlay(elem: &Element<'_>) -> bool {
    matches!(elem, Element::Div(d) if d.is_overlay())
}

#[inline(always)]
pub(super) fn div_padding(div: &DivNode<'_>) -> Edges<i16> {
    Edges {
        top: (div.style.padding.top as i16).saturating_add(div.extra_padding.top),
        right: (div.style.padding.right as i16).saturating_add(div.extra_padding.right),
        bottom: (div.style.padding.bottom as i16).saturating_add(div.extra_padding.bottom),
        left: (div.style.padding.left as i16).saturating_add(div.extra_padding.left),
    }
}

#[inline(always)]
pub(super) fn child_margin(elem: &Element<'_>) -> Edges<i16> {
    match elem {
        Element::Div(d) => {
            let base = d.style.margin_px() as i16;
            Edges {
                top: d.margin.top.saturating_add(base),
                right: d.margin.right.saturating_add(base),
                bottom: d.margin.bottom.saturating_add(base),
                left: d.margin.left.saturating_add(base),
            }
        }
        _ => Edges::all(0),
    }
}

#[inline(always)]
pub(super) fn clamp_w(elem: &Element<'_>, w: i16) -> i16 {
    if let Element::Div(d) = elem {
        let mut res = w.max(d.min_w);
        if d.max_w > 0 {
            res = res.min(d.max_w);
        }
        res
    } else {
        w
    }
}

#[inline(always)]
pub(super) fn clamp_h(elem: &Element<'_>, h: i16) -> i16 {
    if let Element::Div(d) = elem {
        let mut res = h.max(d.min_h);
        if d.max_h > 0 {
            res = res.min(d.max_h);
        }
        res
    } else {
        h
    }
}

