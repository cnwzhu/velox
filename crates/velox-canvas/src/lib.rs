//! # Velox Canvas (`velox-canvas`)
//!
//! 面向高密度 2D 交互场景（万级体育馆弧形选座、反鱼骨机舱选座、船舶集装箱配载图、工程/海图矢量底图）的高性能画布生态层：
//! - [`FlatRTree`]：Hilbert 曲线排序打包扁平空间索引，提供零堆分配块级视锥裁剪、OBB 逆旋转与多边形射线法精确拾取、批量框选与键盘方向键邻近焦点导航；
//! - [`SceneStore`] & [`ItemStateBuffer`]：32B 静态几何与 4B 动态状态严格分离、加载期耳切多边形三角剖分、局部状态变更精准屏幕脏区投射；
//! - [`ViewportController`]：跨端多模态手势相机（Zoom-to-Cursor、Pinch-to-Zoom、橡皮筋框选、松手惯性滑动、边界回弹、120ms Zoom Font Lock）；
//! - [`LodPolicy`] & [`LodLevel`]：按投影像素尺寸 `projected_px` 自动切换全景/中景/近景编号；
//! - [`InteractiveCanvas`] & [`Minimap`]：开箱即用的声明式画布与全景鹰眼组件，无缝衔接 `CoreTheme`、浮层世界锚点与可选 `custom-shader` WGSL 逃生舱。

pub mod lod;
pub mod minimap;
pub mod rtree;
pub mod store;
pub mod viewport;
pub mod widget;

pub use lod::{LodLevel, LodPolicy};
pub use minimap::Minimap;
pub use rtree::{
    aabb_contains_pt, aabb_intersects, aabb_union, geom_contains_point, hilbert_index_16,
    point_in_polygon, ChunkRange, FlatRTree, PolygonHitArea, RTreeNode, CHUNK_LEAF_SIZE,
};
pub use store::{triangulate_polygon_ear_clip, ItemStateBuffer, PolygonBackdrop, SceneStore};
pub use viewport::{CanvasDragMode, ViewportController};
pub use widget::{world_to_overlay_anchor, CanvasInteraction, InteractiveCanvas};

#[cfg(feature = "custom-shader")]
pub use velox_engine::renderers::wgpu::{
    register_custom_shader, unregister_custom_shader, CustomShaderPrimitive, CustomShaderViewport,
};

#[cfg(test)]
mod tests {
    use super::*;
    use velox_engine::arena::{ScratchArena, SmallVec};
    use velox_engine::canvas_scene::{
        CanvasItemState, CanvasStaticGeom, CANVAS_STATE_SELECTED,
    };
    use velox_engine::solver::Region;
    use velox_engine::style::Color;

    #[test]
    fn test_flat_rtree_12000_rotated_seats_cull_hit_marquee_and_local_dirty() {
        let mut geoms = Vec::with_capacity(12_000);
        let mut states = Vec::with_capacity(12_000);
        let mut labels = Vec::with_capacity(12_000);
        let mut ids = Vec::with_capacity(12_000);

        for row in 0..100 {
            for col in 0..120 {
                let idx = row * 120 + col;
                let angle = ((col as f32) - 60.0) * 0.012;
                let (sin_a, cos_a) = angle.sin_cos();
                geoms.push(CanvasStaticGeom::rounded_rect_rotated(
                    col as f32 * 18.0,
                    row as f32 * 18.0,
                    14.0,
                    12.0,
                    cos_a,
                    sin_a,
                    3.0,
                ));
                states.push(CanvasItemState::new((row % 5) as u8));
                labels.push(format!("R{}C{}", row, col));
                ids.push(idx as u32);
            }
        }

        let backdrop = PolygonBackdrop {
            id: 9001,
            points: vec![
                [-20.0, -20.0],
                [2200.0, -20.0],
                [2200.0, 1850.0],
                [-20.0, 1850.0],
            ],
            color: Color::rgba(30, 41, 59, 180),
        };

        let mut store = SceneStore::new();
        store.load_scene(geoms, states, labels, ids, &[backdrop]);
        assert_eq!(store.len(), 12_000);
        assert_eq!(store.mesh_indices.len(), 6);

        // 1. 视锥块级裁剪应大幅剔除不可见区块
        let arena = ScratchArena::new();
        let small_vis = [0.0, 0.0, 180.0, 180.0];
        let ranges = store.rtree.cull_chunks(small_vis, &arena);
        let visible_count: u32 = ranges.iter().map(|&(s, e)| e - s).sum();
        assert!(visible_count > 0 && visible_count < 1500, "got {visible_count}");

        // 2. 精确 OBB 拾取与多边形看台拾取
        let target_g = store.geoms[0];
        let hit = store.rtree.hit_test(&store.geoms, target_g.cx, target_g.cy);
        assert_eq!(hit, Some(0));
        assert_eq!(store.rtree.hit_test_polygon(100.0, 100.0), Some(9001));

        // 3. 批量框选
        let mut picked: SmallVec<u32, 64> = SmallVec::new();
        store.rtree.query_marquee(&store.geoms, [0.0, 0.0, 80.0, 80.0], &mut picked);
        assert!(picked.len() >= 9 && picked.len() <= 36);

        // 4. 单图元修改状态仅投射几十像素局部脏区
        let mut ctrl = ViewportController::new();
        ctrl.fit_bounds(store.rtree.world_bounds, 800.0, 600.0, 20.0);
        let canvas_bounds = Region::new(100, 80, 800, 600);
        // 首帧相机变动返回空（触发整画布绘制）
        let r0 = store.take_dirty_screen_rects(ctrl.viewport, canvas_bounds, &arena);
        assert!(r0.is_empty());
        // 相机不变，单点选中 1 个座位 → 仅投射单个微小屏幕矩形
        assert!(store.toggle_selected(0));
        assert_eq!(store.states.count_with_flag(CANVAS_STATE_SELECTED), 1);
        let r1 = store.take_dirty_screen_rects(ctrl.viewport, canvas_bounds, &arena);
        assert_eq!(r1.len(), 1);
        assert!(r1[0].area() > 0 && r1[0].area() < 2_500, "dirty area={}", r1[0].area());

        // 5. Zoom-to-Cursor 保持锚点世界坐标不变 + 120ms Zoom Font Lock
        let before = ctrl.viewport.local_to_world(400.0, 300.0);
        assert!(ctrl.handle_wheel(400, 300, -20, 1000));
        let after = ctrl.viewport.local_to_world(400.0, 300.0);
        assert!((before.0 - after.0).abs() < 1e-2 && (before.1 - after.1).abs() < 1e-2);
        assert!(ctrl.is_zooming(1050));
        assert!(!ctrl.is_zooming(1150));

        // 6. LOD 三档自动评估
        let lod = LodPolicy::default();
        ctrl.viewport.zoom = 0.2;
        assert_eq!(lod.evaluate(ctrl.viewport).0, LodLevel::Overview);
        ctrl.viewport.zoom = 0.8;
        assert_eq!(lod.evaluate(ctrl.viewport).0, LodLevel::Mid);
        ctrl.viewport.zoom = 2.0;
        assert_eq!(lod.evaluate(ctrl.viewport).0, LodLevel::Detail);
    }
}
