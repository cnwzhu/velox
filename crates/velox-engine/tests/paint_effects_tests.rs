//! 完整渐变（Linear/Radial/Conic + Pad/Repeat/Reflect + sRGB/LinearRgb/OKLab）、
//! 四角独立圆角、27 种 Porter-Duff/W3C 混合模式、组级隔离透明度、模糊/ColorMatrix/DropShadow 滤镜、
//! 实时毛玻璃（`backdrop-filter` / Acrylic）、2D 仿射变换、SVG Path 裁剪遮罩与 GPU/软件双后端一致性测试。

use velox_engine::motion::{DrawOp, DrawOpSnapshot};
use velox_engine::paint::{BlendMode, ColorStop, CornerRadii, ExtendMode, GradientSpace, Paint};
use velox_engine::prelude::*;
use velox_engine::solver::{solve_and_flatten, HitRegion};

fn render_to_rgba<'a, const N: usize>(
    root: &'a Element<'a>,
    cx: &'a FrameScope,
    w: i16,
    h: i16,
) -> [Rgba8888; N] {
    let vp = Region::new(0, 0, w, h);
    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 64> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 64> = SmallVec::new();
    solve_and_flatten(cx.arena(), root, vp, &mut motion, &mut ops, &mut hits);
    let mut buf = [Rgba8888(0); N];
    rasterize_region(ops.as_slice(), vp, 0, 0, w as usize, &mut buf);
    buf
}

#[inline]
fn px_at(buf: &[Rgba8888], w: usize, x: usize, y: usize) -> Color {
    buf[y * w + x].to_color()
}

