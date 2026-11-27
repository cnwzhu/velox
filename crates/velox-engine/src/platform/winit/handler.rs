//! `winit` 主窗口事件处理器（`WinitAppHandler`）：分数 DPI 同步、GPU/软光栅桥接、原生 IME 光标跟随与 0% CPU 休眠。

#[cfg(feature = "winit")]
use core::num::NonZeroU32;
#[cfg(feature = "winit")]
use std::eprintln;
#[cfg(feature = "winit")]
use std::println;
#[cfg(feature = "winit")]
use std::string::String;
#[cfg(feature = "winit")]
use std::sync::Arc;
#[cfg(feature = "winit")]
use std::time::{Duration, Instant};
#[cfg(feature = "winit")]
use std::vec::Vec;

#[cfg(feature = "winit")]
use winit::application::ApplicationHandler;
#[cfg(feature = "winit")]
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
#[cfg(feature = "winit")]
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
#[cfg(feature = "winit")]
use winit::event_loop::{ActiveEventLoop, ControlFlow};
#[cfg(feature = "winit")]
use winit::keyboard::{Key, NamedKey};
#[cfg(feature = "winit")]
use winit::window::{CursorIcon, ImePurpose, Theme as WinitTheme, Window, WindowId};

#[cfg(feature = "winit")]
use super::super::shm_driver::{SoftwareSurfaceDriver, WlSurface};
#[cfg(all(feature = "winit", feature = "gpu"))]
use super::super::DesktopRenderBackend;
#[cfg(feature = "winit")]
use crate::prelude::*;

#[cfg(feature = "winit")]
pub(super) enum WinitUserEvent {
    #[cfg(feature = "gpu")]
    GpuReady(Option<crate::renderers::wgpu::PreparedWgpuContext>),
    /// 后台线程池完成或定时器到期触发 UI 主线程零轮询异步唤醒。
    WakeAsync,
}

#[cfg(feature = "winit")]
pub(super) struct WinitAppHandler {
    pub(super) title: String,
    pub(super) logical_w: usize,
    pub(super) logical_h: usize,
    pub(super) phys_w: usize,
    pub(super) phys_h: usize,
    pub(super) scale_factor: f32,
    pub(super) dpi_scale_explicit: bool,
    pub(super) core: VeloxEngine,
    pub(super) scroll_list_id: Option<u32>,
    pub(super) driver: SoftwareSurfaceDriver,
    pub(super) wl_surface: WlSurface,
    #[cfg(feature = "gpu")]
    pub(super) gpu_surface: Option<crate::renderers::wgpu::WgpuWindowRenderer>,
    #[cfg(feature = "gpu")]
    pub(super) pending_gpu: Option<crate::renderers::wgpu::PreparedWgpuContext>,
    #[cfg(feature = "gpu")]
    pub(super) gpu_warmup_in_flight: bool,
    pub(super) surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pub(super) window: Option<Arc<Window>>,
    pub(super) window_shown: bool,
    pub(super) force_present_once: bool,
    pub(super) last_window_theme_dark: Option<bool>,
    pub(super) cursor_pos: (i16, i16),
    pub(super) mouse_down: bool,
    pub(super) last_tick: Instant,
    pub(super) last_ime_area: Option<Region>,
    pub(super) last_cursor: CursorShape,
}

#[cfg(feature = "winit")]
#[inline]
fn to_winit_cursor(shape: CursorShape) -> CursorIcon {
    match shape {
        CursorShape::Default => CursorIcon::Default,
        CursorShape::Pointer => CursorIcon::Pointer,
        CursorShape::Text => CursorIcon::Text,
        // Windows 原生系统光标集不含 Grab/Grabbing，回退为标准小手 Pointer 避免退化成箭头或十字
        #[cfg(target_os = "windows")]
        CursorShape::Grab | CursorShape::Grabbing => CursorIcon::Pointer,
        #[cfg(not(target_os = "windows"))]
        CursorShape::Grab => CursorIcon::Grab,
        #[cfg(not(target_os = "windows"))]
        CursorShape::Grabbing => CursorIcon::Grabbing,
        CursorShape::NotAllowed => CursorIcon::NotAllowed,
        CursorShape::Progress => CursorIcon::Progress,
        CursorShape::Move => CursorIcon::Move,
        CursorShape::Crosshair => CursorIcon::Crosshair,
        CursorShape::ResizeNS => CursorIcon::NsResize,
        CursorShape::ResizeEW => CursorIcon::EwResize,
        CursorShape::ResizeNWSE => CursorIcon::NwseResize,
        CursorShape::ResizeNESW => CursorIcon::NeswResize,
    }
}

