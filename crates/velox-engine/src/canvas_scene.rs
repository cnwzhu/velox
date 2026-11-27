//! 高密度 2D 交互式场景画布内核原语（`Viewport2D`、`CanvasStaticGeom`、`CanvasItemState`、`SceneCanvasSpec`）。
//!
//! 本模块保持 100% `#![no_std]` 零全局堆分配兼容：
//! - 静态几何（`CanvasStaticGeom`，固定 32 Bytes）与动态状态（`CanvasItemState`，固定 4 Bytes）严格分离；
//! - 通过 8B `ScratchArena` 引用挂入 `Element::SceneCanvas` / `DrawPayload::SceneCanvas`，对通用 UI 零膨胀；
//! - 支持 OBB 任意角旋转圆角矩形 SDF、圆形 SDF、胶囊线段 SDF、静态多边形底图三角网格、ASCII 直通编号标签、
//!   橡皮筋框选、局部屏幕脏矩形精准透传，以及可选 `custom-shader` WGSL 背景/前景渲染钩子与软件降级回调。

use crate::display::PixelFormat;
use crate::element::{Element, IntoElement};
use crate::pod::FrameScope;
use crate::renderers::software::rasterize_text;
use crate::sdf::SurfacePainter;
use crate::solver::Region;
use crate::style::Color;
use crate::svg::math::{floor_f32, sqrt_f32};

#[inline(always)]
fn canon_f32(v: f32) -> u32 {
    if v == 0.0 {
        0
    } else {
        v.to_bits()
    }
}

/// 静态场景几何批次唯一句柄（GPU 端按 `(StaticSceneId, geom_revision)` 常驻显存 `VertexBuffer`）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct StaticSceneId(pub u32);

/// 自定义 WGSL 着色器句柄（配合 `custom-shader` feature 使用，保持 `DrawOp: Copy + Eq`）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct CustomShaderId(pub u32);

/// 自定义着色器在画布内的挂载图层顺序。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum CustomShaderLayer {
    /// 位于静态底图与实例图元下方（如海图水波纹、场馆声波热力底图）。
    #[default]
    Background,
    /// 位于所有座位/实例与文字上方（如聚光灯扫光、雷达高亮叠加）。
    Foreground,
}

/// 画布 2D 仿射视口变换：`screen = world * zoom + offset`（逻辑像素坐标系，相对于画布左上角）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Viewport2D {
    pub offset_x: f32,
    pub offset_y: f32,
    pub zoom: f32,
}

impl Default for Viewport2D {
    #[inline(always)]
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            zoom: 1.0,
        }
    }
}

impl PartialEq for Viewport2D {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        canon_f32(self.offset_x) == canon_f32(other.offset_x)
            && canon_f32(self.offset_y) == canon_f32(other.offset_y)
            && canon_f32(self.zoom) == canon_f32(other.zoom)
    }
}

impl Eq for Viewport2D {}

impl Viewport2D {
    #[inline(always)]
    pub const fn new(offset_x: f32, offset_y: f32, zoom: f32) -> Self {
        Self {
            offset_x,
            offset_y,
            zoom,
        }
    }

    /// 世界坐标转画布局部逻辑像素坐标。
    #[inline(always)]
    pub fn world_to_local(&self, wx: f32, wy: f32) -> (f32, f32) {
        (wx * self.zoom + self.offset_x, wy * self.zoom + self.offset_y)
    }

    /// 画布局部逻辑像素坐标转世界坐标。
    #[inline(always)]
    pub fn local_to_world(&self, lx: f32, ly: f32) -> (f32, f32) {
        let inv = if self.zoom.abs() > 1e-6 {
            1.0 / self.zoom
        } else {
            1.0
        };
        ((lx - self.offset_x) * inv, (ly - self.offset_y) * inv)
    }

    /// 世界尺寸对应的当前屏幕逻辑像素尺寸（供语义 LOD 分级判断）。
    #[inline(always)]
    pub fn projected_px(&self, world_size: f32) -> f32 {
        world_size * self.zoom
    }

