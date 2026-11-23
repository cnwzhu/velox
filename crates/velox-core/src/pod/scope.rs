//! 帧级与视图级渲染上下文：`FrameScope`、`Scope<V>`、`ActionScope` 与作用域 ID 派生。

use core::any::TypeId;
use core::cell::Cell;
use core::marker::PhantomData;
use core::ops::Deref;

use crate::arena::{ScratchArena, StrRef};
use crate::element::View;
use crate::i18n::DynamicCatalog;
use crate::i18n::{I18nArg, I18nContext, Locale, StaticCatalog, TextDirection};

use super::events::{DragEvent, ImePreedit, SafeAreaInsets};
use super::{CoreTheme, Handle, PodId, PodPool};

/// 基于 Murmur3 Avalanche 最终混洗的零分配确定性子元素 ID 派生函数。
///
/// 彻底替代易撞车的 `parent_id.wrapping_add(1 / 100)` 线性偏移写法：
/// 返回值恒将最高位设为 `1`（落在 `0x8000_0000..=0xFFFF_FFFF` 命名空间，永不为 `0`），
/// 与应用层常见的用户显式正小整数 ID（`1..=2_147_483_647`）在数学上完全正交零碰撞。
#[inline(always)]
pub const fn derive_scoped_id(parent_seed: u32, salt: u32, local_id: u32) -> u32 {
    let mut h = parent_seed.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= local_id.wrapping_add(0xC2B2_AE35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h | 0x8000_0000
}

/// 帧级无状态渲染上下文（供 `Widget::render_once` 与 `div().child(cx, ...)` 使用）。
pub struct FrameScope {
    pub(crate) arena: ScratchArena,
    pub(crate) theme: CoreTheme,
    pub(crate) theme_override_ptr: Cell<*const CoreTheme>,
    pub(crate) safe_area: SafeAreaInsets,
    pub(crate) keyboard_inset: i16,
    pub(crate) current_pod: PodId,
    pub(crate) current_pod_gen: Option<u16>,
    pub(crate) pool_ptr: *const PodPool,
    pub(crate) now_ms: u32,
    pub(crate) frame_requested: Cell<bool>,
    pub(crate) id_stack: Cell<[u32; 8]>,
    pub(crate) id_depth: Cell<u8>,
    pub(crate) ime_preedit: ImePreedit,
    pub(crate) i18n: I18nContext,
    pub(crate) locale_override: Cell<Option<Locale>>,
    pub(crate) catalog_override: Cell<Option<&'static StaticCatalog>>,
}

impl Default for FrameScope {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameScope {

    /// 创建默认帧级上下文。
    pub fn new() -> Self {
        Self::with_theme(CoreTheme::dark())
    }


    /// 使用指定主题创建帧级上下文。
    pub fn with_theme(theme: CoreTheme) -> Self {
        Self {
            arena: ScratchArena::new(),
            theme,
            theme_override_ptr: Cell::new(core::ptr::null()),
            safe_area: SafeAreaInsets::ZERO,
            keyboard_inset: 0,
            current_pod: PodId(0),
            current_pod_gen: None,
            pool_ptr: core::ptr::null(),
            now_ms: 0,
            frame_requested: Cell::new(false),
            id_stack: Cell::new([0; 8]),
            id_depth: Cell::new(0),
            ime_preedit: ImePreedit::empty(),
            i18n: I18nContext::new(Locale::EN_US),
            locale_override: Cell::new(None),
            catalog_override: Cell::new(None),
        }
    }

    /// 获取当前正在组合输入的 IME 预编辑字符串（无预编辑内容时返回 `None`）。
    #[inline]
    pub fn ime_preedit(&self) -> Option<&str> {
        if self.ime_preedit.is_empty() {
            None
        } else {
            Some(self.ime_preedit.as_str())
        }
    }

    /// 获取当前帧上下文中的完整 IME 预编辑状态（含可选光标区间）。
    #[inline(always)]
    pub const fn ime_preedit_state(&self) -> ImePreedit {
        self.ime_preedit
    }

    /// 设置当前帧上下文中的 IME 预编辑状态（由引擎自动同步）。
    #[inline(always)]
    pub fn set_ime_preedit(&mut self, preedit: ImePreedit) {
        self.ime_preedit = preedit;
    }

    /// 获取当前屏幕安全区域内边距（状态栏、刘海屏、底部 Home Indicator）。
    #[inline(always)]
    pub const fn safe_area(&self) -> SafeAreaInsets {
        self.safe_area
    }

    /// 设置当前屏幕安全区域内边距。
    #[inline(always)]
    pub fn set_safe_area(&mut self, insets: SafeAreaInsets) {
        self.safe_area = insets;
    }

    /// 获取当前屏幕软键盘占用的底部像素高度（`0` 表示未弹出）。
    #[inline(always)]
    pub const fn keyboard_inset(&self) -> i16 {
        self.keyboard_inset
    }

    /// 设置当前屏幕软键盘占用的底部像素高度。
    #[inline(always)]
    pub fn set_keyboard_inset(&mut self, height: i16) {
        self.keyboard_inset = height.max(0);
    }

    /// 在渲染期安全读取另一个子视图/兄弟视图 `Handle<T>` 的只读状态，返回局部借用守卫 [`super::PodReadGuard`]。
    ///
    /// 守卫通过 `Deref` 将引用生命周期严格限制在守卫存活的局部作用域内（不可泄漏为 `'a` 存入 `Element<'a>`），
    /// 且在守卫析构前阻止目标槽位进入可变借用或 `View::render`。
    #[inline]
    pub fn read<'s, T: View>(&'s self, handle: Handle<T>) -> Option<super::PodReadGuard<'s, T>> {
        if self.pool_ptr.is_null() || handle.id == self.current_pod {
            return None;
        }
        unsafe {
            let pool = &*self.pool_ptr;
            let res = pool.read_guarded(handle);
            if res.is_some() {
                pool.record_cross_read(self.current_pod, handle.id);
            }
            res
        }
    }

    /// 在渲染期通过闭包安全读取另一个 `Handle<T>` 视图并提取计算结果。
    #[inline]
    pub fn read_with<T: View, R>(&self, handle: Handle<T>, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.read(handle)?;
        Some(f(&*guard))
    }

    /// 获取瞬态帧内存池引用。
    #[inline(always)]
    pub fn arena(&self) -> &ScratchArena {
        &self.arena
    }

    /// 获取可变帧内存池引用（用于帧开始时 `rewind()`）。
    #[inline(always)]
    pub fn arena_mut(&mut self) -> &mut ScratchArena {
        self.theme_override_ptr.set(core::ptr::null());
        self.locale_override.set(None);
        self.catalog_override.set(None);
        self.id_depth.set(0);
        &mut self.arena
    }

    /// 返回当前生效的语言区域（若渲染期调用了 [`Self::override_locale`] 则优先返回覆盖值）。
    #[inline(always)]
    pub fn locale(&self) -> Locale {
        self.locale_override.get().unwrap_or(self.i18n.locale)
    }

    /// 返回当前语言的默认书写方向（`Ltr` 或 `Rtl`）。
    #[inline(always)]
    pub fn text_direction(&self) -> TextDirection {
        self.locale().text_direction()
    }

    /// 当前语言区域是否为从右到左（RTL）书写方向。
    #[inline(always)]
    pub fn is_rtl(&self) -> bool {
        self.locale().is_rtl()
    }

    /// 设置帧级默认语言区域。
    #[inline(always)]
    pub fn set_locale(&mut self, locale: Locale) {
        self.i18n.locale = locale;
        self.locale_override.set(None);
    }

    /// 在当前帧渲染期间覆盖活跃语言区域（与 [`Self::override_theme`] 对称，零可变别名风险）。
    #[inline(always)]
    pub fn override_locale(&self, locale: Locale) {
        self.locale_override.set(Some(locale));
    }

    /// 注册静态零堆多语言词典集合。
    #[inline(always)]
    pub fn set_i18n_catalog(&mut self, catalog: &'static StaticCatalog) {
        self.i18n.static_catalog = Some(catalog);
        self.catalog_override.set(None);
    }

    /// 在当前帧渲染期间临时覆盖静态词典集合（例如根视图挂载应用级词典）。
    #[inline(always)]
    pub fn override_i18n_catalog(&self, catalog: &'static StaticCatalog) {
        self.catalog_override.set(Some(catalog));
    }

    /// 注册 `std` 运行时动态翻译目录（支持运行时热加载 JSON / `key=value` 语言包，优先于静态词典）。
    #[inline]
    pub fn set_dynamic_catalog(&mut self, catalog: std::sync::Arc<DynamicCatalog>) {
        self.i18n.dynamic_catalog = Some(catalog);
    }

    #[inline]
    fn resolve_i18n_template(&self, key: &str) -> Option<&str> {
        let loc = self.locale();
        if let Some(dyn_cat) = &self.i18n.dynamic_catalog {
            if let Some(v) = dyn_cat.lookup(loc, key) {
                return Some(v);
            }
        }
        if let Some(ov_cat) = self.catalog_override.get() {
            if let Some(v) = ov_cat.lookup(loc, key) {
                return Some(v);
            }
        }
        if let Some(st_cat) = self.i18n.static_catalog {
            if let Some(v) = st_cat.lookup(loc, key) {
                return Some(v);
            }
        }
        None
    }

    #[inline]
    fn resolve_i18n_plural_template(&self, key: &str, count: i64) -> Option<&str> {
        let loc = self.locale();
        if let Some(dyn_cat) = &self.i18n.dynamic_catalog {
            if let Some(v) = dyn_cat.lookup_plural(loc, key, count) {
                return Some(v);
            }
        }
        if let Some(ov_cat) = self.catalog_override.get() {
            if let Some(v) = ov_cat.lookup_plural(loc, key, count) {
                return Some(v);
            }
        }
        if let Some(st_cat) = self.i18n.static_catalog {
            if let Some(v) = st_cat.lookup_plural(loc, key, count) {
                return Some(v);
            }
        }
        None
    }

    /// 翻译无参数文本键（未命中时回退键名本身；静态词典命中时零拷贝返回）。
    #[inline]
    pub fn t<'a>(&'a self, key: &str) -> StrRef<'a> {
        let loc = self.locale();
        if let Some(dyn_cat) = &self.i18n.dynamic_catalog {
            if let Some(v) = dyn_cat.lookup(loc, key) {
                return self.arena.alloc_str(v);
            }
        }
        if let Some(ov_cat) = self.catalog_override.get() {
            if let Some(v) = ov_cat.lookup(loc, key) {
                return StrRef::from(v);
            }
        }
        if let Some(st_cat) = self.i18n.static_catalog {
            if let Some(v) = st_cat.lookup(loc, key) {
                return StrRef::from(v);
            }
        }
        self.arena.alloc_str(key)
    }

    /// 翻译无参数文本键，若未命中任何词典则返回显式默认值 `fallback`。
    #[inline]
    pub fn t_or<'a>(&'a self, key: &str, fallback: &'a str) -> StrRef<'a> {
        if let Some(v) = self.resolve_i18n_template(key) {
            self.arena.alloc_str(v)
        } else {
            StrRef::from(fallback)
        }
    }

    /// 翻译带命名/位置参数的文本键并在 `ScratchArena` 中零堆完成插值（如 `cx.t_args("greeting", &[("name", "Velox".into())])`）。
    #[inline]
    pub fn t_args<'a>(&'a self, key: &str, args: &[(&str, I18nArg<'_>)]) -> StrRef<'a> {
        let tmpl = self.resolve_i18n_template(key).unwrap_or(key);
        crate::i18n::format_template(&self.arena, tmpl, args)
    }

    /// 按 CLDR 复数规则翻译文本键并自动注入 `{count}` 参数（如 `cx.t_plural("files", 3)`）。
    #[inline]
    pub fn t_plural<'a>(&'a self, key: &str, count: i64) -> StrRef<'a> {
        let tmpl = self.resolve_i18n_plural_template(key, count).unwrap_or(key);
        crate::i18n::format_template(&self.arena, tmpl, &[("count", I18nArg::Int(count))])
    }

    /// 压入一层作用域 ID 种子（ImGui / egui 风格 Scoped ID Stack，最多支持 8 层嵌套）。
    ///
    /// 同一页面中多次复用相同子组件或列表项模板时，通过 `push_id` / `with_id` 自动隔离所有内部交互与动画 ID。
    #[inline]
    pub fn push_id(&self, seed: u32) {
        let depth = self.id_depth.get() as usize;
        let mut stack = self.id_stack.get();
        let parent = if depth == 0 {
            self.current_pod.0 as u32
        } else {
            stack[(depth - 1).min(7)]
        };
        let combined = derive_scoped_id(parent, 0x5343_4F50, seed);
        if depth < 8 {
            stack[depth] = combined;
            self.id_stack.set(stack);
            self.id_depth.set((depth + 1) as u8);
        }
    }

    /// 弹出当前最顶层的作用域 ID 种子（与 [`Self::push_id`] 配对使用）。
    #[inline]
    pub fn pop_id(&self) {
        let depth = self.id_depth.get();
        if depth > 0 {
            self.id_depth.set(depth - 1);
        }
    }

    /// 在指定作用域 ID 种子内执行渲染闭包，退出时自动弹栈恢复外层作用域。
    #[inline]
    pub fn with_id<'a, R>(&'a self, seed: u32, f: impl FnOnce(&'a Self) -> R) -> R {
        self.push_id(seed);
        let res = f(self);
        self.pop_id();
        res
    }

    /// 基于当前作用域 ID 栈与局部 ID 派生全局唯一、不与用户显式 ID 撞车的稳定 `u32` 元素 ID。
    #[inline]
    pub fn scoped_id(&self, local_id: u32) -> u32 {
        let depth = self.id_depth.get() as usize;
        let seed = if depth == 0 {
            self.current_pod.0 as u32
        } else {
            self.id_stack.get()[(depth - 1).min(7)]
        };
        derive_scoped_id(seed, 0x4C4F_434C, local_id)
    }

    /// 获取当前主题配色令牌。
    #[inline(always)]
    pub fn theme(&self) -> &CoreTheme {
        let ptr = self.theme_override_ptr.get();
        if !ptr.is_null() {
            unsafe { &*ptr }
        } else {
            &self.theme
        }
    }

    /// 设置当前主题配色令牌。
    #[inline(always)]
    pub fn set_theme(&mut self, theme: CoreTheme) {
        self.theme = theme;
        self.theme_override_ptr.set(core::ptr::null());
    }

    /// 在当前帧渲染期间覆盖活跃主题配色（分配在 `ScratchArena` 中，零可变别名风险）。
    #[inline]
    pub fn override_theme(&self, theme: CoreTheme) {
        if self.arena.can_alloc_slice::<CoreTheme>(1) {
            let alloc_ref: &CoreTheme = self.arena.alloc(theme);
            self.theme_override_ptr.set(alloc_ref as *const CoreTheme);
        }
    }

    /// 获取当前正在渲染的 `PodId`。
    #[inline(always)]
    pub fn current_pod(&self) -> PodId {
        self.current_pod
    }

    /// 引擎单调动画时钟（毫秒，自引擎创建起按 `step_frame_dt(dt)` 累加，约 49 天回绕）。
    ///
    /// 配合 [`Tween`](crate::motion::Tween) / [`Presence`](crate::motion::Presence) 实现与帧率无关的自定义动画。
    #[inline(always)]
    pub const fn now_ms(&self) -> u32 {
        self.now_ms
    }

    /// 设置动画时钟（由引擎在每帧渲染前自动调用；自定义宿主或测试可手动设置）。
    #[inline(always)]
    pub fn set_now_ms(&mut self, now_ms: u32) {
        self.now_ms = now_ms;
    }

    /// 请求引擎在下一帧继续重新渲染（用于由 `now_ms()` 驱动的连续动画，如旋转加载指示器）。
    #[inline(always)]
    pub fn request_frame(&self) {
        self.frame_requested.set(true);
    }

    /// 若 `running` 为真则请求下一帧，并原样返回 `running`（便于 `cx.keep_animating(tween.is_running(now))`）。
    #[inline(always)]
    pub fn keep_animating(&self, running: bool) -> bool {
        if running {
            self.frame_requested.set(true);
        }
        running
    }

    /// 本帧渲染期间是否有视图请求了下一帧（`request_frame()` / `keep_animating(true)`）。
    #[inline(always)]
    pub fn is_frame_requested(&self) -> bool {
        self.frame_requested.get()
    }
}

