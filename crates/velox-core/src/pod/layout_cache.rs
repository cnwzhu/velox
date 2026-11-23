//! `PodPool` 子视图布局缓存（Pass 1/2/3 重排边界）与递归 `SubView` 渲染展开。

use crate::arena::ScratchArena;
use crate::element::Element;

use super::pool::{
    PodPool, ReadersMask, LAYOUT_FLAG_CLAIMED, LAYOUT_FLAG_DIRTY_DESCENDANT,
    LAYOUT_FLAG_HAS_SUBVIEWS, LAYOUT_FLAG_MULTI_MOUNTED, LAYOUT_FLAG_PARENT_NON_AUTO_H,
    LAYOUT_FLAG_PARENT_NON_AUTO_W, LAYOUT_FLAG_SUBTREE_CLEAN,
};
use super::{FrameScope, PodId};

impl PodPool {
    #[inline]
    pub(crate) fn record_relayout_boundary_hit(&self) {
        self.relayout_boundary_hits
            .set(self.relayout_boundary_hits.get().wrapping_add(1));
    }

    #[inline]
    pub(crate) fn begin_layout_pass(&self) {
        for slot in self.slots.iter() {
            if slot.occupied() {
                slot.layout_flags.set(
                    slot.layout_flags.get() & !(LAYOUT_FLAG_CLAIMED | LAYOUT_FLAG_MULTI_MOUNTED),
                );
            }
        }
    }

    #[inline]
    pub(crate) fn try_claim_subview_layout(
        &self,
        id: PodId,
        parent_non_auto_w: bool,
        parent_non_auto_h: bool,
    ) -> bool {
        let Some(slot) = self.slots.get(id.0 as usize) else {
            return false;
        };
        let mut flags = slot.layout_flags.get();
        if (flags & LAYOUT_FLAG_CLAIMED) == 0 {
            flags |= LAYOUT_FLAG_CLAIMED;
            flags &= !(LAYOUT_FLAG_MULTI_MOUNTED
                | LAYOUT_FLAG_PARENT_NON_AUTO_W
                | LAYOUT_FLAG_PARENT_NON_AUTO_H);
            if parent_non_auto_w {
                flags |= LAYOUT_FLAG_PARENT_NON_AUTO_W;
            }
            if parent_non_auto_h {
                flags |= LAYOUT_FLAG_PARENT_NON_AUTO_H;
            }
            slot.layout_flags.set(flags);
            true
        } else {
            flags |= LAYOUT_FLAG_MULTI_MOUNTED;
            flags &= !LAYOUT_FLAG_SUBTREE_CLEAN;
            slot.layout_flags.set(flags);
            slot.cached_layout.set(core::ptr::null_mut());
            false
        }
    }

    #[inline]
    pub(crate) fn get_clean_subview_layout(
        &self,
        id: PodId,
        arena: &ScratchArena,
    ) -> Option<(*mut u8, i16, i16)> {
        let slot = self.slots.get(id.0 as usize)?;
        let flags = slot.layout_flags.get();
        if (flags & (LAYOUT_FLAG_SUBTREE_CLEAN | LAYOUT_FLAG_MULTI_MOUNTED))
            != LAYOUT_FLAG_SUBTREE_CLEAN
        {
            return None;
        }
        let ptr = slot.cached_layout.get();
        if ptr.is_null() || !arena.contains_temp_ptr(ptr as *const u8) {
            return None;
        }
        Some((ptr, slot.pass1_min_w.get(), slot.pass1_pref_w.get()))
    }

    #[inline]
    pub(crate) fn get_subview_pass1(&self, id: PodId) -> (i16, i16) {
        self.slots
            .get(id.0 as usize)
            .map(|s| (s.pass1_min_w.get(), s.pass1_pref_w.get()))
            .unwrap_or((0, 0))
    }

