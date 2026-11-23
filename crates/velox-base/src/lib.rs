//! # Velox Base (`velox-base`)
//!
//! 建立在 `velox-engine` 之上的无样式（Headless / Unstyled）基础组件行为原语库，
//! 参照 `gpui-base` 分层理念：**默认不附带任何主题背景色、边框、圆角、阴影、悬停渐变、聚焦蓝框或预设语义变体色**，
//! 专注交互行为、焦点与键盘导航、受控状态机与极简结构骨架；完整主题视觉皮肤统一由 `velox-component` 提供。
//! - [`components`]：70+ 个跨端与移动端无样式基础、表单、布局与高级行为原语；
//! - [`input`]：完整文本与代码编辑引擎（多行 [`TextBuffer`]、行列光标 [`BufferPoint`]、多光标选区 [`Selections`]、分组撤销/重做栈 [`UndoHistory`]、掩码格式化器 [`MaskPattern`]、括号自动闭合 [`AutoClosedPairs`]、文本搜索与替换会话 [`SearchSession`]、单词/行边界分词器、[`Textarea`] 与带行号/诊断/语法高亮/缩进导引线的 [`CodeEditor`]）；
//! - [`traits`]：统一组件修饰符特征（[`Sizable`]、[`Variants`]、[`Disableable`]）与尺寸/变体枚举；
//! - [`navigator`]：零堆分配页面路由栈与转场状态机（[`Navigator`]、[`PageTransition`]）；
//! - [`list`]：恒定 `ScratchArena` 内存开销的无样式等高虚拟滚动列表原语（[`UniformList`]、[`VirtualList`]、[`uniform_list`]、[`virtual_list`]）。


pub mod components;
pub mod i18n;
pub mod input;
pub mod list;
pub mod macros;
pub mod navigator;
pub mod traits;

#[doc(hidden)]
pub mod __private {
    pub use velox_engine::style::{Style, StyleExt, Styled, StyledExt};
}

/// 复合组件内部子元素确定性无撞车 ID 派生（映射到 `0x8000_0000..=0xFFFF_FFFF` 独立命名空间，
/// 彻底杜绝线性偏移写法与用户显式 ID 或相邻组件发生 ID 碰撞）。
#[inline(always)]
pub(crate) const fn scoped_child_id(parent_id: u32, salt: u32, index: u32) -> u32 {
    velox_engine::pod::derive_scoped_id(parent_id, salt, index)
}

pub use components::{
    step_value, Accordion, AccordionItem, AccordionItemState, Alert, AlertDialog, AlertVariant,
    ArcGauge, Attachment, AttachmentKind, Avatar, AvatarStatus, Badge, BottomSheet, Bubble,
    BubbleAlign, BubbleReaction, Button, Calendar, CalendarItemKind, CalendarItemState,
    CalendarState, CalendarViewMode, CandlestickBar, Card, Carousel, Chart, ChartKind, Checkbox,
    CheckboxState, Collapsible, CollapsibleState, ColorPicker, ColorPickerState, ColorSwatch,
    ColorSwatchState, Combobox, ComboboxItemState, Command, CommandGroup, CommandItem,
    CommandItemState, ContextMenu, DataTable, DatePicker, DateSelection, DescriptionItem,
    DescriptionList, Dialog, Diff, DiffLine, DiffLineKind, DiffViewMode, Dock, DockPanel,
    DockWorkspaceState, Dropdown, DropdownButton, DropdownItemState, EditableText, Form, FormField,
    FormLayout, GridLayout, GridTile, GroupBox, HoverCard, HourCycle, Icon, IconName, Image,
    ImageFit, Input, Kbd, KeyAction, KeyboardMode, Label, Link, List, ListItem, ListItemState,
    MarkdownView, Marker, MarkerVariant, Menu, MenuItem, MenuItemState, Message, MessageRole,
    MessageScroller, MessageScrollerState, Modal, NavBar, Notification, NotificationVariant,
    NumberInput, NumberStep, OtpInput, OtpSlotState, OtpState, Pagination, PaginationItemKind,
    PaginationItemState, Popover, Progress, ProgressBar, QuestionItem, QuestionKind, Questionnaire,
    QuestionnaireState, Radio, RadioGroup, RadioItemState, RadioState, Rating, RatingItemState,
    Resizable, ResizableSplit, Roller, Root, Scrollable, Scrollbar, ScrollbarAxis, Select,
    SelectItemState, SelectOption, SelectableText, Settings, SettingsGroup, SettingsItem,
    SettingsValue, Sheet, SheetSide, Sidebar, SidebarItem, SidebarItemState, SidebarSection,
    SimpleDate, Skeleton, SkeletonShape, Slider, SliderState, SortDirection, Speech, SpeechState,
    SpeechStatus, Spinner, SplitAxis, StatusBar, StatusBarItem, StepItem, StepStatus, Stepper,
    StepperItemState, Svg, Switch, SwitchLabelSide, SwitchState, TabBar, TabBarItemState,
    TabItemState, Table, TableCellState, TableColumn, TableHeaderState, Tabs, TabsVariant, Tag,
    TagVariant, TextInput, TextView, TextViewFormat, Theme, ThemeMode, TimeField, TimeFieldState,
    TimePrecision, TimeSegment, TimeSegmentState, Toast, Toggle, ToggleGroup, ToggleItemState,
    Toolbar, ToolbarGroup, Tooltip, Tree, TreeItemState, TreeNode, TreeState, VirtualKeyboard,
};
pub use i18n::{widget_t, BUILTIN_WIDGET_CATALOG};
pub use input::{
    clip_offset_left, clip_offset_right, line_range_at, next_word_boundary, prev_word_boundary,
    split_lines_with_ending, word_range_at, Action, Affinity, Align, Attrs, AttrsList, AttrsSpan,
    AutoClosedPairs, Buffer, BufferLine, BufferPoint, Change, ChangeItem, CharacterKind,
    CodeEditor, CosmicEditor, Cursor, CursorSelection, DiagnosticSeverity, DiagnosticSpan, Edit,
    EditSelection, Editor, FontStyle, FontWeight, HighlightKind, HighlightSpan, LayoutCursor,
    LayoutGlyph, LayoutLine, LayoutRun, LayoutRunIter, LineEnding, LineEndingIter, MaskPattern,
    MaskToken, Metrics, Motion, PlainSyntaxHighlighter, RustSyntaxHighlighter, Scroll,
    SearchMatcher, SearchSession, Selection, Selections, SyntaxEditor, SyntaxHighlighter,
    SyntaxTheme, TabSize, TextBuffer, TextDecoration, TextDocBuffer, TextSnapshot, Textarea,
    UndoHistory, Wrap, MASK_CHAR,
};
pub use list::{uniform_list, virtual_list, UniformList, VirtualList};
pub use navigator::{Navigator, PageTransition, TransitionLayer};
pub use traits::{ComponentSize, ComponentVariant, Disableable, Sizable, Variants};
