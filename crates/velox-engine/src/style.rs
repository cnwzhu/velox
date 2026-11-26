//! 24 字节紧凑样式结构体 `Style` 与 Tailwind 风格链式修饰符 `Styled`。

/// ARGB8888 32 位颜色表示（`0xAARRGGBB`）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Color(pub u32);

impl Color {
    /// 完全透明色。
    pub const TRANSPARENT: Self = Self(0x0000_0000);
    /// 纯白色。
    pub const WHITE: Self = Self(0xFFFF_FFFF);
    /// 纯黑色。
    pub const BLACK: Self = Self(0xFF00_0000);

    /// 从不透明 RGB 分量构造颜色。
    #[inline(always)]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// 从 RGBA 分量构造颜色。
    #[inline(always)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    /// 从 24 位十六进制 RGB 整数构造不透明颜色（如 `Color::hex(0xd4d4d4)`）。
    #[inline(always)]
    pub const fn hex(rgb: u32) -> Self {
        Self(0xFF00_0000 | (rgb & 0x00FF_FFFF))
    }

    /// 从 32 位十六进制 `0xRRGGBBAA` 整数构造颜色（如 `Color::hex_alpha(0x171717cc)`）。
    #[inline(always)]
    pub const fn hex_alpha(rgba: u32) -> Self {
        let r = ((rgba >> 24) & 0xFF) as u8;
        let g = ((rgba >> 16) & 0xFF) as u8;
        let b = ((rgba >> 8) & 0xFF) as u8;
        let a = (rgba & 0xFF) as u8;
        Self::rgba(r, g, b, a)
    }

