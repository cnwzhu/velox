//! Velox 跨平台桌面组件画廊示例（`desktop_app` — **Velox Base Gallery**）。
//!
//! 直接使用 `velox-base` 原语构建 11 个分组可交互组件故事页，
//! 完整展示现代桌面视觉样式、焦点环、键盘导航、文本/代码编辑与浮层遮罩能力：
//! - **ACTIONS & INPUTS**：`Overview`、`Button & Toggle`、`Checkbox & Radio`、`Switch & Slider`、`Inputs & Select`；
//! - **NAVIGATION & EDITOR**：`Tabs & Accordion`、`Code Editor`；
//! - **OVERLAYS & FEEDBACK**：`Dialog & Modal`、`Alerts & Badges`；
//! - **DATA DISPLAY**：`Table & List`、`Settings & Icons`。

pub mod async_demo;
pub mod demos;
pub mod i18n;
pub mod reactive;
mod stories;

pub use async_demo::{
    run_async_platform_demo, run_tokio_http_demo, AsyncDemoReport, AsyncTaskStatus, AsyncTaskView,
    HeadlessAsyncPlatform, HttpDemoReport,
};
pub use velox_kit::prelude::block_on;
pub use velox_kit::TokioScheduler;
use i18n::{next_demo_locale, DESKTOP_CATALOG};
pub use stories::canvas::{CanvasDemoState, CanvasPreset};
pub use stories::chat::{ChatDemoState, ChatEntry, ChatStreamPhase};
pub use reactive::{GallerySignals, OverviewPlaygroundView, StatusBarView};
use velox_kit::component::{
    AlertDialog, Button, CalendarState, ColorPickerState, ContextMenu, DateSelection, Dialog, Edit,
    EditableText, IconName, MenuItem, Metrics, Modal, MotionPreset, Notification,
    NotificationVariant, OtpState, RustSyntaxHighlighter, Sheet, SheetSide, Sidebar, SidebarItem,
    SidebarSection, SimpleDate, SortDirection, SpringAnimator, StatusBarItem, Svg, SyntaxEditor,
    SyntaxTheme, TreeState,
};
use velox_kit::prelude::*;

/// 默认桌面画廊窗口逻辑宽度（像素）。
pub const DEFAULT_DESKTOP_WIDTH: i16 = 1080;
/// 默认桌面画廊窗口逻辑高度（像素）。
pub const DEFAULT_DESKTOP_HEIGHT: i16 = 700;

/// Velox 原生多色矢量 Logo SVG。
const VELOX_LOGO_SVG: &str = r##"<svg viewBox="0 0 24 24">
  <rect x="1" y="1" width="22" height="22" rx="6" fill="#09090B" stroke="#3B82F6" stroke-width="1.5"/>
  <path d="M5.5 7.2L12 18.2L18.5 7.2" fill="none" stroke="#3B82F6" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M9.4 7.2L12 12.2L14.6 7.2Z" fill="#60A5FA"/>
  <circle cx="12" cy="5.2" r="1.2" fill="#FAFAFA"/>
</svg>"##;

const INITIAL_CODE_SAMPLE: &str = "\
/* Velox CodeEditor — powered by cosmic-text inspired engine
 * Supports: BufferLine lazy shaping, soft-wrap, subpixel hit(),
 * 2x-click word / 3x-click line selection, Shift+Arrows & Change::reverse() undo/redo.
 */
use velox_kit::base::{Button, Checkbox, Slider, Switch};
use velox_kit::prelude::*;

#[derive(Default, Debug)]
pub struct GalleryCounter {
    pub count: i32,
    pub enabled: bool,
}

impl View for GalleryCounter {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let label = fmt!(cx, \"Clicks: {} (active: {})\", self.count, self.enabled);
        div().size_full().p(20).gap(12).bg(cx.theme().card).child(cx, Button::new(1, label).px(14).h(32).bg(Color::WHITE).text_color(Color::BLACK)).into_element(cx)
    }
}";

/// Gallery 侧边栏组件故事页枚举（15 个交互式故事页）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GalleryStory {
    Overview,
    Buttons,
    Selection,
    Sliders,
    Inputs,
    Navigation,
    Editor,
    Overlays,
    Feedback,
    Data,
    SettingsIcons,
    Effects,
    Motion,
    EngineLab,
    Chat,
    Canvas,
}

/// 兼容旧命名的类型别名。
pub type DesktopTab = GalleryStory;

impl GalleryStory {
    /// 返回与 [`SidebarItem`] 对应的导航项 ID。
    #[inline(always)]
    pub const fn id(self) -> u32 {
        match self {
            Self::Overview => 1,
            Self::Buttons => 2,
            Self::Selection => 3,
            Self::Sliders => 4,
            Self::Inputs => 5,
            Self::Navigation => 6,
            Self::Editor => 7,
            Self::Overlays => 8,
            Self::Feedback => 9,
            Self::Data => 10,
            Self::SettingsIcons => 11,
            Self::Effects => 12,
            Self::Motion => 13,
            Self::EngineLab => 14,
            Self::Chat => 15,
            Self::Canvas => 16,
        }
    }

    /// 由导航项 ID 映射为对应故事页。
    #[inline(always)]
    pub const fn from_id(id: u32) -> Self {
        match id {
            1 => Self::Overview,
            2 => Self::Buttons,
            3 => Self::Selection,
            4 => Self::Sliders,
            5 => Self::Inputs,
            6 => Self::Navigation,
            7 => Self::Editor,
            8 => Self::Overlays,
            9 => Self::Feedback,
            10 => Self::Data,
            11 => Self::SettingsIcons,
            12 => Self::Effects,
            13 => Self::Motion,
            14 => Self::EngineLab,
            15 => Self::Chat,
            _ => Self::Canvas,
        }
    }