#[cfg(feature = "winit")]
#[inline]
fn is_color_dark(c: Color) -> bool {
    let luma = (c.r() as u32) * 299 + (c.g() as u32) * 587 + (c.b() as u32) * 114;
    luma < 128_000
}

#[cfg(feature = "winit")]
#[inline]
fn infer_scene_bg(ops: &[DrawOp<'_>], fallback: Color) -> Color {
    for op in ops {
        if let DrawPayload::Quad { bg, .. } = op.payload {
            if bg.a() >= 250 {
                return bg;
            }
        }
    }
    fallback
}

#[cfg(feature = "winit")]
#[inline]
fn should_defer_window_visible() -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        true
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        // Linux Wayland 由合成器在首次提交缓冲时原子上屏，不会白屏且隐身可能延迟 configure；X11 则延迟显窗防白屏
        std::env::var_os("WAYLAND_DISPLAY").is_none()
    }
}

#[cfg(feature = "winit")]
impl WinitAppHandler {
    /// 初始化 `softbuffer` 软件呈现表面（仅耗时 ~5ms，用于首屏秒开或无 GPU 回退）。
    fn attach_softbuffer_surface(&mut self, win: &Arc<Window>) {
        self.driver.resize(self.phys_w.max(1), self.phys_h.max(1));
        if let Ok(ctx) = softbuffer::Context::new(win.clone()) {
            if let Ok(mut surf) = softbuffer::Surface::new(&ctx, win.clone()) {
                if let (Some(nw), Some(nh)) = (
                    NonZeroU32::new(self.phys_w.max(1) as u32),
                    NonZeroU32::new(self.phys_h.max(1) as u32),
                ) {
                    let _ = surf.resize(nw, nh);
                }
                self.surface = Some(surf);
            }
        }
    }

    /// 当后台预热的 GPU 上下文就绪且窗口尺寸有效时，主线程仅耗时 ~10ms 完成 Swapchain 绑定与零闪烁热升舱，
    /// 并立即将临时软件双缓冲收缩至 1×1 释放约 18MB 内存。
    #[cfg(feature = "gpu")]
    fn try_promote_to_gpu(&mut self) {
        if self.gpu_surface.is_some() || self.phys_w == 0 || self.phys_h == 0 {
            return;
        }
        let Some(win) = self.window.clone() else {
            return;
        };
        let Some(prepared) = self.pending_gpu.take() else {
            return;
        };

        // 绑定 Swapchain 前再次核对窗口真实物理尺寸与 OS DPI，避免启动期 Resize/DPI 事件与后台热升舱交错导致尺寸失配
        let actual_size = win.inner_size();
        if actual_size.width == 0 || actual_size.height == 0 {
            self.pending_gpu = Some(prepared);
            return;
        }
        let actual_scale = win.scale_factor();
        self.sync_window_dimensions(actual_size.width, actual_size.height, Some(actual_scale));

        // 在销毁 softbuffer 前先确保引擎本帧绘制指令已就绪且最新（消除 Resize 后 cached_draw_ops 为空导致跳过首帧提交的空窗）
        self.core.ensure_frame_ops_ready();

        // 先释放 GDI/softbuffer 句柄再绑定 DXGI/Vulkan/Metal Swapchain
        self.surface = None;

        if let Some(mut gpu_surf) = crate::renderers::wgpu::WgpuWindowRenderer::from_prepared(
            prepared,
            win.clone(),
            self.phys_w.max(1) as u32,
            self.phys_h.max(1) as u32,
        ) {
            // 立即将当前已布局的缓存帧热提交至 GPU Swapchain 并同步等待，保证切换瞬间 0 闪烁、0 抖动
            let viewport = self.core.viewport_region();
            let ops = self.core.cached_draw_ops();
            let sf = self.core.scale_factor();
            let scene_bg = infer_scene_bg(ops, self.core.theme().background);
            if !ops.is_empty() && gpu_surf.render_scaled_with_clear(ops, viewport, sf, scene_bg) {
                gpu_surf.flush_and_wait();
                self.force_present_once = false;
                self.last_tick = Instant::now();
            }

            println!(
                "[Velox Winit + wgpu Backend] 硬件加速已就绪 ({} / {}, scale={:.2}x, 逻辑 {}x{}, 物理 HiDPI {}x{})",
                gpu_surf.adapter_name(),
                gpu_surf.backend_name(),
                self.scale_factor,
                self.logical_w,
                self.logical_h,
                self.phys_w,
                self.phys_h
            );
            self.gpu_surface = Some(gpu_surf);
            // 释放首屏临时软件帧缓冲内存
            self.driver.resize(1, 1);
            self.wl_surface = WlSurface::new();
        } else {
            // 若目标表面不支持所选适配器则静默恢复软件表面，零中断
            self.attach_softbuffer_surface(&win);
            self.force_present_once = true;
            win.request_redraw();
        }
    }