    #[inline]
    pub(crate) fn save_subview_pass1(&self, id: PodId, min_w: i16, pref_w: i16) {
        if let Some(slot) = self.slots.get(id.0 as usize) {
            slot.pass1_min_w.set(min_w);
            slot.pass1_pref_w.set(pref_w);
        }
    }

    #[inline]
    pub(crate) fn get_subview_solved_xw(&self, id: PodId) -> (i16, i16) {
        self.slots
            .get(id.0 as usize)
            .map(|s| {
                let b = s.cached_bounds.get();
                (b.x, b.w)
            })
            .unwrap_or((0, 0))
    }

    #[inline]
    pub(crate) fn save_subview_solved_xw(&self, id: PodId, x: i16, w: i16) {
        if let Some(slot) = self.slots.get(id.0 as usize) {
            let mut b = slot.cached_bounds.get();
            b.x = x;
            b.w = w;
            slot.cached_bounds.set(b);
        }
    }

    #[inline]
    pub(crate) fn get_subview_pass3(&self, id: PodId) -> (i16, i16) {
        self.slots
            .get(id.0 as usize)
            .map(|s| (s.pass3_min_h.get(), s.pass3_pref_h.get()))
            .unwrap_or((0, 0))
    }

    #[inline]
    pub(crate) fn save_subview_pass3_and_node(
        &self,
        id: PodId,
        min_h: i16,
        pref_h: i16,
        node_ptr: *mut u8,
    ) {
        if let Some(slot) = self.slots.get(id.0 as usize) {
            slot.pass3_min_h.set(min_h);
            slot.pass3_pref_h.set(pref_h);
            slot.cached_layout.set(node_ptr);
        }
    }

    #[inline]
    pub(crate) fn update_subview_node_ptr(&self, id: PodId, node_ptr: *mut u8) {
        if let Some(slot) = self.slots.get(id.0 as usize) {
            slot.cached_layout.set(node_ptr);
        }
    }

    #[inline]
    pub(crate) fn get_subview_cached_element(
        &self,
        id: PodId,
        arena: &ScratchArena,
    ) -> Option<&'static Element<'static>> {
        let slot = self.slots.get(id.0 as usize)?;
        let ptr = slot.cached_element.get();
        if ptr.is_null() || !arena.contains_temp_ptr(ptr as *const u8) {
            return None;
        }
        Some(unsafe { &*ptr })
    }

    #[inline]
    pub(crate) fn can_relayout_subview_in_place(
        &self,
        id: PodId,
        arena: &ScratchArena,
    ) -> Option<*mut u8> {
        let slot = self.slots.get(id.0 as usize)?;
        if !slot.occupied() {
            return None;
        }
        let flags = slot.layout_flags.get();
        if (flags & (LAYOUT_FLAG_DIRTY_DESCENDANT | LAYOUT_FLAG_MULTI_MOUNTED)) != 0 {
            return None;
        }
        let layout_ptr = slot.cached_layout.get();
        if layout_ptr.is_null() || !arena.contains_temp_ptr(layout_ptr as *const u8) {
            return None;
        }
        let elem_ptr = slot.cached_element.get();
        if elem_ptr.is_null() || !arena.contains_temp_ptr(elem_ptr as *const u8) {
            return None;
        }
        Some(layout_ptr)
    }

    #[inline]
    pub(crate) fn mark_subview_subtree_clean(&self, id: PodId) {
        if let Some(slot) = self.slots.get(id.0 as usize) {
            slot.layout_flags.set(
                (slot.layout_flags.get() | LAYOUT_FLAG_SUBTREE_CLEAN)
                    & !LAYOUT_FLAG_DIRTY_DESCENDANT,
            );
        }
    }

    #[inline]
    pub(crate) fn subview_parent_non_auto_w(&self, id: PodId) -> bool {
        self.slots
            .get(id.0 as usize)
            .map(|s| (s.layout_flags.get() & LAYOUT_FLAG_PARENT_NON_AUTO_W) != 0)
            .unwrap_or(false)
    }

