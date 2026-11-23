//! 零堆分配页面路由栈与过渡转场管理器 `Navigator`。

use velox_core::display::PixelFormat;
use velox_core::renderers::software::blit_translated_layer;
use velox_core::solver::Region;

/// 页面切换过渡动画类型。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum PageTransition {
    /// 无过渡，立即切换。
    None = 0,
    /// 新页面从右侧滑入（向左推进）。
    SlideLeft = 1,
    /// 新页面从左侧滑入（向右返回）。
    SlideRight = 2,
    /// 新页面从底部向上滑入。
    SlideUp = 3,
    /// 渐隐渐显过渡。
    Fade = 4,
}

/// 单帧转场合成中的单个页面图层变换参数（供离屏缓冲平移/透明度合成使用）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TransitionLayer<S> {
    /// 页面标识。
    pub screen: S,
    /// 水平像素偏移量。
    pub offset_x: i16,
    /// 垂直像素偏移量。
    pub offset_y: i16,
    /// 整体不透明度（`0..=255`）。
    pub alpha: u8,
}

/// 固定容量的 `#![no_std]` 页面路由导航栈（默认最大深度 `DEPTH = 8`）。
#[derive(Clone, Debug)]
pub struct Navigator<S: Copy, const DEPTH: usize = 8> {
    stack: [S; DEPTH],
    len: usize,
    prev_screen: Option<S>,
    transition: PageTransition,
    duration_ms: u16,
    elapsed_ms: u16,
}

impl<S: Copy, const DEPTH: usize> Navigator<S, DEPTH> {
    /// 以初始根页面 `root_screen` 创建页面路由栈。
    #[inline(always)]
    pub const fn new(root_screen: S) -> Self {
        Self {
            stack: [root_screen; DEPTH],
            len: 1,
            prev_screen: None,
            transition: PageTransition::None,
            duration_ms: 200,
            elapsed_ms: 200,
        }
    }

    /// 返回当前栈顶激活页面。
    #[inline(always)]
    pub const fn current(&self) -> S {
        self.stack[self.len - 1]
    }

    /// 返回当前转场过程中正在退出的上一页面（转场结束后为 `None`）。
    #[inline(always)]
    pub const fn previous(&self) -> Option<S> {
        self.prev_screen
    }

    /// 当前是否正处于页面切换转场动画中。
    #[inline(always)]
    pub const fn is_transitioning(&self) -> bool {
        self.elapsed_ms < self.duration_ms && !matches!(self.transition, PageTransition::None)
    }

    /// 返回当前路由栈深度（`>= 1`）。
    #[inline(always)]
    pub const fn depth(&self) -> usize {
        self.len
    }

    /// 是否可以返回上一页（`depth() > 1`）。
    #[inline(always)]
    pub const fn can_pop(&self) -> bool {
        self.len > 1
    }

    /// 返回当前正在执行的页面转场动画类型。
    #[inline(always)]
    pub const fn active_transition(&self) -> PageTransition {
        self.transition
    }

    /// 推入新页面并触发指定转场动画。若栈已满则返回 `false`。
    pub fn push(&mut self, screen: S, transition: PageTransition, duration_ms: u16) -> bool {
        if self.len >= DEPTH {
            return false;
        }
        let prev = self.current();
        self.stack[self.len] = screen;
        self.len += 1;
        self.transition = transition;
        self.duration_ms = duration_ms.max(1);
        if matches!(transition, PageTransition::None) {
            self.elapsed_ms = self.duration_ms;
            self.prev_screen = None;
        } else {
            self.elapsed_ms = 0;
            self.prev_screen = Some(prev);
        }
        true
    }

    /// 弹出当前栈顶页面并触发返回转场动画（默认 [`PageTransition::SlideRight`]）。
    pub fn pop(&mut self, duration_ms: u16) -> Option<S> {
        if self.len <= 1 {
            return None;
        }
        let popped = self.stack[self.len - 1];
        self.len -= 1;
        self.prev_screen = Some(popped);
        self.transition = PageTransition::SlideRight;
        self.duration_ms = duration_ms.max(1);
        self.elapsed_ms = 0;
        Some(popped)
    }

    /// 替换当前栈顶页面。
    pub fn replace(&mut self, screen: S, transition: PageTransition, duration_ms: u16) {
        let prev = self.current();
        self.stack[self.len - 1] = screen;
        self.transition = transition;
        self.duration_ms = duration_ms.max(1);
        if matches!(transition, PageTransition::None) {
            self.elapsed_ms = self.duration_ms;
            self.prev_screen = None;
        } else {
            self.elapsed_ms = 0;
            self.prev_screen = Some(prev);
        }
    }

