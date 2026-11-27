//! 无限画布节点编排引擎（`NodeGraphStore` / `NodeGraphView`）：
//! 面向 AI Agent 工作流编排、可视化蓝图与低代码 DAG 编辑器。
//!
//! 核心能力：
//! - **端口吸附连线与三次贝塞尔曲线**：平滑水平切线贝塞尔连线、实时流光脉冲点、精确曲线距离拾取；
//! - **拓扑层次自动布局（`auto_layout_layered`）**：按 Kahn 拓扑分层自动排布 DAG 节点；
//! - **命令历史栈（`Undo` / `Redo`）**：移动节点、连线、删线、增删节点全部支持无限次撤销/重做；
//! - **多模态无限画布相机**：直接复用 [`ViewportController`]（Zoom-to-Cursor、拖拽平移/节点拖动）。

use std::collections::{HashMap, VecDeque};

use velox_kit::canvas::ViewportController;
use velox_kit::core::canvas_scene::Viewport2D;
use velox_kit::prelude::*;

/// 节点唯一标识。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GraphNodeId(pub u32);

/// 端口唯一标识。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GraphPortId(pub u32);

/// 端口方向（输入 / 输出）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PortDir {
    Input,
    Output,
}

/// 节点端口定义。
#[derive(Clone, Debug)]
pub struct GraphPort {
    pub id: GraphPortId,
    pub dir: PortDir,
    pub label: String,
    pub color: Color,
}

/// 无限画布中的单个功能节点。
#[derive(Clone, Debug)]
pub struct GraphNode {
    pub id: GraphNodeId,
    pub title: String,
    pub category: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub accent: Color,
    pub selected: bool,
    pub collapsed: bool,
    pub ports: Vec<GraphPort>,
}

impl GraphNode {
    pub fn new(id: u32, title: impl Into<String>, x: f32, y: f32) -> Self {
        Self {
            id: GraphNodeId(id),
            title: title.into(),
            category: String::from("Node"),
            x,
            y,
            width: 176.0,
            height: 92.0,
            accent: Color::rgb(59, 130, 246),
            selected: false,
            collapsed: false,
            ports: Vec::new(),
        }
    }

    pub fn category(mut self, cat: impl Into<String>) -> Self {
        self.category = cat.into();
        self
    }

    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.width = w.max(96.0);
        self.height = h.max(48.0);
        self
    }

    pub fn accent(mut self, color: Color) -> Self {
        self.accent = color;
        self
    }

    pub fn with_port(
        mut self,
        port_id: u32,
        dir: PortDir,
        label: impl Into<String>,
        color: Color,
    ) -> Self {
        self.ports.push(GraphPort {
            id: GraphPortId(port_id),
            dir,
            label: label.into(),
            color,
        });
        self
    }

    /// 计算节点内指定端口的世界坐标 `(wx, wy)`。
    pub fn port_world_pos(&self, port_id: GraphPortId) -> Option<(f32, f32)> {
        let port = self.ports.iter().find(|p| p.id == port_id)?;
        let same_dir: Vec<GraphPortId> = self
            .ports
            .iter()
            .filter(|p| p.dir == port.dir)
            .map(|p| p.id)
            .collect();
        let rank = same_dir.iter().position(|&id| id == port_id).unwrap_or(0);
        let wx = match port.dir {
            PortDir::Input => self.x,
            PortDir::Output => self.x + self.width,
        };
        let header_h = 28.0;
        let wy = if self.collapsed {
            self.y + header_h * 0.5
        } else {
            self.y + header_h + 18.0 + rank as f32 * 22.0
        };
        Some((wx, wy))
    }

    #[inline]
    pub fn visual_height(&self) -> f32 {
        if self.collapsed {
            30.0
        } else {
            let in_n = self.ports.iter().filter(|p| p.dir == PortDir::Input).count();
            let out_n = self.ports.iter().filter(|p| p.dir == PortDir::Output).count();
            let rows = in_n.max(out_n).max(1) as f32;
            (32.0 + rows * 22.0 + 12.0).max(self.height)
        }
    }
}

/// 节点间有向连线（输出端口 → 输入端口）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GraphEdge {
    pub id: u32,
    pub from_node: GraphNodeId,
    pub from_port: GraphPortId,
    pub to_node: GraphNodeId,
    pub to_port: GraphPortId,
}

/// 支持 Undo/Redo 的原子编辑命令。
#[derive(Clone, Debug)]
pub enum NodeGraphCommand {
    MoveNode {
        id: GraphNodeId,
        old_pos: (f32, f32),
        new_pos: (f32, f32),
    },
    AddEdge(GraphEdge),
    RemoveEdge(GraphEdge),
    ToggleCollapse(GraphNodeId),
}

