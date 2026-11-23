struct ViewportUniform {
    size: vec2<f32>,
    is_srgb: f32,
    _pad: f32,
};

// 严格 128 Bytes（8 × vec4<f32>），对齐 GPU Cache Line 与 WGSL std430：
// - bounds:       [x, y, w, h]
// - clip:         [x0, y0, x1, y1]
// - uv:           Glyph/Sprite 为 [u0, v0, u1, v1]；Arc 为 [start_rad, sweep_rad, fill_sweep_rad, thickness_px]
// - color0:       主填充色 / 文本色 / 阴影色 / Arc 轨道色
// - color1:       渐变终点色 / Arc 填充进度色
// - border_color: 边框颜色
// - params:       [corner_radius, border_or_blur, kind, horiz_grad]
// - extra:        [clip_radius, 0, 0, 0]
struct Primitive {
    bounds: vec4<f32>,
    clip: vec4<f32>,
    uv: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    border_color: vec4<f32>,
    params: vec4<f32>,
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> viewport: ViewportUniform;
@group(0) @binding(1) var<storage, read> primitives: array<Primitive>;
@group(0) @binding(2) var glyph_tex: texture_2d<f32>;
@group(0) @binding(3) var sprite_tex: texture_2d<f32>;
@group(0) @binding(4) var atlas_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) idx: u32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) iid: u32,
) -> VsOut {
    let prim = primitives[iid];
    let p0 = max(prim.bounds.xy, prim.clip.xy);
    let p1 = min(prim.bounds.xy + prim.bounds.zw, prim.clip.zw);

    var out: VsOut;
    out.idx = iid;
    if (p0.x >= p1.x || p0.y >= p1.y) {
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        return out;
    }

    var corner = vec2<f32>(0.0, 0.0);
    switch (vid) {
        case 0u: { corner = vec2<f32>(0.0, 0.0); }
        case 1u: { corner = vec2<f32>(1.0, 0.0); }
        case 2u: { corner = vec2<f32>(0.0, 1.0); }
        case 3u: { corner = vec2<f32>(0.0, 1.0); }
        case 4u: { corner = vec2<f32>(1.0, 0.0); }
        default: { corner = vec2<f32>(1.0, 1.0); }
    }

    let px = mix(p0, p1, corner);
    let ndc = vec2<f32>(
        (px.x / max(viewport.size.x, 1.0)) * 2.0 - 1.0,
        1.0 - (px.y / max(viewport.size.y, 1.0)) * 2.0,
    );
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    return out;
}

fn srgb_channel_to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn maybe_to_linear_straight(c: vec4<f32>) -> vec4<f32> {
    if (viewport.is_srgb > 0.5) {
        return vec4<f32>(
            srgb_channel_to_linear(c.r),
            srgb_channel_to_linear(c.g),
            srgb_channel_to_linear(c.b),
            c.a,
        );
    }
    return c;
}