    /// 计算画布局部视口 `(w, h)` 对应的世界空间可见包围盒 `[min_x, min_y, max_x, max_y]`。
    #[inline]
    pub fn visible_world_bounds(&self, canvas_w: f32, canvas_h: f32) -> [f32; 4] {
        let (x0, y0) = self.local_to_world(0.0, 0.0);
        let (x1, y1) = self.local_to_world(canvas_w, canvas_h);
        [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
    }

    /// 将世界坐标包围盒 `[min_x, min_y, max_x, max_y]` 投影为绝对屏幕 `Region`（自动外扩 2px 抗锯齿余量）。
    #[inline]
    pub fn world_aabb_to_screen_region(&self, aabb: [f32; 4], canvas_bounds: Region) -> Region {
        let (lx0, ly0) = self.world_to_local(aabb[0], aabb[1]);
        let (lx1, ly1) = self.world_to_local(aabb[2], aabb[3]);
        let min_x = floor_f32(lx0.min(lx1)) as i32 - 2;
        let min_y = floor_f32(ly0.min(ly1)) as i32 - 2;
        let max_x = floor_f32(lx0.max(lx1)) as i32 + 3;
        let max_y = floor_f32(ly0.max(ly1)) as i32 + 3;
        let sx = (canvas_bounds.x as i32 + min_x).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let sy = (canvas_bounds.y as i32 + min_y).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let sw = (max_x - min_x).clamp(0, i16::MAX as i32) as i16;
        let sh = (max_y - min_y).clamp(0, i16::MAX as i32) as i16;
        Region::new(sx, sy, sw, sh).intersect(canvas_bounds)
    }

    /// 计算当前相机状态的 32 位哈希指纹。
    #[inline]
    pub fn camera_hash(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for bits in [
            canon_f32(self.offset_x),
            canon_f32(self.offset_y),
            canon_f32(self.zoom),
        ] {
            for b in bits.to_le_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 静态画布图元几何类型。
#[repr(u32)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CanvasPrimitiveKind {
    /// 轴对齐或任意旋转圆角矩形（OBB SDF）。
    #[default]
    RoundedRect = 0,
    /// 解析 SDF 圆形。
    Circle = 1,
    /// 胶囊体线段 SDF（以 `(cx, cy)` 为中心、长度 `w`、直径 `h`、朝向 `(cos, sin)`，适用于舱口围板/隔断墙线框）。
    Capsule = 2,
}

/// 紧凑静态几何实例（严格 `32 Bytes`，GPU 常驻 `VertexBuffer` 零每帧重传）。
///
/// - 对于 `RoundedRect` / `Circle`：`(cx, cy)` 为世界中心，`(w, h)` 为宽高，`(cos, sin)` 为旋转单位向量；
/// - 对于 `Capsule`：`(cx, cy)` 为线段中点，`w` 为线段长度，`h` 为线宽直径，`(cos, sin)` 为线段方向单位向量。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CanvasStaticGeom {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub cos: f32,
    pub sin: f32,
    pub corner_r: f32,
    pub kind_flags: u32,
}

const _: () = assert!(core::mem::size_of::<CanvasStaticGeom>() == 32);

impl Default for CanvasStaticGeom {
    #[inline(always)]
    fn default() -> Self {
        Self {
            cx: 0.0,
            cy: 0.0,
            w: 16.0,
            h: 16.0,
            cos: 1.0,
            sin: 0.0,
            corner_r: 3.0,
            kind_flags: 0,
        }
    }
}

impl CanvasStaticGeom {
    /// 构造带任意旋转角 `(cos, sin)` 的圆角矩形（OBB）图元。
    #[inline(always)]
    pub const fn rounded_rect_rotated(
        cx: f32,
        cy: f32,
        w: f32,
        h: f32,
        cos: f32,
        sin: f32,
        corner_r: f32,
    ) -> Self {
        Self {
            cx,
            cy,
            w,
            h,
            cos,
            sin,
            corner_r,
            kind_flags: CanvasPrimitiveKind::RoundedRect as u32,
        }
    }

    /// 构造圆形图元。
    #[inline(always)]
    pub const fn circle(cx: f32, cy: f32, diameter: f32) -> Self {
        Self {
            cx,
            cy,
            w: diameter,
            h: diameter,
            cos: 1.0,
            sin: 0.0,
            corner_r: diameter * 0.5,
            kind_flags: CanvasPrimitiveKind::Circle as u32,
        }
    }

    /// 构造胶囊线段 SDF 图元（从 `(x0, y0)` 到 `(x1, y1)`，线宽 `thickness`）。
    #[inline]
    pub fn capsule(x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32) -> Self {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = sqrt_f32(dx * dx + dy * dy);
        let (cos, sin) = if len > 1e-5 {
            (dx / len, dy / len)
        } else {
            (1.0, 0.0)
        };
        Self {
            cx: (x0 + x1) * 0.5,
            cy: (y0 + y1) * 0.5,
            w: len,
            h: thickness.max(1.0),
            cos,
            sin,
            corner_r: thickness * 0.5,
            kind_flags: CanvasPrimitiveKind::Capsule as u32,
        }
    }

    /// 计算该旋转图元在世界空间中的精确外接轴对齐包围盒 `[min_x, min_y, max_x, max_y]`。
    #[inline]
    pub fn world_aabb(&self) -> [f32; 4] {
        let hw = if self.kind_flags == CanvasPrimitiveKind::Capsule as u32 {
            (self.w + self.h) * 0.5
        } else {
            self.w * 0.5
        };
        let hh = self.h * 0.5;
        let ac = self.cos.abs();
        let as_ = self.sin.abs();
        let ex = hw * ac + hh * as_;
        let ey = hw * as_ + hh * ac;
        [self.cx - ex, self.cy - ey, self.cx + ex, self.cy + ey]
    }
}

/// 图元动态状态位标志常量。
pub const CANVAS_STATE_HOVERED: u8 = 1 << 0;
pub const CANVAS_STATE_SELECTED: u8 = 1 << 1;
pub const CANVAS_STATE_DISABLED: u8 = 1 << 2;
pub const CANVAS_STATE_LOCKED: u8 = 1 << 3;
pub const CANVAS_STATE_FOCUSED: u8 = 1 << 4;

/// 紧凑动态图元状态（严格 `4 Bytes`，GPU `StorageBuffer` 局部 `queue.write_buffer` 极速更新）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct CanvasItemState {
    /// 调色板颜色索引（`0..32`，指向绑定自 `CoreTheme` 的语义调色板）。
    pub palette_idx: u8,
    /// 状态位掩码（`CANVAS_STATE_HOVERED` / `SELECTED` / `DISABLED` / `LOCKED` / `FOCUSED`）。
    pub flags: u8,
    /// 角标位掩码（如右上角圆点指示、锁标、VIP 标记）。
    pub badge_mask: u8,
    /// LOD 分组编号（`0` 始终可见，`1+` 由语义缩放等级控制显隐）。
    pub lod_group: u8,
}

const _: () = assert!(core::mem::size_of::<CanvasItemState>() == 4);

impl CanvasItemState {
    #[inline(always)]
    pub const fn new(palette_idx: u8) -> Self {
        Self {
            palette_idx,
            flags: 0,
            badge_mask: 0,
            lod_group: 0,
        }
    }