/// 计算水平切线三次贝塞尔曲线上参数 `t ∈ [0, 1]` 的点。
#[inline]
pub fn eval_cubic_bezier(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    let uuu = uu * u;
    let ttt = tt * t;
    let x = uuu * p0.0 + 3.0 * uu * t * p1.0 + 3.0 * u * tt * p2.0 + ttt * p3.0;
    let y = uuu * p0.1 + 3.0 * uu * t * p1.1 + 3.0 * u * tt * p2.1 + ttt * p3.1;
    (x, y)
}

/// 生成一条从 `src` 到 `dst` 的标准流程图三次贝塞尔控制点 `(p0, p1, p2, p3)`。
#[inline]
pub fn edge_bezier_controls(src: (f32, f32), dst: (f32, f32)) -> ((f32, f32), (f32, f32), (f32, f32), (f32, f32)) {
    let dx = (dst.0 - src.0).abs().max(60.0) * 0.48;
    (src, (src.0 + dx, src.1), (dst.0 - dx, dst.1), dst)
}

/// 无限画布节点图状态与编辑存储（含拓扑布局与 Undo/Redo 历史栈）。
pub struct NodeGraphStore {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub hovered_node: Option<GraphNodeId>,
    pub hovered_edge: Option<u32>,
    pub selected_edge: Option<u32>,
    pub dragging_node: Option<GraphNodeId>,
    pub drag_origin_pos: Option<(f32, f32)>,
    pub connecting_from: Option<(GraphNodeId, GraphPortId)>,
    pub cursor_world: (f32, f32),
    next_node_id: u32,
    next_port_id: u32,
    next_edge_id: u32,
    undo_stack: Vec<NodeGraphCommand>,
    redo_stack: Vec<NodeGraphCommand>,
}

impl Default for NodeGraphStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeGraphStore {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            hovered_node: None,
            hovered_edge: None,
            selected_edge: None,
            dragging_node: None,
            drag_origin_pos: None,
            connecting_from: None,
            cursor_world: (0.0, 0.0),
            next_node_id: 10,
            next_port_id: 100,
            next_edge_id: 1,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// 构建一个开箱即用的 AI Agent 工作流编排示例图。
    pub fn demo_ai_workflow() -> Self {
        let mut g = Self::new();
        g.add_node(
            GraphNode::new(1, "User Prompt", 40.0, 120.0)
                .category("Trigger")
                .accent(Color::rgb(59, 130, 246))
                .with_port(11, PortDir::Output, "query", Color::rgb(96, 165, 250)),
        );
        g.add_node(
            GraphNode::new(2, "Vector RAG", 280.0, 48.0)
                .category("Retriever")
                .accent(Color::rgb(16, 185, 129))
                .with_port(21, PortDir::Input, "query", Color::rgb(96, 165, 250))
                .with_port(22, PortDir::Output, "docs", Color::rgb(52, 211, 153)),
        );
        g.add_node(
            GraphNode::new(3, "MCP Tool Router", 280.0, 198.0)
                .category("Agent")
                .accent(Color::rgb(168, 85, 247))
                .with_port(31, PortDir::Input, "intent", Color::rgb(96, 165, 250))
                .with_port(32, PortDir::Output, "tool_out", Color::rgb(192, 132, 252)),
        );
        g.add_node(
            GraphNode::new(4, "LLM Synthesizer", 540.0, 110.0)
                .category("Model")
                .accent(Color::rgb(245, 158, 11))
                .with_port(41, PortDir::Input, "context", Color::rgb(52, 211, 153))
                .with_port(42, PortDir::Input, "tools", Color::rgb(192, 132, 252))
                .with_port(43, PortDir::Output, "stream", Color::rgb(251, 191, 36)),
        );
        g.add_node(
            GraphNode::new(5, "Velox Stream UI", 790.0, 118.0)
                .category("Output")
                .accent(Color::rgb(236, 72, 153))
                .with_port(51, PortDir::Input, "tokens", Color::rgb(251, 191, 36)),
        );
        let _ = g.connect(GraphNodeId(1), GraphPortId(11), GraphNodeId(2), GraphPortId(21));
        let _ = g.connect(GraphNodeId(1), GraphPortId(11), GraphNodeId(3), GraphPortId(31));
        let _ = g.connect(GraphNodeId(2), GraphPortId(22), GraphNodeId(4), GraphPortId(41));
        let _ = g.connect(GraphNodeId(3), GraphPortId(32), GraphNodeId(4), GraphPortId(42));
        let _ = g.connect(GraphNodeId(4), GraphPortId(43), GraphNodeId(5), GraphPortId(51));
        g.undo_stack.clear();
        g.redo_stack.clear();
        g
    }

