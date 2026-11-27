struct ViewportUniform {
    size: vec2<f32>,
    is_srgb: f32,
    _pad: f32,
};

// 严格 192 Bytes（12 × vec4<f32>），对齐 GPU Cache Line 与 WGSL std430：
// - bounds:       [x, y, w, h]
// - clip:         [x0, y0, x1, y1]
// - uv:           Glyph/Sprite 为 [u0, v0, u1, v1]；Arc 为 [start_rad, sweep_rad, fill_sweep_rad, thickness_px]
// - color0:       主填充色 / 文本色 / 阴影色 / Arc 轨道色
// - color1:       2色渐变终点色 / Arc 填充进度色
// - border_color: 边框颜色
// - params:       [corner_radius_legacy, border_or_blur, kind, horiz_grad]
// - extra:        [clip_radius, grad_kind(0=2color,1=linear,2=radial,3=conic), ramp_v, extend_mode(0=pad,1=repeat,2=reflect)]
// - radii:        四角独立圆角 [tl, tr, br, bl]
// - grad_a:       Linear:[sx,sy,ex,ey]; Radial:[cx,cy,r1,fx]; Conic:[cx,cy,start_rad,sweep_rad]
// - grad_b:       [fy, r0, tx_px, ty_px]（后两位兼任 2D 仿射平移分量）
// - xform:        2D 仿射变换逆线性矩阵 [inv_a, inv_b, inv_c, inv_d]（默认单位阵 [1, 0, 0, 1]）
struct Primitive {
    bounds: vec4<f32>,
    clip: vec4<f32>,
    uv: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    border_color: vec4<f32>,
    params: vec4<f32>,
    extra: vec4<f32>,
    radii: vec4<f32>,
    grad_a: vec4<f32>,
    grad_b: vec4<f32>,
    xform: vec4<f32>,
};

@group(0) @binding(0) var<uniform> viewport: ViewportUniform;
@group(0) @binding(1) var<storage, read> primitives: array<Primitive>;
@group(0) @binding(2) var glyph_tex: texture_2d<f32>;
@group(0) @binding(3) var sprite_tex: texture_2d<f32>;
@group(0) @binding(4) var atlas_sampler: sampler;
@group(0) @binding(5) var ramp_tex: texture_2d<f32>;
@group(0) @binding(6) var linear_sampler: sampler;

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
    let has_xform = abs(prim.xform.x - 1.0) > 0.0001
        || abs(prim.xform.y) > 0.0001
        || abs(prim.xform.z) > 0.0001
        || abs(prim.xform.w - 1.0) > 0.0001
        || abs(prim.grad_b.z) > 0.0001
        || abs(prim.grad_b.w) > 0.0001;

    var corner = vec2<f32>(0.0, 0.0);
    switch (vid) {
        case 0u: { corner = vec2<f32>(0.0, 0.0); }
        case 1u: { corner = vec2<f32>(1.0, 0.0); }
        case 2u: { corner = vec2<f32>(0.0, 1.0); }
        case 3u: { corner = vec2<f32>(0.0, 1.0); }
        case 4u: { corner = vec2<f32>(1.0, 0.0); }
        default: { corner = vec2<f32>(1.0, 1.0); }
    }

    var out: VsOut;
    out.idx = iid;

    var px = vec2<f32>(0.0, 0.0);
    if (!has_xform) {
        let p0 = max(prim.bounds.xy, prim.clip.xy);
        let p1 = min(prim.bounds.xy + prim.bounds.zw, prim.clip.zw);
        if (p0.x >= p1.x || p0.y >= p1.y) {
            out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
            return out;
        }
        px = mix(p0, p1, corner);
    } else {
        // 由 2x2 逆线性矩阵还原正向线性矩阵，精确变换四角并外扩 2px 抗锯齿余量
        let ia = prim.xform.x;
        let ib = prim.xform.y;
        let ic = prim.xform.z;
        let id = prim.xform.w;
        let det = ia * id - ib * ic;
        if (abs(det) < 0.000001) {
            out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
            return out;
        }
        let inv_det = 1.0 / det;
        let fa = id * inv_det;
        let fb = -ib * inv_det;
        let fc = -ic * inv_det;
        let fd = ia * inv_det;

        let half_s = prim.bounds.zw * 0.5;
        let center = prim.bounds.xy + half_s;
        let screen_center = center + prim.grad_b.zw;
        let local_rel = (corner - vec2<f32>(0.5, 0.5)) * (prim.bounds.zw + vec2<f32>(4.0, 4.0));
        px = screen_center + vec2<f32>(
            fa * local_rel.x + fc * local_rel.y,
            fb * local_rel.x + fd * local_rel.y,
        );
    }

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

