//! 零外部依赖流式 SVG XML 解析器（支持 `<svg>`、`<g>` 嵌套属性/变换继承与全部基础几何图形）。

use crate::display::PixelFormat;
use crate::solver::Region;
use crate::style::Color;

use super::color::{parse_svg_color, parse_svg_paint, SvgPaint};
use super::math::{parse_number_list, parse_svg_length, Transform2D};
use super::path::FlattenedPath;
use super::raster::{fill_path, stroke_path, SvgFillRule, SvgLineCap, SvgLineJoin};

/// SVG `viewBox` 视口矩形（`min_x`, `min_y`, `width`, `height`）。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SvgViewBox {
    pub min_x: f32,
    pub min_y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for SvgViewBox {
    #[inline(always)]
    fn default() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            width: 24.0,
            height: 24.0,
        }
    }
}

impl SvgViewBox {
    /// 计算将 `viewBox` 等比居中适配（`xMidYMid meet`）到目标像素矩形 `bounds` 的仿射变换矩阵。
    pub fn fit_to_bounds(self, bounds: Region) -> Transform2D {
        let bw = bounds.w.max(1) as f32;
        let bh = bounds.h.max(1) as f32;
        let vw = if self.width > 1e-5 { self.width } else { 24.0 };
        let vh = if self.height > 1e-5 { self.height } else { 24.0 };
        let scale = (bw / vw).min(bh / vh);
        let ox = (bounds.x as f32) + 0.5 * (bw - vw * scale) - self.min_x * scale;
        let oy = (bounds.y as f32) + 0.5 * (bh - vh * scale) - self.min_y * scale;
        Transform2D::new(scale, 0.0, 0.0, scale, ox, oy)
    }
}

/// SVG 光栅化渲染选项。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SvgRenderOptions {
    /// 用于解析 `currentColor` 的前景色，其 Alpha 通道同时作为整幅 SVG 的整体不透明度。
    pub tint: Color,
    /// 可选描边宽度覆盖（在 SVG 用户坐标系单位下；`None` 表示遵循 SVG 内置 `stroke-width`）。
    pub stroke_width_override: Option<f32>,
}

impl Default for SvgRenderOptions {
    #[inline(always)]
    fn default() -> Self {
        Self {
            tint: Color::WHITE,
            stroke_width_override: None,
        }
    }
}

#[derive(Copy, Clone)]
struct SvgStyleState {
    transform: Transform2D,
    fill: SvgPaint,
    fill_opacity: f32,
    fill_rule: SvgFillRule,
    stroke: SvgPaint,
    stroke_width: f32,
    stroke_opacity: f32,
    stroke_linecap: SvgLineCap,
    stroke_linejoin: SvgLineJoin,
    stroke_miterlimit: f32,
    opacity: f32,
    current_color: Color,
    visible: bool,
}

impl SvgStyleState {
    fn root(options: SvgRenderOptions, root_xf: Transform2D) -> Self {
        Self {
            transform: root_xf,
            fill: SvgPaint::Color(Color::BLACK),
            fill_opacity: 1.0,
            fill_rule: SvgFillRule::NonZero,
            stroke: SvgPaint::None,
            stroke_width: options.stroke_width_override.unwrap_or(1.0),
            stroke_opacity: 1.0,
            stroke_linecap: SvgLineCap::Butt,
            stroke_linejoin: SvgLineJoin::Miter,
            stroke_miterlimit: 4.0,
            opacity: (options.tint.a() as f32) / 255.0,
            current_color: Color::rgb(options.tint.r(), options.tint.g(), options.tint.b()),
            visible: true,
        }
    }

