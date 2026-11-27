//! AI-Native UI 语义自动化控制协议（MCP JSON-RPC 2.0）与确定性时间旅行事件录像回放（`.vlxrec`）。
//!
//! 零外部 `serde` 依赖，直接复用 [`crate::a11y`] 与 [`crate::platform::a11y_bridge`]：
//! - **结构化 UI 语义快照（`snapshot_json`）**：任何 Velox 应用零代码即可向 AI Agent / E2E 测试框架暴露
//!   完整语义节点树（id / role / bounds / label / value / focusable / checked / children）；
//! - **原生 MCP 协议处理（`handle_mcp_request`）**：内置 `initialize`、`tools/list`、`tools/call`
//!   （`velox_snapshot_tree`、`velox_click`、`velox_click_label`、`velox_fill`、`velox_scroll`、`velox_wait_idle`）；
//! - **确定性事件录像与回放（`EventRecorder` / `EventReplayer` / `.vlxrec`）**：记录 `InputEvent` + 虚拟时钟 `dt_ms`
//!   + 帧状态校验和，实现线上 Bug 几十 KB 录像文件一帧不差复现。

use std::fmt::Write as _;
use std::format;
use std::string::String;
use std::vec::Vec;

use crate::a11y::A11yRole;
use crate::engine::VeloxEngine;
use crate::platform::a11y_bridge::{AccessKitActionRequest, AccessKitBridge, AccessKitNodeData};
use crate::pod::{InputEvent, KeyCode};

/// 语义节点查询条件。
#[derive(Clone, Debug, Default)]
pub struct SemanticQuery {
    pub id: Option<u32>,
    pub role: Option<A11yRole>,
    pub label_contains: Option<String>,
    pub only_focusable: bool,
}

/// AI Agent 或自动化测试可下发的结构化 UI 语义动作。
#[derive(Clone, Debug)]
pub enum SemanticAction {
    SnapshotTree,
    QueryNodes(SemanticQuery),
    ClickNode(u32),
    ClickByLabel(String),
    FillNode { target_id: u32, text: String },
    ScrollNode { target_id: u32, delta_y: i16 },
    PressKey(KeyCode),
    WaitIdle { max_frames: u16, dt_ms: u16 },
}

/// 语义动作执行回执。
#[derive(Clone, Debug)]
pub struct AutomationResult {
    pub ok: bool,
    pub message: String,
    pub payload_json: String,
    pub frames_stepped: u16,
}

fn role_str(role: A11yRole) -> &'static str {
    match role {
        A11yRole::Window => "window",
        A11yRole::Container => "container",
        A11yRole::Button => "button",
        A11yRole::TextInput => "text_input",
        A11yRole::Slider => "slider",
        A11yRole::ScrollRegion => "scroll_region",
        A11yRole::Text => "text",
        A11yRole::Image => "image",
    }
}

fn push_json_escaped(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn encode_node_json(out: &mut String, n: &AccessKitNodeData) {
    let _ = write!(
        out,
        "{{\"id\":{},\"role\":\"{}\",\"bounds\":[{},{},{},{}],\"focusable\":{},\"focused\":{},\"disabled\":{}",
        n.id,
        role_str(n.role),
        n.bounds.x,
        n.bounds.y,
        n.bounds.w,
        n.bounds.h,
        n.focusable,
        n.focused,
        n.disabled
    );
    if let Some(lbl) = &n.label {
        out.push_str(",\"label\":");
        push_json_escaped(out, lbl);
    }
    if let Some(val) = &n.value {
        out.push_str(",\"value\":");
        push_json_escaped(out, val);
    }
    if let Some(chk) = n.checked {
        let _ = write!(out, ",\"checked\":{chk}");
    }
    out.push_str(",\"children\":[");
    for (i, cid) in n.children.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{cid}");
    }
    out.push_str("]}");
}

/// 面向 AI Agent 与 Playwright 级 E2E 自动化测试的统一会话控制器。
#[derive(Default)]
pub struct AutomationSession {
    bridge: AccessKitBridge,
}

