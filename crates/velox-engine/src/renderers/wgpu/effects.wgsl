// 多 Pass 后处理与离屏图层合成着色器：
// - Dual Kawase Blur（降采样 / 升采样金字塔）
// - 全套 27 种 Porter-Duff + W3C Separable + W3C Non-Separable HSL 混合模式
// - 4x5 ColorMatrix 色彩滤镜
// - 实时毛玻璃（Backdrop Blur + ColorMatrix + Acrylic 颗粒 + Tint + 圆角/Path 掩码）
// - 任意子树轮廓 DropShadow 提取与 Blit

struct EffectUniform {
    // [dst_w, dst_h, src_tex_w, src_tex_h]
    tex_sizes: vec4<f32>,
    // 图层在目标画布上的绘制包围盒 [x, y, w, h]（物理像素）
    dst_rect: vec4<f32>,
    // 源纹理中的有效区域 [x, y, w, h]（物理像素）
    src_rect: vec4<f32>,
    // 目标画布有效裁剪框 [x0, y0, x1, y1]（物理像素）
    clip_rect: vec4<f32>,
    // 图层主体区域 [x, y, w, h]（用于圆角/Path 掩码与变换中心）
    body_rect: vec4<f32>,
    // 四角独立圆角 [tl, tr, br, bl]
    radii: vec4<f32>,
    // [opacity, blend_mode_f32, mode_kind(0=composite,1=backdrop,2=kawase_down,3=kawase_up,4=shadow_extract,5=blit), kawase_offset]
    params: vec4<f32>,
    // [has_color_matrix, has_path_mask, noise_scale, is_srgb]
    flags: vec4<f32>,
    // 毛玻璃着色 / 阴影颜色（直通 RGBA）
    tint: vec4<f32>,
    // 2D 仿射变换逆线性矩阵 [inv_a, inv_b, inv_c, inv_d]
    inv_xform: vec4<f32>,
    // [tx, ty, dst_tex_w, dst_tex_h]
    extra: vec4<f32>,
    // 4x5 色彩矩阵（行 0..3 前 4 列 + 第 5 列偏移）
    cm0: vec4<f32>,
    cm1: vec4<f32>,
    cm2: vec4<f32>,
    cm3: vec4<f32>,
    cm_bias: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: EffectUniform;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var dst_tex: texture_2d<f32>;
@group(0) @binding(3) var mask_tex: texture_2d<f32>;
@group(0) @binding(4) var linear_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    var out: VsOut;
    // 仅覆盖目标画布内 `dst_rect` 的四边形（6 顶点）以最小化像素填充带宽
    var c = vec2<f32>(0.0, 0.0);
    switch (vid) {
        case 0u: { c = vec2<f32>(0.0, 0.0); }
        case 1u: { c = vec2<f32>(1.0, 0.0); }
        case 2u: { c = vec2<f32>(0.0, 1.0); }
        case 3u: { c = vec2<f32>(0.0, 1.0); }
        case 4u: { c = vec2<f32>(1.0, 0.0); }
        default: { c = vec2<f32>(1.0, 1.0); }
    }
    let px = u.dst_rect.xy + c * u.dst_rect.zw;
    let ndc = vec2<f32>(
        (px.x / max(u.tex_sizes.x, 1.0)) * 2.0 - 1.0,
        1.0 - (px.y / max(u.tex_sizes.y, 1.0)) * 2.0,
    );
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = c;
    return out;
}