    pub fn add_node(&mut self, node: GraphNode) {
        self.nodes.push(node);
    }

    pub fn node(&self, id: GraphNodeId) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: GraphNodeId) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// 连接两个端口（自动校验方向兼容性并防自环/重复边，记录至 Undo 栈）。
    pub fn connect(
        &mut self,
        from_node: GraphNodeId,
        from_port: GraphPortId,
        to_node: GraphNodeId,
        to_port: GraphPortId,
    ) -> Option<u32> {
        if from_node == to_node {
            return None;
        }
        let from_ok = self
            .node(from_node)
            .and_then(|n| n.ports.iter().find(|p| p.id == from_port))
            .is_some_and(|p| p.dir == PortDir::Output);
        let to_ok = self
            .node(to_node)
            .and_then(|n| n.ports.iter().find(|p| p.id == to_port))
            .is_some_and(|p| p.dir == PortDir::Input);
        if !from_ok || !to_ok {
            return None;
        }
        if self.edges.iter().any(|e| {
            e.from_node == from_node
                && e.from_port == from_port
                && e.to_node == to_node
                && e.to_port == to_port
        }) {
            return None;
        }
        let edge = GraphEdge {
            id: self.next_edge_id,
            from_node,
            from_port,
            to_node,
            to_port,
        };
        self.next_edge_id = self.next_edge_id.saturating_add(1);
        self.edges.push(edge);
        self.undo_stack.push(NodeGraphCommand::AddEdge(edge));
        self.redo_stack.clear();
        Some(edge.id)
    }

    /// 删除指定连线（记录至 Undo 栈）。
    pub fn disconnect(&mut self, edge_id: u32) -> bool {
        if let Some(pos) = self.edges.iter().position(|e| e.id == edge_id) {
            let removed = self.edges.remove(pos);
            self.undo_stack.push(NodeGraphCommand::RemoveEdge(removed));
            self.redo_stack.clear();
            true
        } else {
            false
        }
    }

    /// 移动节点位置并记录 Undo 命令。
    pub fn move_node(&mut self, id: GraphNodeId, new_x: f32, new_y: f32) -> bool {
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        let old_pos = (node.x, node.y);
        if (old_pos.0 - new_x).abs() < 0.01 && (old_pos.1 - new_y).abs() < 0.01 {
            return false;
        }
        node.x = new_x;
        node.y = new_y;
        self.undo_stack.push(NodeGraphCommand::MoveNode {
            id,
            old_pos,
            new_pos: (new_x, new_y),
        });
        self.redo_stack.clear();
        true
    }

    /// 增量平移节点（供实时拖拽，不产生碎片化历史条目）。
    pub fn translate_node_live(&mut self, id: GraphNodeId, dx: f32, dy: f32) {
        if let Some(node) = self.node_mut(id) {
            node.x += dx;
            node.y += dy;
        }
    }

    /// 折叠/展开节点并记录 Undo 命令。
    pub fn toggle_collapse(&mut self, id: GraphNodeId) -> bool {
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        node.collapsed = !node.collapsed;
        self.undo_stack.push(NodeGraphCommand::ToggleCollapse(id));
        self.redo_stack.clear();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(cmd) = self.undo_stack.pop() else {
            return false;
        };
        match &cmd {
            NodeGraphCommand::MoveNode { id, old_pos, .. } => {
                if let Some(n) = self.node_mut(*id) {
                    n.x = old_pos.0;
                    n.y = old_pos.1;
                }
            }
            NodeGraphCommand::AddEdge(e) => {
                self.edges.retain(|x| x.id != e.id);
            }
            NodeGraphCommand::RemoveEdge(e) => {
                self.edges.push(*e);
            }
            NodeGraphCommand::ToggleCollapse(id) => {
                if let Some(n) = self.node_mut(*id) {
                    n.collapsed = !n.collapsed;
                }
            }
        }
        self.redo_stack.push(cmd);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(cmd) = self.redo_stack.pop() else {
            return false;
        };
        match &cmd {
            NodeGraphCommand::MoveNode { id, new_pos, .. } => {
                if let Some(n) = self.node_mut(*id) {
                    n.x = new_pos.0;
                    n.y = new_pos.1;
                }
            }
            NodeGraphCommand::AddEdge(e) => {
                self.edges.push(*e);
            }
            NodeGraphCommand::RemoveEdge(e) => {
                self.edges.retain(|x| x.id != e.id);
            }
            NodeGraphCommand::ToggleCollapse(id) => {
                if let Some(n) = self.node_mut(*id) {
                    n.collapsed = !n.collapsed;
                }
            }
        }
        self.undo_stack.push(cmd);
        true
    }

    #[inline]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[inline]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// 命中测试：返回世界坐标 `(wx, wy)` 最顶层命中的节点 ID。
    pub fn hit_test_node(&self, wx: f32, wy: f32) -> Option<GraphNodeId> {
        for n in self.nodes.iter().rev() {
            let h = n.visual_height();
            if wx >= n.x && wx <= n.x + n.width && wy >= n.y && wy <= n.y + h {
                return Some(n.id);
            }
        }
        None
    }

    /// 端口吸附命中测试：在 `snap_radius` 世界半径内查找最近的端口。
    pub fn hit_test_port(
        &self,
        wx: f32,
        wy: f32,
        snap_radius: f32,
    ) -> Option<(GraphNodeId, GraphPortId, PortDir)> {
        let r2 = snap_radius * snap_radius;
        let mut best: Option<(f32, GraphNodeId, GraphPortId, PortDir)> = None;
        for n in &self.nodes {
            for p in &n.ports {
                if let Some((px, py)) = n.port_world_pos(p.id) {
                    let dx = wx - px;
                    let dy = wy - py;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= r2 && best.is_none_or(|b| d2 < b.0) {
                        best = Some((d2, n.id, p.id, p.dir));
                    }
                }
            }
        }
        best.map(|(_, nid, pid, dir)| (nid, pid, dir))
    }

    /// 三次贝塞尔曲线连线命中测试（16 段采样距离判定）。
    pub fn hit_test_edge(&self, wx: f32, wy: f32, tolerance: f32) -> Option<u32> {
        let tol2 = tolerance * tolerance;
        for e in &self.edges {
            let Some(src) = self.node(e.from_node).and_then(|n| n.port_world_pos(e.from_port))
            else {
                continue;
            };
            let Some(dst) = self.node(e.to_node).and_then(|n| n.port_world_pos(e.to_port))
            else {
                continue;
            };
            let (p0, p1, p2, p3) = edge_bezier_controls(src, dst);
            let mut prev = p0;
            for step in 1..=16 {
                let t = step as f32 / 16.0;
                let cur = eval_cubic_bezier(p0, p1, p2, p3, t);
                let vx = cur.0 - prev.0;
                let vy = cur.1 - prev.1;
                let len2 = (vx * vx + vy * vy).max(1e-4);
                let proj = (((wx - prev.0) * vx + (wy - prev.1) * vy) / len2).clamp(0.0, 1.0);
                let cx = prev.0 + vx * proj;
                let cy = prev.1 + vy * proj;
                let dx = wx - cx;
                let dy = wy - cy;
                if dx * dx + dy * dy <= tol2 {
                    return Some(e.id);
                }
                prev = cur;
            }
        }
        None
    }

    /// 基于 Kahn 拓扑分层算法自动排布所有节点（左→右有向无环图布局）。
    pub fn auto_layout_layered(&mut self, col_gap: f32, row_gap: f32) {
        let mut indeg: HashMap<GraphNodeId, usize> = HashMap::new();
        let mut adj: HashMap<GraphNodeId, Vec<GraphNodeId>> = HashMap::new();
        for n in &self.nodes {
            indeg.insert(n.id, 0);
            adj.entry(n.id).or_default();
        }
        for e in &self.edges {
            if let Some(d) = indeg.get_mut(&e.to_node) {
                *d += 1;
            }
            adj.entry(e.from_node).or_default().push(e.to_node);
        }
        let mut layer: HashMap<GraphNodeId, usize> = HashMap::new();
        let mut q: VecDeque<GraphNodeId> = VecDeque::new();
        for (&id, &d) in &indeg {
            if d == 0 {
                layer.insert(id, 0);
                q.push_back(id);
            }
        }
        while let Some(u) = q.pop_front() {
            let lu = *layer.get(&u).unwrap_or(&0);
            if let Some(nexts) = adj.get(&u) {
                for &v in nexts {
                    let lv = layer.entry(v).or_insert(0);
                    if lu + 1 > *lv {
                        *lv = lu + 1;
                    }
                    if let Some(d) = indeg.get_mut(&v) {
                        *d = d.saturating_sub(1);
                        if *d == 0 {
                            q.push_back(v);
                        }
                    }
                }
            }
        }
        let mut columns: HashMap<usize, Vec<GraphNodeId>> = HashMap::new();
        for n in &self.nodes {
            let col = *layer.get(&n.id).unwrap_or(&0);
            columns.entry(col).or_default().push(n.id);
        }
        let mut col_keys: Vec<usize> = columns.keys().copied().collect();
        col_keys.sort_unstable();
        for col in col_keys {
            let ids = &columns[&col];
            for (row_idx, &nid) in ids.iter().enumerate() {
                if let Some(n) = self.node_mut(nid) {
                    n.x = 40.0 + col as f32 * (n.width + col_gap);
                    n.y = 40.0 + row_idx as f32 * (n.visual_height() + row_gap);
                }
            }
        }
    }

    /// 计算整个节点图的世界包围盒 `[min_x, min_y, max_x, max_y]`。
    pub fn world_bounds(&self) -> [f32; 4] {
        if self.nodes.is_empty() {
            return [0.0, 0.0, 400.0, 300.0];
        }
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for n in &self.nodes {
            min_x = min_x.min(n.x);
            min_y = min_y.min(n.y);
            max_x = max_x.max(n.x + n.width);
            max_y = max_y.max(n.y + n.visual_height());
        }
        [min_x - 32.0, min_y - 32.0, max_x + 32.0, max_y + 32.0]
    }

    /// 选中单个节点（清除其他节点与边的选中态）。
    pub fn select_node(&mut self, id: Option<GraphNodeId>) {
        self.selected_edge = None;
        for n in &mut self.nodes {
            n.selected = Some(n.id) == id;
        }
    }

    /// 返回当前已选中的节点 ID。
    pub fn selected_node(&self) -> Option<GraphNodeId> {
        self.nodes.iter().find(|n| n.selected).map(|n| n.id)
    }

    /// 动态新增一个可自由拖拽与端口连线的 AI 工具节点，返回新节点 ID。
    pub fn spawn_tool_node(&mut self, wx: f32, wy: f32) -> GraphNodeId {
        let nid = self.next_node_id;
        self.next_node_id = self.next_node_id.saturating_add(1);
        let pin_in = self.next_port_id;
        let pin_out = self.next_port_id.saturating_add(1);
        self.next_port_id = self.next_port_id.saturating_add(2);
        let palettes = [
            ("Guardrail", "Filter", Color::rgb(20, 184, 166)),
            ("SQL Query", "Tool", Color::rgb(99, 102, 241)),
            ("Memory Cache", "Store", Color::rgb(244, 63, 94)),
            ("Eval Grader", "Check", Color::rgb(234, 179, 8)),
        ];
        let (title, cat, accent) = palettes[(nid as usize) % palettes.len()];
        let node = GraphNode::new(nid, format!("{title} #{nid}"), wx, wy)
            .category(cat)
            .accent(accent)
            .with_port(pin_in, PortDir::Input, "input", Color::rgb(96, 165, 250))
            .with_port(pin_out, PortDir::Output, "result", accent);
        self.add_node(node);
        self.select_node(Some(GraphNodeId(nid)));
        GraphNodeId(nid)
    }

    /// 响应鼠标悬停移动：实时更新悬停节点/连线高亮与端口拉线目标预览。
    pub fn handle_pointer_move(&mut self, local_x: i16, local_y: i16, vp: Viewport2D) -> bool {
        let (wx, wy) = vp.local_to_world(local_x as f32, local_y as f32);
        self.cursor_world = (wx, wy);
        let next_node = self.hit_test_node(wx, wy);
        let next_edge = if next_node.is_none() {
            self.hit_test_edge(wx, wy, 10.0 / vp.zoom.max(0.25))
        } else {
            None
        };
        let changed = self.hovered_node != next_node
            || self.hovered_edge != next_edge
            || self.connecting_from.is_some();
        self.hovered_node = next_node;
        self.hovered_edge = next_edge;
        changed
    }

    /// 一站式处理画布按下与拖拽：
    /// - 按在输出端口附近 → 启动/拖动实时贝塞尔拉线，滑到输入端口自动吸附连线；
    /// - 按在节点卡片上 → 选中并实时拖动该节点（连线自动弹性跟随）；
    /// - 按在连线上 → 选中该连线；
    /// - 按在空白网格上 → 平移整张无限画布。
    /// 返回是否消费了本次拖拽（`false` 时调用方可继续平移整张画布）。
    pub fn handle_drag(
        &mut self,
        ev: DragEvent,
        controller: &mut ViewportController,
    ) -> &'static str {
        let vp = controller.viewport;
        let (wx, wy) = vp.local_to_world(ev.local_x as f32, ev.local_y as f32);
        self.cursor_world = (wx, wy);
        let is_down = ev.delta_x == 0 && ev.delta_y == 0;

        if is_down {
            self.dragging_node = None;
            self.drag_origin_pos = None;
            self.connecting_from = None;
            let snap_r = (18.0 / vp.zoom.max(0.25)).clamp(12.0, 32.0);
            if let Some((nid, pid, PortDir::Output)) = self.hit_test_port(wx, wy, snap_r) {
                self.connecting_from = Some((nid, pid));
                self.select_node(Some(nid));
                return "Wiring from output port (drag to an input port)";
            }
            if let Some(nid) = self.hit_test_node(wx, wy) {
                self.select_node(Some(nid));
                self.dragging_node = Some(nid);
                if let Some(n) = self.node(nid) {
                    self.drag_origin_pos = Some((n.x, n.y));
                }
                return "Dragging node";
            }
            if let Some(eid) = self.hit_test_edge(wx, wy, 10.0 / vp.zoom.max(0.25)) {
                self.select_node(None);
                self.selected_edge = Some(eid);
                return "Selected edge (click Delete Edge or right-click to remove)";
            }
            self.select_node(None);
            return "Panning canvas";
        }

        // 正在端口拉线：检测是否滑入任意兼容输入端口并自动吸附连接
        if let Some((from_node, from_port)) = self.connecting_from {
            let snap_r = (20.0 / vp.zoom.max(0.25)).clamp(14.0, 36.0);
            if let Some((to_node, to_port, PortDir::Input)) = self.hit_test_port(wx, wy, snap_r) {
                if to_node != from_node
                    && self.connect(from_node, from_port, to_node, to_port).is_some()
                {
                    self.connecting_from = None;
                    return "Connected new Bezier edge";
                }
            }
            return "Drag to any input port to connect";
        }

        // 正在拖拽单个节点
        if let Some(nid) = self.dragging_node {
            let dwx = ev.delta_x as f32 / vp.zoom.max(0.1);
            let dwy = ev.delta_y as f32 / vp.zoom.max(0.1);
            self.translate_node_live(nid, dwx, dwy);
            return "Moving node";
        }

        // 空白区平移无限画布
        controller.pan_by(ev.delta_x as f32, ev.delta_y as f32);
        "Panning canvas"
    }

    /// 结束拖拽/拉线时提交 Undo 历史记录。
    pub fn finish_pointer_interaction(&mut self) {
        if let (Some(nid), Some(old_pos)) = (self.dragging_node.take(), self.drag_origin_pos.take())
        {
            if let Some(n) = self.node(nid) {
                let new_pos = (n.x, n.y);
                if (old_pos.0 - new_pos.0).abs() > 0.5 || (old_pos.1 - new_pos.1).abs() > 0.5 {
                    self.undo_stack.push(NodeGraphCommand::MoveNode {
                        id: nid,
                        old_pos,
                        new_pos,
                    });
                    self.redo_stack.clear();
                }
            }
        }
        self.connecting_from = None;
    }

    /// 右键快捷操作：右击连线直接删除，右击节点直接折叠/展开。
    pub fn handle_secondary_click(&mut self, local_x: i16, local_y: i16, vp: Viewport2D) -> &'static str {
        let (wx, wy) = vp.local_to_world(local_x as f32, local_y as f32);
        if let Some(nid) = self.hit_test_node(wx, wy) {
            self.toggle_collapse(nid);
            return "Toggled node collapse";
        }
        if let Some(eid) = self.hit_test_edge(wx, wy, 12.0 / vp.zoom.max(0.25)) {
            self.disconnect(eid);
            if self.selected_edge == Some(eid) {
                self.selected_edge = None;
            }
            return "Deleted Bezier edge";
        }
        "Right-click a node to collapse or an edge to delete"
    }
}