/// 有状态视图渲染上下文 `Scope<V>`。
///
/// 通过 `#[repr(transparent)]` 与 `Deref<Target = FrameScope>` 保证与 `FrameScope` 内存布局 100% 一致，
/// 自动兼容所有接受 `&FrameScope` 的 `.child(cx, ...)` 调用。
#[repr(transparent)]
pub struct Scope<V> {
    pub(crate) frame: FrameScope,
    _marker: PhantomData<V>,
}

impl<V> Default for Scope<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Scope<V> {
    /// 创建视图作用域。
    pub fn new() -> Self {
        Self {
            frame: FrameScope::new(),
            _marker: PhantomData,
        }
    }

    /// 设置当前所属的 `PodId`。
    #[inline(always)]
    pub fn set_current_pod(&mut self, id: PodId) {
        self.frame.current_pod = id;
    }

    /// 获取当前主题配色。
    #[inline(always)]
    pub fn theme(&self) -> &CoreTheme {
        self.frame.theme()
    }

    /// 获取帧内存池引用。
    #[inline(always)]
    pub fn arena(&self) -> &ScratchArena {
        self.frame.arena()
    }

    /// 在指定作用域 ID 种子内执行视图渲染闭包（保留 `&'a Scope<V>` 以便直接调用 `cx.listener` / `cx.action` 并返回 `Element<'a>`）。
    #[inline]
    pub fn with_id<'a, R>(&'a self, seed: u32, f: impl FnOnce(&'a Self) -> R) -> R {
        self.frame.push_id(seed);
        let res = f(self);
        self.frame.pop_id();
        res
    }
}