    #[inline(always)]
    pub const fn with_flags(mut self, flags: u8) -> Self {
        self.flags = flags;
        self
    }

    #[inline(always)]
    pub const fn with_badge(mut self, badge_mask: u8) -> Self {
        self.badge_mask = badge_mask;
        self
    }

    #[inline(always)]
    pub const fn is_selected(self) -> bool {
        (self.flags & CANVAS_STATE_SELECTED) != 0
    }

    #[inline(always)]
    pub const fn is_hovered(self) -> bool {
        (self.flags & CANVAS_STATE_HOVERED) != 0
    }

    #[inline(always)]
    pub const fn is_disabled(self) -> bool {
        (self.flags & CANVAS_STATE_DISABLED) != 0
    }

    #[inline(always)]
    pub const fn is_locked(self) -> bool {
        (self.flags & CANVAS_STATE_LOCKED) != 0
    }

    #[inline(always)]
    pub const fn is_focused(self) -> bool {
        (self.flags & CANVAS_STATE_FOCUSED) != 0
    }

    #[inline(always)]
    pub const fn pack_u32(self) -> u32 {
        (self.palette_idx as u32)
            | ((self.flags as u32) << 8)
            | ((self.badge_mask as u32) << 16)
            | ((self.lod_group as u32) << 24)
    }
}

/// 静态多边形底图三角网格顶点（`12 Bytes`，一次性加载期耳切剖分生成，用于异形看台/船体/机身底图）。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct CanvasMeshVertex {
    pub x: f32,
    pub y: f32,
    /// RGBA8888 打包色（`0xAARRGGBB` 与 `Color.0` 一致）。
    pub color: u32,
}

const _: () = assert!(core::mem::size_of::<CanvasMeshVertex>() == 12);

/// 近景 LOD 直通 ASCII 标签（绕过复杂排版引擎，直接查表输出座位号/集装箱编号）。
#[derive(Copy, Clone, Debug)]
pub struct CanvasAsciiLabel<'a> {
    pub wx: f32,
    pub wy: f32,
    pub font_px: f32,
    pub color: Color,
    pub text: &'a str,
}