    /// 按 CLI 短名称解析故事页（用于 `--story <name>` 快速直达任意组件页面）。
    pub fn parse_cli(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("overview") {
            Some(Self::Overview)
        } else if name.eq_ignore_ascii_case("buttons") || name.eq_ignore_ascii_case("button") {
            Some(Self::Buttons)
        } else if name.eq_ignore_ascii_case("selection")
            || name.eq_ignore_ascii_case("checkbox")
            || name.eq_ignore_ascii_case("radio")
        {
            Some(Self::Selection)
        } else if name.eq_ignore_ascii_case("sliders")
            || name.eq_ignore_ascii_case("switch")
            || name.eq_ignore_ascii_case("slider")
        {
            Some(Self::Sliders)
        } else if name.eq_ignore_ascii_case("inputs")
            || name.eq_ignore_ascii_case("select")
            || name.eq_ignore_ascii_case("input")
        {
            Some(Self::Inputs)
        } else if name.eq_ignore_ascii_case("navigation")
            || name.eq_ignore_ascii_case("tabs")
            || name.eq_ignore_ascii_case("accordion")
        {
            Some(Self::Navigation)
        } else if name.eq_ignore_ascii_case("editor") || name.eq_ignore_ascii_case("code") {
            Some(Self::Editor)
        } else if name.eq_ignore_ascii_case("overlays")
            || name.eq_ignore_ascii_case("dialog")
            || name.eq_ignore_ascii_case("modal")
        {
            Some(Self::Overlays)
        } else if name.eq_ignore_ascii_case("feedback")
            || name.eq_ignore_ascii_case("alerts")
            || name.eq_ignore_ascii_case("badges")
        {
            Some(Self::Feedback)
        } else if name.eq_ignore_ascii_case("data")
            || name.eq_ignore_ascii_case("table")
            || name.eq_ignore_ascii_case("list")
        {
            Some(Self::Data)
        } else if name.eq_ignore_ascii_case("settings") || name.eq_ignore_ascii_case("icons") {
            Some(Self::SettingsIcons)
        } else if name.eq_ignore_ascii_case("effects")
            || name.eq_ignore_ascii_case("shader")
            || name.eq_ignore_ascii_case("shaders")
            || name.eq_ignore_ascii_case("fx")
        {
            Some(Self::Effects)
        } else if name.eq_ignore_ascii_case("motion")
            || name.eq_ignore_ascii_case("spring")
            || name.eq_ignore_ascii_case("physics")
            || name.eq_ignore_ascii_case("animation")
        {
            Some(Self::Motion)
        } else if name.eq_ignore_ascii_case("engine")
            || name.eq_ignore_ascii_case("lab")
            || name.eq_ignore_ascii_case("engine-lab")
        {
            Some(Self::EngineLab)
        } else if name.eq_ignore_ascii_case("chat")
            || name.eq_ignore_ascii_case("ai")
            || name.eq_ignore_ascii_case("ai-chat")
        {
            Some(Self::Chat)
        } else if name.eq_ignore_ascii_case("canvas")
            || name.eq_ignore_ascii_case("stadium")
            || name.eq_ignore_ascii_case("map")
        {
            Some(Self::Canvas)
        } else {
            None
        }
    }

    /// 返回故事页对应的国际化翻译键。
    #[inline(always)]
    pub const fn i18n_key(self) -> &'static str {
        match self {
            Self::Overview => "story.overview",
            Self::Buttons => "story.buttons",
            Self::Selection => "story.selection",
            Self::Sliders => "story.sliders",
            Self::Inputs => "story.inputs",
            Self::Navigation => "story.nav",
            Self::Editor => "story.editor",
            Self::Overlays => "story.overlays",
            Self::Feedback => "story.feedback",
            Self::Data => "story.data",
            Self::SettingsIcons => "story.icons",
            Self::Effects => "story.effects",
            Self::Motion => "story.motion",
            Self::EngineLab => "story.engine",
            Self::Chat => "story.chat",
            Self::Canvas => "story.canvas",
        }
    }
}

/// 文本输入框/代码编辑器右键上下文菜单目标标识。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ContextMenuTarget {
    NameInput,
    PasswordInput,
    CodeEditor,
}

/// Velox Base Gallery 跨平台桌面应用主视图状态。
pub struct GalleryApp {
    /// 当前激活的故事页。
    pub active_story: GalleryStory,
    /// 左侧边栏是否处于折叠紧凑模式。
    pub sidebar_collapsed: bool,
    /// 当前明暗主题模式（默认 `Dark`，支持一键切换 `Light`）。
    pub theme_mode: ThemeMode,
    /// 当前界面语言区域（`en-US` / `zh-CN` / `ja-JP`）。
    pub locale: Locale,
    /// 累计交互事件计数（显示在状态栏）。
    pub event_count: u32,
    /// 细粒度响应式信号图（跨子视图共享，避免高频交互标脏根视图）。
    pub signals: GallerySignals,
    /// 独立固定高度底部状态栏子视图句柄（命中 L2 原地局部重排）。
    pub status_bar_view: Option<Handle<StatusBarView>>,
    /// `Overview` 故事页独立实时体验台子视图句柄（命中 L2 原地局部重排）。
    pub overview_playground: Option<Handle<OverviewPlaygroundView>>,
    /// `Overview` 故事页独立后台 `async/await` 异步任务演示子视图句柄（命中 L2 原地局部重排）。
    pub async_task_view: Option<Handle<AsyncTaskView>>,
    last_synced_root_events: u32,

    // Overview & Sliders
    pub cpu_load_pct: u8,
    pub frame_budget_pct: i32,
    pub switch_states: [bool; 4],
    pub slider_continuous: i32,
    pub slider_stepped: i32,

