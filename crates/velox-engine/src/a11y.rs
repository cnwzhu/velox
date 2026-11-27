//! 无障碍语义树（Accessibility Semantic Tree，兼容 AccessKit / AT-SPI / UI Automation 层次与增量 Diff 模型）。
//!
//! 核心特性：
//! - **100% `#![no_std]` 零堆分配**：从当前帧 `HitRegion` 与 `DrawOp` 自动推导带父子归属的层次无障碍树；
//! - **跨帧增量更新（AccessKit `TreeUpdate` 1:1 映射）**：通过 [`snapshot_a11y_tree`] 与 [`diff_a11y_snapshot`] /
//!   [`diff_a11y_trees`] 计算最小变更集（`updated_nodes`、`removed_ids`、`focus_id`），静息帧输出空更新（零屏幕阅读器 IPC 噪声）。

use crate::arena::{SmallVec, StrRef};
use crate::motion::{DrawOp, DrawPayload};
use crate::solver::{HitRegion, Region};

/// 无障碍语义角色分类（对应 AccessKit / WAI-ARIA / AT-SPI 核心 Role）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum A11yRole {
    /// 根视口或顶层窗口。
    Window,
    /// 通用分组容器。
    Container,
    /// 可点击按钮。
    Button,
    /// 单行或多行文本输入框。
    TextInput,
    /// 可拖拽滑块或数值调节器。
    Slider,
    /// 可滚动视口区域。
    ScrollRegion,
    /// 静态文本标签。
    Text,
    /// 图像或图标。
    Image,
}

/// 单个无障碍语义节点快照（带父节点归属与状态属性，兼容 AccessKit `Node` 模型）。
#[derive(Copy, Clone, Debug)]
pub struct A11yNode<'a> {
    /// 节点稳定无障碍 ID（`0` 固定保留为根 `Window`）。
    pub id: u32,
    /// 所属父级语义节点 ID（根 `Window` 为 `None`，其余节点均精确指向其最内层容器或根 `Some(0)`）。
    pub parent_id: Option<u32>,
    /// 节点语义角色。
    pub role: A11yRole,
    /// 屏幕边界矩形。
    pub bounds: Region,
    /// 可选的可读文本标签（Accessible Name，如按钮文字或静态文本）。
    pub label: Option<StrRef<'a>>,
    /// 可选的控件当前值文本（如 `TextInput` 的编辑内容或 `Slider` 数值）。
    pub value_text: Option<StrRef<'a>>,
    /// 是否可接收键盘/旋钮焦点。
    pub focusable: bool,
    /// 当前是否处于键盘/旋钮聚焦态。
    pub focused: bool,
    /// 节点当前是否处于禁用态。
    pub disabled: bool,
    /// 可选勾选/开关状态（`Some(true/false)` 对应复选框/开关/单选态）。
    pub checked: Option<bool>,
    /// 若为滚动容器，包含 `(scroll_id, max_scroll, horizontal)`。
    pub scroll_info: Option<(u32, i16, bool)>,
}

#[inline]
fn hash_opt_str(s: Option<StrRef<'_>>) -> u64 {
    let Some(sr) = s else {
        return 0;
    };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in sr.as_str().as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h | 1
}

impl<'a> A11yNode<'a> {
    /// 跨生命周期按内容比较两个无障碍节点是否完全一致（忽略底层 `ScratchArena` 指针地址差异）。
    #[inline]
    pub fn content_eq(&self, other: &A11yNode<'_>) -> bool {
        self.id == other.id
            && self.parent_id == other.parent_id
            && self.role == other.role
            && self.bounds == other.bounds
            && self.label.map(|s| s.as_str()) == other.label.map(|s| s.as_str())
            && self.value_text.map(|s| s.as_str()) == other.value_text.map(|s| s.as_str())
            && self.focusable == other.focusable
            && self.focused == other.focused
            && self.disabled == other.disabled
            && self.checked == other.checked
            && self.scroll_info == other.scroll_info
    }

    /// 转换为无生命周期依赖的跨帧指纹快照 [`A11yNodeSnapshot`]。
    #[inline]
    pub fn to_snapshot(&self) -> A11yNodeSnapshot {
        A11yNodeSnapshot {
            id: self.id,
            parent_id: self.parent_id,
            role: self.role,
            bounds: self.bounds,
            label_hash: hash_opt_str(self.label),
            value_hash: hash_opt_str(self.value_text),
            focusable: self.focusable,
            focused: self.focused,
            disabled: self.disabled,
            checked: self.checked,
            scroll_info: self.scroll_info,
        }
    }
}