    /// Alpha 通道（`0..=255`）。
    #[inline(always)]
    pub const fn a(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    /// Red 通道（`0..=255`）。
    #[inline(always)]
    pub const fn r(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    /// Green 通道（`0..=255`）。
    #[inline(always)]
    pub const fn g(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// Blue 通道（`0..=255`）。
    #[inline(always)]
    pub const fn b(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// 定点数颜色线性插值（`t_256` 取值范围 `0..=256`）。
    ///
    /// 正确处理透明色与半透明色（预乘 Alpha 权重插值）：
    /// 当任一端为完全透明（`a == 0`，如 `Color::TRANSPARENT`）时保留另一端的 RGB 色相、仅插值 Alpha，
    /// 彻底消除 `Outline` / `Ghost` 等透明底按钮在亮色/暗色主题下悬停过渡时的黑灰闪烁。
    #[inline]
    pub const fn lerp(self, other: Self, t_256: u16) -> Self {
        if t_256 == 0 {
            return self;
        }
        if t_256 >= 256 {
            return other;
        }
        let t = t_256 as u32;
        let inv = 256 - t;
        let sa = self.a() as u32;
        let oa = other.a() as u32;
        let a = ((sa * inv + oa * t + 128) >> 8) as u8;
        if a == 0 {
            return Self::TRANSPARENT;
        }
        if sa == 0 {
            return Self::rgba(other.r(), other.g(), other.b(), a);
        }
        if oa == 0 {
            return Self::rgba(self.r(), self.g(), self.b(), a);
        }
        if sa == 255 && oa == 255 {
            let r = ((self.r() as u32 * inv + other.r() as u32 * t + 128) >> 8) as u8;
            let g = ((self.g() as u32 * inv + other.g() as u32 * t + 128) >> 8) as u8;
            let b = ((self.b() as u32 * inv + other.b() as u32 * t + 128) >> 8) as u8;
            return Self::rgba(r, g, b, 255);
        }
        // 半透明预乘加权插值再还原直通 RGB，避免低 Alpha 端暗色拉黑
        let w0 = sa * inv;
        let w1 = oa * t;
        let denom = w0 + w1;
        if denom == 0 {
            return Self::TRANSPARENT;
        }
        let half = denom >> 1;
        let r = ((self.r() as u32 * w0 + other.r() as u32 * w1 + half) / denom) as u8;
        let g = ((self.g() as u32 * w0 + other.g() as u32 * w1 + half) / denom) as u8;
        let b = ((self.b() as u32 * w0 + other.b() as u32 * w1 + half) / denom) as u8;
        Self::rgba(r, g, b, a)
    }

    /// 按给定不透明度 `opacity`（`0..=255`）调制当前颜色的 Alpha 通道。
    #[inline(always)]
    pub const fn scale_alpha(self, opacity: u8) -> Self {
        if opacity == 255 {
            return self;
        }
        if opacity == 0 {
            return Self::TRANSPARENT;
        }
        let a = ((self.a() as u16 * opacity as u16) / 255) as u8;
        Self::rgba(self.r(), self.g(), self.b(), a)
    }
}

/// 动画缓动曲线类型。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Easing {
    /// 线性匀速插值。
    Linear = 0,
    /// 三次方渐入（加速）。
    EaseIn = 1,
    /// 三次方渐出（减速，默认）。
    #[default]
    EaseOut = 2,
    /// 三次方渐入渐出（S 型曲线）。
    EaseInOut = 3,
    /// 欠阻尼弹簧过冲曲线（带约 8% 物理回弹质感）。
    Spring = 4,
    /// 回拉过冲渐出（`easeOutBack`，末端约 10% 过冲后回落，适合弹窗/卡片入场）。
    EaseOutBack = 5,
    /// 落地弹跳渐出（`easeOutBounce`，模拟小球落地多次弹跳，结果不超出 `0..=256`）。
    Bounce = 6,
    /// 弹性振荡渐出（`easeOutElastic`，指数衰减正弦振荡，适合提醒/抖动效果）。
    Elastic = 7,
}

impl Easing {
    /// 从低 3 位编码还原缓动曲线（与 `Easing as u8` 互逆）。
    #[inline(always)]
    pub const fn from_bits(bits: u8) -> Self {
        match bits & 0x07 {
            0 => Self::Linear,
            1 => Self::EaseIn,
            2 => Self::EaseOut,
            3 => Self::EaseInOut,
            4 => Self::Spring,
            5 => Self::EaseOutBack,
            6 => Self::Bounce,
            _ => Self::Elastic,
        }
    }

    /// 纯整数定点缓动求值：输入归一化进度 `t_256`（`0..=256`），输出 `0..=256`
    /// （`Spring` / `EaseOutBack` / `Elastic` 允许短暂过冲超出 `256` 或低于 `0`）。
    #[inline]
    pub fn apply_256(self, t_256: i32) -> i32 {
        let t = t_256.clamp(0, 256);
        if t >= 256 {
            return 256;
        }
        match self {
            Self::Linear => t,
            Self::EaseIn => (t * t * t) >> 16,
            Self::EaseOut => {
                let inv = 256 - t;
                256 - ((inv * inv * inv) >> 16)
            }
            Self::EaseInOut => {
                if t < 128 {
                    (4 * t * t * t) >> 16
                } else {
                    let inv = 256 - t;
                    256 - ((4 * inv * inv * inv) >> 16)
                }
            }
            Self::Spring => {
                // 二阶欠阻尼弹簧拟合：快速上升并在 t ≈ 180..200 时产生约 +20..26/256 (8%~10%) 的过冲随后收敛至 256
                let inv = 256 - t;
                let base = 256 - ((inv * inv) >> 8);
                let overshoot = ((9 * (t as i64) * (t as i64) * (inv as i64)) >> 19) as i32;
                base + overshoot
            }
            Self::EaseOutBack => {
                // 1 + c3·(t-1)³ + c1·(t-1)²，c1 ≈ 1.70158 (436/256)，c3 ≈ 2.70158 (692/256)
                let u = (t - 256) as i64;
                let cubic = (692 * u * u * u) >> 24;
                let quad = (436 * u * u) >> 16;
                (256 + cubic + quad) as i32
            }
            Self::Bounce => {
                // 标准 easeOutBounce 分段抛物线（n1 = 7.5625 = 121/16，d1 = 2.75）
                let seg = |x: i32, base: i32| ((121 * x * x) >> 12) + base;
                let v = if t < 93 {
                    seg(t, 0)
                } else if t < 186 {
                    seg(t - 140, 192)
                } else if t < 233 {
                    seg(t - 209, 240)
                } else {
                    seg(t - 244, 252)
                };
                v.min(256)
            }
            Self::Elastic => {
                // 2^(-10t) · sin((10t - 0.75) · 2π/3) + 1
                let e = 10 * t; // 以 1/256 为单位的指数
                let k = (e >> 8) as u32;
                let frac = e & 0xFF;
                // 2^-k 与 2^-(k+1) 之间线性插值近似 2^-(k + frac)
                let hi = 256i32 >> k.min(16);
                let decay = hi - ((hi >> 1) * frac >> 8);
                let deg10 = (t * 12000) / 256 - 900;
                let (sin_q10, _) = crate::sdf::sin_cos_deg10(deg10);
                256 + ((decay * sin_q10) >> 10)
            }
        }
    }

    /// 按已流逝时间 `elapsed_ms` / 总时长 `duration_ms` 求值缓动输出（`0..=256` 定点数）。
    #[inline]
    pub fn sample_256(self, elapsed_ms: u32, duration_ms: u32) -> i32 {
        if duration_ms == 0 || elapsed_ms >= duration_ms {
            return 256;
        }
        self.apply_256(((elapsed_ms as u64 * 256) / duration_ms as u64) as i32)
    }
}

/// 动画循环模式。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum LoopMode {
    /// 单次过渡动画（默认）。
    #[default]
    Once = 0,
    /// 无限单向循环（`0 -> 1 -> 0 -> 1`）。
    Infinite = 1,
    /// 无限往复呼吸循环（`0 -> 1 -> 0`）。
    PingPong = 2,
}

/// 文本水平对齐方式。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum TextAlign {
    /// 左对齐（默认）。
    #[default]
    Left = 0,
    /// 水平居中对齐。
    Center = 1,
    /// 右对齐。
    Right = 2,
}

/// 尺寸长度单位（严格 4 字节表示）。
#[repr(C, i16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Length {
    /// 由内容自适应决定尺寸。
    #[default]
    Auto = 0,
    /// 固定像素长度。
    Px(i16) = 1,
    /// 撑满父容器可用空间（100%）。
    Full = 2,
    /// 按父容器可用空间的百分比（`0..=100`）计算长度。
    Pct(i16) = 3,
}

/// 四边边距（4 字节当 `T = i8`）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Edges<T> {
    /// 四边统一边距。
    #[inline(always)]
    pub const fn all(val: T) -> Self {
        Self {
            top: val,
            right: val,
            bottom: val,
            left: val,
        }
    }

    /// 水平与垂直对称边距。
    #[inline(always)]
    pub const fn xy(x: T, y: T) -> Self {
        Self {
            top: y,
            right: x,
            bottom: y,
            left: x,
        }
    }
}

/// 主轴排列方向（1 bit）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FlexDir {
    Row = 0,
    Col = 1,
}

/// 主轴对齐方式（3 bits）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Justify {
    Start = 0,
    Center = 1,
    End = 2,
    Between = 3,
    Around = 4,
}

/// 交叉轴对齐方式（2 bits，默认 `0 = Stretch` 遵循 CSS Flexbox 规范）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AlignItems {
    Stretch = 0,
    Center = 1,
    Start = 2,
    End = 3,
}