fn sdf_rounded_box_4(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let rel = p - center;
    let r_pair = select(radii.yz, radii.xw, rel.x < 0.0);
    let raw_r = select(r_pair.y, r_pair.x, rel.y < 0.0);
    let r = clamp(raw_r, 0.0, min(half_size.x, half_size.y));
    let q = abs(rel) - (half_size - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn apply_color_matrix_premul(pm: vec4<f32>) -> vec4<f32> {
    if (u.flags.x < 0.5) {
        return pm;
    }
    let a = clamp(pm.a, 0.0, 1.0);
    var st = vec4<f32>(0.0);
    if (a > 0.00001) {
        st = vec4<f32>(clamp(pm.rgb / a, vec3<f32>(0.0), vec3<f32>(1.0)), a);
    }
    let r = clamp(dot(u.cm0, st) + u.cm_bias.x, 0.0, 1.0);
    let g = clamp(dot(u.cm1, st) + u.cm_bias.y, 0.0, 1.0);
    let b = clamp(dot(u.cm2, st) + u.cm_bias.z, 0.0, 1.0);
    let na = clamp(dot(u.cm3, st) + u.cm_bias.w, 0.0, 1.0);
    return vec4<f32>(vec3<f32>(r, g, b) * na, na);
}

// --- W3C Compositing & Blending Level 1 辅助函数 ---

fn lum3(c: vec3<f32>) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

fn clip_color3(c_in: vec3<f32>) -> vec3<f32> {
    var c = c_in;
    let l = lum3(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    if (n < 0.0 && (l - n) > 0.000001) {
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * (l / (l - n));
    }
    if (x > 1.0 && (x - l) > 0.000001) {
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * ((1.0 - l) / (x - l));
    }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn set_lum3(c: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum3(c);
    return clip_color3(c + vec3<f32>(d));
}

fn sat3(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn set_sat3(c: vec3<f32>, s: f32) -> vec3<f32> {
    let cmin = min(c.r, min(c.g, c.b));
    let cmax = max(c.r, max(c.g, c.b));
    let span = cmax - cmin;
    if (span <= 0.000001) {
        return vec3<f32>(0.0);
    }
    return ((c - vec3<f32>(cmin)) / span) * s;
}

fn blend_sep_ch(cb: f32, cs: f32, mode: u32) -> f32 {
    switch (mode) {
        case 12u: { return cb * cs; } // Multiply
        case 13u: { return cb + cs - cb * cs; } // Screen
        case 14u: { return select(1.0 - 2.0 * (1.0 - cb) * (1.0 - cs), 2.0 * cb * cs, cb <= 0.5); } // Overlay
        case 15u: { return min(cb, cs); } // Darken
        case 16u: { return max(cb, cs); } // Lighten
        case 17u: { // ColorDodge
            if (cb <= 0.0) { return 0.0; }
            if (cs >= 1.0) { return 1.0; }
            return min(1.0, cb / (1.0 - cs));
        }
        case 18u: { // ColorBurn
            if (cb >= 1.0) { return 1.0; }
            if (cs <= 0.0) { return 0.0; }
            return 1.0 - min(1.0, (1.0 - cb) / cs);
        }
        case 19u: { return select(1.0 - 2.0 * (1.0 - cb) * (1.0 - cs), 2.0 * cb * cs, cs <= 0.5); } // HardLight
        case 20u: { // SoftLight
            if (cs <= 0.5) {
                return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
            }
            let d = select(sqrt(cb), ((16.0 * cb - 12.0) * cb + 4.0) * cb, cb <= 0.25);
            return cb + (2.0 * cs - 1.0) * (d - cb);
        }
        case 21u: { return abs(cb - cs); } // Difference
        case 22u: { return cb + cs - 2.0 * cb * cs; } // Exclusion
        default: { return cs; }
    }
}

fn blend_premul(src_pm: vec4<f32>, dst_pm: vec4<f32>, mode: u32) -> vec4<f32> {
    let sa = clamp(src_pm.a, 0.0, 1.0);
    let da = clamp(dst_pm.a, 0.0, 1.0);
    let inv_sa = 1.0 - sa;
    let inv_da = 1.0 - da;

    switch (mode) {
        case 0u: { return src_pm + dst_pm * inv_sa; } // SrcOver
        case 1u: { return vec4<f32>(0.0); } // Clear
        case 2u: { return src_pm; } // Src
        case 3u: { return dst_pm; } // Dst
        case 4u: { return src_pm * da; } // SrcIn
        case 5u: { return dst_pm * sa; } // DstIn
        case 6u: { return src_pm * inv_da; } // SrcOut
        case 7u: { return dst_pm * inv_sa; } // DstOut
        case 8u: { return vec4<f32>(src_pm.rgb * da + dst_pm.rgb * inv_sa, da); } // SrcAtop
        case 9u: { return vec4<f32>(dst_pm.rgb * sa + src_pm.rgb * inv_da, sa); } // DstAtop
        case 10u: { return src_pm * inv_da + dst_pm * inv_sa; } // Xor
        case 11u: { return min(src_pm + dst_pm, vec4<f32>(1.0)); } // Plus
        default: {}
    }

    var cs = vec3<f32>(0.0);
    if (sa > 0.00001) {
        cs = clamp(src_pm.rgb / sa, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    var cb = vec3<f32>(0.0);
    if (da > 0.00001) {
        cb = clamp(dst_pm.rgb / da, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    var b = cs;
    if (mode == 23u) {
        b = set_lum3(set_sat3(cs, sat3(cb)), lum3(cb));
    } else if (mode == 24u) {
        b = set_lum3(set_sat3(cb, sat3(cs)), lum3(cb));
    } else if (mode == 25u) {
        b = set_lum3(cs, lum3(cb));
    } else if (mode == 26u) {
        b = set_lum3(cb, lum3(cs));
    } else {
        b = vec3<f32>(
            blend_sep_ch(cb.r, cs.r, mode),
            blend_sep_ch(cb.g, cs.g, mode),
            blend_sep_ch(cb.b, cs.b, mode),
        );
    }

    let sada = sa * da;
    let rgb = clamp(src_pm.rgb * inv_da + dst_pm.rgb * inv_sa + sada * b, vec3<f32>(0.0), vec3<f32>(1.0));
    let ao = clamp(sa + da * inv_sa, 0.0, 1.0);
    return vec4<f32>(rgb, ao);
}

fn acrylic_noise(p: vec2<f32>) -> f32 {
    let magic = vec3<f32>(0.06711056, 0.00583715, 52.9829189);
    return fract(magic.z * fract(dot(p, magic.xy))) - 0.5;
}

fn sample_clamped(tex: texture_2d<f32>, uv: vec2<f32>, min_uv: vec2<f32>, max_uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(tex, linear_sampler, clamp(uv, min_uv, max_uv), 0.0);
}

fn srgb_ch_to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn maybe_linear(c: vec4<f32>) -> vec4<f32> {
    if (u.flags.w > 0.5) {
        return vec4<f32>(
            srgb_ch_to_linear(c.r),
            srgb_ch_to_linear(c.g),
            srgb_ch_to_linear(c.b),
            c.a,
        );
    }
    return c;
}

@fragment
fn fs_effect(in: VsOut) -> @location(0) vec4<f32> {
    let kind = u32(u.params.z + 0.5);
    let inv_src = vec2<f32>(1.0 / max(u.tex_sizes.z, 1.0), 1.0 / max(u.tex_sizes.w, 1.0));
    let src_min_uv = (u.src_rect.xy + vec2<f32>(0.5)) * inv_src;
    let src_max_uv = (u.src_rect.xy + u.src_rect.zw - vec2<f32>(0.5)) * inv_src;

    if (kind == 5u) {
        // 5 = 全屏/局部直通 Blit
        let suv = (u.src_rect.xy + in.uv * u.src_rect.zw) * inv_src;
        return textureSampleLevel(src_tex, linear_sampler, suv, 0.0);
    }

    if (kind == 2u) {
        // 2 = Dual Kawase Downsample（5 点加权核）
        let suv = (u.src_rect.xy + in.uv * u.src_rect.zw) * inv_src;
        let hp = inv_src * u.params.w;
        var sum = sample_clamped(src_tex, suv, src_min_uv, src_max_uv) * 4.0;
        sum = sum + sample_clamped(src_tex, suv - hp, src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + hp, src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(hp.x, -hp.y), src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(-hp.x, hp.y), src_min_uv, src_max_uv);
        return sum * 0.125;
    }

    if (kind == 3u) {
        // 3 = Dual Kawase Upsample（8 点加权核）
        let suv = (u.src_rect.xy + in.uv * u.src_rect.zw) * inv_src;
        let hp = inv_src * u.params.w;
        var sum = sample_clamped(src_tex, suv + vec2<f32>(-hp.x * 2.0, 0.0), src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(hp.x * 2.0, 0.0), src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(0.0, -hp.y * 2.0), src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(0.0, hp.y * 2.0), src_min_uv, src_max_uv);
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(-hp.x, hp.y), src_min_uv, src_max_uv) * 2.0;
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(hp.x, hp.y), src_min_uv, src_max_uv) * 2.0;
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(-hp.x, -hp.y), src_min_uv, src_max_uv) * 2.0;
        sum = sum + sample_clamped(src_tex, suv + vec2<f32>(hp.x, -hp.y), src_min_uv, src_max_uv) * 2.0;
        return sum * (1.0 / 12.0);
    }

    if (kind == 4u) {
        // 4 = DropShadow Alpha 轮廓提取（支持同步叠加 SVG Path 矢量遮罩）
        let suv = (u.src_rect.xy + in.uv * u.src_rect.zw) * inv_src;
        let tc = maybe_linear(u.tint);
        var a = textureSampleLevel(src_tex, linear_sampler, suv, 0.0).a * tc.a;
        if (u.flags.y > 0.5) {
            let lp = in.pos.xy;
            let muv = (lp - u.body_rect.xy) / max(u.body_rect.zw, vec2<f32>(1.0));
            if (muv.x < 0.0 || muv.x > 1.0 || muv.y < 0.0 || muv.y > 1.0) {
                a = 0.0;
            } else {
                a = a * textureSampleLevel(mask_tex, linear_sampler, muv, 0.0).r;
            }
        }
        return vec4<f32>(tc.rgb * a, a);
    }

    let p = in.pos.xy;
    if (p.x < u.clip_rect.x || p.x > u.clip_rect.z || p.y < u.clip_rect.y || p.y > u.clip_rect.w) {
        discard;
    }

    let inv_dst = vec2<f32>(1.0 / max(u.extra.z, 1.0), 1.0 / max(u.extra.w, 1.0));
    let hw_blend = u.params.y < -0.5;

    if (kind == 6u) {
        // 6 = 外部零拷贝 GPU 纹理采样（支持四角独立圆角 SDF 裁剪、UV 裁切、预乘/直通 Alpha 与 27 种混合）
        let half_b = u.body_rect.zw * 0.5;
        let center_b = u.body_rect.xy + half_b;
        let cov = clamp(0.5 - sdf_rounded_box_4(p, center_b, half_b, u.radii), 0.0, 1.0);
        if (cov <= 0.0001) {
            if (hw_blend) {
                discard;
            }
            let duv = (p - u.extra.xy) * inv_dst;
            return textureSampleLevel(dst_tex, linear_sampler, duv, 0.0);
        }
        let local_uv = clamp(
            (p - u.body_rect.xy) / max(u.body_rect.zw, vec2<f32>(1.0)),
            vec2<f32>(0.0),
            vec2<f32>(1.0),
        );
        // `u.src_rect` 保存归一化 UV 区间 `[u0, v0, u1, v1]`
        let suv = mix(u.src_rect.xy, u.src_rect.zw, local_uv);
        let sampled = textureSampleLevel(src_tex, linear_sampler, suv, 0.0);
        var src_pm = sampled;
        if (u.flags.x < 0.5) {
            // 直通 Alpha：转换为预乘格式
            let c = maybe_linear(sampled);
            src_pm = vec4<f32>(c.rgb * c.a, c.a);
        } else if (u.flags.w > 0.5) {
            // 已预乘 sRGB -> Linear 预乘
            let a = max(sampled.a, 0.0001);
            let unpm = sampled.rgb / a;
            let lin = vec3<f32>(
                srgb_ch_to_linear(unpm.r),
                srgb_ch_to_linear(unpm.g),
                srgb_ch_to_linear(unpm.b),
            );
            src_pm = vec4<f32>(lin * sampled.a, sampled.a);
        }
        src_pm = src_pm * (cov * u.params.x);
        if (hw_blend) {
            return src_pm;
        }
        let duv = (p - u.extra.xy) * inv_dst;
        let dst_pm = textureSampleLevel(dst_tex, linear_sampler, duv, 0.0);
        let blend_mode = u32(u.params.y + 0.5);
        return blend_premul(src_pm, dst_pm, blend_mode);
    }

    if (kind == 1u) {
        // 1 = BackdropFilter 毛玻璃合成（使用局部快照偏移 u.extra.xy，零全屏拷贝）
        // `u.src_rect` 保存 `sample_rect` 在父画布中的物理坐标 `[sx, sy, sw, sh]`，
        // 经 Dual Kawase Blur 后的有效像素位于 `src_tex` 的 `[0..sw, 0..sh]` 区间。
        let duv = (p - u.extra.xy) * inv_dst;
        let dst_pm = textureSampleLevel(dst_tex, linear_sampler, duv, 0.0);
        let half_b = u.body_rect.zw * 0.5;
        let center_b = u.body_rect.xy + half_b;
        var cov = clamp(0.5 - sdf_rounded_box_4(p, center_b, half_b, u.radii), 0.0, 1.0);
        if (u.flags.y > 0.5) {
            let muv = (p - u.body_rect.xy) / max(u.body_rect.zw, vec2<f32>(1.0));
            cov = cov * textureSampleLevel(mask_tex, linear_sampler, clamp(muv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).r;
        }
        cov = cov * u.params.x;
        if (cov <= 0.0001) {
            return dst_pm;
        }
        let bd_min = vec2<f32>(0.5) * inv_src;
        let bd_max = (u.src_rect.zw - vec2<f32>(0.5)) * inv_src;
        let suv = (p - u.src_rect.xy) * inv_src;
        var bd = apply_color_matrix_premul(sample_clamped(src_tex, suv, bd_min, bd_max));
        if (u.flags.z > 0.0) {
            let n = acrylic_noise(p) * u.flags.z;
            bd = vec4<f32>(clamp(bd.rgb + vec3<f32>(n * bd.a), vec3<f32>(0.0), vec3<f32>(bd.a)), bd.a);
        }
        let tc = maybe_linear(u.tint);
        if (tc.a > 0.0) {
            let tint_pm = vec4<f32>(tc.rgb * tc.a, tc.a);
            bd = tint_pm + bd * (1.0 - tint_pm.a);
        }
        return mix(dst_pm, bd, cov);
    }

    // 0 = 常规离屏图层合成（Transform2D + ColorMatrix + PathMask + Opacity + 27 种 BlendMode）
    let body_half = u.body_rect.zw * 0.5;
    let body_center = u.body_rect.xy + body_half;
    let screen_center = body_center + u.extra.xy;
    let rel = p - screen_center;
    let lp = body_center + vec2<f32>(
        u.inv_xform.x * rel.x + u.inv_xform.z * rel.y,
        u.inv_xform.y * rel.x + u.inv_xform.w * rel.y,
    );

    // 将局部物理坐标映射到 `src_tex` 内的采样 UV（`src_rect` 保存离屏缓冲的物理范围）
    let local_norm = (lp - u.src_rect.xy) / max(u.src_rect.zw, vec2<f32>(1.0));
    var src_pm = vec4<f32>(0.0);
    if (local_norm.x >= 0.0 && local_norm.x <= 1.0 && local_norm.y >= 0.0 && local_norm.y <= 1.0) {
        let suv = (local_norm * u.src_rect.zw) * inv_src;
        src_pm = textureSampleLevel(src_tex, linear_sampler, suv, 0.0);
    }

    src_pm = apply_color_matrix_premul(src_pm);

    if (u.flags.y > 0.5) {
        let muv = (lp - u.body_rect.xy) / max(u.body_rect.zw, vec2<f32>(1.0));
        if (muv.x < 0.0 || muv.x > 1.0 || muv.y < 0.0 || muv.y > 1.0) {
            src_pm = vec4<f32>(0.0);
        } else {
            let m = textureSampleLevel(mask_tex, linear_sampler, muv, 0.0).r;
            src_pm = src_pm * m;
        }
    }

    src_pm = src_pm * u.params.x;
    if (hw_blend) {
        if (src_pm.a <= 0.00001) {
            discard;
        }
        return src_pm;
    }
    let duv = (p - u.tint.xy) * inv_dst;
    let dst_pm = textureSampleLevel(dst_tex, linear_sampler, duv, 0.0);
    let blend_mode = u32(u.params.y + 0.5);
    return blend_premul(src_pm, dst_pm, blend_mode);
}
