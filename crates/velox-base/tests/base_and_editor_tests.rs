//! `velox-base` 文本与代码编辑引擎及全量移植基础组件的集成测试。

use core::cell::Cell;
use velox_base::*;
use velox_engine::prelude::*;

#[test]
fn test_text_boundary_word_and_line_ranges() {
    let text = "fn hello_world(x: i32) {\n    return x + 1;\n}";
    assert_eq!(word_range_at(text, 4), Some(3..14));
    assert_eq!(&text[word_range_at(text, 4).unwrap()], "hello_world");
    assert_eq!(prev_word_boundary(text, 14), 3);
    assert_eq!(next_word_boundary(text, 3), 14);
    assert_eq!(line_range_at(text, 5), 0..24);
    assert_eq!(line_range_at(text, 28), 25..42);
}

#[test]
fn test_cursor_selections_merge_and_active() {
    let mut sels: Selections<8> = Selections::new();
    assert_eq!(sels.len(), 1);
    assert!(sels.add(CursorSelection::new(1, 3, 8)));
    assert!(sels.add(CursorSelection::new(2, 6, 12)));
    sels.merge_overlapping();
    // [0..0] and merged [3..12]
    assert_eq!(sels.len(), 2);
    assert_eq!(sels.as_slice()[1].start, 3);
    assert_eq!(sels.as_slice()[1].end, 12);
    sels.remove_all_but_active();
    assert_eq!(sels.len(), 1);
}

#[test]
fn test_undo_history_grouping_and_max_undos() {
    let mut hist: UndoHistory<u32, 8, 4> = UndoHistory::new().max_undos(3);
    hist.push(1);
    hist.start_grouping();
    hist.push(2);
    hist.push(3);
    hist.end_grouping();
    hist.push(4);
    hist.push(5); // evicts 1 because max_undos = 3

    assert!(hist.can_undo());
    assert_eq!(hist.undo().unwrap().as_slice(), &[5]);
    assert_eq!(hist.undo().unwrap().as_slice(), &[4]);
    // Grouped transaction returns newest-first on undo ([3, 2])
    assert_eq!(hist.undo().unwrap().as_slice(), &[3, 2]);
    assert!(!hist.can_undo());

    // Redo returns oldest-first ([2, 3])
    assert!(hist.can_redo());
    assert_eq!(hist.redo().unwrap().as_slice(), &[2, 3]);
}

#[test]
fn test_mask_pattern_formatting_and_unmasking() {
    let phone = MaskPattern::new("(999) 999-9999");
    let mut formatted = [0u8; 32];
    let flen = phone.format_into("1234567890", &mut formatted);
    let s = core::str::from_utf8(&formatted[..flen]).unwrap();
    assert_eq!(s, "(123) 456-7890");

    let mut raw = [0u8; 32];
    let ulen = phone.unmask_into(s, &mut raw);
    let unmasked = core::str::from_utf8(&raw[..ulen]).unwrap();
    assert_eq!(unmasked, "1234567890");

    let num_mask = MaskPattern::number_with_fraction(Some(','), 2);
    let mut num_out = [0u8; 32];
    let nlen = num_mask.format_into("1234567.891", &mut num_out);
    assert_eq!(
        core::str::from_utf8(&num_out[..nlen]).unwrap(),
        "1,234,567.89"
    );
}

#[test]
fn test_search_matcher_case_and_navigation() {
    let mut session: SearchSession<16> = SearchSession::new();
    let doc = "Velox Core and velox base and VELOX kit";
    session.open(false, false);
    session.update_query(doc, "velox", false);
    assert_eq!(session.matcher.len(), 3);
    assert_eq!(session.matcher.current_match_index(), 0);
    assert_eq!(session.matcher.next_match(), Some(15..20));
    assert_eq!(session.matcher.next_match(), Some(30..35));
    assert_eq!(session.matcher.next_match(), Some(0..5));
    assert_eq!(session.matcher.prev_match(), Some(30..35));

    session.update_query(doc, "Velox", true);
    assert_eq!(session.matcher.len(), 1);
}

