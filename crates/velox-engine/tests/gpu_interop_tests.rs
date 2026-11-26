//! 阶段 A（外部零拷贝纹理导入 + 同设备共享）与阶段 B（跨 API 显存句柄零拷贝导出 +
//! 无锁三缓冲邮箱 + 统一 C ABI `embed_ffi` + Wayland `dmabuf`）端到端硬件闭环测试。

use core::mem::size_of;

use velox_engine::element::{DivNode, DivVisual};
use velox_engine::motion::DrawOpSnapshot;
use velox_engine::prelude::*;

struct ExternalRevisionView {
    rev: u64,
}

impl View for ExternalRevisionView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let rev = self.rev;
        div()
            .w(200)
            .h(120)
            .bg(Color::rgb(12, 16, 24))
            .p(10)
            .child(
                cx,
                div()
                    .w(120)
                    .h(80)
                    .rounded(12)
                    .external_texture_spec(
                        ExternalTextureSpec::new(ExternalTextureId(42))
                            .with_revision(rev)
                            .with_fallback_bg(Color::rgb(40, 120, 180)),
                    ),
            )
            .into_element(cx)
    }
}

#[test]
fn test_external_texture_zero_bloat_and_damage_revision() {
    // 1. 严守核心热数据结构零体积膨胀红线
    assert_eq!(size_of::<Style>(), 24, "Style must stay exactly 24 bytes");
    assert_eq!(
        size_of::<DivNode<'static>>(),
        64,
        "DivNode must stay exactly 64 bytes"
    );
    assert_eq!(
        size_of::<DivVisual<'static>>(),
        64,
        "DivVisual must stay exactly 64 bytes"
    );
    assert_eq!(
        size_of::<DrawOpSnapshot>(),
        12,
        "DrawOpSnapshot must stay exactly 12 bytes"
    );

    // 2. 验证 external_texture_revision 精准驱动局部脏区刷新与软件回退着色
    let mut engine = VeloxEngine::new(200, 120);
    let root = engine.mount_root(ExternalRevisionView { rev: 1 });
    let first_dirty = engine.step_frame_dt(16);
    assert!(!first_dirty.is_empty(), "first frame must be dirty");

    // 用软件光栅器渲染验证 ExternalTexture 自动降级为 fallback_bg
    let vp = engine.viewport_region();
    let mut fb = [Rgba8888(0); 200 * 120];
    rasterize_region(engine.cached_draw_ops(), vp, 0, 0, 200, &mut fb);
    let center = fb[50 * 200 + 50].to_color();
    assert_eq!(center.r(), 40);
    assert_eq!(center.g(), 120);
    assert_eq!(center.b(), 180);

    // 版本号不变时应零脏区
    let idle_dirty = engine.step_frame_dt(16);
    assert!(
        idle_dirty.is_empty(),
        "unchanged external texture revision should yield 0 dirty rects"
    );

    // 外部画面递增版本号后必须精确产出局部脏区
    engine.update_view(root, |v| v.rev = 2);
    let rev_dirty = engine.step_frame_dt(16);
    assert!(
        !rev_dirty.is_empty(),
        "incremented external_texture_revision must trigger damage"
    );
}