    // Buttons & Toggle
    pub button_clicks: u32,
    pub button_selected: bool,
    pub button_loading: bool,
    pub toggle_single: bool,
    pub toggle_group_mask: u32,

    // Checkbox & Radio
    pub cb_items: [bool; 3],
    pub radio_plan: usize,
    pub radio_density: usize,

    // Inputs & Select
    pub active_input_field: u8,
    pub text_input_name: EditableText<64>,
    pub text_input_pass: EditableText<64>,
    pub password_visible: bool,
    pub number_val: i64,
    pub select_open: bool,
    pub select_idx: Option<usize>,
    pub dropdown_open: bool,
    pub dropdown_idx: usize,
    pub calendar_state: CalendarState,

    // Navigation & Accordion
    pub tabs_segmented_idx: usize,
    pub tabs_underline_idx: usize,
    pub tabs_pill_idx: usize,
    pub accordion_mask: u32,
    pub stepper_active: usize,
    pub pagination_page: usize,

    // Code Editor (cosmic-text architecture: Buffer + CosmicEditor + incremental SyntaxEditor)
    pub editor_buf: SyntaxEditor<RustSyntaxHighlighter>,
    pub editor_focused: bool,
    /// 当前打开的文本右键上下文菜单状态：`(目标输入控件, 窗口 X, 窗口 Y)`。
    pub input_ctx_menu: Option<(ContextMenuTarget, i16, i16)>,

    // Overlays & Feedback
    pub about_dialog_open: bool,
    pub notification_open: bool,
    pub demo_dialog_open: bool,
    pub demo_alert_dialog_open: bool,
    pub demo_modal_open: bool,
    pub demo_sheet_open: bool,
    pub last_overlay_result: &'static str,
    pub tooltip_open: bool,
    pub popover_open: bool,
    pub tag_visible_mask: u8,

    // Data Display & Settings
    pub selected_pkg_row: usize,
    pub table_sort_col: usize,
    pub table_sort_dir: SortDirection,
    pub list_selected: usize,
    pub tree_state: TreeState<16>,
    pub chart_dataset_b: bool,
    pub chart_anim_epoch: u32,
    pub vsync_enabled: bool,
    pub hidpi_enabled: bool,
    pub damage_overlay_hint: bool,

    // Effects & Shaders Lab
    pub fx_grad_angle: i32,
    pub fx_grad_space: GradientSpace,
    pub fx_grad_extend: ExtendMode,
    pub fx_blend_idx: usize,
    pub fx_backdrop_blur: i32,
    pub fx_acrylic_enabled: bool,
    pub fx_filter_preset: usize,
    pub fx_rotation_deg: i32,
    pub fx_clip_shape: usize,

    // Motion & Physics Lab (std SpringAnimator + Carousel + Roller)
    pub spring_anim: SpringAnimator,
    pub motion_last_ms: u32,
    pub motion_target_pct: f32,
    pub motion_reveal_open: bool,
    pub motion_carousel_idx: usize,
    pub motion_roller_idx: usize,
    pub motion_accordion_mask: u32,
    pub motion_tab_idx: usize,

    // Engine Lab & Extended Component States
    pub lab_object_fit_idx: usize,
    pub lab_rtl_enabled: bool,
    pub lab_radar_angle: i32,
    pub inspector_enabled: bool,
    pub feedback_rating: u8,
    pub input_otp: OtpState<6>,
    pub input_color_picker: ColorPickerState,

    // AI Chat (gpui-kit animated chat story)
    pub chat: stories::chat::ChatDemoState,

    // Interactive 2D Canvas (Hilbert RTree + OBB SDF + Minimap + optional custom-shader)
    pub canvas_demo: stories::canvas::CanvasDemoState,
}

/// 兼容旧示例主视图命名的类型别名。
pub type DesktopStudioApp = GalleryApp;

impl Default for GalleryApp {
    fn default() -> Self {
        Self::new()
    }
}

