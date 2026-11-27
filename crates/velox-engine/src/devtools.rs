//! 内置 DevTools Inspector 调试透视层与性能遥测（100% `#![no_std]` 零堆核心）。
//!
//! 可在任意业务界面根容器上方一键叠放：
//! - **盒模型透视**：实时高亮鼠标悬浮元素的边界框与坐标尺寸；
//! - **脏矩形红框透视**：实时叠加当前帧 `DamageTracker` 合并后的脏区矩形；
//! - **性能监控 HUD**：展示布局求解次数、L2 局部重排命中次数、Pass-4 快径次数、
//!   `SubView` 缓存命中、Arena 峰值字节数与 `DrawOp`/`HitRegion` 数量。

use crate::element::{div, text, Element, IntoElement};
use crate::engine::VeloxEngine;
use crate::fmt;
use crate::pod::FrameScope;
use crate::solver::Region;
use crate::style::{Color, Styled};

/// DevTools Inspector 开关配置。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct InspectorConfig {
    pub enabled: bool,
    pub show_box_model: bool,
    pub show_dirty_rects: bool,
    pub show_perf_hud: bool,
}

impl Default for InspectorConfig {
    fn default() -> Self {
        Self::ALL
    }
}

impl InspectorConfig {
    pub const OFF: Self = Self {
        enabled: false,
        show_box_model: false,
        show_dirty_rects: false,
        show_perf_hud: false,
    };

    pub const ALL: Self = Self {
        enabled: true,
        show_box_model: true,
        show_dirty_rects: true,
        show_perf_hud: true,
    };
}

/// 单帧实时性能与布局诊断遥测快照（纯 POD、`Copy`，可在 MCU 与桌面端零分配采集）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct FrameTelemetry {
    pub layout_solve_count: u32,
    pub partial_relayout_count: u32,
    pub relayout_boundary_hits: u32,
    pub fast_paint_pass_count: u32,
    pub subview_cache_hits: u32,
    pub arena_used_bytes: usize,
    pub draw_op_count: usize,
    pub hit_region_count: usize,
    pub dirty_rect_count: usize,
    pub hovered_bounds: Option<Region>,
    pub hovered_id: Option<u32>,
}

/// 从运行中的 [`VeloxEngine`] 采集当前帧性能指标与鼠标悬浮盒模型信息。
pub fn capture_telemetry(engine: &VeloxEngine, pointer_pos: Option<(i16, i16)>) -> FrameTelemetry {
    let mut hovered_bounds = None;
    let mut hovered_id = None;
    if let Some((px, py)) = pointer_pos {
        for hit in engine.cached_hit_regions().iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(px, py) {
                hovered_bounds = Some(hit.bounds);
                hovered_id = hit.id;
                break;
            }
        }
    }
    FrameTelemetry {
        layout_solve_count: engine.layout_solve_count(),
        partial_relayout_count: engine.partial_relayout_count(),
        relayout_boundary_hits: engine.relayout_boundary_hits(),
        fast_paint_pass_count: engine.fast_paint_pass_count(),
        subview_cache_hits: engine.subview_cache_hits(),
        arena_used_bytes: engine.last_arena_used_bytes(),
        draw_op_count: engine.cached_draw_ops().len(),
        hit_region_count: engine.cached_hit_regions().len(),
        dirty_rect_count: engine.last_damage().len(),
        hovered_bounds,
        hovered_id,
    }
}

