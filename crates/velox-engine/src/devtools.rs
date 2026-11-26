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
