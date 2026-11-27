//! 跨平台多模态相机手势控制器（`ViewportController`）：
//! - 桌面端：光标中心滚轮缩放（Zoom-to-Cursor）、拖拽平移、左键橡皮筋框选（Marquee）；
//! - 触控板/移动端：双指捏合缩放（Pinch-to-Zoom）、松手惯性滑动（Kinetic Pan）与边界阻尼回弹；
//! - 连续缩放字形防抖锁（Zoom Font Lock）：手势停止 `120ms` 后自动解除离散基准字号锁，恢复 0.25px 亚像素精绘并回落 0% CPU 休眠。

use velox_engine::canvas_scene::Viewport2D;

/// 交互拖拽模式（平移相机 / 橡皮筋框选）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CanvasDragMode {
    /// 拖拽平移画布（默认）。
    #[default]
    Pan,
    /// 左键拖拽拉出橡皮筋矩形批量框选。
    Marquee,
}

/// 跨平台交互画布相机控制器。
#[derive(Clone, Debug)]
pub struct ViewportController {
    pub viewport: Viewport2D,
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub drag_mode: CanvasDragMode,
    /// 当前活跃的橡皮筋选框（画布局部坐标 `[start_x, start_y, cur_x, cur_y]`）。
    pub marquee: Option<[f32; 4]>,
    /// 惯性滑动速度（逻辑像素/秒）。
    pub vel_x: f32,
    pub vel_y: f32,
    /// 最近一次缩放手势发生时刻（毫秒），用于 `120ms` Zoom Font Lock 防抖。
    pub last_zoom_ms: u32,
    pub zoom_lock_active: bool,
    /// 可选世界边界约束（用于松手弹性回弹与自动居中适配）。
    pub world_bounds: Option<[f32; 4]>,
}

impl Default for ViewportController {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewportController {
    /// Zoom Font Lock 手势停止后恢复 0.25px 亚像素精绘的防抖窗口（120ms）。
    pub const ZOOM_LOCK_SETTLE_MS: u32 = 120;

    pub const fn new() -> Self {
        Self {
            viewport: Viewport2D::new(0.0, 0.0, 1.0),
            min_zoom: 0.15,
            max_zoom: 12.0,
            drag_mode: CanvasDragMode::Pan,
            marquee: None,
            vel_x: 0.0,
            vel_y: 0.0,
            last_zoom_ms: 0,
            zoom_lock_active: false,
            world_bounds: None,
        }
    }

    /// 一键将世界包围盒居中适配到指定画布尺寸 `(canvas_w, canvas_h)` 内（留白 `pad` 像素）。
    pub fn fit_bounds(&mut self, wb: [f32; 4], canvas_w: f32, canvas_h: f32, pad: f32) {
        self.world_bounds = Some(wb);
        let bw = (wb[2] - wb[0]).max(1.0);
        let bh = (wb[3] - wb[1]).max(1.0);
        let avail_w = (canvas_w - pad * 2.0).max(16.0);
        let avail_h = (canvas_h - pad * 2.0).max(16.0);
        let z = (avail_w / bw).min(avail_h / bh).clamp(self.min_zoom, self.max_zoom);
        let cx = (wb[0] + wb[2]) * 0.5;
        let cy = (wb[1] + wb[3]) * 0.5;
        self.viewport.zoom = z;
        self.viewport.offset_x = canvas_w * 0.5 - cx * z;
        self.viewport.offset_y = canvas_h * 0.5 - cy * z;
        self.vel_x = 0.0;
        self.vel_y = 0.0;
    }

    /// 光标中心缩放（Zoom-to-Cursor）：以局部坐标 `(anchor_x, anchor_y)` 为不动点按倍率 `factor` 缩放。
    pub fn zoom_at(&mut self, anchor_x: f32, anchor_y: f32, factor: f32, now_ms: u32) -> bool {
        let old_z = self.viewport.zoom;
        let next_z = (old_z * factor).clamp(self.min_zoom, self.max_zoom);
        if (next_z - old_z).abs() < 1e-5 {
            return false;
        }
        let (wx, wy) = self.viewport.local_to_world(anchor_x, anchor_y);
        self.viewport.zoom = next_z;
        self.viewport.offset_x = anchor_x - wx * next_z;
        self.viewport.offset_y = anchor_y - wy * next_z;
        self.last_zoom_ms = now_ms;
        self.zoom_lock_active = true;
        true
    }

    /// 处理桌面滚轮事件（`delta_y < 0` 向上滚放大，`delta_y > 0` 向下滚缩小）。
    pub fn handle_wheel(&mut self, local_x: i16, local_y: i16, delta_y: i16, now_ms: u32) -> bool {
        if delta_y == 0 {
            return false;
        }
        let factor = if delta_y < 0 { 1.14 } else { 1.0 / 1.14 };
        self.zoom_at(local_x as f32, local_y as f32, factor, now_ms)
    }

