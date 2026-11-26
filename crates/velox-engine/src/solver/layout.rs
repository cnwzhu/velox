//! 4-Pass 一维解耦布局引擎前端：树构建与 Pass 1–3（横向测量、横向分配/折行与纵向测量）。

use crate::arena::ScratchArena;
use crate::display::{count_text_lines, measure_text_px};
use crate::element::Element;
use crate::style::{AlignItems, FlexDir, Justify, Length};

use super::types::{
    child_is_overlay, child_margin, child_pos, clamp_h, clamp_w, count_children, div_padding,
    LayoutNode, SUBVIEW_FLAG_CACHED_PASS1, SUBVIEW_FLAG_OWNER, SUBVIEW_FLAG_ROOT,
    SUBVIEW_FLAG_SKIP_PASS3,
};

pub(super) fn build_layout_tree<'a, 'b>(
    arena: &'b ScratchArena,
    elem: &'a Element<'a>,
) -> LayoutNode<'a, 'b> {
    build_layout_tree_with_pool(arena, elem, None, true, true)
}

pub(super) fn build_layout_tree_with_pool<'a, 'b>(
    arena: &'b ScratchArena,
    elem: &'a Element<'a>,
    pool: Option<&crate::pod::PodPool>,
    parent_non_auto_w: bool,
    parent_non_auto_h: bool,
) -> LayoutNode<'a, 'b> {
    build_layout_tree_inner(arena, elem, pool, parent_non_auto_w, parent_non_auto_h, false, false)
}

fn build_layout_tree_inner<'a, 'b>(
    arena: &'b ScratchArena,
    elem: &'a Element<'a>,
    pool: Option<&crate::pod::PodPool>,
    parent_non_auto_w: bool,
    parent_non_auto_h: bool,
    self_grows_w: bool,
    self_grows_h: bool,
) -> LayoutNode<'a, 'b> {
    match elem {
        Element::SubView { id, resolved, .. } => {
            if let Some(inner) = resolved.get() {
                if matches!(inner, Element::SubView { .. }) {
                    return build_layout_tree_inner(
                        arena,
                        inner,
                        pool,
                        parent_non_auto_w,
                        parent_non_auto_h,
                        self_grows_w,
                        self_grows_h,
                    );
                }
                let mut claimed = false;
                if let Some(p) = pool {
                    claimed =
                        p.try_claim_subview_layout(*id, parent_non_auto_w, parent_non_auto_h);
                    if claimed {
                        if let Some((cached_ptr, min_w, pref_w)) =
                            p.get_clean_subview_layout(*id, arena)
                        {
                            let cached_node =
                                unsafe { &mut *(cached_ptr as *mut LayoutNode<'a, 'b>) };
                            let children_valid = cached_node.children.is_empty()
                                || arena
                                    .contains_temp_ptr(cached_node.children.as_ptr() as *const u8);
                            if children_valid {
                                let children_slice: &'b mut [LayoutNode<'a, 'b>] = unsafe {
                                    core::slice::from_raw_parts_mut(
                                        cached_node.children.as_mut_ptr(),
                                        cached_node.children.len(),
                                    )
                                };
                                return LayoutNode {
                                    element: inner,
                                    min_size: min_w,
                                    pref_size: pref_w,
                                    flex_grow: cached_node.flex_grow,
                                    subview_flags: SUBVIEW_FLAG_ROOT
                                        | SUBVIEW_FLAG_CACHED_PASS1
                                        | SUBVIEW_FLAG_OWNER,
                                    x: cached_node.x,
                                    y: cached_node.y,
                                    w: cached_node.w,
                                    h: cached_node.h,
                                    subview_pod: id.0,
                                    children: children_slice,
                                };
                            }
                        }
                    }
                }
                let mut node = build_layout_tree_inner(
                    arena,
                    inner,
                    pool,
                    parent_non_auto_w,
                    parent_non_auto_h,
                    self_grows_w,
                    self_grows_h,
                );
                node.subview_pod = id.0;
                node.subview_flags |= SUBVIEW_FLAG_OWNER;
                if claimed {
                    node.subview_flags |= SUBVIEW_FLAG_ROOT;
                }
                node
            } else {
                LayoutNode {
                    element: elem,
                    min_size: 0,
                    pref_size: 0,
                    flex_grow: 0,
                    subview_flags: 0,
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                    subview_pod: 0,
                    children: &mut [],
                }
            }
        }
        Element::Div(div) => {
            let scroll_horizontal = div.style.flex_dir() == FlexDir::Row
                && div.grid_cols == 0
                && !div.style.flex_wrap();
            let blocks_w_by_scroll = div.scrollable_max().is_some() && scroll_horizontal;
            let blocks_h_by_scroll = div.scrollable_max() == Some(i16::MAX) && !scroll_horizontal;

            let child_non_auto_w = parent_non_auto_w
                && (!matches!(div.style.width, Length::Auto) || self_grows_w)
                && !blocks_w_by_scroll
                && !div.style.flex_wrap();
            let child_non_auto_h = parent_non_auto_h
                && (!matches!(div.style.height, Length::Auto) || self_grows_h)
                && !blocks_h_by_scroll
                && div.grid_cols == 0
                && !div.style.flex_wrap()
                && !div.items_baseline();
            let is_row = div.style.flex_dir() == FlexDir::Row
                && div.grid_cols == 0
                && !div.style.flex_wrap();
            let is_col = div.style.flex_dir() == FlexDir::Col && div.grid_cols == 0;
            let count = count_children(div.first_child);
            let mut cur = div.first_child;
            let children = arena.alloc_temp_iter_exact(
                count,
                (0..count).map(|_| {
                    let node = cur.unwrap();
                    cur = node.next_sibling.get();
                    // 在确定宽/高的 Row/Col 容器中，`flex_grow > 0` 的子 Div 主轴基数为 0（不依赖子树 pref_size），
                    // 同样构成严格的布局隔离边界。
                    let child_grows_w = is_row
                        && child_non_auto_w
                        && matches!(node.element, Element::Div(ref cd) if cd.style.flex_grow() > 0);
                    let child_grows_h = is_col
                        && child_non_auto_h
                        && matches!(node.element, Element::Div(ref cd) if cd.style.flex_grow() > 0);
                    build_layout_tree_inner(
                        arena,
                        &node.element,
                        pool,
                        child_non_auto_w,
                        child_non_auto_h,
                        child_grows_w,
                        child_grows_h,
                    )
                }),
            );
            LayoutNode {
                element: elem,
                min_size: 0,
                pref_size: 0,
                flex_grow: div.style.flex_grow(),
                subview_flags: 0,
                x: 0,
                y: 0,
                w: 0,
                h: 0,
                subview_pod: 0,
                children,
            }
        }
        _ => LayoutNode {
            element: elem,
            min_size: 0,
            pref_size: 0,
            flex_grow: 0,
            subview_flags: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            subview_pod: 0,
            children: &mut [],
        },
    }
}

