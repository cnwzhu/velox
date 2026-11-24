//! # Velox Kit (`velox-kit`)
//!
//! **The one crate applications depend on.**
//!
//! 统一应用层入口库，默认开箱即用（`default = ["gpu", "winit", "component"]`）：
//! - [`velox_engine`] (`velox_kit::core`)：核心渲染引擎、Flex/Grid 布局、SDF 抗锯齿图元、内置软件光栅器、默认 `gpu`（自研轻量级 `wgpu` 2D SDF）硬件加速与跨平台后端（`std` 下全部可用，`winit` 启用原生桌面窗口）；
//! - [`velox_base`] (`velox_kit::base`)：默认无样式（Headless / Unstyled）组件交互行为原语、文本与代码编辑引擎、状态机与布局基础结构（通过 `base` feature 启用，默认随 `component` 开启）；
//! - [`velox_component`] (`velox_kit::component`)：建立在 `velox-base` 之上的完整 Shadcn Zinc + Blue 主题化视觉组件库（通过 `component` feature 启用，**默认开启**）。
//!
//! ## 模块与顶层导出规则
//! - `velox_kit::base::*` 与 `Base*` 别名：**永远**指向无样式 `velox_base::*`（免疫 Cargo feature unification）；
//! - `velox_kit::component::*` 与 `Themed*` 别名：指向主题化 `velox_component::*`；
//! - 顶层组件名与 `velox_kit::prelude::*`：启用 `component` feature 时指向主题化 `velox_component::*`，仅启用 `base` feature 时指向无样式 `velox_base::*`。


pub use velox_engine as core;

#[cfg(feature = "base")]
pub use velox_base as base;

#[cfg(feature = "component")]
pub use velox_component as component;

#[cfg(feature = "base")]
pub use velox_base::{
    clip_offset_left, clip_offset_right, input, line_range_at, next_word_boundary,
    prev_word_boundary, split_lines_with_ending, step_value, widget_t, word_range_at,
    AccordionItem, AccordionItemState, Action, Affinity, AlertVariant, Align, AttachmentKind,
    Attrs, AttrsList, AttrsSpan, AutoClosedPairs, AvatarStatus, BubbleAlign, BubbleReaction,
    Buffer, BufferLine, BufferPoint, CalendarItemKind, CalendarItemState, CalendarState,
    CalendarViewMode, CandlestickBar, Change, ChangeItem, CharacterKind, ChartKind, CheckboxState,
    CollapsibleState, ColorPickerState, ColorSwatchState, ComboboxItemState, CommandGroup,
    CommandItem, CommandItemState, ComponentSize, ComponentVariant, CosmicEditor, Cursor,
    CursorSelection, DateSelection, DescriptionItem, DiagnosticSeverity, DiagnosticSpan, DiffLine,
    DiffLineKind, DiffViewMode, Disableable, DockPanel, DockWorkspaceState, DropdownItemState,
    Edit, EditSelection, EditableText, FontStyle, FontWeight, FormLayout, GridTile, HighlightKind,
    HighlightSpan, HourCycle, IconName, ImageFit, KeyAction, KeyboardMode, LayoutCursor,
    LayoutGlyph, LayoutLine, LayoutRun, LayoutRunIter, LineEnding, LineEndingIter, ListItem,
    ListItemState, MarkerVariant, MaskPattern, MaskToken, MenuItem, MenuItemState, MessageRole,
    MessageScrollerState, Metrics, Motion, Navigator, NotificationVariant, NumberStep,
    OtpSlotState, OtpState, PageTransition, PaginationItemKind, PaginationItemState,
    PlainSyntaxHighlighter, QuestionItem, QuestionKind, QuestionnaireState, RadioItemState,
    RadioState, RatingItemState, RustSyntaxHighlighter, Scroll, ScrollbarAxis, SearchMatcher,
    SearchSession, SelectItemState, SelectOption, Selection, Selections, SettingsGroup,
    SettingsItem, SettingsValue, SheetSide, SidebarItem, SidebarItemState, SidebarSection,
    SimpleDate, Sizable, SkeletonShape, SliderState, SortDirection, SpeechState, SpeechStatus,
    SplitAxis, StatusBarItem, StepItem, StepStatus, StepperItemState, SwitchLabelSide, SwitchState,
    SyntaxEditor, SyntaxHighlighter, SyntaxTheme, TabBarItemState, TabItemState, TabSize,
    TableCellState, TableColumn, TableHeaderState, TabsVariant, TagVariant, TextBuffer,
    TextDecoration, TextDocBuffer, TextSnapshot, TextViewFormat, Theme, ThemeMode, TimeFieldState,
    TimePrecision, TimeSegment, TimeSegmentState, ToggleItemState, ToolbarGroup, TransitionLayer,
    TreeItemState, TreeNode, TreeState, UndoHistory, Variants, Wrap, BUILTIN_WIDGET_CATALOG,
    MASK_CHAR,
};