impl PartialEq for A11yNode<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.content_eq(other)
    }
}

impl Eq for A11yNode<'_> {}

/// 无生命周期借用的跨帧无障碍节点指纹（`Copy`，可跨越 `ScratchArena::rewind()` 持久化存储以驱动增量 Diff）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct A11yNodeSnapshot {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub role: A11yRole,
    pub bounds: Region,
    pub label_hash: u64,
    pub value_hash: u64,
    pub focusable: bool,
    pub focused: bool,
    pub disabled: bool,
    pub checked: Option<bool>,
    pub scroll_info: Option<(u32, i16, bool)>,
}

/// 与 AccessKit `TreeUpdate` 1:1 映射的 `#![no_std]` 零堆增量无障碍树更新补丁。
#[derive(Debug)]
pub struct A11yTreeUpdate<'a, const CAP: usize> {
    /// 根节点 ID（固定为 `0` 对应 [`A11yRole::Window`]）。
    pub root_id: u32,
    /// 本帧当前拥有焦点的语义节点 ID（`None` 表示焦点停留在窗口根）。
    pub focus_id: Option<u32>,
    /// 本帧焦点是否相较上一帧发生转移。
    pub focus_changed: bool,
    /// 本帧新增或属性/位置/父关系发生变更的语义节点集合。
    pub updated_nodes: SmallVec<A11yNode<'a>, CAP>,
    /// 本帧从语义树中移除的旧节点 ID 列表。
    pub removed_ids: SmallVec<u32, CAP>,
}

impl<'a, const CAP: usize> A11yTreeUpdate<'a, CAP> {
    /// 当节点无任何新增、变更、删除且焦点未变化时返回 `true`（平台桥接层可直接跳过 IPC 推送）。
    #[inline]
    pub fn is_empty(&self) -> bool {
        !self.focus_changed && self.updated_nodes.is_empty() && self.removed_ids.is_empty()
    }
}