/// 紧凑位域样式结构体（严格 **24 字节**）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Style {
    /// 宽度策略（4 Bytes）。
    pub width: Length,
    /// 高度策略（4 Bytes）。
    pub height: Length,
    /// 四周内边距（4 Bytes）。
    pub padding: Edges<i8>,
    /// 背景填充色（4 Bytes）。
    pub bg: Color,
    /// 边框颜色（4 Bytes）。
    pub border_color: Color,
    /// 子节点间距（1 Byte）。
    pub gap: i8,
    /// 圆角半径 `0..=127px`（1 Byte）。
    pub rounded: u8,
    /// 边框宽度（1 Byte）。
    pub border_width: u8,
    /// 位域：`flex_dir(1b) | justify(3b) | items_align(2b) | flex_grow(2b)`（1 Byte）。
    pub flex_flags: u8,
}

const _: () = assert!(core::mem::size_of::<Style>() == 24);

impl Default for Style {
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Style {
    /// 全零默认值样式常量（供所有组件 `const fn new` 零成本初始化）。
    pub const DEFAULT: Self = Self::new();

    /// 构造全默认样式。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            padding: Edges::all(0),
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            gap: 0,
            rounded: 0,
            border_width: 0,
            flex_flags: 0,
        }
    }

    /// 设置水平主轴 `Row`。
    #[inline(always)]
    pub fn set_row(&mut self) {
        self.flex_flags &= !0b0000_0001;
    }

    /// 设置垂直主轴 `Col`。
    #[inline(always)]
    pub fn set_col(&mut self) {
        self.flex_flags |= 0b0000_0001;
    }

    /// 读取主轴方向。
    #[inline(always)]
    pub const fn flex_dir(&self) -> FlexDir {
        if (self.flex_flags & 0b0000_0001) != 0 {
            FlexDir::Col
        } else {
            FlexDir::Row
        }
    }

    /// 设置主轴对齐方式。
    #[inline(always)]
    pub fn set_justify(&mut self, justify: Justify) {
        self.flex_flags = (self.flex_flags & !0b0000_1110) | ((justify as u8 & 0b111) << 1);
    }

    /// 设置主轴居中对齐。
    #[inline(always)]
    pub fn set_justify_center(&mut self) {
        self.set_justify(Justify::Center);
    }

    /// 设置主轴两端对齐。
    #[inline(always)]
    pub fn set_justify_between(&mut self) {
        self.set_justify(Justify::Between);
    }

    /// 读取主轴对齐方式。
    #[inline(always)]
    pub const fn justify(&self) -> Justify {
        match (self.flex_flags >> 1) & 0b111 {
            1 => Justify::Center,
            2 => Justify::End,
            3 => Justify::Between,
            4 => Justify::Around,
            _ => Justify::Start,
        }
    }

    /// 设置交叉轴对齐方式。
    #[inline(always)]
    pub fn set_items_align(&mut self, align: AlignItems) {
        self.flex_flags = (self.flex_flags & !0b0011_0000) | ((align as u8 & 0b11) << 4);
    }

    /// 设置交叉轴居中对齐。
    #[inline(always)]
    pub fn set_items_center(&mut self) {
        self.set_items_align(AlignItems::Center);
    }

    /// 读取交叉轴对齐方式。
    #[inline(always)]
    pub const fn items_align(&self) -> AlignItems {
        match (self.flex_flags >> 4) & 0b11 {
            1 => AlignItems::Center,
            2 => AlignItems::Start,
            3 => AlignItems::End,
            _ => AlignItems::Stretch,
        }
    }

    /// 设置弹性拉伸权重（`0..=3`）。
    #[inline(always)]
    pub fn set_grow(&mut self, weight: u8) {
        let w = if weight > 3 { 3 } else { weight };
        self.flex_flags = (self.flex_flags & !0b1100_0000) | (w << 6);
    }

    /// 读取弹性拉伸权重（`0..=3`）。
    #[inline(always)]
    pub const fn flex_grow(&self) -> u8 {
        (self.flex_flags >> 6) & 0b11
    }

    /// 读取圆角半径（`0..=127px`，剥离最高位 `flex_wrap` 标志）。
    #[inline(always)]
    pub const fn rounded_radius(&self) -> u8 {
        self.rounded & 0x7F
    }

    /// 设置圆角半径（`0..=127px`，保留最高位 `flex_wrap` 标志）。
    #[inline(always)]
    pub fn set_rounded(&mut self, radius: u8) {
        self.rounded = (self.rounded & 0x80) | (radius.min(127) & 0x7F);
    }

    /// 是否启用多行自动折行（`flex_wrap`，存储在 `rounded` 的第 7 位）。
    #[inline(always)]
    pub const fn flex_wrap(&self) -> bool {
        (self.rounded & 0x80) != 0
    }

    /// 设置多行自动折行（`flex_wrap`）。
    #[inline(always)]
    pub fn set_flex_wrap(&mut self, wrap: bool) {
        if wrap {
            self.rounded |= 0x80;
        } else {
            self.rounded &= 0x7F;
        }
    }

    /// 读取边框宽度（`0..=15px`，存储在 `border_width` 低 4 位）。
    #[inline(always)]
    pub const fn border_px(&self) -> u8 {
        self.border_width & 0x0F
    }

    /// 设置边框宽度（`0..=15px`）。
    #[inline(always)]
    pub fn set_border_px(&mut self, width: u8) {
        self.border_width = (self.border_width & 0xF0) | (width.min(15) & 0x0F);
    }

    /// 读取四边统一外边距（`0..=15px`，存储在 `border_width` 高 4 位）。
    #[inline(always)]
    pub const fn margin_px(&self) -> i8 {
        ((self.border_width >> 4) & 0x0F) as i8
    }

    /// 设置四边统一外边距（`0..=15px`）。
    #[inline(always)]
    pub fn set_margin_px(&mut self, px: i8) {
        let m = px.clamp(0, 15) as u8;
        self.border_width = (self.border_width & 0x0F) | (m << 4);
    }
}