/// 构建可叠放在任意根视图顶层的 DevTools Inspector 诊断浮层（零侵入业务布局，不拦截鼠标事件）。
pub fn build_inspector_overlay<'a>(
    cx: &'a FrameScope,
    telemetry: &FrameTelemetry,
    dirty_rects: &[Region],
    cfg: InspectorConfig,
) -> Element<'a> {
    if !cfg.enabled {
        return div().w(0).h(0).into_element(cx);
    }

    let mut layer = div().overlay().pos(0, 0).w_full().h_full();

    // 1. 脏矩形红框透视
    if cfg.show_dirty_rects {
        for &r in dirty_rects {
            if !r.is_empty() {
                layer = layer.child(
                    cx,
                    div()
                        .pos(r.x, r.y)
                        .w(r.w)
                        .h(r.h)
                        .bg(Color::rgba(239, 68, 68, 26))
                        .border(1, Color::rgba(239, 68, 68, 210)),
                );
            }
        }
    }

    // 2. 鼠标悬浮元素盒模型透视框与坐标角标
    if cfg.show_box_model {
        if let Some(hb) = telemetry.hovered_bounds {
            layer = layer.child(
                cx,
                div()
                    .pos(hb.x, hb.y)
                    .w(hb.w)
                    .h(hb.h)
                    .bg(Color::rgba(56, 189, 248, 42))
                    .border(1, Color::rgba(56, 189, 248, 235)),
            );
            let badge_y = if hb.y >= 20 { hb.y - 18 } else { hb.bottom() + 2 };
            let label = fmt!(cx, "{}x{} @ ({},{})", hb.w, hb.h, hb.x, hb.y);
            layer = layer.child(
                cx,
                div()
                    .pos(hb.x, badge_y)
                    .px(6)
                    .py(2)
                    .rounded(4)
                    .bg(Color::rgba(15, 23, 42, 230))
                    .child(cx, text(label).size(10).color(Color::rgb(125, 211, 252))),
            );
        }
    }

    // 3. 右上角实时性能监控 HUD
    if cfg.show_perf_hud {
        let l1 = fmt!(
            cx,
            "Solves:{} L2:{} Pass4:{}",
            telemetry.layout_solve_count,
            telemetry.partial_relayout_count,
            telemetry.fast_paint_pass_count
        );
        let l2 = fmt!(
            cx,
            "Ops:{} Hits:{} Dirty:{}",
            telemetry.draw_op_count,
            telemetry.hit_region_count,
            telemetry.dirty_rect_count
        );
        let l3 = fmt!(
            cx,
            "Arena:{}B SubCache:{}",
            telemetry.arena_used_bytes,
            telemetry.subview_cache_hits
        );
        layer = layer.child(
            cx,
            div()
                .pos(8, 8)
                .flex_col()
                .gap(2)
                .px(8)
                .py(6)
                .rounded(6)
                .bg(Color::rgba(15, 23, 42, 220))
                .border(1, Color::rgba(99, 102, 241, 180))
                .child(
                    cx,
                    text("Velox Inspector")
                        .size(11)
                        .color(Color::rgb(165, 180, 252)),
                )
                .child(cx, text(l1).size(10).color(Color::rgb(226, 232, 240)))
                .child(cx, text(l2).size(10).color(Color::rgb(226, 232, 240)))
                .child(cx, text(l3).size(10).color(Color::rgb(148, 163, 184))),
        );
    }

    layer.into_element(cx)
}

// ============================================================================
// 交互式 Live DevTools：64 帧耗时瀑布图 + 节点点选检视 + 零编译热重载样式覆写
// ============================================================================

/// 64 帧环形微秒耗时记录器（100% `#![no_std]` 零堆，供 Live DevTools 实时瀑布图使用）。
#[derive(Copy, Clone, Debug)]
pub struct FrameBudgetRing {
    samples_us: [u32; 64],
    cursor: usize,
    count: usize,
}

