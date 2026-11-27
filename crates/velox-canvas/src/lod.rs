//! 语义多级细节（LOD）调度器：按元素投影像素尺寸 `projected_px` 自动切换全景色块 → 中景实例 → 近景编号+角标。

use velox_engine::arena::{ScratchArena, SmallVec};
use velox_engine::canvas_scene::{CanvasAsciiLabel, Viewport2D};
use velox_engine::style::Color;

use crate::store::SceneStore;

/// 三档语义缩放细节等级。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LodLevel {
    /// 全景视图（单图元投影像素 `< mid_threshold_px`）：突出看台/区域色块与热力分布，隐藏精细文字。
    Overview,
    /// 中景视图（`mid_threshold_px <= projected_px < detail_threshold_px`）：显示独立座位/箱体实例，隐藏编号文字。
    Mid,
    /// 近景视图（`projected_px >= detail_threshold_px`）：完整呈现座位/箱体实例 + ASCII 编号标签 + 状态角标。
    Detail,
}

impl LodLevel {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "Overview (Blocks)",
            Self::Mid => "Mid (Instances)",
            Self::Detail => "Detail (Labels + Badges)",
        }
    }
}

/// LOD 阈值配置（基于单个代表图元在屏幕上的投影逻辑像素宽度 `projected_px`）。
#[derive(Copy, Clone, Debug)]
pub struct LodPolicy {
    /// 代表图元的世界基准宽度（如标准座位宽 `14.0`）。
    pub reference_world_size: f32,
    /// 进入 `Mid` 级别的最小投影像素（默认 `5.0px`）。
    pub mid_threshold_px: f32,
    /// 进入 `Detail` 级别并开启 ASCII 编号标签的最小投影像素（默认 `18.0px`）。
    pub detail_threshold_px: f32,
    /// 单帧最多发射的可见 ASCII 标签上限（防极端全景开启标签拖慢帧率，默认 `256`）。
    pub max_visible_labels: usize,
}

impl Default for LodPolicy {
    fn default() -> Self {
        Self {
            reference_world_size: 14.0,
            mid_threshold_px: 5.0,
            detail_threshold_px: 18.0,
            max_visible_labels: 256,
        }
    }
}

impl LodPolicy {
    /// 根据当前视口计算代表元素的投影像素尺寸与对应 [`LodLevel`]。
    #[inline]
    pub fn evaluate(&self, viewport: Viewport2D) -> (LodLevel, f32) {
        let p = viewport.projected_px(self.reference_world_size);
        let level = if p >= self.detail_threshold_px {
            LodLevel::Detail
        } else if p >= self.mid_threshold_px {
            LodLevel::Mid
        } else {
            LodLevel::Overview
        };
        (level, p)
    }

    /// 在 `LodLevel::Detail` 状态下，直接从视锥裁剪区间收集视口内的短 ASCII 标签并零堆写入 `ScratchArena`。
    pub fn collect_visible_labels<'s, 'a>(
        &self,
        store: &'s SceneStore,
        viewport: Viewport2D,
        visible_ranges: &[(u32, u32)],
        visible_world: [f32; 4],
        label_color: Color,
        arena: &'a ScratchArena,
    ) -> &'a [CanvasAsciiLabel<'a>] {
        let (level, proj_px) = self.evaluate(viewport);
        if level != LodLevel::Detail || store.labels.is_empty() {
            return &[];
        }
        let font_px = (proj_px * 0.42).clamp(8.0, 22.0);
        let limit = self.max_visible_labels.min(256);
        let mut out: SmallVec<CanvasAsciiLabel<'a>, 256> = SmallVec::new();

        'outer: for &(r_start, r_end) in visible_ranges {
            let start = (r_start as usize).min(store.geoms.len());
            let end = (r_end as usize).min(store.geoms.len());
            for i in start..end {
                if out.len() >= limit {
                    break 'outer;
                }
                let g = &store.geoms[i];
                if g.cx < visible_world[0]
                    || g.cx > visible_world[2]
                    || g.cy < visible_world[1]
                    || g.cy > visible_world[3]
                {
                    continue;
                }
                let lbl = store.labels.get(i).map(|s| s.as_str()).unwrap_or("");
                if lbl.is_empty() {
                    continue;
                }
                let text: &'a str = arena.alloc_str(lbl).as_str();
                let _ = out.push(CanvasAsciiLabel {
                    wx: g.cx,
                    wy: g.cy,
                    font_px,
                    color: label_color,
                    text,
                });
            }
        }
        arena.alloc_iter(out.as_slice().iter().copied())
    }
}