fn sdf_rounded_box(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0.0, min(half_size.x, half_size.y));
    let q = abs(p - center) - (half_size - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Evan Wallace (Figma), "Fast Rounded Rectangle Shadows" (2018):
// 高精度误差函数 erf(x) 近似（向量化版本，最大误差 < 0.001）
fn erf_vec2(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    let t2 = t * t;
    return s - s / (t2 * t2);
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    let inv = x / sigma;
    return exp(-0.5 * inv * inv) / (2.50662827 * sigma);
}

// 给定纵向偏移 y，沿 X 轴求高斯核与圆角盒截面的解析积分
fn rounded_box_shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half_size: vec2<f32>) -> f32 {
    let delta = min(half_size.y - corner - abs(y), 0.0);
    let curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf_vec2((vec2<f32>(x) + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

// 4 点高斯积分求圆角矩形与 2D 高斯核的精确解析卷积阴影
fn rounded_box_shadow(center: vec2<f32>, half_size: vec2<f32>, p: vec2<f32>, sigma: f32, corner: f32) -> f32 {
    let pt = p - center;
    let low = pt.y - half_size.y;
    let high = pt.y + half_size.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) * 0.25;
    if (step <= 0.0001) {
        return 0.0;
    }
    var y = start + step * 0.5;
    var val = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        val = val + rounded_box_shadow_x(pt.x, pt.y - y, sigma, corner, half_size) * gaussian(y, sigma) * step;
        y = y + step;
    }
    return clamp(val, 0.0, 1.0);
}

// 带圆形端点的解析圆弧 SDF（0 rad = 12 点钟方向，顺时针增加）
fn sdf_arc_round(rel: vec2<f32>, r_arc: f32, half_thick: f32, start_rad: f32, sweep_rad: f32) -> f32 {
    let half_span = sweep_rad * 0.5;
    let mid = start_rad + half_span;
    // 12 点钟顺时针方向向量：(sin(a), -cos(a))
    let dir_mid = vec2<f32>(sin(mid), -cos(mid));
    let r_dist = length(rel);
    if (r_dist > 0.0001 && dot(rel, dir_mid) >= r_dist * cos(half_span)) {
        return abs(r_dist - r_arc) - half_thick;
    }
    let end_rad = start_rad + sweep_rad;
    let cap_a = vec2<f32>(sin(start_rad), -cos(start_rad)) * r_arc;
    let cap_b = vec2<f32>(sin(end_rad), -cos(end_rad)) * r_arc;
    return min(length(rel - cap_a), length(rel - cap_b)) - half_thick;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let prim = primitives[in.idx];
    let p = in.pos.xy;

    if (p.x < prim.clip.x || p.x > prim.clip.z || p.y < prim.clip.y || p.y > prim.clip.w) {
        discard;
    }

    // 圆角容器抗锯齿真裁切（overflow_hidden + rounded）
    var clip_cov = 1.0;
    if (prim.extra.x > 0.0) {
        let clip_half = (prim.clip.zw - prim.clip.xy) * 0.5;
        let clip_center = prim.clip.xy + clip_half;
        let sd_clip = sdf_rounded_box(p, clip_center, clip_half, prim.extra.x);
        clip_cov = clamp(0.5 - sd_clip, 0.0, 1.0);
        if (clip_cov <= 0.0) {
            discard;
        }
    }

    let kind = prim.params.z;

    if (kind < 0.5) {
        // Quad: 圆角/直角矩形 + 线性渐变 + 边框（单次 SDF 计算）
        let half_size = prim.bounds.zw * 0.5;
        let center = prim.bounds.xy + half_size;
        let radius = prim.params.x;
        let bw = prim.params.y;
        let sd_outer = sdf_rounded_box(p, center, half_size, radius);
        let outer_cov = clamp(0.5 - sd_outer, 0.0, 1.0) * clip_cov;
        if (outer_cov <= 0.0) {
            discard;
        }

        let horiz = prim.params.w > 0.5;
        let grad_t = select(
            clamp((p.y - prim.bounds.y) / max(prim.bounds.w, 1.0), 0.0, 1.0),
            clamp((p.x - prim.bounds.x) / max(prim.bounds.z, 1.0), 0.0, 1.0),
            horiz,
        );
        let fill_col = maybe_to_linear_straight(mix(prim.color0, prim.color1, grad_t));
        let border_col = maybe_to_linear_straight(prim.border_color);

        if (bw > 0.0 && border_col.a > 0.0) {
            let sd_inner = sd_outer + bw;
            let inner_cov = clamp(0.5 - sd_inner, 0.0, 1.0);
            let border_pm = vec4<f32>(border_col.rgb * border_col.a, border_col.a);
            if (fill_col.a > 0.0) {
                let fill_pm = vec4<f32>(fill_col.rgb * fill_col.a, fill_col.a);
                return mix(border_pm, fill_pm, inner_cov) * outer_cov;
            }
            let ring_cov = outer_cov * (1.0 - inner_cov);
            if (ring_cov <= 0.0) {
                discard;
            }
            return border_pm * ring_cov;
        }

        if (fill_col.a <= 0.0) {
            discard;
        }
        return vec4<f32>(fill_col.rgb * fill_col.a, fill_col.a) * outer_cov;
    } else if (kind < 1.5) {
        // SoftShadow: Evan Wallace (Figma) 解析圆角盒高斯卷积阴影
        let half_size = prim.bounds.zw * 0.5;
        let center = prim.bounds.xy + half_size;
        let radius = prim.params.x;
        let blur = max(prim.params.y, 1.0);
        let inner_half = max(half_size - vec2<f32>(blur), vec2<f32>(0.5));
        let sigma = max(blur * 0.45, 0.5);
        let corner = min(radius, min(inner_half.x, inner_half.y));
        let cov = rounded_box_shadow(center, inner_half, p, sigma, corner) * clip_cov;
        if (cov <= 0.001) {
            discard;
        }
        let col = maybe_to_linear_straight(prim.color0);
        return vec4<f32>(col.rgb * col.a, col.a) * cov;
    } else if (kind < 2.5) {
        // Glyph: R8Unorm 动态字形图集
        let local_uv = clamp((p - prim.bounds.xy) / max(prim.bounds.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
        let atlas_uv = mix(prim.uv.xy, prim.uv.zw, local_uv);
        let cov = textureSampleLevel(glyph_tex, atlas_sampler, atlas_uv, 0.0).r * clip_cov;
        if (cov <= 0.0) {
            discard;
        }
        let col = maybe_to_linear_straight(prim.color0);
        return vec4<f32>(col.rgb * col.a, col.a) * cov;
    } else if (kind < 3.5) {
        // Sprite: Rgba8Unorm 预乘 Alpha 精灵/自定义 SDF 画布图集
        let local_uv = clamp((p - prim.bounds.xy) / max(prim.bounds.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
        let atlas_uv = mix(prim.uv.xy, prim.uv.zw, local_uv);
        let s = textureSampleLevel(sprite_tex, atlas_sampler, atlas_uv, 0.0);
        if (s.a <= 0.0) {
            discard;
        }
        if (viewport.is_srgb > 0.5) {
            let unpm = s.rgb / max(s.a, 0.0001);
            let lin = vec3<f32>(
                srgb_channel_to_linear(unpm.r),
                srgb_channel_to_linear(unpm.g),
                srgb_channel_to_linear(unpm.b),
            );
            return vec4<f32>(lin * s.a, s.a) * clip_cov;
        }
        return s * clip_cov;
    } else {
        // Arc: 原生 GPU 解析圆弧 SDF（轨道 + 进度弧一次片元合成，零 CPU 光栅与纹理上传）
        let half_size = prim.bounds.zw * 0.5;
        let center = prim.bounds.xy + half_size;
        let r_max = min(half_size.x, half_size.y);
        let half_thick = clamp(prim.uv.w * 0.5, 0.75, r_max * 0.45);
        let r_arc = max(r_max - half_thick - 1.0, 1.0);
        let rel = p - center;

        let start_rad = prim.uv.x;
        let sweep_rad = prim.uv.y;
        let fill_sweep = prim.uv.z;

        var out_pm = vec4<f32>(0.0);
        let track_col = maybe_to_linear_straight(prim.color0);
        if (track_col.a > 0.0 && sweep_rad > 0.001) {
            let sd_t = sdf_arc_round(rel, r_arc, half_thick, start_rad, sweep_rad);
            let cov_t = clamp(0.5 - sd_t, 0.0, 1.0);
            if (cov_t > 0.0) {
                out_pm = vec4<f32>(track_col.rgb * track_col.a, track_col.a) * cov_t;
            }
        }

        let fill_col = maybe_to_linear_straight(prim.color1);
        if (fill_col.a > 0.0 && fill_sweep > 0.001) {
            let sd_f = sdf_arc_round(rel, r_arc, half_thick, start_rad, fill_sweep);
            let cov_f = clamp(0.5 - sd_f, 0.0, 1.0);
            if (cov_f > 0.0) {
                let src_pm = vec4<f32>(fill_col.rgb * fill_col.a, fill_col.a) * cov_f;
                out_pm = src_pm + out_pm * (1.0 - src_pm.a);
            }
        }

        out_pm = out_pm * clip_cov;
        if (out_pm.a <= 0.0) {
            discard;
        }
        return out_pm;
    }
}