    /// 处理触控板/移动端双指捏合缩放事件。
    pub fn handle_pinch(
        &mut self,
        local_x: i16,
        local_y: i16,
        delta_scale_x1000: i16,
        now_ms: u32,
    ) -> bool {
        if delta_scale_x1000 == 0 {
            return false;
        }
        let factor = (1.0 + (delta_scale_x1000 as f32) * 0.001).clamp(0.5, 2.0);
        self.zoom_at(local_x as f32, local_y as f32, factor, now_ms)
    }

    /// 平移相机并记录瞬时速度以驱动松手惯性滑动（Kinetic Pan）。
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.viewport.offset_x += dx;
        self.viewport.offset_y += dy;
        self.vel_x = dx * 32.0;
        self.vel_y = dy * 32.0;
    }

    /// 更新橡皮筋框选矩形（画布局部坐标）。
    pub fn update_marquee(&mut self, local_x: f32, local_y: f32, is_start: bool) {
        if is_start || self.marquee.is_none() {
            self.marquee = Some([local_x, local_y, local_x, local_y]);
        } else if let Some(ref mut m) = self.marquee {
            m[2] = local_x;
            m[3] = local_y;
        }
    }

    /// 结束框选并返回世界空间包围盒 `[min_x, min_y, max_x, max_y]`。
    pub fn finish_marquee_world(&mut self) -> Option<[f32; 4]> {
        let [x0, y0, x1, y1] = self.marquee.take()?;
        let (wx0, wy0) = self.viewport.local_to_world(x0.min(x1), y0.min(y1));
        let (wx1, wy1) = self.viewport.local_to_world(x0.max(x1), y0.max(y1));
        Some([wx0.min(wx1), wy0.min(wy1), wx0.max(wx1), wy0.max(wy1)])
    }

    /// 将当前相机中心跳转到指定世界坐标 `(wx, wy)`（供 `Minimap` 鹰眼拖拽调用）。
    pub fn center_on_world(&mut self, wx: f32, wy: f32, canvas_w: f32, canvas_h: f32) {
        self.viewport.offset_x = canvas_w * 0.5 - wx * self.viewport.zoom;
        self.viewport.offset_y = canvas_h * 0.5 - wy * self.viewport.zoom;
        self.vel_x = 0.0;
        self.vel_y = 0.0;
    }

    /// 当前是否处于 Zoom Font Lock 窗口内（手势停止 `120ms` 后自动释放）。
    #[inline]
    pub fn is_zooming(&self, now_ms: u32) -> bool {
        self.zoom_lock_active
            && now_ms.wrapping_sub(self.last_zoom_ms) < Self::ZOOM_LOCK_SETTLE_MS
    }

    /// 帧步进：推进松手惯性滑动衰减、边界柔和回弹与 120ms 缩放锁释放；
    /// 返回 `true` 表示仍在运动或等待缩放锁释放（需请求下一帧），返回 `false` 后自动进入 0% CPU 休眠。
    pub fn tick(&mut self, now_ms: u32, dt_ms: u16, canvas_w: f32, canvas_h: f32) -> bool {
        let dt = (dt_ms.max(1).min(50) as f32) * 0.001;
        let mut active = false;

        // 1. 惯性滑动指数衰减
        if self.vel_x.abs() > 4.0 || self.vel_y.abs() > 4.0 {
            self.viewport.offset_x += self.vel_x * dt;
            self.viewport.offset_y += self.vel_y * dt;
            let damp = (1.0 - 9.0 * dt).clamp(0.0, 0.96);
            self.vel_x *= damp;
            self.vel_y *= damp;
            active = true;
        } else {
            self.vel_x = 0.0;
            self.vel_y = 0.0;
        }

        // 2. 边界阻尼回弹（防止画布被完全拖出视口丢失）
        if let Some(wb) = self.world_bounds {
            let z = self.viewport.zoom;
            let min_ox = canvas_w * 0.15 - wb[2] * z;
            let max_ox = canvas_w * 0.85 - wb[0] * z;
            let min_oy = canvas_h * 0.15 - wb[3] * z;
            let max_oy = canvas_h * 0.85 - wb[1] * z;
            if min_ox <= max_ox {
                let target_x = self.viewport.offset_x.clamp(min_ox, max_ox);
                if (target_x - self.viewport.offset_x).abs() > 0.5 {
                    self.viewport.offset_x += (target_x - self.viewport.offset_x) * (12.0 * dt).min(1.0);
                    active = true;
                }
            }
            if min_oy <= max_oy {
                let target_y = self.viewport.offset_y.clamp(min_oy, max_oy);
                if (target_y - self.viewport.offset_y).abs() > 0.5 {
                    self.viewport.offset_y += (target_y - self.viewport.offset_y) * (12.0 * dt).min(1.0);
                    active = true;
                }
            }
        }

        // 3. 检查 120ms Zoom Font Lock 是否需要再等一帧切换高精度亚像素渲染
        if self.zoom_lock_active {
            if now_ms.wrapping_sub(self.last_zoom_ms) < Self::ZOOM_LOCK_SETTLE_MS {
                active = true;
            } else {
                self.zoom_lock_active = false;
                // 本帧恰好释放锁，完成后立即归零进入 0% CPU 休眠
            }
        }

        active
    }
}
