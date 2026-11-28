//! Run with `cargo test -p velox-engine --release --test performance_hot_paths -- --ignored --nocapture --test-threads=1`.

use std::{hint::black_box, time::Instant};
use velox_engine::motion::{DrawOp, DrawPayload};
use velox_engine::prelude::{Color, Region};

fn quad(bounds: Region, color: Color) -> DrawOp<'static> {
    DrawOp {
        id: None,
        bounds,
        clip: bounds,
        clip_radius: 0,
        subpx_q4: 0,
        payload: DrawPayload::Quad {
            bg: color,
            bg_grad: color,
            border_color: Color::TRANSPARENT,
            rounded: 0,
            border_width: 0,
        },
    }
}

fn median_us(iterations: u32, mut run: impl FnMut()) -> f64 {
    for _ in 0..10 {
        run();
    }
    let mut samples = [0.0; 7];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..iterations {
            run();
        }
        *sample = start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations);
    }
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

#[test]
#[ignore = "performance measurement"]
fn desktop_sparse_grid() {
    use velox_engine::prelude::{Xrgb8888, rasterize_region, rasterize_region_parallel};

    const WIDTH: usize = 1200;
    const HEIGHT: usize = 640;
    let viewport = Region::new(0, 0, WIDTH as i16, HEIGHT as i16);
    let mut ops = vec![quad(viewport, Color::rgb(16, 24, 38))];
    ops.extend((0..2000).map(|i| {
        let bounds = Region::new((i % 50) * 24 + 2, (i / 50) * 16 + 2, 20, 12);
        quad(bounds, Color::rgb(50, 80, 140))
    }));

    #[cfg(feature = "gpu")]
    {
        let mut scene = velox_engine::renderers::wgpu::WgpuScene::new();
        let us = median_us(50, || {
            black_box(scene.record_draw_ops(black_box(&ops), 1, &[viewport]));
        });
        assert_eq!(scene.quad_count, ops.len());
        println!("wgpu scene lowering (2001 ops): {us:.2} us");
    }

    let mut pixels = vec![Xrgb8888(0); WIDTH * HEIGHT];
    let mut expected = pixels.clone();
    rasterize_region(&ops, viewport, 0, 0, WIDTH, &mut expected);
    let us = median_us(25, || {
        rasterize_region_parallel(black_box(&ops), viewport, 0, 0, WIDTH, &mut pixels);
        black_box(&pixels);
    });
    assert_eq!(pixels, expected);
    println!("parallel software raster (2001 ops, 1200x640): {us:.2} us");
}

