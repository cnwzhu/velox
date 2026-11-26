//! 外部零拷贝 GPU 渲染视口画布组件 `SurfaceCanvas`。
//!
//! 专为海图引擎、3D 视口、实时视频解码与 GIS 地图等外部 GPU 渲染器设计：
//! - 通过 [`ExternalTextureId`] 零拷贝绑定已注册在 `WgpuRenderer` 中的 `wgpu::TextureView`；
//! - 支持四角独立圆角裁切、归一化 UV 裁切、版本号局部脏区触发（`revision`）与软件/无 GPU 占位降级；
//! - 内置点击、拖拽、滑动与键盘导航事件透传，并支持直接在画布上方叠放悬浮 HUD 子元素（含实时毛玻璃）。

use velox_engine::prelude::*;

/// 零堆无样式外部 GPU 渲染视口画布组件（`SurfaceCanvas`）。
pub struct SurfaceCanvas<'a> {
    pub id: u32,
    pub texture_id: ExternalTextureId,
    pub width: i16,
    pub height: i16,
    pub uv_rect: [f32; 4],
    pub content_revision: u64,
    pub premultiplied: bool,
    pub fallback_bg: Color,
    pub rounded: u8,
    pub corner_radii: Option<CornerRadii>,
    pub focusable: bool,
    pub cursor: CursorShape,
    pub style: Style,
    pub style_ext: StyleExt,
    on_click: Option<&'a dyn Fn(ClickEvent, &mut ActionScope)>,
    on_drag: Option<&'a dyn Fn(DragEvent, &mut ActionScope)>,
    on_swipe: Option<&'a dyn Fn(SwipeDir, &mut ActionScope)>,
    on_key: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    overlays: [Option<Element<'a>>; 8],
    overlay_len: usize,
}

impl<'a> SurfaceCanvas<'a> {
    /// 创建一个绑定指定外部零拷贝纹理 ID 的画布组件。
    #[inline(always)]
    pub const fn new(id: u32, texture_id: ExternalTextureId) -> Self {
        Self {
            id,
            texture_id,
            width: 320,
            height: 240,
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            content_revision: 0,
            premultiplied: true,
            fallback_bg: Color::rgb(12, 18, 28),
            rounded: 0,
            corner_radii: None,
            focusable: true,
            cursor: CursorShape::Crosshair,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_click: None,
            on_drag: None,
            on_swipe: None,
            on_key: None,
            overlays: [const { None }; 8],
            overlay_len: 0,
        }
    }

    /// 设置默认逻辑宽高（若未通过 `.w(...)`/`.h(...)`/`.size_full()` 覆盖）。
    #[inline(always)]
    pub const fn size_px(mut self, w: i16, h: i16) -> Self {
        self.width = if w < 1 { 1 } else { w };
        self.height = if h < 1 { 1 } else { h };
        self
    }

    /// 设置外部纹理归一化采样 UV 矩形 `[u0, v0, u1, v1]`。
    #[inline(always)]
    pub const fn uv_rect(mut self, uv_rect: [f32; 4]) -> Self {
        self.uv_rect = uv_rect;
        self
    }

    /// 绑定外部引擎画面版本号（每次外部引擎重绘新帧时递增，精准触发 Velox 脏区刷新）。
    #[inline(always)]
    pub const fn revision(mut self, rev: u64) -> Self {
        self.content_revision = rev;
        self
    }

    /// 指定外部纹理是否已为预乘 Alpha（默认 `true`）。
    #[inline(always)]
    pub const fn premultiplied(mut self, premul: bool) -> Self {
        self.premultiplied = premul;
        self
    }

    /// 设置软件渲染器或无 GPU 环境下的回退占位背景色。
    #[inline(always)]
    pub const fn fallback_bg(mut self, color: Color) -> Self {
        self.fallback_bg = color;
        self
    }

    /// 设置统一圆角半径（逻辑像素）。
    #[inline(always)]
    pub const fn rounded(mut self, r: u8) -> Self {
        self.rounded = r;
        self
    }