impl<V: View> Scope<V> {
    /// 将组件状态变更闭包分配至 `ScratchArena` 中，并在事件触发时自动路由回当前 `PodId` 对应的 `V` 实例。
    ///
    /// 默认在可变回调结束后自动标记当前有状态 `PodId` 为脏（消除逐字节读取未初始化 Padding 的 UB，并正确捕获 `Vec`/`String` 等堆内修改）；
    /// 若回调仅做只读判断无需重绘，可调用 [`ActionScope::skip_notify`]。
    pub fn listener<'a, E, F>(&'a self, handler: F) -> &'a dyn Fn(E, &mut ActionScope)
    where
        E: 'a,
        F: Fn(&mut V, E, &mut ActionScope) + 'a,
    {
        let pod_id = self.frame.current_pod;
        let pod_gen = self.frame.current_pod_gen;
        self.frame.arena.alloc(move |event: E, acx: &mut ActionScope| {
            let prev_pod = acx.active_pod;
            let prev_skip = acx.notify_skipped;
            acx.active_pod = pod_id;
            acx.notify_skipped = false;
            if let Some(view_ptr) = acx.acquire_view_mut_ptr(pod_id, pod_gen, TypeId::of::<V>()) {
                let view_ref = unsafe { &mut *(view_ptr as *mut V) };
                handler(view_ref, event, acx);
                acx.release_view_borrow(pod_id);
                if core::mem::size_of::<V>() > 0 && !acx.notify_skipped {
                    acx.notify();
                }
            }
            acx.flush_signal_dirty_pods();
            acx.notify_skipped = prev_skip;
            acx.active_pod = prev_pod;
        })
    }

    /// 构造无需事件参数的动作回调 `&'a dyn Fn(&mut ActionScope)`（直接用于 `on_dismiss` / `on_focus` / `on_click_raw`，消除手动 `arena.alloc` 包装）。
    ///
    /// 默认在可变回调结束后自动标记当前有状态 `PodId` 为脏；若无需重绘可调用 [`ActionScope::skip_notify`]。
    pub fn action<'a, F>(&'a self, handler: F) -> &'a dyn Fn(&mut ActionScope)
    where
        F: Fn(&mut V, &mut ActionScope) + 'a,
    {
        let pod_id = self.frame.current_pod;
        let pod_gen = self.frame.current_pod_gen;
        self.frame.arena.alloc(move |acx: &mut ActionScope| {
            let prev_pod = acx.active_pod;
            let prev_skip = acx.notify_skipped;
            acx.active_pod = pod_id;
            acx.notify_skipped = false;
            if let Some(view_ptr) = acx.acquire_view_mut_ptr(pod_id, pod_gen, TypeId::of::<V>()) {
                let view_ref = unsafe { &mut *(view_ptr as *mut V) };
                handler(view_ref, acx);
                acx.release_view_borrow(pod_id);
                if core::mem::size_of::<V>() > 0 && !acx.notify_skipped {
                    acx.notify();
                }
            }
            acx.flush_signal_dirty_pods();
            acx.notify_skipped = prev_skip;
            acx.active_pod = prev_pod;
        })
    }
}