#[test]
fn test_text_buffer_multiline_editing_auto_close_indent_and_undo() {
    let mut buf: TextBuffer<512> = TextBuffer::new();
    // Auto-close bracket + overtype closer
    buf.insert_char('f');
    buf.insert_char('n');
    buf.insert_char('(');
    assert_eq!(buf.as_str(), "fn()");
    assert_eq!(buf.cursor(), 3);
    buf.insert_char(')'); // overtypes closer
    assert_eq!(buf.as_str(), "fn()");
    assert_eq!(buf.cursor(), 4);

    buf.insert_char('{');
    assert_eq!(buf.as_str(), "fn(){}");
    assert_eq!(buf.cursor(), 5);

    // Auto-indent on newline after '{'
    buf.insert_newline();
    assert_eq!(buf.line_count(), 2);
    assert_eq!(buf.cursor_point(), BufferPoint::new(1, 2));

    for ch in "let x = 42;".chars() {
        buf.insert_char(ch);
    }
    assert_eq!(buf.line_str(1), Some("  let x = 42;}"));

    // Move up/down preserving column anchor
    buf.move_up(false);
    assert_eq!(buf.cursor_point().row, 0);
    buf.move_down(false);
    assert_eq!(buf.cursor_point().row, 1);

    // Undo / Redo
    assert!(buf.undo());
    assert!(buf.redo());
}

#[test]
fn test_editable_text_word_navigation_and_password_mask() {
    let mut edit: EditableText<64> = EditableText::from_str("hello rust world");
    assert!(edit.move_word_left());
    assert_eq!(edit.cursor(), 11);
    assert!(edit.delete_word_backward());
    assert_eq!(edit.as_str(), "hello world");
    assert!(edit.select_word(1));
    assert_eq!(edit.selected_text(), "hello");
}

fn assert_view_renders<V: View + 'static>(view: V) {
    let mut engine = VeloxEngine::new(800, 600);
    engine.mount_root(view);
    let damaged = engine.step_frame();
    assert!(!damaged.is_empty());
    assert!(!engine.cached_draw_ops().is_empty());
}

struct AllNewComponentsView {
    batch: usize,
    otp: OtpState<6>,
    time: TimeFieldState,
    color: ColorPickerState,
    tree: TreeState<8>,
    dock: DockWorkspaceState,
    radio_idx: Cell<Option<usize>>,
    number_val: Cell<i64>,
    page: Cell<usize>,
}

