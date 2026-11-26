//! 现代 2D 矢量与合成原语（`#![no_std]` 零全局堆分配兼容）：
//!
//! 对标 Skia / Vello 的核心渲染语义，为 GPU (`wgpu`) 与软件 (`software`) 双后端提供统一真源：
//! - [`CornerRadii`]：四角独立圆角 `[tl, tr, br, bl]`；
//! - [`Paint`] / [`ColorStop`] / [`ExtendMode`] / [`GradientSpace`]：多色标任意角度线性渐变、双圆焦点径向渐变、锥形扫描渐变，支持 `sRGB` / `LinearRgb` / `Oklab` 色彩空间与 `Pad` / `Repeat` / `Reflect` 延展模式；
//! - [`BlendMode`]：完整 27 种 Porter-Duff 合成模式与 W3C Compositing Level 1 可分离 / HSL 不可分离混合模式；
//! - [`ColorMatrix`] / [`DropShadow`] / [`LayerFilter`] / [`BackdropFilter`]：色彩矩阵、子树高斯模糊、任意形状投影与实时毛玻璃背景模糊；
//! - [`StyledQuad`] / [`LayerSpec`] / [`PaintEffects`]：分配于 `ScratchArena` 的零堆冷路径渲染载荷，不增加常规节点一字节内存。

use crate::arena::StrRef;
use crate::solver::Region;
use crate::style::Color;
use crate::svg::math::{
    abs_f32, atan2_f32, cbrt_f32, floor_f32, linear_to_srgb_f32, sin_cos_f32, sqrt_f32,
    srgb_to_linear_f32, PI, TAU,
};
pub use crate::svg::Transform2D;

/// 规范化 `f32` 位模式（消除 `-0.0` 与 `NaN` 差异以保证快照哈希确定性）。
#[inline(always)]
pub(crate) fn canonical_f32_bits(v: f32) -> u32 {
    if v == 0.0 {
        0
    } else if v.is_nan() {
        0x7FC0_0000
    } else {
        v.to_bits()
    }
}

#[inline(always)]
pub(crate) fn mix_u8(h: &mut u32, b: u8) {
    *h ^= b as u32;
    *h = h.wrapping_mul(0x0100_0193);
}

#[inline(always)]
pub(crate) fn mix_u32(h: &mut u32, v: u32) {
    for b in v.to_le_bytes() {
        mix_u8(h, b);
    }
}

#[inline(always)]
pub(crate) fn mix_f32(h: &mut u32, v: f32) {
    mix_u32(h, canonical_f32_bits(v));
}

#[inline(always)]
pub(crate) fn mix_transform(h: &mut u32, t: Transform2D) {
    mix_f32(h, t.a);
    mix_f32(h, t.b);
    mix_f32(h, t.c);
    mix_f32(h, t.d);
    mix_f32(h, t.e);
    mix_f32(h, t.f);
}

// ============================================================================
// 1. 四角独立圆角 `CornerRadii`
// ============================================================================

/// 矩形四角独立圆角半径（逻辑像素，顺序：`[top-left, top-right, bottom-right, bottom-left]`）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub struct CornerRadii {
    pub tl: u8,
    pub tr: u8,
    pub br: u8,
    pub bl: u8,
}

impl CornerRadii {
    /// 直角（四角全为 `0`）。
    pub const ZERO: Self = Self::all(0);

    /// 四角使用统一圆角半径 `r`。
    #[inline(always)]
    pub const fn all(r: u8) -> Self {
        Self {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        }
    }

    /// 分别指定左上、右上、右下、左下四角圆角半径。
    #[inline(always)]
    pub const fn new(tl: u8, tr: u8, br: u8, bl: u8) -> Self {
        Self { tl, tr, br, bl }
    }

    /// 仅顶部两角圆角（底部直角，常用于选项卡、抽屉顶栏、弹窗头）。
    #[inline(always)]
    pub const fn top(r: u8) -> Self {
        Self::new(r, r, 0, 0)
    }

    /// 仅底部两角圆角（顶部直角）。
    #[inline(always)]
    pub const fn bottom(r: u8) -> Self {
        Self::new(0, 0, r, r)
    }

    /// 仅左侧两角圆角（右侧直角，常用于分段按钮左端）。
    #[inline(always)]
    pub const fn left(r: u8) -> Self {
        Self::new(r, 0, 0, r)
    }

    /// 仅右侧两角圆角（左侧直角）。
    #[inline(always)]
    pub const fn right(r: u8) -> Self {
        Self::new(0, r, r, 0)
    }

    /// 四角是否全部相等。
    #[inline(always)]
    pub const fn is_uniform(self) -> bool {
        self.tl == self.tr && self.tr == self.br && self.br == self.bl
    }

    /// 四角是否全为 `0`。
    #[inline(always)]
    pub const fn is_zero(self) -> bool {
        self.tl == 0 && self.tr == 0 && self.br == 0 && self.bl == 0
    }

    /// 返回四角中的最大圆角半径。
    #[inline(always)]
    pub const fn max_radius(self) -> u8 {
        let a = if self.tl > self.tr { self.tl } else { self.tr };
        let b = if self.br > self.bl { self.br } else { self.bl };
        if a > b { a } else { b }
    }

    /// 转为 `[tl, tr, br, bl]` 数组。
    #[inline(always)]
    pub const fn as_array(self) -> [u8; 4] {
        [self.tl, self.tr, self.br, self.bl]
    }
}

// ============================================================================
// 2. 完整多色标渐变系统（Linear / Radial / Conic + ExtendMode + OKLab）
// ============================================================================

/// 渐变单个色标（归一化位置 `offset` 通常位于 `0.0..=1.0`）。
#[derive(Copy, Clone, Debug)]
pub struct ColorStop {
    pub offset: f32,
    pub color: Color,
}

impl ColorStop {
    /// 构造一个渐变色标。
    #[inline(always)]
    pub const fn new(offset: f32, color: Color) -> Self {
        Self { offset, color }
    }
}

impl PartialEq for ColorStop {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        canonical_f32_bits(self.offset) == canonical_f32_bits(other.offset)
            && self.color == other.color
    }
}

impl Eq for ColorStop {}

/// 渐变越界延展模式（对应 W3C / Skia TileMode / Vello Extend）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum ExtendMode {
    /// 钳位至首尾色标（默认）。
    #[default]
    Pad = 0,
    /// 周期重复（`fract(t)`）。
    Repeat = 1,
    /// 镜像往复反射（乒乓锯齿波）。
    Reflect = 2,
}

/// 渐变色标插值色彩空间。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum GradientSpace {
    /// 经典 sRGB 预乘 Alpha 插值（兼容传统 CSS / Canvas）。
    #[default]
    Srgb = 0,
    /// 物理线性光强（Linear sRGB）插值（光晕与暗色过渡更自然）。
    LinearRgb = 1,
    /// 感知均匀 OKLab 色彩空间插值（消除红-绿 / 蓝-黄渐变中段发灰）。
    Oklab = 2,
}

/// 通用填充画刷原语（零堆分配，渐变色标借用自 `ScratchArena` 或 `'static` 切片）。
#[derive(Copy, Clone, Debug)]
pub enum Paint<'a> {
    /// 纯色填充。
    Solid(Color),
    /// 任意角度/任意起止点多色标线性渐变（`start`/`end` 为归一化局部坐标 `[0.0..=1.0]`）。
    LinearGradient {
        start: [f32; 2],
        end: [f32; 2],
        stops: &'a [ColorStop],
        extend: ExtendMode,
        space: GradientSpace,
    },
    /// 支持双圆焦点偏移的多色标径向渐变（所有坐标与半径均为归一化局部坐标，相对元素宽高的最小边或各自轴）。
    RadialGradient {
        center: [f32; 2],
        radius: f32,
        focal: [f32; 2],
        focal_radius: f32,
        stops: &'a [ColorStop],
        extend: ExtendMode,
        space: GradientSpace,
    },
    /// 多色标锥形/扫描渐变（`start_angle_deg` 以 12 点钟为 `0°` 顺时针增加，`sweep_deg` 默认 `360°`）。
    ConicGradient {
        center: [f32; 2],
        start_angle_deg: f32,
        sweep_deg: f32,
        stops: &'a [ColorStop],
        extend: ExtendMode,
        space: GradientSpace,
    },
}