/// Pass 1（自底向上）：汇总每个节点的水平最小宽度与理想宽度（存入 `min_size` / `pref_size`）。
pub(super) fn pass1_horizontal_measure(node: &mut LayoutNode<'_, '_>) {
    pass1_horizontal_measure_with_pool(node, None);
}

pub(super) fn pass1_horizontal_measure_with_pool(
    node: &mut LayoutNode<'_, '_>,
    pool: Option<&crate::pod::PodPool>,
) {
    pass1_horizontal_measure_inner(node, pool, None);
}

fn pass1_horizontal_measure_inner(
    node: &mut LayoutNode<'_, '_>,
    pool: Option<&crate::pod::PodPool>,
    inherited_text_size: Option<u8>,
) {
    if (node.subview_flags & SUBVIEW_FLAG_CACHED_PASS1) != 0 {
        if let Some(p) = pool {
            let (min_w, pref_w) = p.get_subview_pass1(crate::pod::PodId(node.subview_pod));
            node.min_size = min_w;
            node.pref_size = pref_w;
            p.record_relayout_boundary_hit();
            return;
        }
    }

    let next_text_size = match node.element {
        Element::Div(div) => div.text_size().or(inherited_text_size),
        _ => inherited_text_size,
    };

    for child in node.children.iter_mut() {
        pass1_horizontal_measure_inner(child, pool, next_text_size);
    }

    match node.element {
        Element::Div(div) => {
            let pad = div_padding(div);
            let pad_x = pad.left.saturating_add(pad.right);
            let gap = div.style.gap as i16;
            let flow_n = node
                .children
                .iter()
                .filter(|c| child_pos(c.element).is_none())
                .count() as i16;
            let total_gap = if flow_n > 1 {
                gap.saturating_mul(flow_n - 1)
            } else {
                0
            };

            let (content_min, content_pref) = match div.style.flex_dir() {
                _ if div.grid_cols > 0 => {
                    let cols = div.grid_cols as i16;
                    let mut max_col_min: i16 = 0;
                    let mut max_col_pref: i16 = 0;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_none() {
                            let m = child_margin(c.element);
                            let mx = m.left.saturating_add(m.right);
                            max_col_min = max_col_min.max(c.min_size.saturating_add(mx));
                            max_col_pref = max_col_pref.max(c.pref_size.saturating_add(mx));
                        }
                    }
                    let grid_gap = if cols > 1 {
                        gap.saturating_mul(cols - 1)
                    } else {
                        0
                    };
                    (
                        max_col_min.saturating_mul(cols).saturating_add(grid_gap),
                        max_col_pref.saturating_mul(cols).saturating_add(grid_gap),
                    )
                }
                FlexDir::Row => {
                    let mut min_sum: i16 = 0;
                    let mut pref_sum: i16 = 0;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_none() {
                            let m = child_margin(c.element);
                            let mx = m.left.saturating_add(m.right);
                            min_sum = min_sum.saturating_add(c.min_size).saturating_add(mx);
                            pref_sum = pref_sum.saturating_add(c.pref_size).saturating_add(mx);
                        }
                    }
                    (
                        min_sum.saturating_add(total_gap),
                        pref_sum.saturating_add(total_gap),
                    )
                }
                FlexDir::Col => {
                    let mut max_min: i16 = 0;
                    let mut max_pref: i16 = 0;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_none() {
                            let m = child_margin(c.element);
                            let mx = m.left.saturating_add(m.right);
                            max_min = max_min.max(c.min_size.saturating_add(mx));
                            max_pref = max_pref.max(c.pref_size.saturating_add(mx));
                        }
                    }
                    (max_min, max_pref)
                }
            };

            let (raw_min, raw_pref) = match div.style.width {
                Length::Px(px) => (content_min.saturating_add(pad_x).min(px), px),
                Length::Full | Length::Auto | Length::Pct(_) => (
                    content_min.saturating_add(pad_x),
                    content_pref.saturating_add(pad_x),
                ),
            };
            node.min_size = clamp_w(node.element, raw_min);
            node.pref_size = clamp_w(node.element, raw_pref);
        }
        Element::Text(t) => {
            let eff_size = if t.has_explicit_size() {
                t.font_size
            } else {
                inherited_text_size.unwrap_or(t.font_size)
            };
            let (tw, _th) = measure_text_px(t.content.as_str(), eff_size);
            node.min_size = if t.is_nowrap() && !t.is_ellipsis() {
                tw
            } else {
                (eff_size as i16).min(tw)
            };
            node.pref_size = tw;
        }
        Element::Img(img) => {
            node.min_size = img.width;
            node.pref_size = img.width;
        }
        Element::RichText(rt) => {
            let (tw, _th) = crate::text_rich::measure_rich_text_box(rt, 0);
            node.min_size = if rt.nowrap {
                tw
            } else {
                (rt.default_font_size.max(8) as i16).min(tw)
            };
            node.pref_size = tw;
        }
        Element::DecodedImage(img) => {
            node.min_size = img.width;
            node.pref_size = img.width;
        }
        Element::Vector(vec_spec) => {
            node.min_size = vec_spec.width;
            node.pref_size = vec_spec.width;
        }
        Element::SubView { .. } => {
            node.min_size = 0;
            node.pref_size = 0;
        }
    }

    if (node.subview_flags & SUBVIEW_FLAG_ROOT) != 0 {
        if let Some(p) = pool {
            p.save_subview_pass1(
                crate::pod::PodId(node.subview_pod),
                node.min_size,
                node.pref_size,
            );
        }
    }
}