    /// 根据当前场景背景亮度同步操作系统窗口原生标题栏明暗主题（Windows 11 / macOS）。
    fn sync_window_theme(&mut self, bg: Color) {
        let dark = is_color_dark(bg);
        if self.last_window_theme_dark != Some(dark) {
            self.last_window_theme_dark = Some(dark);
            if let Some(win) = &self.window {
                win.set_theme(Some(if dark {
                    WinitTheme::Dark
                } else {
                    WinitTheme::Light
                }));
            }
        }
    }

    /// 渲染并提交当前帧至 GPU Swapchain 或 `softbuffer`（被首帧隐身预渲染与 `RedrawRequested` 共用）。
    fn render_and_present_frame(
        &mut self,
        dt_ms: u16,
        #[allow(unused_variables)] wait_gpu_sync: bool,
        force_present: bool,
    ) -> bool {
        #[cfg(feature = "gpu")]
        if let Some(gpu_surf) = &mut self.gpu_surface {
            let curr_damage: Vec<Region> = self.core.step_frame_dt(dt_ms.max(1)).to_vec();
            if curr_damage.is_empty() && !force_present && self.window_shown {
                self.sync_ime_cursor_area();
                self.sync_cursor_shape();
                return false;
            }
            let viewport = self.core.viewport_region();
            let ops = self.core.cached_draw_ops();
            let sf = self.core.scale_factor();
            let scene_bg = infer_scene_bg(ops, self.core.theme().background);
            let presented = gpu_surf.render_scaled_with_clear(ops, viewport, sf, scene_bg);
            if presented {
                if wait_gpu_sync {
                    gpu_surf.flush_and_wait();
                }
                self.wl_surface.committed_damage = curr_damage;
                self.wl_surface.commit();
            }
            self.sync_window_theme(scene_bg);
            self.sync_ime_cursor_area();
            self.sync_cursor_shape();
            return presented;
        }

        if let Some(old_id) = self.wl_surface.attached_buffer_id {
            self.driver.release_buffer(old_id);
        }
        let updated =
            self.driver
                .flush_frame_dt(&mut self.core, &mut self.wl_surface, dt_ms.max(1));
        let scene_bg = infer_scene_bg(self.core.cached_draw_ops(), self.core.theme().background);
        self.sync_window_theme(scene_bg);
        self.sync_ime_cursor_area();
        self.sync_cursor_shape();

        if let Some(surf) = &mut self.surface {
            if let Ok(mut buffer) = surf.buffer_mut() {
                let dst_stride = buffer.width().get() as usize;
                let dst_h = buffer.height().get() as usize;
                let src_stride = self.driver.width();
                let src_h = self.driver.height();
                let age = buffer.age();
                if (!updated
                    && !force_present
                    && age > 0
                    && src_stride == dst_stride
                    && src_h == dst_h)
                    || self.driver.front_pixels().is_empty()
                {
                    return false;
                }
                let src_u32: &[u32] = unsafe {
                    core::slice::from_raw_parts(
                        self.driver.front_pixels().as_ptr() as *const u32,
                        self.driver.front_pixels().len(),
                    )
                };

                let damage_rects: Vec<softbuffer::Rect> = self
                    .wl_surface
                    .committed_damage
                    .iter()
                    .filter_map(|r| {
                        let sf = self.scale_factor;
                        let x = (((r.x.max(0) as f32) * sf).floor() as u32).min(dst_stride as u32);
                        let y = (((r.y.max(0) as f32) * sf).floor() as u32).min(dst_h as u32);
                        let x1 =
                            (((r.right().max(0) as f32) * sf).ceil() as u32).min(dst_stride as u32);
                        let y1 =
                            (((r.bottom().max(0) as f32) * sf).ceil() as u32).min(dst_h as u32);
                        let w = NonZeroU32::new(x1.saturating_sub(x))?;
                        let h = NonZeroU32::new(y1.saturating_sub(y))?;
                        Some(softbuffer::Rect {
                            x,
                            y,
                            width: w,
                            height: h,
                        })
                    })
                    .collect();

                let copied_partial = if !force_present
                    && age == 1
                    && !damage_rects.is_empty()
                    && src_stride == dst_stride
                    && src_h == dst_h
                {
                    for rect in &damage_rects {
                        let rx = rect.x as usize;
                        let ry = rect.y as usize;
                        let rw = rect.width.get() as usize;
                        let rh = rect.height.get() as usize;
                        for row in ry..ry.saturating_add(rh) {
                            let start = row.saturating_mul(dst_stride).saturating_add(rx);
                            let end = start.saturating_add(rw);
                            if end <= buffer.len() && end <= src_u32.len() {
                                buffer[start..end].copy_from_slice(&src_u32[start..end]);
                            }
                        }
                    }
                    true
                } else {
                    if src_stride == dst_stride {
                        let len = buffer.len().min(src_u32.len());
                        buffer[..len].copy_from_slice(&src_u32[..len]);
                    } else {
                        let copy_w = src_stride.min(dst_stride);
                        let copy_h = src_h.min(dst_h);
                        for row in 0..copy_h {
                            let s0 = row.saturating_mul(src_stride);
                            let d0 = row.saturating_mul(dst_stride);
                            let s1 = s0.saturating_add(copy_w).min(src_u32.len());
                            let d1 = d0.saturating_add(copy_w).min(buffer.len());
                            let n = s1.saturating_sub(s0).min(d1.saturating_sub(d0));
                            if n > 0 {
                                buffer[d0..d0 + n].copy_from_slice(&src_u32[s0..s0 + n]);
                            }
                        }
                    }
                    false
                };

                if copied_partial {
                    let _ = buffer.present_with_damage(&damage_rects);
                } else {
                    let _ = buffer.present();
                }
                return true;
            }
        }
        false
    }

