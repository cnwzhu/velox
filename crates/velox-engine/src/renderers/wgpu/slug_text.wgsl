// Velox Slug (2026 Public Domain) GPU 曲线字形直接光栅化着色器 v2
//
// 升级特性：
// - 16 B/曲线 f16 量化存储（WGSL 内置 unpack2x16float，全后端零扩展兼容，显存与采样带宽减半）；
// - 完整 2x2 线性变换 + 逆雅可比动态顶点膨胀（支持任意旋转、非均匀缩放与斜切 OBB 文本）；
// - Eric Lengyel 公有领域 2-bit 根贡献分类码 0x2E74 + 双轴条带 Early-Exit + 屏幕导数盒滤波抗锯齿。

struct SlugCurve {
    // x = pack2x16float(p0), y = pack2x16float(p1), z = pack2x16float(p2), w = pad
    p012_pad: vec4<u32>,
};

struct SlugBand {
    start: u32,
    count: u32,
};

struct SlugGlyphInstance {
    // 2x2 线性矩阵第 0 行 + 屏幕基线原点：[m00, m01, origin_x_px, origin_y_px]
    xform_row0: vec4<f32>,
    // 2x2 线性矩阵第 1 行 + 膨胀与色彩标志：[m10, m11, dilation_px, is_srgb]
    xform_row1: vec4<f32>,
    // (em_min_x, em_min_y, em_max_x, em_max_y)
    em_bounds: vec4<f32>,
    // (clip_x0, clip_y0, clip_x1, clip_y1)
    clip_rect: vec4<f32>,
    // straight RGBA [0..1]
    color: vec4<f32>,
    // (h_band_offset, num_h_bands, v_band_offset, num_v_bands)
    band_info: vec4<u32>,
    // (viewport_w, viewport_h, approx_font_px, 0.0)
    viewport_info: vec4<f32>,
};

@group(0) @binding(0) var<storage, read> curves: array<SlugCurve>;
@group(0) @binding(1) var<storage, read> band_indices: array<u32>;
@group(0) @binding(2) var<storage, read> bands: array<SlugBand>;
@group(0) @binding(3) var<storage, read> instances: array<SlugGlyphInstance>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) em_pos: vec2<f32>,
    @location(1) screen_pos: vec2<f32>,
    @location(2) @interpolate(flat) inst_idx: u32,
};

@vertex
fn vs_slug(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) iid: u32,
) -> VsOut {
    let inst = instances[iid];
    var corner = vec2<f32>(0.0, 0.0);
    switch (vid) {
        case 0u: { corner = vec2<f32>(0.0, 0.0); }
        case 1u: { corner = vec2<f32>(1.0, 0.0); }
        case 2u: { corner = vec2<f32>(1.0, 1.0); }
        case 3u: { corner = vec2<f32>(0.0, 0.0); }
        case 4u: { corner = vec2<f32>(1.0, 1.0); }
        default: { corner = vec2<f32>(0.0, 1.0); }
    }

    let m00 = inst.xform_row0.x;
    let m01 = inst.xform_row0.y;
    let origin = inst.xform_row0.zw;
    let m10 = inst.xform_row1.x;
    let m11 = inst.xform_row1.y;
    let dilate_px = max(inst.xform_row1.z, 1.5);

    // 2x2 逆雅可比动态顶点膨胀（参考《A Decade of Slug》）：
    // 将 EM 盒四个角点沿屏幕像素外法线外扩 `dilate_px`，再用 J^-1 精确反算对应 EM 空间坐标，
    // 保证任意旋转、非均匀缩放与斜切变换下抗锯齿边缘都严格具备 `dilate_px` 保护带。
    let base_em = mix(inst.em_bounds.xy, inst.em_bounds.zw, corner);
    let base_screen = origin + vec2<f32>(
        m00 * base_em.x + m01 * base_em.y,
        m10 * base_em.x + m11 * base_em.y,
    );

    // EM 框外法线方向（角点 sign ∈ {-1, +1}）经雅可比各列映射到屏幕空间后单位化外扩
    let sign_em = corner * 2.0 - vec2<f32>(1.0, 1.0);
    let col0 = vec2<f32>(m00, m10);
    let col1 = vec2<f32>(m01, m11);
    let len0 = max(length(col0), 1e-4);
    let len1 = max(length(col1), 1e-4);
    let screen_offset = (col0 / len0) * (sign_em.x * dilate_px) + (col1 / len1) * (sign_em.y * dilate_px);
    let screen = base_screen + screen_offset;

    // 使用 2x2 逆雅可比 `J^-1` 将屏幕外扩偏移精确映射回 EM 坐标
    let det = m00 * m11 - m01 * m10;
    var em = base_em;
    if (abs(det) > 1e-6) {
        let inv_det = 1.0 / det;
        let d_em = vec2<f32>(
            (m11 * screen_offset.x - m01 * screen_offset.y) * inv_det,
            (-m10 * screen_offset.x + m00 * screen_offset.y) * inv_det,
        );
        em = base_em + d_em;
    }

    let vw = max(inst.viewport_info.x, 1.0);
    let vh = max(inst.viewport_info.y, 1.0);
    let ndc_x = (screen.x / vw) * 2.0 - 1.0;
    let ndc_y = 1.0 - (screen.y / vh) * 2.0;

    var out: VsOut;
    out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.em_pos = em;
    out.screen_pos = screen;
    out.inst_idx = iid;
    return out;
}

