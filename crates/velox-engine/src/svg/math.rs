//! `#![no_std]` 兼容的轻量浮点数学原语与 SVG 2D 仿射变换矩阵（`Transform2D`）。

/// 圆周率 $\pi$。
pub const PI: f32 = core::f32::consts::PI;
/// $2\pi$。
pub const TAU: f32 = core::f32::consts::TAU;

/// 绝对值（零 `std` 依赖）。
#[inline(always)]
pub fn abs_f32(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7FFF_FFFF)
}

/// 向下取整。
#[inline]
pub fn floor_f32(x: f32) -> f32 {
    {
        x.floor()
    }
}

/// 向上取整。
#[inline]
pub fn ceil_f32(x: f32) -> f32 {
    {
        x.ceil()
    }
}

/// 四舍五入。
#[inline]
pub fn round_f32(x: f32) -> f32 {
    {
        x.round()
    }
}

/// 平方根。
#[inline]
pub fn sqrt_f32(x: f32) -> f32 {
    {
        if x <= 0.0 {
            0.0
        } else {
            x.sqrt()
        }
    }
}

/// 同时计算 `(sin(rad), cos(rad))`。
#[inline]
pub fn sin_cos_f32(rad: f32) -> (f32, f32) {
    {
        rad.sin_cos()
    }
}

/// 正切 `tan(rad)`。
#[inline]
pub fn tan_f32(rad: f32) -> f32 {
    let (s, c) = sin_cos_f32(rad);
    if abs_f32(c) < 1e-6 {
        if s >= 0.0 {
            1e6
        } else {
            -1e6
        }
    } else {
        s / c
    }
}

/// 反正切 `atan2(y, x)`。
#[inline]
pub fn atan2_f32(y: f32, x: f32) -> f32 {
    {
        y.atan2(x)
    }
}