    #[inline]
    pub(crate) fn subview_parent_non_auto_h(&self, id: PodId) -> bool {
        self.slots
            .get(id.0 as usize)
            .map(|s| (s.layout_flags.get() & LAYOUT_FLAG_PARENT_NON_AUTO_H) != 0)
            .unwrap_or(false)
    }

    /// 调用指定 `PodId` 的 `View::render` 生成瞬态 `Element<'a>` 树（自动递归展开所有嵌套 `SubView(PodId)`），并返回根引用与 `ScratchArena`。
    ///
    /// 注意：接收 `&self` 而非 `&mut self`，配合 `UnsafeCell` 与 `Cell<bool>` 借用锁，确保渲染期间 `frame.pool_ptr` 与方法调用严格满足 Stacked Borrows 共享只读起源规则。
    pub fn render_pod_in_arena<'a>(
        &self,
        id: PodId,
        frame: &'a mut FrameScope,
    ) -> Option<(&'a Element<'a>, &'a ScratchArena)> {
        frame.pool_ptr = self as *const PodPool;
        let frame_ptr = frame as *mut FrameScope;
        let root_opt = unsafe { self.render_single_pod_raw(id, None, frame_ptr) };
        frame.pool_ptr = core::ptr::null();
        let root_static = root_opt?;
        let arena = frame.arena();
        let root_ref: &'a Element<'a> =
            unsafe { &*(root_static as *const Element<'static> as *const Element<'a>) };
        Some((root_ref, arena))
    }

    unsafe fn render_single_pod_raw(
        &self,
        id: PodId,
        expected_gen: Option<u16>,
        frame_ptr: *mut FrameScope,
    ) -> Option<&'static Element<'static>> {
        let slot = self.slots.get(id.0 as usize)?;
        if slot.is_borrowed() || slot.is_expanding() || slot.active_readers.get() > 0 {
            return None;
        }
        if let Some(g) = expected_gen {
            if slot.generation != g {
                return None;
            }
        }
        let vt = slot.vtable?;

        // Retained SubView 快径：若当前视图未脏且其缓存的 Element 仍驻留在当前 ScratchArena 的有效临时区内，
        // 则直接跳过 View::render() 调用；若该视图不包含嵌套 SubView，则连同子树遍历一并跳过。
        if !slot.dirty {
            let cached_ptr = slot.cached_element.get();
            if !cached_ptr.is_null() {
                let valid_in_arena = unsafe {
                    (*frame_ptr)
                        .arena()
                        .contains_temp_ptr(cached_ptr as *const u8)
                };
                if valid_in_arena {
                    self.skipped_renders
                        .set(self.skipped_renders.get().wrapping_add(1));
                    let alloc_ref: &'static Element<'static> = unsafe { &*cached_ptr };
                    let flags = slot.layout_flags.get();
                    if (flags & LAYOUT_FLAG_HAS_SUBVIEWS) == 0 {
                        slot.layout_flags.set(
                            (flags | LAYOUT_FLAG_SUBTREE_CLEAN) & !LAYOUT_FLAG_DIRTY_DESCENDANT,
                        );
                        return Some(alloc_ref);
                    }
                    slot.set_borrowed(true);
                    slot.set_expanding(true);
                    let (_has_sub, any_desc_dirty) =
                        unsafe { self.expand_subviews_ref(alloc_ref, frame_ptr) };
                    slot.set_expanding(false);
                    slot.set_borrowed(false);
                    let mut new_flags = slot.layout_flags.get();
                    if any_desc_dirty {
                        new_flags =
                            (new_flags | LAYOUT_FLAG_DIRTY_DESCENDANT) & !LAYOUT_FLAG_SUBTREE_CLEAN;
                    } else {
                        new_flags =
                            (new_flags | LAYOUT_FLAG_SUBTREE_CLEAN) & !LAYOUT_FLAG_DIRTY_DESCENDANT;
                    }
                    slot.layout_flags.set(new_flags);
                    return Some(alloc_ref);
                }
            }
        }

        let mask_bits = (size_of::<ReadersMask>() * 8) as u16;
        if id.0 < mask_bits {
            let bit_mask: ReadersMask = !(1 << id.0);
            for s in self.slots.iter() {
                s.readers_mask.set(s.readers_mask.get() & bit_mask);
            }
        }

        slot.set_borrowed(true);
        slot.set_expanding(true);
        unsafe {
            let prev_pod = (*frame_ptr).current_pod;
            let prev_pod_gen = (*frame_ptr).current_pod_gen;
            (*frame_ptr).current_pod = id;
            (*frame_ptr).current_pod_gen = Some(slot.generation);
            let prev_frame_req = (*frame_ptr).frame_requested.replace(false);
            let prev_tracking = crate::signal::begin_pod_tracking(self.ensure_pool_id(), id);
            let storage_ptr = self.slot_storage_ptr(slot);
            let el_static = (vt.render_fn)(storage_ptr, frame_ptr);
            crate::signal::end_pod_tracking(prev_tracking);
            let this_frame_req = (*frame_ptr).frame_requested.get();
            (*frame_ptr)
                .frame_requested
                .set(prev_frame_req || this_frame_req);
            slot.set_needs_next_frame(this_frame_req);
            slot.render_count
                .set(slot.render_count.get().wrapping_add(1));
            slot.set_borrowed(false);
            let alloc_ref: &'static Element<'static> =
                &*(*frame_ptr).arena().alloc_temp(el_static);
            slot.cached_element
                .set(alloc_ref as *const Element<'static>);
            let (has_sub, any_desc_dirty) = self.expand_subviews_ref(alloc_ref, frame_ptr);
            let mut new_flags = slot.layout_flags.get()
                & !(LAYOUT_FLAG_SUBTREE_CLEAN
                    | LAYOUT_FLAG_HAS_SUBVIEWS
                    | LAYOUT_FLAG_DIRTY_DESCENDANT);
            if has_sub {
                new_flags |= LAYOUT_FLAG_HAS_SUBVIEWS;
            }
            if any_desc_dirty {
                new_flags |= LAYOUT_FLAG_DIRTY_DESCENDANT;
            }
            slot.layout_flags.set(new_flags);
            slot.set_expanding(false);
            (*frame_ptr).current_pod = prev_pod;
            (*frame_ptr).current_pod_gen = prev_pod_gen;
            Some(alloc_ref)
        }
    }

    unsafe fn expand_subviews_ref(
        &self,
        elem: &'static Element<'static>,
        frame_ptr: *mut FrameScope,
    ) -> (bool, bool) {
        match elem {
            Element::Div(div) => {
                let mut has_sub = false;
                let mut any_dirty = false;
                let mut cur = div.first_child;
                while let Some(node) = cur {
                    let (child_sub, child_dirty) =
                        unsafe { self.expand_subviews_ref(&node.element, frame_ptr) };
                    has_sub |= child_sub;
                    any_dirty |= child_dirty;
                    cur = node.next_sibling.get();
                }
                (has_sub, any_dirty)
            }
            Element::SubView {
                id,
                generation,
                resolved,
            } => {
                let sub_id = *id;
                let sub_gen = *generation;
                let sub_was_dirty = self
                    .slots
                    .get(sub_id.0 as usize)
                    .map(|s| s.dirty)
                    .unwrap_or(true);
                let sub_ref =
                    unsafe { self.render_single_pod_raw(sub_id, Some(sub_gen), frame_ptr) };
                resolved.set(sub_ref);
                let sub_desc_dirty = self
                    .slots
                    .get(sub_id.0 as usize)
                    .map(|s| (s.layout_flags.get() & LAYOUT_FLAG_DIRTY_DESCENDANT) != 0)
                    .unwrap_or(true);
                (true, sub_was_dirty || sub_desc_dirty || sub_ref.is_none())
            }
            _ => (false, false),
        }
    }
}

