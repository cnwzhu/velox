//! 四步一维解耦静态布局引擎（4-Pass O(N) Layout Solver）与零分配 `DrawOp` 扁平化器。
//!
//! 模块职责划分：
//! - `types`：几何矩形 [`Region`]、交互热区 [`HitRegion`]、[`InteractionState`] 与布局节点 `LayoutNode`；
//! - `layout`：布局树构建与 Pass 1–3 横纵解耦尺寸测量与分配；
//! - `flatten`：Pass 4 纵向坐标落位、滚动裁剪、过渡动画插值与扁平绘制指令输出。

mod flatten;
mod layout;
mod types;

use crate::arena::{ScratchArena, SmallVec};
use crate::element::Element;
use crate::motion::{DrawOp, MotionCache};
use crate::style::Length;

use flatten::{node_baseline_offset, pass4_vertical_assign_and_flatten};
use layout::{
    build_layout_tree, build_layout_tree_with_pool, pass1_horizontal_measure,
    pass1_horizontal_measure_with_pool, pass2_horizontal_assign,
    pass2_horizontal_assign_with_pool, pass3_vertical_measure, pass3_vertical_measure_with_pool,
};
pub use types::{HitRegion, InteractionState, Region};
pub(crate) use types::LayoutNode;
use types::{
    clamp_h, clamp_w, outer_box_spec, SUBVIEW_FLAG_CACHED_PASS1, SUBVIEW_FLAG_OWNER,
    SUBVIEW_FLAG_ROOT,
};

/// 执行完整的四步一维解耦布局求解，并将元素树拍平为 `DrawOp` 列表与 `HitRegion` 列表（返回本帧因容量上限丢弃的绘制指令数）。
pub fn solve_and_flatten<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    arena: &'a ScratchArena,
    root: &'a Element<'a>,
    viewport: Region,
    motion: &mut MotionCache,
    out_ops: &mut SmallVec<DrawOp<'a>, MAX_OPS>,
    out_hits: &mut SmallVec<HitRegion<'a>, MAX_HITS>,
) -> u32 {
    solve_and_flatten_with_state(
        arena,
        root,
        viewport,
        InteractionState::default(),
        motion,
        out_ops,
        out_hits,
    )
}

/// 携带交互状态（指针坐标、按下态、焦点序号）的四步布局求解与扁平化管线（返回本帧因容量上限丢弃的绘制指令数）。
#[inline(never)]
pub fn solve_and_flatten_with_state<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    arena: &'a ScratchArena,
    root: &'a Element<'a>,
    viewport: Region,
    interaction: InteractionState,
    motion: &mut MotionCache,
    out_ops: &mut SmallVec<DrawOp<'a>, MAX_OPS>,
    out_hits: &mut SmallVec<HitRegion<'a>, MAX_HITS>,
) -> u32 {
    let mut root_node = build_layout_tree(arena, root);

    // Pass 1：水平约束汇总（自底向上）
    pass1_horizontal_measure(&mut root_node);

    // Pass 2：水平宽度切分（自顶向下）
    root_node.x = viewport.x;
    root_node.w = match root {
        Element::Div(d) => match d.style.width {
            Length::Px(px) => clamp_w(root, px),
            Length::Pct(pct) => clamp_w(root, ((viewport.w as i32 * pct as i32) / 100) as i16),
            Length::Full | Length::Auto => clamp_w(root, viewport.w),
        },
        _ => viewport.w,
    };
    pass2_horizontal_assign(&mut root_node);

    // Pass 3：文本折行与垂直约束汇总（自底向上）
    pass3_vertical_measure(&mut root_node);

    // Pass 4：垂直高度切分与指令拍平（自顶向下）
    reflatten_pass4_only(
        &mut root_node,
        viewport,
        interaction,
        motion,
        out_ops,
        out_hits,
    )
}

/// 执行 Pass 1~4 并将已求解完 Pass 1~3 的根 `LayoutNode` 保留在 `ScratchArena` 顶端临时区中，供后续帧直接运行 [`reflatten_pass4_only`]。
#[allow(dead_code)]
pub(crate) fn solve_and_flatten_retained<'a, const MAX_OPS: usize, const MAX_HITS: usize>(
    arena: &'a ScratchArena,
    root: &'a Element<'a>,
    viewport: Region,
    interaction: InteractionState,
    motion: &mut MotionCache,
    out_ops: &mut SmallVec<DrawOp<'a>, MAX_OPS>,
    out_hits: &mut SmallVec<HitRegion<'a>, MAX_HITS>,
) -> (*mut LayoutNode<'static, 'static>, u32) {
    solve_and_flatten_retained_with_pool(
        arena,
        root,
        None,
        viewport,
        interaction,
        motion,
        out_ops,
        out_hits,
    )
}