// Eric Lengyel Slug 2-bit 根贡献分类码（消除端点重复计数与 -0.0 伪影）：
// - bit 0 (1u): t1 = (b - d) / a 有效（曲线从 >=0 穿越到 <0）
// - bit 1 (2u): t2 = (b + d) / a 有效（曲线从 <0 穿越到 >=0）
fn calc_root_code(y1: f32, y2: f32, y3: f32) -> u32 {
    let s1 = select(0u, 1u, y1 < 0.0);
    let s2 = select(0u, 1u, y2 < 0.0);
    let s3 = select(0u, 1u, y3 < 0.0);
    let shift = ((s1 << 2u) | (s2 << 1u) | s3) * 2u;
    return (0x2E74u >> shift) & 3u;
}

fn solve_horiz_poly(p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>) -> vec2<f32> {
    let a = p1 - p2 * 2.0 + p3;
    let b = p1 - p2;
    let d = sqrt(max(b.y * b.y - a.y * p1.y, 0.0));
    var t1: f32;
    var t2: f32;
    if (abs(a.y) < 1e-5) {
        let tl = select(0.0, p1.y / (2.0 * b.y), abs(b.y) > 1e-6);
        t1 = tl;
        t2 = tl;
    } else {
        let ra = 1.0 / a.y;
        t1 = (b.y - d) * ra;
        t2 = (b.y + d) * ra;
    }
    let x1 = (a.x * t1 - b.x * 2.0) * t1 + p1.x;
    let x2 = (a.x * t2 - b.x * 2.0) * t2 + p1.x;
    return vec2<f32>(x1, x2);
}

fn solve_vert_poly(p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>) -> vec2<f32> {
    let a = p1 - p2 * 2.0 + p3;
    let b = p1 - p2;
    let d = sqrt(max(b.x * b.x - a.x * p1.x, 0.0));
    var t1: f32;
    var t2: f32;
    if (abs(a.x) < 1e-5) {
        let tl = select(0.0, p1.x / (2.0 * b.x), abs(b.x) > 1e-6);
        t1 = tl;
        t2 = tl;
    } else {
        let ra = 1.0 / a.x;
        t1 = (b.x - d) * ra;
        t2 = (b.x + d) * ra;
    }
    let y1 = (a.y * t1 - b.y * 2.0) * t1 + p1.y;
    let y2 = (a.y * t2 - b.y * 2.0) * t2 + p1.y;
    return vec2<f32>(y1, y2);
}