/// 声明式无限画布节点图组件（`NodeGraphView`）。
pub struct NodeGraphView<'s, 'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub graph: &'s NodeGraphStore,
    pub controller: &'s ViewportController,
    pub pulse_phase: f32,
    pub on_wheel: Option<&'a dyn Fn(WheelEvent, &mut ActionScope)>,
    pub on_drag: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    pub on_pointer_move: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    pub on_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
    pub on_secondary_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
}

impl<'s, 'a> NodeGraphView<'s, 'a> {
    pub fn new(
        id: u32,
        width: i16,
        height: i16,
        graph: &'s NodeGraphStore,
        controller: &'s ViewportController,
    ) -> Self {
        Self {
            id,
            width,
            height,
            graph,
            controller,
            pulse_phase: 0.35,
            on_wheel: None,
            on_drag: None,
            on_pointer_move: None,
            on_click: None,
            on_secondary_click: None,
        }
    }

    #[inline]
    pub const fn pulse_phase(mut self, phase: f32) -> Self {
        self.pulse_phase = phase;
        self
    }

    #[inline]
    pub fn on_wheel(mut self, cb: &'a dyn Fn(WheelEvent, &mut ActionScope)) -> Self {
        self.on_wheel = Some(cb);
        self
    }

    #[inline]
    pub fn on_drag(mut self, cb: &'a dyn Fn(DragEvent, &mut ActionScope)) -> Self {
        self.on_drag = Some(cb);
        self
    }

