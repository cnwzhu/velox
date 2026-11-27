//! `Minimap` 鹰眼缩略图小部件：共享同一个 `SceneStore` 与 `Viewport2D`，
//! 在主画布角落渲染全景底图与当前视口框，支持点击/拖拽瞬间跳转主相机。

use velox_engine::canvas_scene::{scene_canvas, SceneCanvasSpec, StaticSceneId, Viewport2D};
use velox_engine::prelude::*;

use crate::store::SceneStore;

/// 交互式全景鹰眼小部件（`Minimap`）。
pub struct Minimap<'s, 'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub store: &'s SceneStore,
    pub main_viewport: Viewport2D,
    pub main_canvas_size: (i16, i16),
    pub on_jump_world: Option<&'a dyn Fn((f32, f32), &mut ActionScope)>,
}

impl<'s, 'a> Minimap<'s, 'a> {
    pub fn new(
        id: u32,
        width: i16,
        height: i16,
        store: &'s SceneStore,
        main_viewport: Viewport2D,
        main_canvas_size: (i16, i16),
    ) -> Self {
        Self {
            id,
            width,
            height,
            store,
            main_viewport,
            main_canvas_size,
            on_jump_world: None,
        }
    }

    /// 绑定点击/拖拽鹰眼时的相机跳转回调：参数为目标世界坐标 `(wx, wy)`。
    #[inline]
    pub fn on_jump_world(mut self, cb: &'a dyn Fn((f32, f32), &mut ActionScope)) -> Self {
        self.on_jump_world = Some(cb);
        self
    }

    fn compute_mini_viewport(&self) -> Viewport2D {
        let wb = self.store.rtree.world_bounds;
        let bw = (wb[2] - wb[0]).max(1.0);
        let bh = (wb[3] - wb[1]).max(1.0);
        let pad = 8.0f32;
        let aw = (self.width as f32 - pad * 2.0).max(8.0);
        let ah = (self.height as f32 - pad * 2.0).max(8.0);
        let z = (aw / bw).min(ah / bh);
        let cx = (wb[0] + wb[2]) * 0.5;
        let cy = (wb[1] + wb[3]) * 0.5;
        Viewport2D::new(self.width as f32 * 0.5 - cx * z, self.height as f32 * 0.5 - cy * z, z)
    }
}

impl<'s, 'a> Widget<'a> for Minimap<'s, 'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = *cx.theme();
        let arena = cx.arena();
        let mini_vp = self.compute_mini_viewport();
        let palette = arena.alloc_iter(SceneStore::theme_palette(theme));
        let geoms = arena.alloc_iter(self.store.geoms.iter().copied());
        let states = arena.alloc_iter(self.store.states.as_slice().iter().copied());
        let mesh_verts = arena.alloc_iter(self.store.mesh_verts.iter().copied());
        let mesh_indices = arena.alloc_iter(self.store.mesh_indices.iter().copied());

        // 视口框映射到鹰眼局部坐标
        let main_vis = self.main_viewport.visible_world_bounds(
            self.main_canvas_size.0 as f32,
            self.main_canvas_size.1 as f32,
        );
        let (rx0, ry0) = mini_vp.world_to_local(main_vis[0], main_vis[1]);
        let (rx1, ry1) = mini_vp.world_to_local(main_vis[2], main_vis[3]);
        let vx = (rx0.min(rx1).round() as i32).clamp(0, self.width as i32 - 4) as i16;
        let vy = (ry0.min(ry1).round() as i32).clamp(0, self.height as i32 - 4) as i16;
        let vw = ((rx1 - rx0).abs().round() as i32).clamp(8, self.width as i32 - vx as i32) as i16;
        let vh = ((ry1 - ry0).abs().round() as i32).clamp(8, self.height as i32 - vy as i32) as i16;

        // 鹰眼仅渲染底图与轻量视口缩略图（共享同一 StaticSceneId 显存几何缓冲，零重复上传）
        let spec = SceneCanvasSpec {
            id: self.id ^ 0x4D49_4E49,
            width: self.width,
            height: self.height,
            opacity: 235,
            bg: theme.popover.scale_alpha(230),
            viewport: mini_vp,
            zooming: false,
            scene_id: StaticSceneId(self.store.scene_id.0),
            geom_revision: self.store.geom_revision,
            state_revision: self.store.states.revision(),
            mesh_revision: self.store.mesh_revision,
            palette,
            geoms,
            states,
            visible_ranges: &[],
            mesh_verts,
            mesh_indices,
            labels: &[],
            marquee: None,
            marquee_color: theme.primary,
            dirty_screen_rects: &[],
            bg_shaders: &[],
            fg_shaders: &[],
            software_bg_fallback: None,
            software_fg_fallback: None,
        };

        let mut box_div = div()
            .id(self.id)
            .w(self.width)
            .h(self.height)
            .rounded(6)
            .border(1, theme.border)
            .bg(theme.popover.scale_alpha(230))
            .overflow_hidden()
            .cursor(CursorShape::Pointer)
            .occlude_hit()
            .child(cx, scene_canvas(spec))
            .child(
                cx,
                div()
                    .pos(vx, vy)
                    .w(vw)
                    .h(vh)
                    .rounded(2)
                    .bg(theme.primary.scale_alpha(44))
                    .border(1, theme.primary),
            );

        if let Some(jump_cb) = self.on_jump_world {
            let drag_wrap = arena.alloc(move |ev: DragEvent, acx: &mut ActionScope| {
                let (wx, wy) = mini_vp.local_to_world(ev.local_x as f32, ev.local_y as f32);
                jump_cb((wx, wy), acx);
            });
            box_div = box_div.on_drag(cx, drag_wrap);
        }

        box_div.into_element(cx)
    }
}
