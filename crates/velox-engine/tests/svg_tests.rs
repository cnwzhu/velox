//! 原生零依赖矢量 SVG 引擎单元测试（兼容 `std`、`gpu` 与 `--no-default-features` 双模）。

use velox_engine::display::PixelFormat;
use velox_engine::prelude::*;
use velox_engine::svg::{
    parse_svg_color, parse_svg_viewbox, rasterize_svg, rasterize_svg_path, SvgRenderOptions,
    SvgViewBox, Transform2D,
};

#[test]
fn test_svg_color_and_viewbox_parsing() {
    assert_eq!(parse_svg_color("#f0a"), Some(Color::rgb(255, 0, 170)));
    assert_eq!(parse_svg_color("#0f88"), Some(Color::rgba(0, 255, 136, 136)));
    assert_eq!(parse_svg_color("#38BDF8"), Some(Color::rgb(56, 189, 248)));
    assert_eq!(
        parse_svg_color("#38bdf880"),
        Some(Color::rgba(56, 189, 248, 128))
    );
    assert_eq!(
        parse_svg_color("rgb(10, 20, 30)"),
        Some(Color::rgb(10, 20, 30))
    );
    assert_eq!(
        parse_svg_color("rgba(255, 128, 0, 0.5)"),
        Some(Color::rgba(255, 128, 0, 128))
    );
    assert_eq!(
        parse_svg_color("rgb(100%, 0%, 50%)"),
        Some(Color::rgb(255, 0, 128))
    );
    assert_eq!(parse_svg_color("transparent"), Some(Color::TRANSPARENT));
    assert_eq!(parse_svg_color("SteelBlue"), Some(Color::rgb(70, 130, 180)));
    assert_eq!(parse_svg_color("invalid-color"), None);

    let vb1 = parse_svg_viewbox(r#"<svg viewBox="2 4 48 32" width="100" height="100"></svg>"#)
        .expect("should parse viewBox");
    assert_eq!(
        vb1,
        SvgViewBox {
            min_x: 2.0,
            min_y: 4.0,
            width: 48.0,
            height: 32.0
        }
    );

    let vb2 = parse_svg_viewbox(r#"<svg width="64px" height="32px"></svg>"#)
        .expect("should fallback to width/height");
    assert_eq!(vb2.width, 64.0);
    assert_eq!(vb2.height, 32.0);

    let xf = Transform2D::parse_svg_attr("translate(10, 20) scale(2, 3)");
    let (x, y) = xf.apply(3.0, 4.0);
    assert!((x - 16.0).abs() < 1e-4);
    assert!((y - 32.0).abs() < 1e-4);
}

#[test]
fn test_svg_evenodd_donut_hole_and_nonzero() {
    // 两个同方向嵌套矩形：外框 [4..60]，内框 [20..44]
    let donut_evenodd = r##"<svg viewBox="0 0 64 64">
        <path fill="#38BDF8" fill-rule="evenodd" d="M4 4 H60 V60 H4 Z M20 20 H44 V44 H20 Z"/>
    </svg>"##;
    let donut_nonzero = r##"<svg viewBox="0 0 64 64">
        <path fill="#38BDF8" fill-rule="nonzero" d="M4 4 H60 V60 H4 Z M20 20 H44 V44 H20 Z"/>
    </svg>"##;

    let bounds = Region::new(0, 0, 64, 64);
    let mut buf_eo = [Xrgb8888::BLACK; 64 * 64];
    rasterize_svg(
        donut_evenodd,
        bounds,
        bounds,
        SvgRenderOptions::default(),
        0,
        0,
        64,
        &mut buf_eo,
    );
    // 圆环带区域 (10, 32) 应填充为目标青蓝色
    let ring_col = buf_eo[32 * 64 + 10].to_color();
    assert_eq!(ring_col, Color::rgb(56, 189, 248));
    // 中心孔洞 (32, 32) 在 evenodd 下必须镂空保持纯黑
    let hole_col = buf_eo[32 * 64 + 32].to_color();
    assert_eq!(hole_col, Color::rgb(0, 0, 0));
    // 外部空白 (1, 1) 保持纯黑
    assert_eq!(buf_eo[1 * 64 + 1].to_color(), Color::rgb(0, 0, 0));

    let mut buf_nz = [Xrgb8888::BLACK; 64 * 64];
    rasterize_svg(
        donut_nonzero,
        bounds,
        bounds,
        SvgRenderOptions::default(),
        0,
        0,
        64,
        &mut buf_nz,
    );
    // 同向子路径在 nonzero 下中心应为实心填充
    assert_eq!(buf_nz[32 * 64 + 32].to_color(), Color::rgb(56, 189, 248));
}

#[test]
fn test_svg_curves_arcs_shapes_and_defs_skip() {
    // 覆盖 <defs> 跳过、紧凑椭圆弧 flag (01)、贝塞尔曲线、<rect rx>、<circle>、<polygon>
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
    <!-- test comment -->
    <svg viewBox="0 0 64 64">
      <defs>
        <rect x="0" y="0" width="64" height="64" fill="#FF0000"/>
      </defs>
      <rect x="2" y="2" width="24" height="24" rx="6" fill="#22C55E"/>
      <circle cx="48" cy="14" r="10" fill="#3B82F6"/>
      <path d="M8 48 a10 10 0 0120 0 a10 10 0 01-20 0 Z" fill="#F59E0B"/>
      <path d="M36 48 C36 38 56 38 56 48 Q56 58 46 58 T36 48 Z" fill="#A855F7"/>
    </svg>"##;

    let bounds = Region::new(0, 0, 64, 64);
    let mut buf = [Xrgb8888::BLACK; 64 * 64];
    rasterize_svg(
        xml,
        bounds,
        bounds,
        SvgRenderOptions::default(),
        0,
        0,
        64,
        &mut buf,
    );

    // <defs> 内的红色背景不可被绘制：中心空白 (32, 32) 保持黑色
    assert_eq!(buf[32 * 64 + 32].to_color(), Color::rgb(0, 0, 0));
    // 左上圆角矩形中心 (14, 14) 为绿色，外圆角顶角 (2, 2) 应被圆角削去保持接近黑色
    assert_eq!(buf[14 * 64 + 14].to_color(), Color::rgb(34, 197, 94));
    assert!(buf[2 * 64 + 2].to_color().g() < 30);
    // 右上圆中心 (48, 14) 为蓝色
    assert_eq!(buf[14 * 64 + 48].to_color(), Color::rgb(59, 130, 246));
    // 左下椭圆弧圆盘中心 (18, 48) 为橙黄色
    assert_eq!(buf[48 * 64 + 18].to_color(), Color::rgb(245, 158, 11));
    // 右下贝塞尔封闭图形中心 (46, 48) 为紫色
    assert_eq!(buf[48 * 64 + 46].to_color(), Color::rgb(168, 85, 247));
}

#[test]
fn test_svg_group_inheritance_transform_and_current_color() {
    let xml = r##"<svg viewBox="0 0 40 40">
      <g transform="translate(10, 10)" fill="currentColor">
        <rect x="0" y="0" width="10" height="10"/>
        <rect x="12" y="0" width="10" height="10" fill="#FF0000" style="fill: #00FF00"/>
      </g>
    </svg>"##;

    let bounds = Region::new(0, 0, 40, 40);
    let mut buf = [Xrgb8888::BLACK; 40 * 40];
    let opts = SvgRenderOptions {
        tint: Color::rgb(255, 80, 20),
        stroke_width_override: None,
    };
    rasterize_svg(xml, bounds, bounds, opts, 0, 0, 40, &mut buf);

    // 第一个矩形经 translate(10, 10) 移至 [10..20, 10..20]，继承 currentColor = tint
    assert_eq!(buf[15 * 40 + 15].to_color(), Color::rgb(255, 80, 20));
    // 第二个矩形行内 style 覆盖表现属性 fill="#FF0000" 为纯绿 #00FF00
    assert_eq!(buf[15 * 40 + 27].to_color(), Color::rgb(0, 255, 0));
    // 原点未平移区域应为空白
    assert_eq!(buf[4 * 40 + 4].to_color(), Color::rgb(0, 0, 0));
}

#[test]
fn test_svg_stroke_join_no_double_alpha_overlap() {
    // 半透明直角折线描边：折点 (32, 32) 与直线段中点 (18, 32) 必须具有完全一致的像素亮度（无二次叠乘暗斑）
    let xml = r##"<svg viewBox="0 0 64 64">
      <path d="M 8 32 L 32 32 L 32 56" fill="none" stroke="rgba(255, 0, 0, 0.5)" stroke-width="8" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>"##;

    let bounds = Region::new(0, 0, 64, 64);
    let mut buf = [Xrgb8888::BLACK; 64 * 64];
    rasterize_svg(
        xml,
        bounds,
        bounds,
        SvgRenderOptions::default(),
        0,
        0,
        64,
        &mut buf,
    );

    let straight_r = buf[32 * 64 + 18].to_color().r();
    let corner_r = buf[32 * 64 + 32].to_color().r();
    assert!(
        (120..=136).contains(&straight_r),
        "expected ~50% red on straight segment, got {}",
        straight_r
    );
    assert_eq!(
        corner_r, straight_r,
        "corner pixel must not double-blend semi-transparent stroke (straight={}, corner={})",
        straight_r, corner_r
    );
}

#[test]
fn test_svg_path_icon_and_engine_software_integration() {
    struct SvgScene;
    impl View for SvgScene {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_row()
                .gap(8)
                .p(8)
                .bg(Color::rgb(15, 23, 42))
                .child(
                    cx,
                    svg_icon(24, 24, "M20 6 9 17l-5-5").tint(Color::rgb(56, 189, 248)),
                )
                .child(
                    cx,
                    svg_path(24, 24, "M4 4 H20 V20 H4 Z").tint(Color::rgb(250, 204, 21)),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(96, 48);
    let _ = engine.mount_root(SvgScene);
    let _ = engine.step_frame_dt(16);

    let mut fb = [Xrgb8888::BLACK; 96 * 48];
    engine.rasterize_region(Region::new(0, 0, 96, 48), &mut fb);

    // 第一个图标（勾选路径）区域 [8..32, 8..32] 应出现天蓝色抗锯齿像素
    let mut cyan_px = 0usize;
    for y in 8..32 {
        for x in 8..32 {
            let c = fb[y * 96 + x].to_color();
            if c.b() > 160 && c.g() > 120 {
                cyan_px += 1;
            }
        }
    }
    assert!(cyan_px >= 12, "expected visible svg_icon check stroke, got {} px", cyan_px);

    // 第二个实心路径方块中心 [40..64, 8..32] -> (52, 20) 应为纯黄色
    assert_eq!(fb[20 * 96 + 52].to_color(), Color::rgb(250, 204, 21));

    // 同时直接测试 rasterize_svg_path HiDPI 2x 尺寸无损光栅化
    let mut hi = [Rgba8888(0); 48 * 48];
    rasterize_svg_path(
        "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z",
        Region::new(0, 0, 48, 48),
        Region::new(0, 0, 48, 48),
        Color::rgb(255, 255, 255),
        20,
        0,
        0,
        48,
        &mut hi,
    );
    // 圆环外缘顶部 (24, 4) 应有高覆盖白色，圆心 (24, 24) 应保持完全透明
    assert!(hi[4 * 48 + 24].to_color().a() > 200);
    assert_eq!(hi[24 * 48 + 24].to_color().a(), 0);
}

#[cfg(feature = "gpu")]
#[test]
fn test_svg_gpu_sprite_atlas_cross_frame_cache() {
    use velox_engine::renderers::wgpu::WgpuOffscreenRenderer;

    const LOGO: &str = r##"<svg viewBox="0 0 24 24">
      <rect x="1" y="1" width="22" height="22" rx="6" fill="#0F172A" stroke="#38BDF8" stroke-width="1.5"/>
      <path d="M6 7L12 18L18 7" fill="none" stroke="#38BDF8" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>
      <path d="M9.5 7L12 12L14.5 7Z" fill="#818CF8"/>
    </svg>"##;

    struct GpuSvgScene;
    impl View for GpuSvgScene {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .size_full()
                .flex_row()
                .gap(12)
                .p(12)
                .bg(Color::rgb(15, 23, 42))
                .child(cx, svg_str(32, 32, LOGO))
                .child(
                    cx,
                    svg_icon(24, 24, "M20 6 9 17l-5-5").tint(Color::rgb(34, 197, 94)),
                )
                .into_element(cx)
        }
    }

    let mut renderer = WgpuOffscreenRenderer::new(320, 160);
    let has_gpu = renderer.ensure_gpu();

    let mut engine = VeloxEngine::new(160, 80);
    let _ = engine.mount_root(GpuSvgScene);
    let _ = engine.step_frame_dt(16);
    let vp = Region::new(0, 0, 160, 80);

    let hits_before = renderer.sprite_cache_hits();
    let _ = renderer.render_draw_ops(engine.cached_draw_ops(), vp, 2, &[vp]);
    assert!(!renderer.rgba_buffer.is_empty());

    // 确认离屏 2× HiDPI 帧中存在 Logo 的青蓝色像素与图标的绿色像素
    let mut cyan_found = 0usize;
    let mut green_found = 0usize;
    for chunk in renderer.rgba_buffer.chunks_exact(4) {
        let (r, g, b) = (chunk[0], chunk[1], chunk[2]);
        if b > 190 && g > 140 && r < 110 {
            cyan_found += 1;
        }
        if g > 150 && r < 90 && b < 130 {
            green_found += 1;
        }
    }
    assert!(
        cyan_found >= 40,
        "expected crisp cyan Velox SVG logo on GPU offscreen frame, got {} px",
        cyan_found
    );
    assert!(
        green_found >= 24,
        "expected crisp green SVG check icon on GPU offscreen frame, got {} px",
        green_found
    );

    // 渲染第二帧：在硬件 GPU 路径下，2 个静态 SVG 精灵必须 100% 命中 SpriteAtlas 驻留缓存
    if has_gpu {
        let _ = renderer.render_draw_ops(engine.cached_draw_ops(), vp, 2, &[vp]);
        let hits_after = renderer.sprite_cache_hits();
        assert!(
            hits_after >= hits_before + 2,
            "expected >= 2 SpriteAtlas cache hits on frame 2 for static SVGs (before={}, after={})",
            hits_before,
            hits_after
        );
    }
}