/// 携带 `PodPool` 子视图布局隔离边界缓存的 Pass 1~4 求解器：
/// 对未脏且外部分配宽度未变的 `SubView` 直接复用其 `LayoutNode` 子树与 Pass 1~3 测量结果。
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_and_flatten_retained_with_pool<
    'a,
    const MAX_OPS: usize,
    const MAX_HITS: usize,
>(
    arena: &'a ScratchArena,
    root: &'a Element<'a>,
    pool: Option<&crate::pod::PodPool>,
    viewport: Region,
    interaction: InteractionState,
    motion: &mut MotionCache,
    out_ops: &mut SmallVec<DrawOp<'a>, MAX_OPS>,
    out_hits: &mut SmallVec<HitRegion<'a>, MAX_HITS>,
) -> (*mut LayoutNode<'static, 'static>, u32) {
    if let Some(p) = pool {
        p.begin_layout_pass();
    }
    let root_node = arena.alloc_temp(build_layout_tree_with_pool(arena, root, pool, true, true));

    // Pass 1：水平约束汇总（自底向上，未脏 SubView 直接复用 pass1 缓存）
    pass1_horizontal_measure_with_pool(root_node, pool);

    // Pass 2：水平宽度切分（自顶向下，宽度未变的未脏 SubView 跳过子树 Pass 2）
    root_node.x = viewport.x;
    root_node.w = match root {
        Element::Div(d) => match d.style.width {
            Length::Px(px) => clamp_w(root, px),
            Length::Pct(pct) => clamp_w(root, ((viewport.w as i32 * pct as i32) / 100) as i16),
            Length::Full | Length::Auto => clamp_w(root, viewport.w),
        },
        _ => viewport.w,
    };
    pass2_horizontal_assign_with_pool(root_node, pool);

    // Pass 3：文本折行与垂直约束汇总（自底向上，宽度未变的未脏 SubView 跳过子树 Pass 3）
    pass3_vertical_measure_with_pool(root_node, pool, true);

    let dropped_ops =
        reflatten_pass4_only(root_node, viewport, interaction, motion, out_ops, out_hits);
    let raw_ptr = root_node as *mut LayoutNode<'a, '_> as *mut LayoutNode<'static, 'static>;
    (raw_ptr, dropped_ops)
}