impl AutomationSession {
    pub fn new() -> Self {
        Self {
            bridge: AccessKitBridge::new(),
        }
    }

    /// 采集当前 [`VeloxEngine`] 的全部无障碍语义节点列表。
    pub fn collect_nodes(&mut self, engine: &VeloxEngine) -> Vec<AccessKitNodeData> {
        let mut fresh = AccessKitBridge::new();
        let _ = self.bridge.sync_engine(engine);
        fresh
            .sync_engine(engine)
            .map(|u| u.updated_nodes)
            .unwrap_or_default()
    }

    /// 按查询条件检索匹配的语义节点。
    pub fn query_nodes(&mut self, engine: &VeloxEngine, q: &SemanticQuery) -> Vec<AccessKitNodeData> {
        let needle = q.label_contains.as_deref().map(|s| s.to_ascii_lowercase());
        self.collect_nodes(engine)
            .into_iter()
            .filter(|n| {
                if let Some(id) = q.id {
                    if n.id != id {
                        return false;
                    }
                }
                if let Some(role) = q.role {
                    if n.role != role {
                        return false;
                    }
                }
                if q.only_focusable && !n.focusable {
                    return false;
                }
                if let Some(ref sub) = needle {
                    let matched = n
                        .label
                        .as_deref()
                        .is_some_and(|l| l.to_ascii_lowercase().contains(sub.as_str()))
                        || n.value
                            .as_deref()
                            .is_some_and(|v| v.to_ascii_lowercase().contains(sub.as_str()));
                    if !matched {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// 导出结构化 JSON 语义树快照（可直接喂给 LLM / MCP Client）。
    pub fn snapshot_json(&mut self, engine: &VeloxEngine) -> String {
        let nodes = self.collect_nodes(engine);
        let vp = engine.viewport_region();
        let focus_id = nodes.iter().find(|n| n.focused).map(|n| n.id).unwrap_or(0);
        let mut out = String::with_capacity(nodes.len() * 96 + 64);
        let _ = write!(
            out,
            "{{\"viewport\":[{},{},{},{}],\"focus_id\":{},\"node_count\":{},\"nodes\":[",
            vp.x,
            vp.y,
            vp.w,
            vp.h,
            focus_id,
            nodes.len()
        );
        for (i, n) in nodes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            encode_node_json(&mut out, n);
        }
        out.push_str("]}");
        out
    }

    /// 执行结构化语义动作并自动步进引擎帧循环。
    pub fn execute(&mut self, engine: &mut VeloxEngine, action: SemanticAction) -> AutomationResult {
        match action {
            SemanticAction::SnapshotTree => AutomationResult {
                ok: true,
                message: String::from("snapshot captured"),
                payload_json: self.snapshot_json(engine),
                frames_stepped: 0,
            },
            SemanticAction::QueryNodes(q) => {
                let matched = self.query_nodes(engine, &q);
                let mut out = String::from("[");
                for (i, n) in matched.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    encode_node_json(&mut out, n);
                }
                out.push(']');
                AutomationResult {
                    ok: true,
                    message: format!("matched {} node(s)", matched.len()),
                    payload_json: out,
                    frames_stepped: 0,
                }
            }
            SemanticAction::ClickNode(target_id) => {
                let ok = self
                    .bridge
                    .apply_action_request(engine, AccessKitActionRequest::Click { target_id });
                let _ = engine.step_frame_dt(16);
                AutomationResult {
                    ok,
                    message: if ok {
                        format!("clicked node {target_id}")
                    } else {
                        format!("node {target_id} not clickable or not found")
                    },
                    payload_json: format!("{{\"clicked\":{ok},\"target_id\":{target_id}}}"),
                    frames_stepped: 1,
                }
            }
            SemanticAction::ClickByLabel(label) => {
                let hits = self.query_nodes(
                    engine,
                    &SemanticQuery {
                        label_contains: Some(label.clone()),
                        ..Default::default()
                    },
                );
                if let Some(target) = hits.iter().find(|n| n.focusable).or_else(|| hits.first()) {
                    let cx = target.bounds.x + target.bounds.w / 2;
                    let cy = target.bounds.y + target.bounds.h / 2;
                    let ok = engine.dispatch_click(cx, cy);
                    let _ = engine.step_frame_dt(16);
                    AutomationResult {
                        ok,
                        message: format!("clicked label '{}' (node #{})", label, target.id),
                        payload_json: format!("{{\"clicked\":{ok},\"target_id\":{}}}", target.id),
                        frames_stepped: 1,
                    }
                } else {
                    AutomationResult {
                        ok: false,
                        message: format!("no node matched label '{label}'"),
                        payload_json: String::from("{\"clicked\":false}"),
                        frames_stepped: 0,
                    }
                }
            }
            SemanticAction::FillNode { target_id, text } => {
                let ok = self.bridge.apply_action_request(
                    engine,
                    AccessKitActionRequest::SetValue {
                        target_id,
                        text: text.clone(),
                    },
                );
                let _ = engine.step_frame_dt(16);
                AutomationResult {
                    ok,
                    message: format!("filled node {target_id}"),
                    payload_json: format!("{{\"filled\":{ok},\"target_id\":{target_id}}}"),
                    frames_stepped: 1,
                }
            }
            SemanticAction::ScrollNode { target_id, delta_y } => {
                let ok = self.bridge.apply_action_request(
                    engine,
                    AccessKitActionRequest::Scroll { target_id, delta_y },
                );
                let _ = engine.step_frame_dt(16);
                AutomationResult {
                    ok,
                    message: format!("scrolled node {target_id} by {delta_y}"),
                    payload_json: format!("{{\"scrolled\":{ok},\"target_id\":{target_id}}}"),
                    frames_stepped: 1,
                }
            }
            SemanticAction::PressKey(key) => {
                let ok = engine.dispatch_event(InputEvent::KeyDown(key));
                let _ = engine.step_frame_dt(16);
                AutomationResult {
                    ok,
                    message: String::from("dispatched key"),
                    payload_json: format!("{{\"handled\":{ok}}}"),
                    frames_stepped: 1,
                }
            }
            SemanticAction::WaitIdle { max_frames, dt_ms } => {
                let mut stepped = 0u16;
                let step_dt = dt_ms.max(1);
                for _ in 0..max_frames.max(1) {
                    stepped += 1;
                    let dirty = engine.step_frame_dt(step_dt);
                    if dirty.is_empty() && !engine.needs_redraw() {
                        break;
                    }
                }
                let idle = !engine.needs_redraw();
                AutomationResult {
                    ok: idle,
                    message: format!("stepped {stepped} frame(s), idle={idle}"),
                    payload_json: format!("{{\"idle\":{idle},\"frames\":{stepped}}}"),
                    frames_stepped: stepped,
                }
            }
        }
    }

    /// 处理标准 **MCP (Model Context Protocol) JSON-RPC 2.0** 请求并返回 JSON 响应字符串。
    ///
    /// 内置支持方法：
    /// - `initialize`
    /// - `tools/list`
    /// - `tools/call`（`velox_snapshot_tree`、`velox_click`、`velox_click_label`、`velox_fill`、`velox_scroll`、`velox_wait_idle`）
    pub fn handle_mcp_request(&mut self, engine: &mut VeloxEngine, req_json: &str) -> String {
        let req_id = extract_json_raw_field(req_json, "id").unwrap_or_else(|| String::from("1"));
        let method = extract_json_string_field(req_json, "method").unwrap_or_default();

        match method.as_str() {
            "initialize" => format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{req_id},\"result\":{{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{{\"tools\":{{}}}},\"serverInfo\":{{\"name\":\"velox-automation-mcp\",\"version\":\"0.4.0\"}}}}}}"
            ),
            "tools/list" => format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{req_id},\"result\":{{\"tools\":[{{\"name\":\"velox_snapshot_tree\",\"description\":\"Capture structured Velox UI accessibility/semantic tree as JSON\"}},{{\"name\":\"velox_click\",\"description\":\"Click a UI node by numeric id\"}},{{\"name\":\"velox_click_label\",\"description\":\"Find and click a UI node by label substring\"}},{{\"name\":\"velox_fill\",\"description\":\"Focus and type text into an input node by id\"}},{{\"name\":\"velox_scroll\",\"description\":\"Scroll a container node by delta_y\"}},{{\"name\":\"velox_wait_idle\",\"description\":\"Advance frames until all animations settle to 0% CPU idle\"}}]}}}}"
            ),
            "tools/call" => {
                let tool = extract_json_string_field(req_json, "name").unwrap_or_default();
                let res = match tool.as_str() {
                    "velox_snapshot_tree" => self.execute(engine, SemanticAction::SnapshotTree),
                    "velox_click" => {
                        let tid = extract_json_i64_field(req_json, "target_id").unwrap_or(0) as u32;
                        self.execute(engine, SemanticAction::ClickNode(tid))
                    }
                    "velox_click_label" => {
                        let lbl = extract_json_string_field(req_json, "label").unwrap_or_default();
                        self.execute(engine, SemanticAction::ClickByLabel(lbl))
                    }
                    "velox_fill" => {
                        let tid = extract_json_i64_field(req_json, "target_id").unwrap_or(0) as u32;
                        let txt = extract_json_string_field(req_json, "text").unwrap_or_default();
                        self.execute(
                            engine,
                            SemanticAction::FillNode {
                                target_id: tid,
                                text: txt,
                            },
                        )
                    }
                    "velox_scroll" => {
                        let tid = extract_json_i64_field(req_json, "target_id").unwrap_or(0) as u32;
                        let dy = extract_json_i64_field(req_json, "delta_y").unwrap_or(0) as i16;
                        self.execute(
                            engine,
                            SemanticAction::ScrollNode {
                                target_id: tid,
                                delta_y: dy,
                            },
                        )
                    }
                    "velox_wait_idle" => {
                        let max_f = extract_json_i64_field(req_json, "max_frames").unwrap_or(120) as u16;
                        self.execute(
                            engine,
                            SemanticAction::WaitIdle {
                                max_frames: max_f,
                                dt_ms: 16,
                            },
                        )
                    }
                    _ => AutomationResult {
                        ok: false,
                        message: format!("unknown tool '{tool}'"),
                        payload_json: String::from("{}"),
                        frames_stepped: 0,
                    },
                };
                let mut text_escaped = String::new();
                push_json_escaped(&mut text_escaped, &res.payload_json);
                format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":{req_id},\"result\":{{\"isError\":{},\"content\":[{{\"type\":\"text\",\"text\":{text_escaped}}}]}}}}",
                    !res.ok
                )
            }
            _ => format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{req_id},\"error\":{{\"code\":-32601,\"message\":\"Method not found\"}}}}"
            ),
        }
    }
}