/// 自定义 WGSL 着色器单次调用描述符（纯 `Copy`，驻留 `ScratchArena`）。
#[derive(Copy, Clone)]
pub struct CustomShaderCall<'a> {
    pub id: CustomShaderId,
    pub world_bounds: [f32; 4],
    pub state_hash: u64,
    pub uniform_bytes: &'a [u8],
}

impl<'a> PartialEq for CustomShaderCall<'a> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.state_hash == other.state_hash
            && canon_f32(self.world_bounds[0]) == canon_f32(other.world_bounds[0])
            && canon_f32(self.world_bounds[1]) == canon_f32(other.world_bounds[1])
            && canon_f32(self.world_bounds[2]) == canon_f32(other.world_bounds[2])
            && canon_f32(self.world_bounds[3]) == canon_f32(other.world_bounds[3])
            && self.uniform_bytes == other.uniform_bytes
    }
}

impl<'a> Eq for CustomShaderCall<'a> {}

/// 高密度交互式场景画布绘制载荷（分配在 `ScratchArena`，以 8B 引用挂入 `Element::SceneCanvas` / `DrawPayload::SceneCanvas`）。
#[derive(Copy, Clone)]
pub struct SceneCanvasSpec<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub opacity: u8,
    pub bg: Color,
    pub viewport: Viewport2D,
    /// `true` 表示当前处于连续手势缩放中（启用 Zoom Font Lock 锁定离散基准字号防图集抖动）。
    pub zooming: bool,
    pub scene_id: StaticSceneId,
    pub geom_revision: u32,
    pub state_revision: u32,
    pub mesh_revision: u32,
    /// 语义调色板（最多 32 色，绑定 `CoreTheme` 实现 Dark/Light 实时热切换）。
    pub palette: &'a [Color],
    /// 静态几何数组（GPU 仅在 `geom_revision` 变更时上传常驻 `VertexBuffer`）。
    pub geoms: &'a [CanvasStaticGeom],
    /// 动态状态数组（与 `geoms` 1:1 索引对齐）。
    pub states: &'a [CanvasItemState],
    /// 经空间索引裁剪后的可见连续区间 `(start, end)`（空时回退渲染全量 `0..geoms.len()`）。
    pub visible_ranges: &'a [(u32, u32)],
    /// 静态多边形底图网格顶点与三角形索引。
    pub mesh_verts: &'a [CanvasMeshVertex],
    pub mesh_indices: &'a [u32],
    /// 近景 LOD 直通 ASCII 编号标签。
    pub labels: &'a [CanvasAsciiLabel<'a>],
    /// 橡皮筋框选矩形（画布局部坐标 `[x0, y0, x1, y1]`）。
    pub marquee: Option<[f32; 4]>,
    pub marquee_color: Color,
    /// 仅当相机未变动、只有局部图元状态变更时填入的精确局部屏幕脏矩形切片（驱动 `DamageTracker` 几十像素极速刷新）。
    pub dirty_screen_rects: &'a [Region],
    /// 背景自定义 WGSL 着色器调用（在开启 `custom-shader` 且使用 GPU 时执行）。
    pub bg_shaders: &'a [CustomShaderCall<'a>],
    /// 前景自定义 WGSL 着色器调用。
    pub fg_shaders: &'a [CustomShaderCall<'a>],
    /// 软件渲染模式下的背景自定义特效降级回调。
    pub software_bg_fallback: Option<&'a dyn Fn(&mut dyn SurfacePainter, Viewport2D)>,
    /// 软件渲染模式下的前景自定义特效降级回调。
    pub software_fg_fallback: Option<&'a dyn Fn(&mut dyn SurfacePainter, Viewport2D)>,
}

impl<'a> SceneCanvasSpec<'a> {
    /// 计算绘制负载 32 位内容指纹（用于 `DrawOpSnapshot` 前后帧对比）。
    pub fn content_hash(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut mix = |v: u32| {
            for b in v.to_le_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        };
        mix(self.id);
        mix(self.scene_id.0);
        mix(self.geom_revision);
        mix(self.state_revision);
        mix(self.mesh_revision);
        mix(self.viewport.camera_hash());
        mix(self.bg.0);
        mix(self.opacity as u32 | ((self.zooming as u32) << 8));
        mix(self.labels.len() as u32);
        for c in self.palette.iter().take(16) {
            mix(c.0);
        }
        if let Some(m) = self.marquee {
            for v in m {
                mix(canon_f32(v));
            }
        }
        for s in self.bg_shaders {
            mix(s.id.0);
            mix(s.state_hash as u32);
            mix((s.state_hash >> 32) as u32);
        }
        for s in self.fg_shaders {
            mix(s.id.0);
            mix(s.state_hash as u32);
            mix((s.state_hash >> 32) as u32);
        }
        h
    }
}