/// GPUI 风格像素单位辅助函数（四舍五入转换为 `i16`，兼容 `.w(px(250.))` 与 `.w(250)`）。
#[inline(always)]
pub const fn px(val: f32) -> i16 {
    if val >= 0.0 {
        (val + 0.5) as i16
    } else {
        (val - 0.5) as i16
    }
}

/// GPUI 风格 24 位十六进制 RGB 颜色构造函数（如 `rgb(0xd4d4d4)`）。
#[inline(always)]
pub const fn rgb(hex: u32) -> Color {
    Color::hex(hex)
}

/// GPUI 风格 32 位十六进制 `0xRRGGBBAA` 颜色构造函数（如 `rgba(0x171717cc)`）。
#[inline(always)]
pub const fn rgba(hex: u32) -> Color {
    Color::hex_alpha(hex)
}

/// GPUI 风格条件链式构建 Trait（对所有 `Sized` 类型自动实现，零运行时开销）。
///
/// 支持在任意 `Div`、`Text`、组件或 `StyleRefinement` 上直接使用 `.when(...)` / `.when_some(...)`：
/// ```ignore
/// item.size_8()
///     .when(state.is_active(), |s| s.bg(rgb(0x171717)).text_color(rgb(0xffffff)))
///     .when(!state.is_disabled(), |s| s.hover(|h| h.bg(rgb(0xf5f5f5))))
/// ```
pub trait FluentBuilder: Sized {
    /// 当 `condition` 为 `true` 时执行闭包修饰自身，否则原样返回。
    #[inline(always)]
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if condition {
            then(self)
        } else {
            self
        }
    }

    /// 根据 `condition` 在两个修饰分支之间二选一。
    #[inline(always)]
    fn when_else(
        self,
        condition: bool,
        then: impl FnOnce(Self) -> Self,
        else_fn: impl FnOnce(Self) -> Self,
    ) -> Self {
        if condition {
            then(self)
        } else {
            else_fn(self)
        }
    }

    /// 当 `option` 为 `Some(value)` 时执行闭包修饰自身，否则原样返回。
    #[inline(always)]
    fn when_some<O>(self, option: Option<O>, then: impl FnOnce(Self, O) -> Self) -> Self {
        match option {
            Some(value) => then(self, value),
            None => self,
        }
    }

    /// 当 `option` 为 `None` 时执行闭包修饰自身，否则原样返回。
    #[inline(always)]
    fn when_none<O>(self, option: &Option<O>, then: impl FnOnce(Self) -> Self) -> Self {
        if option.is_none() {
            then(self)
        } else {
            self
        }
    }

    /// 通用管道映射变换（避免与 `Iterator::map` / `Option::map` 重名）。
    #[inline(always)]
    fn apply<U>(self, f: impl FnOnce(Self) -> U) -> U {
        f(self)
    }
}

impl<T: Sized> FluentBuilder for T {}

/// 交互态样式覆写增量（供 `Div::hover(|s| ...)`、`Div::active(|s| ...)`、`Div::focus(|s| ...)` 使用，零堆分配）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct StyleRefinement {
    pub bg: Option<Color>,
    pub border_width: Option<u8>,
    pub border_color: Option<Color>,
    pub text_color: Option<Color>,
    pub opacity: Option<u8>,
    pub rounded: Option<u8>,
}

impl StyleRefinement {
    /// 创建空的交互态样式增量。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            bg: None,
            border_width: None,
            border_color: None,
            text_color: None,
            opacity: None,
            rounded: None,
        }
    }

    /// 是否未配置任何视觉覆写。
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.bg.is_none()
            && self.border_width.is_none()
            && self.border_color.is_none()
            && self.text_color.is_none()
            && self.opacity.is_none()
            && self.rounded.is_none()
    }

    /// 是否仅覆写了背景色（可直接走 `DivNode` 内联零分配快速路径）。
    #[inline(always)]
    pub const fn is_bg_only(&self) -> bool {
        self.bg.is_some()
            && self.border_width.is_none()
            && self.border_color.is_none()
            && self.text_color.is_none()
            && self.opacity.is_none()
            && self.rounded.is_none()
    }

    #[inline(always)]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    #[inline(always)]
    pub const fn border(mut self, width: u8, color: Color) -> Self {
        self.border_width = Some(width);
        self.border_color = Some(color);
        self
    }

    #[inline(always)]
    pub const fn border_0(mut self) -> Self {
        self.border_width = Some(0);
        self
    }

    #[inline(always)]
    pub const fn border_1(mut self) -> Self {
        self.border_width = Some(1);
        self
    }

    #[inline(always)]
    pub const fn border_2(mut self) -> Self {
        self.border_width = Some(2);
        self
    }

    #[inline(always)]
    pub const fn border_4(mut self) -> Self {
        self.border_width = Some(4);
        self
    }

    #[inline(always)]
    pub const fn border_color(mut self, color: Color) -> Self {
        self.border_color = Some(color);
        self
    }

    #[inline(always)]
    pub const fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    #[inline(always)]
    pub const fn opacity(mut self, alpha: u8) -> Self {
        self.opacity = Some(alpha);
        self
    }

    #[inline(always)]
    pub const fn opacity_f(mut self, alpha: f32) -> Self {
        let clamped = if alpha <= 0.0 {
            0.0
        } else if alpha >= 1.0 {
            1.0
        } else {
            alpha
        };
        self.opacity = Some((clamped * 255.0 + 0.5) as u8);
        self
    }

    #[inline(always)]
    pub const fn rounded(mut self, radius: u8) -> Self {
        self.rounded = Some(radius);
        self
    }

    #[inline(always)]
    pub const fn rounded_none(self) -> Self {
        self.rounded(0)
    }

    #[inline(always)]
    pub const fn rounded_sm(self) -> Self {
        self.rounded(4)
    }

    #[inline(always)]
    pub const fn rounded_md(self) -> Self {
        self.rounded(6)
    }

    #[inline(always)]
    pub const fn rounded_lg(self) -> Self {
        self.rounded(8)
    }

    #[inline(always)]
    pub const fn rounded_xl(self) -> Self {
        self.rounded(12)
    }

    #[inline(always)]
    pub const fn rounded_full(self) -> Self {
        self.rounded(127)
    }
}