fn find_json_field_value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let rest = &json[pos + needle.len()..];
    let colon = rest.find(':')?;
    Some(rest[colon + 1..].trim_start())
}

fn extract_json_string_field(json: &str, key: &str) -> Option<String> {
    let val = find_json_field_value(json, key)?;
    if !val.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = val[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                if let Some(esc) = chars.next() {
                    match esc {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        other => out.push(other),
                    }
                }
            }
            other => out.push(other),
        }
    }
    None
}

fn extract_json_i64_field(json: &str, key: &str) -> Option<i64> {
    let val = find_json_field_value(json, key)?;
    let end = val
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(val.len());
    val[..end].parse::<i64>().ok()
}

fn extract_json_raw_field(json: &str, key: &str) -> Option<String> {
    let val = find_json_field_value(json, key)?;
    if val.starts_with('"') {
        let s = extract_json_string_field(json, key)?;
        let mut out = String::new();
        push_json_escaped(&mut out, &s);
        Some(out)
    } else {
        let end = val
            .find(|c: char| matches!(c, ',' | '}' | ']' | ' ' | '\n' | '\r' | '\t'))
            .unwrap_or(val.len());
        Some(String::from(&val[..end]))
    }
}

// ============================================================================
// 确定性时间旅行事件录像与回放（`.vlxrec` 紧凑文本格式）
// ============================================================================