impl View for AllNewComponentsView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let editor_str = "fn main() {\n  println!(\"hi\");\n}";
        let textarea_str = "Line 1\nLine 2";
        let otp_bytes = cx.arena().alloc_bytes(self.otp.len());
        otp_bytes.copy_from_slice(self.otp.as_str().as_bytes());
        let otp_str = unsafe { core::str::from_utf8_unchecked(otp_bytes) };

        let on_radio = cx.arena().alloc(|idx: usize, _s: &mut ActionScope| {
            let _ = idx;
        });
        let on_num = cx.arena().alloc(|val: i64, _s: &mut ActionScope| {
            let _ = val;
        });

        const RADIO_OPTS: &[&str] = &["Option A", "Option B", "Option C"];
        const TOGGLE_OPTS: &[&str] = &["Bold", "Italic", "Underline"];
        const ACC_ITEMS: &[AccordionItem<'static>] = &[
            AccordionItem::new("Section 1", "Details for section 1"),
            AccordionItem::new("Section 2", "Details for section 2"),
        ];
        const COLS: &[TableColumn<'static>] = &[
            TableColumn::new("Name", 100),
            TableColumn::new("Role", 100),
        ];
        const ROW0: &[&str] = &["Alice", "Architect"];
        const ROW1: &[&str] = &["Bob", "Developer"];
        const ROWS: &[&[&str]] = &[ROW0, ROW1];
        const TREE_NODES: &[TreeNode<'static>] = &[
            TreeNode::new(1, "src", 0, true, true),
            TreeNode::new(2, "lib.rs", 1, false, false),
        ];
        const TB_GRP0: &[&str] = &["Cut", "Copy", "Paste"];
        const TB_GROUPS: &[ToolbarGroup<'static>] = &[ToolbarGroup::new(TB_GRP0)];
        const COMBO_OPTS: &[&str] = &["Rust", "TypeScript", "Go"];
        const DIAGS: &[DiagnosticSpan<'static>] = &[DiagnosticSpan {
            range: Selection { start: 0, end: 2 },
            severity: DiagnosticSeverity::Warning,
            message: "unused",
        }];
        const HIGHLIGHTS: &[HighlightSpan] = &[HighlightSpan {
            range: Selection { start: 0, end: 2 },
            kind: HighlightKind::Keyword,
        }];

        let root = div().flex_col().w(800).h(600);
        match self.batch {
            0 => root
                .child(
                    cx,
                    CodeEditor::new(100, editor_str, true)
                        .size(400, 140)
                        .diagnostics(DIAGS)
                        .highlights(HIGHLIGHTS),
                )
                .into_element(cx),
            1 => root
                .child(
                    cx,
                    Textarea::new(200, textarea_str, false)
                        .placeholder("Enter notes...")
                        .size(300, 80),
                )
                .child(
                    cx,
                    TextInput::new(250, "secret", true)
                        .masked(true)
                        .selection(Some((0, 3))),
                )
                .into_element(cx),
            2 => root
                .child(
                    cx,
                    RadioGroup::new(300, RADIO_OPTS, self.radio_idx.get())
                        .horizontal()
                        .on_change(on_radio),
                )
                .child(cx, ToggleGroup::new(310, TOGGLE_OPTS, 0b101).multiple(true))
                .into_element(cx),
            3 => root
                .child(
                    cx,
                    NumberInput::new(320, self.number_val.get())
                        .range(0, 100)
                        .step(5)
                        .suffix("px")
                        .on_change(on_num),
                )
                .child(cx, OtpInput::new(330, otp_str, 6).focused(true))
                .into_element(cx),
            4 => root
                .child(cx, TimeField::new(340, self.time).focused(true))
                .child(
                    cx,
                    DatePicker::new(350, Some(SimpleDate::new(2026, 11, 10)), false),
                )
                .into_element(cx),
            5 => root
                .child(cx, ColorPicker::new(360, self.color))
                .into_element(cx),
            6 => root
                .child(cx, Accordion::new(370, ACC_ITEMS, 0b01))
                .child(
                    cx,
                    Avatar::new(380, "VX").status(AvatarStatus::Online),
                )
                .child(cx, Link::new(390, "Documentation").href("https://velox.dev"))
                .child(
                    cx,
                    Tooltip::new(400, "Helpful tip", true).trigger(cx, text("Hover me")),
                )
                .into_element(cx),
            7 => root
                .child(
                    cx,
                    Popover::new(410, true)
                        .trigger(cx, text("Open Popover"))
                        .content(cx, text("Popover body")),
                )
                .child(
                    cx,
                    HoverCard::new(420, "@velox", "Zero-allocation UI engine", true),
                )
                .into_element(cx),
            8 => root
                .child(
                    cx,
                    AlertDialog::new(430, "Delete file?", "This action cannot be undone.")
                        .destructive(true),
                )
                .child(cx, Pagination::new(440, self.page.get(), 10))
                .into_element(cx),
            9 => root
                .child(cx, Table::new(450, COLS, ROWS).selected_row(Some(0)))
                .child(
                    cx,
                    Tree::new(460, TREE_NODES).selected(self.tree.selected_id),
                )
                .into_element(cx),
            10 => root
                .child(cx, Toolbar::new(470, TB_GROUPS).active(0, 1))
                .child(
                    cx,
                    Scrollbar::new(480, ScrollbarAxis::Vertical, 120, 500, 120, 60),
                )
                .child(
                    cx,
                    Combobox::new(490, "ru", COMBO_OPTS, Some(0), true),
                )
                .into_element(cx),
            _ => root
                .child(
                    cx,
                    SelectableText::new(500, "Selectable snippet").selection(Some((0, 10))),
                )
                .child(
                    cx,
                    MarkdownView::new(510, "# Heading\n- item 1\n> quote\n```\ncode()\n```"),
                )
                .child(
                    cx,
                    ResizableSplit::new(
                        520,
                        SplitAxis::Horizontal,
                        400,
                        120,
                        self.dock.left_width,
                    )
                    .first(cx, text("Left Pane"))
                    .second(cx, text("Right Pane")),
                )
                .into_element(cx),
        }
    }
}