    /// 根据当前指针下的组件交互动作实时同步系统鼠标光标形状（仅在光标形态变更时调用 OS API）。
    fn sync_cursor_shape(&mut self) {
        let next = self.core.cursor_shape();
        if next != self.last_cursor {
            self.last_cursor = next;
            if let Some(win) = &self.window {
                win.set_cursor(to_winit_cursor(next));
            }
        }
    }

    /// 将当前文本光标（优先 `.ime_caret()` 精确坐标，回退焦点框）同步给操作系统输入法（IME），使候选窗紧贴光标实时跟随。
    fn sync_ime_cursor_area(&mut self) {
        let Some(win) = &self.window else {
            return;
        };
        let area = self.core.ime_cursor_bounds();
        if area != self.last_ime_area {
            self.last_ime_area = area;
            if let Some(r) = area {
                win.set_ime_cursor_area(
                    LogicalPosition::new(r.x.max(0) as f64, r.y.max(0) as f64),
                    LogicalSize::new(
                        r.w.max(2) as f64,
                        r.h.saturating_add(4).max(16) as f64,
                    ),
                );
            }
        }
    }

    /// 统一同步物理像素尺寸、分数 DPI 缩放因子（原生支持 125% / 150% / 175% / 200%）、逻辑视口与渲染表面尺寸。
    ///
    /// 使用向上取整 `(phys / scale_factor).ceil()` 确保边缘像素完整覆盖，同时忽略最小化时的 `0x0` 尺寸事件。
    fn sync_window_dimensions(&mut self, phys_w: u32, phys_h: u32, os_scale_factor: Option<f64>) {
        if phys_w == 0 || phys_h == 0 {
            return;
        }
        let new_scale = if !self.dpi_scale_explicit {
            os_scale_factor
                .map(|sf| (sf as f32).clamp(0.5, 4.0))
                .unwrap_or(self.scale_factor)
        } else {
            self.scale_factor
        };
        let new_phys_w = phys_w as usize;
        let new_phys_h = phys_h as usize;
        let new_logical_w = ((new_phys_w as f32) / new_scale)
            .ceil()
            .clamp(1.0, i16::MAX as f32) as usize;
        let new_logical_h = ((new_phys_h as f32) / new_scale)
            .ceil()
            .clamp(1.0, i16::MAX as f32) as usize;

        if new_phys_w == self.phys_w
            && new_phys_h == self.phys_h
            && (new_scale - self.scale_factor).abs() < 0.001
            && new_logical_w == self.logical_w
            && new_logical_h == self.logical_h
        {
            return;
        }

        self.scale_factor = new_scale;
        self.phys_w = new_phys_w;
        self.phys_h = new_phys_h;
        self.logical_w = new_logical_w;
        self.logical_h = new_logical_h;

        self.core.set_scale_factor(self.scale_factor);
        self.core
            .resize(self.logical_w as i16, self.logical_h as i16);
        let use_sw_driver = {
            #[cfg(feature = "gpu")]
            {
                self.gpu_surface.is_none() && !self.driver.backend().is_gpu()
            }
            #[cfg(not(feature = "gpu"))]
            {
                true
            }
        };
        if use_sw_driver || self.surface.is_some() {
            self.driver.resize(self.phys_w, self.phys_h);
        }
        self.wl_surface = WlSurface::new();

        #[cfg(feature = "gpu")]
        if let Some(gpu_surf) = &mut self.gpu_surface {
            gpu_surf.resize(self.phys_w as u32, self.phys_h as u32);
        }
        if let Some(surf) = &mut self.surface {
            if let (Some(nw), Some(nh)) = (
                NonZeroU32::new(self.phys_w as u32),
                NonZeroU32::new(self.phys_h as u32),
            ) {
                let _ = surf.resize(nw, nh);
            }
        }
    }
}