/// 始终可用的无样式基础组件类型别名（开启 `component` 时也不会改变实际类型）。
#[cfg(feature = "base")]
pub use velox_base::{
    Accordion as BaseAccordion, Alert as BaseAlert, AlertDialog as BaseAlertDialog,
    ArcGauge as BaseArcGauge, Attachment as BaseAttachment, Avatar as BaseAvatar,
    Badge as BaseBadge, BottomSheet as BaseBottomSheet, Bubble as BaseBubble, Button as BaseButton,
    Calendar as BaseCalendar, Card as BaseCard, Carousel as BaseCarousel, Chart as BaseChart,
    Checkbox as BaseCheckbox, CodeEditor as BaseCodeEditor, Collapsible as BaseCollapsible,
    ColorPicker as BaseColorPicker, ColorSwatch as BaseColorSwatch, Combobox as BaseCombobox,
    Command as BaseCommand, ContextMenu as BaseContextMenu, DataTable as BaseDataTable,
    DatePicker as BaseDatePicker, DescriptionList as BaseDescriptionList, Dialog as BaseDialog,
    Diff as BaseDiff, Dock as BaseDock, Dropdown as BaseDropdown,
    DropdownButton as BaseDropdownButton, EditableText as BaseEditableText, Editor as BaseEditor,
    Form as BaseForm, FormField as BaseFormField, GridLayout as BaseGridLayout,
    GroupBox as BaseGroupBox, HoverCard as BaseHoverCard, Icon as BaseIcon, Image as BaseImage,
    Input as BaseInput, Kbd as BaseKbd, Label as BaseLabel, Link as BaseLink, List as BaseList,
    MarkdownView as BaseMarkdownView, Marker as BaseMarker, Menu as BaseMenu,
    Message as BaseMessage, MessageScroller as BaseMessageScroller, Modal as BaseModal,
    NavBar as BaseNavBar, Notification as BaseNotification, NumberInput as BaseNumberInput,
    OtpInput as BaseOtpInput, Pagination as BasePagination, Popover as BasePopover,
    Progress as BaseProgress, ProgressBar as BaseProgressBar, Questionnaire as BaseQuestionnaire,
    Radio as BaseRadio, RadioGroup as BaseRadioGroup, Rating as BaseRating,
    Resizable as BaseResizable, ResizableSplit as BaseResizableSplit, Roller as BaseRoller,
    Root as BaseRoot, Scrollable as BaseScrollable, Scrollbar as BaseScrollbar,
    Select as BaseSelect, SelectableText as BaseSelectableText, Settings as BaseSettings,
    Sheet as BaseSheet, Sidebar as BaseSidebar, Skeleton as BaseSkeleton, Slider as BaseSlider,
    Speech as BaseSpeech, Spinner as BaseSpinner, StatusBar as BaseStatusBar,
    Stepper as BaseStepper, Svg as BaseSvg, Switch as BaseSwitch, TabBar as BaseTabBar,
    Table as BaseTable, Tabs as BaseTabs, Tag as BaseTag, TextInput as BaseTextInput,
    TextView as BaseTextView, Textarea as BaseTextarea, TimeField as BaseTimeField,
    Toast as BaseToast, Toggle as BaseToggle, ToggleGroup as BaseToggleGroup,
    Toolbar as BaseToolbar, Tooltip as BaseTooltip, Tree as BaseTree,
    UniformList as BaseUniformList, VirtualKeyboard as BaseVirtualKeyboard,
    VirtualList as BaseVirtualList,
};

