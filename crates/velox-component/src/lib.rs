//! # Velox Component (`velox-component`)
//!
//! 建立在 [`velox_engine`] 与 [`velox_base`] 之上的 Tailwind 风格主题化组件样式库：
//! - 通过 [`define_themed_component!`] 宏为底层 [`velox_base`] 原语生成独立的主题化包装组件；
//! - 通过 [`styled::TailwindStyled`] 提供类似 Tailwind CSS 的链式主题样式预设，自动回退 `cx.theme()` 语义配色，同时完整保留 `Styled` 显式定制优先级。


pub mod components;
pub mod macros;
pub mod styled;
pub mod theme;

pub use components::{
    step_value, uniform_list, virtual_list, Accordion, AccordionItem, AccordionItemState, Alert,
    AlertDialog, AlertVariant, ArcGauge, Attachment, AttachmentKind, Avatar, AvatarStatus, Badge,
    BottomSheet, Bubble, BubbleAlign, BubbleReaction, Button, Calendar, CalendarItemKind,
    CalendarItemState, CalendarState, CalendarViewMode, CandlestickBar, Card, Carousel, Chart,
    ChartKind, Checkbox, CheckboxState, CodeEditor, Collapsible, CollapsibleState, ColorPicker,
    ColorPickerState, ColorSwatch, ColorSwatchState, Combobox, ComboboxItemState, Command,
    CommandGroup, CommandItem, CommandItemState, ContextMenu, DataTable, DatePicker, DateSelection,
    DescriptionItem, DescriptionList, DiagnosticSeverity, DiagnosticSpan, Dialog, Diff, DiffLine,
    DiffLineKind, DiffViewMode, Dock, DockPanel, DockWorkspaceState, Dropdown, DropdownButton,
    DropdownItemState, EditableText, Editor, Form, FormField, FormLayout, GridLayout, GridTile,
    GroupBox, HighlightKind, HighlightSpan, HoverCard, HourCycle, Icon, IconName, Image, ImageFit,
    Input, Kbd, KeyAction, KeyboardMode, Label, Link, List, ListItem, ListItemState, MarkdownView,
    Marker, MarkerVariant, Menu, MenuItem, MenuItemState, Message, MessageRole, MessageScroller,
    MessageScrollerState, Modal, NavBar, Notification, NotificationVariant, NumberInput,
    NumberStep, OtpInput, OtpSlotState, OtpState, Pagination, PaginationItemKind,
    PaginationItemState, Popover, Progress, ProgressBar, QuestionItem, QuestionKind, Questionnaire,
    QuestionnaireState, Radio, RadioGroup, RadioItemState, RadioState, Rating, RatingItemState,
    Resizable, ResizableSplit, Roller, Root, Scrollable, Scrollbar, ScrollbarAxis, Select,
    SelectItemState, SelectOption, SelectableText, Settings, SettingsGroup, SettingsItem,
    SettingsValue, Sheet, SheetSide, Sidebar, SidebarItem, SidebarItemState, SidebarSection,
    SimpleDate, Skeleton, SkeletonShape, Slider, SliderState, SortDirection, Speech, SpeechState,
    SpeechStatus, Spinner, SplitAxis, StatusBar, StatusBarItem, StepItem, StepStatus, Stepper,
    StepperItemState, SurfaceCanvas, Svg, Switch, SwitchLabelSide, SwitchState, TabBar, TabBarItemState,
    TabItemState, Table, TableCellState, TableColumn, TableHeaderState, Tabs, TabsVariant, Tag,
    TagVariant, TextInput, TextView, TextViewFormat, Textarea, ThemeMode, TimeField,
    TimeFieldState, TimePrecision, TimeSegment, TimeSegmentState, Toast, Toggle, ToggleGroup,
    ToggleItemState, Toolbar, ToolbarGroup, Tooltip, Tree, TreeItemState, TreeNode, TreeState,
    UniformList, VirtualKeyboard, VirtualList,
};
pub use styled::TailwindStyled;
pub use theme::Theme;
pub use velox_base::{
    clip_offset_left, clip_offset_right, input, line_range_at, next_word_boundary,
    prev_word_boundary, split_lines_with_ending, word_range_at, Action, Affinity, Align, Attrs,
    AttrsList, AttrsSpan, AutoClosedPairs, Buffer, BufferLine, BufferPoint, Change, ChangeItem,
    CharacterKind, ComponentSize, ComponentVariant, CosmicEditor, Cursor, CursorSelection,
    Disableable, Edit, EditSelection, FontStyle, FontWeight, LayoutCursor, LayoutGlyph, LayoutLine,
    LayoutRun, LayoutRunIter, LineEnding, LineEndingIter, MaskPattern, MaskToken, Metrics, Motion,
    Navigator, PageTransition, PlainSyntaxHighlighter, RustSyntaxHighlighter, Scroll,
    SearchMatcher, SearchSession, Selection, Selections, Sizable, SyntaxEditor, SyntaxHighlighter,
    SyntaxTheme, TabSize, TextBuffer, TextDecoration, TextDocBuffer, TextSnapshot, TransitionLayer,
    UndoHistory, Variants, Wrap, MASK_CHAR,
};