    /// 推进转场动画时钟 `dt_ms`，返回当前页面的 `(offset_x, offset_y, alpha_255)`。
    pub fn step_transition(&mut self, dt_ms: u16, viewport_w: i16, viewport_h: i16) -> (i16, i16, u8) {
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms).min(self.duration_ms);
        if self.elapsed_ms >= self.duration_ms || matches!(self.transition, PageTransition::None) {
            self.prev_screen = None;
            return (0, 0, 255);
        }
        // 使用二次缓出 (EaseOutQuad) 计算归一化剩余比例 rem_256 (256 -> 0)
        let t_256 = ((self.elapsed_ms as u32) * 256) / (self.duration_ms as u32);
        let inv = 256u32.saturating_sub(t_256);
        let rem_256 = ((inv * inv) >> 8) as i32;

        match self.transition {
            PageTransition::None => (0, 0, 255),
            PageTransition::SlideLeft => ((((viewport_w as i32) * rem_256) >> 8) as i16, 0, 255),
            PageTransition::SlideRight => ((-(((viewport_w as i32) * rem_256) >> 8)) as i16, 0, 255),
            PageTransition::SlideUp => (0, (((viewport_h as i32) * rem_256) >> 8) as i16, 255),
            PageTransition::Fade => (0, 0, (255u32.saturating_sub((rem_256 as u32 * 255) >> 8)) as u8),
        }
    }

    /// 推进双页面转场动画时钟 `dt_ms`，同时返回 `(退出旧页面图层, 进入新页面图层)` 的合成变换参数。
    pub fn step_dual_transition(
        &mut self,
        dt_ms: u16,
        viewport_w: i16,
        viewport_h: i16,
    ) -> (Option<TransitionLayer<S>>, TransitionLayer<S>) {
        let prev = self.prev_screen;
        let (ox, oy, alpha) = self.step_transition(dt_ms, viewport_w, viewport_h);
        let curr_layer = TransitionLayer {
            screen: self.current(),
            offset_x: ox,
            offset_y: oy,
            alpha,
        };
        if !self.is_transitioning() {
            return (None, curr_layer);
        }
        let prev_layer = prev.map(|screen| match self.transition {
            PageTransition::SlideLeft => TransitionLayer {
                screen,
                offset_x: ox.saturating_sub(viewport_w),
                offset_y: 0,
                alpha: 255,
            },
            PageTransition::SlideRight => TransitionLayer {
                screen,
                offset_x: viewport_w.saturating_add(ox),
                offset_y: 0,
                alpha: 255,
            },
            PageTransition::SlideUp | PageTransition::Fade | PageTransition::None => {
                TransitionLayer {
                    screen,
                    offset_x: 0,
                    offset_y: 0,
                    alpha: 255,
                }
            }
        });
        (prev_layer, curr_layer)
    }

    /// 直接利用已预渲染的旧页面离屏缓冲 `prev_buf` 与新页面离屏缓冲 `curr_buf` 执行转场平移/Alpha 合成，
    /// 避免在转场动画每一帧重新光栅化整屏组件树。返回当前转场是否仍在继续。
    pub fn composite_transition_buffers<P: PixelFormat>(
        &mut self,
        dt_ms: u16,
        viewport_w: i16,
        viewport_h: i16,
        prev_buf: Option<&[P]>,
        curr_buf: &[P],
        dst: &mut [P],
    ) -> bool {
        let (prev_layer, curr_layer) = self.step_dual_transition(dt_ms, viewport_w, viewport_h);
        let clip = Region::new(0, 0, viewport_w, viewport_h);
        let stride = viewport_w.max(0) as usize;
        if let (Some(pl), Some(pbuf)) = (prev_layer, prev_buf) {
            blit_translated_layer(
                pbuf,
                viewport_w,
                viewport_h,
                pl.offset_x,
                pl.offset_y,
                pl.alpha,
                clip,
                0,
                0,
                stride,
                dst,
            );
        }
        blit_translated_layer(
            curr_buf,
            viewport_w,
            viewport_h,
            curr_layer.offset_x,
            curr_layer.offset_y,
            curr_layer.alpha,
            clip,
            0,
            0,
            stride,
            dst,
        );
        self.is_transitioning()
    }
}