/// 尝试在保留的 `LayoutNode` 树上仅对脏 `SubView` 执行 $O(\text{dirty\_pod})$ 原地局部重排（Relayout Boundary）：
///
/// 若所有脏 `SubView` 的外部盒模型规格不变，且其水平/垂直测量满足布局隔离边界条件（不改变父容器及兄弟节点的尺寸分配），
/// 则直接原地替换脏 `SubView` 的 `LayoutNode` 子树并跳过全局 `build_layout_tree` 与全局 Pass 1~3。
pub(crate) fn try_relayout_dirty_subviews_in_place<'a>(
    arena: &'a ScratchArena,
    pool: &crate::pod::PodPool,
    dirty_ids: &[crate::pod::PodId],
) -> bool {
    if dirty_ids.is_empty() || dirty_ids.len() > 8 {
        return false;
    }

    struct PendingCommit<'a, 'b> {
        id: crate::pod::PodId,
        target_ptr: *mut LayoutNode<'a, 'b>,
        new_node: LayoutNode<'a, 'b>,
        min_w: i16,
        pref_w: i16,
        x: i16,
        w: i16,
        min_h: i16,
        pref_h: i16,
    }

    let saved_mark = arena.save_mark();
    let saved_temp = arena.save_temp_mark();
    let mut commits: SmallVec<PendingCommit<'a, 'a>, 8> = SmallVec::new();

    for &dirty_id in dirty_ids {
        let Some(target_raw_ptr) = pool.can_relayout_subview_in_place(dirty_id, arena) else {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        };
        let target_ptr = target_raw_ptr as *mut LayoutNode<'a, 'a>;
        let old_node = unsafe { &*target_ptr };
        let Some(new_elem_static) = pool.get_subview_cached_element(dirty_id, arena) else {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        };
        let new_elem: &'a Element<'a> =
            unsafe { &*(new_elem_static as *const Element<'static> as *const Element<'a>) };
        if matches!(new_elem, Element::SubView { .. }) {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        }

        let spec = outer_box_spec(new_elem);
        if spec != outer_box_spec(old_node.element) {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        }

        let (old_min_w, old_pref_w) = pool.get_subview_pass1(dirty_id);
        let (old_x, old_w) = pool.get_subview_solved_xw(dirty_id);
        let (old_min_h, old_pref_h) = pool.get_subview_pass3(dirty_id);
        let old_y = old_node.y;
        let old_h = old_node.h;

        if old_x != old_node.x
            || old_w != old_node.w
            || old_min_h != old_node.min_size
            || old_pref_h != old_node.pref_size
        {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        }

        let parent_non_auto_w = pool.subview_parent_non_auto_w(dirty_id);
        let parent_non_auto_h = pool.subview_parent_non_auto_h(dirty_id);
        let child_non_auto_w = match new_elem {
            Element::Div(d) => {
                let scroll_horizontal = d.style.flex_dir() == crate::style::FlexDir::Row
                    && d.grid_cols == 0
                    && !d.style.flex_wrap();
                parent_non_auto_w
                    && !matches!(d.style.width, Length::Auto)
                    && !(d.scrollable_max().is_some() && scroll_horizontal)
                    && !d.style.flex_wrap()
            }
            _ => false,
        };
        let child_non_auto_h = match new_elem {
            Element::Div(d) => {
                let scroll_horizontal = d.style.flex_dir() == crate::style::FlexDir::Row
                    && d.grid_cols == 0
                    && !d.style.flex_wrap();
                parent_non_auto_h
                    && !matches!(d.style.height, Length::Auto)
                    && !(d.scrollable_max() == Some(i16::MAX) && !scroll_horizontal)
                    && d.grid_cols == 0
                    && !d.style.flex_wrap()
                    && !d.items_baseline()
            }
            _ => false,
        };

        let mut new_node = build_layout_tree_with_pool(
            arena,
            new_elem,
            Some(pool),
            child_non_auto_w,
            child_non_auto_h,
        );
        new_node.subview_flags = SUBVIEW_FLAG_ROOT | SUBVIEW_FLAG_OWNER;
        new_node.subview_pod = dirty_id.0;

        pass1_horizontal_measure_with_pool(&mut new_node, Some(pool));
        let new_min_w = new_node.min_size;
        let new_pref_w = new_node.pref_size;

        let horiz_isolated = (new_min_w == old_min_w && new_pref_w == old_pref_w)
            || (parent_non_auto_w
                && matches!(spec.width, Length::Px(px) if old_w == clamp_w(new_elem, px))
                && new_pref_w == old_pref_w
                && new_min_w <= old_w)
            || (parent_non_auto_w
                && matches!(spec.width, Length::Full)
                && old_w > spec.min_w
                && new_min_w <= old_w);

        if !horiz_isolated {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        }

        new_node.x = old_x;
        new_node.w = old_w;
        pass2_horizontal_assign_with_pool(&mut new_node, Some(pool));
        pass3_vertical_measure_with_pool(&mut new_node, Some(pool), false);

        let new_min_h = new_node.min_size;
        let new_pref_h = new_node.pref_size;

        let vert_isolated = (new_min_h == old_min_h && new_pref_h == old_pref_h)
            || (parent_non_auto_h
                && matches!(spec.height, Length::Px(px) if old_h == clamp_h(new_elem, px))
                && new_pref_h == old_pref_h
                && new_min_h <= old_h)
            || (parent_non_auto_h
                && matches!(spec.height, Length::Full)
                && old_h > spec.min_h
                && new_min_h <= old_h);

        if !vert_isolated {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        }

        new_node.y = old_y;
        new_node.h = old_h;
        if !parent_non_auto_h && node_baseline_offset(&new_node) != node_baseline_offset(old_node) {
            unsafe {
                arena.rollback_speculative_marks(saved_mark, saved_temp);
            }
            return false;
        }

        new_node.subview_flags = SUBVIEW_FLAG_ROOT | SUBVIEW_FLAG_CACHED_PASS1 | SUBVIEW_FLAG_OWNER;
        let _ = commits.push(PendingCommit {
            id: dirty_id,
            target_ptr,
            new_node,
            min_w: new_min_w,
            pref_w: new_pref_w,
            x: old_x,
            w: old_w,
            min_h: new_min_h,
            pref_h: new_pref_h,
        });
    }

    for commit in commits.as_mut_slice() {
        commit_subtree_cache_ptrs(&mut commit.new_node, pool);
        unsafe {
            *commit.target_ptr = core::ptr::read(&commit.new_node);
        }
        pool.save_subview_pass1(commit.id, commit.min_w, commit.pref_w);
        pool.save_subview_solved_xw(commit.id, commit.x, commit.w);
        pool.save_subview_pass3_and_node(
            commit.id,
            commit.min_h,
            commit.pref_h,
            commit.target_ptr as *mut u8,
        );
        pool.mark_subview_subtree_clean(commit.id);
        pool.record_relayout_boundary_hit();
    }

    true
}