impl GalleryApp {
    /// 创建默认暗色主题的 Velox Base Gallery 应用实例。
    pub fn new() -> Self {
        let mut tree_state = TreeState::new();
        tree_state.toggle_expand(1);
        tree_state.toggle_expand(2);
        tree_state.select(3);

        let signals = GallerySignals::new(ThemeMode::Dark, Locale::EN_US, GalleryStory::Overview);
        let mut spring_anim = SpringAnimator::with_preset(MotionPreset::Snappy);
        spring_anim.jump_to(stories::motion::MOTION_CH_TARGET, 64.0);
        spring_anim.jump_to(stories::motion::MOTION_CH_PROGRESS, 64.0);
        spring_anim.jump_to(stories::motion::MOTION_CH_REVEAL, 1.0);
        let mut input_otp = OtpState::<6>::new();
        for ch in "4829".chars() {
            input_otp.push_char(ch);
        }

        Self {
            active_story: GalleryStory::Overview,
            sidebar_collapsed: false,
            theme_mode: ThemeMode::Dark,
            locale: Locale::EN_US,
            event_count: 0,
            signals,
            status_bar_view: None,
            overview_playground: None,
            async_task_view: None,
            last_synced_root_events: 0,
            cpu_load_pct: 42,
            frame_budget_pct: 65,
            switch_states: [true, true, false, true],
            slider_continuous: 64,
            slider_stepped: 40,
            button_clicks: 0,
            button_selected: true,
            button_loading: true,
            toggle_single: true,
            toggle_group_mask: 0b0101,
            cb_items: [true, true, false],
            radio_plan: 0,
            radio_density: 1,
            active_input_field: 0,
            text_input_name: EditableText::from_str("velox-base"),
            text_input_pass: EditableText::from_str("s3cr3t"),
            password_visible: false,
            number_val: 144,
            select_open: false,
            select_idx: Some(0),
            dropdown_open: false,
            dropdown_idx: 0,
            calendar_state: CalendarState::new(2026, 11)
                .with_today(SimpleDate::new(2026, 11, 23))
                .with_selection(DateSelection::Single(SimpleDate::new(2026, 11, 18))),
            tabs_segmented_idx: 0,
            tabs_underline_idx: 1,
            tabs_pill_idx: 0,
            accordion_mask: 0b001,
            stepper_active: 1,
            pagination_page: 2,
            editor_buf: SyntaxEditor::from_str(
                Metrics::new(13.0, 21.0),
                INITIAL_CODE_SAMPLE,
                RustSyntaxHighlighter,
                SyntaxTheme::dark(),
            ),
            editor_focused: true,
            input_ctx_menu: None,
            about_dialog_open: false,
            notification_open: false,
            demo_dialog_open: false,
            demo_alert_dialog_open: false,
            demo_modal_open: false,
            demo_sheet_open: false,
            last_overlay_result: "Ready — click any trigger card above",
            tooltip_open: true,
            popover_open: false,
            tag_visible_mask: 0b1111,
            selected_pkg_row: 1,
            table_sort_col: 0,
            table_sort_dir: SortDirection::Asc,
            list_selected: 0,
            tree_state,
            chart_dataset_b: false,
            chart_anim_epoch: 0,
            vsync_enabled: true,
            hidpi_enabled: true,
            damage_overlay_hint: true,
            fx_grad_angle: 135,
            fx_grad_space: GradientSpace::Oklab,
            fx_grad_extend: ExtendMode::Pad,
            fx_blend_idx: 0,
            fx_backdrop_blur: 10,
            fx_acrylic_enabled: true,
            fx_filter_preset: 0,
            fx_rotation_deg: 18,
            fx_clip_shape: 0,
            spring_anim,
            motion_last_ms: 0,
            motion_target_pct: 64.0,
            motion_reveal_open: true,
            motion_carousel_idx: 0,
            motion_roller_idx: 0,
            motion_accordion_mask: 0b01,
            motion_tab_idx: 1,
            lab_object_fit_idx: 1,
            lab_rtl_enabled: false,
            lab_radar_angle: 35,
            inspector_enabled: false,
            feedback_rating: 4,
            input_otp,
            input_color_picker: ColorPickerState::from_color(Color::rgb(59, 130, 246)),
            chat: stories::chat::ChatDemoState::new(),
            canvas_demo: stories::canvas::CanvasDemoState::new(),
        }
    }

    /// 指定明暗主题模式。
    pub fn with_theme_mode(mut self, mode: ThemeMode) -> Self {
        self.theme_mode = mode;
        self.signals.theme_mode.set_if_changed(mode);
        self
    }

    /// 指定初始界面语言区域。
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self.signals.locale.set_if_changed(locale);
        self
    }

    /// 指定初始打开的故事页（配合 `--story <name>`）。
    pub fn with_story(mut self, story: GalleryStory) -> Self {
        self.active_story = story;
        self.signals.active_story.set_if_changed(story);
        self
    }

    /// 读取当前实时 CPU 负载百分比（来自细粒度响应式信号，不建立额外订阅）。
    #[inline]
    pub fn current_cpu_load_pct(&self) -> u8 {
        self.signals.cpu_load_pct.get_untracked()
    }
}

impl View for GalleryApp {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        // 将根视图上同步的计数增量透传至状态栏信号（不调用 .get()，根视图保持零订阅）
        let delta = self.event_count.saturating_sub(self.last_synced_root_events);
        if delta > 0 {
            self.last_synced_root_events = self.event_count;
            self.signals
                .event_count
                .update(|n| *n = n.saturating_add(delta));
        }

        let theme = self.theme_mode.to_theme();
        cx.override_theme(theme);
        cx.override_locale(self.locale);
        cx.override_i18n_catalog(&DESKTOP_CATALOG);

        // 顶部工具栏回调
        let on_cycle_locale = cx.listener(|this, _: ClickEvent, _acx| {
            this.locale = next_demo_locale(this.locale);
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            batch(|| {
                this.signals.locale.set_if_changed(this.locale);
                this.signals
                    .event_count
                    .update(|n| *n = n.saturating_add(1));
            });
        });
        let on_trigger_toast = cx.listener(|this, _: ClickEvent, _acx| {
            this.notification_open = true;
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            this.signals
                .event_count
                .update(|n| *n = n.saturating_add(1));
        });
        let on_toggle_theme = cx.listener(|this, _: ClickEvent, _acx| {
            this.theme_mode = this.theme_mode.toggle();
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            batch(|| {
                this.signals.theme_mode.set_if_changed(this.theme_mode);
                this.signals
                    .event_count
                    .update(|n| *n = n.saturating_add(1));
            });
        });
        let on_open_about = cx.listener(|this, _: ClickEvent, _acx| {
            this.about_dialog_open = true;
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            this.signals
                .event_count
                .update(|n| *n = n.saturating_add(1));
        });
        let on_toggle_hud = cx.listener(|this, _: ClickEvent, _acx| {
            this.inspector_enabled = !this.inspector_enabled;
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            this.signals
                .event_count
                .update(|n| *n = n.saturating_add(1));
        });

