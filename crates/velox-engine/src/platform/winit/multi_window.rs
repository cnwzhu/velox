//! 多窗口生命周期管理、越界浮层窗口与无边框自定义标题栏（CSD）命中测试管理器。

use std::collections::HashMap;
use std::string::String;
use std::vec::Vec;

use crate::element::View;
use crate::engine::VeloxEngine;
use crate::platform::a11y_bridge::{AccessKitBridge, AccessKitTreeUpdate};
use crate::pod::InputEvent;
use crate::solver::Region;

/// 强类型多窗口标识符（与宿主 `winit::window::WindowId` 解耦，可独立单元测试）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct MultiWindowId(pub u32);

/// 窗口语义角色。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum WindowRole {
    #[default]
    Primary = 0,
    Secondary = 1,
    /// 可越出主窗口边界的原生弹出浮层（右键菜单 / 下拉列表）。
    Popover = 2,
    /// 轻量原生悬浮提示窗口。
    Tooltip = 3,
    /// 从主窗口 Dock 工作区拖出形成的独立原生 OS 窗口（支持共享 `Signal` 状态并随时拖回吸附合并）。
    DockTearOff = 4,
}

/// Dock 工作区停靠目标方位（用于拖回吸附合并）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum DockZone {
    Left = 0,
    Right = 1,
    Bottom = 2,
    Center = 3,
}

/// 已从主窗拖出的 Dock 面板记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DetachedDockPanel {
    pub window_id: MultiWindowId,
    pub parent_id: MultiWindowId,
    pub panel_key: String,
    pub title: String,
    pub origin_zone: DockZone,
}

/// 客户端自绘无边框标题栏（Client-Side Decorations / CSD）配置。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CsdConfig {
    /// 是否关闭 OS 原生标题栏并启用 CSD 自定义标题栏。
    pub enabled: bool,
    /// 自定义标题栏高度（逻辑像素，默认 `36`）。
    pub titlebar_height: i16,
    /// 窗口四边可拖拽缩放的边缘热区宽度（逻辑像素，默认 `6`）。
    pub resize_border_px: i16,
    /// 显式排除拖拽的右上角窗口按钮区域宽度（最小化/最大化/关闭按钮，默认 `138`）。
    pub controls_inset_right: i16,
    /// 双击标题栏空白处是否切换最大化。
    pub double_click_maximize: bool,
}

impl Default for CsdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            titlebar_height: 36,
            resize_border_px: 6,
            controls_inset_right: 138,
            double_click_maximize: true,
        }
    }
}

/// 无边框 CSD 命中测试区域（直接对应 Windows `WM_NCHITTEST` / Wayland `xdg_toplevel::move/resize`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CsdHitZone {
    Client,
    Caption,
    ResizeLeft,
    ResizeRight,
    ResizeTop,
    ResizeBottom,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
}

impl CsdConfig {
    /// 根据窗口逻辑尺寸 `(win_w, win_h)` 与局部指针坐标 `(x, y)` 判定 CSD 命中区域。
    pub fn hit_test(&self, x: i16, y: i16, win_w: i16, win_h: i16) -> CsdHitZone {
        if !self.enabled {
            return CsdHitZone::Client;
        }
        let b = self.resize_border_px.max(0);
        let left = x < b;
        let right = x >= win_w.saturating_sub(b);
        let top = y < b;
        let bottom = y >= win_h.saturating_sub(b);

        match (left, right, top, bottom) {
            (true, _, true, _) => CsdHitZone::ResizeTopLeft,
            (_, true, true, _) => CsdHitZone::ResizeTopRight,
            (true, _, _, true) => CsdHitZone::ResizeBottomLeft,
            (_, true, _, true) => CsdHitZone::ResizeBottomRight,
            (true, _, _, _) => CsdHitZone::ResizeLeft,
            (_, true, _, _) => CsdHitZone::ResizeRight,
            (_, _, true, _) => CsdHitZone::ResizeTop,
            (_, _, _, true) => CsdHitZone::ResizeBottom,
            _ => {
                if y < self.titlebar_height
                    && x < win_w.saturating_sub(self.controls_inset_right)
                {
                    CsdHitZone::Caption
                } else {
                    CsdHitZone::Client
                }
            }
        }
    }
}

/// 多窗口创建描述。
#[derive(Clone, Debug)]
pub struct MultiWindowSpec {
    pub title: String,
    pub width: i16,
    pub height: i16,
    pub role: WindowRole,
    pub parent: Option<MultiWindowId>,
    pub transparent: bool,
    pub always_on_top: bool,
    pub csd: CsdConfig,
}

impl MultiWindowSpec {
    pub fn primary(title: impl Into<String>, width: i16, height: i16) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            role: WindowRole::Primary,
            parent: None,
            transparent: false,
            always_on_top: false,
            csd: CsdConfig::default(),
        }
    }

    pub fn popover(parent: MultiWindowId, width: i16, height: i16) -> Self {
        Self {
            title: String::new(),
            width,
            height,
            role: WindowRole::Popover,
            parent: Some(parent),
            transparent: true,
            always_on_top: true,
            csd: CsdConfig::default(),
        }
    }

    pub fn with_csd(mut self, csd: CsdConfig) -> Self {
        self.csd = csd;
        self
    }
}

/// 单个受托管窗口的运行时会话（独立 `VeloxEngine` + 增量 `AccessKitBridge` + CSD 配置）。
pub struct WindowSession {
    pub id: MultiWindowId,
    pub spec: MultiWindowSpec,
    pub engine: VeloxEngine,
    pub a11y: AccessKitBridge,
    pub focused: bool,
    pub maximized: bool,
}