#[test]
fn test_render_all_ported_base_components_and_code_editor() {
    let mut editor_buf: TextBuffer<128> = TextBuffer::from_str("fn main() {\n  println!(\"hi\");\n}");
    editor_buf.search.set_query("main", true);
    let doc = editor_buf.as_str();
    let mut matcher = editor_buf.search.clone();
    matcher.update_matches(doc);
    editor_buf.search = matcher;

    let mut otp = OtpState::new();
    assert!(otp.push_char('4'));
    assert!(otp.push_char('2'));

    let mut color = ColorPickerState::default();
    color.toggle_open();

    let mut tree = TreeState::new();
    tree.toggle_expand(1);
    tree.select(2);

    for batch in 0..=11 {
        assert_view_renders(AllNewComponentsView {
            batch,
            otp: otp.clone(),
            time: TimeFieldState::new(14, 30, 0),
            color,
            tree: tree.clone(),
            dock: DockWorkspaceState::default(),
            radio_idx: Cell::new(Some(1)),
            number_val: Cell::new(25),
            page: Cell::new(3),
        });
    }
}

struct CatalogShowcaseBatchView {
    batch: usize,
    msg_scroller: MessageScrollerState,
    questionnaire: QuestionnaireState<4>,
    speech: SpeechState,
}

