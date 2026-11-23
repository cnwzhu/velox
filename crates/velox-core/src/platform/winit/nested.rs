//! 面向合成器嵌套调试后端的自定义 `Xrgb8888` 帧缓冲窗口宿主运行器。

#[cfg(feature = "winit")]
use core::num::NonZeroU32;
#[cfg(feature = "winit")]
use std::rc::Rc;
#[cfg(feature = "winit")]
use std::string::String;
#[cfg(feature = "winit")]
use std::time::{Duration, Instant};

#[cfg(feature = "winit")]
use winit::application::ApplicationHandler;
#[cfg(feature = "winit")]
use winit::dpi::LogicalSize;
#[cfg(feature = "winit")]
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
#[cfg(feature = "winit")]
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
#[cfg(feature = "winit")]
use winit::keyboard::{Key, NamedKey};
#[cfg(feature = "winit")]
use winit::window::{ImePurpose, Window, WindowId};

use crate::prelude::*;

/// 面向合成器嵌套调试后端的自定义 `Xrgb8888` 帧缓冲窗口宿主运行器。
///
/// 使上层合成器（如 `velox-wm`）无需直接依赖 `winit` 或 `softbuffer` crate，即可在嵌套窗口模式下驱动合成帧与输入派发。
pub fn run_nested_compositor_window<S, FInput, FStep>(
    title: &str,
    width: u32,
    height: u32,
    state: S,
    on_input: FInput,
    on_step_frame: FStep,
) where
    S: 'static,
    FInput: FnMut(&mut S, InputEvent) + 'static,
    FStep: FnMut(&mut S, u16) -> (&[Xrgb8888], bool) + 'static,
{
    #[cfg(not(feature = "winit"))]
    {
        let mut state = state;
        let mut on_input = on_input;
        let mut on_step_frame = on_step_frame;
        let _ = (title, width, height, &mut on_input);
        let _ = on_step_frame(&mut state, 16);
    }
    #[cfg(feature = "winit")]
    {
    struct NestedHost<S, FInput, FStep> {
        title: String,
        w: u32,
        h: u32,
        state: S,
        on_input: FInput,
        on_step_frame: FStep,
        window: Option<Rc<Window>>,
        surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
        cursor_pos: (i16, i16),
        mouse_down: bool,
        last_tick: Instant,
        needs_redraw: bool,
    }

    impl<S, FInput, FStep> NestedHost<S, FInput, FStep>
    where
        FInput: FnMut(&mut S, InputEvent),
        FStep: FnMut(&mut S, u16) -> (&[Xrgb8888], bool),
    {
        fn present_frame(&mut self, dt_ms: u16) {
            let (fb, keep_polling) = (self.on_step_frame)(&mut self.state, dt_ms.max(1));
            self.needs_redraw = keep_polling;
            let lw = self.w.max(1) as usize;
            let lh = self.h.max(1) as usize;
            if let Some(surf) = &mut self.surface {
                if let Ok(mut out_buf) = surf.buffer_mut() {
                    let pw = out_buf.width().get() as usize;
                    let ph = out_buf.height().get() as usize;
                    let raw_u32: &[u32] = unsafe {
                        core::slice::from_raw_parts(fb.as_ptr() as *const u32, fb.len())
                    };
                    if pw == lw && ph == lh && out_buf.len() >= raw_u32.len() {
                        let len = out_buf.len().min(raw_u32.len());
                        out_buf[..len].copy_from_slice(&raw_u32[..len]);
                    } else {
                        for dy in 0..ph {
                            let sy = (dy * lh / ph).min(lh.saturating_sub(1));
                            let dst_row = dy * pw;
                            let src_row = sy * lw;
                            for dx in 0..pw {
                                let sx = (dx * lw / pw).min(lw.saturating_sub(1));
                                if dst_row + dx < out_buf.len() && src_row + sx < raw_u32.len() {
                                    out_buf[dst_row + dx] = raw_u32[src_row + sx];
                                }
                            }
                        }
                    }
                    let _ = out_buf.present();
                }
            }
        }
    }

    impl<S, FInput, FStep> ApplicationHandler for NestedHost<S, FInput, FStep>
    where
        FInput: FnMut(&mut S, InputEvent),
        FStep: FnMut(&mut S, u16) -> (&[Xrgb8888], bool),
    {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let defer_visible = cfg!(any(target_os = "windows", target_os = "macos"))
                || std::env::var_os("WAYLAND_DISPLAY").is_none();
            let attrs = Window::default_attributes()
                .with_title(self.title.clone())
                .with_inner_size(LogicalSize::new(self.w as f64, self.h as f64))
                .with_visible(!defer_visible)
                .with_resizable(false);
            if let Ok(win) = event_loop.create_window(attrs) {
                win.set_cursor_visible(true);
                win.set_ime_allowed(true);
                win.set_ime_purpose(ImePurpose::Normal);
                let phys = win.inner_size();
                let pw = phys.width.max(self.w.max(1));
                let ph = phys.height.max(self.h.max(1));
                let win_rc = Rc::new(win);
                if let Ok(ctx) = softbuffer::Context::new(win_rc.clone()) {
                    if let Ok(mut surf) = softbuffer::Surface::new(&ctx, win_rc.clone()) {
                        let _ = surf.resize(
                            NonZeroU32::new(pw).unwrap(),
                            NonZeroU32::new(ph).unwrap(),
                        );
                        self.surface = Some(surf);
                    }
                }
                self.window = Some(win_rc.clone());
                // 隐身态先提交首帧像素再显窗，彻底避免首屏白屏闪现
                self.last_tick = Instant::now();
                self.present_frame(16);
                if defer_visible {
                    win_rc.set_visible(true);
                    win_rc.focus_window();
                }
                win_rc.request_redraw();
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    if let Some(surf) = &mut self.surface {
                        if let (Some(nw), Some(nh)) = (
                            NonZeroU32::new(size.width.max(1)),
                            NonZeroU32::new(size.height.max(1)),
                        ) {
                            let _ = surf.resize(nw, nh);
                        }
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let (pw, ph) = self
                        .window
                        .as_ref()
                        .map(|w| {
                            let s = w.inner_size();
                            (s.width.max(1) as f64, s.height.max(1) as f64)
                        })
                        .unwrap_or((self.w.max(1) as f64, self.h.max(1) as f64));
                    let px = ((position.x * (self.w as f64)) / pw)
                        .round()
                        .clamp(0.0, (self.w.saturating_sub(1)) as f64)
                        as i16;
                    let py = ((position.y * (self.h as f64)) / ph)
                        .round()
                        .clamp(0.0, (self.h.saturating_sub(1)) as f64)
                        as i16;
                    if (px, py) != self.cursor_pos {
                        self.cursor_pos = (px, py);
                        (self.on_input)(&mut self.state, InputEvent::PointerMove { x: px, y: py });
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
                            (self.on_input)(
                                &mut self.state,
                                InputEvent::PointerKindChanged(PointerKind::Mouse),
                            );
                            (self.on_input)(
                                &mut self.state,
                                InputEvent::PointerDown { x: px, y: py },
                            );
                        }
                        ElementState::Released => {
                            self.mouse_down = false;
                            (self.on_input)(
                                &mut self.state,
                                InputEvent::PointerUp { x: px, y: py },
                            );
                        }
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Right,
                    ..
                } => {
                    let (px, py) = self.cursor_pos;
                    let ev = match state {
                        ElementState::Pressed => InputEvent::PointerSecondaryDown { x: px, y: py },
                        ElementState::Released => InputEvent::PointerSecondaryUp { x: px, y: py },
                    };
                    (self.on_input)(&mut self.state, ev);
                    if let Some(win) = &self.window {
                        win.request_redraw();
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
                        (self.on_input)(
                            &mut self.state,
                            InputEvent::Scroll {
                                x: px,
                                y: py,
                                delta_y: step,
                            },
                        );
                        if let Some(win) = &self.window {
                            win.request_redraw();
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
                    (self.on_input)(&mut self.state, InputEvent::ModifiersChanged(km));
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let pressed = event.state == ElementState::Pressed;
                    let wrap = |kc: KeyCode| {
                        if pressed {
                            InputEvent::KeyDown(kc)
                        } else {
                            InputEvent::KeyUp(kc)
                        }
                    };
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Escape));
                        }
                        Key::Named(NamedKey::Tab) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Tab));
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Down));
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Up));
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Left));
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Right));
                        }
                        Key::Named(NamedKey::Enter) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Enter));
                        }
                        Key::Named(NamedKey::Space) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Space));
                        }
                        Key::Named(NamedKey::Backspace) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Backspace));
                        }
                        Key::Named(NamedKey::Delete) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Delete));
                        }
                        Key::Named(NamedKey::Home) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::Home));
                        }
                        Key::Named(NamedKey::End) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::End));
                        }
                        Key::Named(NamedKey::PageUp) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::PageUp));
                        }
                        Key::Named(NamedKey::PageDown) => {
                            (self.on_input)(&mut self.state, wrap(KeyCode::PageDown));
                        }
                        Key::Character(s) => {
                            for ch in s.chars() {
                                (self.on_input)(&mut self.state, wrap(KeyCode::Char(ch)));
                            }
                        }
                        _ => {}
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                WindowEvent::Ime(ime) => {
                    match ime {
                        Ime::Enabled => (self.on_input)(&mut self.state, InputEvent::ImeEnabled),
                        Ime::Disabled => (self.on_input)(&mut self.state, InputEvent::ImeDisabled),
                        Ime::Preedit(text, cursor) => {
                            let preedit = ImePreedit::from_str(&text, cursor);
                            (self.on_input)(&mut self.state, InputEvent::ImePreedit(preedit));
                        }
                        Ime::Commit(text) => {
                            (self.on_input)(
                                &mut self.state,
                                InputEvent::ImePreedit(ImePreedit::empty()),
                            );
                            for ch in text.chars() {
                                (self.on_input)(&mut self.state, InputEvent::TextCommit(ch));
                            }
                        }
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let dt_ms = now.duration_since(self.last_tick).as_millis().min(100) as u16;
                    self.last_tick = now;
                    self.present_frame(dt_ms.max(1));
                }
                _ => {}
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.needs_redraw {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(16),
                ));
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }

    if let Ok(event_loop) = EventLoop::new() {
        event_loop.listen_device_events(winit::event_loop::DeviceEvents::Never);
        let mut host = NestedHost {
            title: String::from(title),
            w: width,
            h: height,
            state,
            on_input,
            on_step_frame,
            window: None,
            surface: None,
            cursor_pos: (0, 0),
            mouse_down: false,
            last_tick: Instant::now(),
            needs_redraw: true,
        };
        let _ = event_loop.run_app(&mut host);
    }
    }
}