/// 跨平台多窗口会话管理器（支持主窗 + 任意副窗 + 越界浮层 Popover + Dock 拖出独立窗 + 统一广播事件与独立脏区步进）。
pub struct MultiWindowManager {
    next_id: u32,
    windows: HashMap<MultiWindowId, WindowSession>,
    order: Vec<MultiWindowId>,
    detached_docks: Vec<DetachedDockPanel>,
}

impl Default for MultiWindowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiWindowManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            windows: HashMap::new(),
            order: Vec::new(),
            detached_docks: Vec::new(),
        }
    }

    /// 打开并挂载一个新窗口（可为主窗、副窗或越界 Popover 浮层）。
    pub fn open_window<V: View>(&mut self, spec: MultiWindowSpec, root_view: V) -> MultiWindowId {
        let id = MultiWindowId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut engine = VeloxEngine::new(spec.width.max(1), spec.height.max(1));
        let _ = engine.mount_root(root_view);
        let session = WindowSession {
            id,
            spec,
            engine,
            a11y: AccessKitBridge::new(),
            focused: self.windows.is_empty(),
            maximized: false,
        };
        self.windows.insert(id, session);
        self.order.push(id);
        id
    }

    /// 关闭指定窗口（同时自动级联关闭挂接在该父窗口下的所有 `Popover` / `Tooltip` 子浮层）。
    pub fn close_window(&mut self, id: MultiWindowId) -> bool {
        if self.windows.remove(&id).is_none() {
            return false;
        }
        self.order.retain(|&w| w != id);
        let orphans: Vec<MultiWindowId> = self
            .windows
            .values()
            .filter(|w| w.spec.parent == Some(id))
            .map(|w| w.id)
            .collect();
        for child in orphans {
            self.close_window(child);
        }
        true
    }

    #[inline]
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    #[inline]
    pub fn window(&self, id: MultiWindowId) -> Option<&WindowSession> {
        self.windows.get(&id)
    }

    #[inline]
    pub fn window_mut(&mut self, id: MultiWindowId) -> Option<&mut WindowSession> {
        self.windows.get_mut(&id)
    }

    /// 向指定窗口派发输入事件。
    pub fn dispatch_event(&mut self, id: MultiWindowId, event: InputEvent) -> bool {
        self.windows
            .get_mut(&id)
            .is_some_and(|w| w.engine.dispatch_event(event))
    }

    /// 向全部窗口广播系统级事件（如 `InputEvent::SystemThemeChanged`）。
    pub fn broadcast_event(&mut self, event: InputEvent) -> usize {
        let mut changed = 0usize;
        for &id in &self.order {
            if let Some(w) = self.windows.get_mut(&id) {
                if w.engine.dispatch_event(event) {
                    changed += 1;
                }
            }
        }
        changed
    }

    /// 步进所有活跃窗口一帧，返回产出脏区的 `(WindowId, dirty_count, Option<AccessKitTreeUpdate>)` 列表。
    pub fn step_all(
        &mut self,
        dt_ms: u16,
    ) -> Vec<(MultiWindowId, usize, Option<AccessKitTreeUpdate>)> {
        let mut reports = Vec::with_capacity(self.order.len());
        for &id in &self.order {
            if let Some(w) = self.windows.get_mut(&id) {
                let dirty_n = w.engine.step_frame_dt(dt_ms).len();
                let a11y_update = if dirty_n > 0 {
                    w.a11y.sync_engine(&w.engine)
                } else {
                    None
                };
                reports.push((id, dirty_n, a11y_update));
            }
        }
        reports
    }

    /// 判定指定窗口坐标处的无边框 CSD 命中区域（供拖动标题栏/边缘缩放）。
    pub fn hit_test_csd(&self, id: MultiWindowId, x: i16, y: i16) -> CsdHitZone {
        let Some(w) = self.windows.get(&id) else {
            return CsdHitZone::Client;
        };
        let vp: Region = w.engine.viewport_region();
        w.spec.csd.hit_test(x, y, vp.w, vp.h)
    }

    /// 将主窗口中的某个 Dock 面板拖出（Tear-Off）为独立原生 OS 窗口，并共享同一响应式 `Signal` 图。
    pub fn tear_off_dock_panel<V: View>(
        &mut self,
        parent: MultiWindowId,
        panel_key: impl Into<String>,
        title: impl Into<String>,
        origin_zone: DockZone,
        width: i16,
        height: i16,
        view: V,
    ) -> MultiWindowId {
        let title_str = title.into();
        let key_str = panel_key.into();
        let spec = MultiWindowSpec {
            title: title_str.clone(),
            width,
            height,
            role: WindowRole::DockTearOff,
            parent: Some(parent),
            transparent: false,
            always_on_top: false,
            csd: CsdConfig {
                enabled: true,
                ..CsdConfig::default()
            },
        };
        let win_id = self.open_window(spec, view);
        self.detached_docks.push(DetachedDockPanel {
            window_id: win_id,
            parent_id: parent,
            panel_key: key_str,
            title: title_str,
            origin_zone,
        });
        win_id
    }

    /// 将独立拖出窗口重新吸附合并回主窗口指定 [`DockZone`]，关闭该子窗并返回 `(panel_key, target_zone)`。
    pub fn redock_panel(
        &mut self,
        win_id: MultiWindowId,
        target_zone: DockZone,
    ) -> Option<(String, DockZone)> {
        let pos = self
            .detached_docks
            .iter()
            .position(|d| d.window_id == win_id)?;
        let entry = self.detached_docks.remove(pos);
        let _ = self.close_window(win_id);
        Some((entry.panel_key, target_zone))
    }

    /// 返回当前所有处于独立窗口状态的 Dock 拖出面板列表。
    #[inline]
    pub fn detached_dock_panels(&self) -> &[DetachedDockPanel] {
        &self.detached_docks
    }
}