/// 可序列化录像的输入事件子集（涵盖鼠标、滚轮、缩放、文本、按键与时钟推进）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RecordedEvent {
    Tick,
    PointerDown { x: i16, y: i16 },
    PointerMove { x: i16, y: i16 },
    PointerUp { x: i16, y: i16 },
    Scroll { x: i16, y: i16, delta_y: i16 },
    Pinch { x: i16, y: i16, delta_scale_x1000: i16 },
    TextChar(char),
    KeyCodeTag(u8),
}

impl RecordedEvent {
    pub fn from_input_event(ev: &InputEvent) -> Option<Self> {
        match *ev {
            InputEvent::PointerDown { x, y } => Some(Self::PointerDown { x, y }),
            InputEvent::PointerMove { x, y } => Some(Self::PointerMove { x, y }),
            InputEvent::PointerUp { x, y } => Some(Self::PointerUp { x, y }),
            InputEvent::Scroll { x, y, delta_y } => Some(Self::Scroll { x, y, delta_y }),
            InputEvent::Pinch {
                x,
                y,
                delta_scale_x1000,
            } => Some(Self::Pinch {
                x,
                y,
                delta_scale_x1000,
            }),
            InputEvent::TextCommit(ch) => Some(Self::TextChar(ch)),
            InputEvent::KeyDown(KeyCode::Char(ch)) => Some(Self::TextChar(ch)),
            InputEvent::KeyDown(k) => key_to_tag(k).map(Self::KeyCodeTag),
            _ => None,
        }
    }

