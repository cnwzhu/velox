//! SVG / CSS 颜色与绘制源（`SvgPaint`）零堆解析器。

use crate::style::Color;

use super::math::{parse_next_f32, round_f32};

/// SVG `fill` / `stroke` 属性绘制源。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SvgPaint {
    /// `none`：不绘制。
    None,
    /// `currentColor`：继承宿主传入的着色颜色（如 `Img::tint` 或 `Icon::color`）。
    CurrentColor,
    /// 显式 RGBA 颜色。
    Color(Color),
}

impl SvgPaint {
    /// 将当前绘制源结合 `current_color` 与不透明度 `opacity`（`0.0..=1.0`）解析为最终 [`Color`]。
    #[inline]
    pub fn resolve(self, current_color: Color, opacity: f32) -> Option<Color> {
        let base = match self {
            Self::None => return None,
            Self::CurrentColor => current_color,
            Self::Color(c) => c,
        };
        let col = apply_opacity(base, opacity);
        if col.a() == 0 {
            None
        } else {
            Some(col)
        }
    }
}

/// 将 `0.0..=1.0` 浮点不透明度乘算至颜色 Alpha 通道。
#[inline]
pub fn apply_opacity(color: Color, opacity: f32) -> Color {
    let op = opacity.clamp(0.0, 1.0);
    if op >= 0.999 {
        return color;
    }
    if op <= 0.001 || color.a() == 0 {
        return Color::rgba(color.r(), color.g(), color.b(), 0);
    }
    let a = round_f32((color.a() as f32) * op).clamp(0.0, 255.0) as u8;
    Color::rgba(color.r(), color.g(), color.b(), a)
}

/// 解析 SVG `fill` / `stroke` 绘制属性值（支持 `none`、`currentColor`、十六进制、`rgb()`/`rgba()` 与命名色）。
pub fn parse_svg_paint(input: &str) -> Option<SvgPaint> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("none") {
        return Some(SvgPaint::None);
    }
    if s.eq_ignore_ascii_case("currentcolor") {
        return Some(SvgPaint::CurrentColor);
    }
    // 忽略 url(#...) 渐变引用，暂回退 None（不误画纯黑）
    if s.len() >= 4 && s[..4].eq_ignore_ascii_case("url(") {
        return Some(SvgPaint::None);
    }
    parse_svg_color(s).map(SvgPaint::Color)
}

/// 解析 SVG / CSS 颜色字面量（支持 `#RGB`/`#RGBA`/`#RRGGBB`/`#RRGGBBAA`、`rgb()`、`rgba()`、`transparent` 与常见命名色）。
pub fn parse_svg_color(input: &str) -> Option<Color> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if s.len() >= 5 && s[..4].eq_ignore_ascii_case("rgb(") && s.ends_with(')') {
        return parse_rgb_fn(&s[4..s.len() - 1], false);
    }
    if s.len() >= 6 && s[..5].eq_ignore_ascii_case("rgba(") && s.ends_with(')') {
        return parse_rgb_fn(&s[5..s.len() - 1], true);
    }
    parse_named_color(s)
}

#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[inline]
fn hex_byte(hi: u8, lo: u8) -> Option<u8> {
    Some((hex_nibble(hi)? << 4) | hex_nibble(lo)?)
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let b = hex.as_bytes();
    match b.len() {
        3 => {
            let r = hex_nibble(b[0])? * 17;
            let g = hex_nibble(b[1])? * 17;
            let bl = hex_nibble(b[2])? * 17;
            Some(Color::rgb(r, g, bl))
        }
        4 => {
            let r = hex_nibble(b[0])? * 17;
            let g = hex_nibble(b[1])? * 17;
            let bl = hex_nibble(b[2])? * 17;
            let a = hex_nibble(b[3])? * 17;
            Some(Color::rgba(r, g, bl, a))
        }
        6 => {
            let r = hex_byte(b[0], b[1])?;
            let g = hex_byte(b[2], b[3])?;
            let bl = hex_byte(b[4], b[5])?;
            Some(Color::rgb(r, g, bl))
        }
        8 => {
            let r = hex_byte(b[0], b[1])?;
            let g = hex_byte(b[2], b[3])?;
            let bl = hex_byte(b[4], b[5])?;
            let a = hex_byte(b[6], b[7])?;
            Some(Color::rgba(r, g, bl, a))
        }
        _ => None,
    }
}