    /// 设置四角独立圆角裁切。
    #[inline(always)]
    pub const fn rounded_corners(mut self, radii: CornerRadii) -> Self {
        self.corner_radii = Some(radii);
        self
    }

    /// 设置是否可获键盘焦点（默认 `true`）。
    #[inline(always)]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// 设置悬停光标样式（默认 `CursorShape::Crosshair`）。
    #[inline(always)]
    pub const fn cursor(mut self, cursor: CursorShape) -> Self {
        self.cursor = cursor;
        self
    }

    /// 绑定点击/选点事件回调（透传逻辑坐标给外部海图或 3D 拾取器）。
    #[inline(always)]
    pub fn on_click(mut self, f: &'a dyn Fn(ClickEvent, &mut ActionScope)) -> Self {
        self.on_click = Some(f);
        self
    }

    /// 绑定连续拖拽手势回调（如平移海图、旋转 3D 相机）。
    #[inline(always)]
    pub fn on_drag(mut self, f: &'a dyn Fn(DragEvent, &mut ActionScope)) -> Self {
        self.on_drag = Some(f);
        self
    }

    /// 绑定滑动方向手势回调。
    #[inline(always)]
    pub fn on_swipe(mut self, f: &'a dyn Fn(SwipeDir, &mut ActionScope)) -> Self {
        self.on_swipe = Some(f);
        self
    }

    /// 绑定键盘导航按键回调（如方向键平移、`+`/`-` 缩放）。
    #[inline(always)]
    pub fn on_key(mut self, f: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key = Some(f);
        self
    }

    /// 在外部渲染视口上方叠放任意悬浮 HUD 子元素（支持实时 `.backdrop_acrylic(...)` 毛玻璃）。
    #[inline]
    pub fn overlay_child<E: IntoElement<'a>>(mut self, cx: &'a FrameScope, child: E) -> Self {
        if self.overlay_len < self.overlays.len() {
            self.overlays[self.overlay_len] = Some(child.into_element(cx));
            self.overlay_len += 1;
        }
        self
    }
}

crate::impl_base_styled_box!(SurfaceCanvas<'a>);

impl<'a> Widget<'a> for SurfaceCanvas<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        let mut root = div().id(self.id).cursor(self.cursor);

        if matches!(self.style.width, Length::Auto) {
            root = root.w(self.width);
        } else {
            root.style.width = self.style.width;
        }
        if matches!(self.style.height, Length::Auto) {
            root = root.h(self.height);
        } else {
            root.style.height = self.style.height;
        }
        if self.style.flex_grow() > 0 {
            root = root.flex_grow(self.style.flex_grow());
        }
        if self.style.border_px() > 0 && self.style.border_color.a() > 0 {
            root = root.border(self.style.border_px(), self.style.border_color);
        }

        let eff_r = if self.style.rounded_radius() > 0 {
            self.style.rounded_radius()
        } else {
            self.rounded
        };
        if eff_r > 0 {
            root = root.rounded(eff_r);
        }
        if let Some(cr) = self.corner_radii {
            root = root.rounded_corners(cr);
        }

        let spec = ExternalTextureSpec::new(self.texture_id)
            .with_uv(self.uv_rect)
            .with_revision(self.content_revision)
            .with_premultiplied(self.premultiplied)
            .with_fallback_bg(self.fallback_bg);
        root = root.external_texture_spec(spec);

        if self.focusable {
            root = root.focusable();
        }
        if let Some(cb) = self.on_click {
            root = root.on_click(cx, cb);
        }
        if let Some(cb) = self.on_drag {
            root = root.on_drag(cx, cb);
        }
        if let Some(cb) = self.on_swipe {
            root = root.on_swipe(cx, cb);
        }
        if let Some(cb) = self.on_key {
            root = root.on_key_nav(cb);
        }

        for slot in self.overlays.iter_mut().take(self.overlay_len) {
            if let Some(elem) = slot.take() {
                root = root.child(cx, elem);
            }
        }

        self.style_ext.apply_to_div(root, self.style).into_element(cx)
    }
}