/// 预导出全套主题化组件、Tailwind 样式扩展、编辑器组件与修饰符。
pub mod prelude {
    pub use crate::components::{
        step_value, uniform_list, virtual_list, Accordion, AccordionItem, AccordionItemState,
        Alert, AlertDialog, AlertVariant, ArcGauge, Attachment, AttachmentKind, Avatar,
        AvatarStatus, Badge, BottomSheet, Bubble, BubbleAlign, BubbleReaction, Button, Calendar,
        CalendarItemKind, CalendarItemState, CalendarState, CalendarViewMode, CandlestickBar, Card,
        Carousel, Chart, ChartKind, Checkbox, CheckboxState, CodeEditor, Collapsible,
        CollapsibleState, ColorPicker, ColorPickerState, ColorSwatch, ColorSwatchState, Combobox,
        ComboboxItemState, Command, CommandGroup, CommandItem, CommandItemState, ContextMenu,
        DataTable, DatePicker, DateSelection, DescriptionItem, DescriptionList, DiagnosticSeverity,
        DiagnosticSpan, Dialog, Diff, DiffLine, DiffLineKind, DiffViewMode, Dock, DockPanel,
        DockWorkspaceState, Dropdown, DropdownButton, DropdownItemState, EditableText, Editor,
        Form, FormField, FormLayout, GridLayout, GridTile, GroupBox, HighlightKind, HighlightSpan,
        HoverCard, HourCycle, Icon, IconName, Image, ImageFit, Input, Kbd, KeyAction, KeyboardMode,
        Label, Link, List, ListItem, ListItemState, MarkdownView, Marker, MarkerVariant, Menu,
        MenuItem, MenuItemState, Message, MessageRole, MessageScroller, MessageScrollerState,
        Modal, NavBar, Notification, NotificationVariant, NumberInput, NumberStep, OtpInput,
        OtpSlotState, OtpState, Pagination, PaginationItemKind, PaginationItemState, Popover,
        Progress, ProgressBar, QuestionItem, QuestionKind, Questionnaire, QuestionnaireState,
        Radio, RadioGroup, RadioItemState, RadioState, Rating, RatingItemState, Resizable,
        ResizableSplit, Roller, Root, Scrollable, Scrollbar, ScrollbarAxis, Select,
        SelectItemState, SelectOption, SelectableText, Settings, SettingsGroup, SettingsItem,
        SettingsValue, Sheet, SheetSide, Sidebar, SidebarItem, SidebarItemState, SidebarSection,
        SimpleDate, Skeleton, SkeletonShape, Slider, SliderState, SortDirection, Speech,
        SpeechState, SpeechStatus, Spinner, SplitAxis, StatusBar, StatusBarItem, StepItem,
        StepStatus, Stepper, StepperItemState, SurfaceCanvas, Svg, Switch, SwitchLabelSide, SwitchState, TabBar,
        TabBarItemState, TabItemState, Table, TableCellState, TableColumn, TableHeaderState, Tabs,
        TabsVariant, Tag, TagVariant, TextInput, TextView, TextViewFormat, Textarea, ThemeMode,
        TimeField, TimeFieldState, TimePrecision, TimeSegment, TimeSegmentState, Toast, Toggle,
        ToggleGroup, ToggleItemState, Toolbar, ToolbarGroup, Tooltip, Tree, TreeItemState,
        TreeNode, TreeState, UniformList, VirtualKeyboard, VirtualList,
    };
    pub use crate::styled::TailwindStyled;
    pub use crate::theme::Theme;
    pub use velox_base::{
        clip_offset_left, clip_offset_right, line_range_at, next_word_boundary, prev_word_boundary,
        split_lines_with_ending, word_range_at, Action, Affinity, Align, Attrs, AttrsList,
        AttrsSpan, AutoClosedPairs, Buffer, BufferLine, BufferPoint, Change, ChangeItem,
        CharacterKind, ComponentSize, ComponentVariant, CosmicEditor, Cursor, CursorSelection,
        Disableable, Edit, EditSelection, FontStyle, FontWeight, LayoutCursor, LayoutGlyph,
        LayoutLine, LayoutRun, LayoutRunIter, LineEnding, LineEndingIter, MaskPattern, MaskToken,
        Metrics, Motion, Navigator, PageTransition, PlainSyntaxHighlighter, RustSyntaxHighlighter,
        Scroll, SearchMatcher, SearchSession, Selection, Selections, Sizable, SyntaxEditor,
        SyntaxHighlighter, SyntaxTheme, TabSize, TextBuffer, TextDecoration, TextDocBuffer,
        TextSnapshot, UndoHistory, Variants, Wrap, MASK_CHAR,
    };
}