// 四角独立圆角 SDF：radii = [tl, tr, br, bl]
fn sdf_rounded_box_4(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let rel = p - center;
    let r_pair = select(radii.yz, radii.xw, rel.x < 0.0); // [tr, br] or [tl, bl]
    let raw_r = select(r_pair.y, r_pair.x, rel.y < 0.0);
    let r = clamp(raw_r, 0.0, min(half_size.x, half_size.y));
    let q = abs(rel) - (half_size - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Interleaved Gradient Noise（Jimenez 2014）：消除 8-bit 渐变与大阴影色带（banding）
fn ign_dither(p: vec2<f32>) -> f32 {
    let magic = vec3<f32>(0.06711056, 0.00583715, 52.9829189);
    return fract(magic.z * fract(dot(p, magic.xy))) - 0.5;
}

fn apply_extend_mode(t: f32, mode: f32) -> f32 {
    if (mode > 1.5) {
        // Reflect
        let f = fract(t * 0.5) * 2.0;
        return clamp(select(2.0 - f, f, f <= 1.0), 0.0, 1.0);
    } else if (mode > 0.5) {
        // Repeat
        return clamp(fract(t), 0.0, 1.0);
    }
    return clamp(t, 0.0, 1.0);
}

fn sample_gradient_ramp(prim: Primitive, lp: vec2<f32>, screen_p: vec2<f32>) -> vec4<f32> {
    let u = (lp.x - prim.bounds.x) / max(prim.bounds.z, 1.0);
    let v = (lp.y - prim.bounds.y) / max(prim.bounds.w, 1.0);
    let gk = prim.extra.y;
    let ext = prim.extra.w;

    var t_raw = 0.0;
    var valid = true;

    if (gk < 1.5) {
        // 1 = Linear Gradient
        let s = prim.grad_a.xy;
        let e = prim.grad_a.zw;
        let d = e - s;
        let denom = dot(d, d);
        if (denom > 0.0000001) {
            t_raw = dot(vec2<f32>(u, v) - s, d) / denom;
        }
    } else if (gk < 2.5) {
        // 2 = Radial Gradient（支持同心圆与双圆焦点偏移）
        let c = prim.grad_a.xy;
        let r1 = max(prim.grad_a.z, 0.00001);
        let f = vec2<f32>(prim.grad_a.w, prim.grad_b.x);
        let r0 = max(prim.grad_b.y, 0.0);
        let cd = c - f;
        if (dot(cd, cd) < 0.000001 && r0 < 0.000001) {
            t_raw = length(vec2<f32>(u, v) - c) / r1;
        } else {
            let dp = vec2<f32>(u, v) - f;
            let dr = r1 - r0;
            let a = dot(cd, cd) - dr * dr;
            let b = -2.0 * (dot(dp, cd) + r0 * dr);
            let ct = dot(dp, dp) - r0 * r0;
            if (abs(a) < 0.000001) {
                if (abs(b) < 0.000001) {
                    valid = false;
                } else {
                    t_raw = -ct / b;
                }
            } else {
                let disc = b * b - 4.0 * a * ct;
                if (disc < 0.0) {
                    valid = false;
                } else {
                    let sd = sqrt(disc);
                    let r_a = (-b + sd) / (2.0 * a);
                    let r_b = (-b - sd) / (2.0 * a);
                    let t_hi = max(r_a, r_b);
                    let t_lo = min(r_a, r_b);
                    if (r0 + t_hi * dr >= 0.0) {
                        t_raw = t_hi;
                    } else if (r0 + t_lo * dr >= 0.0) {
                        t_raw = t_lo;
                    } else {
                        valid = false;
                    }
                }
            }
        }
    } else {
        // 3 = Conic Gradient（12 点钟方向 0 rad 顺时针）
        let c = prim.grad_a.xy;
        let dp = vec2<f32>(u, v) - c;
        let tau = 6.28318530718;
        var ang = atan2(dp.x, -dp.y);
        if (ang < 0.0) {
            ang = ang + tau;
        }
        var rel = ang - prim.grad_a.z;
        if (rel < 0.0) {
            rel = rel + tau;
        }
        t_raw = rel / max(prim.grad_a.w, 0.0001);
    }

    if (!valid) {
        return vec4<f32>(0.0);
    }

    let t = apply_extend_mode(t_raw, ext);
    let ramp_u = (0.5 + t * 255.0) / 256.0;
    let raw_col = textureSampleLevel(ramp_tex, linear_sampler, vec2<f32>(ramp_u, prim.extra.z), 0.0);
    let dither = ign_dither(screen_p) / 255.0;
    let dithered = vec4<f32>(
        clamp(raw_col.r + dither, 0.0, 1.0),
        clamp(raw_col.g + dither, 0.0, 1.0),
        clamp(raw_col.b + dither, 0.0, 1.0),
        raw_col.a,
    );
    return maybe_to_linear_straight(dithered);
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

fn rounded_box_shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half_size: vec2<f32>) -> f32 {
    let delta = min(half_size.y - corner - abs(y), 0.0);
    let curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf_vec2((vec2<f32>(x) + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

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
        // Quad / StyledQuad: 四角独立圆角 + 2D 仿射变换 + 完整渐变 LUT + 边框
        let half_size = prim.bounds.zw * 0.5;
        let center = prim.bounds.xy + half_size;
        let screen_center = center + prim.grad_b.zw;
        let rel_s = p - screen_center;
        let lp = center + vec2<f32>(
            prim.xform.x * rel_s.x + prim.xform.z * rel_s.y,
            prim.xform.y * rel_s.x + prim.xform.w * rel_s.y,
        );

        let bw = prim.params.y;
        let sd_outer = sdf_rounded_box_4(lp, center, half_size, prim.radii);
        let outer_cov = clamp(0.5 - sd_outer, 0.0, 1.0) * clip_cov;
        if (outer_cov <= 0.0) {
            discard;
        }

        var fill_col = vec4<f32>(0.0);
        if (prim.extra.y > 0.5) {
            fill_col = sample_gradient_ramp(prim, lp, p);
        } else {
            let horiz = prim.params.w > 0.5;
            let grad_t = select(
                clamp((lp.y - prim.bounds.y) / max(prim.bounds.w, 1.0), 0.0, 1.0),
                clamp((lp.x - prim.bounds.x) / max(prim.bounds.z, 1.0), 0.0, 1.0),
                horiz,
            );
            fill_col = maybe_to_linear_straight(mix(prim.color0, prim.color1, grad_t));
        }
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
        // SoftShadow: Evan Wallace (Figma) 解析圆角盒高斯卷积阴影 + 防色带抖动
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
        let dither = ign_dither(p) / 255.0;
        let eff_cov = clamp(cov + dither * 0.5, 0.0, 1.0);
        return vec4<f32>(col.rgb * col.a, col.a) * eff_cov;
    } else if (kind < 2.5) {
        // Glyph: R8Unorm 动态字形图集（像素对齐锐利采样）
        let local_uv = clamp((p - prim.bounds.xy) / max(prim.bounds.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
        let atlas_uv = mix(prim.uv.xy, prim.uv.zw, local_uv);
        let cov = textureSampleLevel(glyph_tex, atlas_sampler, atlas_uv, 0.0).r * clip_cov;
        if (cov <= 0.0) {
            discard;
        }
        let col = maybe_to_linear_straight(prim.color0);
        return vec4<f32>(col.rgb * col.a, col.a) * cov;
    } else if (kind < 3.5) {
        // Sprite: Rgba8Unorm 预乘 Alpha 精灵图集（1:1 精确命中纹素中心，缩放时半纹素钳位防邻边渗色）
        let tex_size = vec2<f32>(textureDimensions(sprite_tex));
        let half_tex = vec2<f32>(0.5) / max(tex_size, vec2<f32>(1.0));
        let uv_min = prim.uv.xy + half_tex;
        let uv_max = max(prim.uv.zw - half_tex, uv_min);
        let local_uv = clamp((p - prim.bounds.xy) / max(prim.bounds.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
        let atlas_uv = clamp(mix(prim.uv.xy, prim.uv.zw, local_uv), uv_min, uv_max);
        let s = textureSampleLevel(sprite_tex, linear_sampler, atlas_uv, 0.0);
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
        // Arc: 原生 GPU 解析圆弧 SDF
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
