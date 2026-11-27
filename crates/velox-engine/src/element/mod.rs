//! 闭合枚举 `Element<'a>`、`Div<'a>`、`Text<'a>`、`Img<'a>`、侵入式单向兄弟链表与 `View` / `Widget` 组件接口。
//!
//! 模块职责划分：
//! - `div`：`Div<'a>` 弹性盒容器构造器、`DivNode` / `DivVisual` / `DivInteractivity` 与过渡动画配置；
//! - `text`：`Text<'a>` 文本原子节点与排版修饰符；
//! - `image`：`SpriteSource<'a>` 与 `Img<'a>` 位图/矢量精灵/GPU 原生圆弧节点。

mod div;
mod div_node;
mod image;
mod text;

use core::cell::Cell;

use crate::pod::{FrameScope, Handle, PodId, Scope};

pub use div::{div, Div};
pub use div_node::{
    DivInteractivity, DivNode, DivRareHandlers, DivVisual, EnterFrom, ExitTo, TransitionSpec,
};
pub use image::{
    img, img_arc, img_rotated_rgb565, img_shader, svg, svg_a4, svg_icon, svg_path, svg_str, Img,
    SpriteSource,
};
pub use text::{text, Text};

/// 有状态视图接口（常驻 `PodPool`，拥有独立局部刷新边界）。
pub trait View: Sized + 'static {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a>;
}

/// 无状态轻量组件接口（不占 `PodPool` 槽位，直接在 `ScratchArena` 中内联展开为 `div` 树）。
pub trait Widget<'a>: Sized {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a>;
}

/// 将基础原语或无状态 `Widget` 统一转换为 `Element<'a>` 的转换 Trait。
pub trait IntoElement<'a>: Sized {
    fn into_element(self, cx: &'a FrameScope) -> Element<'a>;

    /// 与 GPUI `.into_any_element()` 对应的语义别名（零堆分配返回闭合枚举 `Element<'a>`）。
    #[inline(always)]
    fn into_any_element(self, cx: &'a FrameScope) -> Element<'a> {
        self.into_element(cx)
    }
}

impl<'a> IntoElement<'a> for Element<'a> {
    #[inline(always)]
    fn into_element(self, _cx: &'a FrameScope) -> Element<'a> {
        self
    }
}


impl<'a, W: Widget<'a>> IntoElement<'a> for W {
    #[inline(always)]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        self.render_once(cx)
    }
}

impl<'a, V: View> IntoElement<'a> for Handle<V> {
    #[inline(always)]
    fn into_element(self, _cx: &'a FrameScope) -> Element<'a> {
        Element::SubView {
            id: self.id,
            generation: self.generation,
            resolved: Cell::new(None),
        }
    }
}

/// 侵入式单向链表节点（分配在 `ScratchArena` 中，使用 `Cell` 提供安全的内部可变性）。
pub struct ElementNode<'a> {
    pub element: Element<'a>,
    /// 指向 `ScratchArena` 中的下一个同级兄弟节点（实现零 `Vec` 扩容的无限 `.child()` 追加）。
    pub next_sibling: Cell<Option<&'a ElementNode<'a>>>,
}

/// 闭合 UI 元素枚举（严格限定 4 种核心节点，彻底消除 `Box<dyn Element>` 虚表开销与内存碎片）。
pub enum Element<'a> {
    /// 弹性盒容器节点（64B 紧凑核心 + 按需分配在 `ScratchArena` 中的视觉/交互扩展）。
    Div(DivNode<'a>),
    /// 文本节点。
    Text(Text<'a>),
    /// 位图/图标掩码或过程化贴图节点（对应 `DrawPayload::Sprite`）。
    Img(Img<'a>),
    /// 行内多字重/多色/BiDi/彩色 Emoji 富文本段落（8B Arena 引用，0 字节膨胀 `Element`）。
    RichText(&'a crate::text_rich::RichTextSpec<'a>),
    /// 支持 `ObjectFit` / `NineSlice` / LRU 缓存的解码图像元素（8B Arena 引用）。
    DecodedImage(&'a crate::image_pipeline::DecodedImageSpec),
    /// 原生抗锯齿 2D 矢量画布（`Polyline` / `Polygon` / `Circle` / `Sector`，8B Arena 引用）。
    Vector(&'a crate::vector_canvas::VectorDrawSpec<'a>),
    /// 高密度 2D 交互式场景画布（32B 静态几何 + 4B 动态状态分离 + 可选 `custom-shader`，8B Arena 引用）。
    SceneCanvas(&'a crate::canvas_scene::SceneCanvasSpec<'a>),
    /// 视图边界：引用另一个常驻 `Handle<T>`（`PodId` + `generation`），作为局部重绘与独立布局的隔离点。
    SubView {
        id: PodId,
        generation: u16,
        resolved: Cell<Option<&'a Element<'a>>>,
    },
}

