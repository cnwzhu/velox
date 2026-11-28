// Velox Slug (2026 Public Domain) GPU 曲线字形直接光栅化着色器
//
// 基于 Eric Lengyel 公有领域 Slug 算法（US10373352B1 已于 2026-03-17 无条件捐献至公有领域）：
// - 零位图字形图集：直接由 TrueType 二次贝塞尔轮廓与双轴加速条带（Bands）实时解析求值；
// - 0.1x ~ 1000x 任意缩放与任意角度 OBB 旋转下均保持像素级锐利；
// - 动态顶点膨胀（Dynamic Glyph Dilation）+ 屏幕导数盒滤波抗锯齿（Box Filter Coverage）。

struct SlugCurve {
    p01: vec4<f32>, // (p0.x, p0.y, p1.x, p1.y)
    p2_pad: vec4<f32>, // (p2.x, p2.y, 0.0, 0.0)
};

struct SlugBand {
    start: u32,
    count: u32,
};

struct SlugGlyphInstance {
    // (origin_x_px, origin_y_px, font_px, angle_rad)
    origin_scale_rot: vec4<f32>,
    // (em_min_x, em_min_y, em_max_x, em_max_y)
    em_bounds: vec4<f32>,
    // (clip_x0, clip_y0, clip_x1, clip_y1)
    clip_rect: vec4<f32>,
    // straight RGBA [0..1]
    color: vec4<f32>,
    // (h_band_offset, num_h_bands, v_band_offset, num_v_bands)
    band_info: vec4<u32>,
    // (viewport_w, viewport_h, is_srgb, dilation_px)
    extra: vec4<f32>,
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

    let scale = max(inst.origin_scale_rot.z, 1.0);
    let dilate_px = max(inst.extra.w, 1.5);
    let dilate_em = dilate_px / scale;

    let min_em = inst.em_bounds.xy - vec2<f32>(dilate_em, dilate_em);
    let max_em = inst.em_bounds.zw + vec2<f32>(dilate_em, dilate_em);
    let em = mix(min_em, max_em, corner);

    let angle = inst.origin_scale_rot.w;
    let ca = cos(angle);
    let sa = sin(angle);

    // EM 空间 Y 向上；屏幕空间 Y 向下，支持任意角度 OBB 旋转
    let local_px = vec2<f32>(em.x * scale, -em.y * scale);
    let rot_px = vec2<f32>(
        local_px.x * ca - local_px.y * sa,
        local_px.x * sa + local_px.y * ca,
    );
    let screen = inst.origin_scale_rot.xy + rot_px;

    let vw = max(inst.extra.x, 1.0);
    let vh = max(inst.extra.y, 1.0);
    let ndc_x = (screen.x / vw) * 2.0 - 1.0;
    let ndc_y = 1.0 - (screen.y / vh) * 2.0;

    var out: VsOut;
    out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.em_pos = em;
    out.screen_pos = screen;
    out.inst_idx = iid;
    return out;
}

// Eric Lengyel Slug 2-bit 根贡献分类码（完美消除端点重合时的重复计数与 -0.0 伪影）：
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
        let c = curves[band_indices[i]];
        let p1 = c.p01.xy - em;
        let p2 = c.p01.zw - em;
        let p3 = c.p2_pad.xy - em;
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
        let c = curves[band_indices[i]];
        let p1 = c.p01.xy - em;
        let p2 = c.p01.zw - em;
        let p3 = c.p2_pad.xy - em;
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

    // 3. Slug 双轴融合 + 极小字号光学笔画轻微增强
    let combined = max(
        abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1e-5),
        min(abs(xcov), abs(ycov)),
    );
    var coverage = clamp(combined, 0.0, 1.0);
    // 远景小字号（< 18px）做适度光学加粗，防止细笔画发虚
    let font_px = inst.origin_scale_rot.z;
    if (font_px < 18.0 && coverage > 0.0) {
        let boost = clamp((18.0 - font_px) / 18.0, 0.0, 1.0) * 0.25;
        coverage = mix(coverage, sqrt(coverage), boost);
    }

    let alpha = coverage * inst.color.a;
    if (alpha <= 0.002) {
        discard;
    }

    var rgb = inst.color.rgb;
    if (inst.extra.z > 0.5) {
        rgb = vec3<f32>(
            srgb_to_linear_channel(rgb.r),
            srgb_to_linear_channel(rgb.g),
            srgb_to_linear_channel(rgb.b),
        );
    }
    return vec4<f32>(rgb * alpha, alpha);
}