impl<'a> PartialEq for Paint<'a> {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Solid(a), Self::Solid(b)) => a == b,
            (
                Self::LinearGradient {
                    start: s1,
                    end: e1,
                    stops: st1,
                    extend: ex1,
                    space: sp1,
                },
                Self::LinearGradient {
                    start: s2,
                    end: e2,
                    stops: st2,
                    extend: ex2,
                    space: sp2,
                },
            ) => {
                canonical_f32_bits(s1[0]) == canonical_f32_bits(s2[0])
                    && canonical_f32_bits(s1[1]) == canonical_f32_bits(s2[1])
                    && canonical_f32_bits(e1[0]) == canonical_f32_bits(e2[0])
                    && canonical_f32_bits(e1[1]) == canonical_f32_bits(e2[1])
                    && ex1 == ex2
                    && sp1 == sp2
                    && st1 == st2
            }
            (
                Self::RadialGradient {
                    center: c1,
                    radius: r1,
                    focal: f1,
                    focal_radius: fr1,
                    stops: st1,
                    extend: ex1,
                    space: sp1,
                },
                Self::RadialGradient {
                    center: c2,
                    radius: r2,
                    focal: f2,
                    focal_radius: fr2,
                    stops: st2,
                    extend: ex2,
                    space: sp2,
                },
            ) => {
                canonical_f32_bits(c1[0]) == canonical_f32_bits(c2[0])
                    && canonical_f32_bits(c1[1]) == canonical_f32_bits(c2[1])
                    && canonical_f32_bits(r1) == canonical_f32_bits(r2)
                    && canonical_f32_bits(f1[0]) == canonical_f32_bits(f2[0])
                    && canonical_f32_bits(f1[1]) == canonical_f32_bits(f2[1])
                    && canonical_f32_bits(fr1) == canonical_f32_bits(fr2)
                    && ex1 == ex2
                    && sp1 == sp2
                    && st1 == st2
            }
            (
                Self::ConicGradient {
                    center: c1,
                    start_angle_deg: a1,
                    sweep_deg: sw1,
                    stops: st1,
                    extend: ex1,
                    space: sp1,
                },
                Self::ConicGradient {
                    center: c2,
                    start_angle_deg: a2,
                    sweep_deg: sw2,
                    stops: st2,
                    extend: ex2,
                    space: sp2,
                },
            ) => {
                canonical_f32_bits(c1[0]) == canonical_f32_bits(c2[0])
                    && canonical_f32_bits(c1[1]) == canonical_f32_bits(c2[1])
                    && canonical_f32_bits(a1) == canonical_f32_bits(a2)
                    && canonical_f32_bits(sw1) == canonical_f32_bits(sw2)
                    && ex1 == ex2
                    && sp1 == sp2
                    && st1 == st2
            }
            _ => false,
        }
    }
}

impl<'a> Eq for Paint<'a> {}

impl<'a> Paint<'a> {
    /// 按 CSS 角度规范构造多色标线性渐变（`0°` 向上、`90°` 向右、`180°` 向下，`135°` 左上至右下）。
    #[inline]
    pub fn linear_angle(angle_deg: f32, stops: &'a [ColorStop]) -> Self {
        let rad = angle_deg * (PI / 180.0);
        let (sin_a, cos_a) = sin_cos_f32(rad);
        // CSS 0deg = 向上 (0, -1), 90deg = 向右 (1, 0)
        let dx = sin_a;
        let dy = -cos_a;
        let half_len = 0.5 * (abs_f32(dx) + abs_f32(dy));
        let hx = dx * half_len;
        let hy = dy * half_len;
        Self::LinearGradient {
            start: [0.5 - hx, 0.5 - hy],
            end: [0.5 + hx, 0.5 + hy],
            stops,
            extend: ExtendMode::Pad,
            space: GradientSpace::Srgb,
        }
    }

    /// 构造居中径向渐变（默认中心 `[0.5, 0.5]`、半径 `0.5`）。
    #[inline]
    pub const fn radial(stops: &'a [ColorStop]) -> Self {
        Self::RadialGradient {
            center: [0.5, 0.5],
            radius: 0.5,
            focal: [0.5, 0.5],
            focal_radius: 0.0,
            stops,
            extend: ExtendMode::Pad,
            space: GradientSpace::Srgb,
        }
    }

    /// 构造带焦点的双圆径向渐变。
    #[inline]
    pub const fn radial_focal(
        center: [f32; 2],
        radius: f32,
        focal: [f32; 2],
        focal_radius: f32,
        stops: &'a [ColorStop],
    ) -> Self {
        Self::RadialGradient {
            center,
            radius,
            focal,
            focal_radius,
            stops,
            extend: ExtendMode::Pad,
            space: GradientSpace::Srgb,
        }
    }

    /// 构造居中锥形/扫描渐变（`start_angle_deg` 以 12 点钟为 `0°` 顺时针）。
    #[inline]
    pub const fn conic(start_angle_deg: f32, stops: &'a [ColorStop]) -> Self {
        Self::ConicGradient {
            center: [0.5, 0.5],
            start_angle_deg,
            sweep_deg: 360.0,
            stops,
            extend: ExtendMode::Pad,
            space: GradientSpace::Srgb,
        }
    }

    /// 设置渐变的延展模式（`Pad` / `Repeat` / `Reflect`）。
    #[inline]
    pub const fn with_extend(mut self, mode: ExtendMode) -> Self {
        match &mut self {
            Self::LinearGradient { extend, .. }
            | Self::RadialGradient { extend, .. }
            | Self::ConicGradient { extend, .. } => *extend = mode,
            Self::Solid(_) => {}
        }
        self
    }

    /// 设置渐变的色彩插值空间（`Srgb` / `LinearRgb` / `Oklab`）。
    #[inline]
    pub const fn with_space(mut self, color_space: GradientSpace) -> Self {
        match &mut self {
            Self::LinearGradient { space, .. }
            | Self::RadialGradient { space, .. }
            | Self::ConicGradient { space, .. } => *space = color_space,
            Self::Solid(_) => {}
        }
        self
    }

    /// 判断该画刷是否全范围完全透明（可安全跳过填充绘制）。
    #[inline]
    pub fn is_fully_transparent(&self) -> bool {
        match *self {
            Self::Solid(c) => c.a() == 0,
            Self::LinearGradient { stops, .. }
            | Self::RadialGradient { stops, .. }
            | Self::ConicGradient { stops, .. } => {
                stops.is_empty() || stops.iter().all(|s| s.color.a() == 0)
            }
        }
    }

    /// 判断该画刷是否在所有采样点都 100% 不透明。
    #[inline]
    pub fn is_fully_opaque(&self) -> bool {
        match *self {
            Self::Solid(c) => c.a() == 255,
            Self::LinearGradient { stops, .. }
            | Self::RadialGradient { stops, .. }
            | Self::ConicGradient { stops, .. } => {
                !stops.is_empty() && stops.iter().all(|s| s.color.a() == 255)
            }
        }
    }

    /// 将当前画刷全部字段混入 FNV-1a 哈希（供 `DrawOpSnapshot` 与 GPU Ramp 缓存使用）。
    pub fn mix_hash(&self, h: &mut u32) {
        match *self {
            Self::Solid(c) => {
                mix_u8(h, 1);
                mix_u32(h, c.0);
            }
            Self::LinearGradient {
                start,
                end,
                stops,
                extend,
                space,
            } => {
                mix_u8(h, 2);
                mix_f32(h, start[0]);
                mix_f32(h, start[1]);
                mix_f32(h, end[0]);
                mix_f32(h, end[1]);
                mix_u8(h, extend as u8);
                mix_u8(h, space as u8);
                mix_stops_hash(h, stops);
            }
            Self::RadialGradient {
                center,
                radius,
                focal,
                focal_radius,
                stops,
                extend,
                space,
            } => {
                mix_u8(h, 3);
                mix_f32(h, center[0]);
                mix_f32(h, center[1]);
                mix_f32(h, radius);
                mix_f32(h, focal[0]);
                mix_f32(h, focal[1]);
                mix_f32(h, focal_radius);
                mix_u8(h, extend as u8);
                mix_u8(h, space as u8);
                mix_stops_hash(h, stops);
            }
            Self::ConicGradient {
                center,
                start_angle_deg,
                sweep_deg,
                stops,
                extend,
                space,
            } => {
                mix_u8(h, 4);
                mix_f32(h, center[0]);
                mix_f32(h, center[1]);
                mix_f32(h, start_angle_deg);
                mix_f32(h, sweep_deg);
                mix_u8(h, extend as u8);
                mix_u8(h, space as u8);
                mix_stops_hash(h, stops);
            }
        }
    }