impl<'a> PartialEq for SceneCanvasSpec<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.width == other.width
            && self.height == other.height
            && self.opacity == other.opacity
            && self.bg == other.bg
            && self.viewport == other.viewport
            && self.zooming == other.zooming
            && self.scene_id == other.scene_id
            && self.geom_revision == other.geom_revision
            && self.state_revision == other.state_revision
            && self.mesh_revision == other.mesh_revision
            && self.palette == other.palette
            && self.labels.len() == other.labels.len()
            && self.marquee.map(|m| m.map(canon_f32)) == other.marquee.map(|m| m.map(canon_f32))
            && self.bg_shaders == other.bg_shaders
            && self.fg_shaders == other.fg_shaders
    }
}

impl<'a> Eq for SceneCanvasSpec<'a> {}

/// `SceneCanvas` 元素包装器（实现 `IntoElement<'a>`，可作为任意 `div()` 子节点）。
pub struct SceneCanvas<'a> {
    spec: SceneCanvasSpec<'a>,
}

impl<'a> SceneCanvas<'a> {
    #[inline(always)]
    pub const fn from_spec(spec: SceneCanvasSpec<'a>) -> Self {
        Self { spec }
    }
}

impl<'a> IntoElement<'a> for SceneCanvas<'a> {
    #[inline]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        let r = cx.arena().alloc(self.spec);
        Element::SceneCanvas(r)
    }
}

/// 从 [`SceneCanvasSpec`] 构造可嵌入任意布局树的 `SceneCanvas` 元素。
#[inline(always)]
pub const fn scene_canvas<'a>(spec: SceneCanvasSpec<'a>) -> SceneCanvas<'a> {
    SceneCanvas::from_spec(spec)
}

// ============================================================================
// 跨端软件光栅器（支持 `#![no_std]` 直接扫描与 `std` 底图 Pixmap 缓存局部重扫）
// ============================================================================

struct BufferPainter<'a, P: PixelFormat> {
    bounds: Region,
    clip: Region,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &'a mut [P],
}

impl<'a, P: PixelFormat> SurfacePainter for BufferPainter<'a, P> {
    #[inline(always)]
    fn bounds(&self) -> Region {
        self.bounds
    }

    #[inline(always)]
    fn clip(&self) -> Region {
        self.clip
    }

    #[inline(always)]
    fn draw_text(&mut self, bounds: Region, text: &str, color: Color, font_size: u8) {
        rasterize_text(
            bounds,
            self.clip,
            text,
            color,
            font_size,
            0x10,
            self.origin_x,
            self.origin_y,
            self.stride,
            self.target,
        );
    }

    #[inline(always)]
    fn blend_pixel(&mut self, x: i16, y: i16, color: Color, alpha: u8) {
        if alpha == 0 || !self.clip.contains(x, y) {
            return;
        }
        let rx = (x - self.origin_x) as usize;
        let ry = (y - self.origin_y) as usize;
        let idx = ry * self.stride + rx;
        if idx < self.target.len() {
            let eff_a = ((color.a() as u16 * alpha as u16) / 255) as u8;
            if eff_a == 0 {
                return;
            }
            if eff_a == 255 {
                self.target[idx] = P::from_color(color);
            } else {
                let dst = self.target[idx].to_color();
                let inv = 255u16 - eff_a as u16;
                let a16 = eff_a as u16;
                let r = ((color.r() as u16 * a16 + dst.r() as u16 * inv) / 255) as u8;
                let g = ((color.g() as u16 * a16 + dst.g() as u16 * inv) / 255) as u8;
                let b = ((color.b() as u16 * a16 + dst.b() as u16 * inv) / 255) as u8;
                self.target[idx] = P::from_color(Color::rgb(r, g, b));
            }
        }
    }
}