/// 始终可用的主题化视觉组件类型别名。
#[cfg(feature = "component")]
pub use velox_component::{
    Accordion as ThemedAccordion, Alert as ThemedAlert, AlertDialog as ThemedAlertDialog,
    ArcGauge as ThemedArcGauge, Attachment as ThemedAttachment, Avatar as ThemedAvatar,
    Badge as ThemedBadge, BottomSheet as ThemedBottomSheet, Bubble as ThemedBubble,
    Button as ThemedButton, Calendar as ThemedCalendar, Card as ThemedCard,
    Carousel as ThemedCarousel, Chart as ThemedChart, Checkbox as ThemedCheckbox,
    CodeEditor as ThemedCodeEditor, Collapsible as ThemedCollapsible,
    ColorPicker as ThemedColorPicker, ColorSwatch as ThemedColorSwatch, Combobox as ThemedCombobox,
    Command as ThemedCommand, ContextMenu as ThemedContextMenu, DataTable as ThemedDataTable,
    DatePicker as ThemedDatePicker, DescriptionList as ThemedDescriptionList,
    Dialog as ThemedDialog, Diff as ThemedDiff, Dock as ThemedDock, Dropdown as ThemedDropdown,
    DropdownButton as ThemedDropdownButton, EditableText as ThemedEditableText,
    Editor as ThemedEditor, Form as ThemedForm, FormField as ThemedFormField,
    GridLayout as ThemedGridLayout, GroupBox as ThemedGroupBox, HoverCard as ThemedHoverCard,
    Icon as ThemedIcon, Image as ThemedImage, Input as ThemedInput, Kbd as ThemedKbd,
    Label as ThemedLabel, Link as ThemedLink, List as ThemedList,
    MarkdownView as ThemedMarkdownView, Marker as ThemedMarker, Menu as ThemedMenu,
    Message as ThemedMessage, MessageScroller as ThemedMessageScroller, Modal as ThemedModal,
    NavBar as ThemedNavBar, Notification as ThemedNotification, NumberInput as ThemedNumberInput,
    OtpInput as ThemedOtpInput, Pagination as ThemedPagination, Popover as ThemedPopover,
    Progress as ThemedProgress, ProgressBar as ThemedProgressBar,
    Questionnaire as ThemedQuestionnaire, Radio as ThemedRadio, RadioGroup as ThemedRadioGroup,
    Rating as ThemedRating, Resizable as ThemedResizable, ResizableSplit as ThemedResizableSplit,
    Roller as ThemedRoller, Root as ThemedRoot, Scrollable as ThemedScrollable,
    Scrollbar as ThemedScrollbar, Select as ThemedSelect, SelectableText as ThemedSelectableText,
    Settings as ThemedSettings, Sheet as ThemedSheet, Sidebar as ThemedSidebar,
    Skeleton as ThemedSkeleton, Slider as ThemedSlider, Speech as ThemedSpeech,
    Spinner as ThemedSpinner, StatusBar as ThemedStatusBar, Stepper as ThemedStepper,
    Svg as ThemedSvg, Switch as ThemedSwitch, TabBar as ThemedTabBar, Table as ThemedTable,
    Tabs as ThemedTabs, Tag as ThemedTag, TextInput as ThemedTextInput, TextView as ThemedTextView,
    Textarea as ThemedTextarea, TimeField as ThemedTimeField, Toast as ThemedToast,
    Toggle as ThemedToggle, ToggleGroup as ThemedToggleGroup, Toolbar as ThemedToolbar,
    Tooltip as ThemedTooltip, Tree as ThemedTree, UniformList as ThemedUniformList,
    VirtualKeyboard as ThemedVirtualKeyboard, VirtualList as ThemedVirtualList,
};

#[cfg(feature = "component")]
pub use velox_component::{
    uniform_list, virtual_list, Accordion, Alert, AlertDialog, ArcGauge, Attachment, Avatar, Badge,
    BottomSheet, Bubble, Button, Calendar, Card, Carousel, Chart, Checkbox, CodeEditor,
    Collapsible, ColorPicker, ColorSwatch, Combobox, Command, ContextMenu, DataTable, DatePicker,
    DescriptionList, Dialog, Diff, Dock, Dropdown, DropdownButton, Editor, Form, FormField,
    GridLayout, GroupBox, HoverCard, Icon, Image, Input, Kbd, Label, Link, List, MarkdownView,
    Marker, Menu, Message, MessageScroller, Modal, NavBar, Notification, NumberInput, OtpInput,
    Pagination, Popover, Progress, ProgressBar, Questionnaire, Radio, RadioGroup, Rating,
    Resizable, ResizableSplit, Roller, Root, Scrollable, Scrollbar, Select, SelectableText,
    Settings, Sheet, Sidebar, Skeleton, Slider, Speech, Spinner, StatusBar, Stepper, Svg, Switch,
    TabBar, Table, Tabs, Tag, TailwindStyled, TextInput, TextView, Textarea, TimeField, Toast,
    Toggle, ToggleGroup, Toolbar, Tooltip, Tree, UniformList, VirtualKeyboard, VirtualList,
};