/// 统一的 Tailwind 风格链式修饰符 Trait，`Div` 与自定义组件均可实现。
pub trait Styled: Sized {
    /// 获取内部 `Style` 的可变引用。
    fn style_mut(&mut self) -> &mut Style;

    /// 开启弹性布局（等价 CSS `display: flex`，默认水平主轴 `Row`）。
    #[inline(always)]
    fn flex(mut self) -> Self {
        self.style_mut().set_row();
        self
    }

    #[inline(always)]
    fn flex_row(mut self) -> Self {
        self.style_mut().set_row();
        self
    }

    #[inline(always)]
    fn flex_col(mut self) -> Self {
        self.style_mut().set_col();
        self
    }

    #[inline(always)]
    fn flex_wrap(mut self) -> Self {
        self.style_mut().set_flex_wrap(true);
        self
    }

    #[inline(always)]
    fn flex_nowrap(mut self) -> Self {
        self.style_mut().set_flex_wrap(false);
        self
    }

    #[inline(always)]
    fn flex_1(mut self) -> Self {
        self.style_mut().set_grow(1);
        self
    }

    #[inline(always)]
    fn flex_none(mut self) -> Self {
        self.style_mut().set_grow(0);
        self
    }

    #[inline(always)]
    fn size_full(mut self) -> Self {
        self.style_mut().width = Length::Full;
        self.style_mut().height = Length::Full;
        self
    }

    /// 同时设置等宽高正方形尺寸（像素）。
    #[inline(always)]
    fn size(mut self, px: i16) -> Self {
        let s = self.style_mut();
        s.width = Length::Px(px);
        s.height = Length::Px(px);
        self
    }

    #[inline(always)]
    fn w_full(mut self) -> Self {
        self.style_mut().width = Length::Full;
        self
    }

    #[inline(always)]
    fn h_full(mut self) -> Self {
        self.style_mut().height = Length::Full;
        self
    }

    #[inline(always)]
    fn w_auto(mut self) -> Self {
        self.style_mut().width = Length::Auto;
        self
    }

    #[inline(always)]
    fn h_auto(mut self) -> Self {
        self.style_mut().height = Length::Auto;
        self
    }

    #[inline(always)]
    fn w(mut self, px: i16) -> Self {
        self.style_mut().width = Length::Px(px);
        self
    }

    #[inline(always)]
    fn h(mut self, px: i16) -> Self {
        self.style_mut().height = Length::Px(px);
        self
    }

    /// 设置宽度占父容器可用宽度的百分比（`0..=100`）。
    #[inline(always)]
    fn w_pct(mut self, pct: i16) -> Self {
        self.style_mut().width = Length::Pct(pct.clamp(0, 100));
        self
    }

    /// 设置高度占父容器可用高度的百分比（`0..=100`）。
    #[inline(always)]
    fn h_pct(mut self, pct: i16) -> Self {
        self.style_mut().height = Length::Pct(pct.clamp(0, 100));
        self
    }

    #[inline(always)]
    fn w_1_2(self) -> Self { self.w_pct(50) }
    #[inline(always)]
    fn w_1_3(self) -> Self { self.w_pct(33) }
    #[inline(always)]
    fn w_2_3(self) -> Self { self.w_pct(67) }
    #[inline(always)]
    fn w_1_4(self) -> Self { self.w_pct(25) }
    #[inline(always)]
    fn w_3_4(self) -> Self { self.w_pct(75) }
    #[inline(always)]
    fn h_1_2(self) -> Self { self.h_pct(50) }

    // --- Tailwind 尺寸档位简写（1 unit = 4px） ---
    #[inline(always)] fn size_0(self) -> Self { self.size(0) }
    #[inline(always)] fn size_0_5(self) -> Self { self.size(2) }
    #[inline(always)] fn size_1(self) -> Self { self.size(4) }
    #[inline(always)] fn size_1_5(self) -> Self { self.size(6) }
    #[inline(always)] fn size_2(self) -> Self { self.size(8) }
    #[inline(always)] fn size_2_5(self) -> Self { self.size(10) }
    #[inline(always)] fn size_3(self) -> Self { self.size(12) }
    #[inline(always)] fn size_3_5(self) -> Self { self.size(14) }
    #[inline(always)] fn size_4(self) -> Self { self.size(16) }
    #[inline(always)] fn size_5(self) -> Self { self.size(20) }
    #[inline(always)] fn size_6(self) -> Self { self.size(24) }
    #[inline(always)] fn size_7(self) -> Self { self.size(28) }
    #[inline(always)] fn size_8(self) -> Self { self.size(32) }
    #[inline(always)] fn size_9(self) -> Self { self.size(36) }
    #[inline(always)] fn size_10(self) -> Self { self.size(40) }
    #[inline(always)] fn size_11(self) -> Self { self.size(44) }
    #[inline(always)] fn size_12(self) -> Self { self.size(48) }
    #[inline(always)] fn size_14(self) -> Self { self.size(56) }
    #[inline(always)] fn size_16(self) -> Self { self.size(64) }
    #[inline(always)] fn size_20(self) -> Self { self.size(80) }
    #[inline(always)] fn size_24(self) -> Self { self.size(96) }
    #[inline(always)] fn size_32(self) -> Self { self.size(128) }