impl<V> Deref for Scope<V> {
    type Target = FrameScope;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}

/// 事件回调执行期上下文（支持 `cx.notify()` 触发当前 `PodId` 局部脏标记与 `stop_propagation()` 阻断冒泡）。
pub struct ActionScope<'p> {
    pub(crate) active_pod: PodId,
    pub(crate) pool: &'p mut PodPool,
    pub(crate) click_pos: (i16, i16),
    pub(crate) drag_event: DragEvent,
    pub(crate) propagation_stopped: bool,
    pub(crate) notify_skipped: bool,
    pub(crate) now_ms: u32,
    pub(crate) modifiers: super::events::KeyModifiers,
    pub(crate) async_app: Option<crate::async_rt::AsyncApp>,
}

impl<'p> ActionScope<'p> {
    #[inline(always)]
    pub(crate) fn new(
        active_pod: PodId,
        pool: &'p mut PodPool,
        click_pos: (i16, i16),
        drag_event: DragEvent,
        now_ms: u32,
    ) -> Self {
        Self {
            active_pod,
            pool,
            click_pos,
            drag_event,
            propagation_stopped: false,
            notify_skipped: false,
            now_ms,
            modifiers: super::events::KeyModifiers::NONE,
            async_app: None,
        }
    }

    #[inline(always)]
    pub(crate) fn with_async_app(mut self, app: crate::async_rt::AsyncApp) -> Self {
        self.async_app = Some(app);
        self
    }