#[cfg(all(feature = "base", not(feature = "component")))]
pub use velox_base::{
    uniform_list, virtual_list, Accordion, Alert, AlertDialog, ArcGauge, Attachment, Avatar, Badge,
    BottomSheet, Bubble, Button, Calendar, Card, Carousel, Chart, Checkbox, CodeEditor,
    Collapsible, ColorPicker, ColorSwatch, Combobox, Command, ContextMenu, DataTable, DatePicker,
    DescriptionList, Dialog, Diff, Dock, Dropdown, DropdownButton, Editor, Form, FormField,
    GridLayout, GroupBox, HoverCard, Icon, Image, Input, Kbd, Label, Link, List, MarkdownView,
    Marker, Menu, Message, MessageScroller, Modal, NavBar, Notification, NumberInput, OtpInput,
    Pagination, Popover, Progress, ProgressBar, Questionnaire, Radio, RadioGroup, Rating,
    Resizable, ResizableSplit, Roller, Root, Scrollable, Scrollbar, Select, SelectableText,
    Settings, Sheet, Sidebar, Skeleton, Slider, Speech, Spinner, StatusBar, Stepper, Svg, Switch,
    TabBar, Table, Tabs, Tag, TextInput, TextView, Textarea, TimeField, Toast, Toggle, ToggleGroup,
    Toolbar, Tooltip, Tree, UniformList, VirtualKeyboard, VirtualList,
};

pub use velox_engine::prelude::*;

pub use velox_engine::app;
pub use velox_engine::arena;
pub use velox_engine::async_rt;
pub use velox_engine::damage;
pub use velox_engine::display;
pub use velox_engine::element;
pub use velox_engine::engine;
pub use velox_engine::motion;
pub use velox_engine::paint;
pub use velox_engine::platform;
pub use velox_engine::pod;
pub use velox_engine::raster;
pub use velox_engine::renderers;
pub use velox_engine::sdf;
pub use velox_engine::signal;
pub use velox_engine::solver;
pub use velox_engine::style;

/// 独立可插拔异步调度器适配层聚合命名空间（按 feature 开关启用 `tokio` / `smol`）。
pub mod scheduler {
    pub use velox_engine::async_rt::*;

    #[cfg(feature = "tokio")]
    pub use velox_async::tokio;

    #[cfg(feature = "smol")]
    pub use velox_async::smol;

}

#[cfg(feature = "tokio")]
pub use velox_async::{TokioScheduler, VeloxEngineTokioExt, VeloxTokioExt};

#[cfg(feature = "smol")]
pub use velox_async::{SmolScheduler, VeloxEngineSmolExt, VeloxSmolExt};


pub use velox_engine::{export_android_app, export_harmony_app, export_ios_app, export_web_app};

/// 应用开发全量预导入模块（`use velox_kit::prelude::*;`）。
///
/// - 启用 `component` feature（默认）时，组件名指向主题化 `velox_component::*`；
/// - 仅启用 `base` feature 时，组件名指向无样式 `velox_base::*`；
/// - 启用 `tokio` / `smol` feature 时自动导入对应调度器适配与链式扩展。
pub mod prelude {
    #[cfg(feature = "component")]
    pub use velox_component::prelude::*;

    #[cfg(all(feature = "base", not(feature = "component")))]
    pub use velox_base::*;

    #[cfg(feature = "base")]
    pub use velox_base::{widget_t, BUILTIN_WIDGET_CATALOG};

    #[cfg(feature = "tokio")]
    pub use velox_async::{TokioScheduler, VeloxEngineTokioExt, VeloxTokioExt};

    #[cfg(feature = "smol")]
    pub use velox_async::{SmolScheduler, VeloxEngineSmolExt, VeloxSmolExt};


    pub use velox_engine::prelude::*;
}