fn srgb_to_linear_channel(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_slug(in: VsOut) -> @location(0) vec4<f32> {
    // 必须在任何非 uniform 分支前计算屏幕导数（满足 DX12 FXC / Vulkan / Metal 规范）
    let em = in.em_pos;
    let em_dx = dpdx(em);
    let em_dy = dpdy(em);
    let em_size_x = max(length(vec2<f32>(em_dx.x, em_dy.x)), 1e-6);
    let em_size_y = max(length(vec2<f32>(em_dx.y, em_dy.y)), 1e-6);
    let pixels_per_em = vec2<f32>(1.0 / em_size_x, 1.0 / em_size_y);

    let inst = instances[in.inst_idx];
    let clip = inst.clip_rect;
    if (in.screen_pos.x < clip.x || in.screen_pos.x > clip.z || in.screen_pos.y < clip.y || in.screen_pos.y > clip.w) {
        discard;
    }

    let em_min = inst.em_bounds.xy;
    let em_max = inst.em_bounds.zw;
    let em_span = max(em_max - em_min, vec2<f32>(1e-5, 1e-5));
    let norm = clamp((em - em_min) / em_span, vec2<f32>(0.0, 0.0), vec2<f32>(0.99999, 0.99999));

    let num_h = max(inst.band_info.y, 1u);
    let num_v = max(inst.band_info.w, 1u);
    let h_idx = min(u32(norm.y * f32(num_h)), num_h - 1u);
    let v_idx = min(u32(norm.x * f32(num_v)), num_v - 1u);

    // 1. 水平射线绕数与盒滤波覆盖率（沿 +X，曲线按 max_x 降序排列支持提前退出）
    var xcov: f32 = 0.0;
    var xwgt: f32 = 0.0;
    let h_band = bands[inst.band_info.x + h_idx];
    let h_end = h_band.start + h_band.count;
    for (var i = h_band.start; i < h_end; i = i + 1u) {
        let c = curves[band_indices[i]].p012_pad;
        let p1 = unpack2x16float(c.x) - em;
        let p2 = unpack2x16float(c.y) - em;
        let p3 = unpack2x16float(c.z) - em;
        if (max(max(p1.x, p2.x), p3.x) * pixels_per_em.x < -0.5) {
            break;
        }
        let code = calc_root_code(p1.y, p2.y, p3.y);
        if (code != 0u) {
            let r = solve_horiz_poly(p1, p2, p3) * pixels_per_em.x;
            if ((code & 1u) != 0u) {
                xcov = xcov + clamp(r.x + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if ((code & 2u) != 0u) {
                xcov = xcov - clamp(r.y + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    // 2. 垂直射线绕数与盒滤波覆盖率（沿 +Y，曲线按 max_y 降序排列支持提前退出）
    var ycov: f32 = 0.0;
    var ywgt: f32 = 0.0;
    let v_band = bands[inst.band_info.z + v_idx];
    let v_end = v_band.start + v_band.count;
    for (var i = v_band.start; i < v_end; i = i + 1u) {
        let c = curves[band_indices[i]].p012_pad;
        let p1 = unpack2x16float(c.x) - em;
        let p2 = unpack2x16float(c.y) - em;
        let p3 = unpack2x16float(c.z) - em;
        if (max(max(p1.y, p2.y), p3.y) * pixels_per_em.y < -0.5) {
            break;
        }
        let code = calc_root_code(p1.x, p2.x, p3.x);
        if (code != 0u) {
            let r = solve_vert_poly(p1, p2, p3) * pixels_per_em.y;
            if ((code & 1u) != 0u) {
                ycov = ycov - clamp(r.x + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if ((code & 2u) != 0u) {
                ycov = ycov + clamp(r.y + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    // 3. Slug 双轴融合 + 远景小字号光学笔画轻微增强
    let combined = max(
        abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1e-5),
        min(abs(xcov), abs(ycov)),
    );
    var coverage = clamp(combined, 0.0, 1.0);
    let font_px = inst.viewport_info.z;
    if (font_px < 18.0 && coverage > 0.0) {
        let boost = clamp((18.0 - font_px) / 18.0, 0.0, 1.0) * 0.25;
        coverage = mix(coverage, sqrt(coverage), boost);
    }

    let alpha = coverage * inst.color.a;
    if (alpha <= 0.002) {
        discard;
    }

    var rgb = inst.color.rgb;
    if (inst.xform_row1.w > 0.5) {
        rgb = vec3<f32>(
            srgb_to_linear_channel(rgb.r),
            srgb_to_linear_channel(rgb.g),
            srgb_to_linear_channel(rgb.b),
        );
    }
    return vec4<f32>(rgb * alpha, alpha);
}
