//! `InteractiveCanvas` 声明式画布组件：
//! 封装 `SceneStore` + `ViewportController` + `LodPolicy` 为标准 Velox `Widget`，
//! 无缝桥接滚轮 Zoom-to-Cursor、双指捏合、拖拽平移/框选、点选拾取、键盘方向键焦点导航、
//! 世界坐标浮层锚定（`Tooltip` / `Popover` / `ContextMenu`）以及可选 `custom-shader` 背景/前景挂点。

use velox_engine::canvas_scene::{
    scene_canvas, CustomShaderCall, SceneCanvasSpec, Viewport2D,
};
use velox_engine::prelude::*;

use crate::lod::LodPolicy;
use crate::store::SceneStore;
use crate::viewport::{CanvasDragMode, ViewportController};

/// 画布交互事件枚举（回传给业务视图或由内置控制器自动处理）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CanvasInteraction {
    /// 悬停在指定图元索引（`None` 表示悬停空白区或离开画布）。
    Hover(Option<u32>),
    /// 点击命中指定图元索引（`None` 表示点击空白区）。
    ClickItem(Option<u32>),
    /// 右击命中指定图元索引与窗口坐标 `(item, wx, wy)`（便于唤起上下文菜单）。
    ContextMenu(Option<u32>, i16, i16),
    /// 滚轮/捏合缩放或拖拽平移相机后通知视图同步。
    CameraChanged,
    /// 完成框选，携带框选命中的图元数量。
    MarqueeSelected(usize),
}

/// 将任意世界坐标 `(wx, wy)` 转换为相对画布左上角的局部像素坐标 `(lx, ly)`，
/// 便于把 `Tooltip` / `Popover` / `ContextMenu` 精准吸附在选中座位或集装箱上方。
#[inline]
pub fn world_to_overlay_anchor(
    viewport: Viewport2D,
    wx: f32,
    wy: f32,
    canvas_w: i16,
    canvas_h: i16,
    popup_w: i16,
    popup_h: i16,
) -> (i16, i16) {
    let (lx, ly) = viewport.world_to_local(wx, wy);
    let ax = (lx.round() as i32 - popup_w as i32 / 2)
        .clamp(6, (canvas_w as i32 - popup_w as i32 - 6).max(6)) as i16;
    let ay = (ly.round() as i32 - popup_h as i32 - 12)
        .clamp(6, (canvas_h as i32 - popup_h as i32 - 6).max(6)) as i16;
    (ax, ay)
}

/// 高密度 2D 交互式画布标准组件（`InteractiveCanvas`）。
///
/// 生命周期 `'s` 借用业务状态（`SceneStore`、`ViewportController`），`'a` 为帧 `FrameScope` / `ScratchArena` 生命周期；
/// 两者解耦后，业务视图可在 `View::render(&mut self, cx: &'a mut Scope<Self>)` 中直接传入 `&self.store` 与 `&self.controller`。
pub struct InteractiveCanvas<'s, 'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub store: &'s SceneStore,
    pub controller: &'s ViewportController,
    pub lod: LodPolicy,
    pub bg: Option<Color>,
    pub palette: Option<&'s [Color]>,
    pub dirty_screen_rects: &'s [Region],
    pub bg_shaders: &'a [CustomShaderCall<'a>],
    pub fg_shaders: &'a [CustomShaderCall<'a>],
    pub software_bg_fallback: Option<&'a dyn Fn(&mut dyn SurfacePainter, Viewport2D)>,
    pub software_fg_fallback: Option<&'a dyn Fn(&mut dyn SurfacePainter, Viewport2D)>,
    pub on_wheel: Option<&'a dyn Fn(WheelEvent, &mut ActionScope)>,
    pub on_pinch: Option<&'a dyn Fn(PinchEvent, &mut ActionScope)>,
    pub on_drag: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    pub on_pointer_move: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    pub on_hover: Option<&'a dyn Fn(bool, &mut ActionScope)>,
    pub on_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
    pub on_secondary_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
    pub on_key_nav: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    pub overlay_children: Vec<Element<'a>>,
}