impl View for CatalogShowcaseBatchView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        const REACTIONS: &[BubbleReaction<'static>] = &[
            BubbleReaction::new("👍 4", 4, true),
            BubbleReaction::new("🎉 2", 2, false),
        ];
        const ATTS: &[Attachment<'static>] = &[
            Attachment::new(1010, "report.pdf", "2.4 MB")
                .kind(AttachmentKind::File)
                .progress(75),
        ];
        const MSGS: &[Message<'static>] = &[
            Message::new(1020, MessageRole::Assistant, "Velox Bot", "Hello from Velox!")
                .avatar("VB")
                .timestamp("15:00")
                .attachments(ATTS)
                .reactions(REACTIONS),
            Message::new(1030, MessageRole::User, "Developer", "Looks awesome!")
                .avatar("DV")
                .timestamp("15:01"),
        ];
        const QUESTIONS: &[QuestionItem<'static>] = &[
            QuestionItem::new(
                "Which renderer do you prefer?",
                &["Software SDF", "Skia"],
                QuestionKind::SingleChoice,
            ),
        ];
        const STEPS: &[StepItem<'static>] = &[
            StepItem::new("Configure", "Setup target", StepStatus::Completed),
            StepItem::new("Build", "Compile no_std", StepStatus::Active),
        ];
        const SELECT_OPTS: &[SelectOption<'static>] = &[
            SelectOption::new("Rust 2024", "2024"),
            SelectOption::new("Legacy", "2015").disabled(true),
        ];
        const DESC_ITEMS: &[DescriptionItem<'static>] = &[
            DescriptionItem::new("Architecture", "Zero-Allocation Arena"),
        ];
        const SIDEBAR_ITEMS: &[SidebarItem<'static>] = &[
            SidebarItem::new(1, "Dashboard", "⌂").badge("2"),
        ];
        const SIDEBAR_SECS: &[SidebarSection<'static>] =
            &[SidebarSection::new("MAIN", SIDEBAR_ITEMS)];
        const STATUS_LEFT: &[StatusBarItem<'static>] = &[StatusBarItem::new("main*")];
        const STATUS_RIGHT: &[StatusBarItem<'static>] = &[StatusBarItem::new("UTF-8")];
        const CMD_ITEMS: &[CommandItem<'static>] = &[
            CommandItem::new("Format Document", "Shift+Alt+F", "Editor"),
        ];
        const LIST_ITEMS: &[ListItem<'static>] = &[
            ListItem::new("velox-base", "70+ headless & base widgets", "v0.1.0"),
        ];
        const MENU_ITEMS: &[MenuItem<'static>] = &[
            MenuItem::new("Rename", "F2"),
            MenuItem::separator(),
            MenuItem::new("Delete", "Del").destructive(true),
        ];
        const SET_ITEMS: &[SettingsItem<'static>] = &[
            SettingsItem::new("Dark Mode", "Use dark color palette", SettingsValue::Bool(true)),
        ];
        const SET_GROUPS: &[SettingsGroup<'static>] =
            &[SettingsGroup::new("Appearance", SET_ITEMS)];
        const TILES: &[GridTile<'static>] = &[
            GridTile::new(1, 0, 0, 2, 1, "CPU Usage", "14%"),
        ];
        const CANDLES: &[CandlestickBar] = &[
            CandlestickBar::new(100, 120, 95, 115),
            CandlestickBar::new(115, 118, 98, 102),
        ];
        const COLS: &[TableColumn<'static>] = &[
            TableColumn::new("Crate", 120),
            TableColumn::new("Status", 100),
        ];
        const ROW0: &[&str] = &["velox-base", "Ready"];
        const ROWS: &[&[&str]] = &[ROW0];

        let root = div().flex_col().w(800).h(600).gap(4);
        match self.batch {
            0 => root
                .child(cx, Alert::new(1001, "Build Succeeded", "All 70+ components ready").variant(AlertVariant::Success))
                .child(cx, Icon::new(1002, IconName::Check).large())
                .child(cx, Image::new(1003, 80, 48, "logo.png").fallback("VELOX"))
                .child(cx, Kbd::new(1004, "Ctrl+K"))
                .child(cx, Label::new(1005, "Project Name").required(true))
                .child(cx, Marker::new(1006, "2 Unread").variant(MarkerVariant::Unread))
                .into_element(cx),
            1 => root
                .child(cx, MessageScroller::new(1007, MSGS, self.msg_scroller, 360, 120))
                .child(cx, Progress::new(1008, 200, 68))
                .into_element(cx),
            2 => root
                .child(cx, Questionnaire::new(1009, QUESTIONS, self.questionnaire))
                .child(cx, Rating::new(1040, 4).max(5))
                .child(cx, Skeleton::text_lines(1041, 200, 2))
                .into_element(cx),
            3 => root
                .child(cx, Stepper::new(1042, STEPS))
                .child(cx, Tag::new(1043, "no_std").variant(TagVariant::Primary).closable(true))
                .child(cx, TextView::html(1044, "<h1>Title</h1>\n<code>let x = 1;</code>"))
                .child(cx, DropdownButton::new(1045, "Run", &["Run Debug"], true))
                .into_element(cx),
            4 => root
                .child(cx, Input::new(1046, "velox", false))
                .child(cx, Select::new(1047, SELECT_OPTS, Some(0), true))
                .child(cx, Speech::new(1048, self.speech, "Dictated text"))
                .child(
                    cx,
                    Form::new(1049, 320).title("Profile").content(
                        cx,
                        FormField::new(1050, "Email")
                            .required(true)
                            .control(cx, Input::new(1051, "dev@velox.rs", false)),
                    ),
                )
                .into_element(cx),
            5 => root
                .child(cx, DescriptionList::new(1052, DESC_ITEMS).title("Specs"))
                .child(cx, GroupBox::new(1053, "Network").content(cx, text("eth0: Connected")))
                .child(cx, Scrollable::new(1054, 240, 80, 240, 30).content(cx, text("Scrollable")))
                .child(cx, Sidebar::new(1055, "Velox Studio", SIDEBAR_SECS).active(Some(1)))
                .child(cx, StatusBar::new(1056, 600).left(STATUS_LEFT).right(STATUS_RIGHT))
                .into_element(cx),
            6 => root
                .child(cx, Diff::from_patch(1057, "src/lib.rs", "@@ -1 +1 @@\n-old();\n+new();"))
                .child(cx, Carousel::new(1058, &["Slide 1", "Slide 2"], 0))
                .child(cx, Command::new(1059, "format", CMD_ITEMS, 0))
                .into_element(cx),
            7 => root
                .child(cx, Chart::new(1060, 140, 60, &[10, 40, 25, 60], 0, 100).kind(ChartKind::Area))
                .child(cx, Chart::new(1061, 80, 80, &[25, 35, 20, 20], 0, 100).kind(ChartKind::Pie))
                .child(cx, Chart::new(1062, 140, 60, &[], 90, 135).candles(CANDLES))
                .child(cx, List::new(1063, LIST_ITEMS).selected(Some(0)))
                .into_element(cx),
            8 => root
                .child(cx, Menu::new(1064, MENU_ITEMS))
                .child(cx, ContextMenu::new(1065, 120, 80, true, MENU_ITEMS))
                .child(cx, Settings::new(1066, SET_GROUPS))
                .child(cx, DataTable::new(1067, COLS, ROWS).sort_by(0, SortDirection::Asc))
                .into_element(cx),
            9 => root
                .child(
                    cx,
                    Dock::new(1068, 400, 160, DockWorkspaceState::default())
                        .left_panel(DockPanel::new(&["Explorer"], 0, "src/"))
                        .bottom_panel(DockPanel::new(&["Terminal"], 0, "$ cargo test"))
                        .center(cx, text("Main Editor Area")),
                )
                .child(cx, GridLayout::new(1069, 3, 80, 50, TILES).active_tile(Some(1)))
                .into_element(cx),
            _ => {
                let body = div()
                    .flex_col()
                    .gap(4)
                    .child(
                        cx,
                        virtual_list(1070, 500, 24, 48, 24, |_idx, cx| {
                            text("Virtual row").size(12).into_element(cx)
                        }),
                    )
                    .child(cx, Sheet::new(1071, "Inspector", SheetSide::Right, true).content(cx, text("Sheet")));

                Root::new(1000, 800, 600)
                    .theme(ThemeMode::Dark.to_theme())
                    .content(cx, body)
                    .dialog(
                        cx,
                        Dialog::new(1080, "Confirm Deployment", true)
                            .content(cx, text("Target: STM32H7")),
                    )
                    .notification(
                        cx,
                        Notification::new(1090, "Upload Complete", "Firmware verified")
                            .variant(NotificationVariant::Success),
                    )
                    .into_element(cx)
            }
        }
    }
}

#[test]
fn test_full_70_components_catalog_render_and_states() {
    let mut msg_scroller = MessageScrollerState::new();
    msg_scroller.scroll_to(20, 120);
    assert!(!msg_scroller.follow_tail);
    msg_scroller.on_new_message(160);
    assert_eq!(msg_scroller.unread_count, 1);
    msg_scroller.scroll_to_bottom(160);
    assert!(msg_scroller.follow_tail);
    assert_eq!(msg_scroller.unread_count, 0);

    let mut q_state: QuestionnaireState<4> = QuestionnaireState::new();
    q_state.select_option(1, QuestionKind::SingleChoice);
    assert!(q_state.is_selected(0, 1));
    q_state.next_step(2);
    assert_eq!(q_state.current_step, 1);
    q_state.select_option(0, QuestionKind::MultiChoice);
    q_state.select_option(2, QuestionKind::MultiChoice);
    assert!(q_state.is_selected(1, 0) && q_state.is_selected(1, 2));

    let mut speech = SpeechState::new();
    speech.toggle_listening();
    speech.audio_level = 65;
    assert_eq!(speech.status, SpeechStatus::Listening);

    for batch in 0..=10 {
        assert_view_renders(CatalogShowcaseBatchView {
            batch,
            msg_scroller,
            questionnaire: q_state,
            speech,
        });
    }
}

#[test]
fn test_large_multibyte_text_buffer_incremental_undo_redo_lossless() {
    let initial = "// 中文头部注释与多字节符号 🚀✨\r\nfn compute_matrix_transform(alpha: i32, beta: i32) -> i32 {\r\n    let 描述 = \"超长超过两百五十六字节的代码缓冲区撤销重做无损测试验证文本段落\";\r\n    let mut acc = alpha.saturating_mul(beta);\r\n    for step in 0..16 {\r\n        acc = acc.wrapping_add(step * 42);\r\n    }\r\n    let _ = 描述;\r\n    acc\r\n}\r\n";
    assert!(initial.len() > 300);

    let mut buf: TextBuffer<1024> = TextBuffer::from_str(initial);
    assert_eq!(buf.as_str(), initial);

    // 1. 回到首行并移动到第一行末尾（`\r` 之前），验证 CRLF 边界安全性并插入文本
    while buf.move_up(false) {}
    buf.move_line_home(false);
    buf.move_line_end(false);
    assert_eq!(buf.as_str().as_bytes()[buf.cursor()], b'\r');
    let cur = buf.cursor();
    assert!(buf.replace_range(cur..cur, " [已审核]"));
    let after_edit_1 = buf.as_str();
    let mut snap_1 = [0u8; 1024];
    let snap_1_len = after_edit_1.len();
    snap_1[..snap_1_len].copy_from_slice(after_edit_1.as_bytes());

    // 2. 选中超过 300B 的多字节全文并替换（验证超过旧版 256B 快照上限时无损记录删除区）
    buf.select_all();
    assert!(buf.replace_range(0..buf.len(), "fn replaced() -> bool { true // 替换全文测试 🎯 }"));
    let after_edit_2 = "fn replaced() -> bool { true // 替换全文测试 🎯 }";
    assert_eq!(buf.as_str(), after_edit_2);

    // 3. 连续追加编辑
    buf.move_line_end(false);
    let end_cur = buf.cursor();
    assert!(buf.replace_range(end_cur..end_cur, "\n// 第二行尾部追加"));
    let after_edit_3 = "fn replaced() -> bool { true // 替换全文测试 🎯 }\n// 第二行尾部追加";
    assert_eq!(buf.as_str(), after_edit_3);

    // 逐步撤销到底，验证没有任何 256B 截断或 UTF-8 损坏
    assert!(buf.undo());
    assert_eq!(buf.as_str(), after_edit_2);
    assert!(buf.undo());
    assert_eq!(buf.as_str().as_bytes(), &snap_1[..snap_1_len]);
    assert!(buf.undo());
    assert_eq!(buf.as_str(), initial);
    assert!(!buf.undo());

    // 逐步重做到顶，验证双向增量补丁逐字节还原
    assert!(buf.redo());
    assert_eq!(buf.as_str().as_bytes(), &snap_1[..snap_1_len]);
    assert!(buf.redo());
    assert_eq!(buf.as_str(), after_edit_2);
    assert!(buf.redo());
    assert_eq!(buf.as_str(), after_edit_3);
    assert!(!buf.redo());
}

#[test]
fn test_large_uniform_list_i32_scroll_without_i16_overflow() {
    struct HugeListView;
    impl View for HugeListView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            // 10_000 * 24 = 240_000 > i16::MAX (32_767)
            div()
                .w(320)
                .h(240)
                .child(
                    cx,
                    uniform_list(9001, 10_000, 24, 120, 0, |idx, cx| {
                        let label = cx.arena().alloc_fmt(format_args!("Row #{idx}"));
                        div()
                            .w_full()
                            .h(24)
                            .child(cx, text(label).size(12))
                            .into_element(cx)
                    })
                    .scroll_y_i32(180_000)
                    .w(280),
                )
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 240);
    engine.mount_root(HugeListView);
    let damage = engine.step_frame();
    assert!(!damage.is_empty());
    // 180_000 / 24 = 7500，可见窗口应精确渲染 Row #7500 附近的文本指令而不发生溢出或空白
    let has_row_7500 = engine.cached_draw_ops().iter().any(|op| {
        matches!(
            &op.payload,
            DrawPayload::TextRun { text, .. } if text.as_str() == "Row #7500"
        )
    });
    assert!(has_row_7500);
}

#[test]
fn test_headless_explicit_style_preserved_and_root_theme_injection() {
    let custom_red = Color::rgb(220, 38, 38);

    struct CustomAndThemeView {
        custom_bg: Color,
    }
    impl View for CustomAndThemeView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let light = Theme::light();
            Root::new(1, 320, 240)
                .theme(light)
                .content_with(cx, |cx| {
                    div()
                        .flex_col()
                        .gap(8)
                        .child(cx, Button::new(10, "ExplicitRed").bg(self.custom_bg))
                        .child(cx, Button::new(11, "HeadlessBare"))
                })
                .into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 240);
    // 引擎全局默认是暗黑主题，但 Root 注入了 Theme::light()
    engine.set_theme(Theme::dark());
    engine.mount_root(CustomAndThemeView {
        custom_bg: custom_red,
    });
    let _ = engine.step_frame();

    let ops = engine.cached_draw_ops();
    // 1. 显式调用 `.bg(custom_red)` 的无样式 Button 必须保留 `custom_red` 且默认保持 0px 直角
    let has_custom_red_sharp = ops.iter().any(|op| {
        matches!(
            op.payload,
            DrawPayload::Quad { bg, rounded: 0, .. } if bg == custom_red
        )
    });
    assert!(has_custom_red_sharp);

    // 2. 未设置背景色的 velox_base::Button 必须保持透明无样式（不发射多余背景/边框 Quad），
    //    同时其文字前景色正确继承自 Root 注入的 Theme::light().foreground（保证可读性）
    let light_theme = Theme::light();
    assert!(
        !ops.iter().any(|op| op.id == Some(11)),
        "Bare velox_base::Button must not emit background/border Quad ops"
    );
    let has_light_fg_text = ops.iter().any(|op| {
        matches!(
            &op.payload,
            DrawPayload::TextRun { text, color, .. }
                if text.as_str() == "HeadlessBare" && *color == light_theme.foreground
        )
    });
    assert!(has_light_fg_text);
}