#[test]
#[ignore = "performance measurement"]
fn desktop_hidpi_text_multistrip_profile() {
    use velox_engine::prelude::*;

    struct TextDenseView {
        rows: usize,
        cols: usize,
    }
    impl View for TextDenseView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut root = div().flex_col().size_full().bg(Color::rgb(18, 22, 32)).p(8).gap(4);
            for r in 0..self.rows {
                let mut row = div().flex_row().gap(6);
                for c in 0..self.cols {
                    let label = if (r + c) % 3 == 0 {
                        "Velox High-DPI Typography Benchmark 文本性能实测 #128"
                    } else if (r + c) % 3 == 1 {
                        "Latency: 1.42ms | FPS: 120 | 帧率监控与条带度量"
                    } else {
                        "Status: Active (200% Scale) - 持久渲染目标评估"
                    };
                    row = row.child(
                        cx,
                        div()
                            .flex_grow(1)
                            .h(22)
                            .rounded(4)
                            .bg(Color::rgb(28, 34, 48))
                            .px(6)
                            .items_center()
                            .child(cx, text(label).size(13).color(Color::rgb(230, 236, 245))),
                    );
                }
                root = root.child(cx, row);
            }
            root.into_element(cx)
        }
    }

    let logical_w: i16 = 960;
    let logical_h: i16 = 540;
    let viewport = Region::new(0, 0, logical_w, logical_h);

    for sf in [1.0f32, 1.5f32, 2.0f32] {
        let mut engine = VeloxEngine::new(logical_w, logical_h);
        engine.set_scale_factor(sf);
        engine.mount_root(TextDenseView { rows: 20, cols: 4 });
        let _ = engine.step_frame();
        let text_ops: Vec<(String, u8, i16, u8)> = {
            let ops = engine.cached_draw_ops();
            ops.iter()
                .filter_map(|op| match op.payload {
                    DrawPayload::TextRun {
                        text,
                        font_size,
                        text_flags,
                        ..
                    } => Some((text.as_str().to_owned(), font_size, op.bounds.w, text_flags)),
                    _ => None,
                })
                .collect()
        };

        let phys_w = ((logical_w as f32) * sf).ceil() as usize;
        let phys_h = ((logical_h as f32) * sf).ceil() as usize;
        let num_strips = phys_h.div_ceil(16);
        let mut pixels = vec![Xrgb8888(0); phys_w * phys_h];

        // 预热字体缓存后再测稳态每帧耗时
        engine.rasterize_strip(viewport, 0, 0, phys_w, &mut pixels);

        let full_frame_us = median_us(15, || {
            engine.rasterize_strip(viewport, 0, 0, phys_w, &mut pixels);
            black_box(&pixels);
        });

        // 独立测量单遍遍历全部 text_ops 执行 count_text_lines + measure_line_width_f32 的纯度量成本
        let single_pass_metrics_us = median_us(100, || {
            let mut acc = 0i32;
            for (s, fs, w, flags) in &text_ops {
                let is_ellipsis = (*flags & 0x04) != 0;
                let is_nowrap = (*flags & 0x08) != 0;
                acc += velox_engine::display::count_text_lines(
                    s.as_str(),
                    (*fs).max(8),
                    *w,
                    is_nowrap || is_ellipsis,
                ) as i32;
                acc += velox_engine::display::measure_line_width_f32(s.as_str(), (*fs).max(8)) as i32;
            }
            black_box(acc);
        });

        // 当前多条带实现会在每个 16px 条带内对所有 TextRun 无条件调用一次 compute_scaled_text_box
        let est_redundant_metrics_us = if sf > 1.01 {
            single_pass_metrics_us * (num_strips.saturating_sub(1) as f64)
        } else {
            0.0
        };
        let threads = velox_engine::prelude::rasterize_parallel_threads() as f64;
        let est_wall_redundant_us = est_redundant_metrics_us / threads.max(1.0);

        println!(
            "[HiDPI Text Profile] sf={sf:.1} ({phys_w}x{phys_h}, strips={num_strips}, threads={threads:.0}, text_ops={}): \
             full_frame={full_frame_us:.1} us | 1x_metrics={single_pass_metrics_us:.1} us | \
             redundant_metrics_cpu={est_redundant_metrics_us:.1} us (wall~={est_wall_redundant_us:.1} us, ~{:.1}% of frame)",
            text_ops.len(),
            (est_wall_redundant_us / full_frame_us.max(1.0)) * 100.0
        );
    }
}