    #[inline(always)] fn w_0(self) -> Self { self.w(0) }
    #[inline(always)] fn w_0_5(self) -> Self { self.w(2) }
    #[inline(always)] fn w_1(self) -> Self { self.w(4) }
    #[inline(always)] fn w_1_5(self) -> Self { self.w(6) }
    #[inline(always)] fn w_2(self) -> Self { self.w(8) }
    #[inline(always)] fn w_2_5(self) -> Self { self.w(10) }
    #[inline(always)] fn w_3(self) -> Self { self.w(12) }
    #[inline(always)] fn w_3_5(self) -> Self { self.w(14) }
    #[inline(always)] fn w_4(self) -> Self { self.w(16) }
    #[inline(always)] fn w_5(self) -> Self { self.w(20) }
    #[inline(always)] fn w_6(self) -> Self { self.w(24) }
    #[inline(always)] fn w_7(self) -> Self { self.w(28) }
    #[inline(always)] fn w_8(self) -> Self { self.w(32) }
    #[inline(always)] fn w_9(self) -> Self { self.w(36) }
    #[inline(always)] fn w_10(self) -> Self { self.w(40) }
    #[inline(always)] fn w_11(self) -> Self { self.w(44) }
    #[inline(always)] fn w_12(self) -> Self { self.w(48) }
    #[inline(always)] fn w_14(self) -> Self { self.w(56) }
    #[inline(always)] fn w_16(self) -> Self { self.w(64) }
    #[inline(always)] fn w_20(self) -> Self { self.w(80) }
    #[inline(always)] fn w_24(self) -> Self { self.w(96) }
    #[inline(always)] fn w_28(self) -> Self { self.w(112) }
    #[inline(always)] fn w_32(self) -> Self { self.w(128) }
    #[inline(always)] fn w_36(self) -> Self { self.w(144) }
    #[inline(always)] fn w_40(self) -> Self { self.w(160) }
    #[inline(always)] fn w_44(self) -> Self { self.w(176) }
    #[inline(always)] fn w_48(self) -> Self { self.w(192) }
    #[inline(always)] fn w_52(self) -> Self { self.w(208) }
    #[inline(always)] fn w_56(self) -> Self { self.w(224) }
    #[inline(always)] fn w_60(self) -> Self { self.w(240) }
    #[inline(always)] fn w_64(self) -> Self { self.w(256) }
    #[inline(always)] fn w_72(self) -> Self { self.w(288) }
    #[inline(always)] fn w_80(self) -> Self { self.w(320) }
    #[inline(always)] fn w_96(self) -> Self { self.w(384) }

    #[inline(always)] fn h_0(self) -> Self { self.h(0) }
    #[inline(always)] fn h_0_5(self) -> Self { self.h(2) }
    #[inline(always)] fn h_1(self) -> Self { self.h(4) }
    #[inline(always)] fn h_1_5(self) -> Self { self.h(6) }
    #[inline(always)] fn h_2(self) -> Self { self.h(8) }
    #[inline(always)] fn h_2_5(self) -> Self { self.h(10) }
    #[inline(always)] fn h_3(self) -> Self { self.h(12) }
    #[inline(always)] fn h_3_5(self) -> Self { self.h(14) }
    #[inline(always)] fn h_4(self) -> Self { self.h(16) }
    #[inline(always)] fn h_5(self) -> Self { self.h(20) }
    #[inline(always)] fn h_6(self) -> Self { self.h(24) }
    #[inline(always)] fn h_7(self) -> Self { self.h(28) }
    #[inline(always)] fn h_8(self) -> Self { self.h(32) }
    #[inline(always)] fn h_9(self) -> Self { self.h(36) }
    #[inline(always)] fn h_10(self) -> Self { self.h(40) }
    #[inline(always)] fn h_11(self) -> Self { self.h(44) }
    #[inline(always)] fn h_12(self) -> Self { self.h(48) }
    #[inline(always)] fn h_14(self) -> Self { self.h(56) }
    #[inline(always)] fn h_16(self) -> Self { self.h(64) }
    #[inline(always)] fn h_20(self) -> Self { self.h(80) }
    #[inline(always)] fn h_24(self) -> Self { self.h(96) }
    #[inline(always)] fn h_32(self) -> Self { self.h(128) }
    #[inline(always)] fn h_40(self) -> Self { self.h(160) }
    #[inline(always)] fn h_48(self) -> Self { self.h(192) }
    #[inline(always)] fn h_56(self) -> Self { self.h(224) }
    #[inline(always)] fn h_64(self) -> Self { self.h(256) }

    #[inline(always)]
    fn p(mut self, px: i8) -> Self {
        self.style_mut().padding = Edges::all(px);
        self
    }

    #[inline(always)]
    fn px(mut self, px: i8) -> Self {
        let p = &mut self.style_mut().padding;
        p.left = px;
        p.right = px;
        self
    }

    #[inline(always)]
    fn py(mut self, px: i8) -> Self {
        let p = &mut self.style_mut().padding;
        p.top = px;
        p.bottom = px;
        self
    }

    #[inline(always)]
    fn pt(mut self, px: i8) -> Self {
        self.style_mut().padding.top = px;
        self
    }

    #[inline(always)]
    fn pb(mut self, px: i8) -> Self {
        self.style_mut().padding.bottom = px;
        self
    }

    #[inline(always)]
    fn pl(mut self, px: i8) -> Self {
        self.style_mut().padding.left = px;
        self
    }

