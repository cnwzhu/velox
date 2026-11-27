//! 指针交互、拖拽/长按/自动连发/滑动手势状态机与滚轮事件派发。

use crate::pod::{CursorShape, DragEvent, PointerKind, SwipeDir};

use super::VeloxEngine;

impl VeloxEngine {
    /// 根据当前指针位置、按下/拖拽态与最顶层命中热区解析实时鼠标光标形状（严格遵守 `occlude` 遮罩阻断）。
    pub fn cursor_shape(&self) -> CursorShape {
        let hits = self.cached_hit_regions();
        if hits.is_empty() {
            return CursorShape::Default;
        }

        // 1. 正在按住拖拽某个 `on_drag` 热区（如 Slider / ResizableSplit）时锁定拖拽光标，避免指针短暂滑出边缘闪烁
        if self.interaction.is_pressed && self.has_pressed_hit {
            if let Some(hit) = hits
                .iter()
                .rev()
                .find(|h| h.id == Some(self.pressed_hit_id))
            {
                if hit.on_drag().is_some() {
                    return match hit.cursor {
                        Some(
                            c @ (CursorShape::Pointer
                            | CursorShape::Text
                            | CursorShape::ResizeEW
                            | CursorShape::ResizeNS
                            | CursorShape::ResizeNWSE
                            | CursorShape::ResizeNESW
                            | CursorShape::Move
                            | CursorShape::Crosshair),
                        ) => c,
                        Some(CursorShape::NotAllowed) => CursorShape::NotAllowed,
                        _ => CursorShape::Grabbing,
                    };
                }
            }
        }

        let Some((px, py)) = self.interaction.pointer_pos else {
            return CursorShape::Default;
        };

        // 2. 逆序命中测试（后绘制/高 Z-Order 优先），优先采用显式 `.cursor(...)`，否则按语义自动推断
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            if effective.contains(px, py) {
                if let Some(explicit) = hit.cursor {
                    if explicit == CursorShape::Grab && self.interaction.is_pressed {
                        return CursorShape::Grabbing;
                    }
                    return explicit;
                }
                if hit.on_key_input().is_some() {
                    return CursorShape::Text;
                }
                if hit.on_drag().is_some() {
                    return if self.interaction.is_pressed {
                        CursorShape::Grabbing
                    } else {
                        CursorShape::Grab
                    };
                }
                if hit.on_click().is_some() || hit.on_long_press().is_some() {
                    return CursorShape::Pointer;
                }
                if hit.occlude {
                    return CursorShape::Default;
                }
            }
        }
        CursorShape::Default
    }

    /// 以显式指针设备来源（[`PointerKind::Mouse`] / [`PointerKind::Touch`] / [`PointerKind::Pen`]）派发指针按下事件。
    #[inline]
    pub fn dispatch_pointer_down_with_kind(
        &mut self,
        px: i16,
        py: i16,
        kind: PointerKind,
    ) -> bool {
        self.active_pointer_kind = kind;
        self.dispatch_pointer_down(px, py)
    }

    /// 派发桌面鼠标左键按下事件（标记为 [`PointerKind::Mouse`]，禁用空白区域按住拖动画布滚动与边缘右滑返回）。
    #[inline]
    pub fn dispatch_mouse_down(&mut self, px: i16, py: i16) -> bool {
        self.dispatch_pointer_down_with_kind(px, py, PointerKind::Mouse)
    }

    /// 派发触控屏手指按下事件（标记为 [`PointerKind::Touch`]，启用跟手拖拽滚动、惯性滚动与边缘右滑返回）。
    #[inline]
    pub fn dispatch_touch_down(&mut self, px: i16, py: i16) -> bool {
        self.dispatch_pointer_down_with_kind(px, py, PointerKind::Touch)
    }

    /// 派发指针/手指按下事件（激活 `active_bg`、捕获焦点与拖拽起点，直接复用缓存 `HitRegion` 切片避免重复整树排版）。
    #[inline(never)]
    pub fn dispatch_pointer_down(&mut self, px: i16, py: i16) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        self.interaction.pointer_pos = Some((px, py));
        self.interaction.is_pressed = true;
        // 指针点击不激活键盘可见焦点环（`:focus-visible`），仅文本输入控件在聚焦时展示柔和输入边框
        self.interaction.focus_visible = false;
        self.press_start_x = px;
        self.press_start_y = py;
        self.has_pressed_hit = false;
        self.drag_fired_on_down = false;
        self.press_hold_ms = 0;
        self.repeat_accum_ms = 0;
        self.long_press_fired = false;
        self.long_press_handled = false;
        self.last_drag_vy = 0;
        self.interaction_dirty = true;

        let mut hit_focused: Option<u16> = None;
        let mut matched_passive = false;
        let hits = self.cached_hit_regions();
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            if effective.contains(px, py) {
                if hit_focused.is_none() {
                    if let Some(fidx) = hit.focus_index {
                        hit_focused = Some(fidx);
                        self.interaction.focused_index = Some(fidx);
                    }
                }
                if self.allows_drag_scroll() {
                    if let Some((scroll_id, max_s)) = hit.scroll_target {
                        self.motion.set_scroll_inertia(scroll_id, 0, max_s);
                    }
                }
                if let Some(on_drag) = hit.on_drag() {
                    if let Some(hid) = hit.id {
                        self.pressed_hit_id = hid;
                        self.has_pressed_hit = true;
                    }
                    self.drag_fired_on_down = true;
                    let drag_ev = DragEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_x: 0,
                        delta_y: 0,
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let mods = self.modifiers;
                    {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), drag_ev)
                            .with_modifiers(mods);
                        on_drag(&mut acx);
                    }
                    self.flush_async_after_event();
                    return true;
                }
                if hit.on_click().is_some() || hit.on_long_press().is_some() {
                    if let Some(hid) = hit.id {
                        self.pressed_hit_id = hid;
                        self.has_pressed_hit = true;
                    }
                    return true;
                }
                if hit.active_bg_or_focus() {
                    matched_passive = true;
                }
                if hit.occlude {
                    if hit_focused.is_none() {
                        self.interaction.focused_index = None;
                    }
                    return true;
                }
            }
        }
        // 点击空白非可聚焦区域时自然失焦，避免残留过时的聚焦状态
        if hit_focused.is_none() {
            self.interaction.focused_index = None;
        }
        matched_passive
    }

    /// 派发鼠标右键（次键）按下事件（命中注册了 `.on_secondary_click(...)` 的节点并触发回调）。
    #[inline(never)]
    pub fn dispatch_pointer_secondary_down(&mut self, px: i16, py: i16) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        self.interaction.pointer_pos = Some((px, py));
        self.interaction.focus_visible = false;
        self.interaction_dirty = true;

        let hits = self.cached_hit_regions();
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            if effective.contains(px, py) {
                if let Some(fidx) = hit.focus_index {
                    self.interaction.focused_index = Some(fidx);
                }
                if let Some(on_secondary) = hit.on_secondary_click() {
                    let drag_ev = DragEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_x: 0,
                        delta_y: 0,
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let mods = self.modifiers;
                    {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), drag_ev)
                            .with_modifiers(mods);
                        on_secondary(&mut acx);
                    }
                    self.flush_async_after_event();
                    return true;
                }
                if hit.occlude {
                    return true;
                }
            }
        }
        false
    }

    /// 派发指针移动/拖拽事件（更新 `hover_bg`、触发 `on_drag` 滑块连续拖动或滚动容器跟手滑动，复用帧缓存热区）。
    #[inline(never)]
    pub fn dispatch_pointer_move(&mut self, px: i16, py: i16) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        let prev_pos = self.interaction.pointer_pos;
        let (prev_x, prev_y) = prev_pos.unwrap_or((px, py));
        let dx = px.saturating_sub(prev_x);
        let dy = py.saturating_sub(prev_y);
        self.interaction.pointer_pos = Some((px, py));

        let hits = self.cached_hit_regions();
        if !self.interaction.is_pressed {
            let hover_changed = hits.iter().any(|h| {
                if !h.has_hover_bg {
                    return false;
                }
                let eff = h.bounds.intersect(h.clip);
                let was_in = prev_pos.is_some_and(|(ox, oy)| eff.contains(ox, oy));
                was_in != eff.contains(px, py)
            });
            if hover_changed {
                self.interaction_dirty = true;
            }

            // 派发 `on_hover`（先离开后进入）与 `on_pointer_move`（用于图表等悬停交互追踪，零堆分配）
            let mods = self.modifiers;
            let mut fired_any = false;
            for hit in hits.iter().rev() {
                let eff = hit.bounds.intersect(hit.clip);
                let was_in = prev_pos.is_some_and(|(ox, oy)| eff.contains(ox, oy));
                let is_in = eff.contains(px, py);
                if was_in && !is_in {
                    if let Some(on_hover) = hit.on_hover() {
                        let ev = DragEvent {
                            x: px,
                            y: py,
                            local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                            local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                            width: hit.bounds.w.max(1),
                            height: hit.bounds.h.max(1),
                            delta_x: dx,
                            delta_y: dy,
                        };
                        let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                        let mut acx = self
                            .make_action_scope(owner, (px, py), ev)
                            .with_modifiers(mods);
                        on_hover(false, &mut acx);
                        fired_any = true;
                    }
                }
            }
            for hit in hits.iter().rev() {
                let eff = hit.bounds.intersect(hit.clip);
                let was_in = prev_pos.is_some_and(|(ox, oy)| eff.contains(ox, oy));
                let is_in = eff.contains(px, py);
                if is_in {
                    let ev = DragEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_x: dx,
                        delta_y: dy,
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    if !was_in {
                        if let Some(on_hover) = hit.on_hover() {
                            let mut acx = self
                                .make_action_scope(owner, (px, py), ev)
                                .with_modifiers(mods);
                            on_hover(true, &mut acx);
                            fired_any = true;
                        }
                    }
                    if let Some(on_move) = hit.on_pointer_move() {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), ev)
                            .with_modifiers(mods);
                        on_move(&mut acx);
                        fired_any = true;
                    }
                    if hit.occlude {
                        break;
                    }
                }
            }
            if fired_any {
                self.flush_async_after_event();
                self.interaction_dirty = true;
                return true;
            }
            return false;
        }
        if dx == 0 && dy == 0 {
            return false;
        }

        let (sx, sy) = (self.press_start_x, self.press_start_y);
        let slop = self.gesture_config.drag_slop_px;
        if (px - sx).abs() > slop || (py - sy).abs() > slop {
            self.press_hold_ms = 0;
            self.repeat_accum_ms = 0;
            self.long_press_fired = true;
        }

        // 1. 优先路由给按下起点命中的 on_drag 组件（如 Slider）
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            let matches_pressed = self.has_pressed_hit && hit.id == Some(self.pressed_hit_id);
            if matches_pressed || effective.contains(sx, sy) {
                if let Some(on_drag) = hit.on_drag() {
                    let drag_ev = DragEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_x: dx,
                        delta_y: dy,
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let mods = self.modifiers;
                    {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), drag_ev)
                            .with_modifiers(mods);
                        on_drag(&mut acx);
                    }
                    self.flush_async_after_event();
                    self.interaction_dirty = true;
                    return true;
                }
                if hit.occlude && effective.contains(sx, sy) {
                    break;
                }
            }
        }

        // 2. 非触摸鼠标/手写笔默认禁止在空白可滚动容器上按住拖动页面（滚动走滚轮或显式 Scrollbar/on_drag）；
        //    触摸屏（PointerKind::Touch）或显式开启 `gesture_config.mouse_drag_scroll` 时执行跟手滚动并记录瞬时速度。
        if !self.allows_drag_scroll() {
            return false;
        }
        let mut scroll_to_apply: Option<((u32, i16), bool)> = None;
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            if effective.contains(sx, sy) {
                if let Some(target) = hit.scroll_target {
                    scroll_to_apply = Some((target, hit.scroll_horizontal));
                    break;
                }
                if hit.occlude {
                    break;
                }
            }
        }

        if let Some(((scroll_id, max_s), is_horiz)) = scroll_to_apply {
            let delta = if is_horiz { dx } else { dy };
            if delta != 0 {
                let delta_scroll = -delta;
                self.last_drag_vy = delta_scroll;
                self.motion.set_scroll_inertia(scroll_id, 0, max_s);
                let cur = self.motion.scroll_offset(scroll_id);
                let next = cur.saturating_add(delta_scroll).clamp(0, max_s.max(0));
                self.motion.set_scroll_offset(scroll_id, next);
                self.interaction_dirty = true;
                return true;
            }
        }
        false
    }

    /// 派发指针离开表面事件（清除悬停坐标，并在离开 `hover_bg` 热区时标记局部重绘）。
    #[inline(never)]
    pub fn dispatch_pointer_leave(&mut self) -> bool {
        let Some((prev_x, prev_y)) = self.interaction.pointer_pos.take() else {
            return false;
        };
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        let hits = self.cached_hit_regions();
        let was_hovered = hits.iter().any(|h| {
            h.has_hover_bg && h.bounds.intersect(h.clip).contains(prev_x, prev_y)
        });
        let mods = self.modifiers;
        let mut fired_leave = false;
        for hit in hits.iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(prev_x, prev_y) {
                if let Some(on_hover) = hit.on_hover() {
                    let ev = DragEvent::default();
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let mut acx = self
                        .make_action_scope(owner, (prev_x, prev_y), ev)
                        .with_modifiers(mods);
                    on_hover(false, &mut acx);
                    fired_leave = true;
                }
            }
        }
        if fired_leave {
            self.flush_async_after_event();
        }
        if was_hovered || fired_leave {
            self.interaction_dirty = true;
        }
        was_hovered || fired_leave
    }

    /// 派发指针/手指抬起事件（释放 `active_bg`、触发点击或启动惯性滑动衰减）。
    #[inline(never)]
    pub fn dispatch_pointer_up(&mut self, px: i16, py: i16) -> bool {
        self.ensure_hits_available();
        self.interaction.pointer_pos = Some((px, py));
        let was_pressed = self.interaction.is_pressed;
        let was_long_press_handled = self.long_press_handled;
        let expected_id = if self.has_pressed_hit {
            Some(self.pressed_hit_id)
        } else {
            None
        };
        self.has_pressed_hit = false;
        let was_drag_fired = self.drag_fired_on_down;
        self.drag_fired_on_down = false;
        self.interaction.is_pressed = false;
        self.press_hold_ms = 0;
        self.repeat_accum_ms = 0;
        self.long_press_fired = false;
        self.long_press_handled = false;
        self.interaction_dirty = true;

        if !was_pressed {
            return false;
        }

        let (sx, sy) = (self.press_start_x, self.press_start_y);
        let dx = px - sx;
        let dy = py - sy;
        let slop = self.gesture_config.drag_slop_px;
        let moved_far = dx.abs() > slop || dy.abs() > slop;

        if !moved_far {
            if was_long_press_handled {
                return true;
            }
            return self.trigger_click_internal(px, py, true, expected_id, !was_drag_fired);
        }

        // 若允许触摸/拖拽手势且滑动幅度达到 swipe_threshold_px 以上，检测 4 向滑动手势（SwipeDir）与移动端左边缘右滑返回
        if self.allows_drag_scroll() {
            let swipe_th = self.gesture_config.swipe_threshold_px;
            let edge_w = self.gesture_config.edge_back_width_px;
            if dx.abs() >= swipe_th || dy.abs() >= swipe_th {
                let dir = if dx.abs() >= dy.abs() {
                    if dx > 0 {
                        SwipeDir::Right
                    } else {
                        SwipeDir::Left
                    }
                } else if dy > 0 {
                    SwipeDir::Down
                } else {
                    SwipeDir::Up
                };
                if self.dispatch_swipe(sx, sy, dir) {
                    self.last_drag_vy = 0;
                    return true;
                }
                // 移动端左边缘右滑返回手势仅在真实触屏下触发，避免桌面鼠标边缘框选误返回
                if matches!(self.active_pointer_kind, PointerKind::Touch)
                    && sx <= self.viewport.x + edge_w
                    && dx >= edge_w
                    && dx.abs() > dy.abs() * 2
                {
                    if self.dispatch_navigate_back() {
                        self.last_drag_vy = 0;
                        return true;
                    }
                }
            }

            // 若刚刚进行了快速拖拽滑动，启动惯性滚动
            if self.last_drag_vy.abs() > 1 {
                let hits = self.cached_hit_regions();
                let mut scroll_target: Option<(u32, i16)> = None;
                for hit in hits.iter().rev() {
                    if hit.bounds.intersect(hit.clip).contains(sx, sy) {
                        if let Some(t) = hit.scroll_target {
                            scroll_target = Some(t);
                            break;
                        }
                        if hit.occlude {
                            break;
                        }
                    }
                }
                if let Some((scroll_id, max_s)) = scroll_target {
                    self.motion
                        .set_scroll_inertia(scroll_id, self.last_drag_vy, max_s);
                    self.last_drag_vy = 0;
                    return true;
                }
            }
        }
        self.last_drag_vy = 0;
        false
    }

    /// 派发指定坐标 `(x, y)` 处的 4 向滑动手势 [`SwipeDir`]。
    #[inline(never)]
    pub fn dispatch_swipe(&mut self, x: i16, y: i16, dir: SwipeDir) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        let hits = self.cached_hit_regions();
        let mut any_handled = false;
        for hit in hits.iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(x, y) {
                if let Some(cb) = hit.on_swipe() {
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let stopped = {
                        let mut action = self.make_action_scope(owner, (x, y), DragEvent::default());
                        cb(dir, &mut action);
                        action.is_propagation_stopped()
                    };
                    self.interaction_dirty = true;
                    any_handled = true;
                    if stopped {
                        break;
                    }
                }
                if hit.occlude {
                    break;
                }
            }
        }
        if any_handled {
            self.flush_async_after_event();
        }
        any_handled
    }


    #[inline(never)]
    pub(super) fn dispatch_scroll_axis_at(&mut self, px: i16, py: i16, delta: i16, prefer_horiz: bool) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        // 0. 优先派发自定义 `on_wheel` 回调（如交互画布 Zoom-to-Cursor），避免滚轮被外层页面滚动吞掉
        let hits = self.cached_hit_regions();
        let mods = self.modifiers;
        for hit in hits.iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(px, py) {
                if let Some(on_wheel) = hit.on_wheel() {
                    let ev = crate::pod::WheelEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_x: if prefer_horiz { delta } else { 0 },
                        delta_y: if prefer_horiz { 0 } else { delta },
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), DragEvent::default())
                            .with_modifiers(mods);
                        on_wheel(ev, &mut acx);
                    }
                    self.flush_async_after_event();
                    self.interaction_dirty = true;
                    return true;
                }
                if hit.occlude {
                    break;
                }
            }
        }
        // 支持嵌套滚动链（Scroll Chaining）：先尝试最内层匹配滚动容器，若已触顶/触底无法继续滚动则自动向上冒泡至外层容器
        let mut preferred: crate::arena::SmallVec<(u32, i16), 8> = crate::arena::SmallVec::new();
        let mut fallback: crate::arena::SmallVec<(u32, i16), 8> = crate::arena::SmallVec::new();
        for hit in self.cached_hit_regions().iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(px, py) {
                if let Some(t) = hit.scroll_target {
                    if hit.scroll_horizontal == prefer_horiz {
                        let _ = preferred.push(t);
                    } else if !prefer_horiz {
                        let _ = fallback.push(t);
                    }
                }
                if hit.occlude {
                    break;
                }
            }
        }
        let candidates = if !preferred.is_empty() {
            preferred.as_slice()
        } else {
            fallback.as_slice()
        };
        if candidates.is_empty() {
            return false;
        }
        for &(list_id, max_s) in candidates {
            self.motion.set_scroll_inertia(list_id, 0, max_s);
            let cur = self.motion.scroll_offset(list_id);
            let next = cur.saturating_add(delta).clamp(0, max_s.max(0));
            if next != cur {
                self.motion.set_scroll_offset(list_id, next);
                self.interaction_dirty = true;
                return true;
            }
        }
        // 即使均在边界未产生位移，仍返回 true 表示事件已被滚动容器体系消费
        true
    }

    /// 派发指定坐标 `(px, py)` 处的双指捏合缩放手势 [`crate::pod::PinchEvent`]。
    #[inline(never)]
    pub fn dispatch_pinch_at(&mut self, px: i16, py: i16, delta_scale_x1000: i16) -> bool {
        self.last_pinch_delta_x1000 = delta_scale_x1000;
        let Some(root_id) = self.root_pod else {
            self.interaction_dirty = true;
            return true;
        };
        self.ensure_hits_available();
        let hits = self.cached_hit_regions();
        let mods = self.modifiers;
        let mut handled = false;
        for hit in hits.iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(px, py) {
                if let Some(on_pinch) = hit.on_pinch() {
                    let ev = crate::pod::PinchEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_scale_x1000,
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let stopped = {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), DragEvent::default())
                            .with_modifiers(mods);
                        on_pinch(ev, &mut acx);
                        acx.is_propagation_stopped()
                    };
                    handled = true;
                    if stopped {
                        break;
                    }
                }
                if hit.occlude {
                    break;
                }
            }
        }
        if handled {
            self.flush_async_after_event();
        }
        self.interaction_dirty = true;
        true
    }

    /// 派发屏幕坐标 `(px, py)` 的点击事件。若命中某个可点击元素则执行其回调并返回 `true`。
    pub fn dispatch_click(&mut self, px: i16, py: i16) -> bool {
        self.interaction.pointer_pos = Some((px, py));
        let hit = self.trigger_click_internal(px, py, false, None, true);
        if hit {
            self.interaction_dirty = true;
        }
        hit
    }

    #[inline(never)]
    pub(super) fn trigger_click_internal(
        &mut self,
        px: i16,
        py: i16,
        require_pressed_match: bool,
        expected_id: Option<u32>,
        allow_drag_fallback: bool,
    ) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        self.interaction.focus_visible = false;
        let hits = self.cached_hit_regions();
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            if effective.contains(px, py) {
                if let Some(fidx) = hit.focus_index {
                    self.interaction.focused_index = Some(fidx);
                    self.interaction_dirty = true;
                }
                if let Some(on_click) = hit.on_click() {
                    if require_pressed_match && hit.id != expected_id {
                        return false;
                    }
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    {
                        let mut acx =
                            self.make_action_scope(owner, (px, py), DragEvent::default());
                        on_click(&mut acx);
                    }
                    self.flush_async_after_event();
                    return true;
                }
                if let Some(on_drag) = hit.on_drag() {
                    if require_pressed_match && hit.id != expected_id {
                        return false;
                    }
                    if !allow_drag_fallback {
                        return true;
                    }
                    let drag_ev = DragEvent {
                        x: px,
                        y: py,
                        local_x: (px - hit.bounds.x).clamp(0, hit.bounds.w),
                        local_y: (py - hit.bounds.y).clamp(0, hit.bounds.h),
                        width: hit.bounds.w.max(1),
                        height: hit.bounds.h.max(1),
                        delta_x: 0,
                        delta_y: 0,
                    };
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    let mods = self.modifiers;
                    {
                        let mut acx = self
                            .make_action_scope(owner, (px, py), drag_ev)
                            .with_modifiers(mods);
                        on_drag(&mut acx);
                    }
                    self.flush_async_after_event();
                    return true;
                }
                if hit.occlude {
                    return false;
                }
            }
        }
        false
    }

    /// 内部辅助：触发长按回调（`is_long_press == true`）或按住连发回调（`is_long_press == false`）。
    #[inline(never)]
    pub(super) fn trigger_hold_action(&mut self, px: i16, py: i16, is_long_press: bool) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        let hits = self.cached_hit_regions();
        for hit in hits.iter().rev() {
            let effective = hit.bounds.intersect(hit.clip);
            if effective.contains(px, py) {
                if is_long_press {
                    if let Some(on_long_press) = hit.on_long_press() {
                        let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                        {
                            let mut acx =
                                self.make_action_scope(owner, (px, py), DragEvent::default());
                            on_long_press(&mut acx);
                        }
                        self.flush_async_after_event();
                        self.interaction_dirty = true;
                        return true;
                    }
                } else if hit.auto_repeat {
                    if let Some(on_click) = hit.on_click() {
                        let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                        {
                            let mut acx =
                                self.make_action_scope(owner, (px, py), DragEvent::default());
                            on_click(&mut acx);
                        }
                        self.flush_async_after_event();
                        self.interaction_dirty = true;
                        return true;
                    }
                }
                if hit.occlude {
                    return false;
                }
            }
        }
        false
    }

    /// 更新指定列表容器 ID 的垂直滚动偏移量并标记下一帧执行 Pass-4 快径重拍平。
    pub fn scroll(&mut self, list_id: u32, delta_y: i16) {
        let cur = self.motion.scroll_offset(list_id);
        let next = cur.saturating_add(delta_y);
        if next != cur {
            self.motion.set_scroll_offset(list_id, next);
            self.interaction_dirty = true;
        }
    }
}
