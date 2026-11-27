//! 静态几何与动态状态分离存储底座（`SceneStore`、`ItemStateBuffer`、耳切多边形三角剖分与精确局部脏区投射）。

use std::sync::atomic::{AtomicU32, Ordering};
use velox_engine::arena::ScratchArena;
use velox_engine::canvas_scene::{
    CanvasItemState, CanvasMeshVertex, CanvasStaticGeom, StaticSceneId, Viewport2D,
    CANVAS_STATE_DISABLED, CANVAS_STATE_FOCUSED, CANVAS_STATE_HOVERED, CANVAS_STATE_SELECTED,
};
use velox_engine::pod::CoreTheme;
use velox_engine::solver::Region;
use velox_engine::style::Color;

use crate::rtree::{FlatRTree, PolygonHitArea};

static NEXT_SCENE_ID: AtomicU32 = AtomicU32::new(1);

/// 单条静态多边形底图区域（自动执行耳切三角剖分并存入 `SceneStore::mesh_verts/mesh_indices`）。
#[derive(Clone, Debug)]
pub struct PolygonBackdrop {
    pub id: u32,
    pub points: Vec<[f32; 2]>,
    pub color: Color,
}

/// 紧凑位状态池（支持单图元状态切换与精准世界包围盒脏区收集）。
#[derive(Clone, Debug, Default)]
pub struct ItemStateBuffer {
    states: Vec<CanvasItemState>,
    revision: u32,
    dirty_indices: Vec<u32>,
    full_dirty: bool,
}

