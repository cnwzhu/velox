//! 确定性无头自动化测试平台 `TestingPlatform`。
//!
//! 支持 `#![no_std]` 环境，提供泛型像素格式 `P: PixelFormat`（默认 [`Rgb565`]，亦可指定 [`Rgba8888`] 或 [`crate::display::Xrgb8888`]）、
//! 虚拟单调时钟步进、输入事件队列注入、异形屏安全区与离屏像素帧缓冲断言。

use core::time::Duration;

use crate::arena::SmallVec;
use crate::display::{PixelFormat, Rgb565, Rgba8888};
use crate::engine::VeloxEngine;
use crate::platform::mobile::{CDirtyRect, DeviceOrientation, TouchPhase};
use crate::platform::{FlushReport, Platform, PlatformControlFlow, PlatformSurface};
use crate::pod::{InputEvent, KeyCode, SafeAreaInsets};
use crate::solver::Region;

/// 确定性无头测试平台（支持泛型像素格式 `P`，默认 `320x240` `Rgb565`）。
///
/// 同时作为 `#![no_std]` 固定分辨率移动端（[`MobilePlatform`](crate::platform::mobile::MobilePlatform)）与
/// WebAssembly `<canvas>`（[`WebPlatform`](crate::platform::web::WebPlatform)）的统一底层实现。
pub struct TestingPlatform<
    const W: usize = 320,
    const H: usize = 240,
    P: PixelFormat = Rgb565,
> {
    pub framebuffer: [[P; W]; H],
    pending_events: SmallVec<InputEvent, 32>,
    last_dirty: SmallVec<Region, 8>,
    last_dirty_rects: SmallVec<CDirtyRect, 8>,
    primary_touch_id: Option<u32>,
    orientation: DeviceOrientation,
    safe_area: SafeAreaInsets,
    safe_area_dirty: bool,
    elapsed_ms: u64,
    pub total_frames_rendered: u32,
}

impl<const W: usize, const H: usize, P: PixelFormat> Default for TestingPlatform<W, H, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const W: usize, const H: usize, P: PixelFormat> TestingPlatform<W, H, P> {
    pub const fn new() -> Self {
        let is_portrait = H > W;
        Self {
            framebuffer: [[P::BLACK; W]; H],
            pending_events: SmallVec::new(),
            last_dirty: SmallVec::new(),
            last_dirty_rects: SmallVec::new(),
            primary_touch_id: None,
            orientation: DeviceOrientation::Portrait,
            safe_area: if is_portrait {
                SafeAreaInsets::phone_portrait()
            } else {
                SafeAreaInsets::new(0, 0, 0, 0)
            },
            safe_area_dirty: is_portrait,
            elapsed_ms: 0,
            total_frames_rendered: 0,
        }
    }

    /// 注入一条模拟输入事件。
    pub fn send_event(&mut self, event: InputEvent) {
        self.pending_events.push(event);
    }

    /// 模拟在 `(x, y)` 处按下并抬起完成一次点击。
    pub fn send_click(&mut self, x: i16, y: i16) {
        self.pending_events.push(InputEvent::PointerDown { x, y });
        self.pending_events.push(InputEvent::PointerUp { x, y });
    }

    /// 注入浏览器/桌面 `pointerdown` 事件。
    pub fn on_pointer_down(&mut self, x: i16, y: i16) {
        self.pending_events.push(InputEvent::PointerDown { x, y });
    }

    /// 注入浏览器/桌面 `pointermove` 事件（自动合并连续移动事件）。
    pub fn on_pointer_move(&mut self, x: i16, y: i16) {
        if let Some(InputEvent::PointerMove {
            x: last_x,
            y: last_y,
        }) = self.pending_events.as_mut_slice().last_mut()
        {
            *last_x = x;
            *last_y = y;
        } else {
            self.pending_events.push(InputEvent::PointerMove { x, y });
        }
    }

    /// 注入浏览器/桌面 `pointerup` 事件。
    pub fn on_pointer_up(&mut self, x: i16, y: i16) {
        self.pending_events.push(InputEvent::PointerUp { x, y });
    }

    /// 注入浏览器/桌面 `pointercancel` 事件。
    pub fn on_pointer_cancel(&mut self) {
        self.pending_events.push(InputEvent::PointerCancel);
    }

    /// 注入滚轮滚动事件。
    pub fn on_wheel(&mut self, x: i16, y: i16, delta_y: i16) {
        self.pending_events
            .push(InputEvent::Scroll { x, y, delta_y });
    }

    /// 注入键盘按下事件。
    pub fn on_key_down(&mut self, key: KeyCode) {
        self.pending_events.push(InputEvent::KeyDown(key));
    }

    /// 注入浏览器 `popstate` 后退事件。
    pub fn on_popstate_back(&mut self) {
        self.pending_events.push(InputEvent::NavigateBack);
    }