    fn apply_attr(&mut self, name: &str, value: &str, stroke_width_override: Option<f32>) {
        let v = value.trim();
        if name.eq_ignore_ascii_case("fill") {
            if let Some(p) = parse_svg_paint(v) {
                self.fill = p;
            }
        } else if name.eq_ignore_ascii_case("fill-opacity") {
            if let Some(n) = parse_svg_length(v) {
                self.fill_opacity = n.clamp(0.0, 1.0);
            }
        } else if name.eq_ignore_ascii_case("fill-rule") {
            if v.eq_ignore_ascii_case("evenodd") {
                self.fill_rule = SvgFillRule::EvenOdd;
            } else if v.eq_ignore_ascii_case("nonzero") {
                self.fill_rule = SvgFillRule::NonZero;
            }
        } else if name.eq_ignore_ascii_case("stroke") {
            if let Some(p) = parse_svg_paint(v) {
                self.stroke = p;
            }
        } else if name.eq_ignore_ascii_case("stroke-width") {
            if stroke_width_override.is_none() {
                if let Some(w) = parse_svg_length(v) {
                    self.stroke_width = w.max(0.0);
                }
            }
        } else if name.eq_ignore_ascii_case("stroke-opacity") {
            if let Some(n) = parse_svg_length(v) {
                self.stroke_opacity = n.clamp(0.0, 1.0);
            }
        } else if name.eq_ignore_ascii_case("stroke-linecap") {
            if v.eq_ignore_ascii_case("round") {
                self.stroke_linecap = SvgLineCap::Round;
            } else if v.eq_ignore_ascii_case("square") {
                self.stroke_linecap = SvgLineCap::Square;
            } else if v.eq_ignore_ascii_case("butt") {
                self.stroke_linecap = SvgLineCap::Butt;
            }
        } else if name.eq_ignore_ascii_case("stroke-linejoin") {
            if v.eq_ignore_ascii_case("round") {
                self.stroke_linejoin = SvgLineJoin::Round;
            } else if v.eq_ignore_ascii_case("bevel") {
                self.stroke_linejoin = SvgLineJoin::Bevel;
            } else if v.eq_ignore_ascii_case("miter") {
                self.stroke_linejoin = SvgLineJoin::Miter;
            }
        } else if name.eq_ignore_ascii_case("stroke-miterlimit") {
            if let Some(m) = parse_svg_length(v) {
                self.stroke_miterlimit = m.max(1.0);
            }
        } else if name.eq_ignore_ascii_case("opacity") {
            if let Some(n) = parse_svg_length(v) {
                self.opacity *= n.clamp(0.0, 1.0);
            }
        } else if name.eq_ignore_ascii_case("color") {
            if let Some(c) = parse_svg_color(v) {
                self.current_color = c;
            }
        } else if name.eq_ignore_ascii_case("transform") {
            let local = Transform2D::parse_svg_attr(v);
            self.transform = self.transform.concat(local);
        } else if name.eq_ignore_ascii_case("display") {
            if v.eq_ignore_ascii_case("none") {
                self.visible = false;
            }
        } else if name.eq_ignore_ascii_case("visibility") {
            if v.eq_ignore_ascii_case("hidden") || v.eq_ignore_ascii_case("collapse") {
                self.visible = false;
            } else if v.eq_ignore_ascii_case("visible") {
                self.visible = true;
            }
        }
    }

    fn apply_inline_style(&mut self, style_str: &str, stroke_width_override: Option<f32>) {
        for decl in style_str.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                self.apply_attr(k.trim(), v.trim(), stroke_width_override);
            }
        }
    }
}

/// 从完整 SVG XML 文档中提取根 `<svg>` 的 [`SvgViewBox`]。
pub fn parse_svg_viewbox(xml: &str) -> Option<SvgViewBox> {
    let mut scanner = XmlScanner::new(xml);
    while let Some(tok) = scanner.next_token() {
        if let XmlToken::Start { tag, attrs, .. } = tok {
            if local_name(tag).eq_ignore_ascii_case("svg") {
                return Some(extract_viewbox_from_attrs(attrs));
            }
        }
    }
    None
}

fn extract_viewbox_from_attrs(attrs: &str) -> SvgViewBox {
    let mut vb: Option<SvgViewBox> = None;
    let mut w: Option<f32> = None;
    let mut h: Option<f32> = None;
    let mut iter = AttrIter::new(attrs);
    while let Some((k, v)) = iter.next_attr() {
        if k.eq_ignore_ascii_case("viewBox") {
            let mut nums = [0.0f32; 4];
            if parse_number_list(v, &mut nums) == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
                vb = Some(SvgViewBox {
                    min_x: nums[0],
                    min_y: nums[1],
                    width: nums[2],
                    height: nums[3],
                });
            }
        } else if k.eq_ignore_ascii_case("width") {
            w = parse_svg_length(v).filter(|&n| n > 0.0);
        } else if k.eq_ignore_ascii_case("height") {
            h = parse_svg_length(v).filter(|&n| n > 0.0);
        }
    }
    if let Some(found) = vb {
        found
    } else if let (Some(width), Some(height)) = (w, h) {
        SvgViewBox {
            min_x: 0.0,
            min_y: 0.0,
            width,
            height,
        }
    } else {
        SvgViewBox::default()
    }
}