        // 浮层关闭回调
        let raw_close_about = cx.action(|this, _acx| {
            this.about_dialog_open = false;
        });
        let on_close_about_btn = cx.listener(|this, _: ClickEvent, _acx| {
            this.about_dialog_open = false;
        });
        let raw_dismiss_notif = cx.action(|this, _acx| {
            this.notification_open = false;
        });
        let raw_close_demo_dialog = cx.action(|this, _acx| {
            this.demo_dialog_open = false;
            this.last_overlay_result = "Closed Standard Dialog (Close / Escape)";
        });
        let on_close_demo_dialog_btn = cx.listener(|this, _: ClickEvent, _acx| {
            this.demo_dialog_open = false;
            this.last_overlay_result = "Confirmed Standard Dialog";
        });
        let raw_confirm_alert = cx.action(|this, _acx| {
            this.demo_alert_dialog_open = false;
            this.last_overlay_result = "Confirmed Destructive AlertDialog";
        });
        let raw_cancel_alert = cx.action(|this, _acx| {
            this.demo_alert_dialog_open = false;
            this.last_overlay_result = "Cancelled AlertDialog (Cancel / Escape)";
        });
        let raw_close_modal = cx.action(|this, _acx| {
            this.demo_modal_open = false;
            this.last_overlay_result = "Closed Modal (Backdrop / Close / Escape)";
        });
        let on_close_modal_btn = cx.listener(|this, _: ClickEvent, _acx| {
            this.demo_modal_open = false;
            this.last_overlay_result = "Closed Modal via button";
        });
        let raw_close_sheet = cx.action(|this, _acx| {
            this.demo_sheet_open = false;
            this.last_overlay_result = "Dismissed Right Sheet (Close / Escape)";
        });

        // 侧边栏导航回调
        let on_select_story = cx.listener(|this, item_id: u32, _acx| {
            this.active_story = GalleryStory::from_id(item_id);
            this.select_open = false;
            this.dropdown_open = false;
            this.input_ctx_menu = None;
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            batch(|| {
                this.signals.active_story.set_if_changed(this.active_story);
                this.signals
                    .event_count
                    .update(|n| *n = n.saturating_add(1));
            });
        });
        let raw_toggle_sidebar = cx.action(|this, _acx| {
            this.sidebar_collapsed = !this.sidebar_collapsed;
            this.event_count = this.event_count.saturating_add(1);
            this.last_synced_root_events = this.event_count;
            this.signals
                .event_count
                .update(|n| *n = n.saturating_add(1));
        });

        // 1. 顶部标题工具栏 (h = 44)
        let theme_btn_label = if self.theme_mode.is_dark() {
            cx.t("toolbar.light_mode")
        } else {
            cx.t("toolbar.dark_mode")
        };
        let active_story_label = cx.t(self.active_story.i18n_key());

        let hud_btn = if self.inspector_enabled {
            Button::new(106, cx.t("toolbar.hud"))
                .info()
                .small()
                .w(54)
                .on_click(on_toggle_hud)
        } else {
            Button::new(106, cx.t("toolbar.hud"))
                .outline()
                .small()
                .w(54)
                .on_click(on_toggle_hud)
        };

