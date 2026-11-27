//! Velox 核心渲染与事件调度引擎 `VeloxEngine`。
//!
//! 模块职责划分：
//! - `mod`：`GestureConfig`、`VeloxEngine` 结构体、视图树生命周期（`mount_root`/`insert_view`/`view`/`remove_view`）与配置接口；
//! - `frame`：帧步进、局部脏子树重排、Pass-4-Only 快路径、分数 DPI 光栅化与无障碍树导出；
//! - `pointer`：指针、手势（拖拽/长按/连发/滑动）、滚轮与点击派发；
//! - `keyboard`：统一 `dispatch_event`、焦点键盘导航、系统返回与原生 IME 预编辑/提交闭环。

mod frame;
mod keyboard;
mod pointer;

use crate::arena::SmallVec;
use crate::damage::DamageTracker;
use crate::element::View;
use crate::motion::{DrawOp, DrawOpSnapshot, MotionCache};
use crate::pod::{
    ActionScope, CoreTheme, DragEvent, FrameScope, Handle, ImePreedit, KeyModifiers, PodId,
    PodPool, PointerKind, SafeAreaInsets,
};
use crate::renderers::software::precompute_occlusion_clips;
use crate::solver::{HitRegion, InteractionState, LayoutNode, Region};

/// 单帧默认绘制指令容量基准。
pub const MAX_DRAW_OPS: usize = 128;

/// 单帧默认可交互热区容量基准。
pub const MAX_HIT_REGIONS: usize = 64;

/// 引擎触摸与手势交互参数配置（替代硬编码阈值）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct GestureConfig {
    /// 拖拽移动抑制长按/点击的位移死区（默认 `8px`）。
    pub drag_slop_px: i16,
    /// 四向快速滑动手势（`SwipeDir`）最小位移（默认 `16px`）。
    pub swipe_threshold_px: i16,
    /// 移动端左边缘右滑返回手势触发宽度与位移阈值（默认 `24px`）。
    pub edge_back_width_px: i16,
    /// 长按事件触发时长（默认 `500ms`）。
    pub long_press_ms: u16,
    /// 按住自动连发首次触发延迟（默认 `400ms`）。
    pub repeat_delay_ms: u16,
    /// 按住自动连发周期（默认 `100ms`）。
    pub repeat_interval_ms: u16,
    /// 是否允许非触摸指针（鼠标 / 手写笔）在空白可滚动区域按住左键拖动滚动页面（默认 `false`）。
    ///
    /// - 桌面鼠标环境下默认为 `false`：滚动页面仅通过鼠标滚轮或滚动条，避免按住鼠标拖选/误触把整个页面拖走；
    /// - 触摸屏（[`PointerKind::Touch`]）始终支持跟手拖拽滚动与松手惯性滚动，不受此项限制；
    /// - 若地图/白板等场景需要在桌面允许鼠标拖动画布，可显式设为 `true` 或在画布上注册 `.on_drag(...)`。
    pub mouse_drag_scroll: bool,
}

impl GestureConfig {
    /// 编译期默认手势参数常量（支持 `VeloxEngine::new` 的 `#![no_std]` `const fn` 构造）。
    #[inline(always)]
    pub const fn const_default() -> Self {
        Self {
            drag_slop_px: 8,
            swipe_threshold_px: 16,
            edge_back_width_px: 24,
            long_press_ms: 500,
            repeat_delay_ms: 400,
            repeat_interval_ms: 100,
            mouse_drag_scroll: false,
        }
    }
}

impl Default for GestureConfig {
    #[inline(always)]
    fn default() -> Self {
        Self::const_default()
    }
}

/// 引擎内置作用域快捷键表容量。
pub const ENGINE_KEYMAP_CAP: usize = 64;