#[cfg(feature = "gpu")]
#[test]
fn test_stage_a_shared_device_external_texture_and_acrylic_hud() {
    use velox_engine::renderers::wgpu::{
        PreparedWgpuContext, WgpuOffscreenRenderer, wgpu,
    };

    let Some(ctx) = PreparedWgpuContext::from_host_adapter_id(None, None) else {
        // 无硬件 GPU 的 CI 环境自动跳过真实硬件断言
        return;
    };

    let device = ctx.device().clone();
    let queue = ctx.queue().clone();

    // 构造外部渲染引擎的 240x160 纹理：左半纯青 (0,220,255,255)，右半纯橙 (255,130,0,255)
    let make_split_tex = |left: [u8; 4], right: [u8; 4]| {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ext_radar_chart_tex"),
            size: wgpu::Extent3d {
                width: 240,
                height: 160,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut pixels = vec![0u8; 240 * 160 * 4];
        for y in 0..160usize {
            for x in 0..240usize {
                let c = if x < 120 { left } else { right };
                let idx = (y * 240 + x) * 4;
                pixels[idx..idx + 4].copy_from_slice(&c);
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(240 * 4),
                rows_per_image: Some(160),
            },
            wgpu::Extent3d {
                width: 240,
                height: 160,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    };

    let (_tex1, view1) = make_split_tex([0, 220, 255, 255], [255, 130, 0, 255]);
    let mut offscreen =
        WgpuOffscreenRenderer::from_shared_device(device.clone(), queue.clone(), 240, 160);
    let ext_id = ExternalTextureId(7);
    assert!(offscreen.register_external_texture_with_id(ext_id, view1, None));

    struct ChartWithAcrylicHudView {
        rev: u64,
    }
    impl View for ChartWithAcrylicHudView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let rev = self.rev;
            div()
                .w(240)
                .h(160)
                .bg(Color::rgb(10, 12, 18))
                .child(
                    cx,
                    div()
                        .w(240)
                        .h(160)
                        .rounded(18)
                        .external_texture_spec(
                            ExternalTextureSpec::new(ExternalTextureId(7))
                                .with_revision(rev)
                                .with_premultiplied(false),
                        )
                        // 在外部纹理之上横跨青/橙分界线叠放实时毛玻璃 HUD 卡片
                        .child(
                            cx,
                            div()
                                .pos(80, 44)
                                .w(80)
                                .h(72)
                                .rounded(10)
                                .backdrop_acrylic(8, Color::rgba(255, 255, 255, 28)),
                        ),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(240, 160);
    let root = engine.mount_root(ChartWithAcrylicHudView { rev: 1 });
    let _ = engine.step_frame_dt(16);
    assert!(offscreen.render_draw_ops_scaled(
        engine.cached_draw_ops(),
        engine.viewport_region(),
        1.0,
    ));

    let px = |buf: &[u8], x: usize, y: usize| -> [u8; 4] {
        let i = (y * 240 + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };

    // 1. 左侧裸露外部海图区域为强青色
    let left_chart = px(&offscreen.rgba_buffer, 30, 80);
    assert!(
        left_chart[0] < 30 && left_chart[1] > 190 && left_chart[2] > 220,
        "expected cyan external texture on left, got {left_chart:?}"
    );
    // 2. 右侧裸露外部海图区域为强橙色
    let right_chart = px(&offscreen.rgba_buffer, 210, 80);
    assert!(
        right_chart[0] > 220 && right_chart[1] > 100 && right_chart[2] < 30,
        "expected orange external texture on right, got {right_chart:?}"
    );
    // 3. 四角圆角外应裁切露出深色底层背景
    let corner = px(&offscreen.rgba_buffer, 1, 1);
    assert!(
        corner[0] < 25 && corner[1] < 25 && corner[2] < 35,
        "expected rounded corner clip showing dark bg, got {corner:?}"
    );
    // 4. 毛玻璃卡片跨越青/橙分界处（x=120 中心）必须同时融合青+橙两通道（r>70 且 b>70）
    let seam_blur = px(&offscreen.rgba_buffer, 120, 80);
    assert!(
        seam_blur[0] > 70 && seam_blur[2] > 70,
        "acrylic HUD over external texture seam must blur both cyan & orange channels, got {seam_blur:?}"
    );

    // 5. 热替换外部纹理视图为纯绿色，验证零拷贝即时刷新
    let (_tex2, view2) = make_split_tex([20, 240, 80, 255], [20, 240, 80, 255]);
    assert!(offscreen.update_external_texture(ext_id, view2));
    engine.update_view(root, |v| v.rev = 2);
    let _ = engine.step_frame_dt(16);
    assert!(offscreen.render_draw_ops_scaled(
        engine.cached_draw_ops(),
        engine.viewport_region(),
        1.0,
    ));

    let updated_left = px(&offscreen.rgba_buffer, 30, 80);
    assert!(
        updated_left[0] < 45 && updated_left[1] > 210 && updated_left[2] < 110,
        "expected hot-swapped green external texture, got {updated_left:?}"
    );
}

#[cfg(feature = "gpu")]
#[test]
fn test_stage_b_shared_exporter_triple_buffer_dx12_nt_handle_and_resize_gc() {
    use velox_engine::renderers::wgpu::{
        PreparedWgpuContext, WgpuSharedExportRenderer,
    };

    let Some(probe_ctx) = PreparedWgpuContext::from_host_adapter_id(None, None) else {
        return;
    };
    let luid = probe_ctx.adapter_luid();

    let Some(mut exporter) = WgpuSharedExportRenderer::new(160, 120, luid, None) else {
        return;
    };

    struct TransparentHudView;
    impl View for TransparentHudView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            // 根节点透明（无 bg），只在居中绘制一块亮蓝圆角 HUD 卡片
            div()
                .w(160)
                .h(120)
                .child(
                    cx,
                    div()
                        .pos(40, 30)
                        .w(80)
                        .h(60)
                        .rounded(10)
                        .bg(Color::rgb(30, 144, 255)),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(160, 120);
    let _ = engine.mount_root(TransparentHudView);
    let fence1 = exporter
        .step_and_render(&mut engine, 16)
        .expect("first frame must render and publish");
    assert!(fence1 >= 1);

    // 无锁三缓冲邮箱：首次 acquire 拿到 fresh 帧，二次 acquire 复用同槽位且 is_fresh == false
    let lease1 = exporter.acquire_shared_frame();
    assert!(lease1.is_fresh);
    assert_eq!(lease1.generation_id, 1);
    assert_eq!(lease1.width, 160);
    assert_eq!(lease1.height, 120);
    assert_eq!(lease1.fence_value, fence1);

    let lease1_repeat = exporter.acquire_shared_frame();
    assert!(!lease1_repeat.is_fresh);
    assert_eq!(lease1_repeat.slot_index, lease1.slot_index);

    // 回读共享槽位验证预乘透明背景 + 居中亮蓝 HUD 像素
    let bytes = exporter
        .readback_slot_bytes(lease1.slot_index)
        .expect("readback shared slot must succeed");
    let px = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * 160 + x) * 4;
        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
    };
    let cutout = px(4, 4);
    assert_eq!(
        cutout[3], 0,
        "transparent HUD background corner must have alpha == 0, got {cutout:?}"
    );
    let card_center = px(80, 60);
    assert_eq!(card_center[3], 255, "HUD card center must be opaque");
    // 兼容 BGRA/RGBA 通道顺序（DX12 默认 Bgra8Unorm，R/B 对调）
    let max_blue_or_red = card_center[0].max(card_center[2]);
    assert!(
        max_blue_or_red > 220 && card_center[1] > 120,
        "HUD card center should be bright blue (RGBA or BGRA), got {card_center:?}"
    );

    // Windows + gpu-interop 下验证真实 DX12 NT HANDLE & ID3D12Fence 可被原生 OpenSharedHandle 打开
    #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
    {
        let ExportedHandle::Win32Dx12NtHandle { nt_handle, .. } = lease1.handle else {
            panic!(
                "expected Win32Dx12NtHandle on Windows gpu-interop, got {:?}",
                lease1.handle
            );
        };
        let ExternalSyncHandle::Win32Dx12FenceNtHandle(fence_handle) = lease1.sync_handle else {
            panic!(
                "expected Win32Dx12FenceNtHandle on Windows gpu-interop, got {:?}",
                lease1.sync_handle
            );
        };
        assert_ne!(nt_handle, 0);
        assert_ne!(fence_handle, 0);

        let verified = velox_engine::renderers::wgpu::interop::verify_dx12_exported_handles(
            exporter.device(),
            nt_handle,
            fence_handle,
        )
        .expect("ID3D12Device::OpenSharedHandle must open both exported texture and fence");
        assert_eq!(verified.0, 160, "opened DX12 shared texture width mismatch");
        assert_eq!(verified.1, 120, "opened DX12 shared texture height mismatch");
        assert!(
            verified.2 >= fence1,
            "opened DX12 shared fence completed value {} < {}",
            verified.2,
            fence1
        );
    }

    // Resize 触发代数递增 + 旧显存池进入 retired_pools 延迟 GC；归还旧租约后安全释放归零
    exporter.resize(200, 140);
    assert_eq!(exporter.generation_id(), 2);
    assert_eq!(exporter.retired_pool_count(), 1);
    exporter.release_shared_frame(1, lease1.slot_index);
    assert_eq!(exporter.retired_pool_count(), 0);
}

#[test]
fn test_stage_b_embed_ffi_gpu_and_software_fallback_and_wayland_dmabuf() {
    use core::sync::atomic::{AtomicU32, Ordering};
    use velox_engine::platform::embed_ffi::{
        VELOX_EMBED_HEADER_H, VeloxEmbedSession, VeloxHandleKind, VeloxHostAdapterConfig,
        VeloxSharedFrameDesc, velox_embed_acquire_shared_frame, velox_embed_backend_is_gpu,
        velox_embed_destroy, velox_embed_inject_key, velox_embed_inject_pointer,
        velox_embed_inject_wheel, velox_embed_release_shared_frame, velox_embed_resize,
        velox_embed_step_frame,
    };
    use velox_engine::platform::wayland::{DmaBufPlaneDesc, WaylandDmaBufDriver};

    static CLICKS: AtomicU32 = AtomicU32::new(0);
    CLICKS.store(0, Ordering::Relaxed);

    struct FfiButtonView;
    impl View for FfiButtonView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            div()
                .w(120)
                .h(80)
                .bg(Color::rgb(20, 40, 80))
                .child(
                    cx,
                    div()
                        .id(101)
                        .w(80)
                        .h(40)
                        .bg(Color::rgb(220, 60, 40))
                        .on_click(cx, &|_ev, _acx| {
                            CLICKS.fetch_add(1, Ordering::Relaxed);
                        }),
                )
                .into_element(cx)
        }
    }

    // 1. 测试 Software 回退模式 C ABI 全生命周期
    let sw_cfg = VeloxHostAdapterConfig {
        prefer_gpu: 0,
        transparent_bg: 0,
        ..Default::default()
    };
    let sw_ptr = Box::into_raw(Box::new(VeloxEmbedSession::new(
        FfiButtonView,
        120,
        80,
        1.0,
        sw_cfg,
    )));
    unsafe {
        assert_eq!(velox_embed_backend_is_gpu(sw_ptr), 0);
        assert_eq!(velox_embed_step_frame(sw_ptr, 16), 1);

        let mut frame = VeloxSharedFrameDesc::default();
        assert_eq!(velox_embed_acquire_shared_frame(sw_ptr, &mut frame), 1);
        assert_eq!(frame.handle_kind, VeloxHandleKind::SoftwareRgbaPtr as u8);
        assert_eq!(frame.width, 120);
        assert_eq!(frame.height, 80);
        assert!(!frame.software_rgba_ptr.is_null());
        // 左上角按钮处应为红色 (220, 60, 40, 255)
        let slice = core::slice::from_raw_parts(frame.software_rgba_ptr, 120 * 80 * 4);
        let idx = (20 * 120 + 20) * 4;
        assert_eq!(slice[idx], 220);
        assert_eq!(slice[idx + 1], 60);
        assert_eq!(slice[idx + 2], 40);
        assert_eq!(slice[idx + 3], 255);

        // 指针点击按钮触发回调 + 滚轮/键盘接口安全性验证
        assert_eq!(velox_embed_inject_pointer(sw_ptr, 0, 20, 20), 1);
        assert_eq!(velox_embed_inject_pointer(sw_ptr, 2, 20, 20), 1);
        assert_eq!(CLICKS.load(Ordering::Relaxed), 1);
        let _ = velox_embed_inject_wheel(sw_ptr, 20, 20, 12);
        let _ = velox_embed_inject_key(sw_ptr, 9, 0);

        // Resize + Release + Destroy
        velox_embed_resize(sw_ptr, 160, 100, 1.0);
        assert_eq!(velox_embed_step_frame(sw_ptr, 16), 1);
        assert_eq!(velox_embed_acquire_shared_frame(sw_ptr, &mut frame), 1);
        assert_eq!(frame.width, 160);
        assert_eq!(frame.height, 100);
        assert_eq!(frame.generation_id, 2);
        velox_embed_release_shared_frame(sw_ptr, 1, 0);
        velox_embed_destroy(sw_ptr);
    }

    // 2. GPU 模式下测试 C ABI（若存在可用硬件 GPU）
    #[cfg(feature = "gpu")]
    {
        let gpu_cfg = VeloxHostAdapterConfig {
            prefer_gpu: 1,
            transparent_bg: 1,
            ..Default::default()
        };
        let gpu_ptr = Box::into_raw(Box::new(VeloxEmbedSession::new(
            FfiButtonView,
            120,
            80,
            1.0,
            gpu_cfg,
        )));
        unsafe {
            let is_gpu = velox_embed_backend_is_gpu(gpu_ptr);
            assert_eq!(velox_embed_step_frame(gpu_ptr, 16), 1);
            let mut frame = VeloxSharedFrameDesc::default();
            assert_eq!(velox_embed_acquire_shared_frame(gpu_ptr, &mut frame), 1);
            assert_eq!(frame.width, 120);
            assert_eq!(frame.height, 80);
            if is_gpu == 1 {
                assert!(frame.software_rgba_ptr.is_null());
                #[cfg(all(feature = "gpu-interop", target_os = "windows"))]
                {
                    assert_eq!(frame.handle_kind, VeloxHandleKind::Win32Dx12NtHandle as u8);
                    assert_ne!(frame.raw_handle, 0);
                    assert_ne!(frame.sync_handle, 0);
                }
            }
            velox_embed_release_shared_frame(gpu_ptr, frame.generation_id, frame.slot_index);
            velox_embed_destroy(gpu_ptr);
        }
    }

    // 3. 验证 C 头文件模板与 Wayland zwp_linux_dmabuf_v1 wire 编码器
    assert!(VELOX_EMBED_HEADER_H.contains("velox_embed_acquire_shared_frame"));
    assert!(VELOX_EMBED_HEADER_H.contains("VELOX_HANDLE_WIN32_DX12_NT"));

    let mut dmabuf = WaylandDmaBufDriver::new(11, 22, 100);
    let pkt = dmabuf.encode_present_packet(
        DmaBufPlaneDesc {
            fd: 7,
            plane_idx: 0,
            width: 1920,
            height: 1080,
            offset: 0,
            stride: 1920 * 4,
            drm_format: 0x3432_5241,
            modifier: 0,
        },
        &[Region {
            x: 10,
            y: 20,
            w: 300,
            h: 200,
        }],
    );
    assert_eq!(dmabuf.submitted_frames(), 1);
    assert_eq!(pkt.attached_fds, vec![7]);
    assert!(pkt.wl_buffer_id >= 100);
    assert!(
        pkt.wire_bytes.len() >= 100 && pkt.wire_bytes.len() % 4 == 0,
        "Wayland wire message must be 32-bit aligned and non-empty"
    );
}
