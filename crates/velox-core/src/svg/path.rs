//! 完整 SVG Path `d` 语法解析器、物理像素平坦度曲线细分与基础几何形状展开。

use super::math::{
    abs_f32, atan2_f32, ceil_f32, parse_next_f32, sin_cos_f32, sqrt_f32, Transform2D, PI, TAU,
};

use std::vec::Vec;


/// 单个连续子路径描述符（指向 [`FlattenedPath`] 顶点数组区间）。
#[derive(Copy, Clone, Debug)]
pub struct SubPath {
    pub start: usize,
    pub len: usize,
    pub closed: bool,
}

/// 已经过仿射变换并细分至目标物理像素空间的折线路径集合（可在多个图形元素间复用缓冲）。
pub struct FlattenedPath {
    points: Vec<(f32, f32)>,
    subpaths: Vec<SubPath>,
}

impl Default for FlattenedPath {
    fn default() -> Self {
        Self::new()
    }
}

impl FlattenedPath {
    /// 创建空路径缓冲。
    pub fn new() -> Self {
        Self {
            points: Vec::with_capacity(64),
            subpaths: Vec::with_capacity(8),
        }
    }

    /// 清空顶点与子路径（保留底层容量以便下一图形复用）。
    pub fn clear(&mut self) {
        self.points.clear();
        self.subpaths.clear();
    }

    /// 返回全部物理像素顶点切片。
    #[inline]
    pub fn points(&self) -> &[(f32, f32)] {
        self.points.as_slice()
    }

    /// 返回全部子路径切片。
    #[inline]
    pub fn subpaths(&self) -> &[SubPath] {
        self.subpaths.as_slice()
    }

