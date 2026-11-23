//! 参照 `pop-os/cosmic-text/tree/main/src` 重构的 `velox_base::input::text_engine`
//! 与 `CodeEditor` / `Textarea` 全面单元测试及无头端到端渲染交互测试。

use velox_base::input::text_engine::{
    split_lines_with_ending, Action, Affinity, Attrs, AttrsList, BufferLine, CosmicEditor, Cursor,
    Edit, EditSelection, LineEnding, Metrics, Motion, RustSyntaxHighlighter, SyntaxEditor,
    SyntaxTheme, TextDocBuffer, Wrap,
};
use velox_base::{CodeEditor, Textarea};
use velox_core::display::measure_line_width_px;
use velox_core::prelude::*;

#[test]
fn test_line_ending_preservation_and_roundtrip() {
    let raw = "alpha\r\nbeta\ngamma\rdelta\n";
    let mut parts: Vec<(&str, LineEnding)> = Vec::new();
    for (line, ending) in split_lines_with_ending(raw) {
        parts.push((line, ending));
    }
    assert_eq!(
        parts,
        vec![
            ("alpha", LineEnding::CrLf),
            ("beta", LineEnding::Lf),
            ("gamma", LineEnding::Cr),
            ("delta", LineEnding::Lf),
            ("", LineEnding::None),
        ]
    );

    let buf = TextDocBuffer::from_str(Metrics::default(), raw, Attrs::new());
    assert_eq!(buf.line_count(), 5);
    assert_eq!(buf.to_flat_string::<256>().as_str(), raw);
}

#[test]
fn test_attrs_list_span_override_split_and_append() {
    let base = Attrs::new().color(Color::WHITE);
    let kw = Attrs::new().color(Color::rgb(192, 132, 252));
    let num = Attrs::new().color(Color::rgb(251, 146, 60));

    let mut list = AttrsList::new(base);
    list.add_span(0..10, kw);
    list.add_span(3..6, num);

    assert_eq!(list.get_span(0), kw);
    assert_eq!(list.get_span(2), kw);
    assert_eq!(list.get_span(3), num);
    assert_eq!(list.get_span(5), num);
    assert_eq!(list.get_span(6), kw);
    assert_eq!(list.get_span(10), base);

    let right = list.split_off(4);
    assert_eq!(list.get_span(3), num);
    assert_eq!(list.get_span(4), base);
    assert_eq!(right.get_span(0), num);
    assert_eq!(right.get_span(1), num);
    assert_eq!(right.get_span(2), kw);

    list.append(&right, 4);
    assert_eq!(list.get_span(3), num);
    assert_eq!(list.get_span(5), num);
    assert_eq!(list.get_span(6), kw);
}

#[test]
fn test_buffer_line_soft_wrap_and_tab_stops() {
    let mut line = BufferLine::new(
        "fn render_widget(alpha: i32, beta: i32, gamma: i32) -> bool",
        LineEnding::None,
        AttrsList::default(),
    );

    // 不折行时恒为 1 条视觉行
    let unwrapped = line.layout(13, Some(180.0), Wrap::None, 4);
    assert_eq!(unwrapped.len(), 1);
    assert!(unwrapped[0].w > 180.0);

    // 开启 WordOrGlyph 后拆为多条视觉行，且所有字符覆盖完整无遗漏
    let wrapped = line.layout(13, Some(180.0), Wrap::WordOrGlyph, 4);
    assert!(wrapped.len() >= 2, "expected soft wrap into >= 2 visual lines");
    assert_eq!(wrapped[0].start_index(), 0);
    assert_eq!(wrapped.last().unwrap().end_index(), line.text().len());

    // 制表符对齐到 4 空格整数倍列宽
    let mut tab_line = BufferLine::new("\tx", LineEnding::None, AttrsList::default());
    let tab_layout = tab_line.layout(13, None, Wrap::None, 4);
    assert_eq!(tab_layout[0].glyphs.len(), 2);
    let tab_w = tab_layout[0].glyphs[0].w;
    let x_pos = tab_layout[0].glyphs[1].x;
    assert!(tab_w > 10.0 && (x_pos - tab_w).abs() < 0.01);
}

