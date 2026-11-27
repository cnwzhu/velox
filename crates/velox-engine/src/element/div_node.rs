//! 紧凑 64B `DivNode`、按需分配视觉/交互扩展（`DivVisual` / `DivInteractivity`）与过渡动画配置（`TransitionSpec`）。

use core::time::Duration;

use crate::paint::PaintEffects;
use crate::pod::{ActionScope, CursorShape};
use crate::style::{AlignItems, Color, Easing, Edges, LoopMode, Style, StyleRefinement};

use super::ElementNode;

const VIS_FLAG_HAS_POS: u8 = 1 << 0;
const VIS_FLAG_HAS_SCROLL: u8 = 1 << 1;
const VIS_FLAG_HAS_BG_GRAD: u8 = 1 << 2;
const VIS_FLAG_BG_GRAD_HORIZ: u8 = 1 << 3;
const VIS_FLAG_ITEMS_BASELINE: u8 = 1 << 4;
const VIS_ALIGN_SHIFT: u8 = 5;
const VIS_ALIGN_IME_CARET: u8 = 7;
pub(super) const FOCUS_BORDER_NONE: u8 = u8::MAX;

#[inline(always)]
pub(super) const fn pack_visual_flags(
    has_pos: bool,
    has_scroll: bool,
    has_bg_grad: bool,
    bg_grad_horiz: bool,
    items_baseline: bool,
    align_self: Option<AlignItems>,
    is_ime_caret: bool,
) -> u8 {
    let mut f = 0u8;
    if has_pos {
        f |= VIS_FLAG_HAS_POS;
    }
    if has_scroll {
        f |= VIS_FLAG_HAS_SCROLL;
    }
    if has_bg_grad {
        f |= VIS_FLAG_HAS_BG_GRAD;
    }
    if bg_grad_horiz {
        f |= VIS_FLAG_BG_GRAD_HORIZ;
    }
    if items_baseline {
        f |= VIS_FLAG_ITEMS_BASELINE;
    }
    let align_code: u8 = if is_ime_caret {
        VIS_ALIGN_IME_CARET
    } else {
        match align_self {
            None => 0,
            Some(AlignItems::Stretch) => 1,
            Some(AlignItems::Center) => 2,
            Some(AlignItems::Start) => 3,
            Some(AlignItems::End) => 4,
        }
    };
    f | (align_code << VIS_ALIGN_SHIFT)
}

const STATE_FLAG_HAS_TEXT_COLOR: u8 = 1 << 0;

/// 按需分配在 [`crate::arena::ScratchArena`] 中的容器级文本继承与交互态样式覆写表（仅 **32 字节**，普通 `DivVisual` 零分配开销）。
#[derive(Copy, Clone, Debug, Default)]
pub struct DivStateStyles<'a> {
    pub hover_style: Option<&'a StyleRefinement>,
    pub active_style: Option<&'a StyleRefinement>,
    pub focus_style: Option<&'a StyleRefinement>,
    pub text_color: Color,
    pub text_size: u8,
    pub flags: u8,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<DivStateStyles<'static>>() == 32);

/// 冷路径二级扩展包（仅 16 字节，仅当节点同时拥有 `DivStateStyles` 与 `PaintEffects` 时按需分配）。
#[derive(Copy, Clone, Debug, Default)]
pub struct DivExtended<'a> {
    pub state_styles: Option<&'a DivStateStyles<'a>>,
    pub effects: Option<&'a PaintEffects<'a>>,
}

/// 8 字节紧凑 Tagged Pointer，直接嵌入 [`DivVisual`]：
/// - `0b00`: 无扩展（0 字节分配）；
/// - `0b01`: 直接指向 `&'a DivStateStyles<'a>`（免去 16B `DivExtended` 包装，护航 `no_std` 4.9KB `ScratchArena`）；
/// - `0b10`: 直接指向 `&'a PaintEffects<'a>`（免去 16B `DivExtended` 包装）；
/// - `0b11`: 同时存在两者时指向 `&'a DivExtended<'a>`。
#[derive(Copy, Clone)]
pub struct DivExtPtr<'a> {
    raw: *const u8,
    _marker: core::marker::PhantomData<&'a ()>,
}