/// 2D 仿射变换矩阵，按 SVG 规范表示：
///
/// $$
/// \begin{bmatrix} x' \\ y' \\ 1 \end{bmatrix}
/// =
/// \begin{bmatrix} a & c & e \\ b & d & f \\ 0 & 0 & 1 \end{bmatrix}
/// \begin{bmatrix} x \\ y \\ 1 \end{bmatrix}
/// $$
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Transform2D {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Transform2D {
    #[inline(always)]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    /// 单位矩阵。
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// 构造任意 2D 仿射矩阵 `[a, b, c, d, e, f]`。
    #[inline(always)]
    pub const fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// 平移变换。
    #[inline(always)]
    pub const fn translate(tx: f32, ty: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// 缩放变换。
    #[inline(always)]
    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    /// 绕原点顺时针旋转（角度制 `deg`）。
    #[inline]
    pub fn rotate(deg: f32) -> Self {
        let (s, c) = sin_cos_f32(deg * (PI / 180.0));
        Self {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    /// 绕指定中心 `(cx, cy)` 顺时针旋转（角度制 `deg`）。
    #[inline]
    pub fn rotate_around(deg: f32, cx: f32, cy: f32) -> Self {
        Self::translate(cx, cy)
            .concat(Self::rotate(deg))
            .concat(Self::translate(-cx, -cy))
    }

    /// 沿 X 轴错切（角度制 `deg`）。
    #[inline]
    pub fn skew_x(deg: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: tan_f32(deg * (PI / 180.0)),
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// 沿 Y 轴错切（角度制 `deg`）。
    #[inline]
    pub fn skew_y(deg: f32) -> Self {
        Self {
            a: 1.0,
            b: tan_f32(deg * (PI / 180.0)),
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// 矩阵右乘 `self * rhs`（先应用 `rhs`，再应用 `self`，符合 SVG 父子嵌套顺序）。
    #[inline]
    pub fn concat(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            e: self.a * rhs.e + self.c * rhs.f + self.e,
            f: self.b * rhs.e + self.d * rhs.f + self.f,
        }
    }

    /// 将点 `(x, y)` 变换至目标坐标系。
    #[inline(always)]
    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// 估算该仿射变换对应的平均各向同性缩放系数（用于缩放 `stroke-width`）。
    #[inline]
    pub fn approx_scale(self) -> f32 {
        let sx = sqrt_f32(self.a * self.a + self.b * self.b);
        let sy = sqrt_f32(self.c * self.c + self.d * self.d);
        let avg = 0.5 * (sx + sy);
        if avg > 1e-5 {
            avg
        } else {
            1.0
        }
    }

    /// 解析 SVG `transform` 属性字符串（支持 `translate`/`scale`/`rotate`/`skewX`/`skewY`/`matrix` 任意组合）。
    pub fn parse_svg_attr(input: &str) -> Self {
        let bytes = input.as_bytes();
        let mut pos = 0usize;
        let mut acc = Self::IDENTITY;

        while pos < bytes.len() {
            while pos < bytes.len() && is_sep(bytes[pos]) {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }
            let name_start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_alphabetic() {
                pos += 1;
            }
            let name = &input[name_start..pos];
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= bytes.len() || bytes[pos] != b'(' {
                // 跳过非预期字符避免死循环
                pos += 1;
                continue;
            }
            pos += 1; // skip '('
            let args_start = pos;
            while pos < bytes.len() && bytes[pos] != b')' {
                pos += 1;
            }
            let args_str = &input[args_start..pos];
            if pos < bytes.len() && bytes[pos] == b')' {
                pos += 1;
            }

            let mut nums = [0.0f32; 6];
            let count = parse_number_list(args_str, &mut nums);
            let step = if name.eq_ignore_ascii_case("translate") {
                if count >= 2 {
                    Self::translate(nums[0], nums[1])
                } else if count == 1 {
                    Self::translate(nums[0], 0.0)
                } else {
                    Self::IDENTITY
                }
            } else if name.eq_ignore_ascii_case("scale") {
                if count >= 2 {
                    Self::scale(nums[0], nums[1])
                } else if count == 1 {
                    Self::scale(nums[0], nums[0])
                } else {
                    Self::IDENTITY
                }
            } else if name.eq_ignore_ascii_case("rotate") {
                if count >= 3 {
                    Self::rotate_around(nums[0], nums[1], nums[2])
                } else if count >= 1 {
                    Self::rotate(nums[0])
                } else {
                    Self::IDENTITY
                }
            } else if name.eq_ignore_ascii_case("skewX") {
                if count >= 1 {
                    Self::skew_x(nums[0])
                } else {
                    Self::IDENTITY
                }
            } else if name.eq_ignore_ascii_case("skewY") {
                if count >= 1 {
                    Self::skew_y(nums[0])
                } else {
                    Self::IDENTITY
                }
            } else if name.eq_ignore_ascii_case("matrix") && count >= 6 {
                Self::new(nums[0], nums[1], nums[2], nums[3], nums[4], nums[5])
            } else {
                Self::IDENTITY
            };

            acc = acc.concat(step);
        }

        acc
    }
}

#[inline(always)]
fn is_sep(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == b','
}

/// 从字符串中顺序提取最多 `out.len()` 个浮点数（供 `transform`、`viewBox` 等共用）。
pub(crate) fn parse_number_list(s: &str, out: &mut [f32]) -> usize {
    let mut count = 0usize;
    let mut cur = s;
    while count < out.len() {
        let Some((val, rest)) = parse_next_f32(cur) else {
            break;
        };
        out[count] = val;
        count += 1;
        cur = rest;
    }
    count
}

/// 跳过空白与逗号后解析下一个 SVG 浮点数（支持科学计数法与省略前导零如 `.5`、`-0.25`、`1e-3`）。
pub(crate) fn parse_next_f32(s: &str) -> Option<(f32, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && is_sep(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i;
    if bytes[i] == b'+' || bytes[i] == b'-' {
        i += 1;
    }
    let mut has_digits = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let exp_pos = i;
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut exp_digits = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            exp_digits = true;
            i += 1;
        }
        if !exp_digits {
            i = exp_pos;
        }
    }
    let token = &s[start..i];
    let val = token.parse::<f32>().ok()?;
    Some((val, &s[i..]))
}

/// 解析单个 SVG 长度值（忽略 `px`/`pt`/`%` 等常见单位后缀，提取数值部分）。
pub(crate) fn parse_svg_length(s: &str) -> Option<f32> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    parse_next_f32(trimmed).map(|(v, _)| v)
}