pub(super) fn shift_subtree_x(
    children: &mut [LayoutNode<'_, '_>],
    dx: i16,
    pool: Option<&crate::pod::PodPool>,
) {
    for child in children.iter_mut() {
        child.x = child.x.saturating_add(dx);
        if (child.subview_flags & SUBVIEW_FLAG_ROOT) != 0 {
            if let Some(p) = pool {
                p.save_subview_solved_xw(crate::pod::PodId(child.subview_pod), child.x, child.w);
            }
        }
        if !child.children.is_empty() {
            shift_subtree_x(child.children, dx, pool);
        }
    }
}

/// Pass 2（自顶向下）：按 `flex_grow` 整数比例切分父容器剩余水平空间，确定所有节点的最终 `x` 与 `w`。
pub(super) fn pass2_horizontal_assign(node: &mut LayoutNode<'_, '_>) {
    pass2_horizontal_assign_with_pool(node, None);
}

pub(super) fn pass2_horizontal_assign_with_pool(
    node: &mut LayoutNode<'_, '_>,
    pool: Option<&crate::pod::PodPool>,
) {
    if (node.subview_flags & SUBVIEW_FLAG_ROOT) != 0 {
        if let Some(p) = pool {
            let id = crate::pod::PodId(node.subview_pod);
            if (node.subview_flags & SUBVIEW_FLAG_CACHED_PASS1) != 0 {
                let (old_x, old_w) = p.get_subview_solved_xw(id);
                if node.w == old_w {
                    if node.x != old_x {
                        let dx = node.x.saturating_sub(old_x);
                        shift_subtree_x(node.children, dx, pool);
                        p.save_subview_solved_xw(id, node.x, node.w);
                    }
                    node.subview_flags |= SUBVIEW_FLAG_SKIP_PASS3;
                    return;
                }
                node.subview_flags &= !(SUBVIEW_FLAG_CACHED_PASS1 | SUBVIEW_FLAG_SKIP_PASS3);
                for child in node.children.iter_mut() {
                    pass1_horizontal_measure_with_pool(child, pool);
                }
            }
            p.save_subview_solved_xw(id, node.x, node.w);
        }
    }

    let Element::Div(div) = node.element else {
        return;
    };

    let pad = div_padding(div);
    let pad_l = pad.left;
    let pad_r = pad.right;
    let inner_x = node.x.saturating_add(pad_l);
    let inner_w = node.w.saturating_sub(pad_l).saturating_sub(pad_r).max(0);
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
            // 二维网格布局（Grid）：支持等分列、自定义 `fr` 权重列（`grid_template_fr`）与子项跨列（`grid_col_span`）
            let cols = (div.grid_cols as usize).max(1);
            let gap = div.style.gap as i16;
            let total_gap = gap.saturating_mul((cols as i16) - 1);
            let avail_tracks = inner_w.saturating_sub(total_gap).max(0);
            let fr_weights: &[u16] = div
                .layout_ext()
                .map(|l| l.grid_template_fr)
                .unwrap_or(&[]);

            // 预计算每列的 (offset_x, width)（支持最多 16 列 `fr` 权重零堆排布）
            let mut col_x = [0i16; 16];
            let mut col_w = [0i16; 16];
            let track_n = cols.min(16);
            if !fr_weights.is_empty() {
                let mut sum_fr: i32 = 0;
                for i in 0..track_n {
                    sum_fr += (*fr_weights.get(i).unwrap_or(&1)).max(1) as i32;
                }
                let sum_fr = sum_fr.max(1);
                let mut cx = inner_x;
                let mut used = 0i16;
                for i in 0..track_n {
                    let w = if i + 1 == track_n {
                        avail_tracks.saturating_sub(used).max(0)
                    } else {
                        let fr = (*fr_weights.get(i).unwrap_or(&1)).max(1) as i32;
                        ((avail_tracks as i32 * fr) / sum_fr) as i16
                    };
                    col_x[i] = cx;
                    col_w[i] = w;
                    cx = cx.saturating_add(w).saturating_add(gap);
                    used = used.saturating_add(w);
                }
            } else {
                let cell_w = avail_tracks / (cols as i16);
                for i in 0..track_n {
                    col_x[i] = inner_x.saturating_add((i as i16).saturating_mul(cell_w.saturating_add(gap)));
                    col_w[i] = cell_w;
                }
            }

            let mut cur_col = 0usize;
            for child in node.children.iter_mut() {
                if let Some((ox, _oy)) = child_pos(child.element) {
                    child.x = inner_x.saturating_add(ox);
                    let avail = inner_w.saturating_sub(ox).max(0);
                    let raw = if child_is_overlay(child.element) {
                        child.pref_size
                    } else {
                        child.pref_size.min(avail)
                    };
                    child.w = clamp_w(child.element, raw);
                    continue;
                }
                let span = match child.element {
                    Element::Div(cd) => cd
                        .layout_ext()
                        .map(|l| l.grid_col_span as usize)
                        .unwrap_or(1)
                        .clamp(1, cols),
                    _ => 1,
                };
                if cur_col + span > cols {
                    cur_col = 0;
                }
                let start_col = cur_col;
                let end_col = (start_col + span).min(cols);
                let (cx, span_w) = if end_col <= track_n {
                    let sx = col_x[start_col];
                    let mut sw = 0i16;
                    for c in start_col..end_col {
                        sw = sw.saturating_add(col_w[c]);
                        if c > start_col {
                            sw = sw.saturating_add(gap);
                        }
                    }
                    (sx, sw)
                } else {
                    let cell_w = avail_tracks / (cols as i16);
                    let sx = inner_x
                        .saturating_add((start_col as i16).saturating_mul(cell_w.saturating_add(gap)));
                    let sw = cell_w
                        .saturating_mul((end_col - start_col) as i16)
                        .saturating_add(gap.saturating_mul((end_col - start_col - 1) as i16));
                    (sx, sw)
                };
                let m = child_margin(child.element);
                child.x = cx.saturating_add(m.left);
                child.w = clamp_w(
                    child.element,
                    span_w.saturating_sub(m.left).saturating_sub(m.right).max(0),
                );
                cur_col = end_col % cols;
            }
        }
        FlexDir::Row if div.style.flex_wrap() => {
            // 多行流式折行布局（Flex Wrap）
            let gap = div.style.gap as i16;
            let mut cur_x = inner_x;
            for child in node.children.iter_mut() {
                if let Some((ox, _oy)) = child_pos(child.element) {
                    child.x = inner_x.saturating_add(ox);
                    let avail = inner_w.saturating_sub(ox).max(0);
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.width, Length::Full)
                    );
                    child.w = clamp_w(
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
                let cw = clamp_w(child.element, child.pref_size.min(inner_w));
                child.w = cw;
                let span_w = m.left.saturating_add(cw).saturating_add(m.right);
                if cur_x > inner_x && cur_x.saturating_add(span_w) > inner_x.saturating_add(inner_w)
                {
                    cur_x = inner_x;
                }
                child.x = cur_x.saturating_add(m.left);
                cur_x = cur_x.saturating_add(span_w).saturating_add(gap);
            }
        }
        FlexDir::Row => {
            let gap = div.style.gap as i16;
            let total_gap = if flow_n > 1 {
                gap.saturating_mul((flow_n as i16) - 1)
            } else {
                0
            };
            let mut used_w: i16 = total_gap;
            let mut total_grow: i32 = 0;

            for child in node.children.iter_mut() {
                if let Some((ox, _oy)) = child_pos(child.element) {
                    child.x = inner_x.saturating_add(ox);
                    let avail = inner_w.saturating_sub(ox).max(0);
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.width, Length::Full)
                    );
                    child.w = clamp_w(
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
                let mx = m.left.saturating_add(m.right);
                let is_full = matches!(
                    child.element,
                    Element::Div(d) if matches!(d.style.width, Length::Full)
                );
                let eff_grow = if is_full && child.flex_grow == 0 {
                    1
                } else {
                    child.flex_grow
                };
                if eff_grow > 0 {
                    let base = match child.element {
                        Element::Div(d) => match d.style.width {
                            Length::Px(px) => px,
                            Length::Pct(pct) => ((inner_w as i32 * pct as i32) / 100) as i16,
                            _ => 0,
                        },
                        _ => child.pref_size,
                    };
                    child.w = clamp_w(child.element, base);
                    used_w = used_w.saturating_add(child.w).saturating_add(mx);
                    total_grow += eff_grow as i32;
                } else {
                    let base = match child.element {
                        Element::Div(d) => match d.style.width {
                            Length::Pct(pct) => ((inner_w as i32 * pct as i32) / 100) as i16,
                            _ => child.pref_size,
                        },
                        _ => child.pref_size,
                    };
                    child.w = clamp_w(child.element, base);
                    used_w = used_w.saturating_add(child.w).saturating_add(mx);
                }
            }

            let rem_w = inner_w.saturating_sub(used_w).max(0);
            if total_grow > 0 && rem_w > 0 {
                let mut distributed: i16 = 0;
                let mut last_grow_idx = 0;
                for (i, child) in node.children.iter_mut().enumerate() {
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.width, Length::Full)
                    );
                    let eff_grow = if is_full && child.flex_grow == 0 {
                        1
                    } else {
                        child.flex_grow
                    };
                    if child_pos(child.element).is_none() && eff_grow > 0 {
                        let share = ((rem_w as i32 * eff_grow as i32) / total_grow) as i16;
                        child.w = clamp_w(child.element, child.w.saturating_add(share));
                        distributed = distributed.saturating_add(share);
                        last_grow_idx = i;
                    }
                }
                let leftover = rem_w.saturating_sub(distributed);
                if leftover > 0 {
                    let last = &mut node.children[last_grow_idx];
                    last.w = clamp_w(last.element, last.w.saturating_add(leftover));
                }
            } else if used_w > inner_w && div.scrollable_max().is_none() {
                // Flex-Shrink：当行内总宽度超出容器宽度时，在保证 min_size 的前提下按可缩减空间等比收缩
                let overflow = used_w.saturating_sub(inner_w);
                let mut shrinkable_total: i32 = 0;
                for child in node.children.iter() {
                    if child_pos(child.element).is_none() {
                        let can_shrink = child.w.saturating_sub(child.min_size).max(0);
                        shrinkable_total += can_shrink as i32;
                    }
                }
                if shrinkable_total > 0 {
                    for child in node.children.iter_mut() {
                        if child_pos(child.element).is_none() {
                            let can_shrink = child.w.saturating_sub(child.min_size).max(0);
                            if can_shrink > 0 {
                                let cut = ((overflow as i32 * can_shrink as i32) / shrinkable_total)
                                    .min(can_shrink as i32)
                                    as i16;
                                child.w = clamp_w(child.element, child.w.saturating_sub(cut));
                            }
                        }
                    }
                }
            }

            let actual_total_w: i16 = node
                .children
                .iter()
                .filter(|c| child_pos(c.element).is_none())
                .map(|c| {
                    let m = child_margin(c.element);
                    c.w.saturating_add(m.left).saturating_add(m.right)
                })
                .fold(0i16, |a, b| a.saturating_add(b))
                .saturating_add(total_gap);
            let free_space = inner_w.saturating_sub(actual_total_w).max(0);

            let (mut cur_x, extra_gap) = match div.style.justify() {
                Justify::Start => (inner_x, 0),
                Justify::Center => (inner_x.saturating_add(free_space >> 1), 0),
                Justify::End => (inner_x.saturating_add(free_space), 0),
                Justify::Between => {
                    if flow_n > 1 {
                        (inner_x, free_space / ((flow_n as i16) - 1))
                    } else {
                        (inner_x, 0)
                    }
                }
                Justify::Around => {
                    let step = if flow_n > 0 {
                        free_space / (flow_n as i16)
                    } else {
                        0
                    };
                    (inner_x.saturating_add(step >> 1), step)
                }
            };

            let last_flow_w = node
                .children
                .iter()
                .rev()
                .find(|c| child_pos(c.element).is_none())
                .map(|c| {
                    let m = child_margin(c.element);
                    c.w.saturating_add(m.left).saturating_add(m.right)
                })
                .unwrap_or(0);
            let mut flow_idx = 0usize;
            for child in node.children.iter_mut() {
                if child_pos(child.element).is_some() {
                    continue;
                }
                let m = child_margin(child.element);
                child.x = cur_x.saturating_add(m.left);
                cur_x = cur_x
                    .saturating_add(m.left)
                    .saturating_add(child.w)
                    .saturating_add(m.right)
                    .saturating_add(gap);
                if flow_idx + 1 == flow_n.saturating_sub(1)
                    && matches!(div.style.justify(), Justify::Between)
                    && flow_n > 1
                {
                    let right_edge = inner_x.saturating_add(inner_w);
                    cur_x = right_edge.saturating_sub(last_flow_w);
                } else {
                    cur_x = cur_x.saturating_add(extra_gap);
                }
                flow_idx += 1;
            }
            // RTL 布局镜像：当父容器启用 `.rtl(true)` 时，将 Row 内流式子节点按内容区水平镜像排布
            if div.layout_ext().is_some_and(|l| l.rtl) {
                let right_edge = inner_x.saturating_add(inner_w);
                for child in node.children.iter_mut() {
                    if child_pos(child.element).is_none() {
                        let rel_l = child.x.saturating_sub(inner_x);
                        child.x = right_edge.saturating_sub(rel_l).saturating_sub(child.w);
                    }
                }
            }
        }
        FlexDir::Col => {
            for child in node.children.iter_mut() {
                if let Some((ox, _oy)) = child_pos(child.element) {
                    child.x = inner_x.saturating_add(ox);
                    let avail = inner_w.saturating_sub(ox).max(0);
                    let is_full = matches!(
                        child.element,
                        Element::Div(d) if matches!(d.style.width, Length::Full)
                    );
                    child.w = clamp_w(
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
                let avail_w = inner_w.saturating_sub(m.left).saturating_sub(m.right).max(0);
                let is_full = matches!(
                    child.element,
                    Element::Div(d) if matches!(d.style.width, Length::Full)
                );
                let child_self_align = match child.element {
                    Element::Div(d) => d.align_self(),
                    _ => None,
                };
                let align = child_self_align.unwrap_or_else(|| div.style.items_align());
                let raw_w = match child.element {
                    Element::Div(d) => match d.style.width {
                        Length::Px(px) => px,
                        Length::Pct(pct) => ((avail_w as i32 * pct as i32) / 100) as i16,
                        Length::Full => avail_w,
                        Length::Auto => {
                            if matches!(align, AlignItems::Stretch)
                                || (child_self_align.is_none()
                                    && (matches!(d.style.justify(), Justify::Between)
                                        || d.style.flex_dir() == FlexDir::Row))
                            {
                                avail_w
                            } else {
                                child.pref_size.min(avail_w)
                            }
                        }
                    },
                    _ => child.pref_size.min(avail_w),
                };
                child.w = clamp_w(child.element, if is_full { avail_w } else { raw_w });
                child.x = match align {
                    AlignItems::Start | AlignItems::Stretch => inner_x.saturating_add(m.left),
                    AlignItems::Center => inner_x
                        .saturating_add(m.left)
                        .saturating_add(avail_w.saturating_sub(child.w).max(0) >> 1),
                    AlignItems::End => inner_x
                        .saturating_add(m.left)
                        .saturating_add(avail_w.saturating_sub(child.w).max(0)),
                };
            }
        }
    }

    for child in node.children.iter_mut() {
        pass2_horizontal_assign_with_pool(child, pool);
    }
}