/// Velox 核心渲染与事件调度引擎。
pub struct VeloxEngine {
    pool: PodPool,
    frame_scope: FrameScope,
    async_rt: crate::async_rt::ForegroundExecutor,
    motion: MotionCache,
    damage: DamageTracker,
    prev_snapshots: SmallVec<DrawOpSnapshot, MAX_DRAW_OPS>,
    cached_ops_buf: std::vec::Vec<DrawOp<'static>>,
    cached_hits_buf: std::vec::Vec<HitRegion<'static>>,
    cached_ops_ptr: *const DrawOp<'static>,
    cached_ops_len: u32,
    cached_hits_ptr: *const HitRegion<'static>,
    cached_hits_len: u32,
    cached_layout_ptr: *mut LayoutNode<'static, 'static>,
    tree_cursor_mark: u32,
    tree_temp_mark: u32,
    ops_cursor_mark: u32,
    frame_cached: bool,
    partial_relayout_count: u16,
    solve_count: u32,
    fast_paint_count: u32,
    dropped_draw_ops: u32,
    viewport: Region,
    dpi_scale: i16,
    scale_factor: f32,
    gesture_config: GestureConfig,
    root_pod: Option<PodId>,
    interaction: InteractionState,
    pressed_hit_id: u32,
    press_start_x: i16,
    press_start_y: i16,
    press_hold_ms: u16,
    repeat_accum_ms: u16,
    last_drag_vy: i16,
    last_arena_used_bytes: u32,
    last_arena_peak_bytes: u32,
    last_arena_truncations: u32,
    total_arena_truncations: u32,
    modifiers: KeyModifiers,
    active_pointer_kind: PointerKind,
    ime_preedit: ImePreedit,
    ime_enabled: bool,
    first_frame: bool,
    interaction_dirty: bool,
    has_pressed_hit: bool,
    drag_fired_on_down: bool,
    long_press_fired: bool,
    long_press_handled: bool,
    pointer_capture_id: Option<u32>,
    active_drag_payload: Option<crate::pod::DragPayload>,
    system_theme_dark: bool,
    last_pinch_delta_x1000: i16,
    last_rotate_deg10: i16,
    file_drop_hover: Option<(i16, i16)>,
    dropped_files: std::vec::Vec<std::path::PathBuf>,
    keymap: crate::keymap::Keymap<ENGINE_KEYMAP_CAP>,
}

impl VeloxEngine {

    /// 创建指定屏幕宽高 `(width, height)` 的 Velox 核心引擎实例。
    pub fn new(width: i16, height: i16) -> Self {
        Self {
            pool: PodPool::new(),
            frame_scope: FrameScope::new(),
            async_rt: crate::async_rt::ForegroundExecutor::new(),
            motion: MotionCache::new(),
            damage: DamageTracker::new(),
            prev_snapshots: SmallVec::new(),
            cached_ops_buf: std::vec::Vec::new(),
            cached_hits_buf: std::vec::Vec::new(),
            cached_ops_ptr: core::ptr::null(),
            cached_ops_len: 0,
            cached_hits_ptr: core::ptr::null(),
            cached_hits_len: 0,
            cached_layout_ptr: core::ptr::null_mut(),
            tree_cursor_mark: 0,
            tree_temp_mark: 0,
            ops_cursor_mark: 0,
            frame_cached: false,
            solve_count: 0,
            partial_relayout_count: 0,
            fast_paint_count: 0,
            dropped_draw_ops: 0,
            viewport: Region::new(0, 0, width, height),
            dpi_scale: 1,
            scale_factor: 1.0,
            gesture_config: GestureConfig::const_default(),
            root_pod: None,
            interaction: InteractionState::const_default(),
            pressed_hit_id: 0,
            press_start_x: 0,
            press_start_y: 0,
            press_hold_ms: 0,
            repeat_accum_ms: 0,
            last_drag_vy: 0,
            last_arena_used_bytes: 0,
            last_arena_peak_bytes: 0,
            last_arena_truncations: 0,
            total_arena_truncations: 0,
            modifiers: KeyModifiers::NONE,
            active_pointer_kind: PointerKind::Touch,
            ime_preedit: ImePreedit::empty(),
            ime_enabled: false,
            first_frame: true,
            interaction_dirty: false,
            has_pressed_hit: false,
            drag_fired_on_down: false,
            long_press_fired: false,
            long_press_handled: false,
            pointer_capture_id: None,
            active_drag_payload: None,
            system_theme_dark: true,
            last_pinch_delta_x1000: 0,
            last_rotate_deg10: 0,
            file_drop_hover: None,
            dropped_files: std::vec::Vec::new(),
            keymap: crate::keymap::Keymap::new(),
        }
    }

    /// 显式设置指针捕获目标元素 ID（拖拽或滑块期间锁定后续指针事件不脱靶）。
    #[inline(always)]
    pub fn set_pointer_capture(&mut self, hit_id: u32) {
        self.pointer_capture_id = Some(hit_id);
    }

    /// 释放当前指针捕获。
    #[inline(always)]
    pub fn release_pointer_capture(&mut self) {
        self.pointer_capture_id = None;
    }

    /// 获取当前正在捕获指针的元素 ID。
    #[inline(always)]
    pub const fn pointer_capture_id(&self) -> Option<u32> {
        self.pointer_capture_id
    }