impl ItemStateBuffer {
    #[inline]
    pub fn with_len(len: usize, default_state: CanvasItemState) -> Self {
        Self {
            states: vec![default_state; len],
            revision: 1,
            dirty_indices: Vec::with_capacity(16),
            full_dirty: true,
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[CanvasItemState] {
        &self.states
    }

    #[inline]
    pub fn get(&self, idx: u32) -> Option<CanvasItemState> {
        self.states.get(idx as usize).copied()
    }

    #[inline]
    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// 修改单个图元状态并记录局部脏索引；若状态未变则返回 `false`（0 脏区、0 重绘）。
    pub fn set_state(&mut self, idx: u32, next: CanvasItemState) -> bool {
        let Some(slot) = self.states.get_mut(idx as usize) else {
            return false;
        };
        if *slot == next {
            return false;
        }
        *slot = next;
        self.revision = self.revision.wrapping_add(1);
        if self.dirty_indices.len() < 32 {
            self.dirty_indices.push(idx);
        } else {
            self.full_dirty = true;
        }
        true
    }

    /// 切换指定状态位标志（如选中/取消选中）。
    pub fn toggle_flag(&mut self, idx: u32, flag: u8) -> bool {
        let Some(mut cur) = self.get(idx) else {
            return false;
        };
        if (cur.flags & CANVAS_STATE_DISABLED) != 0 && flag == CANVAS_STATE_SELECTED {
            return false;
        }
        cur.flags ^= flag;
        self.set_state(idx, cur)
    }

    /// 设置或清除指定状态位标志。
    pub fn set_flag(&mut self, idx: u32, flag: u8, on: bool) -> bool {
        let Some(mut cur) = self.get(idx) else {
            return false;
        };
        if on {
            cur.flags |= flag;
        } else {
            cur.flags &= !flag;
        }
        self.set_state(idx, cur)
    }

    /// 清空全部指定标志位（如清除所有选区或清除旧悬停态）。
    pub fn clear_all_flag(&mut self, flag: u8) -> bool {
        let mut changed = false;
        for (i, st) in self.states.iter_mut().enumerate() {
            if (st.flags & flag) != 0 {
                st.flags &= !flag;
                changed = true;
                if self.dirty_indices.len() < 32 {
                    self.dirty_indices.push(i as u32);
                } else {
                    self.full_dirty = true;
                }
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }

    /// 统计带有指定标志位的图元数量（如已选座位数）。
    pub fn count_with_flag(&self, flag: u8) -> usize {
        self.states.iter().filter(|s| (s.flags & flag) != 0).count()
    }

    /// 将自上次帧以来累积的局部变更图元投影为精确屏幕脏矩形切片（相机未动时直连 `DamageTracker`）。
    pub fn take_dirty_screen_rects<'a>(
        &mut self,
        geoms: &[CanvasStaticGeom],
        viewport: Viewport2D,
        canvas_bounds: Region,
        camera_changed: bool,
        arena: &'a ScratchArena,
    ) -> &'a [Region] {
        let full = self.full_dirty || camera_changed || self.dirty_indices.len() > 8;
        let indices = core::mem::take(&mut self.dirty_indices);
        self.full_dirty = false;
        if full || indices.is_empty() {
            return &[];
        }
        arena.alloc_iter(indices.into_iter().filter_map(|idx| {
            let g = geoms.get(idx as usize)?;
            let r = viewport.world_aabb_to_screen_region(g.world_aabb(), canvas_bounds);
            if r.is_empty() { None } else { Some(r) }
        }))
    }
}

/// 高密度 2D 场景数据底座（SoA 静态几何 + Hilbert 扁平空间索引 + 紧凑状态池 + 静态多边形三角网格）。
#[derive(Clone, Debug)]
pub struct SceneStore {
    pub scene_id: StaticSceneId,
    pub geoms: Vec<CanvasStaticGeom>,
    pub states: ItemStateBuffer,
    /// 每个图元的短 ASCII 标签（如 `"A12"`, `"14F"`, `"B04"`，按 Hilbert 顺序与 `geoms` 1:1 对齐）。
    pub labels: Vec<String>,
    /// 每个图元对应的业务外部 ID（按 Hilbert 顺序与 `geoms` 1:1 对齐）。
    pub item_ids: Vec<u32>,
    pub rtree: FlatRTree,
    pub mesh_verts: Vec<CanvasMeshVertex>,
    pub mesh_indices: Vec<u32>,
    pub geom_revision: u32,
    pub mesh_revision: u32,
    pub hovered_idx: Option<u32>,
    pub focused_idx: Option<u32>,
    last_camera_hash: u32,
}

impl Default for SceneStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneStore {
    pub fn new() -> Self {
        Self {
            scene_id: StaticSceneId(NEXT_SCENE_ID.fetch_add(1, Ordering::Relaxed)),
            geoms: Vec::new(),
            states: ItemStateBuffer::default(),
            labels: Vec::new(),
            item_ids: Vec::new(),
            rtree: FlatRTree::default(),
            mesh_verts: Vec::new(),
            mesh_indices: Vec::new(),
            geom_revision: 1,
            mesh_revision: 1,
            hovered_idx: None,
            focused_idx: None,
            last_camera_hash: 0,
        }
    }

    /// 一次性加载整套静态图元、初始状态、短标签与异形底图多边形，自动按 Hilbert 空间曲线排序并打包 `FlatRTree`。
    pub fn load_scene(
        &mut self,
        raw_geoms: Vec<CanvasStaticGeom>,
        raw_states: Vec<CanvasItemState>,
        raw_labels: Vec<String>,
        raw_ids: Vec<u32>,
        backdrops: &[PolygonBackdrop],
    ) {
        let (mut rtree, order) = FlatRTree::build_with_hilbert_order(&raw_geoms);
        let n = order.len();
        let mut geoms = Vec::with_capacity(n);
        let mut states = Vec::with_capacity(n);
        let mut labels = Vec::with_capacity(n);
        let mut item_ids = Vec::with_capacity(n);

        for (pos, &orig) in order.iter().enumerate() {
            geoms.push(raw_geoms[orig]);
            states.push(raw_states.get(orig).copied().unwrap_or_default());
            labels.push(raw_labels.get(orig).cloned().unwrap_or_default());
            item_ids.push(raw_ids.get(orig).copied().unwrap_or(pos as u32));
        }

        // 加载期一次性耳切三角剖分生成静态多边形底图网格
        self.mesh_verts.clear();
        self.mesh_indices.clear();
        for poly in backdrops {
            rtree
                .polygons
                .push(PolygonHitArea::new(poly.id, poly.points.clone()));
            triangulate_polygon_ear_clip(
                &poly.points,
                poly.color,
                &mut self.mesh_verts,
                &mut self.mesh_indices,
            );
        }

        self.geoms = geoms;
        self.states = ItemStateBuffer {
            states,
            revision: self.states.revision.wrapping_add(1).max(1),
            dirty_indices: Vec::with_capacity(16),
            full_dirty: true,
        };
        self.labels = labels;
        self.item_ids = item_ids;
        self.rtree = rtree;
        self.geom_revision = self.geom_revision.wrapping_add(1).max(1);
        self.mesh_revision = self.mesh_revision.wrapping_add(1).max(1);
        self.hovered_idx = None;
        self.focused_idx = None;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.geoms.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.geoms.is_empty()
    }

    /// 更新当前悬停图元（自动清除旧悬停并仅将新旧两个图元加入局部脏区）。
    pub fn set_hovered(&mut self, next: Option<u32>) -> bool {
        if self.hovered_idx == next {
            return false;
        }
        if let Some(old) = self.hovered_idx {
            self.states.set_flag(old, CANVAS_STATE_HOVERED, false);
        }
        if let Some(new_i) = next {
            self.states.set_flag(new_i, CANVAS_STATE_HOVERED, true);
        }
        self.hovered_idx = next;
        true
    }

    /// 更新当前键盘焦点图元（驱动 `:focus-visible` 高亮框）。
    pub fn set_focused(&mut self, next: Option<u32>) -> bool {
        if self.focused_idx == next {
            return false;
        }
        if let Some(old) = self.focused_idx {
            self.states.set_flag(old, CANVAS_STATE_FOCUSED, false);
        }
        if let Some(new_i) = next {
            self.states.set_flag(new_i, CANVAS_STATE_FOCUSED, true);
        }
        self.focused_idx = next;
        true
    }

    /// 切换指定图元的选中态（自动记录单图元局部脏区）。
    pub fn toggle_selected(&mut self, idx: u32) -> bool {
        self.states.toggle_flag(idx, CANVAS_STATE_SELECTED)
    }

    /// 批量选中指定图元切片（供左键框选使用）。
    pub fn select_batch(&mut self, indices: &[u32], additive: bool) -> usize {
        if !additive {
            self.states.clear_all_flag(CANVAS_STATE_SELECTED);
        }
        let mut count = 0usize;
        for &idx in indices {
            if let Some(st) = self.states.get(idx) {
                if !st.is_disabled() {
                    self.states.set_flag(idx, CANVAS_STATE_SELECTED, true);
                    count += 1;
                }
            }
        }
        count
    }

    /// 消费并投射本帧精确局部屏幕脏矩形（仅当相机未移动且少量图元变脏时返回非空切片）。
    pub fn take_dirty_screen_rects<'a>(
        &mut self,
        viewport: Viewport2D,
        canvas_bounds: Region,
        arena: &'a ScratchArena,
    ) -> &'a [Region] {
        let ch = viewport.camera_hash();
        let cam_changed = ch != self.last_camera_hash;
        self.last_camera_hash = ch;
        self.states.take_dirty_screen_rects(
            &self.geoms,
            viewport,
            canvas_bounds,
            cam_changed,
            arena,
        )
    }