    #[inline]
    pub fn on_pointer_move(mut self, cb: &'a dyn Fn(DragEvent, &mut ActionScope)) -> Self {
        self.on_pointer_move = Some(cb);
        self
    }

    #[inline]
    pub fn on_click(mut self, cb: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.on_click = Some(cb);
        self
    }

    #[inline]
    pub fn on_secondary_click(mut self, cb: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.on_secondary_click = Some(cb);
        self
    }
}

impl<'s, 'a> Widget<'a> for NodeGraphView<'s, 'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = *cx.theme();
        let arena = cx.arena();
        let vp: Viewport2D = self.controller.viewport;

        // 1. 使用原生 VectorCanvas 绘制背景点阵网格 + 三次贝塞尔连线 + 流光数据脉冲点
        let mut prims: Vec<VectorPrimitive<'a>> = Vec::with_capacity(self.graph.edges.len() * 3 + 24);
        let grid_step_world = 32.0f32;
        let vis = vp.visible_world_bounds(self.width as f32, self.height as f32);
        let gx0 = (vis[0] / grid_step_world).floor() as i32;
        let gx1 = (vis[2] / grid_step_world).ceil() as i32;
        let gy0 = (vis[1] / grid_step_world).floor() as i32;
        let gy1 = (vis[3] / grid_step_world).ceil() as i32;
        let grid_col = theme.border.scale_alpha(70);
        if (gx1 - gx0).abs() <= 48 && (gy1 - gy0).abs() <= 36 {
            for gx in gx0..=gx1 {
                let (lx, _) = vp.world_to_local(gx as f32 * grid_step_world, 0.0);
                let pts = arena.alloc_iter([
                    VecPoint::new(lx, 0.0),
                    VecPoint::new(lx, self.height as f32),
                ]);
                prims.push(VectorPrimitive::Polyline {
                    points: pts,
                    color: grid_col,
                    width: 1.0,
                    dash_len: 0.0,
                    gap_len: 0.0,
                    closed: false,
                });
            }
            for gy in gy0..=gy1 {
                let (_, ly) = vp.world_to_local(0.0, gy as f32 * grid_step_world);
                let pts = arena.alloc_iter([
                    VecPoint::new(0.0, ly),
                    VecPoint::new(self.width as f32, ly),
                ]);
                prims.push(VectorPrimitive::Polyline {
                    points: pts,
                    color: grid_col,
                    width: 1.0,
                    dash_len: 0.0,
                    gap_len: 0.0,
                    closed: false,
                });
            }
        }

        // 绘制所有三次贝塞尔曲线边
        for e in &self.graph.edges {
            let Some(src_w) = self
                .graph
                .node(e.from_node)
                .and_then(|n| n.port_world_pos(e.from_port))
            else {
                continue;
            };
            let Some(dst_w) = self
                .graph
                .node(e.to_node)
                .and_then(|n| n.port_world_pos(e.to_port))
            else {
                continue;
            };
            let (p0, p1, p2, p3) = edge_bezier_controls(src_w, dst_w);
            let mut sample_pts = [VecPoint::default(); 21];
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let (wx, wy) = eval_cubic_bezier(p0, p1, p2, p3, t);
                let (lx, ly) = vp.world_to_local(wx, wy);
                sample_pts[i] = VecPoint::new(lx, ly);
            }
            let is_hot = self.graph.hovered_edge == Some(e.id)
                || self.graph.selected_edge == Some(e.id);
            let edge_col = if self.graph.selected_edge == Some(e.id) {
                Color::rgb(250, 204, 21)
            } else if is_hot {
                theme.primary
            } else {
                self.graph
                    .node(e.from_node)
                    .map(|n| n.accent)
                    .unwrap_or(theme.primary)
                    .scale_alpha(210)
            };
            let pts_slice = arena.alloc_iter(sample_pts);
            prims.push(VectorPrimitive::Polyline {
                points: pts_slice,
                color: edge_col,
                width: if is_hot { 3.4 } else { 2.2 },
                dash_len: 0.0,
                gap_len: 0.0,
                closed: false,
            });
            // 数据流光脉冲圆点
            let pt_t = self.pulse_phase.rem_euclid(1.0);
            let (pw, py) = eval_cubic_bezier(p0, p1, p2, p3, pt_t);
            let (plx, ply) = vp.world_to_local(pw, py);
            prims.push(VectorPrimitive::Circle {
                cx: plx,
                cy: ply,
                radius: (3.8 * vp.zoom).clamp(2.5, 6.0),
                fill: Color::WHITE,
                stroke: edge_col,
                stroke_width: 1.5,
            });
        }

        // 若正在从输出端口拉线，绘制跟随鼠标的虚线贝塞尔预览线
        if let Some((from_node, from_port)) = self.graph.connecting_from {
            if let Some(src_w) = self
                .graph
                .node(from_node)
                .and_then(|n| n.port_world_pos(from_port))
            {
                let (p0, p1, p2, p3) = edge_bezier_controls(src_w, self.graph.cursor_world);
                let mut sample_pts = [VecPoint::default(); 21];
                for i in 0..=20 {
                    let t = i as f32 / 20.0;
                    let (wx, wy) = eval_cubic_bezier(p0, p1, p2, p3, t);
                    let (lx, ly) = vp.world_to_local(wx, wy);
                    sample_pts[i] = VecPoint::new(lx, ly);
                }
                let pts_slice = arena.alloc_iter(sample_pts);
                prims.push(VectorPrimitive::Polyline {
                    points: pts_slice,
                    color: Color::rgb(56, 189, 248),
                    width: 2.6,
                    dash_len: 6.0,
                    gap_len: 4.0,
                    closed: false,
                });
            }
        }

        let mut root = div()
            .id(self.id)
            .w(self.width)
            .h(self.height)
            .rounded(8)
            .bg(theme.background)
            .border(1, theme.border)
            .overflow_hidden()
            .cursor(CursorShape::Grab)
            .child(cx, vector_canvas(cx, self.width, self.height, &prims));

        // 2. 在视口裁剪范围内叠加清晰交互节点卡片与端口圆点
        for n in &self.graph.nodes {
            let nh = n.visual_height();
            if n.x + n.width < vis[0] || n.x > vis[2] || n.y + nh < vis[1] || n.y > vis[3] {
                continue;
            }
            let (lx0, ly0) = vp.world_to_local(n.x, n.y);
            let sw = (n.width * vp.zoom).round().clamp(48.0, 640.0) as i16;
            let sh = (nh * vp.zoom).round().clamp(24.0, 480.0) as i16;
            let is_hot = n.selected || self.graph.hovered_node == Some(n.id);
            let border_col = if is_hot { n.accent } else { theme.border };
            let title_str = arena.alloc_str(&n.title).as_str();
            let cat_str = arena.alloc_str(&n.category).as_str();

            let header = div()
                .flex_row()
                .w_full()
                .h((26.0 * vp.zoom).round().clamp(18.0, 36.0) as i16)
                .px(8)
                .bg(n.accent.scale_alpha(46))
                .items_center()
                .justify_between()
                .child(
                    cx,
                    text(title_str)
                        .size((11.5 * vp.zoom).round().clamp(9.0, 16.0) as u8)
                        .color(theme.foreground)
                        .nowrap(),
                )
                .child(
                    cx,
                    text(cat_str)
                        .size((9.5 * vp.zoom).round().clamp(8.0, 12.0) as u8)
                        .color(n.accent)
                        .nowrap(),
                );

            let mut card = div()
                .id(self.id.wrapping_add(n.id.0.wrapping_mul(31)))
                .pos(lx0.round() as i16, ly0.round() as i16)
                .w(sw)
                .h(sh)
                .flex_col()
                .rounded(8)
                .bg(theme.card.scale_alpha(244))
                .border(if is_hot { 2 } else { 1 }, border_col)
                .overflow_hidden()
                .child(cx, header);

            if !n.collapsed {
                let mut body = div().flex_col().w_full().px(8).py(5).gap(4);
                for p in &n.ports {
                    let plbl = arena.alloc_str(&p.label).as_str();
                    let dot = div().w(7).h(7).rounded(4).bg(p.color);
                    let row = match p.dir {
                        PortDir::Input => div()
                            .flex_row()
                            .items_center()
                            .gap(5)
                            .child(cx, dot)
                            .child(
                                cx,
                                text(plbl)
                                    .size((10.0 * vp.zoom).round().clamp(8.0, 13.0) as u8)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            ),
                        PortDir::Output => div()
                            .flex_row()
                            .w_full()
                            .items_center()
                            .justify_end()
                            .gap(5)
                            .child(
                                cx,
                                text(plbl)
                                    .size((10.0 * vp.zoom).round().clamp(8.0, 13.0) as u8)
                                    .color(theme.muted_foreground)
                                    .nowrap(),
                            )
                            .child(cx, dot),
                    };
                    body = body.child(cx, row);
                }
                card = card.child(cx, body);
            }
            root = root.child(cx, card);
        }

        if let Some(cb) = self.on_wheel {
            root = root.on_wheel(cx, cb);
        }
        if let Some(cb) = self.on_drag {
            root = root.on_drag(cx, cb);
        }
        if let Some(cb) = self.on_pointer_move {
            root = root.on_pointer_move(cx, cb);
        }
        if let Some(cb) = self.on_click {
            root = root.on_click(cx, cb);
        }
        if let Some(cb) = self.on_secondary_click {
            root = root.on_secondary_click(cx, cb);
        }

        root.into_element(cx)
    }
}