    pub fn to_input_event(self) -> Option<InputEvent> {
        match self {
            Self::Tick => None,
            Self::PointerDown { x, y } => Some(InputEvent::PointerDown { x, y }),
            Self::PointerMove { x, y } => Some(InputEvent::PointerMove { x, y }),
            Self::PointerUp { x, y } => Some(InputEvent::PointerUp { x, y }),
            Self::Scroll { x, y, delta_y } => Some(InputEvent::Scroll { x, y, delta_y }),
            Self::Pinch {
                x,
                y,
                delta_scale_x1000,
            } => Some(InputEvent::Pinch {
                x,
                y,
                delta_scale_x1000,
            }),
            Self::TextChar(ch) => Some(InputEvent::KeyDown(KeyCode::Char(ch))),
            Self::KeyCodeTag(t) => tag_to_key(t).map(InputEvent::KeyDown),
        }
    }
}

fn key_to_tag(k: KeyCode) -> Option<u8> {
    match k {
        KeyCode::Enter => Some(1),
        KeyCode::Escape => Some(2),
        KeyCode::Backspace => Some(3),
        KeyCode::Delete => Some(4),
        KeyCode::Tab => Some(5),
        KeyCode::Up => Some(6),
        KeyCode::Down => Some(7),
        KeyCode::Left => Some(8),
        KeyCode::Right => Some(9),
        KeyCode::Space => Some(10),
        KeyCode::Home => Some(11),
        KeyCode::End => Some(12),
        _ => None,
    }
}

fn tag_to_key(t: u8) -> Option<KeyCode> {
    match t {
        1 => Some(KeyCode::Enter),
        2 => Some(KeyCode::Escape),
        3 => Some(KeyCode::Backspace),
        4 => Some(KeyCode::Delete),
        5 => Some(KeyCode::Tab),
        6 => Some(KeyCode::Up),
        7 => Some(KeyCode::Down),
        8 => Some(KeyCode::Left),
        9 => Some(KeyCode::Right),
        10 => Some(KeyCode::Space),
        11 => Some(KeyCode::Home),
        12 => Some(KeyCode::End),
        _ => None,
    }
}