    /// 评估归一化局部坐标 `(u, v) ∈ [0, 1]²` 处的渐变位置参数 `t`（`None` 表示退化或双圆锥外无效区）。
    #[inline]
    pub fn eval_gradient_t(&self, u: f32, v: f32) -> Option<f32> {
        match *self {
            Self::Solid(_) => Some(0.0),
            Self::LinearGradient {
                start, end, extend, ..
            } => {
                let dx = end[0] - start[0];
                let dy = end[1] - start[1];
                let denom = dx * dx + dy * dy;
                if denom < 1e-8 {
                    return Some(0.0);
                }
                let t = ((u - start[0]) * dx + (v - start[1]) * dy) / denom;
                Some(apply_extend(t, extend))
            }
            Self::RadialGradient {
                center,
                radius,
                focal,
                focal_radius,
                extend,
                ..
            } => {
                let r1 = radius.max(1e-5);
                let r0 = focal_radius.max(0.0);
                let cdx = center[0] - focal[0];
                let cdy = center[1] - focal[1];
                if abs_f32(cdx) < 1e-6 && abs_f32(cdy) < 1e-6 && r0 < 1e-6 {
                    let dx = u - center[0];
                    let dy = v - center[1];
                    let t = sqrt_f32(dx * dx + dy * dy) / r1;
                    return Some(apply_extend(t, extend));
                }
                // 双圆焦点径向渐变：求解 |p - (f + t*(c - f))|² = (r0 + t*(r1 - r0))²
                let px = u - focal[0];
                let py = v - focal[1];
                let dr = r1 - r0;
                let a = cdx * cdx + cdy * cdy - dr * dr;
                let b = -2.0 * (px * cdx + py * cdy + r0 * dr);
                let c_term = px * px + py * py - r0 * r0;
                let t = if abs_f32(a) < 1e-6 {
                    if abs_f32(b) < 1e-6 {
                        return None;
                    }
                    -c_term / b
                } else {
                    let disc = b * b - 4.0 * a * c_term;
                    if disc < 0.0 {
                        return None;
                    }
                    let sd = sqrt_f32(disc);
                    let t_hi = (-b + sd) / (2.0 * a);
                    let t_lo = (-b - sd) / (2.0 * a);
                    if r0 + t_hi * dr >= 0.0 {
                        t_hi
                    } else if r0 + t_lo * dr >= 0.0 {
                        t_lo
                    } else {
                        return None;
                    }
                };
                Some(apply_extend(t, extend))
            }
            Self::ConicGradient {
                center,
                start_angle_deg,
                sweep_deg,
                extend,
                ..
            } => {
                let dx = u - center[0];
                let dy = v - center[1];
                // 12 点钟方向为 0 rad，顺时针增加到 TAU
                let mut angle = atan2_f32(dx, -dy);
                if angle < 0.0 {
                    angle += TAU;
                }
                let start_rad_raw = start_angle_deg * (PI / 180.0);
                let start_rad = start_rad_raw - floor_f32(start_rad_raw / TAU) * TAU;
                let mut rel = angle - start_rad;
                if rel < 0.0 {
                    rel += TAU;
                }
                let sweep_rad = (abs_f32(sweep_deg).max(1e-3) * (PI / 180.0)).min(TAU);
                let t = rel / sweep_rad;
                Some(apply_extend(t, extend))
            }
        }
    }
}

#[inline]
fn mix_stops_hash(h: &mut u32, stops: &[ColorStop]) {
    mix_u32(h, stops.len() as u32);
    for s in stops {
        mix_f32(h, s.offset);
        mix_u32(h, s.color.0);
    }
}

/// 计算 64 位渐变色标与色彩空间指纹（供 GPU `GradientRampAtlas` 缓存复用）。
pub fn gradient_ramp_hash(stops: &[ColorStop], space: GradientSpace, opacity: u8) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mix = |h: &mut u64, v: u64| {
        *h ^= v;
        *h = h.wrapping_mul(0x0100_0000_01b3);
    };
    mix(&mut h, space as u64);
    mix(&mut h, opacity as u64);
    mix(&mut h, stops.len() as u64);
    for s in stops {
        mix(&mut h, canonical_f32_bits(s.offset) as u64);
        mix(&mut h, s.color.0 as u64);
    }
    h
}

/// 按 [`ExtendMode`] 将任意实数 `t` 归约到 `[0.0, 1.0]`。
#[inline(always)]
pub fn apply_extend(t: f32, mode: ExtendMode) -> f32 {
    match mode {
        ExtendMode::Pad => t.clamp(0.0, 1.0),
        ExtendMode::Repeat => {
            let f = t - floor_f32(t);
            f.clamp(0.0, 1.0)
        }
        ExtendMode::Reflect => {
            let half = t * 0.5;
            let f = (half - floor_f32(half)) * 2.0;
            let r = if f <= 1.0 { f } else { 2.0 - f };
            r.clamp(0.0, 1.0)
        }
    }
}

/// Linear sRGB 转感知均匀 OKLab（Björn Ottosson 官方标准系数）。
#[inline]
pub fn linear_srgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let l_ = cbrt_f32(l);
    let m_ = cbrt_f32(m);
    let s_ = cbrt_f32(s);
    (
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    )
}

/// OKLab 转 Linear sRGB（带 `[0.0, 1.0]` 色域安全钳位）。
#[inline]
pub fn oklab_to_linear_srgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), bl.clamp(0.0, 1.0))
}