    /// 注入移动端单指/多指触控事件（自动锁定主触控点 `primary_touch_id` 并合并高频 `Moved` 事件）。
    pub fn on_touch(&mut self, touch_id: u32, phase: TouchPhase, x: i16, y: i16) {
        match phase {
            TouchPhase::Began => {
                if self.primary_touch_id.is_none() {
                    self.primary_touch_id = Some(touch_id);
                    self.pending_events.push(InputEvent::PointerDown { x, y });
                }
            }
            TouchPhase::Moved => {
                if self.primary_touch_id == Some(touch_id) {
                    self.on_pointer_move(x, y);
                }
            }
            TouchPhase::Ended => {
                if self.primary_touch_id == Some(touch_id) {
                    self.primary_touch_id = None;
                    self.pending_events.push(InputEvent::PointerUp { x, y });
                }
            }
            TouchPhase::Cancelled => {
                if self.primary_touch_id == Some(touch_id) {
                    self.primary_touch_id = None;
                    self.pending_events.push(InputEvent::PointerCancel);
                }
            }
        }
    }

    /// 切换移动设备屏幕朝向并同步更新默认异形屏安全区域。
    pub fn set_orientation(&mut self, orientation: DeviceOrientation) {
        if self.orientation != orientation {
            self.orientation = orientation;
            self.safe_area = match orientation {
                DeviceOrientation::Portrait => SafeAreaInsets::phone_portrait(),
                DeviceOrientation::LandscapeLeft => SafeAreaInsets::new(0, 0, 21, 44),
                DeviceOrientation::LandscapeRight => SafeAreaInsets::new(0, 44, 21, 0),
                DeviceOrientation::PortraitUpsideDown => SafeAreaInsets::new(34, 0, 44, 0),
            };
            self.safe_area_dirty = true;
        }
    }

    /// 更新异形屏安全区域内边距。
    pub fn set_safe_area(&mut self, insets: SafeAreaInsets) {
        self.safe_area = insets;
        self.safe_area_dirty = false;
        self.pending_events.push(InputEvent::SafeAreaChanged(insets));
    }

    /// 触发系统返回导航（Android 返回键 / 边缘右滑返回），返回是否被当前页面消费。
    pub fn handle_back(&mut self, engine: &mut VeloxEngine) -> bool {
        engine.dispatch_navigate_back()
    }

    /// 获取最近一帧合并计算的脏矩形切片（[`Region`] 格式）。
    #[inline(always)]
    pub fn last_dirty_regions(&self) -> &[Region] {
        self.last_dirty.as_slice()
    }

    /// 获取最近一帧合并计算的脏矩形切片（FFI 兼容 [`CDirtyRect`] 格式）。
    #[inline(always)]
    pub fn dirty_rects(&self) -> &[CDirtyRect] {
        self.last_dirty_rects.as_slice()
    }

    /// 获取展平的只读像素帧缓冲切片。
    pub fn pixels(&self) -> &[P] {
        unsafe { core::slice::from_raw_parts(self.framebuffer.as_ptr() as *const P, W * H) }
    }
}

impl<const W: usize, const H: usize> TestingPlatform<W, H, Rgba8888> {
    /// 获取供 FFI / WebAssembly `Uint8ClampedArray` 直接引用的 `RGBA8888` 像素首地址。
    #[inline(always)]
    pub fn rgba_ptr(&self) -> *const u8 {
        self.framebuffer.as_ptr() as *const u8
    }

    /// 获取 `RGBA8888` 帧缓冲区的总字节长度（`W * H * 4`）。
    #[inline(always)]
    pub const fn rgba_byte_len(&self) -> usize {
        W * H * 4
    }
}

impl<const W: usize, const H: usize, P: PixelFormat> PlatformSurface
    for TestingPlatform<W, H, P>
{
    #[inline(always)]
    fn surface_size(&self) -> (i16, i16) {
        (W as i16, H as i16)
    }

    fn flush_frame_dt(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> FlushReport {
        self.last_dirty.clear();
        self.last_dirty_rects.clear();
        for &r in engine.step_frame_dt(dt_ms) {
            self.last_dirty.push(r);
            self.last_dirty_rects.push(CDirtyRect {
                x: r.x,
                y: r.y,
                w: r.w,
                h: r.h,
            });
        }
        if self.last_dirty.is_empty() {
            return FlushReport::default();
        }
        let fb_slice = unsafe {
            core::slice::from_raw_parts_mut(self.framebuffer.as_mut_ptr() as *mut P, W * H)
        };
        let mut px_count = 0usize;
        for &reg in self.last_dirty.as_slice() {
            engine.rasterize_region(reg, fb_slice);
            px_count += (reg.w.max(0) as usize) * (reg.h.max(0) as usize);
        }
        self.total_frames_rendered += 1;
        FlushReport::with_engine(self.last_dirty.len(), 1, px_count, engine)
    }
}

impl<const W: usize, const H: usize, P: PixelFormat> Platform for TestingPlatform<W, H, P> {
    #[inline(always)]
    fn duration_since_start(&self) -> Duration {
        Duration::from_millis(self.elapsed_ms)
    }

    fn step_platform(&mut self, engine: &mut VeloxEngine, dt_ms: u16) -> PlatformControlFlow {
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms as u64);
        if self.safe_area_dirty {
            self.safe_area_dirty = false;
            engine.set_safe_area(self.safe_area);
        }
        let mut events: SmallVec<InputEvent, 32> = SmallVec::new();
        for ev in self.pending_events.as_slice() {
            events.push(*ev);
        }
        self.pending_events.clear();
        for ev in events.as_slice() {
            engine.dispatch_event(*ev);
        }
        let report = self.flush_frame_dt(engine, dt_ms);
        if report.has_updates() || engine.needs_redraw() {
            PlatformControlFlow::Poll
        } else {
            PlatformControlFlow::Wait
        }
    }
}