/// 单帧录像记录条（含虚拟时间差与帧确定性哈希）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RecordedStep {
    pub dt_ms: u16,
    pub event: RecordedEvent,
    pub state_checksum: u64,
}

/// 计算 [`VeloxEngine`] 当前帧的结构化确定性校验和（DrawOp 数量 + HitRegion 数量 + 焦点索引 + 布局边界摘要）。
pub fn compute_engine_frame_checksum(engine: &VeloxEngine) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    let ops = engine.cached_draw_ops();
    let hits = engine.cached_hit_regions();
    mix(ops.len() as u64);
    mix(hits.len() as u64);
    mix(engine.focused_index().unwrap_or(u16::MAX) as u64);
    for op in ops.iter().take(64) {
        mix(op.bounds.x as u16 as u64);
        mix(op.bounds.y as u16 as u64);
        mix(op.bounds.w as u16 as u64);
        mix(op.bounds.h as u16 as u64);
    }
    h
}

/// 确定性事件录像机（支持导出 `.vlxrec` 紧凑文本格式供线上 Bug 复现）。
#[derive(Clone, Debug, Default)]
pub struct EventRecorder {
    pub steps: Vec<RecordedStep>,
}

impl EventRecorder {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.steps.clear();
    }

    /// 录制一次事件派发 + 步进帧的完整确定性快照。
    pub fn record_dispatch(
        &mut self,
        engine: &mut VeloxEngine,
        dt_ms: u16,
        event: InputEvent,
    ) -> bool {
        let rec = RecordedEvent::from_input_event(&event).unwrap_or(RecordedEvent::Tick);
        let handled = engine.dispatch_event(event);
        let _ = engine.step_frame_dt(dt_ms);
        let checksum = compute_engine_frame_checksum(engine);
        self.steps.push(RecordedStep {
            dt_ms,
            event: rec,
            state_checksum: checksum,
        });
        handled
    }

    /// 仅录制一次虚拟时钟步进帧。
    pub fn record_tick(&mut self, engine: &mut VeloxEngine, dt_ms: u16) {
        let _ = engine.step_frame_dt(dt_ms);
        self.steps.push(RecordedStep {
            dt_ms,
            event: RecordedEvent::Tick,
            state_checksum: compute_engine_frame_checksum(engine),
        });
    }

    /// 序列化为人类可读且极紧凑的 `.vlxrec` 录像文本。
    pub fn to_vlxrec_text(&self) -> String {
        let mut out = String::from("VLXREC v1\n");
        for s in &self.steps {
            let _ = write!(out, "{} ", s.dt_ms);
            match s.event {
                RecordedEvent::Tick => out.push_str("TICK"),
                RecordedEvent::PointerDown { x, y } => {
                    let _ = write!(out, "PD {x} {y}");
                }
                RecordedEvent::PointerMove { x, y } => {
                    let _ = write!(out, "PM {x} {y}");
                }
                RecordedEvent::PointerUp { x, y } => {
                    let _ = write!(out, "PU {x} {y}");
                }
                RecordedEvent::Scroll { x, y, delta_y } => {
                    let _ = write!(out, "SC {x} {y} {delta_y}");
                }
                RecordedEvent::Pinch {
                    x,
                    y,
                    delta_scale_x1000,
                } => {
                    let _ = write!(out, "PN {x} {y} {delta_scale_x1000}");
                }
                RecordedEvent::TextChar(ch) => {
                    let _ = write!(out, "CH {}", ch as u32);
                }
                RecordedEvent::KeyCodeTag(tag) => {
                    let _ = write!(out, "KY {tag}");
                }
            }
            let _ = writeln!(out, " #{:016x}", s.state_checksum);
        }
        out
    }
}

/// 回放验证报告。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct ReplayReport {
    pub total_steps: usize,
    pub checksum_matches: usize,
    pub checksum_mismatches: usize,
}

impl ReplayReport {
    #[inline]
    pub const fn is_deterministic(&self) -> bool {
        self.total_steps > 0 && self.checksum_mismatches == 0
    }
}