#[test]
fn test_buffer_subpixel_hit_cjk_and_cursor_x_memory() {
    let doc = "WWWWWW\n你好世界\na\n1234567890";
    let mut buf = TextDocBuffer::from_str(Metrics::new(13.0, 20.0), doc, Attrs::new());
    buf.set_wrap(Wrap::None);
    buf.layout_all();

    // 亚像素命中：第 0 行 "WWWWWW" 前半个 W 落索引 0，超过 2 个 W 落 >= 2
    let w2_px = measure_line_width_px("WW", 13) as f32;
    let c0 = buf.hit(1.0, 10.0).unwrap();
    assert_eq!(c0.line, 0);
    assert_eq!(c0.index, 0);

    let c2 = buf.hit(w2_px + 0.5, 10.0).unwrap();
    assert_eq!(c2.line, 0);
    assert_eq!(c2.index, 2);

    // 第 1 行 CJK 多字节 UTF-8 命中必落在合法字符边界
    let cn_mid_px = measure_line_width_px("你好", 13) as f32;
    let cn_hit = buf.hit(cn_mid_px + 0.5, 30.0).unwrap();
    assert_eq!(cn_hit.line, 1);
    assert!(buf.lines[1].text().is_char_boundary(cn_hit.index));
    assert_eq!(cn_hit.index, "你好".len());

    // 跨短行垂直导航保持 cursor_x_opt 记忆：从第 0 行末下移到第 2 行 "a" 再下移到第 3 行
    let start = Cursor::new(0, 5);
    let (c_line1, x_mem1) = buf.cursor_motion(start, None, Motion::Down).unwrap();
    assert_eq!(c_line1.line, 1);
    assert!(x_mem1.is_some());

    let (c_line2, x_mem2) = buf.cursor_motion(c_line1, x_mem1, Motion::Down).unwrap();
    assert_eq!(c_line2.line, 2);
    assert_eq!(c_line2.index, 1); // 短行 "a" 被钳制到 1
    assert_eq!(x_mem2, x_mem1); // 视觉列 X 记忆保持不变！

    let (c_line3, _) = buf.cursor_motion(c_line2, x_mem2, Motion::Down).unwrap();
    assert_eq!(c_line3.line, 3);
    assert!(
        c_line3.index >= 4,
        "vertical motion across short line should restore visual x column, got {}",
        c_line3.index
    );
}

