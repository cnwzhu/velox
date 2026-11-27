// 高密度 2D 交互画布专用 WGSL 着色器：
// 1. StaticMesh 管线：加载期耳切剖分的异形看台/船体/机身底图，顶点级 Viewport2D 变换；
// 2. CanvasInstanced 管线：32B 静态几何 (VertexBuffer) + 4B 动态状态 (StorageBuffer) + 32 色主题调色板，
//    支持 OBB 任意旋转圆角矩形 SDF、圆形 SDF、胶囊线段 SDF、悬停/选中/焦点边框与角标圆点。

struct CanvasUniform {
    // [viewport_w, viewport_h, origin_x, origin_y] (物理像素)
    screen_info: vec4<f32>,
    // [offset_x_phys, offset_y_phys, zoom_phys, scale_factor]
    camera: vec4<f32>,
    // [clip_x0, clip_y0, clip_x1, clip_y1] (物理像素)
    clip_rect: vec4<f32>,
    // [opacity, is_srgb, _pad, _pad]
    flags: vec4<f32>,
    // 32 槽位语义调色板（直连 CoreTheme，Dark/Light 切换零缓冲重建）
    palette: array<vec4<f32>, 32>,
};

@group(0) @binding(0)
var<uniform> u_canvas: CanvasUniform;

@group(0) @binding(1)
var<storage, read> item_states: array<u32>;

fn srgb_to_linear_ch(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn to_target_color(c: vec4<f32>) -> vec4<f32> {
    if (u_canvas.flags.y > 0.5) {
        return vec4<f32>(
            srgb_to_linear_ch(c.r),
            srgb_to_linear_ch(c.g),
            srgb_to_linear_ch(c.b),
            c.a,
        );
    }
    return c;
}

// ---------------- 1. 静态底图三角网格管线 ----------------

struct MeshVertIn {
    @location(0) pos: vec2<f32>,
    @location(1) color_u32: u32,
};

struct MeshVertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) screen_px: vec2<f32>,
};

@vertex
fn vs_mesh(v: MeshVertIn) -> MeshVertOut {
    let px = v.pos * u_canvas.camera.z + u_canvas.camera.xy;
    let ndc_x = (px.x / max(u_canvas.screen_info.x, 1.0)) * 2.0 - 1.0;
    let ndc_y = 1.0 - (px.y / max(u_canvas.screen_info.y, 1.0)) * 2.0;
    let a = f32((v.color_u32 >> 24u) & 255u) / 255.0;
    let r = f32((v.color_u32 >> 16u) & 255u) / 255.0;
    let g = f32((v.color_u32 >> 8u) & 255u) / 255.0;
    let b = f32(v.color_u32 & 255u) / 255.0;
    var out: MeshVertOut;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = vec4<f32>(r, g, b, a * u_canvas.flags.x);
    out.screen_px = px;
    return out;
}

@fragment
fn fs_mesh(in: MeshVertOut) -> @location(0) vec4<f32> {
    if (in.screen_px.x < u_canvas.clip_rect.x || in.screen_px.x > u_canvas.clip_rect.z ||
        in.screen_px.y < u_canvas.clip_rect.y || in.screen_px.y > u_canvas.clip_rect.w) {
        discard;
    }
    let c = to_target_color(in.color);
    return vec4<f32>(c.rgb * c.a, c.a);
}

// ---------------- 2. 32B 静态几何 + 4B 动态状态 OBB/圆/胶囊 SDF 管线 ----------------

struct InstIn {
    @location(0) rect_wh: vec4<f32>,   // [cx, cy, w, h]
    @location(1) rot_rk: vec4<f32>,    // [cos, sin, corner_r, bitcast<f32>(kind_flags)]
};

struct InstOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) local_px: vec2<f32>,  // 旋转对齐后的图元局部像素坐标
    @location(1) half_size: vec2<f32>, // [hw, hh]（胶囊时 x=half_seg, y=radius）
    @location(2) fill_col: vec4<f32>,
    @location(3) border_col: vec4<f32>,
    @location(4) params: vec4<f32>,    // [corner_r, border_px, kind, badge_flag]
    @location(5) screen_px: vec2<f32>,
};