    /// 路径是否为空。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.subpaths.is_empty()
    }

    #[inline]
    fn push_pt_raw(&mut self, pt: (f32, f32)) -> bool {
        {
            self.points.push(pt);
            true
        }
    }

    #[inline]
    fn push_sub_raw(&mut self, sp: SubPath) -> bool {
        {
            self.subpaths.push(sp);
            true
        }
    }

    /// 开启一条位于屏幕物理坐标 `(x, y)` 的新子路径。
    pub fn move_to(&mut self, x: f32, y: f32) {
        let start = self.points.len();
        if self.push_pt_raw((x, y)) {
            let _ = self.push_sub_raw(SubPath {
                start,
                len: 1,
                closed: false,
            });
        }
    }

    /// 向当前子路径追加线段终点 `(x, y)`（自动过滤零长度重复顶点）。
    pub fn line_to(&mut self, x: f32, y: f32) {
        let sub_len = self.subpaths.len();
        if sub_len == 0 {
            self.move_to(x, y);
            return;
        }
        let last_idx = sub_len - 1;
        let (start, len) = {
            let sp = self.subpaths.as_slice()[last_idx];
            (sp.start, sp.len)
        };
        if len > 0 {
            let prev = self.points.as_slice()[start + len - 1];
            if abs_f32(prev.0 - x) < 1e-4 && abs_f32(prev.1 - y) < 1e-4 {
                return;
            }
        }
        if self.push_pt_raw((x, y)) {
            self.subpaths.as_mut_slice()[last_idx].len += 1;
        }
    }

    /// 标记当前子路径为闭合。
    pub fn close(&mut self) {
        if let Some(last_sub) = self.subpaths.as_mut_slice().last_mut() {
            last_sub.closed = true;
        }
    }

    /// 添加二次贝塞尔曲线（输入已变换至屏幕物理像素坐标），平坦度误差 $\le 0.25\text{px}$。
    pub fn quad_to(&mut self, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) {
        self.flatten_quad(p0, p1, p2, 0);
    }

    fn flatten_quad(&mut self, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), depth: u8) {
        // 中点到弦的距离估计：0.25 * |p0 - 2*p1 + p2|
        let dx = p0.0 - 2.0 * p1.0 + p2.0;
        let dy = p0.1 - 2.0 * p1.1 + p2.1;
        if depth >= 8 || (dx * dx + dy * dy) <= 1.0 {
            self.line_to(p2.0, p2.1);
            return;
        }
        let m01 = (0.5 * (p0.0 + p1.0), 0.5 * (p0.1 + p1.1));
        let m12 = (0.5 * (p1.0 + p2.0), 0.5 * (p1.1 + p2.1));
        let mid = (0.5 * (m01.0 + m12.0), 0.5 * (m01.1 + m12.1));
        self.flatten_quad(p0, m01, mid, depth + 1);
        self.flatten_quad(mid, m12, p2, depth + 1);
    }

    /// 添加三次贝塞尔曲线（输入已变换至屏幕物理像素坐标），平坦度误差 $\le 0.25\text{px}$。
    pub fn cubic_to(
        &mut self,
        p0: (f32, f32),
        p1: (f32, f32),
        p2: (f32, f32),
        p3: (f32, f32),
    ) {
        self.flatten_cubic(p0, p1, p2, p3, 0);
    }

    fn flatten_cubic(
        &mut self,
        p0: (f32, f32),
        p1: (f32, f32),
        p2: (f32, f32),
        p3: (f32, f32),
        depth: u8,
    ) {
        // Wang / de Casteljau 平坦度上界估计
        let d1x = 3.0 * p1.0 - 2.0 * p0.0 - p3.0;
        let d1y = 3.0 * p1.1 - 2.0 * p0.1 - p3.1;
        let d2x = 3.0 * p2.0 - p0.0 - 2.0 * p3.0;
        let d2y = 3.0 * p2.1 - p0.1 - 2.0 * p3.1;
        let err_sq = (d1x * d1x + d1y * d1y).max(d2x * d2x + d2y * d2y);
        // 阈值 (4 * 0.25)^2 = 1.0
        if depth >= 8 || err_sq <= 1.0 {
            self.line_to(p3.0, p3.1);
            return;
        }
        let m01 = (0.5 * (p0.0 + p1.0), 0.5 * (p0.1 + p1.1));
        let m12 = (0.5 * (p1.0 + p2.0), 0.5 * (p1.1 + p2.1));
        let m23 = (0.5 * (p2.0 + p3.0), 0.5 * (p2.1 + p3.1));
        let m012 = (0.5 * (m01.0 + m12.0), 0.5 * (m01.1 + m12.1));
        let m123 = (0.5 * (m12.0 + m23.0), 0.5 * (m12.1 + m23.1));
        let mid = (0.5 * (m012.0 + m123.0), 0.5 * (m012.1 + m123.1));
        self.flatten_cubic(p0, m01, m012, mid, depth + 1);
        self.flatten_cubic(mid, m123, m23, p3, depth + 1);
    }

    /// 依据 W3C SVG 1.1 F.6.5/F.6.6 规范展开椭圆弧（在用户空间求中心参数化后按 `xf` 变换细分）。
    #[allow(clippy::too_many_arguments)]
    pub fn arc_to_user(
        &mut self,
        xf: Transform2D,
        x1: f32,
        y1: f32,
        rx_in: f32,
        ry_in: f32,
        x_axis_rot_deg: f32,
        large_arc: bool,
        sweep: bool,
        x2: f32,
        y2: f32,
    ) {
        if abs_f32(x1 - x2) < 1e-5 && abs_f32(y1 - y2) < 1e-5 {
            return;
        }
        let mut rx = abs_f32(rx_in);
        let mut ry = abs_f32(ry_in);
        if rx < 1e-5 || ry < 1e-5 {
            let (tx, ty) = xf.apply(x2, y2);
            self.line_to(tx, ty);
            return;
        }

        let phi = x_axis_rot_deg * (PI / 180.0);
        let (sin_phi, cos_phi) = sin_cos_f32(phi);

        let dx2 = 0.5 * (x1 - x2);
        let dy2 = 0.5 * (y1 - y2);
        let x1p = cos_phi * dx2 + sin_phi * dy2;
        let y1p = -sin_phi * dx2 + cos_phi * dy2;

        // 半径过小时自动等比放大（F.6.6.3）
        let rx_sq = rx * rx;
        let ry_sq = ry * ry;
        let x1p_sq = x1p * x1p;
        let y1p_sq = y1p * y1p;
        let lambda = x1p_sq / rx_sq + y1p_sq / ry_sq;
        if lambda > 1.0 {
            let s = sqrt_f32(lambda);
            rx *= s;
            ry *= s;
        }

        let rx_sq = rx * rx;
        let ry_sq = ry * ry;
        let num = (rx_sq * ry_sq - rx_sq * y1p_sq - ry_sq * x1p_sq).max(0.0);
        let den = rx_sq * y1p_sq + ry_sq * x1p_sq;
        let coef_mag = if den > 1e-10 {
            sqrt_f32(num / den)
        } else {
            0.0
        };
        let coef = if large_arc == sweep {
            -coef_mag
        } else {
            coef_mag
        };
        let cxp = coef * (rx * y1p / ry);
        let cyp = coef * (-ry * x1p / rx);

        let cx = cos_phi * cxp - sin_phi * cyp + 0.5 * (x1 + x2);
        let cy = sin_phi * cxp + cos_phi * cyp + 0.5 * (y1 + y2);

        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;

        let theta1 = vec_angle(1.0, 0.0, ux, uy);
        let mut dtheta = vec_angle(ux, uy, vx, vy);
        if !sweep && dtheta > 0.0 {
            dtheta -= TAU;
        } else if sweep && dtheta < 0.0 {
            dtheta += TAU;
        }

        let approx_r_px = rx.max(ry) * xf.approx_scale();
        let arc_len_px = abs_f32(dtheta) * approx_r_px;
        let steps = (ceil_f32(arc_len_px * 0.45) as usize).clamp(4, 96);
        let step_dt = dtheta / (steps as f32);

        for i in 1..=steps {
            let ang = theta1 + step_dt * (i as f32);
            let (sa, ca) = sin_cos_f32(ang);
            let ex = rx * ca;
            let ey = ry * sa;
            let px = cos_phi * ex - sin_phi * ey + cx;
            let py = sin_phi * ex + cos_phi * ey + cy;
            let (tx, ty) = xf.apply(px, py);
            self.line_to(tx, ty);
        }
    }

    /// 解析完整 SVG Path `d` 字符串（全部 20 个命令 `M/m L/l H/h V/v C/c S/s Q/q T/t A/a Z/z`）并写入当前路径。
    pub fn parse_svg_path_d(&mut self, d: &str, xf: Transform2D) {
        let mut parser = PathScanner::new(d);
        let mut cmd = 0u8;
        let mut cur = (0.0f32, 0.0f32);
        let mut sub_start = (0.0f32, 0.0f32);
        let mut last_ctrl: Option<(f32, f32)> = None;
        let mut last_cmd = 0u8;

        while parser.skip_ws_and_commas() {
            if let Some(c) = parser.peek_command() {
                cmd = c;
                parser.advance(1);
            } else if cmd == 0 {
                break;
            }

            match cmd {
                b'M' | b'm' => {
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    let nx = if cmd == b'm' { cur.0 + x } else { x };
                    let ny = if cmd == b'm' { cur.1 + y } else { y };
                    cur = (nx, ny);
                    sub_start = cur;
                    let (tx, ty) = xf.apply(cur.0, cur.1);
                    self.move_to(tx, ty);
                    last_ctrl = None;
                    last_cmd = cmd;
                    // 规范要求：M/m 后续隐含坐标对按 L/l 处理
                    cmd = if cmd == b'm' { b'l' } else { b'L' };
                }
                b'L' | b'l' => {
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    cur = if cmd == b'l' {
                        (cur.0 + x, cur.1 + y)
                    } else {
                        (x, y)
                    };
                    let (tx, ty) = xf.apply(cur.0, cur.1);
                    self.line_to(tx, ty);
                    last_ctrl = None;
                    last_cmd = cmd;
                }
                b'H' | b'h' => {
                    let Some(x) = parser.next_f32() else {
                        break;
                    };
                    cur.0 = if cmd == b'h' { cur.0 + x } else { x };
                    let (tx, ty) = xf.apply(cur.0, cur.1);
                    self.line_to(tx, ty);
                    last_ctrl = None;
                    last_cmd = cmd;
                }
                b'V' | b'v' => {
                    let Some(y) = parser.next_f32() else {
                        break;
                    };
                    cur.1 = if cmd == b'v' { cur.1 + y } else { y };
                    let (tx, ty) = xf.apply(cur.0, cur.1);
                    self.line_to(tx, ty);
                    last_ctrl = None;
                    last_cmd = cmd;
                }
                b'C' | b'c' => {
                    let Some((x1, y1)) = parser.next_pair() else {
                        break;
                    };
                    let Some((x2, y2)) = parser.next_pair() else {
                        break;
                    };
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    let rel = cmd == b'c';
                    let c1 = if rel { (cur.0 + x1, cur.1 + y1) } else { (x1, y1) };
                    let c2 = if rel { (cur.0 + x2, cur.1 + y2) } else { (x2, y2) };
                    let p3 = if rel { (cur.0 + x, cur.1 + y) } else { (x, y) };
                    self.cubic_to(
                        xf.apply(cur.0, cur.1),
                        xf.apply(c1.0, c1.1),
                        xf.apply(c2.0, c2.1),
                        xf.apply(p3.0, p3.1),
                    );
                    cur = p3;
                    last_ctrl = Some(c2);
                    last_cmd = cmd;
                }
                b'S' | b's' => {
                    let Some((x2, y2)) = parser.next_pair() else {
                        break;
                    };
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    let rel = cmd == b's';
                    let c1 = match last_cmd {
                        b'C' | b'c' | b'S' | b's' => {
                            if let Some(lc) = last_ctrl {
                                (2.0 * cur.0 - lc.0, 2.0 * cur.1 - lc.1)
                            } else {
                                cur
                            }
                        }
                        _ => cur,
                    };
                    let c2 = if rel { (cur.0 + x2, cur.1 + y2) } else { (x2, y2) };
                    let p3 = if rel { (cur.0 + x, cur.1 + y) } else { (x, y) };
                    self.cubic_to(
                        xf.apply(cur.0, cur.1),
                        xf.apply(c1.0, c1.1),
                        xf.apply(c2.0, c2.1),
                        xf.apply(p3.0, p3.1),
                    );
                    cur = p3;
                    last_ctrl = Some(c2);
                    last_cmd = cmd;
                }
                b'Q' | b'q' => {
                    let Some((x1, y1)) = parser.next_pair() else {
                        break;
                    };
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    let rel = cmd == b'q';
                    let c1 = if rel { (cur.0 + x1, cur.1 + y1) } else { (x1, y1) };
                    let p2 = if rel { (cur.0 + x, cur.1 + y) } else { (x, y) };
                    self.quad_to(
                        xf.apply(cur.0, cur.1),
                        xf.apply(c1.0, c1.1),
                        xf.apply(p2.0, p2.1),
                    );
                    cur = p2;
                    last_ctrl = Some(c1);
                    last_cmd = cmd;
                }
                b'T' | b't' => {
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    let rel = cmd == b't';
                    let c1 = match last_cmd {
                        b'Q' | b'q' | b'T' | b't' => {
                            if let Some(lc) = last_ctrl {
                                (2.0 * cur.0 - lc.0, 2.0 * cur.1 - lc.1)
                            } else {
                                cur
                            }
                        }
                        _ => cur,
                    };
                    let p2 = if rel { (cur.0 + x, cur.1 + y) } else { (x, y) };
                    self.quad_to(
                        xf.apply(cur.0, cur.1),
                        xf.apply(c1.0, c1.1),
                        xf.apply(p2.0, p2.1),
                    );
                    cur = p2;
                    last_ctrl = Some(c1);
                    last_cmd = cmd;
                }
                b'A' | b'a' => {
                    let Some(rx) = parser.next_f32() else {
                        break;
                    };
                    let Some(ry) = parser.next_f32() else {
                        break;
                    };
                    let Some(x_rot) = parser.next_f32() else {
                        break;
                    };
                    let Some(large_arc) = parser.next_flag() else {
                        break;
                    };
                    let Some(sweep) = parser.next_flag() else {
                        break;
                    };
                    let Some((x, y)) = parser.next_pair() else {
                        break;
                    };
                    let rel = cmd == b'a';
                    let target = if rel { (cur.0 + x, cur.1 + y) } else { (x, y) };
                    self.arc_to_user(
                        xf, cur.0, cur.1, rx, ry, x_rot, large_arc, sweep, target.0, target.1,
                    );
                    cur = target;
                    last_ctrl = None;
                    last_cmd = cmd;
                }
                b'Z' | b'z' => {
                    self.close();
                    cur = sub_start;
                    last_ctrl = None;
                    last_cmd = cmd;
                    cmd = 0;
                }
                _ => break,
            }
        }
    }

    /// 展开 `<rect x y width height rx ry>` 形状。
    #[allow(clippy::too_many_arguments)]
    pub fn add_rect(
        &mut self,
        xf: Transform2D,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx_in: f32,
        ry_in: f32,
    ) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let rx = rx_in.max(0.0).min(0.5 * w);
        let ry = ry_in.max(0.0).min(0.5 * h);
        if rx < 1e-4 && ry < 1e-4 {
            let (p0x, p0y) = xf.apply(x, y);
            let (p1x, p1y) = xf.apply(x + w, y);
            let (p2x, p2y) = xf.apply(x + w, y + h);
            let (p3x, p3y) = xf.apply(x, y + h);
            self.move_to(p0x, p0y);
            self.line_to(p1x, p1y);
            self.line_to(p2x, p2y);
            self.line_to(p3x, p3y);
            self.close();
        } else {
            let (sx, sy) = xf.apply(x + rx, y);
            self.move_to(sx, sy);
            let (t1x, t1y) = xf.apply(x + w - rx, y);
            self.line_to(t1x, t1y);
            self.arc_to_user(xf, x + w - rx, y, rx, ry, 0.0, false, true, x + w, y + ry);
            let (r1x, r1y) = xf.apply(x + w, y + h - ry);
            self.line_to(r1x, r1y);
            self.arc_to_user(
                xf,
                x + w,
                y + h - ry,
                rx,
                ry,
                0.0,
                false,
                true,
                x + w - rx,
                y + h,
            );
            let (b1x, b1y) = xf.apply(x + rx, y + h);
            self.line_to(b1x, b1y);
            self.arc_to_user(xf, x + rx, y + h, rx, ry, 0.0, false, true, x, y + h - ry);
            let (l1x, l1y) = xf.apply(x, y + ry);
            self.line_to(l1x, l1y);
            self.arc_to_user(xf, x, y + ry, rx, ry, 0.0, false, true, x + rx, y);
            self.close();
        }
    }

    /// 展开 `<circle cx cy r>` 或 `<ellipse cx cy rx ry>` 形状。
    pub fn add_ellipse(&mut self, xf: Transform2D, cx: f32, cy: f32, rx: f32, ry: f32) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let approx_r_px = rx.max(ry) * xf.approx_scale();
        let steps = (ceil_f32(TAU * approx_r_px * 0.45) as usize).clamp(16, 128);
        let step_a = TAU / (steps as f32);
        let (sx, sy) = xf.apply(cx + rx, cy);
        self.move_to(sx, sy);
        for i in 1..steps {
            let (sa, ca) = sin_cos_f32(step_a * (i as f32));
            let (px, py) = xf.apply(cx + rx * ca, cy + ry * sa);
            self.line_to(px, py);
        }
        self.close();
    }

    /// 展开 `<line x1 y1 x2 y2>`。
    pub fn add_line(&mut self, xf: Transform2D, x1: f32, y1: f32, x2: f32, y2: f32) {
        let (p0x, p0y) = xf.apply(x1, y1);
        let (p1x, p1y) = xf.apply(x2, y2);
        self.move_to(p0x, p0y);
        self.line_to(p1x, p1y);
    }

    /// 展开 `<polyline points="...">` 或 `<polygon points="...">`。
    pub fn add_points(&mut self, xf: Transform2D, points_str: &str, closed: bool) {
        let mut cur = points_str;
        let mut first = true;
        while let Some((x, r1)) = parse_next_f32(cur) {
            let Some((y, r2)) = parse_next_f32(r1) else {
                break;
            };
            let (px, py) = xf.apply(x, y);
            if first {
                self.move_to(px, py);
                first = false;
            } else {
                self.line_to(px, py);
            }
            cur = r2;
        }
        if !first && closed {
            self.close();
        }
    }
}