        let toolbar = div()
            .id(100)
            .flex_row()
            .w_full()
            .h(44)
            .px(16)
            .items_center()
            .justify_between()
            .bg(theme.card)
            .border(1, theme.border)
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(10)
                    .items_center()
                    .child(cx, Svg::new(105, VELOX_LOGO_SVG).square(24))
                    .child(cx, text(cx.t("app.title")).size(15).color(theme.foreground))
                    .child(
                        cx,
                        div()
                            .px(8)
                            .py(2)
                            .rounded(6)
                            .bg(theme.secondary)
                            .border(1, theme.border)
                            .child(
                                cx,
                                text(active_story_label)
                                    .size(11)
                                    .color(theme.muted_foreground),
                            ),
                    )
                    .child(cx, hud_btn),
            )
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(8)
                    .items_center()
                    .child(
                        cx,
                        Button::new(104, self.locale.native_short_name())
                            .outline()
                            .small()
                            .w(64)
                            .on_click(on_cycle_locale),
                    )
                    .child(
                        cx,
                        Button::new(101, cx.t("toolbar.toast"))
                            .primary()
                            .small()
                            .w(84)
                            .on_click(on_trigger_toast),
                    )
                    .child(
                        cx,
                        Button::new(102, theme_btn_label)
                            .outline()
                            .small()
                            .w(96)
                            .on_click(on_toggle_theme),
                    )
                    .child(
                        cx,
                        Button::new(103, cx.t("toolbar.about"))
                            .outline()
                            .small()
                            .w(68)
                            .on_click(on_open_about),
                    ),
            );

        // 2. 左侧分组可折叠导航栏（14 个 Story 分 4 大组，直接使用 velox-component::Sidebar）
        let sec_actions = cx.arena().alloc_iter([
            SidebarItem::new(GalleryStory::Overview.id(), cx.t("story.overview").as_str(), "*")
                .icon_name(IconName::Info),
            SidebarItem::new(GalleryStory::Buttons.id(), cx.t("story.buttons").as_str(), "B")
                .icon_name(IconName::Star)
                .badge("8"),
            SidebarItem::new(GalleryStory::Selection.id(), cx.t("story.selection").as_str(), "C")
                .icon_name(IconName::Check),
            SidebarItem::new(GalleryStory::Sliders.id(), cx.t("story.sliders").as_str(), "S")
                .icon_name(IconName::Plus),
            SidebarItem::new(GalleryStory::Inputs.id(), cx.t("story.inputs").as_str(), "I")
                .icon_name(IconName::Search),
        ]);
        let sec_nav = cx.arena().alloc_iter([
            SidebarItem::new(GalleryStory::Navigation.id(), cx.t("story.nav").as_str(), "T")
                .icon_name(IconName::ChevronRight),
            SidebarItem::new(GalleryStory::Editor.id(), cx.t("story.editor").as_str(), "#")
                .icon_name(IconName::File),
        ]);
        let sec_feedback = cx.arena().alloc_iter([
            SidebarItem::new(GalleryStory::Overlays.id(), cx.t("story.overlays").as_str(), "D")
                .icon_name(IconName::Warning)
                .badge("4"),
            SidebarItem::new(GalleryStory::Feedback.id(), cx.t("story.feedback").as_str(), "A")
                .icon_name(IconName::Info),
        ]);
        let sec_data = cx.arena().alloc_iter([
            SidebarItem::new(GalleryStory::Data.id(), cx.t("story.data").as_str(), "L")
                .icon_name(IconName::Folder),
            SidebarItem::new(GalleryStory::SettingsIcons.id(), cx.t("story.icons").as_str(), "G")
                .icon_name(IconName::Settings)
                .badge("19"),
            SidebarItem::new(GalleryStory::Effects.id(), cx.t("story.effects").as_str(), "F")
                .icon_name(IconName::Star)
                .badge("FX"),
            SidebarItem::new(GalleryStory::Motion.id(), cx.t("story.motion").as_str(), "M")
                .icon_name(IconName::Star)
                .badge("SPR"),
            SidebarItem::new(GalleryStory::EngineLab.id(), cx.t("story.engine").as_str(), "E")
                .icon_name(IconName::Settings)
                .badge("PRO"),
            SidebarItem::new(GalleryStory::Chat.id(), cx.t("story.chat").as_str(), "C")
                .icon_name(IconName::Info)
                .badge("AI"),
            SidebarItem::new(GalleryStory::Canvas.id(), cx.t("story.canvas").as_str(), "M")
                .icon_name(IconName::Star)
                .badge("2D"),
        ]);

        let sidebar_sections = cx.arena().alloc_iter([
            SidebarSection::new(cx.t("grp.actions").as_str(), sec_actions),
            SidebarSection::new(cx.t("grp.nav").as_str(), sec_nav),
            SidebarSection::new(cx.t("grp.feedback").as_str(), sec_feedback),
            SidebarSection::new(cx.t("grp.data").as_str(), sec_data),
        ]);

        let sidebar_w = if self.sidebar_collapsed { 52 } else { 220 };
        let sidebar = Sidebar::new(200, cx.t("sidebar.title").as_str(), sidebar_sections)
            .size(220, 0)
            .collapsed(self.sidebar_collapsed)
            .active(Some(self.active_story.id()))
            .on_select(on_select_story)
            .on_toggle_collapse(raw_toggle_sidebar);

        let sidebar_panel = div()
            .id(250)
            .w(sidebar_w)
            .h_full()
            .bg(theme.sidebar)
            .border(1, theme.border)
            .scrollable(280)
            .overflow_hidden()
            .child(cx, sidebar);

        // 3. 右侧可滚动 Story 展示主画布
        let content_inner = match self.active_story {
            GalleryStory::Overview => self.render_story_overview(cx, theme),
            GalleryStory::Buttons => self.render_story_buttons(cx, theme),
            GalleryStory::Selection => self.render_story_selection(cx, theme),
            GalleryStory::Sliders => self.render_story_sliders(cx, theme),
            GalleryStory::Inputs => self.render_story_inputs(cx, theme),
            GalleryStory::Navigation => self.render_story_navigation(cx, theme),
            GalleryStory::Editor => self.render_story_editor(cx, theme),
            GalleryStory::Overlays => self.render_story_overlays(cx, theme),
            GalleryStory::Feedback => self.render_story_feedback(cx, theme),
            GalleryStory::Data => self.render_story_data(cx, theme),
            GalleryStory::SettingsIcons => self.render_story_settings_icons(cx, theme),
            GalleryStory::Effects => self.render_story_effects(cx, theme),
            GalleryStory::Motion => self.render_story_motion(cx, theme),
            GalleryStory::EngineLab => self.render_story_engine_lab(cx, theme),
            GalleryStory::Chat => self.render_story_chat(cx, theme),
            GalleryStory::Canvas => self.render_story_canvas(cx, theme),
        };

        let content_scroll_max = match self.active_story {
            GalleryStory::Effects | GalleryStory::Data | GalleryStory::Inputs => 760,
            GalleryStory::Motion
            | GalleryStory::EngineLab
            | GalleryStory::Feedback
            | GalleryStory::Chat
            | GalleryStory::Canvas => 620,
            _ => 420,
        };

        let body_row = div()
            .id(150)
            .flex_row()
            .w_full()
            .flex_grow(1)
            .overflow_hidden()
            .child(cx, sidebar_panel)
            .child(
                cx,
                div()
                    .id(160)
                    .flex_col()
                    .flex_grow(1)
                    .h_full()
                    .p(20)
                    .scrollable(content_scroll_max)
                    .overflow_hidden()
                    .child(cx, content_inner),
            );

        // 4. 底部全宽状态栏 (h = 26)：优先挂载独立子视图实现 L2 原地局部重排
        let status_bar_elem: Element<'a> = if let Some(status_handle) = self.status_bar_view {
            status_handle.into_element(cx)
        } else {
            let theme_name = if self.theme_mode.is_dark() {
                cx.t("theme.dark")
            } else {
                cx.t("theme.light")
            };
            let left_item = StatusBarItem::new(cx.t("status.left").as_str());
            let center_item = StatusBarItem::new(
                cx.t_args("status.center", &[("story", active_story_label.into())])
                    .as_str(),
            )
            .color(theme.primary);
            let right_item = StatusBarItem::new(
                cx.t_args(
                    "status.right",
                    &[
                        ("events", self.signals.event_count.get_untracked().into()),
                        ("theme", theme_name.into()),
                    ],
                )
                .as_str(),
            );
            div()
                .id(600)
                .flex_row()
                .w_full()
                .h(26)
                .px(12)
                .bg(theme.card)
                .border(1, theme.border)
                .items_center()
                .justify_between()
                .child(cx, text(left_item.label).size(11).color(theme.muted_foreground))
                .child(cx, text(center_item.label).size(11).color(theme.foreground))
                .child(cx, text(right_item.label).size(11).color(theme.muted_foreground))
                .into_element(cx)
        };

        let mut root = div()
            .id(1)
            .size_full()
            .flex_col()
            .bg(theme.background)
            .child(cx, toolbar)
            .child(cx, body_row)
            .child(cx, status_bar_elem);

        // 5. 顶层浮动通知层（直接使用 velox-component::Notification，自带 std 物理弹簧滑入动画）
        if self.notification_open {
            root = root.child(
                cx,
                div()
                    .id(850)
                    .pos(0, 52)
                    .w_full()
                    .overlay()
                    .flex_row()
                    .justify_end()
                    .pr(16)
                    .child(
                        cx,
                        Notification::new(
                            800,
                            cx.t("notif.title").as_str(),
                            cx.t("notif.body").as_str(),
                        )
                        .variant(NotificationVariant::Success)
                        .on_dismiss(raw_dismiss_notif),
                    ),
            );
        }

        // 6. Overlays Story 交互式演示浮层（直接使用 velox-component::{Dialog, AlertDialog, Modal, Sheet}，自带 std 弹簧/回弹入场动画）
        if self.demo_dialog_open {
            root = root.child(
                cx,
                div()
                    .id(910)
                    .pos(0, 0)
                    .size_full()
                    .overlay()
                    .occlude_hit()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .bg(Color::rgba(0, 0, 0, 145))
                    .child(
                        cx,
                        Dialog::new(911, "Edit Component Tokens", true)
                            .width(400)
                            .description("Themed velox-component Dialog with EaseOutBack spring entry, SVG close & Escape dismiss.")
                            .content(
                                cx,
                                text("Background hits are strictly occluded while this dialog is open.")
                                    .size(12)
                                    .color(theme.muted_foreground),
                            )
                            .on_close(raw_close_demo_dialog)
                            .footer(
                                cx,
                                Button::new(912, "Done")
                                    .primary()
                                    .small()
                                    .w(80)
                                    .on_click(on_close_demo_dialog_btn),
                            ),
                    ),
            );
        }

        if self.demo_alert_dialog_open {
            root = root.child(
                cx,
                div()
                    .id(920)
                    .pos(0, 0)
                    .size_full()
                    .overlay()
                    .occlude_hit()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .bg(Color::rgba(0, 0, 0, 145))
                    .child(
                        cx,
                        AlertDialog::new(
                            921,
                            "Delete Workspace Cache?",
                            "This action uses velox-component destructive styling & spring enter animation.",
                        )
                        .destructive(true)
                        .on_confirm(raw_confirm_alert)
                        .on_cancel(raw_cancel_alert),
                    ),
            );
        }

        if self.demo_modal_open {
            root = root.child(
                cx,
                Modal::new(930, "Centered Themed Modal")
                    .size(360, 170)
                    .on_close(raw_close_modal)
                    .content(
                        cx,
                        div()
                            .flex_col()
                            .gap(12)
                            .child(
                                cx,
                                text("Click the dark backdrop, click X, or press Escape to close.")
                                    .size(12)
                                    .color(theme.muted_foreground),
                            )
                            .child(
                                cx,
                                div().flex_row().justify_end().child(
                                    cx,
                                    Button::new(931, "Dismiss")
                                        .primary()
                                        .small()
                                        .on_click(on_close_modal_btn),
                                ),
                            ),
                    ),
            );
        }

        if self.demo_sheet_open {
            root = root.child(
                cx,
                div()
                    .id(940)
                    .pos(0, 0)
                    .size_full()
                    .overlay()
                    .occlude_hit()
                    .flex_row()
                    .justify_end()
                    .items_center()
                    .pr(12)
                    .bg(Color::rgba(0, 0, 0, 130))
                    .child(
                        cx,
                        Sheet::new(941, "Inspector Drawer", SheetSide::Right, true)
                            .size(300)
                            .on_dismiss(raw_close_sheet)
                            .content(
                                cx,
                                div()
                                    .flex_col()
                                    .gap(8)
                                    .child(
                                        cx,
                                        text("Directional Spring slide-in Sheet from velox-component.")
                                            .size(12)
                                            .color(theme.muted_foreground),
                                    )
                                    .child(
                                        cx,
                                        text("Supports Left / Right / Top / Bottom edges & Escape dismiss")
                                            .size(11)
                                            .color(theme.foreground),
                                    ),
                            ),
                    ),
            );
        }

        // 7. About 模态对话框遮罩层
        if self.about_dialog_open {
            root = root.child(
                cx,
                div()
                    .id(750)
                    .pos(0, 0)
                    .size_full()
                    .overlay()
                    .occlude_hit()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .bg(Color::rgba(0, 0, 0, 145))
                    .child(
                        cx,
                        Dialog::new(700, cx.t("about.title").as_str(), true)
                            .width(400)
                            .description(cx.t("about.desc").as_str())
                            .content(
                                cx,
                                text(cx.t("about.body"))
                                    .size(12)
                                    .color(theme.muted_foreground),
                            )
                            .on_close(raw_close_about)
                            .footer(
                                cx,
                                Button::new(705, cx.t("about.close"))
                                    .primary()
                                    .small()
                                    .w(84)
                                    .on_click(on_close_about_btn),
                            ),
                    ),
            );
        }

        // 7.5 内置 DevTools Inspector HUD 实时浮层条
        if self.inspector_enabled {
            root = root.child(
                cx,
                div()
                    .id(860)
                    .pos(236, 52)
                    .overlay()
                    .px(10)
                    .py(5)
                    .rounded(6)
                    .bg(Color::rgba(15, 23, 42, 235))
                    .border(1, theme.primary)
                    .flex_row()
                    .gap(12)
                    .items_center()
                    .child(
                        cx,
                        text(fmt!(
                            cx,
                            "DEVTOOLS HUD  |  Story: {:?}  |  Springs: {} active  |  Events: {}  |  Preset: {}",
                            self.active_story,
                            self.spring_anim.active_count(),
                            self.event_count,
                            self.spring_anim.preset().label()
                        ))
                        .size(11)
                        .color(Color::rgb(125, 211, 252)),
                    ),
            );
        }

        // 8. 输入框 / 代码编辑器全局右键上下文菜单（自由剪切 / 复制 / 粘贴 / 全选）
        if let Some((target, wx, wy)) = self.input_ctx_menu {
            let (has_sel, can_copy_cut, has_text) = match target {
                ContextMenuTarget::NameInput => (
                    self.text_input_name.selected_range().is_some(),
                    self.text_input_name.selected_range().is_some(),
                    !self.text_input_name.is_empty(),
                ),
                ContextMenuTarget::PasswordInput => (
                    self.text_input_pass.selected_range().is_some(),
                    false, // 密码掩码框安全禁止明文剪切/复制
                    !self.text_input_pass.is_empty(),
                ),
                ContextMenuTarget::CodeEditor => (
                    self.editor_buf.selection_bounds().is_some(),
                    self.editor_buf.selection_bounds().is_some(),
                    !self.editor_buf.is_empty(),
                ),
            };
            let _ = has_sel;
            let can_paste = clipboard_has_text();

            let ctx_items = cx.arena().alloc_iter([
                MenuItem::new("Cut", "Ctrl+X").disabled(!can_copy_cut),
                MenuItem::new("Copy", "Ctrl+C").disabled(!can_copy_cut),
                MenuItem::new("Paste", "Ctrl+V").disabled(!can_paste),
                MenuItem::separator(),
                MenuItem::new("Select All", "Ctrl+A").disabled(!has_text),
            ]);
            let on_ctx_dismiss = cx.action(|this, _acx| {
                this.input_ctx_menu = None;
            });
            let on_ctx_select = cx.listener(move |this, idx: usize, acx| {
                let now = acx.now_ms();
                match target {
                    ContextMenuTarget::NameInput => {
                        this.active_input_field = 0;
                        match idx {
                            0 => {
                                let _ = this.text_input_name.cut_to_clipboard(false, now);
                            }
                            1 => {
                                let _ = this.text_input_name.copy_to_clipboard(false);
                            }
                            2 => {
                                let _ = this.text_input_name.paste_from_clipboard(now);
                            }
                            4 => {
                                this.text_input_name.select_all();
                                this.text_input_name.reset_blink(now);
                            }
                            _ => {}
                        }
                    }
                    ContextMenuTarget::PasswordInput => {
                        this.active_input_field = 1;
                        match idx {
                            2 => {
                                let _ = this.text_input_pass.paste_from_clipboard(now);
                            }
                            4 => {
                                this.text_input_pass.select_all();
                                this.text_input_pass.reset_blink(now);
                            }
                            _ => {}
                        }
                    }
                    ContextMenuTarget::CodeEditor => {
                        this.editor_focused = true;
                        match idx {
                            0 => {
                                let _ = this.editor_buf.cut_to_clipboard();
                                this.editor_buf.reset_blink(now);
                            }
                            1 => {
                                let _ = this.editor_buf.copy_to_clipboard();
                            }
                            2 => {
                                let _ = this.editor_buf.paste_from_clipboard();
                                this.editor_buf.reset_blink(now);
                            }
                            4 => {
                                this.editor_buf.select_all();
                                this.editor_buf.reset_blink(now);
                            }
                            _ => {}
                        }
                    }
                }
                this.input_ctx_menu = None;
                this.event_count = this.event_count.saturating_add(1);
            });

            let mx = wx.clamp(6, DEFAULT_DESKTOP_WIDTH - 190);
            let my = wy.clamp(6, DEFAULT_DESKTOP_HEIGHT - 165);
            root = root.child(
                cx,
                ContextMenu::new(980, mx, my, true, ctx_items)
                    .width(178)
                    .on_select(on_ctx_select)
                    .on_dismiss(on_ctx_dismiss),
            );
        }

        root.into_element(cx)
    }
}