/// 解析完整 SVG XML 文档并直接光栅化至目标像素缓冲 `target` 的 `bounds` 矩形与 `clip` 裁剪区内。
#[allow(clippy::too_many_arguments)]
pub fn rasterize_svg<P: PixelFormat>(
    xml: &str,
    bounds: Region,
    clip: Region,
    options: SvgRenderOptions,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let eff_clip = clip.intersect(bounds);
    if eff_clip.is_empty() || stride == 0 || options.tint.a() == 0 {
        return;
    }

    let mut stack = [SvgStyleState::root(options, Transform2D::IDENTITY); 16];
    let mut depth = 0usize;
    let mut skip_depth = 0usize;
    let mut path = FlattenedPath::new();
    let mut root_svg_seen = false;

    let mut scanner = XmlScanner::new(xml);
    while let Some(tok) = scanner.next_token() {
        match tok {
            XmlToken::End { .. } => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                } else if depth > 0 {
                    depth -= 1;
                }
            }
            XmlToken::Start {
                tag,
                attrs,
                self_closing,
            } => {
                let name = local_name(tag);
                if skip_depth > 0 {
                    if !self_closing {
                        skip_depth += 1;
                    }
                    continue;
                }
                if is_ignored_container(name) {
                    if !self_closing {
                        skip_depth = 1;
                    }
                    continue;
                }

                let mut state = stack[depth];

                if name.eq_ignore_ascii_case("svg") && !root_svg_seen {
                    root_svg_seen = true;
                    let vb = extract_viewbox_from_attrs(attrs);
                    state = SvgStyleState::root(options, vb.fit_to_bounds(bounds));
                }

                // 遍历属性并收集形状几何参数（先应用普通属性，再应用行内 style）
                let mut inline_style: Option<&str> = None;
                let mut d_attr: Option<&str> = None;
                let mut points_attr: Option<&str> = None;
                let mut x = 0.0f32;
                let mut y = 0.0f32;
                let mut w = 0.0f32;
                let mut h = 0.0f32;
                let mut rx: Option<f32> = None;
                let mut ry: Option<f32> = None;
                let mut cx = 0.0f32;
                let mut cy = 0.0f32;
                let mut r = 0.0f32;
                let mut x1 = 0.0f32;
                let mut y1 = 0.0f32;
                let mut x2 = 0.0f32;
                let mut y2 = 0.0f32;

                let mut iter = AttrIter::new(attrs);
                while let Some((k, v)) = iter.next_attr() {
                    if k.eq_ignore_ascii_case("style") {
                        inline_style = Some(v);
                    } else if k.eq_ignore_ascii_case("d") {
                        d_attr = Some(v);
                    } else if k.eq_ignore_ascii_case("points") {
                        points_attr = Some(v);
                    } else if k.eq_ignore_ascii_case("x") {
                        x = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("y") {
                        y = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("width") {
                        w = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("height") {
                        h = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("rx") {
                        rx = parse_svg_length(v);
                    } else if k.eq_ignore_ascii_case("ry") {
                        ry = parse_svg_length(v);
                    } else if k.eq_ignore_ascii_case("cx") {
                        cx = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("cy") {
                        cy = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("r") {
                        r = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("x1") {
                        x1 = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("y1") {
                        y1 = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("x2") {
                        x2 = parse_svg_length(v).unwrap_or(0.0);
                    } else if k.eq_ignore_ascii_case("y2") {
                        y2 = parse_svg_length(v).unwrap_or(0.0);
                    } else {
                        state.apply_attr(k, v, options.stroke_width_override);
                    }
                }
                if let Some(st) = inline_style {
                    state.apply_inline_style(st, options.stroke_width_override);
                }

                if !root_svg_seen {
                    // 支持不含根 `<svg>` 标签的片段直接以 24×24 默认视口渲染
                    root_svg_seen = true;
                    let base = SvgViewBox::default().fit_to_bounds(bounds);
                    state.transform = base.concat(state.transform);
                }

                if state.visible && state.opacity > 1e-4 {
                    path.clear();
                    let is_shape = if name.eq_ignore_ascii_case("path") {
                        if let Some(d) = d_attr {
                            path.parse_svg_path_d(d, state.transform);
                        }
                        true
                    } else if name.eq_ignore_ascii_case("rect") {
                        let eff_rx = rx.or(ry).unwrap_or(0.0);
                        let eff_ry = ry.or(rx).unwrap_or(0.0);
                        path.add_rect(state.transform, x, y, w, h, eff_rx, eff_ry);
                        true
                    } else if name.eq_ignore_ascii_case("circle") {
                        path.add_ellipse(state.transform, cx, cy, r, r);
                        true
                    } else if name.eq_ignore_ascii_case("ellipse") {
                        path.add_ellipse(
                            state.transform,
                            cx,
                            cy,
                            rx.unwrap_or(0.0),
                            ry.unwrap_or(0.0),
                        );
                        true
                    } else if name.eq_ignore_ascii_case("line") {
                        path.add_line(state.transform, x1, y1, x2, y2);
                        true
                    } else if name.eq_ignore_ascii_case("polyline") {
                        if let Some(pts) = points_attr {
                            path.add_points(state.transform, pts, false);
                        }
                        true
                    } else if name.eq_ignore_ascii_case("polygon") {
                        if let Some(pts) = points_attr {
                            path.add_points(state.transform, pts, true);
                        }
                        true
                    } else {
                        false
                    };

                    if is_shape && !path.is_empty() {
                        if let Some(fill_col) = state
                            .fill
                            .resolve(state.current_color, state.opacity * state.fill_opacity)
                        {
                            fill_path(
                                &path,
                                fill_col,
                                state.fill_rule,
                                eff_clip,
                                origin_x,
                                origin_y,
                                stride,
                                target,
                            );
                        }
                        if let Some(stroke_col) = state
                            .stroke
                            .resolve(state.current_color, state.opacity * state.stroke_opacity)
                        {
                            let sw_px = state.stroke_width * state.transform.approx_scale();
                            stroke_path(
                                &path,
                                stroke_col,
                                sw_px,
                                state.stroke_linecap,
                                state.stroke_linejoin,
                                state.stroke_miterlimit,
                                eff_clip,
                                origin_x,
                                origin_y,
                                stride,
                                target,
                            );
                        }
                    }
                }

                if !self_closing {
                    if depth + 1 < stack.len() {
                        depth += 1;
                        stack[depth] = state;
                    }
                }
            }
        }
    }
}

/// 直接光栅化单条标准 24×24 SVG Path `d` 数据字符串。
///
/// - 当 `stroke_width_x10 == 0` 时按实心填充渲染（`NonZero` 规则）；
/// - 当 `stroke_width_x10 > 0` 时按圆头圆角矢量线条图标（`stroke-width = stroke_width_x10 / 10.0`）渲染。
#[allow(clippy::too_many_arguments)]
pub fn rasterize_svg_path<P: PixelFormat>(
    d: &str,
    bounds: Region,
    clip: Region,
    tint: Color,
    stroke_width_x10: u8,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let eff_clip = clip.intersect(bounds);
    if eff_clip.is_empty() || stride == 0 || tint.a() == 0 || d.is_empty() {
        return;
    }
    let xf = SvgViewBox::default().fit_to_bounds(bounds);
    let mut path = FlattenedPath::new();
    path.parse_svg_path_d(d, xf);
    if path.is_empty() {
        return;
    }
    if stroke_width_x10 == 0 {
        fill_path(
            &path,
            tint,
            SvgFillRule::NonZero,
            eff_clip,
            origin_x,
            origin_y,
            stride,
            target,
        );
    } else {
        let sw_px = ((stroke_width_x10 as f32) * 0.1) * xf.approx_scale();
        stroke_path(
            &path,
            tint,
            sw_px,
            SvgLineCap::Round,
            SvgLineJoin::Round,
            4.0,
            eff_clip,
            origin_x,
            origin_y,
            stride,
            target,
        );
    }
}

#[inline]
fn local_name(tag: &str) -> &str {
    match tag.rsplit_once(':') {
        Some((_, right)) => right,
        None => tag,
    }
}

#[inline]
fn is_ignored_container(name: &str) -> bool {
    name.eq_ignore_ascii_case("defs")
        || name.eq_ignore_ascii_case("title")
        || name.eq_ignore_ascii_case("desc")
        || name.eq_ignore_ascii_case("metadata")
        || name.eq_ignore_ascii_case("style")
        || name.eq_ignore_ascii_case("clipPath")
        || name.eq_ignore_ascii_case("mask")
        || name.eq_ignore_ascii_case("filter")
        || name.eq_ignore_ascii_case("linearGradient")
        || name.eq_ignore_ascii_case("radialGradient")
}

enum XmlToken<'a> {
    Start {
        tag: &'a str,
        attrs: &'a str,
        self_closing: bool,
    },
    End,
}

struct XmlScanner<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> XmlScanner<'a> {
    #[inline]
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn next_token(&mut self) -> Option<XmlToken<'a>> {
        let bytes = self.src.as_bytes();
        loop {
            while self.pos < bytes.len() && bytes[self.pos] != b'<' {
                self.pos += 1;
            }
            if self.pos >= bytes.len() {
                return None;
            }
            self.pos += 1; // skip '<'
            if self.pos >= bytes.len() {
                return None;
            }

            // 注释 <!-- ... -->
            if self.src[self.pos..].starts_with("!--") {
                if let Some(end) = self.src[self.pos + 3..].find("-->") {
                    self.pos += 3 + end + 3;
                    continue;
                }
                return None;
            }
            // 处理指令 <?...?> 或 <!DOCTYPE ...>
            if bytes[self.pos] == b'?' || bytes[self.pos] == b'!' {
                if let Some(end) = find_tag_end(&self.src[self.pos..]) {
                    self.pos += end + 1;
                    continue;
                }
                return None;
            }
            // 结束标签 </tag>
            if bytes[self.pos] == b'/' {
                if let Some(end) = find_tag_end(&self.src[self.pos..]) {
                    self.pos += end + 1;
                    return Some(XmlToken::End);
                }
                return None;
            }

            let tag_body_start = self.pos;
            let end_rel = find_tag_end(&self.src[tag_body_start..])?;
            let raw_body = &self.src[tag_body_start..tag_body_start + end_rel];
            self.pos = tag_body_start + end_rel + 1;

            let trimmed = raw_body.trim();
            let (body, self_closing) = match trimmed.strip_suffix('/') {
                Some(b) => (b.trim_end(), true),
                None => (trimmed, false),
            };
            if body.is_empty() {
                continue;
            }
            let name_end = body
                .bytes()
                .position(|b| b.is_ascii_whitespace())
                .unwrap_or(body.len());
            let tag = &body[..name_end];
            let attrs = body[name_end..].trim();
            return Some(XmlToken::Start {
                tag,
                attrs,
                self_closing,
            });
        }
    }
}

/// 在忽略单/双引号包裹值的前提下寻找 `>` 结束符下标。
fn find_tag_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut quote: u8 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return Some(i);
        }
    }
    None
}

struct AttrIter<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> AttrIter<'a> {
    #[inline]
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn next_attr(&mut self) -> Option<(&'a str, &'a str)> {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if self.pos >= bytes.len() {
            return None;
        }
        let name_start = self.pos;
        while self.pos < bytes.len()
            && !bytes[self.pos].is_ascii_whitespace()
            && bytes[self.pos] != b'='
        {
            self.pos += 1;
        }
        let name = &self.src[name_start..self.pos];
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if self.pos >= bytes.len() || bytes[self.pos] != b'=' {
            return None;
        }
        self.pos += 1; // skip '='
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if self.pos >= bytes.len() {
            return None;
        }
        let q = bytes[self.pos];
        if q == b'"' || q == b'\'' {
            self.pos += 1;
            let val_start = self.pos;
            while self.pos < bytes.len() && bytes[self.pos] != q {
                self.pos += 1;
            }
            let val = &self.src[val_start..self.pos];
            if self.pos < bytes.len() {
                self.pos += 1;
            }
            Some((name, val))
        } else {
            let val_start = self.pos;
            while self.pos < bytes.len() && !bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            Some((name, &self.src[val_start..self.pos]))
        }
    }
}