// SAFETY: `DivExtPtr<'a>` 仅持有指向只读静态或 `'a` 帧内存的不可变指针。
unsafe impl<'a> Send for DivExtPtr<'a> {}
unsafe impl<'a> Sync for DivExtPtr<'a> {}

impl<'a> core::fmt::Debug for DivExtPtr<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DivExtPtr")
            .field("tag", &(self.raw.addr() & 0b11))
            .finish()
    }
}

const EXT_TAG_MASK: usize = 0b11;
const EXT_TAG_STATE_ONLY: usize = 0b01;
const EXT_TAG_EFFECTS_ONLY: usize = 0b10;
const EXT_TAG_BOTH: usize = 0b11;

const _: () = assert!(core::mem::align_of::<DivStateStyles<'static>>() >= 4);
const _: () = assert!(core::mem::align_of::<PaintEffects<'static>>() >= 4);
const _: () = assert!(core::mem::align_of::<DivExtended<'static>>() >= 4);

impl<'a> DivExtPtr<'a> {
    pub const NONE: Self = Self {
        raw: core::ptr::null(),
        _marker: core::marker::PhantomData,
    };

    #[inline(always)]
    pub fn from_state_styles(s: &'a DivStateStyles<'a>) -> Self {
        Self {
            raw: (s as *const DivStateStyles<'a> as *const u8).map_addr(|a| a | EXT_TAG_STATE_ONLY),
            _marker: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub fn from_effects(e: &'a PaintEffects<'a>) -> Self {
        Self {
            raw: (e as *const PaintEffects<'a> as *const u8).map_addr(|a| a | EXT_TAG_EFFECTS_ONLY),
            _marker: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub fn from_both(ext: &'a DivExtended<'a>) -> Self {
        Self {
            raw: (ext as *const DivExtended<'a> as *const u8).map_addr(|a| a | EXT_TAG_BOTH),
            _marker: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub fn state_styles(&self) -> Option<&'a DivStateStyles<'a>> {
        if self.raw.is_null() {
            return None;
        }
        let tag = self.raw.addr() & EXT_TAG_MASK;
        let base = self.raw.map_addr(|a| a & !EXT_TAG_MASK);
        match tag {
            EXT_TAG_STATE_ONLY => Some(unsafe { &*(base as *const DivStateStyles<'a>) }),
            EXT_TAG_BOTH => unsafe { &*(base as *const DivExtended<'a>) }.state_styles,
            _ => None,
        }
    }

    #[inline(always)]
    pub fn effects(&self) -> Option<&'a PaintEffects<'a>> {
        if self.raw.is_null() {
            return None;
        }
        let tag = self.raw.addr() & EXT_TAG_MASK;
        let base = self.raw.map_addr(|a| a & !EXT_TAG_MASK);
        match tag {
            EXT_TAG_EFFECTS_ONLY => Some(unsafe { &*(base as *const PaintEffects<'a>) }),
            EXT_TAG_BOTH => unsafe { &*(base as *const DivExtended<'a>) }.effects,
            _ => None,
        }
    }
}

/// 按需分配在 [`crate::arena::ScratchArena`] 中的扩展视觉与布局约束属性（严格紧凑 **64 字节**，零对齐空洞）。
#[derive(Copy, Clone, Debug)]
pub struct DivVisual<'a> {
    pub extended: DivExtPtr<'a>,
    pub transition: Option<&'a TransitionSpec>,
    pub margin: Edges<i16>,
    pub extra_padding: Edges<i16>,
    pub bg_grad_to: Color,
    pub focus_border_color: Color,
    pub shadow_color: Color,
    pub min_w: i16,
    pub max_w: i16,
    pub min_h: i16,
    pub max_h: i16,
    pub pos_x: i16,
    pub pos_y: i16,
    pub raw_scrollable_max: i16,
    pub extra_flags: u8,
    pub focus_border_w: u8,
    pub shadow_blur: u8,
    pub shadow_ox: i8,
    pub shadow_oy: i8,
    pub cursor: Option<CursorShape>,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<DivVisual<'static>>() == 64);

impl<'a> DivVisual<'a> {
    #[inline(always)]
    pub const fn pos(&self) -> Option<(i16, i16)> {
        if (self.extra_flags & VIS_FLAG_HAS_POS) != 0 {
            Some((self.pos_x, self.pos_y))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn scrollable_max(&self) -> Option<i16> {
        if (self.extra_flags & VIS_FLAG_HAS_SCROLL) != 0 {
            Some(self.raw_scrollable_max)
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn bg_grad(&self) -> Option<(Color, bool)> {
        if (self.extra_flags & VIS_FLAG_HAS_BG_GRAD) != 0 {
            Some((
                self.bg_grad_to,
                (self.extra_flags & VIS_FLAG_BG_GRAD_HORIZ) != 0,
            ))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn focus_border(&self) -> Option<(u8, Color)> {
        if self.focus_border_w != FOCUS_BORDER_NONE {
            Some((self.focus_border_w, self.focus_border_color))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn shadow(&self) -> Option<(u8, i8, i8, Color)> {
        if self.shadow_blur > 0 {
            Some((
                self.shadow_blur,
                self.shadow_ox,
                self.shadow_oy,
                self.shadow_color,
            ))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn items_baseline(&self) -> bool {
        (self.extra_flags & VIS_FLAG_ITEMS_BASELINE) != 0
    }

    #[inline(always)]
    pub const fn align_self(&self) -> Option<AlignItems> {
        match self.extra_flags >> VIS_ALIGN_SHIFT {
            1 => Some(AlignItems::Stretch),
            2 => Some(AlignItems::Center),
            3 => Some(AlignItems::Start),
            4 => Some(AlignItems::End),
            _ => None,
        }
    }

    /// 是否被标记为系统输入法（IME）文本光标锚点节点（零字节膨胀编码于 `extra_flags` 保留值）。
    #[inline(always)]
    pub const fn is_ime_caret(&self) -> bool {
        (self.extra_flags >> VIS_ALIGN_SHIFT) == VIS_ALIGN_IME_CARET
    }

    #[inline(always)]
    pub fn transition_spec(&self) -> Option<TransitionSpec> {
        self.transition.copied()
    }
}

static DEFAULT_DIV_VISUAL: DivVisual<'static> = DivVisual {
    extended: DivExtPtr::NONE,
    transition: None,
    margin: Edges::all(0),
    extra_padding: Edges::all(0),
    bg_grad_to: Color::TRANSPARENT,
    focus_border_color: Color::TRANSPARENT,
    shadow_color: Color::TRANSPARENT,
    min_w: 0,
    max_w: 0,
    min_h: 0,
    max_h: 0,
    pos_x: 0,
    pos_y: 0,
    raw_scrollable_max: 0,
    extra_flags: 0,
    focus_border_w: FOCUS_BORDER_NONE,
    shadow_blur: 0,
    shadow_ox: 0,
    shadow_oy: 0,
    cursor: None,
};

/// 仅禁用了默认焦点边框（`.no_focus_border()`）的零分配共享只读扩展节点。
pub(super) static NO_FOCUS_BORDER_DIV_VISUAL: DivVisual<'static> = DivVisual {
    extended: DivExtPtr::NONE,
    transition: None,
    margin: Edges::all(0),
    extra_padding: Edges::all(0),
    bg_grad_to: Color::TRANSPARENT,
    focus_border_color: Color::TRANSPARENT,
    shadow_color: Color::TRANSPARENT,
    min_w: 0,
    max_w: 0,
    min_h: 0,
    max_h: 0,
    pos_x: 0,
    pos_y: 0,
    raw_scrollable_max: 0,
    extra_flags: 0,
    focus_border_w: 0,
    shadow_blur: 0,
    shadow_ox: 0,
    shadow_oy: 0,
    cursor: None,
};

/// 按需分配在 [`crate::arena::ScratchArena`] 中的高级手势与键盘事件回调表（普通点击按钮零分配开销）。
#[derive(Copy, Clone)]
pub struct DivRareHandlers<'a> {
    pub on_long_press: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_drag: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_hover: Option<&'a dyn Fn(bool, &mut ActionScope)>,
    pub on_pointer_move: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_wheel: Option<&'a dyn Fn(crate::pod::WheelEvent, &mut ActionScope)>,
    pub on_pinch: Option<&'a dyn Fn(crate::pod::PinchEvent, &mut ActionScope)>,
    pub on_swipe: Option<&'a dyn Fn(crate::pod::SwipeDir, &mut ActionScope)>,
    pub on_key_input: Option<&'a dyn Fn(crate::pod::KeyCode, &mut ActionScope)>,
    pub on_key_nav: Option<&'a dyn Fn(crate::pod::KeyCode, &mut ActionScope)>,
    pub on_navigate_back: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_secondary_click: Option<&'a dyn Fn(&mut ActionScope)>,
}

/// 按需分配在 [`crate::arena::ScratchArena`] 中的紧凑交互入口（仅 24 Bytes）。
#[derive(Copy, Clone)]
pub struct DivInteractivity<'a> {
    pub on_click: Option<&'a dyn Fn(&mut ActionScope)>,
    pub rare: Option<&'a DivRareHandlers<'a>>,
}

/// 存储在 [`super::Element::Div`] 中的紧凑容器节点（固定 64 Bytes，将 `ElementNode` 压缩至 72B）。
pub struct DivNode<'a> {
    pub style: Style,
    pub first_child: Option<&'a ElementNode<'a>>,
    pub visual: Option<&'a DivVisual<'a>>,
    pub interact: Option<&'a DivInteractivity<'a>>,
    pub raw_id: u32,
    pub raw_active_bg: Color,
    pub raw_hover_bg: Color,
    pub opacity: u8,
    pub z_index: i8,
    pub grid_cols: u8,
    pub flags: u8,
}

impl<'a> core::ops::Deref for DivNode<'a> {
    type Target = DivVisual<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.visual.unwrap_or(&DEFAULT_DIV_VISUAL)
    }
}

impl<'a> DivNode<'a> {
    #[inline(always)]
    pub fn state_styles(&self) -> Option<&'a DivStateStyles<'a>> {
        self.extended.state_styles()
    }

    #[inline(always)]
    pub fn effects(&self) -> Option<&'a PaintEffects<'a>> {
        self.extended.effects()
    }

    #[inline(always)]
    pub fn layout_ext(&self) -> Option<&'a crate::paint::LayoutExtras<'a>> {
        self.extended.effects().and_then(|e| e.layout_ext)
    }

    #[inline(always)]
    pub fn text_color(&self) -> Option<Color> {
        self.state_styles().and_then(|s| {
            if (s.flags & STATE_FLAG_HAS_TEXT_COLOR) != 0 {
                Some(s.text_color)
            } else {
                None
            }
        })
    }

    #[inline(always)]
    pub fn text_size(&self) -> Option<u8> {
        self.state_styles()
            .and_then(|s| if s.text_size > 0 { Some(s.text_size) } else { None })
    }

    #[inline(always)]
    pub fn hover_style(&self) -> Option<StyleRefinement> {
        self.state_styles().and_then(|s| s.hover_style.copied())
    }

    #[inline(always)]
    pub fn active_style(&self) -> Option<StyleRefinement> {
        self.state_styles().and_then(|s| s.active_style.copied())
    }

    #[inline(always)]
    pub fn focus_style(&self) -> Option<StyleRefinement> {
        self.state_styles().and_then(|s| s.focus_style.copied())
    }

    #[inline(always)]
    pub const fn id(&self) -> Option<u32> {
        if (self.flags & (1 << 5)) != 0 {
            Some(self.raw_id)
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn active_bg(&self) -> Option<Color> {
        if (self.flags & (1 << 6)) != 0 {
            Some(self.raw_active_bg)
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn hover_bg(&self) -> Option<Color> {
        if (self.flags & (1 << 7)) != 0 {
            Some(self.raw_hover_bg)
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn focusable(&self) -> bool {
        (self.flags & (1 << 0)) != 0
    }

    #[inline(always)]
    pub const fn is_overlay(&self) -> bool {
        (self.flags & (1 << 1)) != 0
    }

    #[inline(always)]
    pub const fn occlude_hit(&self) -> bool {
        (self.flags & (1 << 2)) != 0
    }

    #[inline(always)]
    pub const fn auto_repeat(&self) -> bool {
        (self.flags & (1 << 3)) != 0
    }

    #[inline(always)]
    pub const fn clip_children(&self) -> bool {
        (self.flags & (1 << 4)) != 0
    }

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

/// 将 `Duration` 饱和转换为 `u16` 毫秒（上限约 65 秒）。
#[inline(always)]
pub(super) fn duration_to_ms(d: Duration) -> u16 {
    d.as_millis().min(u16::MAX as u128) as u16
}

/// 元素首次出现（挂载）时的入场起始状态（从该状态平滑过渡到布局目标状态）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct EnterFrom {
    /// 入场起始不透明度（`0` 为完全透明淡入）。
    pub opacity: u8,
    /// 入场起始水平偏移（像素，相对布局目标位置）。
    pub dx: i16,
    /// 入场起始垂直偏移（像素，相对布局目标位置）。
    pub dy: i16,
}

/// 元素从视图树移除（卸载）时的幽灵退场目标状态（自动保留上一帧 `DrawOp` 快照继续插值至目标状态后回收）。
pub type ExitTo = EnterFrom;

/// 声明式过渡动画属性配置（分配在 `ScratchArena` 冷路径，对常规 `DivNode`/`DivVisual` 零体积膨胀）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TransitionSpec {
    /// 过渡动画时长（毫秒）。
    pub duration_ms: u16,
    /// 独立的幽灵退场动画时长（毫秒；`0` 表示沿用 `duration_ms`，确保同时配置入场与退场时互不覆盖时长）。
    pub exit_duration_ms: u16,
    /// 动画开始前的延迟（毫秒，对入场与每次属性变更均生效；循环动画仅首轮生效）。
    pub delay_ms: u16,
    /// 容器级子项交错延迟步长（毫秒，`child_index * stagger_ms` 自动叠加到子节点动画延迟，借鉴 Anime.js `stagger`）。
    pub stagger_ms: u16,
    /// 是否插值背景色与边框/文本颜色。
    pub animate_color: bool,
    /// 是否插值几何位置与尺寸（子树随容器位移同步平移）。
    pub animate_bounds: bool,
    /// 是否插值容器整体不透明度（`.opacity(...)` 变化时淡入淡出，作用于整个子树）。
    pub animate_opacity: bool,
    /// 容器级零配置自动动画（借鉴 AutoAnimate：自动赋予直接子节点重排 FLIP、淡入与幽灵快照退场）。
    pub auto_animate: bool,
    /// 是否将 FLIP 几何缩放与位移下沉到合成层 `Transform2D` 并自动逆缩放修正圆角（避免卡片缩放时多行文本反复折行抖动）。
    pub flip_transform: bool,
    /// 缓动曲线类型（`Easing::Spring` 在 `LoopMode::Once` 下自动启用定点数二阶速度继承弹簧）。
    pub easing: Easing,
    /// 循环模式（单次、无限循环、往复呼吸）。
    pub loop_mode: LoopMode,
    /// 往复呼吸/循环动画的起始背景色（若设置则自动在 `pulse_from_bg` 与 `style.bg` 之间循环插值）。
    pub pulse_from_bg: Option<Color>,
    /// 入场动画起始状态（元素首次出现时从该状态过渡至目标状态）。
    pub enter: Option<EnterFrom>,
    /// 退场动画目标状态（元素从树中移除时自动保留幽灵快照过渡至此状态后回收）。
    pub exit: Option<ExitTo>,
}

impl Default for TransitionSpec {
    #[inline(always)]
    fn default() -> Self {
        Self {
            duration_ms: 180,
            exit_duration_ms: 0,
            delay_ms: 0,
            stagger_ms: 0,
            animate_color: false,
            animate_bounds: false,
            animate_opacity: false,
            auto_animate: false,
            flip_transform: false,
            easing: Easing::EaseOut,
            loop_mode: LoopMode::Once,
            pulse_from_bg: None,
            enter: None,
            exit: None,
        }
    }
}