/// Pass 3（自底向上）：代入 Pass 2 确定的 `w` 计算文本折行高度，原地覆写复用 `min_size` 与 `pref_size` 存储垂直约束。
pub(super) fn pass3_vertical_measure(node: &mut LayoutNode<'_, '_>) {
    pass3_vertical_measure_with_pool(node, None, false);
}

pub(super) fn pass3_vertical_measure_with_pool(
    node: &mut LayoutNode<'_, '_>,
    pool: Option<&crate::pod::PodPool>,
    commit_cache: bool,
) {
    pass3_vertical_measure_inner(node, pool, commit_cache, None);
}

fn pass3_vertical_measure_inner(
    node: &mut LayoutNode<'_, '_>,
    pool: Option<&crate::pod::PodPool>,
    commit_cache: bool,
    inherited_text_size: Option<u8>,
) {
    if (node.subview_flags & SUBVIEW_FLAG_SKIP_PASS3) != 0 {
        if let Some(p) = pool {
            let id = crate::pod::PodId(node.subview_pod);
            let (min_h, pref_h) = p.get_subview_pass3(id);
            node.min_size = min_h;
            node.pref_size = pref_h;
            node.subview_flags = SUBVIEW_FLAG_ROOT | SUBVIEW_FLAG_CACHED_PASS1;
            if commit_cache {
                p.update_subview_node_ptr(id, node as *mut LayoutNode<'_, '_> as *mut u8);
            }
            return;
        }
    }

    let next_text_size = match node.element {
        Element::Div(div) => div.text_size().or(inherited_text_size),
        _ => inherited_text_size,
    };

    for child in node.children.iter_mut() {
        pass3_vertical_measure_inner(child, pool, commit_cache, next_text_size);
    }

    match node.element {
        Element::Div(div) => {
            let pad = div_padding(div);
            let pad_y = pad.top.saturating_add(pad.bottom);
            let gap = div.style.gap as i16;
            let flow_n = node
                .children
                .iter()
                .filter(|c| child_pos(c.element).is_none())
                .count() as i16;
            let total_gap = if flow_n > 1 {
                gap.saturating_mul(flow_n - 1)
            } else {
                0
            };

            let (content_min, content_pref) = match div.style.flex_dir() {
                _ if div.grid_cols > 0 => {
                    let cols = (div.grid_cols as usize).max(1);
                    let mut total_h: i16 = 0;
                    let mut row_h: i16 = 0;
                    let mut col_idx = 0usize;
                    let mut rows = 0i16;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_some() {
                            continue;
                        }
                        let m = child_margin(c.element);
                        let ch_h = c.pref_size.saturating_add(m.top).saturating_add(m.bottom);
                        if col_idx == 0 {
                            rows += 1;
                            row_h = ch_h;
                        } else {
                            row_h = row_h.max(ch_h);
                        }
                        col_idx += 1;
                        if col_idx >= cols {
                            total_h = total_h.saturating_add(row_h);
                            row_h = 0;
                            col_idx = 0;
                        }
                    }
                    if col_idx > 0 {
                        total_h = total_h.saturating_add(row_h);
                    }
                    if rows > 1 {
                        total_h = total_h.saturating_add(gap.saturating_mul(rows - 1));
                    }
                    (total_h, total_h)
                }
                FlexDir::Col => {
                    let mut min_sum: i16 = 0;
                    let mut pref_sum: i16 = 0;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_none() {
                            let m = child_margin(c.element);
                            let my = m.top.saturating_add(m.bottom);
                            min_sum = min_sum.saturating_add(c.min_size).saturating_add(my);
                            pref_sum = pref_sum.saturating_add(c.pref_size).saturating_add(my);
                        }
                    }
                    (
                        min_sum.saturating_add(total_gap),
                        pref_sum.saturating_add(total_gap),
                    )
                }
                FlexDir::Row if div.style.flex_wrap() => {
                    // 多行折行后的总高度 = 各行最大高度之和 + 行间距
                    let mut total_h: i16 = 0;
                    let mut row_h: i16 = 0;
                    let mut prev_right: Option<i16> = None;
                    let mut lines = 0i16;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_some() {
                            continue;
                        }
                        let m = child_margin(c.element);
                        let ch_h = c.pref_size.saturating_add(m.top).saturating_add(m.bottom);
                        if let Some(pr) = prev_right {
                            if c.x < pr {
                                total_h = total_h.saturating_add(row_h);
                                row_h = ch_h;
                                lines += 1;
                            } else {
                                row_h = row_h.max(ch_h);
                            }
                        } else {
                            row_h = ch_h;
                            lines = 1;
                        }
                        prev_right = Some(c.x.saturating_add(c.w));
                    }
                    total_h = total_h.saturating_add(row_h);
                    if lines > 1 {
                        total_h = total_h.saturating_add(gap.saturating_mul(lines - 1));
                    }
                    (total_h, total_h)
                }
                FlexDir::Row => {
                    let mut max_min: i16 = 0;
                    let mut max_pref: i16 = 0;
                    for c in node.children.iter() {
                        if child_pos(c.element).is_none() {
                            let m = child_margin(c.element);
                            let my = m.top.saturating_add(m.bottom);
                            max_min = max_min.max(c.min_size.saturating_add(my));
                            max_pref = max_pref.max(c.pref_size.saturating_add(my));
                        }
                    }
                    (max_min, max_pref)
                }
            };

            let (raw_min, raw_pref) = match div.style.height {
                Length::Px(px) => (px, px),
                Length::Full | Length::Auto | Length::Pct(_) => {
                    if let Some(ar) = div
                        .layout_ext()
                        .map(|l| l.aspect_ratio)
                        .filter(|&r| r > 0.01)
                    {
                        let h_ar = ((node.w.max(0) as f32 / ar) + 0.5) as i16;
                        (h_ar, h_ar)
                    } else {
                        (
                            content_min.saturating_add(pad_y),
                            content_pref.saturating_add(pad_y),
                        )
                    }
                }
            };
            node.min_size = clamp_h(node.element, raw_min);
            node.pref_size = clamp_h(node.element, raw_pref);
        }
        Element::Text(t) => {
            let eff_size = if t.has_explicit_size() {
                t.font_size
            } else {
                inherited_text_size.unwrap_or(t.font_size)
            };
            let th = crate::display::text_line_height_px(eff_size);
            let avail_w = node.w.max(eff_size as i16);
            let single_line = t.is_nowrap() || t.is_ellipsis();
            // Pass 1 已将单行像素总宽保存在 node.pref_size 中：若单行宽度已可容纳且无显式换行符，无需重扫折行。
            let lines = if single_line
                || (node.pref_size <= avail_w && !t.content.as_str().contains('\n'))
            {
                1
            } else {
                count_text_lines(t.content.as_str(), eff_size, avail_w, false)
            };
            let h = th.saturating_mul(lines);
            node.min_size = h;
            node.pref_size = h;
        }
        Element::Img(img) => {
            node.min_size = img.height;
            node.pref_size = img.height;
        }
        Element::RichText(rt) => {
            let avail_w = node.w.max(rt.default_font_size.max(8) as i16);
            let (_tw, th) = crate::text_rich::measure_rich_text_box(rt, avail_w);
            node.min_size = th;
            node.pref_size = th;
        }
        Element::DecodedImage(img) => {
            node.min_size = img.height;
            node.pref_size = img.height;
        }
        Element::Vector(vec_spec) => {
            node.min_size = vec_spec.height;
            node.pref_size = vec_spec.height;
        }
        Element::SubView { .. } => {
            node.min_size = 0;
            node.pref_size = 0;
        }
    }

    if (node.subview_flags & SUBVIEW_FLAG_ROOT) != 0 {
        node.subview_flags = SUBVIEW_FLAG_ROOT | SUBVIEW_FLAG_CACHED_PASS1 | SUBVIEW_FLAG_OWNER;
        if commit_cache {
            if let Some(p) = pool {
                let id = crate::pod::PodId(node.subview_pod);
                p.save_subview_pass3_and_node(
                    id,
                    node.min_size,
                    node.pref_size,
                    node as *mut LayoutNode<'_, '_> as *mut u8,
                );
                p.mark_subview_subtree_clean(id);
            }
        }
    }
}