#[test]
fn test_cosmic_editor_multiclick_block_indent_autoclose_and_undo_redo() {
    let mut ed = CosmicEditor::from_str(
        Metrics::new(13.0, 20.0),
        "let alpha = 10;\nlet beta = 20;",
    );
    ed.buffer_mut().layout_all();

    // 1. 双击第 0 行 "alpha" 区域选中整词
    let alpha_x = (measure_line_width_px("let al", 13) as i32) + 1;
    ed.handle_pointer_press(alpha_x, 10, 100, false);
    ed.handle_pointer_press(alpha_x, 10, 240, false);
    assert_eq!(ed.copy_selection().unwrap().as_str(), "alpha");

    // 2. 三击选中整行
    ed.handle_pointer_press(alpha_x, 10, 350, false);
    assert!(matches!(ed.selection(), EditSelection::Line(_)));
    assert_eq!(ed.copy_selection().unwrap().as_str(), "let alpha = 10;");

    // 3. Ctrl+A 全选 + Tab 多行块缩进 + Shift+Tab 反缩进 + Undo/Redo 往返
    assert!(ed.handle_key_with_mods(KeyCode::Char('a'), KeyModifiers::CTRL));
    assert!(ed.handle_key_with_mods(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        ed.to_flat_string::<256>().as_str(),
        "    let alpha = 10;\n    let beta = 20;"
    );

    assert!(ed.handle_key_with_mods(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(
        ed.to_flat_string::<256>().as_str(),
        "let alpha = 10;\nlet beta = 20;"
    );

    // Undo 撤销反缩进 -> 恢复 4 空格缩进；再 Undo -> 回到无缩进
    assert!(ed.undo());
    assert_eq!(
        ed.to_flat_string::<256>().as_str(),
        "    let alpha = 10;\n    let beta = 20;"
    );
    assert!(ed.undo());
    assert_eq!(
        ed.to_flat_string::<256>().as_str(),
        "let alpha = 10;\nlet beta = 20;"
    );
    assert!(ed.redo());
    assert_eq!(
        ed.to_flat_string::<256>().as_str(),
        "    let alpha = 10;\n    let beta = 20;"
    );

    // 4. 自动括号闭合与 Enter 自动缩进
    let mut ed2 = CosmicEditor::from_str(Metrics::default(), "fn main()");
    assert!(ed2.handle_key_with_mods(KeyCode::Space, KeyModifiers::NONE));
    assert!(ed2.handle_key_with_mods(KeyCode::Char('{'), KeyModifiers::NONE));
    assert_eq!(ed2.to_flat_string::<128>().as_str(), "fn main() {}");
    assert!(ed2.handle_key_with_mods(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(ed2.to_flat_string::<128>().as_str(), "fn main() {\n    }");

    // Undo 一次性回滚 Enter 自动缩进
    assert!(ed2.undo());
    assert_eq!(ed2.to_flat_string::<128>().as_str(), "fn main() {}");
    // 再 Undo 回滚 `{}` 自动配对
    assert!(ed2.undo());
    assert_eq!(ed2.to_flat_string::<128>().as_str(), "fn main() ");
}

#[test]
fn test_syntax_editor_incremental_nested_block_comments_and_theme_sync() {
    let code = "let x = 1;\nlet y = \"hello\";\nfn run() {}";
    let mut sed = SyntaxEditor::from_str(
        Metrics::new(13.0, 20.0),
        code,
        RustSyntaxHighlighter,
        SyntaxTheme::dark(),
    );

    let dark = SyntaxTheme::dark();
    assert_eq!(
        sed.buffer_ref().lines[0].attrs_list().get_span(0).color_opt,
        Some(dark.keyword)
    );
    assert_eq!(
        sed.buffer_ref().lines[1]
            .attrs_list()
            .get_span(9)
            .color_opt,
        Some(dark.string)
    );
    assert_eq!(
        sed.buffer_ref().lines[2].attrs_list().get_span(0).color_opt,
        Some(dark.keyword)
    );

    // 在第 0 行开头插入未闭合的 `/* ` -> 整篇后续行均应增量重染为注释色
    sed.set_cursor(Cursor::new(0, 0));
    sed.insert_string("/* ", None);
    assert_eq!(
        sed.buffer_ref().lines[1]
            .attrs_list()
            .get_span(9)
            .color_opt,
        Some(dark.comment)
    );
    assert_eq!(
        sed.buffer_ref().lines[2].attrs_list().get_span(0).color_opt,
        Some(dark.comment)
    );

    // 在第 1 行末尾闭合 ` */` -> 第 2 行自动恢复关键字配色
    let line1_end = sed.buffer_ref().lines[1].text().len();
    sed.set_cursor(Cursor::new(1, line1_end));
    sed.insert_string(" */", None);
    assert_eq!(
        sed.buffer_ref().lines[2].attrs_list().get_span(0).color_opt,
        Some(dark.keyword)
    );

    // 切换亮色主题时整篇同步更新为亮色配色板
    sed.sync_theme(&CoreTheme::light());
    let light = SyntaxTheme::light();
    assert_eq!(
        sed.buffer_ref().lines[2].attrs_list().get_span(0).color_opt,
        Some(light.keyword)
    );
}

#[test]
fn test_code_editor_zero_jitter_cursor_glyph_selection_and_text_cursor_shape() {
    struct EditorHost {
        editor: Box<SyntaxEditor<RustSyntaxHighlighter>>,
        focused: bool,
    }

    impl View for EditorHost {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            self.editor.buffer_mut().set_size(Some(500.0), None);
            self.editor.shape_as_needed(false);

            let on_focus = cx.action(|this, _| this.focused = true);
            let on_key = cx.listener(|this, key: KeyCode, acx| {
                let _ = this.editor.handle_key_with_mods(key, acx.modifiers());
            });
            let on_ptr = cx.listener(
                |this, (x, y, is_drag, now_ms, shift): (i32, i32, bool, u32, bool), _| {
                    this.focused = true;
                    if is_drag {
                        this.editor.handle_pointer_drag(x, y);
                    } else {
                        this.editor.handle_pointer_press(x, y, now_ms, shift);
                    }
                },
            );

            div()
                .size_full()
                .p(16)
                .child(
                    cx,
                    CodeEditor::from_edit(cx, 500, &*self.editor, self.focused)
                        .size(560, 240)
                        .status_bar(false)
                        .indent_guides(false)
                        .on_focus(on_focus)
                        .on_key_input(on_key)
                        .on_pointer_action(on_ptr),
                )
                .into_element(cx)
        }
    }

    let mut ed = SyntaxEditor::from_str(
        Metrics::new(13.0, 20.0),
        "fn compute_sum(x)",
        RustSyntaxHighlighter,
        SyntaxTheme::dark(),
    );
    ed.set_cursor(Cursor::new(0, 0));

    let mut engine = VeloxEngine::new(640, 320);
    let root = engine.mount_root(EditorHost {
        editor: Box::new(ed),
        focused: true,
    });
    let _ = engine.step_frame();

    // 记录光标在 (0, 0) 时所有文本 DrawOp 的 (x, y) 位置
    let text_positions_a: Vec<(i16, i16)> = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| matches!(op.payload, DrawPayload::TextRun { .. }))
        .map(|op| (op.bounds.x, op.bounds.y))
        .collect();
    assert!(!text_positions_a.is_empty());

    // 将光标移动到第 0 行中间（第 10 列），验证所有文字坐标 100% 纹丝不动（彻底告别旧版 2px 抖动）
    engine.view_mut(root).unwrap().editor.set_cursor(Cursor::new(0, 10));
    let _ = engine.step_frame();

    let text_positions_b: Vec<(i16, i16)> = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| matches!(op.payload, DrawPayload::TextRun { .. }))
        .map(|op| (op.bounds.x, op.bounds.y))
        .collect();
    assert_eq!(
        text_positions_a, text_positions_b,
        "moving cursor must never shift text glyph x/y positions"
    );

    // 选中单词 "compute_sum"，验证产生的是字形级局部选区矩形（远窄于整行 500px 宽度）
    let word_x = (measure_line_width_px("fn comp", 13) as i32) + 1;
    engine
        .view_mut(root)
        .unwrap()
        .editor
        .action(Action::DoubleClick { x: word_x, y: 10 });
    assert_eq!(
        engine
            .view(root)
            .unwrap()
            .editor
            .copy_selection()
            .unwrap()
            .as_str(),
        "compute_sum"
    );
    let _ = engine.step_frame();

    let sel_color = CoreTheme::dark().foreground.scale_alpha(48);
    let sel_rects: Vec<Region> = engine
        .cached_draw_ops()
        .iter()
        .filter_map(|op| match op.payload {
            DrawPayload::Quad { bg, .. } if bg == sel_color => Some(op.bounds),
            _ => None,
        })
        .collect();
    assert_eq!(sel_rects.len(), 1, "expected exactly 1 glyph-level selection rect");
    let expected_w = measure_line_width_px("compute_sum", 13);
    assert!(
        (sel_rects[0].w - expected_w).abs() <= 4,
        "selection rect width {} should match word width {}",
        sel_rects[0].w,
        expected_w
    );
    assert!(sel_rects[0].w < 200, "selection must not paint entire row width");

    // 验证悬停与拖选时鼠标光标始终保持 CursorShape::Text（而非 Grab/Grabbing）
    let editor_rect = engine
        .hit_regions()
        .iter()
        .find(|h| h.id == Some(500))
        .unwrap()
        .bounds;
    let cx_pt = editor_rect.x + 120;
    let cy_pt = editor_rect.y + 12;
    let _ = engine.dispatch_pointer_move(cx_pt, cy_pt);
    assert_eq!(engine.cursor_shape(), CursorShape::Text);
    assert!(engine.dispatch_pointer_down(cx_pt, cy_pt));
    assert_eq!(engine.cursor_shape(), CursorShape::Text);
    let _ = engine.dispatch_pointer_move(cx_pt + 40, cy_pt);
    assert_eq!(engine.cursor_shape(), CursorShape::Text);
    let _ = engine.dispatch_pointer_up(cx_pt + 40, cy_pt);
}

#[test]
fn test_textarea_from_edit_soft_wrap_renders_cleanly() {
    struct TaHost {
        editor: Box<CosmicEditor>,
    }
    impl View for TaHost {
        fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
            self.editor.buffer_mut().set_wrap(Wrap::WordOrGlyph);
            self.editor.buffer_mut().set_size(Some(180.0), None);
            self.editor.shape_as_needed(false);
            div()
                .size_full()
                .p(12)
                .child(
                    cx,
                    Textarea::from_edit(cx, 600, &*self.editor, true).size(200, 120),
                )
                .into_element(cx)
        }
    }

    let mut ed = CosmicEditor::from_str(
        Metrics::new(13.0, 20.0),
        "Velox cosmic-text powered multi-line textarea with automatic word wrapping.",
    );
    ed.set_cursor(Cursor::new_with_affinity(0, 12, Affinity::After));
    let mut engine = VeloxEngine::new(320, 200);
    let _ = engine.mount_root(TaHost {
        editor: Box::new(ed),
    });
    let _ = engine.step_frame();

    let text_ops = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| matches!(op.payload, DrawPayload::TextRun { .. }))
        .count();
    assert!(
        text_ops >= 2,
        "soft-wrapped textarea should render >= 2 visual lines, got {}",
        text_ops
    );
}