#[test]
fn test_svg_and_vector_icon_components() {
    use velox_base::{Icon, IconName, Sizable, Svg};

    const BADGE_SVG: &str = r##"<svg viewBox="0 0 24 24">
        <circle cx="12" cy="12" r="10" fill="#22C55E"/>
        <path d="M8 12l3 3 5-6" fill="none" stroke="#FFFFFF" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>"##;

    struct IconGalleryView;
    impl View for IconGalleryView {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            let mut row = div()
                .size_full()
                .flex_row()
                .flex_wrap()
                .gap(4)
                .p(8)
                .bg(Color::rgb(15, 23, 42))
                .child(cx, Svg::new(5001, BADGE_SVG).square(24))
                .child(
                    cx,
                    Svg::from_path(5002, "M12 2L22 22H2Z")
                        .square(20)
                        .color(Color::rgb(250, 204, 21)),
                )
                .child(
                    cx,
                    Svg::icon_path(5003, "M5 12h14")
                        .square(20)
                        .color(Color::rgb(56, 189, 248)),
                );
            let names = [
                IconName::Info,
                IconName::Check,
                IconName::Close,
                IconName::Plus,
                IconName::Minus,
                IconName::Search,
                IconName::Settings,
                IconName::Star,
                IconName::StarOutline,
                IconName::Warning,
                IconName::Error,
                IconName::ChevronUp,
                IconName::ChevronDown,
                IconName::ChevronLeft,
                IconName::ChevronRight,
                IconName::File,
                IconName::Folder,
                IconName::Mic,
                IconName::Trash,
            ];
            for (i, name) in names.iter().enumerate() {
                assert!(name.svg_path().is_some());
                row = row.child(
                    cx,
                    Icon::new(5100 + i as u32, *name)
                        .medium()
                        .color(Color::rgb(226, 232, 240)),
                );
            }
            // Custom 变体仍保留符号文本回退
            row = row.child(cx, Icon::symbol(5200, "V").color(Color::rgb(248, 250, 252)));
            row.into_element(cx)
        }
    }

    let mut engine = VeloxEngine::new(320, 160);
    let _ = engine.mount_root(IconGalleryView);
    let _ = engine.step_frame_dt(16);

    let svg_ops = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| matches!(op.payload, DrawPayload::Sprite(SpriteSource::Svg { .. })))
        .count();
    // 3 个 Svg 组件 + 19 个内置矢量 IconName = 22 个原生矢量 SVG 指令
    assert_eq!(svg_ops, 22);

    let mut fb = vec![Xrgb8888::BLACK; 320 * 160];
    engine.rasterize_region(Region::new(0, 0, 320, 160), &mut fb);
    // 确认绿色圆徽标与白色矢量图标像素均已成功光栅化
    let green_px = fb
        .iter()
        .filter(|px| {
            let c = px.to_color();
            c.g() > 170 && c.r() < 80
        })
        .count();
    let light_icon_px = fb
        .iter()
        .filter(|px| {
            let c = px.to_color();
            c.r() > 180 && c.g() > 180 && c.b() > 180
        })
        .count();
    assert!(green_px >= 120, "expected rendered green Svg badge, got {}", green_px);
    assert!(
        light_icon_px >= 150,
        "expected crisp vector Icon pixels across all 19 icons, got {}",
        light_icon_px
    );
}