#[cfg(feature = "winit")]
impl ApplicationHandler<WinitUserEvent> for WinitAppHandler {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: WinitUserEvent) {
        match event {
            #[cfg(feature = "gpu")]
            WinitUserEvent::GpuReady(prepared) => {
                self.gpu_warmup_in_flight = false;
                if let Some(ctx) = prepared {
                    self.pending_gpu = Some(ctx);
                    self.try_promote_to_gpu();
                } else {
                    println!(
                        "[Velox Winit Backend] 未检测到可用硬件 GPU，继续以内置多线程 SDF 软件光栅稳定运行"
                    );
                }
            }
            WinitUserEvent::WakeAsync => {
                let _ = self.core.poll_async_tasks();
                if self.core.needs_redraw() {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let defer_visible = should_defer_window_visible();
        let initial_dark = is_color_dark(self.core.theme().background);
        let mut attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_visible(!defer_visible)
            .with_theme(Some(if initial_dark {
                WinitTheme::Dark
            } else {
                WinitTheme::Light
            }));
        attrs = if self.dpi_scale_explicit {
            attrs.with_inner_size(PhysicalSize::new(
                ((self.logical_w as f32) * self.scale_factor)
                    .round()
                    .max(1.0) as u32,
                ((self.logical_h as f32) * self.scale_factor)
                    .round()
                    .max(1.0) as u32,
            ))
        } else {
            attrs.with_inner_size(LogicalSize::new(
                self.logical_w.max(1) as u32,
                self.logical_h.max(1) as u32,
            ))
        };

        let win = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("[velox::platform::winit] 创建窗口失败 ({e})");
                event_loop.exit();
                return;
            }
        };
        self.last_window_theme_dark = Some(initial_dark);
        win.set_ime_allowed(true);
        win.set_ime_purpose(ImePurpose::Normal);

        let actual_size = win.inner_size();
        let actual_scale = win.scale_factor();
        if actual_size.width > 0 && actual_size.height > 0 {
            self.sync_window_dimensions(actual_size.width, actual_size.height, Some(actual_scale));
        }

        #[allow(unused_mut)]
        let mut gpu_ready = false;
        #[cfg(feature = "gpu")]
        if self.driver.backend() == DesktopRenderBackend::Wgpu {
            if let Some(prepared) = self.pending_gpu.take() {
                if let Some(gpu_surf) = crate::renderers::wgpu::WgpuWindowRenderer::from_prepared(
                    prepared,
                    win.clone(),
                    self.phys_w.max(1) as u32,
                    self.phys_h.max(1) as u32,
                ) {
                    gpu_ready = true;
                    println!(
                        "[Velox Winit + wgpu 2D SDF GPU Backend] 硬件加速窗口已就绪 ({} / {}, scale={:.2}x, 逻辑 {}x{}, 物理 HiDPI {}x{})",
                        gpu_surf.adapter_name(),
                        gpu_surf.backend_name(),
                        self.scale_factor,
                        self.logical_w,
                        self.logical_h,
                        self.phys_w,
                        self.phys_h
                    );
                    self.gpu_surface = Some(gpu_surf);
                }
            } else if !self.gpu_warmup_in_flight {
                if let Some(gpu_surf) = crate::renderers::wgpu::WgpuWindowRenderer::new(
                    win.clone(),
                    self.phys_w.max(1) as u32,
                    self.phys_h.max(1) as u32,
                ) {
                    gpu_ready = true;
                    println!(
                        "[Velox Winit + wgpu 2D SDF GPU Backend] 硬件加速窗口已就绪 ({} / {}, scale={:.2}x, 逻辑 {}x{}, 物理 HiDPI {}x{})",
                        gpu_surf.adapter_name(),
                        gpu_surf.backend_name(),
                        self.scale_factor,
                        self.logical_w,
                        self.logical_h,
                        self.phys_w,
                        self.phys_h
                    );
                    self.gpu_surface = Some(gpu_surf);
                }
            }
        }

        if !gpu_ready {
            self.attach_softbuffer_surface(&win);
            #[cfg(feature = "gpu")]
            let warming_gpu = self.gpu_warmup_in_flight;
            #[cfg(not(feature = "gpu"))]
            let warming_gpu = false;
            if warming_gpu {
                println!(
                    "[Velox Winit Backend] 首屏已就绪 (scale={:.2}x, 逻辑 {}x{}, 物理 HiDPI {}x{}) — GPU 管线后台异步切换中...",
                    self.scale_factor, self.logical_w, self.logical_h, self.phys_w, self.phys_h
                );
            } else {
                println!(
                    "[Velox Winit Backend] 窗口已就绪 (逻辑 {}x{}, 物理 HiDPI {}x{}) — 支持 Wayland/X11/Win32/macOS",
                    self.logical_w, self.logical_h, self.phys_w, self.phys_h
                );
            }
        }

        self.window = Some(win.clone());

        // Zero-Flash First Paint：在隐藏态同步完成第 1 帧完整布局、字形/SVG 上传与 Swapchain/DIB 提交，
        // 确认前台已有真实像素后再瞬间显窗，彻底杜绝首屏加载出现白色空白窗体。
        self.last_tick = Instant::now();
        let t_first = Instant::now();
        let _ = self.render_and_present_frame(16, defer_visible, true);
        if std::env::var_os("VELOX_STARTUP_TRACE").is_some() {
            println!(
                "[VELOX_STARTUP_TRACE] first_frame_render_and_present={:.1}ms",
                t_first.elapsed().as_secs_f64() * 1000.0
            );
        }

        if defer_visible {
            win.set_visible(true);
            win.focus_window();
            self.window_shown = true;
            // 显窗瞬间操作系统会派发首次 WM_PAINT/RedrawRequested，确保无条件热提交一帧防系统背景刷擦白
            self.force_present_once = true;
            win.request_redraw();
        } else {
            self.window_shown = true;
            win.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let inner = self
                    .window
                    .as_ref()
                    .map(|w| w.inner_size())
                    .unwrap_or_default();
                let target_w = if inner.width > 0 {
                    inner.width
                } else {
                    self.phys_w as u32
                };
                let target_h = if inner.height > 0 {
                    inner.height
                } else {
                    self.phys_h as u32
                };
                self.sync_window_dimensions(target_w, target_h, Some(scale_factor));
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                self.sync_window_dimensions(size.width, size.height, None);
                if size.width > 0 && size.height > 0 {
                    #[cfg(feature = "gpu")]
                    if self.pending_gpu.is_some() {
                        self.try_promote_to_gpu();
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let px = ((position.x / (self.scale_factor as f64)).round() as i32)
                    .clamp(0, i16::MAX as i32) as i16;
                let py = ((position.y / (self.scale_factor as f64)).round() as i32)
                    .clamp(0, i16::MAX as i32) as i16;
                if (px, py) != self.cursor_pos {
                    self.cursor_pos = (px, py);
                    self.core.dispatch_pointer_move(px, py);
                    self.sync_cursor_shape();
                    if self.mouse_down || self.core.needs_redraw() {
                        if let Some(win) = &self.window {
                            win.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let changed = self.core.dispatch_pointer_leave();
                self.sync_cursor_shape();
                if changed {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let (px, py) = self.cursor_pos;
                match state {
                    ElementState::Pressed => {
                        self.mouse_down = true;
                        self.core.dispatch_mouse_down(px, py);
                    }
                    ElementState::Released => {
                        self.mouse_down = false;
                        self.core.dispatch_pointer_up(px, py);
                    }
                }
                self.sync_cursor_shape();
                if self.core.needs_redraw() {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                self.sync_ime_cursor_area();
            }
            WindowEvent::Touch(touch) => {
                let tx = ((touch.location.x / (self.scale_factor as f64)).round() as i32)
                    .clamp(0, i16::MAX as i32) as i16;
                let ty = ((touch.location.y / (self.scale_factor as f64)).round() as i32)
                    .clamp(0, i16::MAX as i32) as i16;
                self.cursor_pos = (tx, ty);
                match touch.phase {
                    TouchPhase::Started => {
                        self.mouse_down = true;
                        self.core.dispatch_touch_down(tx, ty);
                    }
                    TouchPhase::Moved => {
                        self.core.dispatch_pointer_move(tx, ty);
                    }
                    TouchPhase::Ended => {
                        self.mouse_down = false;
                        self.core.dispatch_pointer_up(tx, ty);
                    }
                    TouchPhase::Cancelled => {
                        self.mouse_down = false;
                        self.core.dispatch_event(InputEvent::PointerCancel);
                    }
                }
                if self.core.needs_redraw() {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                self.sync_ime_cursor_area();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                if matches!(state, ElementState::Pressed) {
                    let (px, py) = self.cursor_pos;
                    self.core.dispatch_pointer_secondary_down(px, py);
                    if self.core.needs_redraw() {
                        if let Some(win) = &self.window {
                            win.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let sy = match delta {
                    MouseScrollDelta::LineDelta(_x, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y as f32) / 20.0,
                };
                if sy.abs() > 0.01 {
                    let step = if sy > 0.0 { -20 } else { 20 };
                    let (px, py) = self.cursor_pos;
                    let handled = self.core.dispatch_event(InputEvent::Scroll {
                        x: px,
                        y: py,
                        delta_y: step,
                    });
                    if !handled {
                        if let Some(list_id) = self.scroll_list_id {
                            self.core.scroll(list_id, step);
                        }
                    }
                    if self.core.needs_redraw() {
                        if let Some(win) = &self.window {
                            win.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                let state = mods.state();
                let mut km = KeyModifiers::NONE;
                if state.shift_key() {
                    km = km.with(KeyModifiers::SHIFT);
                }
                if state.control_key() {
                    km = km.with(KeyModifiers::CTRL);
                }
                if state.alt_key() {
                    km = km.with(KeyModifiers::ALT);
                }
                if state.super_key() {
                    km = km.with(KeyModifiers::LOGO);
                }
                self.core.dispatch_event(InputEvent::ModifiersChanged(km));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let dispatch = |core: &mut VeloxEngine, kc: KeyCode| {
                    if pressed {
                        core.dispatch_key(kc)
                    } else {
                        core.dispatch_key_up(kc)
                    }
                };
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        if pressed {
                            if !self.core.dispatch_key(KeyCode::Escape) {
                                event_loop.exit();
                                return;
                            }
                        } else {
                            self.core.dispatch_key_up(KeyCode::Escape);
                        }
                    }
                    Key::Named(NamedKey::Tab) => {
                        let kc = if self.core.modifiers().shift() {
                            KeyCode::BackTab
                        } else {
                            KeyCode::Tab
                        };
                        dispatch(&mut self.core, kc);
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        dispatch(&mut self.core, KeyCode::Down);
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        dispatch(&mut self.core, KeyCode::Up);
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        dispatch(&mut self.core, KeyCode::Left);
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        dispatch(&mut self.core, KeyCode::Right);
                    }
                    Key::Named(NamedKey::Backspace) => {
                        dispatch(&mut self.core, KeyCode::Backspace);
                    }
                    Key::Named(NamedKey::Delete) => {
                        dispatch(&mut self.core, KeyCode::Delete);
                    }
                    Key::Named(NamedKey::Home) => {
                        dispatch(&mut self.core, KeyCode::Home);
                    }
                    Key::Named(NamedKey::End) => {
                        dispatch(&mut self.core, KeyCode::End);
                    }
                    Key::Named(NamedKey::PageUp) => {
                        dispatch(&mut self.core, KeyCode::PageUp);
                    }
                    Key::Named(NamedKey::PageDown) => {
                        dispatch(&mut self.core, KeyCode::PageDown);
                    }
                    Key::Named(NamedKey::Enter) => {
                        dispatch(&mut self.core, KeyCode::Enter);
                    }
                    Key::Named(NamedKey::Space) => {
                        dispatch(&mut self.core, KeyCode::Space);
                    }
                    Key::Character(s) => {
                        let ctrl = self.core.modifiers().ctrl();
                        for ch in s.chars() {
                            // Windows 下 Ctrl+A..Z 可能产生 ASCII 控制码 0x01..=0x1A，统一还原为对应小写字母配合 modifiers().ctrl() 处理快捷键
                            let norm = if ctrl && ('\x01'..='\x1a').contains(&ch) {
                                ((ch as u8) - 1 + b'a') as char
                            } else {
                                ch
                            };
                            dispatch(&mut self.core, KeyCode::Char(norm));
                        }
                    }
                    _ => {}
                }
                if self.core.needs_redraw() {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                self.sync_ime_cursor_area();
            }
            WindowEvent::Ime(ime) => {
                let changed = match ime {
                    Ime::Enabled => {
                        self.last_ime_area = None;
                        self.core.dispatch_event(InputEvent::ImeEnabled)
                    }
                    Ime::Disabled => self.core.dispatch_event(InputEvent::ImeDisabled),
                    Ime::Preedit(text, cursor) => self.core.dispatch_ime_preedit(&text, cursor),
                    Ime::Commit(text) => self.core.dispatch_ime_commit(&text),
                };
                self.sync_ime_cursor_area();
                if changed || self.core.needs_redraw() {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::ThemeChanged(theme) => {
                let dark = matches!(theme, WinitTheme::Dark);
                if self.core.dispatch_event(InputEvent::SystemThemeChanged { dark }) {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::HoveredFile(_) => {
                let (x, y) = self.cursor_pos;
                if self.core.dispatch_event(InputEvent::FileHovered { x, y }) {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::HoveredFileCancelled => {
                if self.core.dispatch_event(InputEvent::FileHoverCancelled) {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::DroppedFile(path) => {
                let (x, y) = self.cursor_pos;
                self.core.push_dropped_file(path, x, y);
                let _ = self.core.dispatch_event(InputEvent::FileDropped { x, y });
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let (x, y) = self.cursor_pos;
                let d = (delta * 1000.0).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
                if self.core.dispatch_event(InputEvent::Pinch {
                    x,
                    y,
                    delta_scale_x1000: d,
                }) {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::RotationGesture { delta, .. } => {
                let (x, y) = self.cursor_pos;
                let d = (delta * 10.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                if self.core.dispatch_event(InputEvent::Rotate {
                    x,
                    y,
                    delta_deg10: d,
                }) {
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(inner) = self.window.as_ref().map(|w| w.inner_size()) {
                    if inner.width == 0 || inner.height == 0 {
                        return;
                    }
                    if inner.width as usize != self.phys_w || inner.height as usize != self.phys_h {
                        self.sync_window_dimensions(inner.width, inner.height, None);
                    }
                }

                let now = Instant::now();
                let raw_dt = now.duration_since(self.last_tick).as_millis();
                // 空闲休眠后唤醒首帧（>50ms）钳制为标称单帧步长，活跃帧钳制在 1..=33ms，
                // 避免休眠醒来瞬间给 120ms 按钮悬停过渡灌入 100ms 巨额步长引发跳帧闪烁。
                let nominal_dt: u16 = if self.driver.backend().is_gpu() { 8 } else { 16 };
                let dt_ms: u16 = if raw_dt > 50 {
                    nominal_dt
                } else {
                    (raw_dt as u16).clamp(1, 33)
                };
                self.last_tick = now;

                let force = self.force_present_once;
                self.force_present_once = false;
                let _ = self.render_and_present_frame(dt_ms, false, force);

                if !self.window_shown {
                    if let Some(win) = &self.window {
                        win.set_visible(true);
                        self.window_shown = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let minimized = self
            .window
            .as_ref()
            .map(|w| {
                let sz = w.inner_size();
                sz.width == 0 || sz.height == 0 || w.is_minimized() == Some(true)
            })
            .unwrap_or(false);
        if minimized || !self.core.needs_redraw() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }

        // 按目标刷新率节拍休眠（GPU 120Hz = 8ms / 软件 60Hz = 16ms）：
        // 仅在到达下一帧时刻时触发 `request_redraw()`，避免在 `WaitUntil` 期间立即重绘导致 0ms 忙等。
        let frame_interval = Duration::from_millis(if self.driver.backend().is_gpu() {
            8
        } else {
            16
        });
        let now = Instant::now();
        let next_frame = self.last_tick + frame_interval;
        if now >= next_frame {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + frame_interval));
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame));
        }
    }
}