    /// 开始一次应用内跨控件拖拽并附加自定义载荷 [`DragPayload`](crate::pod::DragPayload)。
    #[inline(always)]
    pub fn begin_drag_payload(&mut self, payload: crate::pod::DragPayload) {
        self.active_drag_payload = Some(payload);
        self.interaction_dirty = true;
    }

    /// 查询当前活跃的应用内拖拽载荷。
    #[inline(always)]
    pub const fn active_drag_payload(&self) -> Option<crate::pod::DragPayload> {
        self.active_drag_payload
    }

    /// 消费并结束当前应用内拖拽载荷。
    #[inline(always)]
    pub fn take_drag_payload(&mut self) -> Option<crate::pod::DragPayload> {
        self.interaction_dirty = true;
        self.active_drag_payload.take()
    }

    /// 查询当前宿主操作系统是否处于暗黑模式（随 `InputEvent::SystemThemeChanged` 自动热更新）。
    #[inline(always)]
    pub const fn system_theme_is_dark(&self) -> bool {
        self.system_theme_dark
    }

    /// 获取最近一次双指缩放千分比增量（如 `+50` 表示放大 5%）。
    #[inline(always)]
    pub const fn last_pinch_delta_x1000(&self) -> i16 {
        self.last_pinch_delta_x1000
    }

    /// 获取最近一次双指旋转增量（单位 `0.1°`）。
    #[inline(always)]
    pub const fn last_rotate_deg10(&self) -> i16 {
        self.last_rotate_deg10
    }

    /// 查询当前外部 OS 文件拖拽悬停坐标（未悬停返回 `None`）。
    #[inline(always)]
    pub const fn file_drop_hover_pos(&self) -> Option<(i16, i16)> {
        self.file_drop_hover
    }

    /// 记录外部 OS 拖入的文件路径并派发 `FileDropped` 事件。
    pub fn push_dropped_file(&mut self, path: std::path::PathBuf, x: i16, y: i16) {
        self.dropped_files.push(path);
        self.file_drop_hover = None;
        self.interaction_dirty = true;
        let _ = (x, y);
    }

    /// 读取并清空所有已拖入窗口的外部 OS 文件路径。
    pub fn take_dropped_files(&mut self) -> std::vec::Vec<std::path::PathBuf> {
        core::mem::take(&mut self.dropped_files)
    }

    /// 只读访问引擎作用域快捷键路由表 [`Keymap`](crate::keymap::Keymap)。
    #[inline(always)]
    pub fn keymap(&self) -> &crate::keymap::Keymap<ENGINE_KEYMAP_CAP> {
        &self.keymap
    }

    /// 可变访问引擎作用域快捷键路由表（用于注册单键或弦键命令绑定）。
    #[inline(always)]
    pub fn keymap_mut(&mut self) -> &mut crate::keymap::Keymap<ENGINE_KEYMAP_CAP> {
        &mut self.keymap
    }

    /// 结合当前引擎修饰键状态与焦点作用域链解析快捷键命令（支持两段弦键与超时复位）。
    pub fn resolve_key_action(
        &mut self,
        context_chain: &[&str],
        key: crate::pod::KeyCode,
    ) -> crate::keymap::KeymapMatch {
        let stroke = crate::keymap::KeyStroke::new(key, self.modifiers);
        let now_ms = self.frame_scope.now_ms();
        self.keymap.resolve(context_chain, stroke, now_ms)
    }

    /// 在未初始化静态槽位中原地构造 [`VeloxEngine`]，彻底消除 MCU 上先在栈上构造再拷贝的峰值栈开销。
    #[inline]
    pub fn init_at(slot: &mut core::mem::MaybeUninit<Self>, width: i16, height: i16) -> &mut Self {
        slot.write(Self::new(width, height))
    }

    /// 尝试注册一个独立的子视图（不触发 panic）；若 `#![no_std]` `PodPool` Slab 或槽位耗尽返回 `None`。
    #[inline]
    pub fn try_insert_view<V: View>(&mut self, view: V) -> Option<Handle<V>> {
        let handle = self.pool.try_insert(view)?;
        self.invalidate_frame_cache();
        Some(handle)
    }

    #[inline(always)]
    pub(super) fn invalidate_frame_cache(&mut self) {
        self.frame_cached = false;
        self.cached_ops_ptr = core::ptr::null();
        self.cached_ops_len = 0;
        self.cached_hits_ptr = core::ptr::null();
        self.cached_hits_len = 0;
        {
            self.cached_ops_buf.clear();
            self.cached_hits_buf.clear();
        }
    }

