//! Pass 4（自顶向下纵向坐标分配与扁平化）：在单次深度优先遍历中生成 `DrawOp` 与 `HitRegion` 序列。

use crate::arena::{ScratchArena, SmallVec};
use crate::element::{DivNode, Element, SpriteSource, TransitionSpec};
use crate::motion::{DrawOp, DrawPayload, MotionCache, MotionState};
use crate::paint::{compute_flip_transform, BlendMode, CornerRadii, LayerSpec, Paint, StyledQuad};
use crate::style::{AlignItems, Color, FlexDir, Justify, Length};

use super::types::{
    child_is_overlay, child_margin, child_pos, clamp_h, div_padding, HitRegion, InteractionState,
    LayoutNode, Region, SUBVIEW_FLAG_OWNER, SUBVIEW_FLAG_ROOT,
};

/// Pass 4 遍历期间不变/共享的可变上下文（打包为单指针传入递归，大幅压缩每层栈帧）。
pub(super) struct Pass4Ctx<'a, 'm, const MAX_OPS: usize, const MAX_HITS: usize> {
    pub arena: &'a ScratchArena,
    pub viewport: Region,
    pub interaction: InteractionState,
    pub overlay_pass: bool,
    pub has_overlays: bool,
    pub focus_counter: u16,
    pub dropped_ops: u32,
    pub motion: &'m mut MotionCache,
    pub out_ops: &'m mut SmallVec<DrawOp<'a>, MAX_OPS>,
    pub out_hits: &'m mut SmallVec<HitRegion<'a>, MAX_HITS>,
}

/// Pass 4 逐层下传的紧凑继承状态（仅约 32 Bytes）。
#[derive(Copy, Clone)]
pub(super) struct Pass4Inherit {
    pub owner_pod: u16,
    pub path_hash: u32,
    pub parent_origin: (i16, i16),
    pub parent_clip: Region,
    pub parent_clip_radius: u8,
    pub opacity: u8,
    pub offset: (i16, i16),
    pub text_size: Option<u8>,
    pub text_color: Option<Color>,
    pub in_overlay: bool,
}

#[inline(never)]
fn merge_div_transition(
    own_spec: Option<TransitionSpec>,
    inherited_transition: Option<&(u32, TransitionSpec)>,
) -> Option<TransitionSpec> {
    let auto_spec_from_parent = inherited_transition.and_then(|(_, ps)| {
        if ps.auto_animate || ps.stagger_ms > 0 {
            Some(*ps)
        } else {
            None
        }
    });
    match (own_spec, auto_spec_from_parent) {
        (Some(mut own), Some(parent_auto)) => {
            if parent_auto.delay_ms > 0 {
                own.delay_ms = own.delay_ms.saturating_add(parent_auto.delay_ms);
            }
            if parent_auto.auto_animate {
                if !own.animate_bounds {
                    own.animate_bounds = true;
                }
                if own.enter.is_none() {
                    own.enter = parent_auto.enter;
                }
                if own.exit.is_none() {
                    own.exit = parent_auto.exit;
                }
            }
            Some(own)
        }
        (Some(own), None) => Some(own),
        (None, Some(parent_auto)) if parent_auto.auto_animate => Some(TransitionSpec {
            auto_animate: false,
            stagger_ms: 0,
            ..parent_auto
        }),
        (None, _) => None,
    }
}

#[inline(never)]
fn compute_base_child_transition(
    effective_id: u32,
    own_spec: Option<TransitionSpec>,
    inherited_transition: Option<&(u32, TransitionSpec)>,
) -> Option<(u32, TransitionSpec)> {
    match own_spec {
        Some(spec) => Some((effective_id, spec)),
        None => inherited_transition.and_then(|&(pid, mut s)| {
            s.auto_animate = false;
            s.stagger_ms = 0;
            if s.animate_color {
                Some((pid, s))
            } else {
                None
            }
        }),
    }
}