/// 在指定 [`GradientSpace`] 下于两个颜色之间执行预乘 Alpha 友好插值，返回 sRGB 直通 `Color`。
#[inline]
pub fn interpolate_colors(c0: Color, c1: Color, t: f32, space: GradientSpace) -> Color {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    let a0 = (c0.a() as f32) * (1.0 / 255.0);
    let a1 = (c1.a() as f32) * (1.0 / 255.0);
    let a = a0 * inv + a1 * t;
    if a <= 1e-5 {
        return Color::TRANSPARENT;
    }
    let (r0, g0, b0) = (
        (c0.r() as f32) * (1.0 / 255.0),
        (c0.g() as f32) * (1.0 / 255.0),
        (c0.b() as f32) * (1.0 / 255.0),
    );
    let (r1, g1, b1) = (
        (c1.r() as f32) * (1.0 / 255.0),
        (c1.g() as f32) * (1.0 / 255.0),
        (c1.b() as f32) * (1.0 / 255.0),
    );

    let (r, g, b) = match space {
        GradientSpace::Srgb => {
            // 预乘 Alpha 插值，避免透明色过渡出现发黑脏边
            let pr = (r0 * a0) * inv + (r1 * a1) * t;
            let pg = (g0 * a0) * inv + (g1 * a1) * t;
            let pb = (b0 * a0) * inv + (b1 * a1) * t;
            (pr / a, pg / a, pb / a)
        }
        GradientSpace::LinearRgb => {
            let lr0 = srgb_to_linear_f32(r0);
            let lg0 = srgb_to_linear_f32(g0);
            let lb0 = srgb_to_linear_f32(b0);
            let lr1 = srgb_to_linear_f32(r1);
            let lg1 = srgb_to_linear_f32(g1);
            let lb1 = srgb_to_linear_f32(b1);
            let pr = (lr0 * a0) * inv + (lr1 * a1) * t;
            let pg = (lg0 * a0) * inv + (lg1 * a1) * t;
            let pb = (lb0 * a0) * inv + (lb1 * a1) * t;
            (
                linear_to_srgb_f32(pr / a),
                linear_to_srgb_f32(pg / a),
                linear_to_srgb_f32(pb / a),
            )
        }
        GradientSpace::Oklab => {
            let (l0, oa0, ob0) = linear_srgb_to_oklab(
                srgb_to_linear_f32(r0),
                srgb_to_linear_f32(g0),
                srgb_to_linear_f32(b0),
            );
            let (l1, oa1, ob1) = linear_srgb_to_oklab(
                srgb_to_linear_f32(r1),
                srgb_to_linear_f32(g1),
                srgb_to_linear_f32(b1),
            );
            // 预乘 Alpha 权重以在透明端保持色相稳定
            let w0 = inv * a0;
            let w1 = t * a1;
            let wsum = w0 + w1;
            let (l, oa, ob) = if wsum > 1e-6 {
                (
                    (l0 * w0 + l1 * w1) / wsum,
                    (oa0 * w0 + oa1 * w1) / wsum,
                    (ob0 * w0 + ob1 * w1) / wsum,
                )
            } else {
                (l0 * inv + l1 * t, oa0 * inv + oa1 * t, ob0 * inv + ob1 * t)
            };
            let (lr, lg, lb) = oklab_to_linear_srgb(l, oa, ob);
            (
                linear_to_srgb_f32(lr),
                linear_to_srgb_f32(lg),
                linear_to_srgb_f32(lb),
            )
        }
    };

    Color::rgba(
        (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
    )
}

/// 在色标切片 `stops` 中按 `t ∈ [0.0, 1.0]` 采样插值颜色（支持任意顺序或重复 offset，零堆分配）。
pub fn sample_gradient_stops(
    stops: &[ColorStop],
    t: f32,
    space: GradientSpace,
    opacity: u8,
) -> Color {
    if stops.is_empty() || opacity == 0 {
        return Color::TRANSPARENT;
    }
    if stops.len() == 1 {
        return stops[0].color.scale_alpha(opacity);
    }
    let t = t.clamp(0.0, 1.0);
    // 支持升序常见路径，同时对非单调输入稳健回退
    let first = stops[0];
    let last = stops[stops.len() - 1];
    if t <= first.offset {
        return first.color.scale_alpha(opacity);
    }
    if t >= last.offset {
        return last.color.scale_alpha(opacity);
    }
    for i in 0..(stops.len() - 1) {
        let s0 = stops[i];
        let s1 = stops[i + 1];
        if t <= s1.offset {
            let span = s1.offset - s0.offset;
            let local_t = if span <= 1e-6 {
                1.0
            } else {
                ((t - s0.offset) / span).clamp(0.0, 1.0)
            };
            return interpolate_colors(s0.color, s1.color, local_t, space).scale_alpha(opacity);
        }
    }
    last.color.scale_alpha(opacity)
}

/// 烘焙 1D 渐变查找表（GPU `GradientRampAtlas` 与软件光栅器共享真源，输出 sRGB 直通 RGBA8 字节）。
pub fn bake_gradient_lut<const N: usize>(
    stops: &[ColorStop],
    space: GradientSpace,
    opacity: u8,
    out: &mut [[u8; 4]; N],
) {
    let denom = (N.saturating_sub(1)).max(1) as f32;
    for (i, slot) in out.iter_mut().enumerate() {
        let t = (i as f32) / denom;
        let c = sample_gradient_stops(stops, t, space, opacity);
        *slot = [c.r(), c.g(), c.b(), c.a()];
    }
}

// ============================================================================
// 3. 完整 27 种 Porter-Duff 与 W3C 混合模式 `BlendMode`
// ============================================================================

/// 完整 27 种 Porter-Duff 合成模式与 W3C Compositing & Blending Level 1 混合模式。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum BlendMode {
    // --- Porter-Duff 合成模式 (0..=11) ---
    /// 标准源覆盖目标（默认）。
    #[default]
    SrcOver = 0,
    /// 清空目标像素。
    Clear = 1,
    /// 仅保留源像素（替换）。
    Src = 2,
    /// 仅保留目标像素。
    Dst = 3,
    /// 源裁剪至目标 Alpha 内（遮罩保留源）。
    SrcIn = 4,
    /// 目标裁剪至源 Alpha 内（使用源作为 Alpha Mask 裁切下层）。
    DstIn = 5,
    /// 源仅绘制在目标透明区。
    SrcOut = 6,
    /// 目标仅保留在源透明区（镂空/擦除）。
    DstOut = 7,
    /// 源绘制在目标已有像素上并保留目标轮廓。
    SrcAtop = 8,
    /// 目标绘制在源轮廓内。
    DstAtop = 9,
    /// 源与目标异或镂空。
    Xor = 10,
    /// 线性光强相加（Additive / Lighter）。
    Plus = 11,

    // --- W3C 可分离混合模式 (12..=22) ---
    /// 正片叠底。
    Multiply = 12,
    /// 滤色。
    Screen = 13,
    /// 叠加。
    Overlay = 14,
    /// 变暗。
    Darken = 15,
    /// 变亮。
    Lighten = 16,
    /// 颜色减淡。
    ColorDodge = 17,
    /// 颜色加深。
    ColorBurn = 18,
    /// 强光。
    HardLight = 19,
    /// 柔光。
    SoftLight = 20,
    /// 差值。
    Difference = 21,
    /// 排除。
    Exclusion = 22,

    // --- W3C 不可分离 HSL 混合模式 (23..=26) ---
    /// 色相。
    Hue = 23,
    /// 饱和度。
    Saturation = 24,
    /// 颜色（保留目标明度，替换源色相与饱和度）。
    Color = 25,
    /// 明度（保留目标色相与饱和度，替换源明度）。
    Luminosity = 26,
}

impl BlendMode {
    /// 是否为默认 `SrcOver`。
    #[inline(always)]
    pub const fn is_normal(self) -> bool {
        matches!(self, Self::SrcOver)
    }

    /// 是否需要读取目标画布像素以执行可编程合成（即非默认 `SrcOver`）。
    #[inline(always)]
    pub const fn requires_backdrop_read(self) -> bool {
        !matches!(self, Self::SrcOver)
    }
}

#[inline(always)]
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

#[inline]
fn clip_color(mut c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 && (l - n) > 1e-6 {
        let f = l / (l - n);
        c[0] = l + (c[0] - l) * f;
        c[1] = l + (c[1] - l) * f;
        c[2] = l + (c[2] - l) * f;
    }
    if x > 1.0 && (x - l) > 1e-6 {
        let f = (1.0 - l) / (x - l);
        c[0] = l + (c[0] - l) * f;
        c[1] = l + (c[1] - l) * f;
        c[2] = l + (c[2] - l) * f;
    }
    [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)]
}

#[inline]
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

#[inline(always)]
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

#[inline]
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    if c[idx[0]] > c[idx[1]] {
        idx.swap(0, 1);
    }
    if c[idx[1]] > c[idx[2]] {
        idx.swap(1, 2);
    }
    if c[idx[0]] > c[idx[1]] {
        idx.swap(0, 1);
    }
    let mut out = [0.0f32; 3];
    let min_v = c[idx[0]];
    let mid_v = c[idx[1]];
    let max_v = c[idx[2]];
    if max_v > min_v {
        out[idx[1]] = ((mid_v - min_v) * s) / (max_v - min_v);
        out[idx[2]] = s;
    }
    out
}