/// 将配置完成的 [`GalleryApp`] 及其独立隔离边界子视图（[`StatusBarView`]、[`OverviewPlaygroundView`]、[`AsyncTaskView`]）挂载到引擎。
pub fn mount_desktop_app(engine: &mut VeloxEngine, mut app: GalleryApp) -> Handle<GalleryApp> {
    let status_bar = engine.insert_view(StatusBarView::new(app.signals));
    let overview_playground = engine.insert_view(OverviewPlaygroundView::new(app.signals));
    let async_task_view = engine.insert_view(
        AsyncTaskView::new(app.signals).with_scheduler_label(engine.scheduler_name()),
    );
    app.status_bar_view = Some(status_bar);
    app.overview_playground = Some(overview_playground);
    app.async_task_view = Some(async_task_view);
    engine.mount_root(app)
}

/// 创建已接入 [`TokioScheduler`] 并挂载 [`GalleryApp`] 与细粒度响应子视图的 [`VeloxEngine`] 实例与根视图句柄。
pub fn create_desktop_app(width: i16, height: i16) -> (VeloxEngine, Handle<GalleryApp>) {
    let mut engine = VeloxEngine::new(width, height).with_scheduler(TokioScheduler::global());
    let handle = mount_desktop_app(&mut engine, GalleryApp::new());
    (engine, handle)
}