impl Default for FrameBudgetRing {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl FrameBudgetRing {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            samples_us: [0; 64],
            cursor: 0,
            count: 0,
        }
    }

    /// 推入一帧耗时（微秒）。
    #[inline]
    pub fn push_us(&mut self, us: u32) {
        self.samples_us[self.cursor] = us;
        self.cursor = (self.cursor + 1) & 63;
        if self.count < 64 {
            self.count += 1;
        }
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 计算环形窗口内平均帧耗时（微秒）。
    pub fn avg_us(&self) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let mut sum = 0u64;
        for i in 0..self.count {
            sum += self.samples_us[i] as u64;
        }
        (sum / self.count as u64) as u32
    }

    /// 计算环形窗口内峰值帧耗时（微秒）。
    pub fn max_us(&self) -> u32 {
        let mut peak = 0u32;
        for i in 0..self.count {
            if self.samples_us[i] > peak {
                peak = self.samples_us[i];
            }
        }
        peak
    }

    /// 读取按时间顺序排列的第 `idx` 条样本（`0` 最旧，`len()-1` 最新）。
    #[inline]
    pub fn sample_at(&self, idx: usize) -> u32 {
        if idx >= self.count {
            return 0;
        }
        let start = if self.count < 64 { 0 } else { self.cursor };
        self.samples_us[(start + idx) & 63]
    }
}

/// 单个 UI 节点的运行时热覆写样式（无需重新编译 Rust 即可实时微调布局与配色）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct StyleOverride {
    pub target_id: u32,
    pub padding: Option<i8>,
    pub gap: Option<i8>,
    pub rounded: Option<u8>,
    pub bg: Option<Color>,
    pub border_color: Option<Color>,
}

/// 零编译样式热重载覆写表（16 槽位 `#![no_std]` 零堆核心 + `std` 文本样式表热加载）。
#[derive(Clone, Debug)]
pub struct LiveStyleOverrides {
    entries: [Option<StyleOverride>; 16],
    revision: u32,
}