    #[inline(always)]
    fn pr(mut self, px: i8) -> Self {
        self.style_mut().padding.right = px;
        self
    }

    // --- Tailwind 内边距档位简写 ---
    #[inline(always)] fn p_0(self) -> Self { self.p(0) }
    #[inline(always)] fn p_0p5(self) -> Self { self.p(2) }
    #[inline(always)] fn p_0_5(self) -> Self { self.p(2) }
    #[inline(always)] fn p_1(self) -> Self { self.p(4) }
    #[inline(always)] fn p_1p5(self) -> Self { self.p(6) }
    #[inline(always)] fn p_1_5(self) -> Self { self.p(6) }
    #[inline(always)] fn p_2(self) -> Self { self.p(8) }
    #[inline(always)] fn p_2p5(self) -> Self { self.p(10) }
    #[inline(always)] fn p_2_5(self) -> Self { self.p(10) }
    #[inline(always)] fn p_3(self) -> Self { self.p(12) }
    #[inline(always)] fn p_3p5(self) -> Self { self.p(14) }
    #[inline(always)] fn p_3_5(self) -> Self { self.p(14) }
    #[inline(always)] fn p_4(self) -> Self { self.p(16) }
    #[inline(always)] fn p_5(self) -> Self { self.p(20) }
    #[inline(always)] fn p_6(self) -> Self { self.p(24) }
    #[inline(always)] fn p_8(self) -> Self { self.p(32) }
    #[inline(always)] fn p_10(self) -> Self { self.p(40) }
    #[inline(always)] fn p_12(self) -> Self { self.p(48) }

    #[inline(always)] fn px_0(self) -> Self { self.px(0) }
    #[inline(always)] fn px_0p5(self) -> Self { self.px(2) }
    #[inline(always)] fn px_0_5(self) -> Self { self.px(2) }
    #[inline(always)] fn px_1(self) -> Self { self.px(4) }
    #[inline(always)] fn px_1p5(self) -> Self { self.px(6) }
    #[inline(always)] fn px_1_5(self) -> Self { self.px(6) }
    #[inline(always)] fn px_2(self) -> Self { self.px(8) }
    #[inline(always)] fn px_2p5(self) -> Self { self.px(10) }
    #[inline(always)] fn px_2_5(self) -> Self { self.px(10) }
    #[inline(always)] fn px_3(self) -> Self { self.px(12) }
    #[inline(always)] fn px_3p5(self) -> Self { self.px(14) }
    #[inline(always)] fn px_3_5(self) -> Self { self.px(14) }
    #[inline(always)] fn px_4(self) -> Self { self.px(16) }
    #[inline(always)] fn px_5(self) -> Self { self.px(20) }
    #[inline(always)] fn px_6(self) -> Self { self.px(24) }
    #[inline(always)] fn px_8(self) -> Self { self.px(32) }

    #[inline(always)] fn py_0(self) -> Self { self.py(0) }
    #[inline(always)] fn py_0p5(self) -> Self { self.py(2) }
    #[inline(always)] fn py_0_5(self) -> Self { self.py(2) }
    #[inline(always)] fn py_1(self) -> Self { self.py(4) }
    #[inline(always)] fn py_1p5(self) -> Self { self.py(6) }
    #[inline(always)] fn py_1_5(self) -> Self { self.py(6) }
    #[inline(always)] fn py_2(self) -> Self { self.py(8) }
    #[inline(always)] fn py_2p5(self) -> Self { self.py(10) }
    #[inline(always)] fn py_2_5(self) -> Self { self.py(10) }
    #[inline(always)] fn py_3(self) -> Self { self.py(12) }
    #[inline(always)] fn py_3p5(self) -> Self { self.py(14) }
    #[inline(always)] fn py_3_5(self) -> Self { self.py(14) }
    #[inline(always)] fn py_4(self) -> Self { self.py(16) }
    #[inline(always)] fn py_5(self) -> Self { self.py(20) }
    #[inline(always)] fn py_6(self) -> Self { self.py(24) }
    #[inline(always)] fn py_8(self) -> Self { self.py(32) }

    #[inline(always)] fn pt_0(self) -> Self { self.pt(0) }
    #[inline(always)] fn pt_1(self) -> Self { self.pt(4) }
    #[inline(always)] fn pt_2(self) -> Self { self.pt(8) }
    #[inline(always)] fn pt_3(self) -> Self { self.pt(12) }
    #[inline(always)] fn pt_4(self) -> Self { self.pt(16) }
    #[inline(always)] fn pb_0(self) -> Self { self.pb(0) }
    #[inline(always)] fn pb_1(self) -> Self { self.pb(4) }
    #[inline(always)] fn pb_2(self) -> Self { self.pb(8) }
    #[inline(always)] fn pb_3(self) -> Self { self.pb(12) }
    #[inline(always)] fn pb_4(self) -> Self { self.pb(16) }
    #[inline(always)] fn pl_0(self) -> Self { self.pl(0) }
    #[inline(always)] fn pl_1(self) -> Self { self.pl(4) }
    #[inline(always)] fn pl_2(self) -> Self { self.pl(8) }
    #[inline(always)] fn pl_3(self) -> Self { self.pl(12) }
    #[inline(always)] fn pl_4(self) -> Self { self.pl(16) }
    #[inline(always)] fn pr_0(self) -> Self { self.pr(0) }
    #[inline(always)] fn pr_1(self) -> Self { self.pr(4) }
    #[inline(always)] fn pr_2(self) -> Self { self.pr(8) }
    #[inline(always)] fn pr_3(self) -> Self { self.pr(12) }
    #[inline(always)] fn pr_4(self) -> Self { self.pr(16) }

