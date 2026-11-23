//! 统一事件入口 `dispatch_event`、原生 IME 组合输入状态机、系统返回与键盘/旋钮焦点导航。

use crate::pod::{DragEvent, ImePreedit, InputEvent, KeyCode, SwipeDir};
use crate::solver::Region;

use super::VeloxEngine;

impl VeloxEngine {
    /// 统一派发输入事件（支持 `PointerDown` / `PointerMove` / `PointerUp` / `Scroll` / `KeyDown` / `KeyUp` / `ModifiersChanged`）。
    #[inline(never)]
    pub fn dispatch_event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::PointerDown { x, y } => self.dispatch_pointer_down(x, y),
            InputEvent::PointerMove { x, y } => self.dispatch_pointer_move(x, y),
            InputEvent::PointerUp { x, y } => self.dispatch_pointer_up(x, y),
            InputEvent::PointerSecondaryDown { x, y } => {
                self.dispatch_pointer_secondary_down(x, y)
            }
            InputEvent::PointerSecondaryUp { .. } => false,
            InputEvent::PointerLeave => self.dispatch_pointer_leave(),
            InputEvent::PointerCancel => {
                self.interaction.is_pressed = false;
                self.has_pressed_hit = false;
                self.drag_fired_on_down = false;
                self.press_hold_ms = 0;
                self.repeat_accum_ms = 0;
                self.long_press_fired = false;
                self.long_press_handled = false;
                self.last_drag_vy = 0;
                self.interaction_dirty = true;
                true
            }
            InputEvent::PointerKindChanged(kind) => {
                self.active_pointer_kind = kind;
                false
            }
            InputEvent::Scroll { x, y, delta_y } => self.dispatch_scroll_axis_at(x, y, delta_y, false),
            InputEvent::ScrollX { x, y, delta_x } => self.dispatch_scroll_axis_at(x, y, delta_x, true),
            InputEvent::KeyDown(key) => self.dispatch_key(key),
            InputEvent::KeyUp(_) => false,
            InputEvent::ModifiersChanged(mods) => {
                self.modifiers = mods;
                false
            }
            InputEvent::Backspace => self.dispatch_key(KeyCode::Backspace),
            InputEvent::Delete => self.dispatch_key(KeyCode::Delete),
            InputEvent::TextCommit(ch) => self.dispatch_key(KeyCode::Char(ch)),
            InputEvent::ImeEnabled => {
                self.ime_enabled = true;
                false
            }
            InputEvent::ImeDisabled => {
                self.ime_enabled = false;
                if !self.ime_preedit.is_empty() {
                    self.ime_preedit = ImePreedit::empty();
                    self.frame_scope.set_ime_preedit(ImePreedit::empty());
                    self.mark_focused_or_root_dirty();
                    true
                } else {
                    false
                }
            }
            InputEvent::ImePreedit(preedit) => self.apply_ime_preedit(preedit),
            InputEvent::NavigateBack => self.dispatch_navigate_back(),
            InputEvent::SafeAreaChanged(insets) => {
                self.set_safe_area(insets);
                true
            }
            InputEvent::KeyboardInsetChanged(height) => {
                self.set_keyboard_inset(height);
                true
            }
        }
    }

    fn mark_focused_or_root_dirty(&mut self) {
        self.interaction_dirty = true;
        if let Some(root_id) = self.root_pod {
            let focused_idx = self.interaction.focused_index;
            let owner = self
                .cached_hit_regions()
                .iter()
                .rev()
                .find(|h| focused_idx.is_some() && h.focus_index == focused_idx)
                .map(|h| Self::resolve_hit_pod(root_id, h.owner_pod))
                .unwrap_or(root_id);
            self.pool.mark_dirty(owner);
        }
    }

    fn apply_ime_preedit(&mut self, preedit: ImePreedit) -> bool {
        if self.ime_preedit == preedit {
            return false;
        }
        self.ime_preedit = preedit;
        self.frame_scope.set_ime_preedit(preedit);
        self.mark_focused_or_root_dirty();
        true
    }

    /// 返回当前正在进行的 IME 预编辑（组合输入）字符串；未组合时返回空串 `""`。
    #[inline]
    pub fn ime_preedit(&self) -> &str {
        self.ime_preedit.as_str()
    }

    /// 返回当前完整的 IME 预编辑状态（含字节级光标选区区间）。
    #[inline(always)]
    pub const fn ime_preedit_state(&self) -> ImePreedit {
        self.ime_preedit
    }

    /// 返回系统 IME 会话当前是否处于激活态。
    #[inline(always)]
    pub const fn ime_enabled(&self) -> bool {
        self.ime_enabled
    }

    /// 派发系统 IME 组合输入预编辑更新（如拼音 `"zhongwen"` 与候选光标区间）。
    #[inline]
    pub fn dispatch_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        let preedit = ImePreedit::from_str(text, cursor);
        self.apply_ime_preedit(preedit)
    }

    /// 派发系统 IME 最终提交的 UTF-8 文本（如 `"中文"`）：自动清空预编辑状态并逐字符送入当前聚焦输入控件。
    pub fn dispatch_ime_commit(&mut self, text: &str) -> bool {
        let mut handled = false;
        if !self.ime_preedit.is_empty() {
            self.ime_preedit = ImePreedit::empty();
            self.frame_scope.set_ime_preedit(ImePreedit::empty());
            self.mark_focused_or_root_dirty();
            handled = true;
        }
        for ch in text.chars() {
            if self.dispatch_key(KeyCode::Char(ch)) {
                handled = true;
            }
        }
        handled
    }

    /// 返回当前拥有焦点的可交互/文本输入区域（逻辑像素坐标），供桌面端 `winit::Window::set_ime_cursor_area`
    /// 将系统输入法候选框精确对齐至输入框位置。
    pub fn focused_bounds(&self) -> Option<Region> {
        let hits = self.cached_hit_regions();
        if let Some(fidx) = self.interaction.focused_index {
            if let Some(hit) = hits.iter().rev().find(|h| h.focus_index == Some(fidx)) {
                return Some(hit.bounds);
            }
        }
        // 回退：若存在显式注册了文本键盘回调的活跃输入区，也提供其边界供 IME 锚定
        hits.iter()
            .rev()
            .find(|h| h.on_key_input().is_some())
            .map(|h| h.bounds)
    }

    /// 判断当前焦点节点（或活跃热区）是否接受键盘文本输入。
    pub fn has_focused_text_input(&self) -> bool {
        let hits = self.cached_hit_regions();
        let focused_idx = self.interaction.focused_index;
        hits.iter().rev().any(|h| {
            h.on_key_input().is_some()
                && (focused_idx.is_none() || h.focus_index == focused_idx)
        })
    }

    /// 派发系统级返回事件（Android 返回键 / iOS 左边缘右滑返回 / 浏览器后退）：
    /// 优先调用当前活动树中注册了 `.on_navigate_back(...)` 的处理器；若无，则回退到屏幕中心右滑返回手势（`SwipeDir::Right`）。
    #[inline(never)]
    pub fn dispatch_navigate_back(&mut self) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        let hits = self.cached_hit_regions();
        for hit in hits.iter().rev() {
            if let Some(cb) = hit.on_navigate_back() {
                let cx = hit.bounds.x + hit.bounds.w / 2;
                let cy = hit.bounds.y + hit.bounds.h / 2;
                let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                {
                    let mut acx = self.make_action_scope(owner, (cx, cy), DragEvent::default());
                    cb(&mut acx);
                }
                self.flush_async_after_event();
                self.interaction_dirty = true;
                return true;
            }
        }
        let cx = self.viewport.x + self.viewport.w / 2;
        let cy = self.viewport.y + self.viewport.h / 2;
        self.dispatch_swipe(cx, cy, SwipeDir::Right)
    }


    /// 派发物理按键、字符输入或旋转编码器按键事件（支持无触摸屏 MCU 的焦点切换、`Enter` 触发与聚焦输入框文本编辑）。
    #[inline(never)]
    pub fn dispatch_key(&mut self, key: KeyCode) -> bool {
        let Some(root_id) = self.root_pod else {
            return false;
        };
        self.ensure_hits_available();
        let hits = self.cached_hit_regions();

        // 0. 顶层浮层 / 模态框的 Escape 关闭通道：优先逆序派发给注册了 `on_navigate_back` 的浮层或焦点节点
        if matches!(key, KeyCode::Escape) {
            for hit in hits.iter().rev() {
                if let Some(cb) = hit.on_navigate_back() {
                    let cx_px = hit.bounds.x + (hit.bounds.w >> 1);
                    let cy_py = hit.bounds.y + (hit.bounds.h >> 1);
                    let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                    {
                        let mut acx =
                            self.make_action_scope(owner, (cx_px, cy_py), DragEvent::default());
                        cb(&mut acx);
                    }
                    self.flush_async_after_event();
                    self.interaction_dirty = true;
                    return true;
                }
            }
        }

        // 0.5 非文本控件局部键盘导航（Slider / Tabs / RadioGroup / 展开的 Select）：
        // 仅拦截方向键 / Home / End / Enter / Space，绝不吞掉 Tab / BackTab 焦点切换！
        let is_nav_key = matches!(
            key,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Enter
                | KeyCode::Space
        );
        if is_nav_key {
            let focused_idx = self.interaction.focused_index;
            // 优先派发给当前显式聚焦且注册了 `on_key_nav` 的控件
            if let Some(fidx) = focused_idx {
                for hit in hits.iter().rev() {
                    if hit.focus_index == Some(fidx) {
                        if let Some(on_nav) = hit.on_key_nav() {
                            let cx_px = hit.bounds.x + (hit.bounds.w >> 1);
                            let cy_py = hit.bounds.y + (hit.bounds.h >> 1);
                            let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                            {
                                let mut acx = self.make_action_scope(
                                    owner,
                                    (cx_px, cy_py),
                                    DragEvent::default(),
                                );
                                on_nav(key, &mut acx);
                            }
                            self.flush_async_after_event();
                            self.interaction_dirty = true;
                            return true;
                        }
                    }
                }
            }
            // 次优先：顶层打开的弹出菜单（如展开态 Select / Dropdown）即使未显式 Tab 聚焦也支持方向键选择
            for hit in hits.iter().rev() {
                if hit.occlude {
                    if let Some(on_nav) = hit.on_key_nav() {
                        let cx_px = hit.bounds.x + (hit.bounds.w >> 1);
                        let cy_py = hit.bounds.y + (hit.bounds.h >> 1);
                        let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                        {
                            let mut acx =
                                self.make_action_scope(owner, (cx_px, cy_py), DragEvent::default());
                            on_nav(key, &mut acx);
                        }
                        self.flush_async_after_event();
                        self.interaction_dirty = true;
                        return true;
                    }
                    break;
                }
            }
        }

        // 1. 优先检查当前焦点元素是否注册了 `on_key_input`（如聚焦中的 TextInput / CodeEditor 接收编辑与导航键）
        let is_edit_key = matches!(
            key,
            KeyCode::Char(_)
                | KeyCode::Space
                | KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::Enter
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Escape
        );
        if is_edit_key {
            let focused_idx = self.interaction.focused_index;
            // Tab / BackTab / Space / Escape 仅在当前显式聚焦文本控件时由其消费，绝不吞掉未聚焦时的全局导航/按钮激活/退出通道
            let require_explicit_focus = matches!(
                key,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Space | KeyCode::Escape
            );
            let cur_mods = self.modifiers;
            for hit in hits.iter().rev() {
                let matches_focus = if require_explicit_focus {
                    focused_idx.is_some() && hit.focus_index == focused_idx
                } else {
                    focused_idx.is_none() || hit.focus_index == focused_idx
                };
                if matches_focus {
                    if let Some(on_key_in) = hit.on_key_input() {
                        let cx_px = hit.bounds.x + (hit.bounds.w >> 1);
                        let cy_py = hit.bounds.y + (hit.bounds.h >> 1);
                        let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                        {
                            let mut acx = self
                                .make_action_scope(owner, (cx_px, cy_py), DragEvent::default())
                                .with_modifiers(cur_mods);
                            on_key_in(key, &mut acx);
                        }
                        self.flush_async_after_event();
                        self.interaction_dirty = true;
                        return true;
                    }
                }
            }
            if matches!(
                key,
                KeyCode::Char(_)
                    | KeyCode::Backspace
                    | KeyCode::Delete
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ) {
                return false;
            }
        }

        let explicit_max = hits
            .iter()
            .filter_map(|h| h.focus_index)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        let clickable_count = hits.iter().filter(|h| h.on_click().is_some()).count() as u16;
        let max_focus = if explicit_max > 0 {
            explicit_max
        } else {
            clickable_count
        };

        match key {
            KeyCode::Tab | KeyCode::Down | KeyCode::Right => {
                if max_focus == 0 {
                    return false;
                }
                let next = match self.interaction.focused_index {
                    Some(cur) => (cur + 1) % max_focus,
                    None => 0,
                };
                self.interaction.focused_index = Some(next);
                self.interaction.focus_visible = true;
                self.interaction_dirty = true;
                return true;
            }
            KeyCode::BackTab | KeyCode::Up | KeyCode::Left => {
                if max_focus == 0 {
                    return false;
                }
                let prev = match self.interaction.focused_index {
                    Some(cur) => (cur + max_focus - 1) % max_focus,
                    None => max_focus - 1,
                };
                self.interaction.focused_index = Some(prev);
                self.interaction.focus_visible = true;
                self.interaction_dirty = true;
                return true;
            }
            KeyCode::Enter | KeyCode::Space => {
                if max_focus == 0 {
                    return false;
                }
                let fidx = match self.interaction.focused_index {
                    Some(idx) => idx,
                    None => {
                        self.interaction.focused_index = Some(0);
                        self.interaction.focus_visible = true;
                        self.interaction_dirty = true;
                        0
                    }
                };
                if explicit_max > 0 {
                    for hit in hits.iter().rev() {
                        if hit.focus_index == Some(fidx) {
                            if let Some(on_click) = hit.on_click() {
                                let cx_px = hit.bounds.x + (hit.bounds.w >> 1);
                                let cy_py = hit.bounds.y + (hit.bounds.h >> 1);
                                let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                                {
                                    let mut acx = self.make_action_scope(
                                        owner,
                                        (cx_px, cy_py),
                                        DragEvent::default(),
                                    );
                                    on_click(&mut acx);
                                }
                                self.flush_async_after_event();
                                self.interaction_dirty = true;
                                return true;
                            }
                        }
                    }
                } else if let Some(hit) = hits
                    .iter()
                    .filter(|h| h.on_click().is_some())
                    .nth(fidx as usize)
                {
                    if let Some(on_click) = hit.on_click() {
                        let cx_px = hit.bounds.x + (hit.bounds.w >> 1);
                        let cy_py = hit.bounds.y + (hit.bounds.h >> 1);
                        let owner = Self::resolve_hit_pod(root_id, hit.owner_pod);
                        {
                            let mut acx =
                                self.make_action_scope(owner, (cx_px, cy_py), DragEvent::default());
                            on_click(&mut acx);
                        }
                        self.flush_async_after_event();
                        self.interaction_dirty = true;
                        return true;
                    }
                }
            }
            KeyCode::Escape => {
                if self.interaction.focused_index.is_some() || self.interaction.focus_visible {
                    self.interaction.focused_index = None;
                    self.interaction.focus_visible = false;
                    self.interaction_dirty = true;
                    return true;
                }
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => {}
        }
        false
    }

    /// 派发键盘按键抬起事件。
    pub fn dispatch_key_up(&mut self, _key: KeyCode) -> bool {
        false
    }

}