impl Default for LiveStyleOverrides {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl LiveStyleOverrides {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            entries: [None; 16],
            revision: 0,
        }
    }

    #[inline(always)]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    /// 写入或更新指定节点 ID 的实时热覆写参数。
    pub fn set(&mut self, ov: StyleOverride) -> bool {
        for slot in &mut self.entries {
            if let Some(existing) = slot {
                if existing.target_id == ov.target_id {
                    *existing = ov;
                    self.revision = self.revision.wrapping_add(1);
                    return true;
                }
            }
        }
        for slot in &mut self.entries {
            if slot.is_none() {
                *slot = Some(ov);
                self.revision = self.revision.wrapping_add(1);
                return true;
            }
        }
        false
    }

    /// 查询指定节点的实时覆写。
    #[inline]
    pub fn get(&self, target_id: u32) -> Option<StyleOverride> {
        for slot in &self.entries {
            if let Some(ov) = *slot {
                if ov.target_id == target_id {
                    return Some(ov);
                }
            }
        }
        None
    }

    /// 将覆写应用到目标 [`crate::style::Style`]（保持其他未覆写属性不变，零状态丢失）。
    pub fn apply_to_style(&self, target_id: u32, style: &mut crate::style::Style) -> bool {
        let Some(ov) = self.get(target_id) else {
            return false;
        };
        if let Some(p) = ov.padding {
            style.padding = crate::style::Edges::all(p);
        }
        if let Some(g) = ov.gap {
            style.gap = g;
        }
        if let Some(r) = ov.rounded {
            style.set_rounded(r);
        }
        if let Some(bg) = ov.bg {
            style.bg = bg;
        }
        if let Some(bc) = ov.border_color {
            style.border_color = bc;
        }
        true
    }

    /// 从轻量热重载样式文本解析并批量生效（格式：`id: pad=12 gap=8 rounded=10 bg=#1E293B`），
    /// 返回成功解析并生效的规则数。
    pub fn load_hot_reload_sheet(&mut self, sheet: &str) -> usize {
        let mut applied = 0usize;
        for raw_line in sheet.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((id_str, rest)) = line.split_once(':') else {
                continue;
            };
            let Ok(target_id) = id_str.trim().parse::<u32>() else {
                continue;
            };
            let mut ov = self.get(target_id).unwrap_or(StyleOverride {
                target_id,
                ..StyleOverride::default()
            });
            for tok in rest.split_whitespace() {
                if let Some((k, v)) = tok.split_once('=') {
                    match k {
                        "pad" | "padding" => {
                            if let Ok(p) = v.parse::<i8>() {
                                ov.padding = Some(p);
                            }
                        }
                        "gap" => {
                            if let Ok(g) = v.parse::<i8>() {
                                ov.gap = Some(g);
                            }
                        }
                        "rounded" | "radius" => {
                            if let Ok(r) = v.parse::<u8>() {
                                ov.rounded = Some(r);
                            }
                        }
                        "bg" => {
                            if let Some(c) = crate::svg::parse_svg_color(v) {
                                ov.bg = Some(c);
                            }
                        }
                        "border" => {
                            if let Some(c) = crate::svg::parse_svg_color(v) {
                                ov.border_color = Some(c);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if self.set(ov) {
                applied += 1;
            }
        }
        applied
    }
}

/// 交互式 Live DevTools 面板运行态（含节点拾取锁定、帧耗时瀑布图与热重载覆写）。
#[derive(Clone, Debug, Default)]
pub struct LiveInspectorState {
    pub config: InspectorConfig,
    pub selected_node_id: Option<u32>,
    pub selected_bounds: Option<Region>,
    pub budget: FrameBudgetRing,
    pub overrides: LiveStyleOverrides,
}

impl LiveInspectorState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 在鼠标点击点拾取最顶层可见元素并锁定为当前检视节点。
    pub fn pick_node_at(&mut self, engine: &VeloxEngine, px: i16, py: i16) -> Option<u32> {
        for hit in engine.cached_hit_regions().iter().rev() {
            if hit.bounds.intersect(hit.clip).contains(px, py) {
                if let Some(id) = hit.id {
                    self.selected_node_id = Some(id);
                    self.selected_bounds = Some(hit.bounds);
                    return Some(id);
                }
            }
        }
        None
    }
}

/// 构建交互式 Live DevTools 检视卡片（含 64 帧耗时柱状瀑布图、锁定节点盒模型与热重载修订号）。
pub fn build_live_devtools_panel<'a>(
    cx: &'a FrameScope,
    state: &LiveInspectorState,
    telemetry: FrameTelemetry,
) -> Element<'a> {
    let mut bars = div().flex_row().items_end().gap(2).h(34);
    let peak = state.budget.max_us().max(1000);
    let n = state.budget.len();
    let start = n.saturating_sub(24);
    for i in start..n {
        let us = state.budget.sample_at(i);
        let bh = ((us as u64 * 30) / peak as u64).clamp(3, 30) as i16;
        let col = if us > 16_000 {
            Color::rgb(239, 68, 68)
        } else if us > 8_000 {
            Color::rgb(245, 158, 11)
        } else {
            Color::rgb(52, 211, 153)
        };
        bars = bars.child(cx, div().w(5).h(bh).rounded(2).bg(col));
    }

    let sel_text = if let (Some(id), Some(b)) = (state.selected_node_id, state.selected_bounds) {
        fmt!(cx, "Picked #{}: {}x{} @({},{})", id, b.w, b.h, b.x, b.y)
    } else {
        crate::arena::StrRef::from("Picked: None (click element to inspect)")
    };
    let perf_line = fmt!(
        cx,
        "Avg:{}us Peak:{}us | L2:{} Pass4:{} | HotRev:{}",
        state.budget.avg_us(),
        state.budget.max_us(),
        telemetry.partial_relayout_count,
        telemetry.fast_paint_pass_count,
        state.overrides.revision()
    );

    div()
        .flex_col()
        .gap(6)
        .p(10)
        .rounded(8)
        .bg(Color::rgba(15, 23, 42, 235))
        .border(1, Color::rgba(99, 102, 241, 190))
        .child(
            cx,
            text("Velox Live DevTools (Waterfall + Hot Style Reload)")
                .size(11)
                .color(Color::rgb(165, 180, 252)),
        )
        .child(cx, bars)
        .child(cx, text(perf_line).size(10).color(Color::rgb(226, 232, 240)))
        .child(cx, text(sel_text).size(10).color(Color::rgb(125, 211, 252)))
        .into_element(cx)
}
