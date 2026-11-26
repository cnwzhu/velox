//! `velox_base::SurfaceCanvas` 外部零拷贝 GPU 视口画布组件交互与叠加层测试。

use core::sync::atomic::{AtomicI16, AtomicU32, Ordering};
use velox_base::SurfaceCanvas;
use velox_engine::prelude::*;

static CANVAS_CLICKS: AtomicU32 = AtomicU32::new(0);
static LAST_DRAG_X: AtomicI16 = AtomicI16::new(-1);
static OVERLAY_CLICKS: AtomicU32 = AtomicU32::new(0);

struct RadarViewportTestView;

impl View for RadarViewportTestView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let overlay_btn = div()
            .id(200)
            .pos(10, 10)
            .w(60)
            .h(30)
            .bg(Color::rgba(20, 30, 50, 180))
            .on_click(cx, &|_ev, _acx| {
                OVERLAY_CLICKS.fetch_add(1, Ordering::Relaxed);
            });

        SurfaceCanvas::new(100, ExternalTextureId(9))
            .size_px(240, 160)
            .rounded(12)
            .uv_rect([0.0, 0.0, 1.0, 1.0])
            .revision(3)
            .fallback_bg(Color::rgb(15, 25, 45))
            .on_click(&|_ev, _acx| {
                CANVAS_CLICKS.fetch_add(1, Ordering::Relaxed);
            })
            .on_drag(&|ev, _acx| {
                LAST_DRAG_X.store(ev.x, Ordering::Relaxed);
            })
            .overlay_child(cx, overlay_btn)
            .into_element(cx)
    }
}

#[test]
fn test_surface_canvas_renders_external_op_and_dispatches_overlay_and_drag() {
    CANVAS_CLICKS.store(0, Ordering::Relaxed);
    LAST_DRAG_X.store(-1, Ordering::Relaxed);
    OVERLAY_CLICKS.store(0, Ordering::Relaxed);

    let mut engine = VeloxEngine::new(240, 160);
    let _ = engine.mount_root(RadarViewportTestView);
    let dirty = engine.step_frame_dt(16);
    assert!(!dirty.is_empty());

    // 确认绘制列表中发射了正确的 ExternalTexture 指令且携带 revision=3
    let has_ext_op = engine.cached_draw_ops().iter().any(|op| {
        matches!(
            op.payload,
            DrawPayload::ExternalTexture(spec)
                if spec.id == ExternalTextureId(9) && spec.content_revision == 3
        )
    });
    assert!(
        has_ext_op,
        "SurfaceCanvas must emit DrawPayload::ExternalTexture"
    );

    // 点击左上角 HUD 悬浮按钮应命中 overlay 而不透穿到底层画布
    assert!(engine.dispatch_click(25, 20));
    assert_eq!(OVERLAY_CLICKS.load(Ordering::Relaxed), 1);
    assert_eq!(CANVAS_CLICKS.load(Ordering::Relaxed), 0);

    // 点击裸露画布区域触发画布点击，拖拽触发画布 on_drag
    assert!(engine.dispatch_click(150, 100));
    assert_eq!(CANVAS_CLICKS.load(Ordering::Relaxed), 1);

    assert!(engine.dispatch_pointer_down(150, 100));
    assert!(engine.dispatch_pointer_move(180, 110));
    let _ = engine.dispatch_pointer_up(180, 110);
    assert_eq!(LAST_DRAG_X.load(Ordering::Relaxed), 180);
}