#[inline(always)]
fn blend_px<P: PixelFormat>(target: &mut [P], idx: usize, color: Color, alpha: u8) {
    if alpha == 0 || idx >= target.len() {
        return;
    }
    if alpha == 255 {
        target[idx] = P::from_color(color);
        return;
    }
    let dst = target[idx].to_color();
    let inv = 255u16 - alpha as u16;
    let a16 = alpha as u16;
    let r = ((color.r() as u16 * a16 + dst.r() as u16 * inv) / 255) as u8;
    let g = ((color.g() as u16 * a16 + dst.g() as u16 * inv) / 255) as u8;
    let b = ((color.b() as u16 * a16 + dst.b() as u16 * inv) / 255) as u8;
    target[idx] = P::from_color(Color::rgb(r, g, b));
}

#[inline]
fn resolve_item_colors(state: CanvasItemState, palette: &[Color]) -> (Color, Color, bool) {
    let base = palette
        .get(state.palette_idx as usize)
        .copied()
        .unwrap_or(Color::rgb(59, 130, 246));
    if state.is_disabled() {
        return (base.scale_alpha(90), Color::TRANSPARENT, false);
    }
    let mut fill = base;
    let mut border = Color::TRANSPARENT;
    if state.is_selected() {
        fill = base.lerp(Color::WHITE, 46);
        border = Color::WHITE;
    } else if state.is_hovered() {
        fill = base.lerp(Color::WHITE, 28);
        border = base.lerp(Color::WHITE, 140);
    }
    if state.is_focused() {
        border = Color::rgb(96, 165, 250);
    }
    (fill, border, state.badge_mask != 0 || state.is_locked())
}

#[inline]
fn rasterize_triangle<P: PixelFormat>(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: Color,
    eff_alpha: u8,
    clip: Region,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    if eff_alpha == 0 {
        return;
    }
    let min_x = (floor_f32(x0.min(x1).min(x2)) as i32).max(clip.x as i32);
    let max_x = (floor_f32(x0.max(x1).max(x2)) as i32 + 1).min(clip.right() as i32);
    let min_y = (floor_f32(y0.min(y1).min(y2)) as i32).max(clip.y as i32);
    let max_y = (floor_f32(y0.max(y1).max(y2)) as i32 + 1).min(clip.bottom() as i32);
    if min_x >= max_x || min_y >= max_y {
        return;
    }
    let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
    if area.abs() < 1e-4 {
        return;
    }
    let sign = if area > 0.0 { 1.0 } else { -1.0 };
    for py in min_y..max_y {
        let fy = py as f32 + 0.5;
        let row_off = ((py as i16 - origin_y) as usize) * stride;
        for px in min_x..max_x {
            let fx = px as f32 + 0.5;
            let e0 = ((x1 - x0) * (fy - y0) - (y1 - y0) * (fx - x0)) * sign;
            let e1 = ((x2 - x1) * (fy - y1) - (y2 - y1) * (fx - x1)) * sign;
            let e2 = ((x0 - x2) * (fy - y2) - (y0 - y2) * (fx - x2)) * sign;
            if e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0 {
                let idx = row_off + ((px as i16 - origin_x) as usize);
                blend_px(target, idx, color, eff_alpha);
            }
        }
    }
}