#[inline]
fn region_contains(outer: Region, inner: Region) -> bool {
    !outer.is_empty()
        && inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

#[inline]
fn region_area(r: Region) -> i32 {
    (r.w.max(0) as i32) * (r.h.max(0) as i32)
}

/// 在已收集的语义节点中选择包含 `bounds` 的最内层合法父节点 ID（若无更小容器则回退为根窗口 `0`）。
fn find_best_parent_id(nodes: &[A11yNode<'_>], bounds: Region, self_id: u32, allow_leaf_parents: bool) -> u32 {
    let mut best_id = 0u32;
    let mut best_area = i32::MAX;
    for n in nodes.iter().skip(1) {
        if n.id == self_id {
            continue;
        }
        let can_be_parent = match n.role {
            A11yRole::Window | A11yRole::Container | A11yRole::ScrollRegion => true,
            A11yRole::Button | A11yRole::TextInput | A11yRole::Slider => allow_leaf_parents,
            A11yRole::Text | A11yRole::Image => false,
        };
        if !can_be_parent {
            continue;
        }
        if region_contains(n.bounds, bounds) {
            let area = region_area(n.bounds);
            if area <= best_area {
                best_area = area;
                best_id = n.id;
            }
        }
    }
    best_id
}

/// 从当前帧缓存的 `HitRegion` 与 `DrawOp` 列表构建层次化无障碍语义树节点序列（含 `parent_id` 归属关系）。
pub fn collect_a11y_nodes<'a, const CAP: usize>(
    viewport: Region,
    hits: &[HitRegion<'a>],
    ops: &[DrawOp<'a>],
    focused_index: Option<u16>,
) -> SmallVec<A11yNode<'a>, CAP> {
    let mut out = SmallVec::new();
    let _ = out.push(A11yNode {
        id: 0,
        parent_id: None,
        role: A11yRole::Window,
        bounds: viewport,
        label: None,
        value_text: None,
        focusable: false,
        focused: false,
        disabled: false,
        checked: None,
        scroll_info: None,
    });

    for (idx, hit) in hits.iter().enumerate() {
        let has_key_input = hit
            .interact
            .and_then(|i| i.rare)
            .and_then(|r| r.on_key_input)
            .is_some();
        let has_drag = hit
            .interact
            .and_then(|i| i.rare)
            .and_then(|r| r.on_drag)
            .is_some();
        let has_click = hit.interact.and_then(|i| i.on_click).is_some();

        let role = if has_key_input {
            A11yRole::TextInput
        } else if has_drag {
            A11yRole::Slider
        } else if has_click {
            A11yRole::Button
        } else if hit.scroll_target.is_some() {
            A11yRole::ScrollRegion
        } else {
            A11yRole::Container
        };

        // 查找落在该交互区域内的第一段文本作为无障碍标签（Accessible Name）
        let label = ops.iter().find_map(|op| match op.payload {
            DrawPayload::TextRun { text, .. }
                if !op.bounds.intersect(hit.bounds).is_empty() =>
            {
                Some(text)
            }
            _ => None,
        });
        let value_text = if role == A11yRole::TextInput {
            label
        } else {
            None
        };

        let id = hit.id.unwrap_or((idx as u32) + 1);
        let parent_id = Some(find_best_parent_id(out.as_slice(), hit.bounds, id, false));
        let focusable = hit.focus_index.is_some();
        let focused = hit.focus_index.is_some() && hit.focus_index == focused_index;
        let scroll_info = hit
            .scroll_target
            .map(|(sid, max_s)| (sid, max_s, hit.scroll_horizontal));

        let _ = out.push(A11yNode {
            id,
            parent_id,
            role,
            bounds: hit.bounds,
            label,
            value_text,
            focusable,
            focused,
            disabled: false,
            checked: None,
            scroll_info,
        });
    }

    for op in ops.iter() {
        match op.payload {
            DrawPayload::TextRun { text, .. } => {
                let parent = find_best_parent_id(out.as_slice(), op.bounds, u32::MAX, true);
                let ordinal = out
                    .as_slice()
                    .iter()
                    .filter(|n| n.role == A11yRole::Text && n.parent_id == Some(parent))
                    .count() as u32;
                let id = op
                    .id
                    .unwrap_or_else(|| crate::pod::derive_scoped_id(parent, 0xA117_7E57, ordinal));
                let _ = out.push(A11yNode {
                    id,
                    parent_id: Some(parent),
                    role: A11yRole::Text,
                    bounds: op.bounds,
                    label: Some(text),
                    value_text: None,
                    focusable: false,
                    focused: false,
                    disabled: false,
                    checked: None,
                    scroll_info: None,
                });
            }
            DrawPayload::Sprite(_) => {
                let parent = find_best_parent_id(out.as_slice(), op.bounds, u32::MAX, true);
                let ordinal = out
                    .as_slice()
                    .iter()
                    .filter(|n| n.role == A11yRole::Image && n.parent_id == Some(parent))
                    .count() as u32;
                let id = op
                    .id
                    .unwrap_or_else(|| crate::pod::derive_scoped_id(parent, 0xA117_16A9, ordinal));
                let _ = out.push(A11yNode {
                    id,
                    parent_id: Some(parent),
                    role: A11yRole::Image,
                    bounds: op.bounds,
                    label: None,
                    value_text: None,
                    focusable: false,
                    focused: false,
                    disabled: false,
                    checked: None,
                    scroll_info: None,
                });
            }
            DrawPayload::RichText(rt) => {
                if let Some(first_span) = rt.spans.first() {
                    let parent = find_best_parent_id(out.as_slice(), op.bounds, u32::MAX, true);
                    let ordinal = out
                        .as_slice()
                        .iter()
                        .filter(|n| n.role == A11yRole::Text && n.parent_id == Some(parent))
                        .count() as u32;
                    let id = op.id.unwrap_or_else(|| {
                        crate::pod::derive_scoped_id(parent, 0xA117_7E88, ordinal)
                    });
                    let _ = out.push(A11yNode {
                        id,
                        parent_id: Some(parent),
                        role: A11yRole::Text,
                        bounds: op.bounds,
                        label: Some(first_span.text),
                        value_text: None,
                        focusable: false,
                        focused: false,
                        disabled: false,
                        checked: None,
                        scroll_info: None,
                    });
                }
            }
            DrawPayload::DecodedImage(_)
            | DrawPayload::Vector(_)
            | DrawPayload::SceneCanvas(_) => {
                let parent = find_best_parent_id(out.as_slice(), op.bounds, u32::MAX, true);
                let ordinal = out
                    .as_slice()
                    .iter()
                    .filter(|n| n.role == A11yRole::Image && n.parent_id == Some(parent))
                    .count() as u32;
                let id = op
                    .id
                    .unwrap_or_else(|| crate::pod::derive_scoped_id(parent, 0xA117_16AA, ordinal));
                let _ = out.push(A11yNode {
                    id,
                    parent_id: Some(parent),
                    role: A11yRole::Image,
                    bounds: op.bounds,
                    label: None,
                    value_text: None,
                    focusable: false,
                    focused: false,
                    disabled: false,
                    checked: None,
                    scroll_info: None,
                });
            }
            DrawPayload::Quad { .. }
            | DrawPayload::StyledQuad(_)
            | DrawPayload::PushLayer(_)
            | DrawPayload::PopLayer
            | DrawPayload::ExternalTexture(_) => {}
        }
    }

    out
}

/// 将一帧无障碍节点列表压缩为无生命周期借用的 [`A11yNodeSnapshot`] 列表，供跨帧增量 Diff 使用。
pub fn snapshot_a11y_tree<const CAP: usize>(
    nodes: &[A11yNode<'_>],
) -> SmallVec<A11yNodeSnapshot, CAP> {
    let mut out = SmallVec::new();
    for n in nodes {
        let _ = out.push(n.to_snapshot());
    }
    out
}

/// 迭代查询指定 `parent_id` 的全部直接子节点（便于 AccessKit 桥接层填充 `Node::children`）。
#[inline]
pub fn children_of<'a, 'b>(
    nodes: &'b [A11yNode<'a>],
    parent_id: u32,
) -> impl Iterator<Item = &'b A11yNode<'a>> {
    nodes.iter().filter(move |n| n.parent_id == Some(parent_id))
}

/// 对比上一帧无生命周期快照 `prev` 与当前帧 `curr`，生成最小增量 [`A11yTreeUpdate`]。
pub fn diff_a11y_snapshot<'a, const CAP: usize>(
    prev: &[A11yNodeSnapshot],
    curr: &[A11yNode<'a>],
) -> A11yTreeUpdate<'a, CAP> {
    let mut updated_nodes = SmallVec::new();
    let mut removed_ids = SmallVec::new();

    let prev_focus = prev.iter().find(|n| n.focused).map(|n| n.id);
    let curr_focus = curr.iter().find(|n| n.focused).map(|n| n.id);

    for c in curr {
        let snap = c.to_snapshot();
        let unchanged = prev.iter().any(|p| p.id == snap.id && *p == snap);
        if !unchanged {
            let _ = updated_nodes.push(*c);
        }
    }

    for p in prev {
        if !curr.iter().any(|c| c.id == p.id) {
            let _ = removed_ids.push(p.id);
        }
    }

    A11yTreeUpdate {
        root_id: 0,
        focus_id: curr_focus,
        focus_changed: prev_focus != curr_focus,
        updated_nodes,
        removed_ids,
    }
}

/// 直接对比两帧无障碍节点序列 `prev` 与 `curr`，生成最小增量 [`A11yTreeUpdate`]。
pub fn diff_a11y_trees<'a, const CAP: usize>(
    prev: &[A11yNode<'_>],
    curr: &[A11yNode<'a>],
) -> A11yTreeUpdate<'a, CAP> {
    let mut updated_nodes = SmallVec::new();
    let mut removed_ids = SmallVec::new();

    let prev_focus = prev.iter().find(|n| n.focused).map(|n| n.id);
    let curr_focus = curr.iter().find(|n| n.focused).map(|n| n.id);

    for c in curr {
        let unchanged = prev.iter().any(|p| p.id == c.id && c.content_eq(p));
        if !unchanged {
            let _ = updated_nodes.push(*c);
        }
    }

    for p in prev {
        if !curr.iter().any(|c| c.id == p.id) {
            let _ = removed_ids.push(p.id);
        }
    }

    A11yTreeUpdate {
        root_id: 0,
        focus_id: curr_focus,
        focus_changed: prev_focus != curr_focus,
        updated_nodes,
        removed_ids,
    }
}