    #[inline(always)]
    fn m(mut self, px: i8) -> Self {
        self.style_mut().set_margin_px(px);
        self
    }

    #[inline(always)] fn m_0(self) -> Self { self.m(0) }
    #[inline(always)] fn m_0_5(self) -> Self { self.m(2) }
    #[inline(always)] fn m_1(self) -> Self { self.m(4) }
    #[inline(always)] fn m_1_5(self) -> Self { self.m(6) }
    #[inline(always)] fn m_2(self) -> Self { self.m(8) }
    #[inline(always)] fn m_2_5(self) -> Self { self.m(10) }
    #[inline(always)] fn m_3(self) -> Self { self.m(12) }
    #[inline(always)] fn m_4(self) -> Self { self.m(16) }

    #[inline(always)]
    fn gap(mut self, px: i8) -> Self {
        self.style_mut().gap = px;
        self
    }

    #[inline(always)] fn gap_0(self) -> Self { self.gap(0) }
    #[inline(always)] fn gap_0p5(self) -> Self { self.gap(2) }
    #[inline(always)] fn gap_0_5(self) -> Self { self.gap(2) }
    #[inline(always)] fn gap_1(self) -> Self { self.gap(4) }
    #[inline(always)] fn gap_1p5(self) -> Self { self.gap(6) }
    #[inline(always)] fn gap_1_5(self) -> Self { self.gap(6) }
    #[inline(always)] fn gap_2(self) -> Self { self.gap(8) }
    #[inline(always)] fn gap_2p5(self) -> Self { self.gap(10) }
    #[inline(always)] fn gap_2_5(self) -> Self { self.gap(10) }
    #[inline(always)] fn gap_3(self) -> Self { self.gap(12) }
    #[inline(always)] fn gap_3p5(self) -> Self { self.gap(14) }
    #[inline(always)] fn gap_3_5(self) -> Self { self.gap(14) }
    #[inline(always)] fn gap_4(self) -> Self { self.gap(16) }
    #[inline(always)] fn gap_5(self) -> Self { self.gap(20) }
    #[inline(always)] fn gap_6(self) -> Self { self.gap(24) }
    #[inline(always)] fn gap_8(self) -> Self { self.gap(32) }

    #[inline(always)]
    fn flex_grow(mut self, weight: u8) -> Self {
        self.style_mut().set_grow(weight);
        self
    }

    #[inline(always)]
    fn items_center(mut self) -> Self {
        self.style_mut().set_items_center();
        self
    }

    #[inline(always)]
    fn items_start(mut self) -> Self {
        self.style_mut().set_items_align(AlignItems::Start);
        self
    }

    #[inline(always)]
    fn items_end(mut self) -> Self {
        self.style_mut().set_items_align(AlignItems::End);
        self
    }

    #[inline(always)]
    fn items_stretch(mut self) -> Self {
        self.style_mut().set_items_align(AlignItems::Stretch);
        self
    }

    #[inline(always)]
    fn justify_start(mut self) -> Self {
        self.style_mut().set_justify(Justify::Start);
        self
    }

    #[inline(always)]
    fn justify_center(mut self) -> Self {
        self.style_mut().set_justify_center();
        self
    }

    #[inline(always)]
    fn justify_between(mut self) -> Self {
        self.style_mut().set_justify_between();
        self
    }

    #[inline(always)]
    fn justify_end(mut self) -> Self {
        self.style_mut().set_justify(Justify::End);
        self
    }

    #[inline(always)]
    fn justify_around(mut self) -> Self {
        self.style_mut().set_justify(Justify::Around);
        self
    }

    #[inline(always)]
    fn bg(mut self, color: Color) -> Self {
        self.style_mut().bg = color;
        self
    }

    #[inline(always)]
    fn rounded(mut self, radius: u8) -> Self {
        self.style_mut().set_rounded(radius);
        self
    }

    #[inline(always)] fn rounded_none(self) -> Self { self.rounded(0) }
    #[inline(always)] fn rounded_xs(self) -> Self { self.rounded(2) }
    #[inline(always)] fn rounded_sm(self) -> Self { self.rounded(4) }
    #[inline(always)] fn rounded_md(self) -> Self { self.rounded(6) }
    #[inline(always)] fn rounded_lg(self) -> Self { self.rounded(8) }
    #[inline(always)] fn rounded_xl(self) -> Self { self.rounded(12) }
    #[inline(always)] fn rounded_2xl(self) -> Self { self.rounded(16) }
    #[inline(always)] fn rounded_3xl(self) -> Self { self.rounded(24) }
    #[inline(always)] fn rounded_full(self) -> Self { self.rounded(127) }

    #[inline(always)]
    fn border(mut self, width: u8, color: Color) -> Self {
        let s = self.style_mut();
        s.set_border_px(width);
        s.border_color = color;
        self
    }

    #[inline(always)]
    fn border_0(mut self) -> Self {
        self.style_mut().set_border_px(0);
        self
    }

    #[inline(always)]
    fn border_1(mut self) -> Self {
        self.style_mut().set_border_px(1);
        self
    }

    #[inline(always)]
    fn border_2(mut self) -> Self {
        self.style_mut().set_border_px(2);
        self
    }

    #[inline(always)]
    fn border_4(mut self) -> Self {
        self.style_mut().set_border_px(4);
        self
    }

    #[inline(always)]
    fn border_8(mut self) -> Self {
        self.style_mut().set_border_px(8);
        self
    }

    #[inline(always)]
    fn border_color(mut self, color: Color) -> Self {
        self.style_mut().border_color = color;
        self
    }

    #[inline(always)]
    fn flex_shrink_0(self) -> Self {
        self
    }
}

pub use crate::style_ext::{StyleExt, StyledExt};