/// 在给定物理裁剪区 `clip` 内执行高密度 2D 场景画布的软件抗锯齿光栅化（支持任意旋转 OBB、圆、胶囊线段、底图网格与 ASCII 标签）。
#[allow(clippy::too_many_arguments)]
pub fn rasterize_scene_canvas<P: PixelFormat>(
    bounds: Region,
    clip: Region,
    spec: &SceneCanvasSpec<'_>,
    scale: f32,
    origin_x: i16,
    origin_y: i16,
    stride: usize,
    target: &mut [P],
) {
    let eff = bounds.intersect(clip);
    if eff.is_empty() || spec.opacity == 0 {
        return;
    }
    let s = if scale > 0.0 { scale } else { 1.0 };
    let vp_zoom = spec.viewport.zoom * s;
    let vp_ox = bounds.x as f32 + spec.viewport.offset_x * s;
    let vp_oy = bounds.y as f32 + spec.viewport.offset_y * s;

    // 1. 背景色填充
    if spec.bg.a() > 0 {
        let a = ((spec.bg.a() as u16 * spec.opacity as u16) / 255) as u8;
        for py in eff.y..eff.bottom() {
            let row_off = ((py - origin_y) as usize) * stride;
            for px in eff.x..eff.right() {
                blend_px(target, row_off + ((px - origin_x) as usize), spec.bg, a);
            }
        }
    }

    // 2. 背景自定义着色器软件降级回调
    if let Some(bg_cb) = spec.software_bg_fallback {
        let mut painter = BufferPainter {
            bounds,
            clip: eff,
            origin_x,
            origin_y,
            stride,
            target,
        };
        bg_cb(&mut painter, spec.viewport);
    }

    // 3. 静态多边形底图三角网格
    let mut t_idx = 0usize;
    while t_idx + 2 < spec.mesh_indices.len() {
        let i0 = spec.mesh_indices[t_idx] as usize;
        let i1 = spec.mesh_indices[t_idx + 1] as usize;
        let i2 = spec.mesh_indices[t_idx + 2] as usize;
        t_idx += 3;
        if i0 >= spec.mesh_verts.len()
            || i1 >= spec.mesh_verts.len()
            || i2 >= spec.mesh_verts.len()
        {
            continue;
        }
        let v0 = spec.mesh_verts[i0];
        let v1 = spec.mesh_verts[i1];
        let v2 = spec.mesh_verts[i2];
        let col = Color(v0.color);
        let a = ((col.a() as u16 * spec.opacity as u16) / 255) as u8;
        rasterize_triangle(
            v0.x * vp_zoom + vp_ox,
            v0.y * vp_zoom + vp_oy,
            v1.x * vp_zoom + vp_ox,
            v1.y * vp_zoom + vp_oy,
            v2.x * vp_zoom + vp_ox,
            v2.y * vp_zoom + vp_oy,
            col,
            a,
            eff,
            origin_x,
            origin_y,
            stride,
            target,
        );
    }

    // 4. 按视锥可见区间渲染 OBB 旋转圆角矩形 / 圆形 / 胶囊线段 SDF 实例
    let full_range = [(0u32, spec.geoms.len() as u32)];
    let ranges: &[(u32, u32)] = if spec.visible_ranges.is_empty() {
        &full_range
    } else {
        spec.visible_ranges
    };

    for &(r_start, r_end) in ranges {
        let start = (r_start as usize).min(spec.geoms.len());
        let end = (r_end as usize).min(spec.geoms.len());
        for i in start..end {
            let g = &spec.geoms[i];
            let st = spec
                .states
                .get(i)
                .copied()
                .unwrap_or_default();
            let (fill_col, border_col, has_badge) = resolve_item_colors(st, spec.palette);
            let scx = g.cx * vp_zoom + vp_ox;
            let scy = g.cy * vp_zoom + vp_oy;
            let sw = g.w * vp_zoom;
            let sh = g.h * vp_zoom;
            let is_capsule = g.kind_flags == CanvasPrimitiveKind::Capsule as u32;
            let is_circle = g.kind_flags == CanvasPrimitiveKind::Circle as u32;
            let hw = if is_capsule { (sw + sh) * 0.5 } else { sw * 0.5 };
            let hh = sh * 0.5;
            let ext_x = hw * g.cos.abs() + hh * g.sin.abs() + 2.0;
            let ext_y = hw * g.sin.abs() + hh * g.cos.abs() + 2.0;

            let min_x = (floor_f32(scx - ext_x) as i32).max(eff.x as i32);
            let max_x = (floor_f32(scx + ext_x) as i32 + 1).min(eff.right() as i32);
            let min_y = (floor_f32(scy - ext_y) as i32).max(eff.y as i32);
            let max_y = (floor_f32(scy + ext_y) as i32 + 1).min(eff.bottom() as i32);
            if min_x >= max_x || min_y >= max_y {
                continue;
            }

            let corner_r = (g.corner_r * vp_zoom).min(hw.min(hh)).max(0.0);
            let border_px = if border_col.a() > 0 {
                (1.5 * s).clamp(1.0, 3.0)
            } else {
                0.0
            };
            let fill_a = ((fill_col.a() as u16 * spec.opacity as u16) / 255) as u8;
            let border_a = ((border_col.a() as u16 * spec.opacity as u16) / 255) as u8;

            for py in min_y..max_y {
                let dy = py as f32 + 0.5 - scy;
                let row_off = ((py as i16 - origin_y) as usize) * stride;
                for px in min_x..max_x {
                    let dx = px as f32 + 0.5 - scx;
                    // 逆旋转至图元局部坐标系（无分支、精确 OBB 采样）
                    let lx = dx * g.cos + dy * g.sin;
                    let ly = -dx * g.sin + dy * g.cos;

                    let dist = if is_circle {
                        sqrt_f32(lx * lx + ly * ly) - hw.min(hh)
                    } else if is_capsule {
                        let half_seg = (sw * 0.5).max(0.0);
                        let qx = (lx.abs() - half_seg).max(0.0);
                        sqrt_f32(qx * qx + ly * ly) - hh
                    } else {
                        let qx = lx.abs() - (hw - corner_r);
                        let qy = ly.abs() - (hh - corner_r);
                        let ox = qx.max(0.0);
                        let oy = qy.max(0.0);
                        sqrt_f32(ox * ox + oy * oy) + qx.max(qy).min(0.0) - corner_r
                    };

                    if dist <= 0.75 {
                        let cov = (0.75 - dist).clamp(0.0, 1.0);
                        let idx = row_off + ((px as i16 - origin_x) as usize);
                        if border_px > 0.0 && dist > -border_px {
                            let a = ((border_a as f32) * cov) as u8;
                            blend_px(target, idx, border_col, a);
                        } else {
                            let a = ((fill_a as f32) * cov) as u8;
                            blend_px(target, idx, fill_col, a);
                        }
                        // 角标圆点（局部右上角，投影尺寸足够时显示）
                        if has_badge && hw >= 6.0 && hh >= 6.0 {
                            let bx = lx - (hw - 3.5);
                            let by = ly + (hh - 3.5);
                            if bx * bx + by * by <= 4.5 {
                                blend_px(target, idx, Color::rgb(250, 204, 21), spec.opacity);
                            }
                        }
                    }
                }
            }
        }
    }

    // 5. 近景 LOD ASCII 直通标签（使用引擎内置抗锯齿文本通道）
    for lbl in spec.labels {
        if lbl.text.is_empty() {
            continue;
        }
        let sx = (lbl.wx * vp_zoom + vp_ox) as i32;
        let sy = (lbl.wy * vp_zoom + vp_oy) as i32;
        let fpx = (lbl.font_px * s).clamp(7.0, 32.0) as u8;
        let est_w = ((lbl.text.len() as i32) * (fpx as i32) * 3 / 4).max(16);
        let est_h = (fpx as i32) + 4;
        let tb = Region::new(
            (sx - est_w / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            (sy - est_h / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            est_w.min(i16::MAX as i32) as i16,
            est_h.min(i16::MAX as i32) as i16,
        );
        if !tb.intersect(eff).is_empty() {
            // text_flags bit0=1 (center align) | bit4=1 (nowrap)
            rasterize_text(
                tb,
                eff,
                lbl.text,
                lbl.color,
                fpx,
                1 | 0x10,
                origin_x,
                origin_y,
                stride,
                target,
            );
        }
    }

    // 6. 橡皮筋框选矩形
    if let Some([mx0, my0, mx1, my1]) = spec.marquee {
        let rx0 = (bounds.x as f32 + mx0.min(mx1) * s) as i32;
        let ry0 = (bounds.y as f32 + my0.min(my1) * s) as i32;
        let rx1 = (bounds.x as f32 + mx0.max(mx1) * s) as i32;
        let ry1 = (bounds.y as f32 + my0.max(my1) * s) as i32;
        let mr = Region::new(
            rx0.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            ry0.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            (rx1 - rx0).clamp(0, i16::MAX as i32) as i16,
            (ry1 - ry0).clamp(0, i16::MAX as i32) as i16,
        )
        .intersect(eff);
        if !mr.is_empty() {
            let fill_a = ((spec.marquee_color.a() as u16 * 56) / 255) as u8;
            let edge_a = ((spec.marquee_color.a() as u16 * 210) / 255) as u8;
            for py in mr.y..mr.bottom() {
                let row_off = ((py - origin_y) as usize) * stride;
                let is_y_edge = py == mr.y || py == mr.bottom() - 1;
                for px in mr.x..mr.right() {
                    let is_edge = is_y_edge || px == mr.x || px == mr.right() - 1;
                    blend_px(
                        target,
                        row_off + ((px - origin_x) as usize),
                        spec.marquee_color,
                        if is_edge { edge_a } else { fill_a },
                    );
                }
            }
        }
    }

    // 7. 前景自定义着色器软件降级回调
    if let Some(fg_cb) = spec.software_fg_fallback {
        let mut painter = BufferPainter {
            bounds,
            clip: eff,
            origin_x,
            origin_y,
            stride,
            target,
        };
        fg_cb(&mut painter, spec.viewport);
    }
}