#[inline]
fn vec_angle(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    atan2_f32(ux * vy - uy * vx, ux * vx + uy * vy)
}

/// SVG Path `d` 专用扫描器（精确处理省略空白的负数/小数点分隔与紧凑 `A` 弧单字符标志位）。
struct PathScanner<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> PathScanner<'a> {
    #[inline]
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.src.len());
    }

    #[inline]
    fn skip_ws_and_commas(&mut self) -> bool {
        let b = self.src.as_bytes();
        while self.pos < b.len() {
            match b[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' | b',' => self.pos += 1,
                _ => return true,
            }
        }
        false
    }

    #[inline]
    fn peek_command(&self) -> Option<u8> {
        let b = *self.src.as_bytes().get(self.pos)?;
        match b {
            b'M' | b'm' | b'L' | b'l' | b'H' | b'h' | b'V' | b'v' | b'C' | b'c' | b'S' | b's'
            | b'Q' | b'q' | b'T' | b't' | b'A' | b'a' | b'Z' | b'z' => Some(b),
            _ => None,
        }
    }

    #[inline]
    fn next_f32(&mut self) -> Option<f32> {
        let (val, rest) = parse_next_f32(&self.src[self.pos..])?;
        self.pos = self.src.len() - rest.len();
        Some(val)
    }

    #[inline]
    fn next_pair(&mut self) -> Option<(f32, f32)> {
        let x = self.next_f32()?;
        let y = self.next_f32()?;
        Some((x, y))
    }

    /// 按 W3C SVG Path 规范读取单个标志字符 `'0'` 或 `'1'`（允许紧连如 `012` 解析为 `false, true, 2`）。
    #[inline]
    fn next_flag(&mut self) -> Option<bool> {
        if !self.skip_ws_and_commas() {
            return None;
        }
        let b = *self.src.as_bytes().get(self.pos)?;
        match b {
            b'0' => {
                self.pos += 1;
                Some(false)
            }
            b'1' => {
                self.pos += 1;
                Some(true)
            }
            _ => None,
        }
    }
}