    #[inline(always)]
    pub(super) fn resolve_hit_pod(root_id: PodId, hit_owner: u16) -> PodId {
        if hit_owner == u16::MAX {
            root_id
        } else {
            PodId(hit_owner)
        }
    }

    #[inline]
    pub(super) fn make_action_scope(
        &mut self,
        owner: PodId,
        click_pos: (i16, i16),
        drag_ev: DragEvent,
    ) -> ActionScope<'_> {
        let acx = ActionScope::new(
            owner,
            &mut self.pool,
            click_pos,
            drag_ev,
            self.frame_scope.now_ms,
        );
        let acx = acx.with_async_app(self.async_rt.async_app());
        acx
    }

    /// 在交互事件回调结束后立即排空已就绪的前景协程、`defer` 队列与取消清理，
    /// 保证同次交互中 `acx.defer` 立刻生效且刚 `acx.spawn` 出的协程 0 延迟跑到首个 `.await`。
    #[inline]
    pub(super) fn flush_async_after_event(&mut self) {
        if self.async_rt.has_work_to_poll() {
            let _ = self.poll_async_tasks();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cache_ops_and_hits_into<'a>(
        arena: &crate::arena::ScratchArena,
        ops: &mut [DrawOp<'a>],
        hits: &[HitRegion<'a>],
        cached_ops_buf: &mut std::vec::Vec<DrawOp<'static>>,
        cached_hits_buf: &mut std::vec::Vec<HitRegion<'static>>,
        cached_ops_ptr: &mut *const DrawOp<'static>,
        cached_ops_len: &mut u32,
        cached_hits_ptr: &mut *const HitRegion<'static>,
        cached_hits_len: &mut u32,
        _dropped_draw_ops: &mut u32,
        frame_cached: &mut bool,
        last_arena_used_bytes: &mut u32,
    ) {
        precompute_occlusion_clips(ops);
        {
            cached_ops_buf.clear();
            // SAFETY: `ops` 与 `hits` 内引用的 `StrRef` / `SpriteSource` / `DivInteractivity` 均驻留于
            // `self.frame_scope.arena()`（或 `'static`），且 `invalidate_frame_cache()` 在任何 `arena.rewind()`
            // 之前清空 `cached_ops_buf` / `cached_hits_buf`，从而在 Pass-4-Only 帧中彻底避免向 `Bump` 重复分配内存。
            let static_ops: &[DrawOp<'static>] = unsafe {
                core::slice::from_raw_parts(ops.as_ptr() as *const DrawOp<'static>, ops.len())
            };
            cached_ops_buf.extend_from_slice(static_ops);
            *cached_ops_ptr = cached_ops_buf.as_ptr();
            *cached_ops_len = cached_ops_buf.len() as u32;

            cached_hits_buf.clear();
            let static_hits: &[HitRegion<'static>] = unsafe {
                core::slice::from_raw_parts(hits.as_ptr() as *const HitRegion<'static>, hits.len())
            };
            cached_hits_buf.extend_from_slice(static_hits);
            *cached_hits_ptr = cached_hits_buf.as_ptr();
            *cached_hits_len = cached_hits_buf.len() as u32;
            *frame_cached = true;
            *last_arena_used_bytes = arena.used_bytes() as u32;
        }
    }

    #[inline(always)]
    pub(super) fn invalidate_layout_cache(&mut self) {
        self.invalidate_frame_cache();
        self.cached_layout_ptr = core::ptr::null_mut();
        self.tree_cursor_mark = 0;
        self.tree_temp_mark = 0;
        self.ops_cursor_mark = 0;
    }

    /// 获取当前键盘修饰键状态。
    #[inline(always)]
    pub const fn modifiers(&self) -> KeyModifiers {
        self.modifiers
    }

    /// 更新当前键盘修饰键状态。
    #[inline(always)]
    pub fn set_modifiers(&mut self, modifiers: KeyModifiers) {
        self.modifiers = modifiers;
    }

    /// 获取或修改手势判定参数配置。
    #[inline(always)]
    pub const fn gesture_config(&self) -> GestureConfig {
        self.gesture_config
    }

    /// 设置自定义手势判定参数配置。
    #[inline(always)]
    pub fn set_gesture_config(&mut self, config: GestureConfig) {
        self.gesture_config = config;
    }

    /// 获取当前活跃指针输入设备来源类型（[`PointerKind::Touch`] / [`PointerKind::Mouse`] / [`PointerKind::Pen`]）。
    #[inline(always)]
    pub const fn pointer_kind(&self) -> PointerKind {
        self.active_pointer_kind
    }

    /// 设置当前指针输入设备来源类型（桌面平台应设为 [`PointerKind::Mouse`]，触屏设为 [`PointerKind::Touch`]）。
    #[inline(always)]
    pub fn set_pointer_kind(&mut self, kind: PointerKind) {
        self.active_pointer_kind = kind;
    }

    /// 判断当前指针设备是否允许在空白可滚动区域通过按住拖拽触发跟手/惯性滚动。
    #[inline(always)]
    pub(super) const fn allows_drag_scroll(&self) -> bool {
        matches!(self.active_pointer_kind, PointerKind::Touch)
            || self.gesture_config.mouse_drag_scroll
    }

    /// 获取累计因超出 `MAX_DRAW_OPS`（`no_std` 下）而被截断的绘制指令数量（用于可观测容量监控）。
    #[inline(always)]
    pub const fn dropped_draw_ops_count(&self) -> u32 {
        self.dropped_draw_ops
    }

    /// 获取最近一帧 `ScratchArena` 触及的峰值占用字节数（`#![no_std]` 下计入双端底/顶水位之和）。
    #[inline(always)]
    pub const fn last_arena_peak_bytes(&self) -> usize {
        self.last_arena_peak_bytes as usize
    }

    /// 获取最近一帧 `ScratchArena` 发生静默截断（`alloc_str`/`alloc_fmt`）或临时切片空降级（`alloc_temp_iter_exact`）的次数。
    #[inline(always)]
    pub const fn last_arena_truncations(&self) -> u32 {
        self.last_arena_truncations
    }

    /// 获取自引擎启动以来 `ScratchArena` 累计发生静默截断/空降级的总次数。
    #[inline(always)]
    pub const fn total_arena_truncations(&self) -> u32 {
        self.total_arena_truncations
    }

    /// 获取累计执行的完整布局求解次数（Pass 1~4，用于性能剖析与单帧条带缓存验证）。
    #[inline(always)]
    pub const fn layout_solve_count(&self) -> u32 {
        self.solve_count
    }

    /// 获取累计通过子视图布局隔离边界（Relayout Boundary）执行 $O(\text{dirty\_pod})$ 原地局部重排（跳过全局 `build_layout_tree` 与全局 Pass 1~3）完成的帧计数。
    #[inline(always)]
    pub const fn partial_relayout_count(&self) -> u32 {
        self.partial_relayout_count as u32
    }

    /// 获取累计命中子视图布局隔离边界（包含原地局部重排与跳过未脏 `SubView` 子树 Pass 1~3）的次数。
    #[inline(always)]
    pub fn relayout_boundary_hits(&self) -> u32 {
        self.pool.relayout_boundary_hits()
    }

    /// 获取累计通过 Pass-4-Only 快径（跳过 `View::render()` 与 Pass 1~3 水平/折行测量）完成的帧计数。
    #[inline(always)]
    pub const fn fast_paint_pass_count(&self) -> u32 {
        self.fast_paint_count
    }

    /// 获取累计因 `SubView` 未脏而直接复用缓存 `Element` 跳过 `View::render()` 的命中次数。
    #[inline(always)]
    pub fn subview_cache_hits(&self) -> u32 {
        self.pool.skipped_render_count()
    }

    /// 获取指定视图句柄自创建以来实际执行 `View::render()` 的次数。
    #[inline(always)]
    pub fn pod_render_count<V: View>(&self, handle: Handle<V>) -> u32 {
        self.pool.render_count(handle.pod_id())
    }

    /// 设置物理像素 HiDPI 缩放倍率（默认 `1`，桌面端可设为 `2` 实现 2x Retina 矢量光栅化）。
    /// 设置 HiDPI 整数缩放倍率（如 `1` / `2` / `3`，同步更新精确浮点 `scale_factor`）。
    #[inline(always)]
    pub fn set_dpi_scale(&mut self, scale: i16) {
        let s = scale.max(1);
        self.dpi_scale = s;
        self.scale_factor = s as f32;
    }

    /// 设置精确分数 DPI 缩放因子（如 `1.25` / `1.5` / `1.75` / `2.0`，支持 Pixel Snapping）。
    #[inline(always)]
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        let sf = scale_factor.clamp(0.5, 4.0);
        self.scale_factor = sf;
        self.dpi_scale = ((sf + 0.5) as i16).max(1);
    }

    /// 获取当前整数 HiDPI 缩放倍率（兼容旧接口）。
    #[inline(always)]
    pub const fn dpi_scale(&self) -> i16 {
        self.dpi_scale
    }

    /// 获取当前精确分数 DPI 缩放因子（如 `1.5`）。
    #[inline(always)]
    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// 获取视口矩形区域。
    #[inline(always)]
    pub const fn viewport_region(&self) -> Region {
        self.viewport
    }

    /// 调整视口尺寸（触发全屏重绘）。
    pub fn resize(&mut self, width: i16, height: i16) {
        let new_vp = Region::new(0, 0, width, height);
        if new_vp != self.viewport {
            self.viewport = new_vp;
            self.first_frame = true;
            self.invalidate_layout_cache();
            self.pool.mark_all_dirty();
        }
    }

    /// 强制标记整帧为脏并在下一帧完整重绘视口（适用于离屏截图导出或外部显存重建）。
    pub fn force_full_repaint(&mut self) {
        self.first_frame = true;
        self.invalidate_layout_cache();
        self.pool.mark_all_dirty();
    }

    /// 返回当前引擎生效的全局主题配色（[`CoreTheme`]）。
    #[inline(always)]
    pub fn theme(&self) -> CoreTheme {
        *self.frame_scope.theme()
    }

    /// 设置全局主题配色。
    pub fn set_theme(&mut self, theme: CoreTheme) {
        self.frame_scope.set_theme(theme);
        self.invalidate_layout_cache();
        self.pool.mark_all_dirty();
    }

    /// 返回当前引擎生效的国际化语言区域（[`crate::i18n::Locale`]）。
    #[inline(always)]
    pub fn locale(&self) -> crate::i18n::Locale {
        self.frame_scope.locale()
    }

    /// 运行时热切换全局语言区域：同步刷新 `FrameScope`、失效布局缓存并标脏整棵视图树（零重启切换语言）。
    pub fn set_locale(&mut self, locale: crate::i18n::Locale) {
        if self.frame_scope.locale() != locale {
            self.frame_scope.set_locale(locale);
            self.first_frame = true;
            self.invalidate_layout_cache();
            self.pool.mark_all_dirty();
        }
    }

    /// 注册静态零堆多语言词典集合并标记整棵视图树重绘。
    pub fn set_i18n_catalog(&mut self, catalog: &'static crate::i18n::StaticCatalog) {
        self.frame_scope.set_i18n_catalog(catalog);
        self.first_frame = true;
        self.invalidate_layout_cache();
        self.pool.mark_all_dirty();
    }

    /// 注册运行时动态翻译目录（支持 JSON / `key=value` 语言包热加载）并标记整棵视图树重绘。
    pub fn set_dynamic_catalog(&mut self, catalog: std::sync::Arc<crate::i18n::DynamicCatalog>) {
        self.frame_scope.set_dynamic_catalog(catalog);
        self.first_frame = true;
        self.invalidate_layout_cache();
        self.pool.mark_all_dirty();
    }

    /// 获取当前屏幕安全区域内边距。
    #[inline(always)]
    pub const fn safe_area(&self) -> SafeAreaInsets {
        self.frame_scope.safe_area()
    }

    /// 设置移动端/异形屏安全区域内边距并标记根视图重绘。
    pub fn set_safe_area(&mut self, insets: SafeAreaInsets) {
        if self.frame_scope.safe_area() != insets {
            self.frame_scope.set_safe_area(insets);
            self.first_frame = true;
            self.invalidate_layout_cache();
            self.pool.mark_all_dirty();
        }
    }

    /// 获取当前屏幕软键盘占用的底部像素高度。
    #[inline(always)]
    pub const fn keyboard_inset(&self) -> i16 {
        self.frame_scope.keyboard_inset()
    }

    /// 设置屏幕软键盘占用的底部像素高度并标记根视图重绘。
    pub fn set_keyboard_inset(&mut self, height: i16) {
        let clamped = height.max(0);
        if self.frame_scope.keyboard_inset() != clamped {
            self.frame_scope.set_keyboard_inset(clamped);
            self.first_frame = true;
            self.invalidate_layout_cache();
            self.pool.mark_all_dirty();
        }
    }

    /// 挂载根视图组件，返回其强类型句柄 `Handle<V>`。
    pub fn mount_root<V: View>(&mut self, view: V) -> Handle<V> {
        let handle = self.pool.insert(view);
        self.root_pod = Some(handle.pod_id());
        self.pool.set_cached_bounds(handle.pod_id(), self.viewport);
        self.first_frame = true;
        self.invalidate_layout_cache();
        handle
    }

    /// 在 `PodPool` 中注册一个独立子视图，返回其句柄。
    pub fn insert_view<V: View>(&mut self, view: V) -> Handle<V> {
        self.pool.insert(view)
    }

    /// 销毁并移除一个子视图（调用其 `Drop` 并回收 `PodPool` 槽位与内存）。
    pub fn remove_view<V: View>(&mut self, handle: Handle<V>) -> bool {
        let removed = self.pool.remove(handle);
        if removed {
            if self.root_pod == Some(handle.pod_id()) {
                self.root_pod = None;
            }
            self.invalidate_layout_cache();
            if let Some(root) = self.root_pod {
                self.pool.mark_dirty(root);
            }
        }
        removed
    }

    /// 获取视图可变引用并自动标记该视图为脏。
    pub fn view_mut<V: View>(&mut self, handle: Handle<V>) -> Option<&mut V> {
        self.pool.mark_dirty(handle.pod_id());
        self.pool.get_mut(handle)
    }

    /// 通过闭包修改视图状态，仅在视图字段实际发生变更时自动标记该视图为脏。
    pub fn update_view<V: View, R>(&mut self, handle: Handle<V>, f: impl FnOnce(&mut V) -> R) -> Option<R> {
        self.pool.update(handle, f)
    }

    /// 获取视图只读引用。
    pub fn view<V: View>(&self, handle: Handle<V>) -> Option<&V> {
        self.pool.get(handle)
    }

    /// 获取跨帧动画与滚动缓存池。
    #[inline(always)]
    pub fn motion_mut(&mut self) -> &mut MotionCache {
        self.interaction_dirty = true;
        &mut self.motion
    }

    /// 获取跨帧动画与滚动缓存池只读引用。
    #[inline(always)]
    pub const fn motion(&self) -> &MotionCache {
        &self.motion
    }

    /// 配置无障碍「减弱动态效果」（联动关闭几何位移、橡皮筋越界回弹与幽灵滑出）。
    #[inline]
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.motion.set_reduced_motion(reduced);
        self.interaction_dirty = true;
    }

    /// 查询当前是否开启了无障碍「减弱动态效果」。
    #[inline(always)]
    pub const fn reduced_motion(&self) -> bool {
        self.motion.reduced_motion()
    }

    /// 查询当前正在执行零配置退场动画的幽灵节点数量。
    #[inline]
    pub fn active_ghost_count(&self) -> usize {
        self.motion.active_ghost_count()
    }

    /// 获取最近一帧 `ScratchArena` 的峰值使用字节数。
    #[inline(always)]
    pub const fn last_arena_used_bytes(&self) -> usize {
        self.last_arena_used_bytes as usize
    }

    /// 获取最近一次 `step_frame_dt` 合并生成的 8 槽位脏矩形列表（供模拟器脏矩形红框透视使用）。
    #[inline(always)]
    pub fn last_damage(&self) -> &[Region] {
        self.damage.regions()
    }

    /// 获取当前物理按键/旋钮聚焦的元素序号。
    #[inline(always)]
    pub const fn focused_index(&self) -> Option<u16> {
        self.interaction.focused_index
    }

    /// 返回当前焦点是否处于键盘导航激活的可见焦点态（`:focus-visible`）。
    #[inline(always)]
    pub const fn is_focus_visible(&self) -> bool {
        self.interaction.focus_visible
    }

    /// 设置当前物理按键/旋钮聚焦的元素序号。
    #[inline(always)]
    pub fn set_focused_index(&mut self, idx: Option<u16>) {
        if self.interaction.focused_index != idx {
            self.interaction.focused_index = idx;
            if idx.is_none() {
                self.interaction.focus_visible = false;
            }
            self.interaction_dirty = true;
        }
    }

    /// 是否存在待处理的脏视图、响应式信号更新、交互状态变更、就绪异步任务或正在进行中的过渡动画。
    #[inline]
    pub fn needs_redraw(&self) -> bool {
        if self.async_rt.has_ready_work() {
            return true;
        }
        self.first_frame
            || self.interaction_dirty
            || self.frame_scope.frame_requested.get()
            || self.pool.has_dirty()
            || crate::signal::has_pending_dirty_pods()
            || self.motion.has_active_animations()
    }

    /// 获取引擎单调动画时钟（毫秒）。
    #[inline(always)]
    pub const fn clock_ms(&self) -> u32 {
        self.frame_scope.now_ms
    }

    /// 注入自定义可插拔异步调度器后端（链式构建器），如 [`crate::async_rt::ManualScheduler`] 或第三方运行时适配器。
    pub fn with_scheduler(
        self,
        backend: std::sync::Arc<dyn crate::async_rt::SchedulerBackend>,
    ) -> Self {
        self.async_rt.set_backend(backend);
        self
    }

    /// 在运行时切换当前引擎所使用的底层可插拔异步调度器后端。
    pub fn set_scheduler(
        &mut self,
        backend: std::sync::Arc<dyn crate::async_rt::SchedulerBackend>,
    ) {
        self.async_rt.set_backend(backend);
    }

    /// 返回当前引擎所绑定异步调度器的名称（如 `"velox-native-std"` / `"velox-manual"`）。
    #[inline]
    pub fn scheduler_name(&self) -> &'static str {
        self.async_rt.scheduler_name()
    }

    /// 返回当前引擎所绑定异步调度器的能力描述符。
    #[inline]
    pub fn scheduler_capabilities(&self) -> crate::async_rt::SchedulerCapabilities {
        self.async_rt.scheduler_capabilities()
    }

    /// 注入平台级异步唤醒回调（当后台线程池任务完成或定时器到期时触发事件循环零轮询唤醒）。
    pub fn set_async_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.async_rt.set_wake_ui(waker);
    }

    /// 在 UI 主线程排空所有已就绪的前景异步协程与 `defer` 回调；返回是否至少推进了 1 项任务或回调。
    pub fn poll_async_tasks(&mut self) -> bool {
        self.async_rt
            .poll_ready(&mut self.pool, self.frame_scope.now_ms)
    }

    /// 是否存在立即待执行的前景就绪异步任务或 `defer` 回调。
    #[inline]
    pub fn has_pending_async_tasks(&self) -> bool {
        self.async_rt.has_ready_work()
    }

    /// 返回当前仍活跃挂载在前景执行器中的协程数量（用于监控与测试验证取消回收无泄漏）。
    #[inline]
    pub fn active_async_task_count(&self) -> usize {
        self.async_rt.active_task_count()
    }

    /// 在 UI 主线程前景执行器上派生异步任务（闭包接收 [`crate::async_rt::AsyncApp`]）。
    pub fn spawn<T, F, Fut>(&mut self, f: F) -> crate::async_rt::Task<T>
    where
        T: 'static,
        F: FnOnce(crate::async_rt::AsyncApp) -> Fut,
        Fut: core::future::Future<Output = T> + 'static,
    {
        self.async_rt.spawn(f)
    }

    /// 在 UI 主线程前景执行器上为指定视图句柄派生绑定异步任务（闭包接收 `(Handle<V>, AsyncApp)`）。
    pub fn spawn_for<V, T, F, Fut>(
        &mut self,
        handle: Handle<V>,
        f: F,
    ) -> crate::async_rt::Task<T>
    where
        V: View,
        T: 'static,
        F: FnOnce(Handle<V>, crate::async_rt::AsyncApp) -> Fut,
        Fut: core::future::Future<Output = T> + 'static,
    {
        self.async_rt.async_app().spawn_for(handle, f)
    }

    /// 在后台常驻线程池中执行 CPU 密集型计算或阻塞 I/O，返回可 `.await` 的 [`crate::async_rt::Task<T>`]。
    #[inline]
    pub fn background_spawn<T, Fut>(&self, fut: Fut) -> crate::async_rt::Task<T>
    where
        T: Send + 'static,
        Fut: core::future::Future<Output = T> + Send + 'static,
    {
        self.background_executor().spawn(fut)
    }

    /// 获取绑定到当前引擎调度器后端的后台执行器句柄（用于 `.spawn(...)` / `.timer(Duration)`）。
    #[inline]
    pub fn background_executor(&self) -> crate::async_rt::BackgroundExecutor {
        self.async_rt.background_executor()
    }

    /// 创建由当前引擎调度器时钟驱动的非阻塞异步定时器。
    #[inline]
    pub fn timer(&self, duration: core::time::Duration) -> crate::async_rt::Timer {
        self.background_executor().timer(duration)
    }

    /// 排入一个延迟到下次前景异步轮询执行的 UI 主线程回调。
    #[inline]
    pub fn defer<F>(&mut self, f: F)
    where
        F: FnOnce(&mut ActionScope) + 'static,
    {
        self.async_rt.defer(f);
    }

    /// 立即结束所有单次过渡动画并停留在目标状态（用于“减少动态效果”无障碍设置或测试快进）。
    pub fn finish_animations(&mut self) {
        self.motion.finish_all();
        self.invalidate_frame_cache();
        if let Some(root) = self.root_pod {
            self.pool.mark_dirty(root);
        }
    }

}

impl HitRegion<'_> {
    #[inline(always)]
    const fn active_bg_or_focus(&self) -> bool {
        self.has_active_bg || self.focus_index.is_some() || self.scroll_target.is_some()
    }
}