/// 确定性时间旅行事件回放器。
#[derive(Clone, Debug)]
pub struct EventReplayer {
    steps: Vec<RecordedStep>,
    cursor: usize,
}

impl EventReplayer {
    pub fn from_vlxrec_text(text: &str) -> Result<Self, &'static str> {
        let mut lines = text.lines();
        let header = lines.next().ok_or("empty vlxrec")?;
        if !header.trim().starts_with("VLXREC") {
            return Err("invalid vlxrec header");
        }
        let mut steps = Vec::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                return Err("malformed vlxrec line");
            }
            let dt_ms: u16 = parts[0].parse().map_err(|_| "bad dt_ms")?;
            let checksum_hex = parts
                .last()
                .and_then(|s| s.strip_prefix('#'))
                .ok_or("missing checksum")?;
            let state_checksum = u64::from_str_radix(checksum_hex, 16).map_err(|_| "bad checksum")?;
            let event = match parts[1] {
                "TICK" => RecordedEvent::Tick,
                "PD" if parts.len() >= 5 => RecordedEvent::PointerDown {
                    x: parts[2].parse().map_err(|_| "bad x")?,
                    y: parts[3].parse().map_err(|_| "bad y")?,
                },
                "PM" if parts.len() >= 5 => RecordedEvent::PointerMove {
                    x: parts[2].parse().map_err(|_| "bad x")?,
                    y: parts[3].parse().map_err(|_| "bad y")?,
                },
                "PU" if parts.len() >= 5 => RecordedEvent::PointerUp {
                    x: parts[2].parse().map_err(|_| "bad x")?,
                    y: parts[3].parse().map_err(|_| "bad y")?,
                },
                "SC" if parts.len() >= 6 => RecordedEvent::Scroll {
                    x: parts[2].parse().map_err(|_| "bad x")?,
                    y: parts[3].parse().map_err(|_| "bad y")?,
                    delta_y: parts[4].parse().map_err(|_| "bad dy")?,
                },
                "PN" if parts.len() >= 6 => RecordedEvent::Pinch {
                    x: parts[2].parse().map_err(|_| "bad x")?,
                    y: parts[3].parse().map_err(|_| "bad y")?,
                    delta_scale_x1000: parts[4].parse().map_err(|_| "bad scale")?,
                },
                "CH" if parts.len() >= 4 => {
                    let cp: u32 = parts[2].parse().map_err(|_| "bad char")?;
                    RecordedEvent::TextChar(char::from_u32(cp).ok_or("invalid unicode")?)
                }
                "KY" if parts.len() >= 4 => {
                    let tag: u8 = parts[2].parse().map_err(|_| "bad key tag")?;
                    RecordedEvent::KeyCodeTag(tag)
                }
                _ => return Err("unknown vlxrec opcode"),
            };
            steps.push(RecordedStep {
                dt_ms,
                event,
                state_checksum,
            });
        }
        Ok(Self { steps, cursor: 0 })
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    #[inline]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 逐帧推进回放 1 步，返回 `Some(checksum_matched)`；结束时返回 `None`。
    pub fn step_next(&mut self, engine: &mut VeloxEngine) -> Option<bool> {
        let step = *self.steps.get(self.cursor)?;
        self.cursor += 1;
        if let Some(ev) = step.event.to_input_event() {
            let _ = engine.dispatch_event(ev);
        }
        let _ = engine.step_frame_dt(step.dt_ms);
        let actual = compute_engine_frame_checksum(engine);
        Some(actual == step.state_checksum)
    }

    /// 一次性回放全部剩余步骤并返回确定性验证报告。
    pub fn replay_all(&mut self, engine: &mut VeloxEngine) -> ReplayReport {
        let mut report = ReplayReport::default();
        while let Some(matched) = self.step_next(engine) {
            report.total_steps += 1;
            if matched {
                report.checksum_matches += 1;
            } else {
                report.checksum_mismatches += 1;
            }
        }
        report
    }
}