#[inline]
fn blend_channel_separable(cb: f32, cs: f32, mode: BlendMode) -> f32 {
    match mode {
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => {
            if cb <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::ColorDodge => {
            if cb <= 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BlendMode::HardLight => {
            if cs <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    sqrt_f32(cb)
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::Difference => abs_f32(cb - cs),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => cs,
    }
}

/// 对预乘 Alpha 的源像素 `src_pm` 与目标像素 `dst_pm` 执行完整 W3C / Porter-Duff 合成，返回预乘 Alpha 结果。
#[inline]
pub fn blend_premul_f32(src_pm: [f32; 4], dst_pm: [f32; 4], mode: BlendMode) -> [f32; 4] {
    let sa = src_pm[3].clamp(0.0, 1.0);
    let da = dst_pm[3].clamp(0.0, 1.0);
    let inv_sa = 1.0 - sa;
    let inv_da = 1.0 - da;

    match mode {
        BlendMode::SrcOver => [
            src_pm[0] + dst_pm[0] * inv_sa,
            src_pm[1] + dst_pm[1] * inv_sa,
            src_pm[2] + dst_pm[2] * inv_sa,
            sa + da * inv_sa,
        ],
        BlendMode::Clear => [0.0; 4],
        BlendMode::Src => src_pm,
        BlendMode::Dst => dst_pm,
        BlendMode::SrcIn => [
            src_pm[0] * da,
            src_pm[1] * da,
            src_pm[2] * da,
            sa * da,
        ],
        BlendMode::DstIn => [
            dst_pm[0] * sa,
            dst_pm[1] * sa,
            dst_pm[2] * sa,
            da * sa,
        ],
        BlendMode::SrcOut => [
            src_pm[0] * inv_da,
            src_pm[1] * inv_da,
            src_pm[2] * inv_da,
            sa * inv_da,
        ],
        BlendMode::DstOut => [
            dst_pm[0] * inv_sa,
            dst_pm[1] * inv_sa,
            dst_pm[2] * inv_sa,
            da * inv_sa,
        ],
        BlendMode::SrcAtop => [
            src_pm[0] * da + dst_pm[0] * inv_sa,
            src_pm[1] * da + dst_pm[1] * inv_sa,
            src_pm[2] * da + dst_pm[2] * inv_sa,
            da,
        ],
        BlendMode::DstAtop => [
            dst_pm[0] * sa + src_pm[0] * inv_da,
            dst_pm[1] * sa + src_pm[1] * inv_da,
            dst_pm[2] * sa + src_pm[2] * inv_da,
            sa,
        ],
        BlendMode::Xor => [
            src_pm[0] * inv_da + dst_pm[0] * inv_sa,
            src_pm[1] * inv_da + dst_pm[1] * inv_sa,
            src_pm[2] * inv_da + dst_pm[2] * inv_sa,
            sa * inv_da + da * inv_sa,
        ],
        BlendMode::Plus => [
            (src_pm[0] + dst_pm[0]).min(1.0),
            (src_pm[1] + dst_pm[1]).min(1.0),
            (src_pm[2] + dst_pm[2]).min(1.0),
            (sa + da).min(1.0),
        ],
        _ => {
            // W3C Compositing Level 1:
            // co = αs*(1-αb)*Cs + αb*(1-αs)*Cb + αs*αb*B(Cb, Cs)
            let cs = if sa > 1e-6 {
                [
                    (src_pm[0] / sa).clamp(0.0, 1.0),
                    (src_pm[1] / sa).clamp(0.0, 1.0),
                    (src_pm[2] / sa).clamp(0.0, 1.0),
                ]
            } else {
                [0.0; 3]
            };
            let cb = if da > 1e-6 {
                [
                    (dst_pm[0] / da).clamp(0.0, 1.0),
                    (dst_pm[1] / da).clamp(0.0, 1.0),
                    (dst_pm[2] / da).clamp(0.0, 1.0),
                ]
            } else {
                [0.0; 3]
            };
            let blended = match mode {
                BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
                BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
                BlendMode::Color => set_lum(cs, lum(cb)),
                BlendMode::Luminosity => set_lum(cb, lum(cs)),
                _ => [
                    blend_channel_separable(cb[0], cs[0], mode),
                    blend_channel_separable(cb[1], cs[1], mode),
                    blend_channel_separable(cb[2], cs[2], mode),
                ],
            };
            let sada = sa * da;
            let ao = (sa + da * inv_sa).clamp(0.0, 1.0);
            [
                (src_pm[0] * inv_da + dst_pm[0] * inv_sa + sada * blended[0]).clamp(0.0, 1.0),
                (src_pm[1] * inv_da + dst_pm[1] * inv_sa + sada * blended[1]).clamp(0.0, 1.0),
                (src_pm[2] * inv_da + dst_pm[2] * inv_sa + sada * blended[2]).clamp(0.0, 1.0),
                ao,
            ]
        }
    }
}

/// 将直通 sRGB `Color` 转为归一化预乘 `[f32; 4]`。
#[inline(always)]
pub fn color_to_premul_f32(c: Color) -> [f32; 4] {
    const INV: f32 = 1.0 / 255.0;
    let a = (c.a() as f32) * INV;
    [
        (c.r() as f32) * INV * a,
        (c.g() as f32) * INV * a,
        (c.b() as f32) * INV * a,
        a,
    ]
}

/// 将归一化预乘 `[f32; 4]` 还原为直通 sRGB `Color`。
#[inline(always)]
pub fn premul_f32_to_color(pm: [f32; 4]) -> Color {
    let a = pm[3].clamp(0.0, 1.0);
    if a <= 1e-5 {
        return Color::TRANSPARENT;
    }
    let inv_a = 1.0 / a;
    Color::rgba(
        ((pm[0] * inv_a).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        ((pm[1] * inv_a).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        ((pm[2] * inv_a).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (a * 255.0 + 0.5) as u8,
    )
}

/// 以指定 [`BlendMode`] 合成两个直通 sRGB `Color`。
#[inline]
pub fn blend_colors(src: Color, dst: Color, mode: BlendMode) -> Color {
    premul_f32_to_color(blend_premul_f32(
        color_to_premul_f32(src),
        color_to_premul_f32(dst),
        mode,
    ))
}

// ============================================================================
// 4. 滤镜原语（ColorMatrix / DropShadow / LayerFilter / BackdropFilter）
// ============================================================================

/// 标准 4×5 RGBA 色彩变换矩阵（行主序，第 5 列为归一化偏移 `[-1.0..=1.0]`，对齐 Skia `SkColorMatrix` / SVG `<feColorMatrix>`）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ColorMatrix(pub [f32; 20]);

impl PartialEq for ColorMatrix {
    fn eq(&self, other: &Self) -> bool {
        for i in 0..20 {
            if canonical_f32_bits(self.0[i]) != canonical_f32_bits(other.0[i]) {
                return false;
            }
        }
        true
    }
}

impl Eq for ColorMatrix {}

impl Default for ColorMatrix {
    #[inline(always)]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl ColorMatrix {
    /// 单位矩阵（无色彩变化）。
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, 0.0, // R
        0.0, 1.0, 0.0, 0.0, 0.0, // G
        0.0, 0.0, 1.0, 0.0, 0.0, // B
        0.0, 0.0, 0.0, 1.0, 0.0, // A
    ]);

    /// 灰度滤镜（`amount ∈ [0.0, 1.0]`，`1.0` 为完全灰度）。
    #[inline]
    pub fn grayscale(amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        let (lr, lg, lb) = (0.2126 * t, 0.7152 * t, 0.0722 * t);
        Self([
            lr + inv, lg, lb, 0.0, 0.0,
            lr, lg + inv, lb, 0.0, 0.0,
            lr, lg, lb + inv, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 怀旧棕褐滤镜（`amount ∈ [0.0, 1.0]`）。
    #[inline]
    pub fn sepia(amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Self([
            inv + 0.393 * t, 0.769 * t, 0.189 * t, 0.0, 0.0,
            0.349 * t, inv + 0.686 * t, 0.168 * t, 0.0, 0.0,
            0.272 * t, 0.534 * t, inv + 0.131 * t, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 饱和度调整（`amount = 0.0` 为灰度，`1.0` 保持不变，`> 1.0` 增强鲜艳度，毛玻璃常配 `1.4~1.8`）。
    #[inline]
    pub fn saturate(amount: f32) -> Self {
        let s = amount.max(0.0);
        let inv = 1.0 - s;
        let (lr, lg, lb) = (0.2126 * inv, 0.7152 * inv, 0.0722 * inv);
        Self([
            lr + s, lg, lb, 0.0, 0.0,
            lr, lg + s, lb, 0.0, 0.0,
            lr, lg, lb + s, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 亮度调整（`1.0` 保持不变，`> 1.0` 提亮，`< 1.0` 压暗）。
    #[inline]
    pub fn brightness(amount: f32) -> Self {
        let b = amount.max(0.0);
        Self([
            b, 0.0, 0.0, 0.0, 0.0,
            0.0, b, 0.0, 0.0, 0.0,
            0.0, 0.0, b, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 对比度调整（`1.0` 保持不变，`> 1.0` 提高对比度）。
    #[inline]
    pub fn contrast(amount: f32) -> Self {
        let c = amount.max(0.0);
        let bias = 0.5 * (1.0 - c);
        Self([
            c, 0.0, 0.0, 0.0, bias,
            0.0, c, 0.0, 0.0, bias,
            0.0, 0.0, c, 0.0, bias,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 色相旋转（角度制 `deg`）。
    #[inline]
    pub fn hue_rotate(deg: f32) -> Self {
        let (s, c) = sin_cos_f32(deg * (PI / 180.0));
        Self([
            0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928, 0.0, 0.0,
            0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140, 0.072 - c * 0.072 - s * 0.283, 0.0, 0.0,
            0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 反色滤镜（`amount ∈ [0.0, 1.0]`，`1.0` 为完全反相）。
    #[inline]
    pub fn invert(amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let scale = 1.0 - 2.0 * t;
        Self([
            scale, 0.0, 0.0, 0.0, t,
            0.0, scale, 0.0, 0.0, t,
            0.0, 0.0, scale, 0.0, t,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// 复合两个色彩矩阵：先应用 `self`，再应用 `next`（等价于 `next * self`）。
    #[inline]
    pub fn then(self, next: Self) -> Self {
        let a = &next.0;
        let b = &self.0;
        let mut out = [0.0f32; 20];
        for row in 0..4 {
            let r = row * 5;
            for col in 0..4 {
                out[r + col] = a[r] * b[col]
                    + a[r + 1] * b[5 + col]
                    + a[r + 2] * b[10 + col]
                    + a[r + 3] * b[15 + col];
            }
            out[r + 4] = a[r] * b[4]
                + a[r + 1] * b[9]
                + a[r + 2] * b[14]
                + a[r + 3] * b[19]
                + a[r + 4];
        }
        Self(out)
    }

    /// 对直通 RGBA `[0.0, 1.0]` 应用色彩矩阵。
    #[inline]
    pub fn apply_straight(&self, rgba: [f32; 4]) -> [f32; 4] {
        let m = &self.0;
        let r = m[0] * rgba[0] + m[1] * rgba[1] + m[2] * rgba[2] + m[3] * rgba[3] + m[4];
        let g = m[5] * rgba[0] + m[6] * rgba[1] + m[7] * rgba[2] + m[8] * rgba[3] + m[9];
        let b = m[10] * rgba[0] + m[11] * rgba[1] + m[12] * rgba[2] + m[13] * rgba[3] + m[14];
        let a = m[15] * rgba[0] + m[16] * rgba[1] + m[17] * rgba[2] + m[18] * rgba[3] + m[19];
        [
            r.clamp(0.0, 1.0),
            g.clamp(0.0, 1.0),
            b.clamp(0.0, 1.0),
            a.clamp(0.0, 1.0),
        ]
    }

    /// 对预乘 RGBA `[0.0, 1.0]` 应用色彩矩阵（解预乘 -> 变换 -> 再预乘）。
    #[inline]
    pub fn apply_premul(&self, pm: [f32; 4]) -> [f32; 4] {
        let a = pm[3].clamp(0.0, 1.0);
        let straight = if a > 1e-5 {
            [
                (pm[0] / a).clamp(0.0, 1.0),
                (pm[1] / a).clamp(0.0, 1.0),
                (pm[2] / a).clamp(0.0, 1.0),
                a,
            ]
        } else {
            [0.0; 4]
        };
        let out = self.apply_straight(straight);
        [out[0] * out[3], out[1] * out[3], out[2] * out[3], out[3]]
    }

    /// 混入 FNV-1a 哈希。
    pub fn mix_hash(&self, h: &mut u32) {
        for &v in &self.0 {
            mix_f32(h, v);
        }
    }
}

/// 任意子树/形状投影滤镜（CSS `filter: drop-shadow(...)`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct DropShadow {
    pub blur: u8,
    pub offset_x: i8,
    pub offset_y: i8,
    pub color: Color,
}

impl DropShadow {
    #[inline(always)]
    pub const fn new(blur: u8, offset_x: i8, offset_y: i8, color: Color) -> Self {
        Self {
            blur,
            offset_x,
            offset_y,
            color,
        }
    }

    #[inline(always)]
    pub const fn is_active(self) -> bool {
        self.color.a() > 0 && (self.blur > 0 || self.offset_x != 0 || self.offset_y != 0)
    }
}

/// 作用在离屏图层内容上的后处理滤镜（`filter`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct LayerFilter {
    /// 高斯/Kawase 模糊半径（逻辑像素，`0` 表示关闭）。
    pub blur_radius: u8,
    /// 可选 4×5 色彩矩阵（灰度、饱和度、亮度、对比度、色相旋转等）。
    pub color_matrix: Option<ColorMatrix>,
    /// 可选任意形状投影（基于子树 Alpha 轮廓）。
    pub drop_shadow: Option<DropShadow>,
}

impl LayerFilter {
    pub const NONE: Self = Self {
        blur_radius: 0,
        color_matrix: None,
        drop_shadow: None,
    };

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.blur_radius == 0 && self.color_matrix.is_none() && self.drop_shadow.is_none()
    }

    /// 计算该滤镜导致绘制向外溢出的安全边界膨胀量 `(left, top, right, bottom)`（逻辑像素）。
    #[inline]
    pub const fn outset(&self) -> (i16, i16, i16, i16) {
        let mut l = (self.blur_radius as i16).saturating_mul(2);
        let mut t = l;
        let mut r = l;
        let mut b = l;
        if let Some(ds) = self.drop_shadow {
            if ds.color.a() > 0 {
                let db = (ds.blur as i16).saturating_mul(2);
                let sl = db.saturating_sub(ds.offset_x as i16);
                let sr = db.saturating_add(ds.offset_x as i16);
                let st = db.saturating_sub(ds.offset_y as i16);
                let sb = db.saturating_add(ds.offset_y as i16);
                if sl > l { l = sl; }
                if sr > r { r = sr; }
                if st > t { t = st; }
                if sb > b { b = sb; }
            }
        }
        (l, t, r, b)
    }

    pub fn mix_hash(&self, h: &mut u32) {
        mix_u8(h, self.blur_radius);
        if let Some(cm) = &self.color_matrix {
            mix_u8(h, 1);
            cm.mix_hash(h);
        } else {
            mix_u8(h, 0);
        }
        if let Some(ds) = self.drop_shadow {
            mix_u8(h, 1);
            mix_u8(h, ds.blur);
            mix_u8(h, ds.offset_x as u8);
            mix_u8(h, ds.offset_y as u8);
            mix_u32(h, ds.color.0);
        } else {
            mix_u8(h, 0);
        }
    }
}

/// 作用在元素下方已有背景上的实时背景滤镜（CSS `backdrop-filter`，用于毛玻璃 / Acrylic / Mica）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct BackdropFilter {
    /// 背景模糊半径（逻辑像素，`0` 表示不模糊）。
    pub blur_radius: u8,
    /// 可选背景色彩矩阵（如毛玻璃 `saturate(1.6)` + `brightness(1.08)`）。
    pub color_matrix: Option<ColorMatrix>,
    /// 可选叠层磨砂着色（`#no_std` 嵌入式低内存档位亦自动降级为此着色）。
    pub tint: Color,
    /// 可选 Acrylic 细微噪点颗粒强度（`0..=255`，推荐 `8..=20`）。
    pub noise_amount: u8,
}

impl BackdropFilter {
    pub const NONE: Self = Self {
        blur_radius: 0,
        color_matrix: None,
        tint: Color::TRANSPARENT,
        noise_amount: 0,
    };

    /// 快捷创建毛玻璃背景模糊。
    #[inline(always)]
    pub const fn blur(radius: u8) -> Self {
        Self {
            blur_radius: radius,
            color_matrix: None,
            tint: Color::TRANSPARENT,
            noise_amount: 0,
        }
    }

    /// 快捷创建现代 Acrylic / Vibrancy 毛玻璃（模糊 + 提饱和 + 细微颗粒 + 着色）。
    #[inline]
    pub fn acrylic(blur_radius: u8, tint: Color) -> Self {
        Self {
            blur_radius,
            color_matrix: Some(ColorMatrix::saturate(1.45)),
            tint,
            noise_amount: 12,
        }
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.blur_radius == 0
            && self.color_matrix.is_none()
            && self.tint.a() == 0
            && self.noise_amount == 0
    }

    pub fn mix_hash(&self, h: &mut u32) {
        mix_u8(h, self.blur_radius);
        mix_u32(h, self.tint.0);
        mix_u8(h, self.noise_amount);
        if let Some(cm) = &self.color_matrix {
            mix_u8(h, 1);
            cm.mix_hash(h);
        } else {
            mix_u8(h, 0);
        }
    }
}

// ============================================================================
// 5. 冷路径节点特效包与高级 DrawOp 载荷（分配在 ScratchArena，常规节点 0 字节开销）
// ============================================================================

/// 外部零拷贝 GPU 纹理槽位 ID（如海图渲染引擎、视频帧或 3D 视口注册至 `WgpuRenderer` 的共享纹理标识）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub struct ExternalTextureId(pub u32);

/// 外部零拷贝纹理绘制描述（分配于 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::ExternalTexture`，零膨胀 `DrawOp`）。
#[derive(Copy, Clone, Debug)]
pub struct ExternalTextureSpec {
    /// 外部纹理注册表中的句柄 ID。
    pub id: ExternalTextureId,
    /// 归一化采样 UV 矩形 `[u0, v0, u1, v1]`（默认 `[0.0, 0.0, 1.0, 1.0]`）。
    pub uv_rect: [f32; 4],
    /// 四角独立圆角裁切（逻辑像素，复用 GPU SDF 抗锯齿圆角裁切）。
    pub radii: CornerRadii,
    /// 整体不透明度（`0..=255`）。
    pub opacity: u8,
    /// 外部纹理是否已为预乘 Alpha（`false` 时采样后自动执行预乘）。
    pub premultiplied: bool,
    /// 与下方画布的合成混合模式（默认 `BlendMode::SrcOver`，支持全部 27 种混合）。
    pub blend_mode: BlendMode,
    /// 外部画面内容版本号（同一纹理被外部引擎重绘时递增此值以精确触发 `DamageTracker` 局部刷新）。
    pub content_revision: u64,
    /// 软件光栅器或无 GPU 环境下显示的占位回退颜色。
    pub fallback_bg: Color,
}

impl ExternalTextureSpec {
    /// 按外部纹理 ID 构造默认全幅（`[0,0,1,1]`）绘制描述。
    #[inline(always)]
    pub const fn new(id: ExternalTextureId) -> Self {
        Self {
            id,
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            radii: CornerRadii::ZERO,
            opacity: 255,
            premultiplied: true,
            blend_mode: BlendMode::SrcOver,
            content_revision: 0,
            fallback_bg: Color::rgb(12, 18, 28),
        }
    }

    #[inline(always)]
    pub const fn with_uv(mut self, uv_rect: [f32; 4]) -> Self {
        self.uv_rect = uv_rect;
        self
    }

    #[inline(always)]
    pub const fn with_radii(mut self, radii: CornerRadii) -> Self {
        self.radii = radii;
        self
    }

    #[inline(always)]
    pub const fn with_opacity(mut self, opacity: u8) -> Self {
        self.opacity = opacity;
        self
    }

    #[inline(always)]
    pub const fn with_premultiplied(mut self, premultiplied: bool) -> Self {
        self.premultiplied = premultiplied;
        self
    }

    #[inline(always)]
    pub const fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    #[inline(always)]
    pub const fn with_revision(mut self, rev: u64) -> Self {
        self.content_revision = rev;
        self
    }

    #[inline(always)]
    pub const fn with_fallback_bg(mut self, color: Color) -> Self {
        self.fallback_bg = color;
        self
    }

    pub fn mix_hash(&self, h: &mut u32) {
        mix_u32(h, self.id.0);
        for v in self.uv_rect {
            mix_u32(h, canonical_f32_bits(v));
        }
        mix_u8(h, self.radii.tl);
        mix_u8(h, self.radii.tr);
        mix_u8(h, self.radii.br);
        mix_u8(h, self.radii.bl);
        mix_u8(h, self.opacity);
        mix_u8(h, if self.premultiplied { 1 } else { 0 });
        mix_u8(h, self.blend_mode as u8);
        mix_u32(h, self.content_revision as u32);
        mix_u32(h, (self.content_revision >> 32) as u32);
        mix_u32(h, self.fallback_bg.0);
    }
}

impl PartialEq for ExternalTextureSpec {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && canonical_f32_bits(self.uv_rect[0]) == canonical_f32_bits(other.uv_rect[0])
            && canonical_f32_bits(self.uv_rect[1]) == canonical_f32_bits(other.uv_rect[1])
            && canonical_f32_bits(self.uv_rect[2]) == canonical_f32_bits(other.uv_rect[2])
            && canonical_f32_bits(self.uv_rect[3]) == canonical_f32_bits(other.uv_rect[3])
            && self.radii == other.radii
            && self.opacity == other.opacity
            && self.premultiplied == other.premultiplied
            && self.blend_mode == other.blend_mode
            && self.content_revision == other.content_revision
            && self.fallback_bg == other.fallback_bg
    }
}

impl Eq for ExternalTextureSpec {}

/// 按需分配于 `ScratchArena` 的 `Div` 高级渲染特效包（多色标渐变、独立圆角、混合模式、滤镜、2D 变换、路径裁剪与外部零拷贝纹理）。
/// 冷路径高级布局扩展（按需挂接于 [`PaintEffects::layout_ext`]，常规节点 0 字节占用）：
/// 支持 `aspect-ratio` 纵横比、CSS Grid 跨列与 `fr` 列权重、`position: sticky` 吸顶、RTL 镜像与嵌套滚动链。
#[derive(Copy, Clone, Debug)]
pub struct LayoutExtras<'a> {
    /// 纵横比约束 `width / height`（如 `16.0 / 9.0`，`<= 0.0` 表示关闭）。
    pub aspect_ratio: f32,
    /// CSS Grid 子项跨列数（默认 `1`）。
    pub grid_col_span: u8,
    /// CSS Grid 自定义列权重数组（如 `&[1, 2, 1]` 对应 `1fr 2fr 1fr`）。
    pub grid_template_fr: &'a [u16],
    /// `position: sticky` 相对最近滚动视口顶部的吸顶偏移（`None` 表示普通流式排布）。
    pub sticky_top: Option<i16>,
    /// RTL 布局镜像（`FlexDir::Row` 自动水平从右向左排布子元素）。
    pub rtl: bool,
    /// 嵌套滚动链：内层滚动触顶/触底时是否继续向上冒泡给外层滚动容器（默认 `true`）。
    pub scroll_chaining: bool,
}

impl<'a> LayoutExtras<'a> {
    pub const EMPTY: Self = Self {
        aspect_ratio: 0.0,
        grid_col_span: 1,
        grid_template_fr: &[],
        sticky_top: None,
        rtl: false,
        scroll_chaining: true,
    };

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.aspect_ratio <= 0.0
            && self.grid_col_span <= 1
            && self.grid_template_fr.is_empty()
            && self.sticky_top.is_none()
            && !self.rtl
            && self.scroll_chaining
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct PaintEffects<'a> {
    pub bg_paint: Option<Paint<'a>>,
    pub corner_radii: Option<CornerRadii>,
    pub blend_mode: BlendMode,
    pub isolate: bool,
    pub filter: LayerFilter,
    pub backdrop_filter: BackdropFilter,
    pub transform: Option<Transform2D>,
    pub clip_path: Option<StrRef<'a>>,
    pub external_texture: Option<&'a ExternalTextureSpec>,
    pub layout_ext: Option<&'a LayoutExtras<'a>>,
}

impl<'a> PaintEffects<'a> {
    pub const EMPTY: Self = Self {
        bg_paint: None,
        corner_radii: None,
        blend_mode: BlendMode::SrcOver,
        isolate: false,
        filter: LayerFilter::NONE,
        backdrop_filter: BackdropFilter::NONE,
        transform: None,
        clip_path: None,
        external_texture: None,
        layout_ext: None,
    };

    /// 判断是否未配置任何高级视觉特效。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bg_paint.is_none()
            && self.corner_radii.is_none()
            && self.blend_mode.is_normal()
            && !self.isolate
            && self.filter.is_empty()
            && self.backdrop_filter.is_empty()
            && self.transform.is_none()
            && self.clip_path.is_none()
            && self.external_texture.is_none()
            && self.layout_ext.is_none()
    }

    /// 判断是否需要创建独立离屏合成图层（`PushLayer` / `PopLayer`）。
    #[inline]
    pub fn needs_layer(&self, has_children: bool, opacity: u8) -> bool {
        !self.filter.is_empty()
            || !self.backdrop_filter.is_empty()
            || self.clip_path.is_some()
            || (!self.blend_mode.is_normal() && has_children)
            || (self.isolate && opacity < 255)
            || (self.transform.is_some() && has_children)
    }
}

/// 高级矩形绘制载荷（分配于 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::StyledQuad`）。
#[derive(Copy, Clone, Debug)]
pub struct StyledQuad<'a> {
    pub paint: Paint<'a>,
    pub border_color: Color,
    pub border_width: u8,
    pub radii: CornerRadii,
    pub opacity: u8,
    pub blend_mode: BlendMode,
    pub transform: Option<Transform2D>,
}

impl<'a> PartialEq for StyledQuad<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.paint == other.paint
            && self.border_color == other.border_color
            && self.border_width == other.border_width
            && self.radii == other.radii
            && self.opacity == other.opacity
            && self.blend_mode == other.blend_mode
            && match (self.transform, other.transform) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    canonical_f32_bits(a.a) == canonical_f32_bits(b.a)
                        && canonical_f32_bits(a.b) == canonical_f32_bits(b.b)
                        && canonical_f32_bits(a.c) == canonical_f32_bits(b.c)
                        && canonical_f32_bits(a.d) == canonical_f32_bits(b.d)
                        && canonical_f32_bits(a.e) == canonical_f32_bits(b.e)
                        && canonical_f32_bits(a.f) == canonical_f32_bits(b.f)
                }
                _ => false,
            }
    }
}

impl<'a> Eq for StyledQuad<'a> {}

impl<'a> StyledQuad<'a> {
    pub fn mix_hash(&self, h: &mut u32) {
        self.paint.mix_hash(h);
        mix_u32(h, self.border_color.0);
        mix_u8(h, self.border_width);
        mix_u8(h, self.radii.tl);
        mix_u8(h, self.radii.tr);
        mix_u8(h, self.radii.br);
        mix_u8(h, self.radii.bl);
        mix_u8(h, self.opacity);
        mix_u8(h, self.blend_mode as u8);
        if let Some(t) = self.transform {
            mix_u8(h, 1);
            mix_transform(h, t);
        } else {
            mix_u8(h, 0);
        }
    }
}

/// 隔离合成图层规格（分配于 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::PushLayer`）。
#[derive(Copy, Clone, Debug)]
pub struct LayerSpec<'a> {
    /// 图层逻辑范围（用于离屏纹理尺寸估算与圆角/路径掩码坐标系）。
    pub bounds: Region,
    /// 图层四角独立圆角裁切（全 0 表示矩形裁切）。
    pub clip_radii: CornerRadii,
    /// 可选任意 SVG Path 裁剪/遮罩路径字符串（在图层局部 `[0..w, 0..h]` 坐标系内评估）。
    pub clip_path: Option<StrRef<'a>>,
    /// 组级隔离不透明度（`0..=255`）。
    pub opacity: u8,
    /// 图层合成混合模式（27 种 Porter-Duff / W3C 混合）。
    pub blend_mode: BlendMode,
    /// 图层后处理滤镜（模糊、色彩矩阵、任意形状投影）。
    pub filter: LayerFilter,
    /// 图层下方背景滤镜（实时毛玻璃、饱和度增强、颗粒感）。
    pub backdrop_filter: BackdropFilter,
    /// 可选图层 2D 仿射变换（作用于整个子树，以图层中心为原点）。
    pub transform: Option<Transform2D>,
}

impl<'a> PartialEq for LayerSpec<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.bounds == other.bounds
            && self.clip_radii == other.clip_radii
            && self.clip_path == other.clip_path
            && self.opacity == other.opacity
            && self.blend_mode == other.blend_mode
            && self.filter == other.filter
            && self.backdrop_filter == other.backdrop_filter
            && match (self.transform, other.transform) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    canonical_f32_bits(a.a) == canonical_f32_bits(b.a)
                        && canonical_f32_bits(a.b) == canonical_f32_bits(b.b)
                        && canonical_f32_bits(a.c) == canonical_f32_bits(b.c)
                        && canonical_f32_bits(a.d) == canonical_f32_bits(b.d)
                        && canonical_f32_bits(a.e) == canonical_f32_bits(b.e)
                        && canonical_f32_bits(a.f) == canonical_f32_bits(b.f)
                }
                _ => false,
            }
    }
}

impl<'a> Eq for LayerSpec<'a> {}

impl<'a> LayerSpec<'a> {
    /// 计算包含模糊核、阴影与仿射变换膨胀后的有效脏区矩形（防止局部重绘出现模糊接缝）。
    pub fn expanded_bounds(&self) -> Region {
        let mut reg = if let Some(t) = self.transform {
            let cx = self.bounds.x as f32 + self.bounds.w as f32 * 0.5;
            let cy = self.bounds.y as f32 + self.bounds.h as f32 * 0.5;
            t.centered_at(cx, cy).transform_region(self.bounds)
        } else {
            self.bounds
        };
        let (fl, ft, fr, fb) = self.filter.outset();
        let bd_pad = (self.backdrop_filter.blur_radius as i16).saturating_mul(2);
        let l = fl.max(bd_pad);
        let t = ft.max(bd_pad);
        let r = fr.max(bd_pad);
        let b = fb.max(bd_pad);
        if l > 0 || t > 0 || r > 0 || b > 0 {
            reg = Region::new(
                reg.x.saturating_sub(l),
                reg.y.saturating_sub(t),
                reg.w.saturating_add(l).saturating_add(r),
                reg.h.saturating_add(t).saturating_add(b),
            );
        }
        reg
    }

    pub fn mix_hash(&self, h: &mut u32) {
        mix_u32(h, ((self.bounds.x as u16 as u32) << 16) | (self.bounds.y as u16 as u32));
        mix_u32(h, ((self.bounds.w as u16 as u32) << 16) | (self.bounds.h as u16 as u32));
        mix_u8(h, self.clip_radii.tl);
        mix_u8(h, self.clip_radii.tr);
        mix_u8(h, self.clip_radii.br);
        mix_u8(h, self.clip_radii.bl);
        mix_u8(h, self.opacity);
        mix_u8(h, self.blend_mode as u8);
        self.filter.mix_hash(h);
        self.backdrop_filter.mix_hash(h);
        if let Some(cp) = self.clip_path {
            mix_u8(h, 1);
            for &b in cp.as_str().as_bytes() {
                mix_u8(h, b);
            }
        } else {
            mix_u8(h, 0);
        }
        if let Some(t) = self.transform {
            mix_u8(h, 1);
            mix_transform(h, t);
        } else {
            mix_u8(h, 0);
        }
    }
}
