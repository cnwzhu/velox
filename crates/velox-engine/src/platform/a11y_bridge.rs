//! 原生无障碍协议桥接层（`AccessKitBridge`）：将 [`crate::a11y`] 层次语义树与增量 Diff
//! 1:1 映射为 AccessKit / Windows UIA / macOS NSAccessibility / Linux AT-SPI 兼容更新包，
//! 并将屏幕阅读器下发的聚焦/点击/滚动/输入动作回灌至 [`VeloxEngine`]。

use std::string::String;
use std::vec::Vec;

use crate::a11y::{
    children_of, collect_a11y_nodes, diff_a11y_snapshot, snapshot_a11y_tree, A11yNodeSnapshot,
    A11yRole,
};
use crate::arena::SmallVec;
use crate::engine::VeloxEngine;
use crate::pod::{InputEvent, KeyCode};
use crate::solver::Region;

/// AccessKit 兼容节点更新载荷（无生命周期借用，可直接序列化或喂给 `accesskit::Node`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AccessKitNodeData {
    pub id: u32,
    pub role: A11yRole,
    pub bounds: Region,
    pub label: Option<String>,
    pub value: Option<String>,
    pub focusable: bool,
    pub focused: bool,
    pub disabled: bool,
    pub checked: Option<bool>,
    pub children: Vec<u32>,
}

/// 单帧增量无障碍树更新包（对应 `accesskit::TreeUpdate`，静息帧不生成任何 IPC 流量）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AccessKitTreeUpdate {
    pub root_id: u32,
    pub focus_id: u32,
    pub updated_nodes: Vec<AccessKitNodeData>,
    pub removed_ids: Vec<u32>,
}

impl AccessKitTreeUpdate {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.updated_nodes.is_empty() && self.removed_ids.is_empty()
    }
}

/// 来自操作系统屏幕阅读器 / 辅助技术的逆向操作请求（对应 `accesskit::ActionRequest`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AccessKitActionRequest {
    Focus { target_id: u32 },
    Click { target_id: u32 },
    Scroll { target_id: u32, delta_y: i16 },
    SetValue { target_id: u32, text: String },
}

/// 跨帧增量无障碍桥接器（维护上一帧快照基线，实现 0 冗余更新）。
pub struct AccessKitBridge {
    prev_snapshot: SmallVec<A11yNodeSnapshot, 256>,
    last_focus_id: u32,
    initialized: bool,
}

impl Default for AccessKitBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessKitBridge {
    pub fn new() -> Self {
        Self {
            prev_snapshot: SmallVec::new(),
            last_focus_id: 0,
            initialized: false,
        }
    }

    /// 采集当前 [`VeloxEngine`] 的无障碍树并与上一帧做增量 Diff；若无任何变化返回 `None`。
    pub fn sync_engine(&mut self, engine: &VeloxEngine) -> Option<AccessKitTreeUpdate> {
        let nodes = collect_a11y_nodes::<256>(
            engine.viewport_region(),
            engine.cached_hit_regions(),
            engine.cached_draw_ops(),
            engine.focused_index(),
        );
        let cur_snap = snapshot_a11y_tree::<256>(nodes.as_slice());
        let focus_id = nodes
            .as_slice()
            .iter()
            .find(|n| n.focused)
            .map(|n| n.id)
            .unwrap_or(0);

        if !self.initialized {
            self.initialized = true;
            self.prev_snapshot = cur_snap;
            self.last_focus_id = focus_id;
            let updated_nodes = nodes
                .as_slice()
                .iter()
                .map(|n| AccessKitNodeData {
                    id: n.id,
                    role: n.role,
                    bounds: n.bounds,
                    label: n.label.map(|s| String::from(s.as_str())),
                    value: n.value_text.map(|s| String::from(s.as_str())),
                    focusable: n.focusable,
                    focused: n.focused,
                    disabled: n.disabled,
                    checked: n.checked,
                    children: children_of(nodes.as_slice(), n.id).map(|c| c.id).collect(),
                })
                .collect();
            return Some(AccessKitTreeUpdate {
                root_id: 0,
                focus_id,
                updated_nodes,
                removed_ids: Vec::new(),
            });
        }

        let diff = diff_a11y_snapshot::<256>(self.prev_snapshot.as_slice(), nodes.as_slice());
        if diff.is_empty() && focus_id == self.last_focus_id {
            return None;
        }

        let updated_nodes: Vec<AccessKitNodeData> = diff
            .updated_nodes
            .as_slice()
            .iter()
            .map(|n| AccessKitNodeData {
                id: n.id,
                role: n.role,
                bounds: n.bounds,
                label: n.label.map(|s| String::from(s.as_str())),
                value: n.value_text.map(|s| String::from(s.as_str())),
                focusable: n.focusable,
                focused: n.focused,
                disabled: n.disabled,
                checked: n.checked,
                children: children_of(nodes.as_slice(), n.id).map(|c| c.id).collect(),
            })
            .collect();
        let removed_ids = diff.removed_ids.as_slice().to_vec();

        self.prev_snapshot = cur_snap;
        self.last_focus_id = focus_id;

        Some(AccessKitTreeUpdate {
            root_id: 0,
            focus_id,
            updated_nodes,
            removed_ids,
        })
    }

    /// 将操作系统屏幕阅读器发出的逆向操作请求翻译并派发给 [`VeloxEngine`]。
    pub fn apply_action_request(
        &self,
        engine: &mut VeloxEngine,
        req: AccessKitActionRequest,
    ) -> bool {
        let nodes = collect_a11y_nodes::<256>(
            engine.viewport_region(),
            engine.cached_hit_regions(),
            engine.cached_draw_ops(),
            engine.focused_index(),
        );
        match req {
            AccessKitActionRequest::Focus { target_id } => {
                if let Some(node) = nodes.as_slice().iter().find(|n| n.id == target_id) {
                    let cx = node.bounds.x.saturating_add(node.bounds.w >> 1);
                    let cy = node.bounds.y.saturating_add(node.bounds.h >> 1);
                    return engine.dispatch_pointer_down(cx, cy)
                        | engine.dispatch_pointer_up(cx, cy);
                }
                false
            }
            AccessKitActionRequest::Click { target_id } => {
                if let Some(node) = nodes.as_slice().iter().find(|n| n.id == target_id) {
                    let cx = node.bounds.x.saturating_add(node.bounds.w >> 1);
                    let cy = node.bounds.y.saturating_add(node.bounds.h >> 1);
                    return engine.dispatch_click(cx, cy);
                }
                false
            }
            AccessKitActionRequest::Scroll { target_id, delta_y } => {
                if let Some(node) = nodes.as_slice().iter().find(|n| n.id == target_id) {
                    let cx = node.bounds.x.saturating_add(node.bounds.w >> 1);
                    let cy = node.bounds.y.saturating_add(node.bounds.h >> 1);
                    return engine.dispatch_event(InputEvent::Scroll {
                        x: cx,
                        y: cy,
                        delta_y,
                    });
                }
                false
            }
            AccessKitActionRequest::SetValue { target_id, text } => {
                let mut handled = false;
                if let Some(node) = nodes.as_slice().iter().find(|n| n.id == target_id) {
                    let cx = node.bounds.x.saturating_add(node.bounds.w >> 1);
                    let cy = node.bounds.y.saturating_add(node.bounds.h >> 1);
                    handled |= engine.dispatch_click(cx, cy);
                    for ch in text.chars() {
                        handled |= engine.dispatch_key(KeyCode::Char(ch));
                    }
                }
                handled
            }
        }
    }
}