fn commit_subtree_cache_ptrs(node: &mut LayoutNode<'_, '_>, pool: &crate::pod::PodPool) {
    for child in node.children.iter_mut() {
        if (child.subview_flags & SUBVIEW_FLAG_ROOT) != 0 {
            let id = crate::pod::PodId(child.subview_pod);
            pool.save_subview_pass3_and_node(
                id,
                child.min_size,
                child.pref_size,
                child as *mut LayoutNode<'_, '_> as *mut u8,
            );
            pool.mark_subview_subtree_clean(id);
        }
        if !child.children.is_empty() {
            commit_subtree_cache_ptrs(child, pool);
        }
    }
}

/// Pass-4-Only 极速重拍平路径：当本帧无任何 `Pod` 状态变更（仅发生指针悬停/按下、焦点切换、滚动偏移或过渡动画插值）时，
/// 直接跳过 `View::render()` 与 Pass 1~3 水平/折行求解，仅在保留的 `LayoutNode` 树上执行 Pass 4 生成最新 `DrawOp` 与 `HitRegion`。
#[inline(never)]
pub(crate) fn reflatten_pass4_only<'a, 'b, const MAX_OPS: usize, const MAX_HITS: usize>(
    root_node: &mut LayoutNode<'a, 'b>,
    viewport: Region,
    interaction: InteractionState,
    motion: &mut MotionCache,
    out_ops: &mut SmallVec<DrawOp<'a>, MAX_OPS>,
    out_hits: &mut SmallVec<HitRegion<'a>, MAX_HITS>,
) -> u32 {
    let root = root_node.element;
    root_node.y = viewport.y;
    root_node.h = match root {
        Element::Div(d) => match d.style.height {
            Length::Px(px) => clamp_h(root, px),
            Length::Pct(pct) => clamp_h(root, ((viewport.h as i32 * pct as i32) / 100) as i16),
            Length::Full | Length::Auto => clamp_h(root, viewport.h),
        },
        _ => viewport.h,
    };
    let mut focus_counter: u16 = 0;
    let mut has_overlays = false;
    let mut dropped_ops: u32 = 0;
    let root_owner = if (root_node.subview_flags & (SUBVIEW_FLAG_ROOT | SUBVIEW_FLAG_OWNER)) != 0 {
        root_node.subview_pod
    } else {
        u16::MAX
    };
    motion.begin_frame();
    pass4_vertical_assign_and_flatten(
        root_node,
        root_owner,
        0x811c_9dc5,
        (0, 0),
        viewport,
        viewport,
        0,
        None,
        255,
        (0, 0),
        None,
        None,
        interaction,
        false,
        false,
        &mut has_overlays,
        &mut focus_counter,
        &mut dropped_ops,
        motion,
        out_ops,
        out_hits,
    );
    // 若树中存在标记了 `.overlay()` 的顶层浮层/弹窗节点，在第二遍扫描中将其统一拍平至 Z-Order 最顶层
    if has_overlays {
        pass4_vertical_assign_and_flatten(
            root_node,
            root_owner,
            0x811c_9dc5,
            (0, 0),
            viewport,
            viewport,
            0,
            None,
            255,
            (0, 0),
            None,
            None,
            interaction,
            true,
            false,
            &mut has_overlays,
            &mut focus_counter,
            &mut dropped_ops,
            motion,
            out_ops,
            out_hits,
        );
    }
    // 回收本帧已从元素树中消失的过渡动画槽位
    motion.end_frame();
    dropped_ops
}