#[inline(never)]
fn compute_child_transition(
    base_transition: Option<&(u32, TransitionSpec)>,
    slot_key: u32,
    child_idx: u16,
) -> Option<(u32, TransitionSpec)> {
    base_transition.map(|&(pid, mut spec)| {
        if spec.auto_animate {
            spec.animate_bounds = true;
            spec.animate_opacity = true;
            if spec.enter.is_none() {
                spec.enter = Some(crate::element::EnterFrom {
                    opacity: 0,
                    dx: 0,
                    dy: 8,
                });
            }
            if spec.exit.is_none() {
                spec.exit = Some(crate::element::ExitTo {
                    opacity: 0,
                    dx: 0,
                    dy: -8,
                });
            }
        }
        if spec.stagger_ms > 0 {
            spec.delay_ms = spec
                .delay_ms
                .saturating_add(child_idx.saturating_mul(spec.stagger_ms));
        }
        (
            (pid ^ 0xA5A5_5A5A)
                .wrapping_mul(0x0100_0193)
                .wrapping_add(slot_key),
            spec,
        )
    })
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn try_push_div_layer<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    ctx: &mut Pass4Ctx<'a, '_, MAX_OPS, MAX_HITS>,
    effects: Option<&'a crate::paint::PaintEffects<'a>>,
    flip_info: Option<(crate::paint::Transform2D, f32, f32)>,
    has_children: bool,
    effective_id: u32,
    bounds: Region,
    rounded: u8,
    my_opacity: u8,
    eff_parent_clip: Region,
    eff_parent_clip_radius: u8,
) -> Option<Region> {
    let flip_needs_layer = flip_info.is_some() && has_children;
    let spec = if let Some(eff) = effects {
        if !(eff.needs_layer(has_children, my_opacity) || flip_needs_layer) {
            return None;
        }
        let base_radii = eff.corner_radii.unwrap_or(CornerRadii::all(rounded));
        let clip_radii = if let Some((_, sx, sy)) = flip_info {
            base_radii.inverse_scaled(sx, sy)
        } else {
            base_radii
        };
        let combined_xform = match (eff.transform, flip_info.map(|(t, _, _)| t)) {
            (Some(a), Some(b)) => Some(b.concat(a)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        LayerSpec {
            bounds,
            clip_radii,
            clip_path: eff.clip_path,
            opacity: my_opacity,
            blend_mode: eff.blend_mode,
            filter: eff.filter,
            backdrop_filter: eff.backdrop_filter,
            transform: combined_xform,
        }
    } else if let Some((flip_xform, sx, sy)) = flip_info {
        if !has_children {
            return None;
        }
        LayerSpec {
            bounds,
            clip_radii: CornerRadii::all(rounded).inverse_scaled(sx, sy),
            clip_path: None,
            opacity: my_opacity,
            blend_mode: BlendMode::SrcOver,
            filter: crate::paint::LayerFilter::default(),
            backdrop_filter: crate::paint::BackdropFilter::default(),
            transform: Some(flip_xform),
        }
    } else {
        return None;
    };
    let exp_bounds = spec.expanded_bounds();
    if let Some(spec_ref) = ctx.arena.try_alloc(spec) {
        if ctx.out_ops.push(DrawOp {
            id: Some(effective_id ^ 0x6C00_0000),
            bounds: exp_bounds,
            clip: eff_parent_clip,
            clip_radius: eff_parent_clip_radius,
            payload: DrawPayload::PushLayer(spec_ref),
        }) {
            return Some(exp_bounds);
        }
        ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
    }
    None
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn try_push_div_styled_quad<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    ctx: &mut Pass4Ctx<'a, '_, MAX_OPS, MAX_HITS>,
    div: &DivNode<'a>,
    effects: Option<&'a crate::paint::PaintEffects<'a>>,
    flip_info: Option<(crate::paint::Transform2D, f32, f32)>,
    layer_pushed: bool,
    has_transition: bool,
    effective_id: u32,
    bounds: Region,
    eff_parent_clip: Region,
    eff_parent_clip_radius: u8,
    raw_bg: Color,
    border_color: Color,
    border_width: u8,
    rounded: u8,
    inner_opacity: u8,
) {
    let paint = effects.and_then(|e| e.bg_paint).unwrap_or(Paint::Solid(raw_bg));
    let base_radii = effects
        .and_then(|e| e.corner_radii)
        .unwrap_or(CornerRadii::all(rounded));
    let radii = if !layer_pushed {
        if let Some((_, sx, sy)) = flip_info {
            base_radii.inverse_scaled(sx, sy)
        } else {
            base_radii
        }
    } else {
        base_radii
    };
    let eff_xform = if layer_pushed {
        None
    } else {
        match (
            effects.and_then(|e| e.transform),
            flip_info.map(|(t, _, _)| t),
        ) {
            (Some(a), Some(b)) => Some(b.concat(a)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    };
    let has_fill = !paint.is_fully_transparent() && inner_opacity > 0;
    let has_border = border_width > 0 && border_color.a() > 0;
    if !(has_fill || has_border) {
        return;
    }
    let sq = StyledQuad {
        paint,
        border_color,
        border_width: border_width & 0x7F,
        radii,
        opacity: inner_opacity,
        blend_mode: if layer_pushed {
            BlendMode::SrcOver
        } else {
            effects.map(|e| e.blend_mode).unwrap_or(BlendMode::SrcOver)
        },
        transform: eff_xform,
    };
    if let Some(sq_ref) = ctx.arena.try_alloc(sq) {
        if !ctx.out_ops.push(DrawOp {
            id: div.id().or(if has_transition {
                Some(effective_id)
            } else {
                None
            }),
            bounds,
            clip: eff_parent_clip,
            clip_radius: eff_parent_clip_radius,
            payload: DrawPayload::StyledQuad(sq_ref),
        }) {
            ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
        }
    } else {
        ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
    }
}

/// 递归查找退场节点内部的首个非空标题文本（最多深入 3 层，适配 `Dialog`/`Modal`/`Sheet`/`Notification`/`Popover`）。
fn find_first_ghost_text<'a>(
    node: &LayoutNode<'a, '_>,
    root_x: i16,
    root_y: i16,
    fallback_pad: (i16, i16),
    inherited_color: Option<Color>,
    inherited_size: Option<u8>,
    depth: u8,
) -> Option<(&'a str, Color, u8, i16, i16)> {
    for ch in node.children.iter() {
        match ch.element {
            Element::Text(t) if !t.content.as_str().is_empty() => {
                let col = if t.color.a() > 0 {
                    t.color
                } else {
                    inherited_color.unwrap_or(Color::WHITE)
                };
                let fs = if t.font_size > 0 {
                    t.font_size
                } else {
                    inherited_size.unwrap_or(14)
                };
                let dx = if depth == 0 && ch.x >= root_x {
                    ch.x.saturating_sub(root_x)
                } else {
                    fallback_pad.0.max(10)
                };
                let dy = if depth == 0 && ch.y >= root_y {
                    ch.y.saturating_sub(root_y)
                } else {
                    fallback_pad.1.max(10)
                };
                return Some((t.content.as_str(), col, fs, dx, dy));
            }
            Element::Div(d) if depth < 2 => {
                let next_col = d.text_color().or(inherited_color);
                let next_sz = d.text_size().or(inherited_size);
                if let Some(found) = find_first_ghost_text(
                    ch,
                    root_x,
                    root_y,
                    fallback_pad,
                    next_col,
                    next_sz,
                    depth + 1,
                ) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn try_record_div_ghost<'a>(
    motion: &mut MotionCache,
    node: &LayoutNode<'a, '_>,
    div: &DivNode<'a>,
    spec: TransitionSpec,
    effective_id: u32,
    path_hash: u32,
    bounds: Region,
    eff_parent_clip: Region,
    eff_parent_clip_radius: u8,
    raw_bg: Color,
    border_color: Color,
    rounded: u8,
    encoded_bw: u8,
    my_opacity: u8,
    child_text_color: Option<Color>,
    child_text_size: Option<u8>,
) {
    let Some(exit_to) = spec.exit else {
        return;
    };
    let pad = div_padding(div);
    let first_label = find_first_ghost_text(
        node,
        node.x,
        node.y,
        (pad.left, pad.top),
        child_text_color,
        child_text_size,
        0,
    );
    let exit_dur = if spec.exit_duration_ms > 0 {
        spec.exit_duration_ms
    } else {
        spec.duration_ms.max(1)
    };
    motion.record_ghost_snapshot(
        effective_id,
        path_hash,
        bounds,
        eff_parent_clip,
        eff_parent_clip_radius,
        raw_bg,
        div.bg_grad().map(|(c, _)| c).unwrap_or(raw_bg),
        border_color,
        rounded,
        encoded_bw,
        my_opacity,
        exit_to,
        exit_dur,
        spec.easing,
        first_label,
    );
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn try_push_div_external_texture<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    ctx: &mut Pass4Ctx<'a, '_, MAX_OPS, MAX_HITS>,
    div: &DivNode<'a>,
    effects: Option<&'a crate::paint::PaintEffects<'a>>,
    layer_pushed: bool,
    effective_id: u32,
    bounds: Region,
    eff_parent_clip: Region,
    eff_parent_clip_radius: u8,
    rounded: u8,
    inner_opacity: u8,
) {
    let Some(mut ext) = effects.and_then(|e| e.external_texture).copied() else {
        return;
    };
    if ext.radii.is_zero() {
        ext.radii = effects
            .and_then(|e| e.corner_radii)
            .unwrap_or(CornerRadii::all(rounded));
    }
    ext.opacity = ((ext.opacity as u16 * inner_opacity as u16) / 255) as u8;
    if layer_pushed {
        ext.blend_mode = BlendMode::SrcOver;
    } else if ext.blend_mode.is_normal() {
        if let Some(e) = effects {
            ext.blend_mode = e.blend_mode;
        }
    }
    if ext.opacity > 0 {
        if let Some(ext_ref) = ctx.arena.try_alloc(ext) {
            if !ctx.out_ops.push(DrawOp {
                id: div.id().or(if div.transition.is_some() {
                    Some(effective_id)
                } else {
                    None
                }),
                bounds,
                clip: eff_parent_clip,
                clip_radius: eff_parent_clip_radius,
                payload: DrawPayload::ExternalTexture(ext_ref),
            }) {
                ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
            }
        } else {
            ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
        }
    }
}

/// Pass 4（自顶向下）：求解所有节点的最终 `y` 与 `h`，并在同一次遍历中将层级树拍平为一维连续的 `DrawOp` 数组。
pub(super) fn pass4_vertical_assign_and_flatten<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    ctx: &mut Pass4Ctx<'a, '_, MAX_OPS, MAX_HITS>,
    node: &mut LayoutNode<'a, '_>,
    inh: Pass4Inherit,
    inherited_transition: Option<&(u32, TransitionSpec)>,
) {
    let cur_owner_pod = if (node.subview_flags & (SUBVIEW_FLAG_ROOT | SUBVIEW_FLAG_OWNER)) != 0 {
        node.subview_pod
    } else {
        inh.owner_pod
    };
    match node.element {
        Element::Div(div) => {
            let effective_id = div.id().unwrap_or(inh.path_hash | 0x8000_0000);
            if !ctx.overlay_pass || inh.in_overlay {
                layout_div_children_vertical(node, div);
            }
            let resolved_scroll_max = resolve_div_scroll_max(node, div);
            if div.is_overlay() && !inh.in_overlay {
                ctx.has_overlays = true;
                if !ctx.overlay_pass {
                    return;
                }
            }
            let now_in_overlay = inh.in_overlay || div.is_overlay();
            let emit_here =
                (!ctx.overlay_pass && !now_in_overlay) || (ctx.overlay_pass && now_in_overlay);

            let layout_bounds = Region::new(node.x, node.y, node.w, node.h);
            let mut bounds = translate_region(layout_bounds, inh.offset);
            let (eff_parent_clip, eff_parent_clip_radius) = if div.is_overlay() && !inh.in_overlay {
                (ctx.viewport, 0)
            } else {
                (inh.parent_clip, inh.parent_clip_radius)
            };
            let hit_box = bounds.intersect(eff_parent_clip);

            let my_focus_idx = if emit_here && div.focusable() {
                let idx = ctx.focus_counter;
                ctx.focus_counter = ctx.focus_counter.saturating_add(1);
                Some(idx)
            } else {
                None
            };
            let is_focused =
                my_focus_idx.is_some() && my_focus_idx == ctx.interaction.focused_index;

            let is_hovered = ctx
                .interaction
                .pointer_pos
                .is_some_and(|(px, py)| hit_box.contains(px, py));
            let is_pressed_here = is_hovered && ctx.interaction.is_pressed;

            let mut bg = div.style.bg;
            let mut border_width = div.style.border_px();
            let mut border_color = div.style.border_color;
            let mut rounded = div.style.rounded_radius();
            let mut base_opacity = div.opacity;
            let mut child_text_color = div.text_color().or(inh.text_color);
            let child_text_size = div.text_size().or(inh.text_size);

            if is_hovered {
                if let Some(hov_bg) = div.hover_bg() {
                    bg = hov_bg;
                }
                if let Some(r) = div.hover_style() {
                    if let Some(c) = r.bg {
                        bg = c;
                    }
                    if let Some(bw) = r.border_width {
                        border_width = bw;
                    }
                    if let Some(bc) = r.border_color {
                        border_color = bc;
                    }
                    if let Some(tc) = r.text_color {
                        child_text_color = Some(tc);
                    }
                    if let Some(op) = r.opacity {
                        base_opacity = op;
                    }
                    if let Some(rd) = r.rounded {
                        rounded = rd;
                    }
                }
            }

            if is_pressed_here {
                if let Some(act_bg) = div.active_bg() {
                    bg = act_bg;
                }
                if let Some(r) = div.active_style() {
                    if let Some(c) = r.bg {
                        bg = c;
                    }
                    if let Some(bw) = r.border_width {
                        border_width = bw;
                    }
                    if let Some(bc) = r.border_color {
                        border_color = bc;
                    }
                    if let Some(tc) = r.text_color {
                        child_text_color = Some(tc);
                    }
                    if let Some(op) = r.opacity {
                        base_opacity = op;
                    }
                    if let Some(rd) = r.rounded {
                        rounded = rd;
                    }
                }
            }

            let show_focus_border =
                is_focused && (ctx.interaction.focus_visible || div.on_key_input().is_some());
            if show_focus_border {
                let (fw, fc) = div.focus_border().unwrap_or((
                    1,
                    if ctx.interaction.focus_ring_color.a() > 0 {
                        ctx.interaction.focus_ring_color
                    } else {
                        Color::rgb(91, 139, 217)
                    },
                ));
                if fw > 0 && fc.a() > 0 {
                    border_width = fw;
                    border_color = fc;
                }
            }
            if is_focused {
                if let Some(r) = div.focus_style() {
                    if let Some(c) = r.bg {
                        bg = c;
                    }
                    if let Some(bw) = r.border_width {
                        border_width = bw;
                    }
                    if let Some(bc) = r.border_color {
                        border_color = bc;
                    }
                    if let Some(tc) = r.text_color {
                        child_text_color = Some(tc);
                    }
                    if let Some(op) = r.opacity {
                        base_opacity = op;
                    }
                    if let Some(rd) = r.rounded {
                        rounded = rd;
                    }
                }
            }

            let own_spec = div.transition_spec();
            let eff_transition = if own_spec.is_some() || inherited_transition.is_some() {
                merge_div_transition(own_spec, inherited_transition)
            } else {
                None
            };

            let mut opacity = base_opacity;
            let mut child_offset = inh.offset;
            let mut flip_info: Option<(crate::paint::Transform2D, f32, f32)> = None;
            if div.id().is_some() || eff_transition.is_some() {
                if emit_here {
                    let st = ctx.motion.resolve_scoped_with_ancestor_offset(
                        effective_id,
                        inh.path_hash,
                        inh.parent_origin,
                        inh.offset,
                        MotionState {
                            bounds: layout_bounds,
                            bg,
                            border: border_color,
                            opacity: base_opacity,
                        },
                        eff_transition,
                    );
                    if eff_transition.is_some_and(|s| s.flip_transform) && st.bounds != layout_bounds
                    {
                        flip_info = compute_flip_transform(layout_bounds, st.bounds);
                        bounds = translate_region(layout_bounds, inh.offset);
                    } else {
                        child_offset = (
                            inh.offset
                                .0
                                .saturating_add(st.bounds.x.saturating_sub(layout_bounds.x)),
                            inh.offset
                                .1
                                .saturating_add(st.bounds.y.saturating_sub(layout_bounds.y)),
                        );
                        bounds = translate_region(st.bounds, inh.offset);
                    }
                    bg = st.bg;
                    if !show_focus_border {
                        border_color = st.border;
                    }
                    opacity = st.opacity;
                } else if let Some((dx, dy, op)) =
                    ctx.motion.peek_offset_opacity_scoped(effective_id, inh.path_hash)
                {
                    child_offset = (
                        inh.offset.0.saturating_add(dx),
                        inh.offset.1.saturating_add(dy),
                    );
                    bounds = translate_region(bounds, (dx, dy));
                    opacity = op;
                }
            }
            if let Some(st) = div.layout_ext().and_then(|l| l.sticky_top) {
                let min_y = eff_parent_clip.y.saturating_add(st);
                let max_y = eff_parent_clip.bottom().saturating_sub(bounds.h);
                if bounds.y < min_y && max_y >= min_y {
                    let clamped_y = min_y.min(max_y);
                    let dy = clamped_y.saturating_sub(bounds.y);
                    bounds.y = clamped_y;
                    child_offset.1 = child_offset.1.saturating_add(dy);
                }
            }
            if let Some(max_s) = resolved_scroll_max {
                if emit_here {
                    ctx.motion.sync_scroll_bounds(effective_id, max_s);
                }
                let s = ctx.motion.scroll_offset(effective_id);
                if div.style.flex_dir() == FlexDir::Row
                    && div.grid_cols == 0
                    && !div.style.flex_wrap()
                {
                    child_offset.0 = child_offset.0.saturating_sub(s);
                } else {
                    child_offset.1 = child_offset.1.saturating_sub(s);
                }
            }
            let my_opacity = ((inh.opacity as u16 * opacity as u16) / 255) as u8;
            if emit_here && div.is_ime_caret() {
                let clipped = bounds.intersect(eff_parent_clip);
                let eff_caret = if !clipped.is_empty() { clipped } else { bounds };
                ctx.motion.set_ime_caret_bounds(Some(eff_caret));
            }
            let effects = div.effects();
            let has_children = !node.children.is_empty();
            let layer_pushed = if emit_here && (effects.is_some() || flip_info.is_some()) {
                try_push_div_layer(
                    ctx,
                    effects,
                    flip_info,
                    has_children,
                    effective_id,
                    bounds,
                    rounded,
                    my_opacity,
                    eff_parent_clip,
                    eff_parent_clip_radius,
                )
            } else {
                None
            };

            let inner_opacity = if layer_pushed.is_some() { 255 } else { my_opacity };

            let raw_bg = bg;
            bg = bg.scale_alpha(inner_opacity);
            border_color = border_color.scale_alpha(inner_opacity);
            let (bg_grad, grad_horiz) = match div.bg_grad() {
                Some((to_col, horiz)) => (to_col.scale_alpha(inner_opacity), horiz),
                None => (bg, false),
            };
            let encoded_bw = (border_width & 0x7F) | (if grad_horiz { 0x80 } else { 0 });

            let (my_clip, my_clip_radius) = if div.clip_children() {
                let intersected = eff_parent_clip.intersect(bounds);
                let my_r = rounded;
                let eff_r = if my_r > 0 && intersected == bounds {
                    my_r
                } else if my_r > 0 {
                    my_r.min(eff_parent_clip_radius.max(my_r))
                } else if intersected == eff_parent_clip {
                    eff_parent_clip_radius
                } else {
                    0
                };
                (intersected, eff_r)
            } else {
                (eff_parent_clip, eff_parent_clip_radius)
            };

            if emit_here {
                if let Some((blur, ox, oy, scol)) = div.shadow() {
                    let b_i16 = blur as i16;
                    let sh_bounds = Region::new(
                        bounds.x.saturating_add(ox as i16).saturating_sub(b_i16),
                        bounds.y.saturating_add(oy as i16).saturating_sub(b_i16),
                        bounds.w.saturating_add(b_i16.saturating_mul(2)),
                        bounds.h.saturating_add(b_i16.saturating_mul(2)),
                    );
                    let sh_col = scol.scale_alpha(inner_opacity);
                    if sh_col.a() > 0
                        && !ctx.out_ops.push(DrawOp {
                            id: Some(effective_id ^ 0x5A00_0000),
                            bounds: sh_bounds,
                            clip: eff_parent_clip,
                            clip_radius: eff_parent_clip_radius,
                            payload: DrawPayload::Quad {
                                bg: sh_col,
                                bg_grad: sh_col,
                                border_color: Color::TRANSPARENT,
                                rounded,
                                border_width: blur & 0x7F,
                            },
                        })
                    {
                        ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                    }
                }
            }

            let use_styled_quad = effects.is_some_and(|eff| {
                eff.bg_paint.is_some()
                    || eff.corner_radii.is_some()
                    || (layer_pushed.is_none()
                        && (eff.transform.is_some() || !eff.blend_mode.is_normal()))
            }) || (layer_pushed.is_none() && flip_info.is_some());

            if emit_here && use_styled_quad {
                try_push_div_styled_quad(
                    ctx,
                    div,
                    effects,
                    flip_info,
                    layer_pushed.is_some(),
                    eff_transition.is_some(),
                    effective_id,
                    bounds,
                    eff_parent_clip,
                    eff_parent_clip_radius,
                    raw_bg,
                    border_color,
                    border_width,
                    rounded,
                    inner_opacity,
                );
            } else if emit_here
                && (bg.a() > 0 || bg_grad.a() > 0 || (border_width > 0 && border_color.a() > 0))
            {
                if !ctx.out_ops.push(DrawOp {
                    id: div.id().or(if eff_transition.is_some() {
                        Some(effective_id)
                    } else {
                        None
                    }),
                    bounds,
                    clip: eff_parent_clip,
                    clip_radius: eff_parent_clip_radius,
                    payload: DrawPayload::Quad {
                        bg,
                        bg_grad,
                        border_color,
                        rounded,
                        border_width: encoded_bw,
                    },
                }) {
                    ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                }
            }

            if emit_here {
                if let Some(spec) = eff_transition {
                    if spec.exit.is_some() {
                        try_record_div_ghost(
                            ctx.motion,
                            node,
                            div,
                            spec,
                            effective_id,
                            inh.path_hash,
                            bounds,
                            eff_parent_clip,
                            eff_parent_clip_radius,
                            raw_bg,
                            border_color,
                            rounded,
                            encoded_bw,
                            my_opacity,
                            child_text_color,
                            child_text_size,
                        );
                    }
                }
            }

            if emit_here && effects.is_some_and(|e| e.external_texture.is_some()) {
                try_push_div_external_texture(
                    ctx,
                    div,
                    effects,
                    layer_pushed.is_some(),
                    effective_id,
                    bounds,
                    eff_parent_clip,
                    eff_parent_clip_radius,
                    rounded,
                    inner_opacity,
                );
            }

            let scroll_target = resolved_scroll_max.map(|max_s| (effective_id, max_s));
            let scroll_horizontal = div.style.flex_dir() == FlexDir::Row
                && div.grid_cols == 0
                && !div.style.flex_wrap();
            let has_hover_effect = div.hover_bg().is_some() || div.hover_style().is_some();
            let has_active_effect = div.active_bg().is_some() || div.active_style().is_some();

            if emit_here
                && (div.interact.is_some()
                    || div.occlude_hit()
                    || my_focus_idx.is_some()
                    || scroll_target.is_some()
                    || has_hover_effect
                    || has_active_effect
                    || div.cursor.is_some())
            {
                let _ = ctx.out_hits.push(HitRegion {
                    id: Some(effective_id),
                    bounds,
                    clip: eff_parent_clip,
                    focus_index: my_focus_idx,
                    owner_pod: cur_owner_pod,
                    scroll_target,
                    interact: div.interact,
                    occlude: div.occlude_hit(),
                    auto_repeat: div.auto_repeat(),
                    has_hover_bg: has_hover_effect,
                    has_active_bg: has_active_effect,
                    scroll_horizontal,
                    cursor: div.cursor,
                });
            }

            let base_transition = if own_spec.is_some() || inherited_transition.is_some() {
                compute_base_child_transition(effective_id, own_spec, inherited_transition)
            } else {
                None
            };

            let has_custom_z = node
                .children
                .iter()
                .any(|c| matches!(c.element, Element::Div(d) if d.z_index != 0));

            let child_stable_slot = |elem: &Element<'_>, unkeyed_ord: u32| -> u32 {
                match elem {
                    Element::Div(d) => {
                        if let Some(cid) = d.id() {
                            cid ^ 0x9E37_79B9
                        } else {
                            unkeyed_ord
                        }
                    }
                    Element::SubView { id, .. } => 0x4000_0000 | (id.0 as u32),
                    Element::Text(_) => 0x1000_0000 | unkeyed_ord,
                    Element::Img(_) => 0x2000_0000 | unkeyed_ord,
                    Element::RichText(_) => 0x3000_0000 | unkeyed_ord,
                    Element::DecodedImage(_) => 0x5000_0000 | unkeyed_ord,
                    Element::Vector(_) => 0x6000_0000 | unkeyed_ord,
                    Element::SceneCanvas(s) => s.id,
                }
            };

            let base_child_inh = Pass4Inherit {
                owner_pod: cur_owner_pod,
                path_hash: 0,
                parent_origin: (node.x, node.y),
                parent_clip: my_clip,
                parent_clip_radius: my_clip_radius,
                opacity: inner_opacity,
                offset: child_offset,
                text_size: child_text_size,
                text_color: child_text_color,
                in_overlay: now_in_overlay,
            };

            if !has_custom_z {
                let mut unkeyed_ord = 1u32;
                for (child_idx, child) in node.children.iter_mut().enumerate() {
                    let is_keyed = matches!(
                        child.element,
                        Element::Div(d) if d.id().is_some()
                    ) || matches!(child.element, Element::SubView { .. });
                    let slot_key = child_stable_slot(child.element, unkeyed_ord);
                    if !is_keyed {
                        unkeyed_ord = unkeyed_ord.wrapping_add(1);
                    }
                    let child_hash = effective_id
                        .wrapping_mul(0x0100_0193)
                        .wrapping_add(slot_key);
                    let child_transition = if base_transition.is_some() {
                        compute_child_transition(base_transition.as_ref(), slot_key, child_idx as u16)
                    } else {
                        None
                    };
                    let mut c_inh = base_child_inh;
                    c_inh.path_hash = child_hash;
                    pass4_vertical_assign_and_flatten(
                        ctx,
                        child,
                        c_inh,
                        child_transition.as_ref(),
                    );
                }
            } else {
                let mut cur_z = node
                    .children
                    .iter()
                    .map(|c| match c.element {
                        Element::Div(d) => d.z_index,
                        _ => 0,
                    })
                    .min()
                    .unwrap_or(0);
                loop {
                    let mut unkeyed_ord = 1u32;
                    for (child_idx, child) in node.children.iter_mut().enumerate() {
                        let is_keyed = matches!(
                            child.element,
                            Element::Div(d) if d.id().is_some()
                        ) || matches!(child.element, Element::SubView { .. });
                        let slot_key = child_stable_slot(child.element, unkeyed_ord);
                        if !is_keyed {
                            unkeyed_ord = unkeyed_ord.wrapping_add(1);
                        }
                        let cz = match child.element {
                            Element::Div(d) => d.z_index,
                            _ => 0,
                        };
                        if cz == cur_z {
                            let child_hash = effective_id
                                .wrapping_mul(0x0100_0193)
                                .wrapping_add(slot_key);
                            let child_transition = if base_transition.is_some() {
                                compute_child_transition(
                                    base_transition.as_ref(),
                                    slot_key,
                                    child_idx as u16,
                                )
                            } else {
                                None
                            };
                            let mut c_inh = base_child_inh;
                            c_inh.path_hash = child_hash;
                            pass4_vertical_assign_and_flatten(
                                ctx,
                                child,
                                c_inh,
                                child_transition.as_ref(),
                            );
                        }
                    }
                    let next_z = node
                        .children
                        .iter()
                        .map(|c| match c.element {
                            Element::Div(d) => d.z_index,
                            _ => 0,
                        })
                        .filter(|&z| z > cur_z)
                        .min();
                    match next_z {
                        Some(nz) => cur_z = nz,
                        None => break,
                    }
                }
            }

            if let Some(exp_bounds) = layer_pushed {
                if !ctx.out_ops.push(DrawOp {
                    id: Some(effective_id ^ 0x6D00_0000),
                    bounds: exp_bounds,
                    clip: eff_parent_clip,
                    clip_radius: eff_parent_clip_radius,
                    payload: DrawPayload::PopLayer,
                }) {
                    ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                }
            }
        }
        Element::Text(t) => {
            let emit_here =
                (!ctx.overlay_pass && !inh.in_overlay) || (ctx.overlay_pass && inh.in_overlay);
            if !emit_here {
                return;
            }
            let bounds = translate_region(Region::new(node.x, node.y, node.w, node.h), inh.offset);
            let mut color = if t.has_explicit_color() {
                t.color
            } else {
                inh.text_color.unwrap_or(t.color)
            };
            let eff_font_size = if t.has_explicit_size() {
                t.font_size
            } else {
                inh.text_size.unwrap_or(t.font_size)
            };
            let mut op_id = None;
            if let Some(&(text_id, spec)) = inherited_transition {
                if spec.animate_color {
                    op_id = Some(text_id);
                    color = ctx.motion.resolve_text_color_with(text_id, color, spec);
                }
            }
            color = color.scale_alpha(inh.opacity);
            if !ctx.out_ops.push(DrawOp {
                id: op_id,
                bounds,
                clip: inh.parent_clip,
                clip_radius: inh.parent_clip_radius,
                payload: DrawPayload::TextRun {
                    text: t.content,
                    color,
                    font_size: eff_font_size,
                    text_flags: t.flags,
                },
            }) {
                ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
            }
        }
        Element::Img(img) => {
            let emit_here =
                (!ctx.overlay_pass && !inh.in_overlay) || (ctx.overlay_pass && inh.in_overlay);
            if !emit_here {
                return;
            }
            let bounds = translate_region(Region::new(node.x, node.y, node.w, node.h), inh.offset);
            let source = match img.source {
                SpriteSource::Bitmap {
                    bytes,
                    format,
                    tint,
                } => SpriteSource::Bitmap {
                    bytes,
                    format,
                    tint: tint.scale_alpha(inh.opacity),
                },
                SpriteSource::Arc {
                    thickness,
                    start_deg10,
                    sweep_deg10,
                    fill_sweep_deg10,
                    track_color,
                    fill_color,
                } => SpriteSource::Arc {
                    thickness,
                    start_deg10,
                    sweep_deg10,
                    fill_sweep_deg10,
                    track_color: track_color.scale_alpha(inh.opacity),
                    fill_color: fill_color.scale_alpha(inh.opacity),
                },
                SpriteSource::Svg {
                    source,
                    is_path_only,
                    tint,
                    stroke_width_x10,
                } => SpriteSource::Svg {
                    source,
                    is_path_only,
                    tint: tint.scale_alpha(inh.opacity),
                    stroke_width_x10,
                },
                other => other,
            };
            if !ctx.out_ops.push(DrawOp {
                id: None,
                bounds,
                clip: inh.parent_clip,
                clip_radius: inh.parent_clip_radius,
                payload: DrawPayload::Sprite(source),
            }) {
                ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
            }
        }
        Element::RichText(rt) => {
            let emit_here =
                (!ctx.overlay_pass && !inh.in_overlay) || (ctx.overlay_pass && inh.in_overlay);
            if !emit_here {
                return;
            }
            let bounds = translate_region(Region::new(node.x, node.y, node.w, node.h), inh.offset);
            let mut spec = **rt;
            spec.default_color = spec.default_color.scale_alpha(inh.opacity);
            if let Some(rt_ref) = ctx.arena.try_alloc(spec) {
                if !ctx.out_ops.push(DrawOp {
                    id: None,
                    bounds,
                    clip: inh.parent_clip,
                    clip_radius: inh.parent_clip_radius,
                    payload: DrawPayload::RichText(rt_ref),
                }) {
                    ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                }
            }
        }
        Element::DecodedImage(img) => {
            let emit_here =
                (!ctx.overlay_pass && !inh.in_overlay) || (ctx.overlay_pass && inh.in_overlay);
            if !emit_here {
                return;
            }
            let bounds = translate_region(Region::new(node.x, node.y, node.w, node.h), inh.offset);
            let mut spec = **img;
            spec.opacity = ((spec.opacity as u16 * inh.opacity as u16) / 255) as u8;
            if let Some(img_ref) = ctx.arena.try_alloc(spec) {
                if !ctx.out_ops.push(DrawOp {
                    id: None,
                    bounds,
                    clip: inh.parent_clip,
                    clip_radius: inh.parent_clip_radius,
                    payload: DrawPayload::DecodedImage(img_ref),
                }) {
                    ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                }
            }
        }
        Element::Vector(vec_spec) => {
            let emit_here =
                (!ctx.overlay_pass && !inh.in_overlay) || (ctx.overlay_pass && inh.in_overlay);
            if !emit_here {
                return;
            }
            let bounds = translate_region(Region::new(node.x, node.y, node.w, node.h), inh.offset);
            let mut spec = **vec_spec;
            spec.opacity = ((spec.opacity as u16 * inh.opacity as u16) / 255) as u8;
            if let Some(v_ref) = ctx.arena.try_alloc(spec) {
                if !ctx.out_ops.push(DrawOp {
                    id: None,
                    bounds,
                    clip: inh.parent_clip,
                    clip_radius: inh.parent_clip_radius,
                    payload: DrawPayload::Vector(v_ref),
                }) {
                    ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                }
            }
        }
        Element::SceneCanvas(canvas_spec) => {
            let emit_here =
                (!ctx.overlay_pass && !inh.in_overlay) || (ctx.overlay_pass && inh.in_overlay);
            if !emit_here {
                return;
            }
            let bounds = translate_region(Region::new(node.x, node.y, node.w, node.h), inh.offset);
            let mut spec = **canvas_spec;
            spec.opacity = ((spec.opacity as u16 * inh.opacity as u16) / 255) as u8;
            if let Some(c_ref) = ctx.arena.try_alloc(spec) {
                if !ctx.out_ops.push(DrawOp {
                    id: Some(spec.id),
                    bounds,
                    clip: inh.parent_clip,
                    clip_radius: inh.parent_clip_radius,
                    payload: DrawPayload::SceneCanvas(c_ref),
                }) {
                    ctx.dropped_ops = ctx.dropped_ops.saturating_add(1);
                }
            }
        }
        Element::SubView { .. } => {}
    }
}

/// 将矩形整体平移 `(dx, dy)`（用于祖先容器位移动画时子树同步跟随）。
#[inline(always)]
fn translate_region(r: Region, offset: (i16, i16)) -> Region {
    if offset.0 == 0 && offset.1 == 0 {
        return r;
    }
    Region::new(
        r.x.saturating_add(offset.0),
        r.y.saturating_add(offset.1),
        r.w,
        r.h,
    )
}

pub(super) fn node_baseline_offset(node: &LayoutNode<'_, '_>) -> i16 {
    let eff_h = if node.h > 0 { node.h } else { node.pref_size };
    match node.element {
        Element::Text(t) => (((t.font_size as i16) * 4) / 5).max(8).min(eff_h.max(8)),
        Element::Div(d) => {
            let pad_t = div_padding(d).top;
            if let Some(first) = node.children.iter().find(|c| child_pos(c.element).is_none()) {
                let m_top = child_margin(first.element).top;
                pad_t
                    .saturating_add(m_top)
                    .saturating_add(node_baseline_offset(first))
                    .min(eff_h)
            } else {
                eff_h
            }
        }
        _ => eff_h,
    }
}

fn resolve_div_scroll_max(node: &LayoutNode<'_, '_>, div: &DivNode<'_>) -> Option<i16> {
    let raw_max = div.scrollable_max()?;
    if raw_max < i16::MAX {
        return Some(raw_max);
    }
    let pad = div_padding(div);
    let gap = div.style.gap as i16;
    let is_horiz_scroll = div.style.flex_dir() == FlexDir::Row
        && div.grid_cols == 0
        && !div.style.flex_wrap();
    if is_horiz_scroll {
        let inner_w = node.w.saturating_sub(pad.left).saturating_sub(pad.right).max(0);
        let mut flow_count = 0i16;
        let mut sum_w = 0i16;
        for c in node.children.iter() {
            if child_pos(c.element).is_none() {
                let m = child_margin(c.element);
                sum_w = sum_w
                    .saturating_add(c.w)
                    .saturating_add(m.left)
                    .saturating_add(m.right);
                flow_count += 1;
            }
        }
        let total_gap = if flow_count > 1 {
            gap.saturating_mul(flow_count - 1)
        } else {
            0
        };
        let content_w = sum_w.saturating_add(total_gap);
        Some(content_w.saturating_sub(inner_w).max(0))
    } else {
        let inner_h = node.h.saturating_sub(pad.top).saturating_sub(pad.bottom).max(0);
        let mut flow_count = 0i16;
        let mut sum_h = 0i16;
        for c in node.children.iter() {
            if child_pos(c.element).is_none() {
                let m = child_margin(c.element);
                let ch = if c.h > 0 { c.h } else { c.pref_size };
                sum_h = sum_h
                    .saturating_add(ch)
                    .saturating_add(m.top)
                    .saturating_add(m.bottom);
                flow_count += 1;
            }
        }
        let total_gap = if flow_count > 1 {
            gap.saturating_mul(flow_count - 1)
        } else {
            0
        };
        let content_h = sum_h.saturating_add(total_gap);
        Some(content_h.saturating_sub(inner_h).max(0))
    }
}

fn layout_div_children_vertical(node: &mut LayoutNode<'_, '_>, div: &DivNode<'_>) {
    let pad = div_padding(div);
    let pad_t = pad.top;
    let pad_b = pad.bottom;
    // 垂直滚动偏移统一在 Pass 4 通过 `child_offset.1` 向下传递（与水平滚动对称），
    // 此处保持未滚动内容空间坐标，确保滚动过程不会误触发子孙节点的 `animate_bounds` 漂移。
    let inner_y = node.y.saturating_add(pad_t);
    let inner_h = node.h.saturating_sub(pad_t).saturating_sub(pad_b).max(0);
    if node.children.is_empty() {
        return;
    }

    let flow_n = node
        .children
        .iter()
        .filter(|c| child_pos(c.element).is_none())
        .count();

    match div.style.flex_dir() {
        _ if div.grid_cols > 0 => {
            let cols = (div.grid_cols as usize).max(1);
            let gap = div.style.gap as i16;
            let mut row_y = inner_y;
            let mut row_h: i16 = 0;
            let mut col_idx = 0usize;
            for child in node.children.iter_mut() {
                if let Some((_ox, oy)) = child_pos(child.element) {
                    child.y = inner_y.saturating_add(oy);
                    let avail = inner_h.saturating_sub(oy).max(0);
                    let raw = if child_is_overlay(child.element) {
                        child.pref_size
                    } else {
                        child.pref_size.min(avail)
                    };
                    child.h = clamp_h(child.element, raw);
                    continue;
                }
                let m = child_margin(child.element);
                let ch_h = clamp_h(child.element, child.pref_size);
                child.h = ch_h;
                let span_h = m.top.saturating_add(ch_h).saturating_add(m.bottom);
                if col_idx == 0 {
                    row_h = span_h;
                } else {
                    row_h = row_h.max(span_h);
                }
                child.y = row_y.saturating_add(m.top);
                col_idx += 1;
                if col_idx >= cols {
                    row_y = row_y.saturating_add(row_h).saturating_add(gap);
                    row_h = 0;
                    col_idx = 0;
                }
            }
        }
        FlexDir::Col => {
            let gap = div.style.gap as i16;
            let total_gap = if flow_n > 1 {
                gap.saturating_mul((flow_n as i16) - 1)
            } else {
                0
            };
            let mut used_h: i16 = total_gap;
            let mut total_grow: i32 = 0;

            for child in node.children.iter_mut() {
                if let Some((_ox, oy)) = child_pos(child.element) {
                    child.y = inner_y.saturating_add(oy);
                    let avail = inner_h.saturating_sub(oy).max(0);
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.height, Length::Full)
                    );
                    child.h = clamp_h(
                        child.element,
                        if is_full {
                            avail
                        } else if child_is_overlay(child.element) {
                            child.pref_size
                        } else {
                            child.pref_size.min(avail)
                        },
                    );
                    continue;
                }
                let m = child_margin(child.element);
                let my = m.top.saturating_add(m.bottom);
                let is_full = matches!(
                    child.element,
                    Element::Div(d) if matches!(d.style.height, Length::Full)
                );
                let eff_grow = if is_full && child.flex_grow == 0 {
                    1
                } else {
                    child.flex_grow
                };
                if eff_grow > 0 {
                    let base = match child.element {
                        Element::Div(d) => match d.style.height {
                            Length::Px(px) => px,
                            Length::Pct(pct) => ((inner_h as i32 * pct as i32) / 100) as i16,
                            _ => 0,
                        },
                        _ => child.pref_size,
                    };
                    child.h = clamp_h(child.element, base);
                    used_h = used_h.saturating_add(child.h).saturating_add(my);
                    total_grow += eff_grow as i32;
                } else {
                    let base = match child.element {
                        Element::Div(d) => match d.style.height {
                            Length::Pct(pct) => ((inner_h as i32 * pct as i32) / 100) as i16,
                            _ => child.pref_size,
                        },
                        _ => child.pref_size,
                    };
                    child.h = clamp_h(child.element, base);
                    used_h = used_h.saturating_add(child.h).saturating_add(my);
                }
            }

            let rem_h = inner_h.saturating_sub(used_h).max(0);
            if total_grow > 0 && rem_h > 0 {
                let mut distributed: i16 = 0;
                let mut last_grow_idx = 0;
                for (i, child) in node.children.iter_mut().enumerate() {
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.height, Length::Full)
                    );
                    let eff_grow = if is_full && child.flex_grow == 0 {
                        1
                    } else {
                        child.flex_grow
                    };
                    if child_pos(child.element).is_none() && eff_grow > 0 {
                        let share = ((rem_h as i32 * eff_grow as i32) / total_grow) as i16;
                        child.h = clamp_h(child.element, child.h.saturating_add(share));
                        distributed = distributed.saturating_add(share);
                        last_grow_idx = i;
                    }
                }
                let leftover = rem_h.saturating_sub(distributed);
                if leftover > 0 {
                    let last = &mut node.children[last_grow_idx];
                    last.h = clamp_h(last.element, last.h.saturating_add(leftover));
                }
            } else if used_h > inner_h && div.scrollable_max().is_none() {
                // Flex-Shrink：非滚动垂直容器超出高度时，在保证 min_size 的前提下按可缩减空间等比收缩
                let overflow = used_h.saturating_sub(inner_h);
                let mut shrinkable_total: i32 = 0;
                for child in node.children.iter() {
                    if child_pos(child.element).is_none() {
                        let can_shrink = child.h.saturating_sub(child.min_size).max(0);
                        shrinkable_total += can_shrink as i32;
                    }
                }
                if shrinkable_total > 0 {
                    for child in node.children.iter_mut() {
                        if child_pos(child.element).is_none() {
                            let can_shrink = child.h.saturating_sub(child.min_size).max(0);
                            if can_shrink > 0 {
                                let cut = ((overflow as i32 * can_shrink as i32) / shrinkable_total)
                                    .min(can_shrink as i32)
                                    as i16;
                                child.h = clamp_h(child.element, child.h.saturating_sub(cut));
                            }
                        }
                    }
                }
            }

            let actual_total_h: i16 = node
                .children
                .iter()
                .filter(|c| child_pos(c.element).is_none())
                .map(|c| {
                    let m = child_margin(c.element);
                    c.h.saturating_add(m.top).saturating_add(m.bottom)
                })
                .fold(0i16, |a, b| a.saturating_add(b))
                .saturating_add(total_gap);
            let free_space = inner_h.saturating_sub(actual_total_h).max(0);

            let (mut cur_y, extra_gap) = match div.style.justify() {
                Justify::Start => (inner_y, 0),
                Justify::Center => (inner_y.saturating_add(free_space >> 1), 0),
                Justify::End => (inner_y.saturating_add(free_space), 0),
                Justify::Between => {
                    if flow_n > 1 {
                        (inner_y, free_space / ((flow_n as i16) - 1))
                    } else {
                        (inner_y, 0)
                    }
                }
                Justify::Around => {
                    let step = if flow_n > 0 {
                        free_space / (flow_n as i16)
                    } else {
                        0
                    };
                    (inner_y.saturating_add(step >> 1), step)
                }
            };

            for child in node.children.iter_mut() {
                if child_pos(child.element).is_some() {
                    continue;
                }
                let m = child_margin(child.element);
                child.y = cur_y.saturating_add(m.top);
                cur_y = cur_y
                    .saturating_add(m.top)
                    .saturating_add(child.h)
                    .saturating_add(m.bottom)
                    .saturating_add(gap)
                    .saturating_add(extra_gap);
            }
        }
        FlexDir::Row if div.style.flex_wrap() => {
            let gap = div.style.gap as i16;
            let mut row_y = inner_y;
            let mut row_h: i16 = 0;
            let mut prev_right: Option<i16> = None;
            for child in node.children.iter_mut() {
                if let Some((_ox, oy)) = child_pos(child.element) {
                    child.y = inner_y.saturating_add(oy);
                    let avail = inner_h.saturating_sub(oy).max(0);
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.height, Length::Full)
                    );
                    child.h = clamp_h(
                        child.element,
                        if is_full {
                            avail
                        } else if child_is_overlay(child.element) {
                            child.pref_size
                        } else {
                            child.pref_size.min(avail)
                        },
                    );
                    continue;
                }
                let m = child_margin(child.element);
                let ch_h = clamp_h(child.element, child.pref_size);
                child.h = ch_h;
                let span_h = m.top.saturating_add(ch_h).saturating_add(m.bottom);
                if let Some(pr) = prev_right {
                    if child.x < pr {
                        row_y = row_y.saturating_add(row_h).saturating_add(gap);
                        row_h = span_h;
                    } else {
                        row_h = row_h.max(span_h);
                    }
                } else {
                    row_h = span_h;
                }
                child.y = row_y.saturating_add(m.top);
                prev_right = Some(child.x.saturating_add(child.w));
            }
        }
        FlexDir::Row => {
            let align = div.style.items_align();
            let use_baseline = div.items_baseline();
            for child in node.children.iter_mut() {
                if let Some((_ox, oy)) = child_pos(child.element) {
                    child.y = inner_y.saturating_add(oy);
                    let avail = inner_h.saturating_sub(oy).max(0);
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.height, Length::Full)
                    );
                    child.h = clamp_h(
                        child.element,
                        if is_full {
                            avail
                        } else if child_is_overlay(child.element) {
                            child.pref_size
                        } else {
                            child.pref_size.min(avail)
                        },
                    );
                    continue;
                }
                let child_self_align = match child.element {
                    Element::Div(d) => d.align_self(),
                    _ => None,
                };
                let eff_align = child_self_align.unwrap_or(align);
                let m = child_margin(child.element);
                let avail_h = inner_h.saturating_sub(m.top).saturating_sub(m.bottom).max(0);
                let raw_h = match child.element {
                    Element::Div(d) => match d.style.height {
                        Length::Px(px) => px,
                        Length::Pct(pct) => ((avail_h as i32 * pct as i32) / 100) as i16,
                        Length::Full => avail_h,
                        Length::Auto => {
                            if !use_baseline
                                && matches!(eff_align, AlignItems::Stretch)
                            {
                                avail_h
                            } else if use_baseline && matches!(child_self_align, Some(AlignItems::Stretch)) {
                                avail_h
                            } else {
                                child.pref_size.min(avail_h)
                            }
                        }
                    },
                    _ => child.pref_size.min(avail_h),
                };
                child.h = clamp_h(child.element, raw_h);
                child.y = match eff_align {
                    AlignItems::Start | AlignItems::Stretch => inner_y.saturating_add(m.top),
                    AlignItems::Center => inner_y
                        .saturating_add(m.top)
                        .saturating_add(avail_h.saturating_sub(child.h).max(0) >> 1),
                    AlignItems::End => inner_y
                        .saturating_add(m.top)
                        .saturating_add(avail_h.saturating_sub(child.h).max(0)),
                };
            }
            if use_baseline {
                let mut max_baseline: i16 = 0;
                for child in node.children.iter() {
                    if child_pos(child.element).is_none() {
                        let self_align = match child.element {
                            Element::Div(d) => d.align_self(),
                            _ => None,
                        };
                        if self_align.is_none() {
                            let m_top = child_margin(child.element).top;
                            max_baseline = max_baseline
                                .max(m_top.saturating_add(node_baseline_offset(child)));
                        }
                    }
                }
                for child in node.children.iter_mut() {
                    if child_pos(child.element).is_none() {
                        let self_align = match child.element {
                            Element::Div(d) => d.align_self(),
                            _ => None,
                        };
                        if self_align.is_none() {
                            let my_bl = node_baseline_offset(child);
                            child.y = inner_y.saturating_add(max_baseline.saturating_sub(my_bl));
                        }
                    }
                }
            }
        }
    }
}
