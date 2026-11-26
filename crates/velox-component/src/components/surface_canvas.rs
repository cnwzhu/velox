//! 主题化外部零拷贝 GPU 渲染视口画布组件 `SurfaceCanvas`。

use velox_engine::prelude::*;

crate::define_themed_component! {
    name: SurfaceCanvas<'a>,
    base: velox_base::SurfaceCanvas<'a>,
    new: (id: u32, texture_id: ExternalTextureId),
    styled: false,
    variants: false,
    sizable: false,
    disableable: false,
    apply_tw: |base, theme| {
        if base.rounded == 0 && base.corner_radii.is_none() {
            base.rounded = 8;
        }
        if base.style.border_px() == 0 {
            base.style.set_border_px(1);
            base.style.border_color = theme.border;
        }
    },
    forward: [
        size_px(w: i16, h: i16),
        uv_rect(uv_rect: [f32; 4]),
        revision(rev: u64),
        premultiplied(premul: bool),
        fallback_bg(color: Color),
        rounded(r: u8),
        rounded_corners(radii: CornerRadii),
        focusable(focusable: bool),
        cursor(cursor: CursorShape),
        on_click(f: &'a dyn Fn(ClickEvent, &mut ActionScope)),
        on_drag(f: &'a dyn Fn(DragEvent, &mut ActionScope)),
        on_swipe(f: &'a dyn Fn(SwipeDir, &mut ActionScope)),
        on_key(f: &'a dyn Fn(KeyCode, &mut ActionScope)),
    ],
}

impl<'a> SurfaceCanvas<'a> {
    /// 在外部渲染视口上方叠放任意悬浮 HUD 子元素（支持实时 `.backdrop_acrylic(...)` 毛玻璃）。
    #[inline]
    pub fn overlay_child<E: IntoElement<'a>>(mut self, cx: &'a FrameScope, child: E) -> Self {
        self.inner = self.inner.overlay_child(cx, child);
        self
    }
}