fn parse_rgb_channel(s: &str) -> Option<(u8, &str)> {
    let (v, rest) = parse_next_f32(s)?;
    let trimmed = rest.trim_start();
    if let Some(after_pct) = trimmed.strip_prefix('%') {
        let px = round_f32((v.clamp(0.0, 100.0) / 100.0) * 255.0) as u8;
        Some((px, after_pct))
    } else {
        Some((round_f32(v.clamp(0.0, 255.0)) as u8, rest))
    }
}

fn parse_rgb_fn(args: &str, expect_alpha: bool) -> Option<Color> {
    let (r, rest) = parse_rgb_channel(args)?;
    let (g, rest) = parse_rgb_channel(rest)?;
    let (b, rest) = parse_rgb_channel(rest)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if let Some((a_raw, tail)) = parse_next_f32(rest) {
        let a = if tail.trim_start().starts_with('%') {
            round_f32((a_raw.clamp(0.0, 100.0) / 100.0) * 255.0) as u8
        } else if a_raw <= 1.0 {
            round_f32(a_raw.clamp(0.0, 1.0) * 255.0) as u8
        } else {
            round_f32(a_raw.clamp(0.0, 255.0)) as u8
        };
        Some(Color::rgba(r, g, b, a))
    } else if expect_alpha {
        None
    } else {
        Some(Color::rgb(r, g, b))
    }
}

fn parse_named_color(s: &str) -> Option<Color> {
    const NAMED: &[(&str, Color)] = &[
        ("transparent", Color::TRANSPARENT),
        ("black", Color::rgb(0, 0, 0)),
        ("white", Color::rgb(255, 255, 255)),
        ("red", Color::rgb(255, 0, 0)),
        ("green", Color::rgb(0, 128, 0)),
        ("lime", Color::rgb(0, 255, 0)),
        ("blue", Color::rgb(0, 0, 255)),
        ("yellow", Color::rgb(255, 255, 0)),
        ("cyan", Color::rgb(0, 255, 255)),
        ("aqua", Color::rgb(0, 255, 255)),
        ("magenta", Color::rgb(255, 0, 255)),
        ("fuchsia", Color::rgb(255, 0, 255)),
        ("gray", Color::rgb(128, 128, 128)),
        ("grey", Color::rgb(128, 128, 128)),
        ("darkgray", Color::rgb(169, 169, 169)),
        ("darkgrey", Color::rgb(169, 169, 169)),
        ("lightgray", Color::rgb(211, 211, 211)),
        ("lightgrey", Color::rgb(211, 211, 211)),
        ("silver", Color::rgb(192, 192, 192)),
        ("maroon", Color::rgb(128, 0, 0)),
        ("olive", Color::rgb(128, 128, 0)),
        ("teal", Color::rgb(0, 128, 128)),
        ("navy", Color::rgb(0, 0, 128)),
        ("purple", Color::rgb(128, 0, 128)),
        ("orange", Color::rgb(255, 165, 0)),
        ("gold", Color::rgb(255, 215, 0)),
        ("pink", Color::rgb(255, 192, 203)),
        ("coral", Color::rgb(255, 127, 80)),
        ("crimson", Color::rgb(220, 20, 60)),
        ("indigo", Color::rgb(75, 0, 130)),
        ("violet", Color::rgb(238, 130, 238)),
        ("skyblue", Color::rgb(135, 206, 235)),
        ("steelblue", Color::rgb(70, 130, 180)),
        ("slategray", Color::rgb(112, 128, 144)),
        ("slategrey", Color::rgb(112, 128, 144)),
        ("tomato", Color::rgb(255, 99, 71)),
        ("turquoise", Color::rgb(64, 224, 208)),
        ("dodgerblue", Color::rgb(30, 144, 255)),
        ("seagreen", Color::rgb(46, 139, 87)),
        ("rebeccapurple", Color::rgb(102, 51, 153)),
    ];
    for &(name, col) in NAMED {
        if s.eq_ignore_ascii_case(name) {
            return Some(col);
        }
    }
    None
}