    /// 根据当前 `CoreTheme`（Dark / Light）生成同步的语义 8 色画布调色板（热切换主题零显存重建）。
    #[inline]
    pub fn theme_palette(theme: CoreTheme) -> [Color; 8] {
        [
            theme.primary,                             // 0: 主选区 / 标准可选座位
            theme.success,                             // 1: 优质 / VIP / 空位绿
            theme.warning,                             // 2: 紧张 / 锁定 / 特殊配载橙
            theme.danger,                              // 3: 危险品 / 不可售 / 告警红
            theme.info,                                // 4: 普通舱 / 常规分区青蓝
            Color::rgb(168, 85, 247),                  // 5: 贵宾包厢紫
            theme.muted_foreground.scale_alpha(150),   // 6: 隔断墙 / 胶囊线框灰
            theme.secondary_foreground.scale_alpha(95),// 7: 已售 / 占位低对比度灰
        ]
    }
}

/// 简单多边形耳切法三角剖分（Ear Clipping，零外部依赖，加载期一次性将凹/凸异形看台或船体轮廓转为三角形索引）。
pub fn triangulate_polygon_ear_clip(
    pts: &[[f32; 2]],
    color: Color,
    out_verts: &mut Vec<CanvasMeshVertex>,
    out_indices: &mut Vec<u32>,
) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let base_v = out_verts.len() as u32;
    for &[x, y] in pts {
        out_verts.push(CanvasMeshVertex {
            x,
            y,
            color: color.0,
        });
    }

    // 计算有向面积以确定环绕方向（保证逆时针耳切判定一致）
    let mut area2 = 0.0f32;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        area2 += a[0] * b[1] - b[0] * a[1];
    }
    let ccw = area2 >= 0.0;
    let mut rem: Vec<usize> = if ccw {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };

    #[inline]
    fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }

    #[inline]
    fn pt_in_tri(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
        let c1 = cross(a, b, p);
        let c2 = cross(b, c, p);
        let c3 = cross(c, a, p);
        c1 >= -1e-4 && c2 >= -1e-4 && c3 >= -1e-4
    }

    let mut guard = n * n;
    while rem.len() > 3 && guard > 0 {
        guard -= 1;
        let m = rem.len();
        let mut cut = false;
        for i in 0..m {
            let prev = rem[(i + m - 1) % m];
            let curr = rem[i];
            let next = rem[(i + 1) % m];
            let (a, b, c) = (pts[prev], pts[curr], pts[next]);
            if cross(a, b, c) <= 1e-5 {
                continue;
            }
            let mut has_inside = false;
            for &other in &rem {
                if other == prev || other == curr || other == next {
                    continue;
                }
                if pt_in_tri(pts[other], a, b, c) {
                    has_inside = true;
                    break;
                }
            }
            if !has_inside {
                out_indices.push(base_v + prev as u32);
                out_indices.push(base_v + curr as u32);
                out_indices.push(base_v + next as u32);
                rem.remove(i);
                cut = true;
                break;
            }
        }
        if !cut {
            break;
        }
    }
    if rem.len() == 3 {
        out_indices.push(base_v + rem[0] as u32);
        out_indices.push(base_v + rem[1] as u32);
        out_indices.push(base_v + rem[2] as u32);
    }
}