/// 1. 严守核心数据结构零体积膨胀红线。
#[test]
fn test_core_struct_memory_footprint_invariants() {
    assert_eq!(core::mem::size_of::<Style>(), 24);
    assert_eq!(core::mem::size_of::<DivNode<'_>>(), 64);
    assert_eq!(core::mem::size_of::<DivVisual<'_>>(), 64);
    assert_eq!(core::mem::size_of::<DrawOpSnapshot>(), 12);
}

/// 2. 完整多色标渐变（任意角度 Linear、焦点 Radial、Conic、Pad/Repeat/Reflect、OKLab vs sRGB）。
#[test]
fn test_multi_stop_gradients_extend_modes_and_oklab() {
    const W: usize = 64;
    const H: usize = 64;
    const N: usize = W * H;

    // 2a. 三色标 135°（CSS 左上至右下）对角线性渐变（红 -> 绿 -> 蓝）
    {
        static STOPS: [ColorStop; 3] = [
            ColorStop::new(0.0, Color::rgb(255, 0, 0)),
            ColorStop::new(0.5, Color::rgb(0, 255, 0)),
            ColorStop::new(1.0, Color::rgb(0, 0, 255)),
        ];
        let cx = FrameScope::new();
        let root = div()
            .w(W as i16)
            .h(H as i16)
            .bg_linear_gradient(&cx, 135.0, &STOPS)
            .into_element(&cx);
        let buf = render_to_rgba::<N>(&root, &cx, W as i16, H as i16);
        let tl = px_at(&buf, W, 2, 2);
        let mid = px_at(&buf, W, 32, 32);
        let br = px_at(&buf, W, 61, 61);
        assert!(tl.r() > 210 && tl.b() < 40, "top-left should be red: {tl:?}");
        assert!(
            mid.g() > 200 && mid.r() < 60 && mid.b() < 60,
            "center should be green: {mid:?}"
        );
        assert!(br.b() > 210 && br.r() < 40, "bottom-right should be blue: {br:?}");
    }

    // 2b. OKLab vs sRGB 感知均匀插值：红(#FF0000) 到 绿(#00FF00) 中点，OKLab 亮度显著高于 sRGB 泥色中点
    {
        static RG: [ColorStop; 2] = [
            ColorStop::new(0.0, Color::rgb(255, 0, 0)),
            ColorStop::new(1.0, Color::rgb(0, 255, 0)),
        ];
        let cx_srgb = FrameScope::new();
        let r_srgb = div()
            .w(W as i16)
            .h(H as i16)
            .bg_linear_gradient(&cx_srgb, 90.0, &RG)
            .bg_gradient_space(GradientSpace::Srgb)
            .into_element(&cx_srgb);
        let b_srgb = render_to_rgba::<N>(&r_srgb, &cx_srgb, W as i16, H as i16);

        let cx_ok = FrameScope::new();
        let r_ok = div()
            .w(W as i16)
            .h(H as i16)
            .bg_linear_gradient(&cx_ok, 90.0, &RG)
            .bg_gradient_space(GradientSpace::Oklab)
            .into_element(&cx_ok);
        let b_ok = render_to_rgba::<N>(&r_ok, &cx_ok, W as i16, H as i16);

        let m_srgb = px_at(&b_srgb, W, 32, 32);
        let m_ok = px_at(&b_ok, W, 32, 32);
        let sum_srgb = m_srgb.r() as u32 + m_srgb.g() as u32;
        let sum_ok = m_ok.r() as u32 + m_ok.g() as u32;
        assert!(
            sum_ok > sum_srgb + 40,
            "OKLab midpoint ({m_ok:?}, sum={sum_ok}) should be noticeably brighter than sRGB ({m_srgb:?}, sum={sum_srgb})"
        );
    }

    // 2c. 焦点径向渐变（Focal Radial）
    {
        static STOPS: [ColorStop; 2] = [
            ColorStop::new(0.0, Color::rgb(255, 255, 255)),
            ColorStop::new(1.0, Color::rgb(0, 0, 200)),
        ];
        let cx = FrameScope::new();
        let root = div()
            .w(W as i16)
            .h(H as i16)
            .bg_radial_focal_gradient(&cx, [0.5, 0.5], 0.6, [0.25, 0.25], 0.0, &STOPS)
            .into_element(&cx);
        let buf = render_to_rgba::<N>(&root, &cx, W as i16, H as i16);
        let focal = px_at(&buf, W, 16, 16);
        let corner = px_at(&buf, W, 62, 62);
        assert!(
            focal.r() > 220 && focal.g() > 220,
            "focal point should be bright white: {focal:?}"
        );
        assert!(
            corner.r() < 40 && corner.b() > 170,
            "far corner should be deep blue: {corner:?}"
        );
    }

    // 2d. 锥形（角度）渐变（Conic：12 点钟方向 0° 顺时针旋转）
    {
        static STOPS: [ColorStop; 4] = [
            ColorStop::new(0.0, Color::rgb(255, 0, 0)),
            ColorStop::new(0.25, Color::rgb(0, 255, 0)),
            ColorStop::new(0.5, Color::rgb(0, 0, 255)),
            ColorStop::new(1.0, Color::rgb(255, 0, 0)),
        ];
        let cx = FrameScope::new();
        let root = div()
            .w(W as i16)
            .h(H as i16)
            .bg_conic_gradient(&cx, 0.0, &STOPS)
            .into_element(&cx);
        let buf = render_to_rgba::<N>(&root, &cx, W as i16, H as i16);
        let top = px_at(&buf, W, 33, 8); // ~0° -> 红
        let right = px_at(&buf, W, 56, 32); // ~90° (t=0.25) -> 绿
        let bottom = px_at(&buf, W, 32, 56); // ~180° (t=0.5) -> 蓝
        let left = px_at(&buf, W, 8, 32); // ~270° (t=0.75) -> 紫红过渡
        assert!(top.r() > 180, "12 o'clock (0 deg) should be red: {top:?}");
        assert!(right.g() > 180, "3 o'clock (90 deg) should be green: {right:?}");
        assert!(bottom.b() > 180, "6 o'clock (180 deg) should be blue: {bottom:?}");
        assert!(
            left.r() > 80 && left.b() > 80,
            "9 o'clock (270 deg) should blend blue/red: {left:?}"
        );
    }

    // 2e. ExtendMode: Pad vs Repeat vs Reflect
    {
        static STOPS: [ColorStop; 2] = [
            ColorStop::new(0.0, Color::rgb(255, 0, 0)),
            ColorStop::new(1.0, Color::rgb(0, 0, 255)),
        ];
        let sample_at_x = |extend: ExtendMode, x: usize| -> Color {
            let cx = FrameScope::new();
            let root = div()
                .w(W as i16)
                .h(16)
                .bg_paint(Paint::LinearGradient {
                    start: [0.0, 0.5],
                    end: [0.25, 0.5],
                    stops: &STOPS,
                    extend,
                    space: GradientSpace::Srgb,
                })
                .into_element(&cx);
            let buf = render_to_rgba::<{ W * 16 }>(&root, &cx, W as i16, 16);
            px_at(&buf, W, x, 8)
        };
        // x = 18 -> t = 18.5/16 = 1.156:
        // Pad => t=1.0 (纯蓝); Repeat => t=0.156 (偏红); Reflect at x=32 (t=2.03) => 回到 0.03 (纯红)
        let pad_c = sample_at_x(ExtendMode::Pad, 18);
        let rep_c = sample_at_x(ExtendMode::Repeat, 18);
        let ref_c = sample_at_x(ExtendMode::Reflect, 32);
        assert!(
            pad_c.b() > 240 && pad_c.r() < 15,
            "Pad should clamp to blue: {pad_c:?}"
        );
        assert!(
            rep_c.r() > 180,
            "Repeat at t=1.15 should wrap back near red: {rep_c:?}"
        );
        assert!(
            ref_c.r() > 210,
            "Reflect at t=2.0 should bounce back to red: {ref_c:?}"
        );
    }
}

/// 3. 四角独立圆角（`CornerRadii`）：验证指定圆角被精确裁透、直角保持完全不透明。
#[test]
fn test_per_corner_radii_quad() {
    const W: usize = 64;
    const H: usize = 64;
    let cx = FrameScope::new();
    let root = div()
        .w(W as i16)
        .h(H as i16)
        .bg(Color::rgb(20, 180, 120))
        .rounded_corners(CornerRadii::new(16, 0, 16, 0))
        .into_element(&cx);
    let buf = render_to_rgba::<{ W * H }>(&root, &cx, W as i16, H as i16);

    assert_eq!(px_at(&buf, W, 1, 1).a(), 0, "TL rounded corner must be transparent");
    assert_eq!(px_at(&buf, W, 62, 1).a(), 255, "TR sharp corner must be opaque");
    assert_eq!(px_at(&buf, W, 62, 62).a(), 0, "BR rounded corner must be transparent");
    assert_eq!(px_at(&buf, W, 1, 62).a(), 255, "BL sharp corner must be opaque");
    assert_eq!(px_at(&buf, W, 32, 32).a(), 255, "Center must be opaque");
}

/// 4. 27 种 Porter-Duff / W3C 混合模式与 2D 仿射旋转（零堆路径即可运行）。
#[test]
fn test_blend_modes_and_styled_quad_transform() {
    const W: usize = 48;
    const H: usize = 48;

    let blend_pair = |dst: Color, src: Color, mode: BlendMode| -> Color {
        let cx = FrameScope::new();
        let root = div()
            .w(W as i16)
            .h(H as i16)
            .bg(dst)
            .child(
                &cx,
                div()
                    .pos(0, 0)
                    .w(W as i16)
                    .h(H as i16)
                    .bg(src)
                    .blend_mode(mode),
            )
            .into_element(&cx);
        let buf = render_to_rgba::<{ W * H }>(&root, &cx, W as i16, H as i16);
        px_at(&buf, W, 24, 24)
    };

    // Multiply: (200, 100, 255) * (128, 255, 0) -> (~100, ~100, 0)
    let mul = blend_pair(
        Color::rgb(200, 100, 255),
        Color::rgb(128, 255, 0),
        BlendMode::Multiply,
    );
    assert!(
        (mul.r() as i16 - 100).abs() <= 4 && (mul.g() as i16 - 100).abs() <= 4 && mul.b() <= 2,
        "Multiply: {mul:?}"
    );

    // Screen: (128, 0, 0) screen (0, 128, 0) -> (128, 128, 0)
    let scr = blend_pair(
        Color::rgb(128, 0, 0),
        Color::rgb(0, 128, 0),
        BlendMode::Screen,
    );
    assert!(
        (scr.r() as i16 - 128).abs() <= 3 && (scr.g() as i16 - 128).abs() <= 3,
        "Screen: {scr:?}"
    );

    // Difference: |200 - 50| = 150, |40 - 200| = 160
    let diff = blend_pair(
        Color::rgb(200, 40, 100),
        Color::rgb(50, 200, 100),
        BlendMode::Difference,
    );
    assert!(
        (diff.r() as i16 - 150).abs() <= 3
            && (diff.g() as i16 - 160).abs() <= 3
            && diff.b() <= 3,
        "Difference: {diff:?}"
    );

    // Plus (Lighter): clamp(180 + 180) = 255
    let plus = blend_pair(
        Color::rgb(180, 60, 0),
        Color::rgb(180, 70, 50),
        BlendMode::Plus,
    );
    assert!(
        plus.r() >= 253
            && (plus.g() as i16 - 130).abs() <= 3
            && (plus.b() as i16 - 50).abs() <= 3,
        "Plus: {plus:?}"
    );

    // DstOut: opaque src punches full hole in dst -> alpha 0
    let dst_out = blend_pair(
        Color::rgb(255, 0, 0),
        Color::rgb(0, 255, 0),
        BlendMode::DstOut,
    );
    assert_eq!(dst_out.a(), 0, "DstOut should punch transparent hole: {dst_out:?}");

    // Luminosity: 保留底层色相/饱和度，替换为顶层高亮度
    let lum = blend_pair(
        Color::rgb(120, 20, 20),
        Color::rgb(220, 220, 220),
        BlendMode::Luminosity,
    );
    assert!(
        lum.r() > 220 && lum.g() > 160 && lum.r() > lum.g(),
        "Luminosity should lighten red while preserving red hue: {lum:?}"
    );

    // 45° 旋转 StyledQuad：在 64×64 画布中央放 32×32 正方形并旋转 45°
    // 旋转后顶点向上下左右伸展约 22.6px（超过原 16px 半宽），原正方形四角 (17,17) 变为画布外透明
    {
        const S: usize = 64;
        let cx = FrameScope::new();
        let root = div()
            .w(S as i16)
            .h(S as i16)
            .child(
                &cx,
                div()
                    .pos(16, 16)
                    .w(32)
                    .h(32)
                    .bg(Color::rgb(255, 80, 40))
                    .rotate_deg(45.0),
            )
            .into_element(&cx);
        let buf = render_to_rgba::<{ S * S }>(&root, &cx, S as i16, S as i16);
        let center = px_at(&buf, S, 32, 32);
        let tip_top = px_at(&buf, S, 32, 12);
        let old_corner = px_at(&buf, S, 17, 17);
        assert_eq!(center.a(), 255, "rotated center must be filled");
        assert!(
            tip_top.a() > 200,
            "rotated diamond top tip at (32,12) must be filled: {tip_top:?}"
        );
        assert_eq!(
            old_corner.a(),
            0,
            "original corner (17,17) must be empty after 45° rotation: {old_corner:?}"
        );
    }
}

/// 5. 脏矩形自动膨胀：带模糊或旋转的 `DrawOp::to_snapshot()` 必须覆盖溢出边界以防残影。
#[test]
fn test_damage_snapshot_expands_for_blur_and_transform() {
    let cx = FrameScope::new();
    let root = div()
        .w(200)
        .h(200)
        .child(
            &cx,
            div()
                .pos(50, 50)
                .w(40)
                .h(40)
                .bg(Color::rgb(255, 0, 0))
                .blur(8)
                .drop_shadow(6, 10, 12, Color::rgba(0, 0, 0, 180)),
        )
        .into_element(&cx);

    let vp = Region::new(0, 0, 200, 200);
    let mut motion = MotionCache::new();
    let mut ops: SmallVec<DrawOp<'_>, 32> = SmallVec::new();
    let mut hits: SmallVec<HitRegion<'_>, 32> = SmallVec::new();
    solve_and_flatten(cx.arena(), &root, vp, &mut motion, &mut ops, &mut hits);

    let layer_snap = ops
        .as_slice()
        .iter()
        .find(|op| matches!(op.payload, DrawPayload::PushLayer(_)))
        .expect("should emit PushLayer for blur+drop_shadow")
        .to_snapshot();

    assert!(
        layer_snap.effective_bounds.x < 50,
        "blur snapshot should expand left: {:?}",
        layer_snap.effective_bounds
    );
    assert!(
        layer_snap.effective_bounds.y < 50,
        "blur snapshot should expand top: {:?}",
        layer_snap.effective_bounds
    );
    assert!(
        layer_snap.effective_bounds.right() > 100,
        "drop_shadow+blur snapshot should expand right past 100: {:?}",
        layer_snap.effective_bounds
    );
    assert!(
        layer_snap.effective_bounds.bottom() > 102,
        "drop_shadow+blur snapshot should expand bottom past 102: {:?}",
        layer_snap.effective_bounds
    );
}

mod std_effects_tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn render_std<'a>(root: &'a Element<'a>, cx: &'a FrameScope, w: i16, h: i16) -> Vec<Rgba8888> {
        let vp = Region::new(0, 0, w, h);
        let mut motion = MotionCache::new();
        let mut ops: SmallVec<DrawOp<'_>, 128> = SmallVec::new();
        let mut hits: SmallVec<HitRegion<'_>, 128> = SmallVec::new();
        solve_and_flatten(cx.arena(), root, vp, &mut motion, &mut ops, &mut hits);
        let mut buf = vec![Rgba8888(0); (w as usize) * (h as usize)];
        rasterize_region(ops.as_slice(), vp, 0, 0, w as usize, &mut buf);
        buf
    }

    /// 6. 组级隔离透明度（`.isolate().opacity(128)`）：重叠子块绝不互相透底。
    #[test]
    fn test_group_opacity_isolation_vs_per_node_opacity() {
        const W: i16 = 80;
        const H: i16 = 80;

        let build = |isolate: bool| -> Vec<Rgba8888> {
            let cx = FrameScope::new();
            let mut group = div()
                .pos(10, 10)
                .w(60)
                .h(60)
                .opacity(128)
                .child(&cx, div().pos(0, 0).w(40).h(40).bg(Color::rgb(255, 0, 0)))
                .child(&cx, div().pos(20, 20).w(40).h(40).bg(Color::rgb(255, 0, 0)));
            if isolate {
                group = group.isolate();
            }
            let root = div()
                .w(W)
                .h(H)
                .bg(Color::WHITE)
                .child(&cx, group)
                .into_element(&cx);
            render_std(&root, &cx, W, H)
        };

        let iso = build(true);
        let non_iso = build(false);

        // 单块覆盖点 (20, 20) vs 重叠区 (35, 35)
        let iso_single = px_at(&iso, W as usize, 20, 20);
        let iso_overlap = px_at(&iso, W as usize, 35, 35);
        assert!(
            (iso_single.g() as i16 - iso_overlap.g() as i16).abs() <= 2,
            "Isolated group overlap ({iso_overlap:?}) must match single block ({iso_single:?})"
        );

        let non_single = px_at(&non_iso, W as usize, 20, 20);
        let non_overlap = px_at(&non_iso, W as usize, 35, 35);
        assert!(
            non_overlap.g() + 30 < non_single.g(),
            "Non-isolated overlap ({non_overlap:?}) should darken green channel compared to single ({non_single:?})"
        );
    }

    /// 7. 图层模糊（Blur）、ColorMatrix（灰度/反相）与任意子树 DropShadow。
    #[test]
    fn test_layer_blur_color_matrix_and_drop_shadow() {
        const W: i16 = 100;
        const H: i16 = 100;

        // 7a. Blur 溢出光晕
        {
            let cx = FrameScope::new();
            let root = div()
                .w(W)
                .h(H)
                .bg(Color::BLACK)
                .child(&cx, div().pos(30, 30).w(40).h(40).bg(Color::WHITE).blur(6))
                .into_element(&cx);
            let buf = render_std(&root, &cx, W, H);
            let halo = px_at(&buf, W as usize, 27, 50); // 矩形外 3px
            let far = px_at(&buf, W as usize, 5, 5);
            assert!(
                halo.r() > 20 && halo.r() < 245,
                "blur halo 3px outside should be mid-gray: {halo:?}"
            );
            assert!(far.r() < 5, "far pixel should stay black: {far:?}");
        }

        // 7b. ColorMatrix: grayscale & invert
        {
            let cx = FrameScope::new();
            let root = div()
                .w(W)
                .h(H)
                .bg(Color::BLACK)
                .child(
                    &cx,
                    div()
                        .pos(10, 10)
                        .w(30)
                        .h(30)
                        .bg(Color::rgb(255, 0, 0))
                        .grayscale(1.0),
                )
                .child(
                    &cx,
                    div()
                        .pos(60, 10)
                        .w(30)
                        .h(30)
                        .bg(Color::rgb(255, 0, 0))
                        .invert(1.0),
                )
                .into_element(&cx);
            let buf = render_std(&root, &cx, W, H);
            let gray = px_at(&buf, W as usize, 25, 25);
            let inv = px_at(&buf, W as usize, 75, 25);
            assert!(
                (gray.r() as i16 - gray.g() as i16).abs() <= 2
                    && (gray.g() as i16 - gray.b() as i16).abs() <= 2
                    && gray.r() > 40
                    && gray.r() < 120,
                "grayscale(red) should be neutral gray (~54): {gray:?}"
            );
            assert!(
                inv.r() < 5 && inv.g() > 250 && inv.b() > 250,
                "invert(red) should be cyan: {inv:?}"
            );
        }

        // 7c. DropShadow 偏移投影
        {
            let cx = FrameScope::new();
            let root = div()
                .w(W)
                .h(H)
                .bg(Color::WHITE)
                .child(
                    &cx,
                    div()
                        .pos(25, 25)
                        .w(40)
                        .h(40)
                        .bg(Color::rgb(240, 240, 240))
                        .drop_shadow(4, 10, 10, Color::rgba(0, 0, 0, 220)),
                )
                .into_element(&cx);
            let buf = render_std(&root, &cx, W, H);
            let shadow_pt = px_at(&buf, W as usize, 70, 70); // 右下偏移区
            let clear_pt = px_at(&buf, W as usize, 15, 15); // 左上无阴影区
            assert!(
                shadow_pt.r() < 160,
                "drop shadow region (70,70) should be darkened: {shadow_pt:?}"
            );
            assert!(
                clear_pt.r() > 250,
                "opposite region (15,15) should remain white: {clear_pt:?}"
            );
        }
    }

    /// 8. 实时毛玻璃（`backdrop_blur` / `backdrop_acrylic`）验证对下层双色背景的实时模糊融合。
    #[test]
    fn test_backdrop_blur_and_acrylic() {
        const W: i16 = 120;
        const H: i16 = 80;
        let cx = FrameScope::new();
        let root = div()
            .w(W)
            .h(H)
            .child(&cx, div().pos(0, 0).w(60).h(H).bg(Color::rgb(255, 0, 0)))
            .child(&cx, div().pos(60, 0).w(60).h(H).bg(Color::rgb(0, 0, 255)))
            .child(
                &cx,
                div()
                    .pos(20, 15)
                    .w(80)
                    .h(50)
                    .rounded(8)
                    .backdrop_acrylic(10, Color::rgba(255, 255, 255, 45)),
            )
            .into_element(&cx);
        let buf = render_std(&root, &cx, W, H);

        // 毛玻璃覆盖交界处 (60, 40) 应同时混合红蓝两通道
        let glass_seam = px_at(&buf, W as usize, 60, 40);
        assert!(
            glass_seam.r() > 60 && glass_seam.b() > 60,
            "backdrop acrylic over red/blue seam should mix both red & blue: {glass_seam:?}"
        );
        // 卡片外部上方 (55, 5) 仍保持纯红，(65, 5) 仍保持纯蓝
        let out_red = px_at(&buf, W as usize, 55, 5);
        let out_blue = px_at(&buf, W as usize, 65, 5);
        assert!(
            out_red.r() > 250 && out_red.b() < 5,
            "outside glass left must stay pure red: {out_red:?}"
        );
        assert!(
            out_blue.b() > 250 && out_blue.r() < 5,
            "outside glass right must stay pure blue: {out_blue:?}"
        );
    }

    /// 9. 任意 SVG Path 矢量裁剪遮罩与带子节点的 2D 变换离屏层。
    #[test]
    fn test_svg_clip_path_and_container_transform() {
        const W: i16 = 80;
        const H: i16 = 80;

        // 9a. 三角形 SVG Path 裁剪（标准 24×24 归一化坐标，自动适配 80×80）
        {
            let cx = FrameScope::new();
            let root = div()
                .w(W)
                .h(H)
                .bg(Color::rgb(10, 20, 30))
                .child(
                    &cx,
                    div()
                        .pos(0, 0)
                        .w(W)
                        .h(H)
                        .bg(Color::rgb(255, 60, 60))
                        .clip_path("M12 2 L22 22 L2 22 Z"),
                )
                .into_element(&cx);
            let buf = render_std(&root, &cx, W, H);
            let tl = px_at(&buf, W as usize, 6, 6);
            let tr = px_at(&buf, W as usize, 74, 6);
            let inside = px_at(&buf, W as usize, 40, 56);
            assert!(tl.r() < 25, "outside triangle top-left must show dark bg: {tl:?}");
            assert!(tr.r() < 25, "outside triangle top-right must show dark bg: {tr:?}");
            assert!(inside.r() > 240, "inside triangle must show red fill: {inside:?}");
        }

        // 9b. 带子节点的容器 45° 旋转（触发离屏层整体变换）
        {
            let cx = FrameScope::new();
            let root = div()
                .w(W)
                .h(H)
                .bg(Color::BLACK)
                .child(
                    &cx,
                    div()
                        .pos(20, 20)
                        .w(40)
                        .h(40)
                        .bg(Color::rgb(0, 200, 100))
                        .rotate_deg(45.0)
                        .child(&cx, div().pos(10, 10).w(20).h(20).bg(Color::rgb(255, 220, 0))),
                )
                .into_element(&cx);
            let buf = render_std(&root, &cx, W, H);
            let center = px_at(&buf, W as usize, 40, 40);
            let tip = px_at(&buf, W as usize, 40, 15);
            let corner = px_at(&buf, W as usize, 22, 22);
            assert!(
                center.r() > 220 && center.g() > 190,
                "rotated inner child center should be yellow: {center:?}"
            );
            assert!(tip.g() > 150, "rotated parent diamond tip should be green: {tip:?}");
            assert!(
                corner.g() < 15,
                "original corner (22,22) should be empty black after 45° rotation: {corner:?}"
            );
        }
    }

    /// 10. 真实硬件 GPU (`wgpu`) 离屏管线与软件光栅双后端像素级校验。
    #[cfg(feature = "gpu")]
    #[test]
    fn test_wgpu_hardware_pipeline_gradients_blends_filters_backdrop_clip() {
        use velox_engine::renderers::wgpu::WgpuOffscreenRenderer;

        let mut off = WgpuOffscreenRenderer::new(128, 128);
        if !off.ensure_gpu() {
            return;
        }

        fn gpu_render<'a>(
            off: &mut WgpuOffscreenRenderer,
            root: &'a Element<'a>,
            cx: &'a FrameScope,
            w: i16,
            h: i16,
        ) -> Vec<Color> {
            let vp = Region::new(0, 0, w, h);
            let mut motion = MotionCache::new();
            let mut ops: SmallVec<DrawOp<'_>, 128> = SmallVec::new();
            let mut hits: SmallVec<HitRegion<'_>, 128> = SmallVec::new();
            solve_and_flatten(cx.arena(), root, vp, &mut motion, &mut ops, &mut hits);
            let ok = off.render_draw_ops_scaled(ops.as_slice(), vp, 1.0);
            assert!(ok, "hardware WgpuOffscreenRenderer must succeed");
            off.rgba_buffer
                .chunks_exact(4)
                .map(|c| Color::rgba(c[0], c[1], c[2], c[3]))
                .collect()
        }

        // 场景 A：OKLab 渐变 + 四角独立圆角 + 45° 旋转（单 Draw Call `shader.wgsl` + `GradientRampAtlas`）
        {
            static STOPS: [ColorStop; 3] = [
                ColorStop::new(0.0, Color::rgb(255, 0, 0)),
                ColorStop::new(0.5, Color::rgb(0, 255, 0)),
                ColorStop::new(1.0, Color::rgb(0, 0, 255)),
            ];
            let cx = FrameScope::new();
            let root = div()
                .w(128)
                .h(128)
                .bg(Color::BLACK)
                .child(
                    &cx,
                    div()
                        .pos(0, 0)
                        .w(64)
                        .h(64)
                        .bg_linear_gradient(&cx, 135.0, &STOPS)
                        .bg_gradient_space(GradientSpace::Oklab)
                        .rounded_corners(CornerRadii::new(16, 0, 16, 0)),
                )
                .child(
                    &cx,
                    div()
                        .pos(80, 16)
                        .w(32)
                        .h(32)
                        .bg(Color::rgb(255, 160, 0))
                        .rotate_deg(45.0),
                )
                .into_element(&cx);

            let gpu = gpu_render(&mut off, &root, &cx, 128, 128);
            let sw = render_std(&root, &cx, 128, 128);

            assert!(gpu[1 * 128 + 1].r() < 10, "GPU TL rounded corner must show black bg");
            assert!(
                gpu[32 * 128 + 32].g() > 190,
                "GPU OKLab diagonal center must be bright green: {:?}",
                gpu[32 * 128 + 32]
            );
            assert!(
                gpu[12 * 128 + 96].r() > 200,
                "GPU rotated diamond top tip must be orange: {:?}",
                gpu[12 * 128 + 96]
            );
            let sw_mid = px_at(&sw, 128, 32, 32);
            let gp_mid = gpu[32 * 128 + 32];
            assert!(
                (sw_mid.g() as i16 - gp_mid.g() as i16).abs() <= 16,
                "GPU ({gp_mid:?}) vs SW ({sw_mid:?}) gradient center mismatch"
            );
        }

        // 场景 B：Multiply & Difference 可编程混合模式（`effects.wgsl`）
        {
            let cx = FrameScope::new();
            let root = div()
                .w(128)
                .h(64)
                .bg(Color::rgb(200, 100, 255))
                .child(
                    &cx,
                    div()
                        .pos(0, 0)
                        .w(64)
                        .h(64)
                        .bg(Color::rgb(128, 255, 0))
                        .blend_mode(BlendMode::Multiply),
                )
                .child(
                    &cx,
                    div()
                        .pos(64, 0)
                        .w(64)
                        .h(64)
                        .bg(Color::rgb(50, 200, 100))
                        .blend_mode(BlendMode::Difference),
                )
                .into_element(&cx);

            let gpu = gpu_render(&mut off, &root, &cx, 128, 64);
            let mul = gpu[32 * 128 + 32];
            let dif = gpu[32 * 128 + 96];
            assert!(
                (mul.r() as i16 - 100).abs() <= 6
                    && (mul.g() as i16 - 100).abs() <= 6
                    && mul.b() <= 4,
                "GPU Multiply mismatch: {mul:?}"
            );
            assert!(
                (dif.r() as i16 - 150).abs() <= 6
                    && (dif.g() as i16 - 100).abs() <= 6
                    && (dif.b() as i16 - 155).abs() <= 6,
                "GPU Difference mismatch: {dif:?}"
            );
        }

        // 场景 C：隔离透明度 + Dual Kawase Blur + ColorMatrix + DropShadow
        {
            let cx = FrameScope::new();
            let root = div()
                .w(128)
                .h(128)
                .bg(Color::WHITE)
                .child(
                    &cx,
                    div()
                        .pos(10, 10)
                        .w(50)
                        .h(50)
                        .opacity(128)
                        .isolate()
                        .child(&cx, div().pos(0, 0).w(35).h(35).bg(Color::rgb(255, 0, 0)))
                        .child(&cx, div().pos(15, 15).w(35).h(35).bg(Color::rgb(255, 0, 0))),
                )
                .child(
                    &cx,
                    div()
                        .pos(74, 16)
                        .w(36)
                        .h(36)
                        .bg(Color::rgb(255, 0, 0))
                        .grayscale(1.0)
                        .drop_shadow(4, 8, 8, Color::rgba(0, 0, 0, 210)),
                )
                .child(
                    &cx,
                    div()
                        .pos(32, 76)
                        .w(40)
                        .h(30)
                        .bg(Color::rgb(0, 0, 0))
                        .blur(6),
                )
                .into_element(&cx);

            let gpu = gpu_render(&mut off, &root, &cx, 128, 128);
            let iso_a = gpu[18 * 128 + 18];
            let iso_b = gpu[32 * 128 + 32];
            assert!(
                (iso_a.g() as i16 - iso_b.g() as i16).abs() <= 4,
                "GPU isolated group opacity overlap ({iso_b:?}) should match single ({iso_a:?})"
            );
            let gray = gpu[34 * 128 + 92];
            assert!(
                (gray.r() as i16 - gray.g() as i16).abs() <= 4
                    && gray.r() > 35
                    && gray.r() < 120,
                "GPU grayscale(red) should be neutral gray: {gray:?}"
            );
            let sh = gpu[55 * 128 + 113];
            assert!(
                sh.r() < 200,
                "GPU drop shadow region should darken white bg: {sh:?}"
            );
            let bl = gpu[91 * 128 + 29];
            assert!(
                bl.r() > 15 && bl.r() < 245,
                "GPU Kawase blur edge should soften: {bl:?}"
            );
        }

        // 场景 D：实时毛玻璃（Backdrop Acrylic）+ SVG Path 三角形矢量裁剪
        {
            let cx = FrameScope::new();
            let root = div()
                .w(128)
                .h(128)
                .child(&cx, div().pos(0, 0).w(64).h(64).bg(Color::rgb(255, 0, 0)))
                .child(&cx, div().pos(64, 0).w(64).h(64).bg(Color::rgb(0, 0, 255)))
                .child(
                    &cx,
                    div()
                        .pos(16, 12)
                        .w(96)
                        .h(40)
                        .rounded(8)
                        .backdrop_acrylic(8, Color::rgba(255, 255, 255, 50)),
                )
                .child(&cx, div().pos(0, 64).w(128).h(64).bg(Color::rgb(12, 18, 28)))
                .child(
                    &cx,
                    div()
                        .pos(32, 64)
                        .w(64)
                        .h(64)
                        .bg(Color::rgb(40, 220, 120))
                        .clip_path("M12 2 L22 22 L2 22 Z"),
                )
                .into_element(&cx);

            let gpu = gpu_render(&mut off, &root, &cx, 128, 128);
            let seam = gpu[32 * 128 + 64];
            assert!(
                seam.r() > 50 && seam.b() > 50,
                "GPU backdrop acrylic over red/blue seam should blend both channels: {seam:?}"
            );
            let tri_out = gpu[70 * 128 + 36];
            let tri_in = gpu[112 * 128 + 64];
            assert!(
                tri_out.g() < 40,
                "GPU SVG clip outside triangle should show dark bg: {tri_out:?}"
            );
            assert!(
                tri_in.g() > 190,
                "GPU SVG clip inside triangle should show green fill: {tri_in:?}"
            );
        }
    }
}