#[cfg(all(feature = "std", feature = "gpu"))]
#[test]
#[ignore = "performance measurement"]
fn desktop_gpu_persistent_target_partial_repaint_profile() {
    use velox_engine::prelude::*;
    use velox_engine::renderers::wgpu::{PreparedWgpuContext, WgpuRenderer};

    let Some(prepared) = PreparedWgpuContext::init_headless() else {
        println!("[GPU Profile] No hardware wgpu adapter available, skipping.");
        return;
    };
    let device = prepared.device().clone();
    let queue = prepared.queue().clone();
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let mut renderer = WgpuRenderer::new(&device, format);
    println!(
        "[GPU Profile] Adapter={} ({})",
        prepared.adapter_name(),
        prepared.backend_name()
    );

    for (label, w, h, sf) in [
        ("1080p@1x", 1920u32, 1080u32, 1.0f32),
        ("4K@2x", 3840u32, 2160u32, 2.0f32),
    ] {
        let log_w = (w as f32 / sf) as i16;
        let log_h = (h as f32 / sf) as i16;
        let viewport = Region::new(0, 0, log_w, log_h);

        // 构造真实中等复杂度桌面界面：全屏不透明背景 + 480 圆角卡片 + 120 文本（含大字 Slug）
        let mut ops: Vec<DrawOp<'static>> = Vec::with_capacity(620);
        ops.push(quad(viewport, Color::rgb(16, 20, 28)));
        for i in 0..480i16 {
            let col = i % 24;
            let row = i / 24;
            let cw = (log_w - 40) / 24;
            let ch = (log_h - 40) / 20;
            let b = Region::new(20 + col * cw + 2, 20 + row * ch + 2, (cw - 4).max(8), (ch - 4).max(8));
            ops.push(DrawOp {
                id: Some(i as u32 + 1),
                bounds: b,
                clip: viewport,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::Quad {
                    bg: Color::rgb(28 + (col as u8) * 2, 34 + (row as u8) * 3, 52),
                    bg_grad: Color::rgb(22, 28, 44),
                    border_color: Color::rgba(90, 130, 200, 120),
                    rounded: 6,
                    border_width: 1,
                },
            });
        }
        for i in 0..120i16 {
            let row = i / 6;
            let col = i % 6;
            let cw = (log_w - 40) / 6;
            let b = Region::new(24 + col * cw, 24 + row * 24, cw - 8, 20);
            ops.push(DrawOp {
                id: Some(1000 + i as u32),
                bounds: b,
                clip: viewport,
                clip_radius: 0,
                subpx_q4: 0,
                payload: DrawPayload::TextRun {
                    text: StrRef::Static("Velox GPU Persistent Target Test 128.4ms"),
                    color: Color::rgb(230, 236, 245),
                    font_size: if i % 10 == 0 { 28 } else { 13 },
                    text_flags: 0,
                },
            });
        }

        let persistent_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bench_persistent_rt"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let persistent_view = persistent_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let swap_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bench_swap_target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let swap_view = swap_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let clear = wgpu::Color {
            r: 16.0 / 255.0,
            g: 20.0 / 255.0,
            b: 28.0 / 255.0,
            a: 1.0,
        };

        // 首帧预热
        renderer.render_to_view(&device, &queue, &ops, viewport, sf, &persistent_view, w, h, clear);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());

        // 1. 全屏渲染（当前 WgpuWindowRenderer 现状）
        let full_us = median_us(15, || {
            renderer.render_to_view(&device, &queue, &ops, viewport, sf, &swap_view, w, h, clear);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        });

        // 2. 纯整帧硬件拷贝 copy_texture_to_texture 开销
        let copy_only_us = median_us(20, || {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.copy_texture_to_texture(
                persistent_tex.as_image_copy(),
                swap_tex.as_image_copy(),
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([enc.finish()]);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        });

        // 3. 持久渲染目标局部 Scissor 重绘 + copy_texture_to_texture（3 档脏区）
        let scenarios = [
            ("caret(4x24)", Region::new(120, 120, 4, 24)),
            ("button(160x48)", Region::new(120, 120, 160, 48)),
            ("quarter", Region::new(0, 0, log_w / 2, log_h / 2)),
        ];
        println!("[GPU Profile {label}] full_clear_render={full_us:.1} us | copy_full_tex={copy_only_us:.1} us");
        for (sname, dirty) in scenarios {
            let dirty_only_us = median_us(15, || {
                renderer.render_to_view_dirty(
                    &device,
                    &queue,
                    &ops,
                    viewport,
                    sf,
                    &persistent_view,
                    w,
                    h,
                    &[dirty],
                    clear,
                );
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            });
            let dirty_plus_copy_us = median_us(15, || {
                let used = renderer.render_to_view_dirty_with_copy(
                    &device,
                    &queue,
                    &ops,
                    viewport,
                    sf,
                    &persistent_view,
                    w,
                    h,
                    &[dirty],
                    clear,
                    Some((&persistent_tex, &swap_tex)),
                );
                assert!(used);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            });
            println!(
                "  -> {sname:15}: dirty_scissor_only={dirty_only_us:.1} us | dirty+copy(1-enc)={dirty_plus_copy_us:.1} us (vs full: {:+.1}%)",
                ((dirty_plus_copy_us - full_us) / full_us.max(1.0)) * 100.0
            );
        }
    }
}