@vertex
fn vs_inst(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) iid: u32,
    inst: InstIn,
) -> InstOut {
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    let corner = quad[vid];

    let zoom = u_canvas.camera.z;
    let sc = inst.rect_wh.xy * zoom + u_canvas.camera.xy;
    let sw = inst.rect_wh.z * zoom;
    let sh = inst.rect_wh.w * zoom;
    let cs = inst.rot_rk.x;
    let sn = inst.rot_rk.y;
    let kind_u = bitcast<u32>(inst.rot_rk.w);
    let is_capsule = (kind_u == 2u);

    let hw = select(sw * 0.5, (sw + sh) * 0.5, is_capsule);
    let hh = sh * 0.5;
    let pad = 2.0;
    let local = vec2<f32>(corner.x * (hw + pad), corner.y * (hh + pad));
    // 正旋转 local -> 屏幕偏移
    let world_off = vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let px = sc + world_off;

    let ndc_x = (px.x / max(u_canvas.screen_info.x, 1.0)) * 2.0 - 1.0;
    let ndc_y = 1.0 - (px.y / max(u_canvas.screen_info.y, 1.0)) * 2.0;

    // 解包 4B 动态状态
    let st_len = arrayLength(&item_states);
    let packed = select(0u, item_states[min(iid, st_len - 1u)], st_len > 0u);
    let pal_idx = packed & 31u;
    let st_flags = (packed >> 8u) & 255u;
    let badge_mask = (packed >> 16u) & 255u;

    let is_hovered = (st_flags & 1u) != 0u;
    let is_selected = (st_flags & 2u) != 0u;
    let is_disabled = (st_flags & 4u) != 0u;
    let is_locked = (st_flags & 8u) != 0u;
    let is_focused = (st_flags & 16u) != 0u;

    var fill = u_canvas.palette[pal_idx];
    var border = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (is_disabled) {
        fill.a = fill.a * 0.35;
    } else {
        if (is_selected) {
            fill = vec4<f32>(mix(fill.rgb, vec3<f32>(1.0), 0.18), fill.a);
            border = vec4<f32>(1.0, 1.0, 1.0, 1.0);
        } else if (is_hovered) {
            fill = vec4<f32>(mix(fill.rgb, vec3<f32>(1.0), 0.12), fill.a);
            border = vec4<f32>(mix(fill.rgb, vec3<f32>(1.0), 0.55), 1.0);
        }
        if (is_focused) {
            border = vec4<f32>(0.376, 0.647, 0.980, 1.0);
        }
    }

    let cr = clamp(inst.rot_rk.z * zoom, 0.0, min(hw, hh));
    let bpx = select(0.0, clamp(1.5 * u_canvas.camera.w, 1.0, 3.0), border.a > 0.01);
    let badge_f = select(0.0, 1.0, (badge_mask != 0u || is_locked) && !is_disabled);

    var out: InstOut;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.local_px = local;
    out.half_size = select(vec2<f32>(hw, hh), vec2<f32>(sw * 0.5, hh), is_capsule);
    out.fill_col = vec4<f32>(fill.rgb, fill.a * u_canvas.flags.x);
    out.border_col = vec4<f32>(border.rgb, border.a * u_canvas.flags.x);
    out.params = vec4<f32>(cr, bpx, f32(kind_u), badge_f);
    out.screen_px = px;
    return out;
}

@fragment
fn fs_inst(in: InstOut) -> @location(0) vec4<f32> {
    if (in.screen_px.x < u_canvas.clip_rect.x || in.screen_px.x > u_canvas.clip_rect.z ||
        in.screen_px.y < u_canvas.clip_rect.y || in.screen_px.y > u_canvas.clip_rect.w) {
        discard;
    }

    let kind = u32(in.params.z + 0.5);
    var dist: f32;
    if (kind == 1u) {
        dist = length(in.local_px) - min(in.half_size.x, in.half_size.y);
    } else if (kind == 2u) {
        let qx = max(abs(in.local_px.x) - in.half_size.x, 0.0);
        dist = length(vec2<f32>(qx, in.local_px.y)) - in.half_size.y;
    } else {
        let r = in.params.x;
        let q = abs(in.local_px) - (in.half_size - vec2<f32>(r, r));
        dist = length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
    }

    let cov = clamp(0.75 - dist, 0.0, 1.0);
    if (cov <= 0.001) {
        discard;
    }

    var col = in.fill_col;
    if (in.params.y > 0.0 && dist > -in.params.y) {
        let bt = clamp((dist + in.params.y), 0.0, 1.0);
        col = mix(in.fill_col, in.border_col, bt);
    }

    // 右上角状态/VIP角标指示圆点
    if (in.params.w > 0.5 && in.half_size.x >= 6.0 && in.half_size.y >= 6.0) {
        let bpos = vec2<f32>(in.half_size.x - 3.5, -(in.half_size.y - 3.5));
        let bd = length(in.local_px - bpos) - 2.1;
        let bc = clamp(0.75 - bd, 0.0, 1.0);
        if (bc > 0.01) {
            col = mix(col, vec4<f32>(0.980, 0.800, 0.082, col.a), bc);
        }
    }

    let out_c = to_target_color(vec4<f32>(col.rgb, col.a * cov));
    return vec4<f32>(out_c.rgb * out_c.a, out_c.a);
}