    #[inline(always)]
    pub(crate) fn with_modifiers(mut self, modifiers: super::events::KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// 获取当前正在执行事件回调的视图句柄 [`Handle<V>`]（带 ABA 代号与运行时类型校验；全平台 `#![no_std]` 可用）。
    #[inline]
    pub fn handle<V: View>(&self) -> Option<Handle<V>> {
        let generation = self
            .pool
            .current_generation_for_type(self.active_pod, TypeId::of::<V>())?;
        Some(Handle::with_generation(self.active_pod, generation))
    }

    /// 在当前事件回调中派生绑定到当前视图的 UI 前景异步任务（与 GPUI `cx.spawn(async move |this, cx| ...)` 对齐）。
    ///
    /// 返回 RAII [`crate::async_rt::Task<T>`]：存入视图字段可在覆盖或视图卸载时自动取消，或调用 `.detach()` 独立运行。
    pub fn spawn<V, T, F, Fut>(&mut self, f: F) -> crate::async_rt::Task<T>
    where
        V: View,
        T: 'static,
        F: FnOnce(Handle<V>, crate::async_rt::AsyncApp) -> Fut,
        Fut: core::future::Future<Output = T> + 'static,
    {
        let handle = self
            .handle::<V>()
            .expect("ActionScope::spawn::<V> 类型与当前活跃视图不匹配");
        let app = self
            .async_app
            .clone()
            .expect("ActionScope 未绑定 ForegroundExecutor");
        app.spawn_for(handle, f)
    }

    /// 在当前事件回调中派生不绑定特定视图句柄的 UI 前景异步任务（闭包接收 [`crate::async_rt::AsyncApp`]）。
    pub fn spawn_app<T, F, Fut>(&mut self, f: F) -> crate::async_rt::Task<T>
    where
        T: 'static,
        F: FnOnce(crate::async_rt::AsyncApp) -> Fut,
        Fut: core::future::Future<Output = T> + 'static,
    {
        let app = self
            .async_app
            .clone()
            .expect("ActionScope 未绑定 ForegroundExecutor");
        app.spawn(f)
    }

    /// 获取绑定到当前引擎调度器后端的后台异步执行器句柄（用于 `.spawn(...)` / `.timer(Duration)`）。
    #[inline]
    pub fn background_executor(&self) -> crate::async_rt::BackgroundExecutor {
        self.async_app
            .as_ref()
            .map(|app| app.background_executor())
            .unwrap_or_else(crate::async_rt::BackgroundExecutor::new)
    }

    /// 在当前引擎配置的后台调度器中直接派生 CPU 密集型计算或阻塞 I/O 任务。
    #[inline]
    pub fn background_spawn<T, Fut>(&self, fut: Fut) -> crate::async_rt::Task<T>
    where
        T: Send + 'static,
        Fut: core::future::Future<Output = T> + Send + 'static,
    {
        self.background_executor().spawn(fut)
    }

    /// 创建由当前引擎调度器时钟驱动的非阻塞异步定时器。
    #[inline]
    pub fn timer(&self, duration: core::time::Duration) -> crate::async_rt::Timer {
        self.background_executor().timer(duration)
    }

    /// 排入一个延迟到当前事件冒泡结束后在 UI 主线程执行的回调（避免与当前视图可变借用冲突）。
    #[inline]
    pub fn defer<F>(&mut self, f: F)
    where
        F: FnOnce(&mut ActionScope) + 'static,
    {
        if let Some(app) = &self.async_app {
            app.defer(f);
        }
    }

    /// 排入一个延迟到当前事件冒泡结束后在当前视图上执行的回调（提供直接 `&mut V` 引用，与 GPUI `cx.defer` 对齐）。
    pub fn defer_self<V, F>(&mut self, f: F)
    where
        V: View,
        F: FnOnce(&mut V, &mut ActionScope) + 'static,
    {
        let Some(handle) = self.handle::<V>() else {
            return;
        };
        if let Some(app) = &self.async_app {
            app.defer(move |acx| {
                let _ = acx.update(handle, f);
            });
        }
    }

    /// 标记当前正在处理事件的组件视图（`PodId`）为脏，将其压入局部重绘队列。
    #[inline]
    pub fn notify(&mut self) {
        self.notify_skipped = false;
        self.pool.mark_dirty(self.active_pod);
    }

    /// 显式声明本次 `listener` / `action` / `update` 回调未更改视图状态，跳过默认自动标脏。
    #[inline(always)]
    pub fn skip_notify(&mut self) {
        self.notify_skipped = true;
    }

    /// 将当前响应式信号（`Signal<T>`）收集到的脏 `PodId` 刷入 `PodPool` 脏队列。
    #[inline]
    pub fn flush_signal_dirty_pods(&mut self) {
        self.pool.flush_signal_dirty_pods();
    }

    /// 阻止当前事件继续向父级容器冒泡传递。
    #[inline(always)]
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    /// 当前事件是否已调用 `stop_propagation()` 停止冒泡。
    #[inline(always)]
    pub const fn is_propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }

    /// 标记指定句柄对应的子视图为脏。
    #[inline]
    pub fn notify_handle<T: View>(&mut self, handle: Handle<T>) {
        if self.pool.get(handle).is_some() {
            self.pool.mark_dirty(handle.id);
        }
    }

    /// 在事件回调中读取另一个 `Handle<T>` 的只读状态（若目标视图已被可变借用则返回 `None`）。
    #[inline]
    pub fn read<'s, T: View>(&'s self, handle: Handle<T>) -> Option<&'s T> {
        if handle.id == self.active_pod {
            return None;
        }
        self.pool.get(handle)
    }

    /// 在事件回调中安全更新另一个 `Handle<T>` 视图的状态并自动标记其局部脏区。
    /// 若发生 `A -> B -> A` 循环重入，内层对已借用视图的 `update` 将安全返回 `None`，彻底杜绝双 `&mut` 别名 UB。
    pub fn update<T: View, R>(
        &mut self,
        handle: Handle<T>,
        f: impl FnOnce(&mut T, &mut ActionScope) -> R,
    ) -> Option<R> {
        if handle.id == self.active_pod {
            return None;
        }
        let target_ptr =
            self.pool
                .acquire_raw_mut(handle.id, Some(handle.generation), TypeId::of::<T>())?;
        let prev_pod = self.active_pod;
        let prev_skip = self.notify_skipped;
        self.active_pod = handle.id;
        self.notify_skipped = false;
        let target_ref = unsafe { &mut *(target_ptr as *mut T) };
        let res = f(target_ref, self);
        let skipped = self.notify_skipped;
        self.pool.release_borrow(handle.id);
        if core::mem::size_of::<T>() > 0 && !skipped {
            self.pool.mark_dirty(handle.id);
        }
        self.flush_signal_dirty_pods();
        self.notify_skipped = prev_skip;
        self.active_pod = prev_pod;
        Some(res)
    }

    /// 获取当前点击事件的屏幕坐标。
    #[inline(always)]
    pub const fn click_position(&self) -> (i16, i16) {
        self.click_pos
    }

    /// 获取当前拖拽事件详情。
    #[inline(always)]
    pub const fn drag_event(&self) -> DragEvent {
        self.drag_event
    }

    /// 事件触发时刻的引擎动画时钟（毫秒，与渲染期 `FrameScope::now_ms()` 同源，用于启动 `Tween` / `Presence`）。
    #[inline(always)]
    pub const fn now_ms(&self) -> u32 {
        self.now_ms
    }

    /// 事件触发时刻的键盘修饰键状态（`Shift` / `Ctrl` / `Alt` / `Logo`）。
    #[inline(always)]
    pub const fn modifiers(&self) -> super::events::KeyModifiers {
        self.modifiers
    }

    pub(crate) fn acquire_view_mut_ptr(
        &mut self,
        id: PodId,
        expected_gen: Option<u16>,
        expected_type: TypeId,
    ) -> Option<*mut u8> {
        self.pool.acquire_raw_mut(id, expected_gen, expected_type)
    }

    pub(crate) fn release_view_borrow(&mut self, id: PodId) {
        self.pool.release_borrow(id);
    }
}