impl<'s, 'a> InteractiveCanvas<'s, 'a> {
    pub fn new(
        id: u32,
        width: i16,
        height: i16,
        store: &'s SceneStore,
        controller: &'s ViewportController,
    ) -> Self {
        Self {
            id,
            width,
            height,
            store,
            controller,
            lod: LodPolicy::default(),
            bg: None,
            palette: None,
            dirty_screen_rects: &[],
            bg_shaders: &[],
            fg_shaders: &[],
            software_bg_fallback: None,
            software_fg_fallback: None,
            on_wheel: None,
            on_pinch: None,
            on_drag: None,
            on_pointer_move: None,
            on_hover: None,
            on_click: None,
            on_secondary_click: None,
            on_key_nav: None,
            overlay_children: Vec::new(),
        }
    }

    #[inline]
    pub fn lod(mut self, lod: LodPolicy) -> Self {
        self.lod = lod;
        self
    }

    #[inline]
    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    #[inline]
    pub fn palette(mut self, palette: &'s [Color]) -> Self {
        self.palette = Some(palette);
        self
    }

    #[inline]
    pub fn dirty_screen_rects(mut self, rects: &'s [Region]) -> Self {
        self.dirty_screen_rects = rects;
        self
    }

    /// 挂载背景自定义 WGSL 着色器调用（开启 `custom-shader` 时在 GPU 端生效）。
    #[inline]
    pub fn bg_shaders(mut self, calls: &'a [CustomShaderCall<'a>]) -> Self {
        self.bg_shaders = calls;
        self
    }

    /// 挂载前景自定义 WGSL 着色器调用（开启 `custom-shader` 时在 GPU 端生效）。
    #[inline]
    pub fn fg_shaders(mut self, calls: &'a [CustomShaderCall<'a>]) -> Self {
        self.fg_shaders = calls;
        self
    }

    /// 注册软件光栅模式下的背景降级绘制回调。
    #[inline]
    pub fn software_bg_fallback(
        mut self,
        cb: &'a dyn Fn(&mut dyn SurfacePainter, Viewport2D),
    ) -> Self {
        self.software_bg_fallback = Some(cb);
        self
    }

    /// 注册软件光栅模式下的前景降级绘制回调。
    #[inline]
    pub fn software_fg_fallback(
        mut self,
        cb: &'a dyn Fn(&mut dyn SurfacePainter, Viewport2D),
    ) -> Self {
        self.software_fg_fallback = Some(cb);
        self
    }

    #[inline]
    pub fn on_wheel(mut self, cb: &'a dyn Fn(WheelEvent, &mut ActionScope)) -> Self {
        self.on_wheel = Some(cb);
        self
    }

    #[inline]
    pub fn on_pinch(mut self, cb: &'a dyn Fn(PinchEvent, &mut ActionScope)) -> Self {
        self.on_pinch = Some(cb);
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
    pub fn on_hover(mut self, cb: &'a dyn Fn(bool, &mut ActionScope)) -> Self {
        self.on_hover = Some(cb);
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

    #[inline]
    pub fn on_key_nav(mut self, cb: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_nav = Some(cb);
        self
    }

    /// 在画布上方叠放子浮层元素（如世界坐标锚定的 `Tooltip`、`Popover`、`ContextMenu` 或 `Minimap` 鹰眼）。
    #[inline]
    pub fn overlay(mut self, cx: &'a FrameScope, child: impl IntoElement<'a>) -> Self {
        self.overlay_children.push(child.into_element(cx));
        self
    }
}

impl<'s, 'a> Widget<'a> for InteractiveCanvas<'s, 'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = *cx.theme();
        let vp = self.controller.viewport;
        let arena = cx.arena();

        // 1. 零堆视锥块级裁剪
        let vis_world = vp.visible_world_bounds(self.width as f32, self.height as f32);
        let visible_ranges = self.store.rtree.cull_chunks(vis_world, arena);

        // 2. 语义 LOD 近景 ASCII 标签收集
        let label_col = if theme.background.r() < 128 {
            Color::WHITE
        } else {
            Color::rgb(15, 23, 42)
        };
        let labels = self.lod.collect_visible_labels(
            self.store,
            vp,
            visible_ranges,
            vis_world,
            label_col,
            arena,
        );

        // 3. 绑定 CoreTheme 语义调色板，并将瞬态帧切片拷入帧 ScratchArena（彻底解耦 `&self` 生命周期 `'s` 与 `'a`）
        let palette: &'a [Color] = if let Some(p) = self.palette {
            arena.alloc_iter(p.iter().copied())
        } else {
            arena.alloc_iter(SceneStore::theme_palette(theme))
        };
        let geoms = arena.alloc_iter(self.store.geoms.iter().copied());
        let states = arena.alloc_iter(self.store.states.as_slice().iter().copied());
        let mesh_verts = arena.alloc_iter(self.store.mesh_verts.iter().copied());
        let mesh_indices = arena.alloc_iter(self.store.mesh_indices.iter().copied());
        let dirty_screen_rects: &'a [Region] = if self.dirty_screen_rects.is_empty() {
            &[]
        } else {
            arena.alloc_iter(self.dirty_screen_rects.iter().copied())
        };

        let bg = self.bg.unwrap_or(theme.background);
        let zooming = self.controller.is_zooming(cx.now_ms());
        if zooming {
            cx.request_frame();
        }

        let spec = SceneCanvasSpec {
            id: self.id ^ 0x5C4E_0001,
            width: self.width,
            height: self.height,
            opacity: 255,
            bg,
            viewport: vp,
            zooming,
            scene_id: self.store.scene_id,
            geom_revision: self.store.geom_revision,
            state_revision: self.store.states.revision(),
            mesh_revision: self.store.mesh_revision,
            palette,
            geoms,
            states,
            visible_ranges,
            mesh_verts,
            mesh_indices,
            labels,
            marquee: self.controller.marquee,
            marquee_color: theme.primary,
            dirty_screen_rects,
            bg_shaders: self.bg_shaders,
            fg_shaders: self.fg_shaders,
            software_bg_fallback: self.software_bg_fallback,
            software_fg_fallback: self.software_fg_fallback,
        };

        let cursor = match self.controller.drag_mode {
            CanvasDragMode::Pan => CursorShape::Grab,
            CanvasDragMode::Marquee => CursorShape::Crosshair,
        };

        let mut root = div()
            .id(self.id)
            .w(self.width)
            .h(self.height)
            .rounded(8)
            .border(1, theme.border)
            .overflow_hidden()
            .focusable()
            .cursor(cursor)
            .child(cx, scene_canvas(spec));

        if let Some(cb) = self.on_wheel {
            root = root.on_wheel(cx, cb);
        }
        if let Some(cb) = self.on_pinch {
            root = root.on_pinch(cx, cb);
        }
        if let Some(cb) = self.on_drag {
            root = root.on_drag(cx, cb);
        }
        if let Some(cb) = self.on_pointer_move {
            root = root.on_pointer_move(cx, cb);
        }
        if let Some(cb) = self.on_hover {
            root = root.on_hover(cx, cb);
        }
        if let Some(cb) = self.on_click {
            root = root.on_click(cx, cb);
        }
        if let Some(cb) = self.on_secondary_click {
            root = root.on_secondary_click(cx, cb);
        }
        if let Some(cb) = self.on_key_nav {
            root = root.on_key_nav(cb);
        }

        for child in self.overlay_children {
            root = root.child(cx, child);
        }

        root.into_element(cx)
    }
}
