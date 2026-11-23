//! `desktop_app`（**Velox Base Gallery**）跨平台桌面组件画廊端到端集成测试。
//!
//! 覆盖：
//! - `WinitPlatform` 与纯 `WaylandPlatform` (`xdg_toplevel`) 帧步进、0% CPU 休眠与缓冲区尺寸；
//! - 顶部栏明暗主题切换（`Dark` <-> `Light`）、浮动 `Notification` 弹出与关闭、模态 `Dialog` 遮罩拦截与 `Escape` 键关闭；
//! - 左侧分组 `Sidebar` 折叠/展开与全部 11 个 Story 故事页遍历渲染与核心交互（Button/Toggle/Checkbox/Radio/Switch/Slider/Inputs/Select/Tabs/Accordion/CodeEditor/Overlays/Feedback/DataTable/Settings）；
//! - 实时多语言热切换（`en-US` -> `zh-CN` -> `ja-JP`）、150% DPI 状态栏文本零裁剪与奇数 HiDPI 无斜切验证。

use desktop_app::{
    create_desktop_app, GalleryStory, DEFAULT_DESKTOP_HEIGHT, DEFAULT_DESKTOP_WIDTH,
};
use velox_kit::base::SortDirection;
use velox_kit::platform::wayland::WaylandPlatform;
use velox_kit::platform::winit::WinitPlatform;
use velox_kit::platform::{Platform, PlatformControlFlow, PlatformSurface};
use velox_kit::prelude::*;

/// 从当前帧引擎绘制指令中查找指定元素 `id` 的中心逻辑坐标 `(cx, cy)`。
fn hit_center(engine: &VeloxEngine, target_id: u32) -> (i16, i16) {
    let op = engine
        .cached_draw_ops()
        .iter()
        .rev()
        .find(|op| op.id == Some(target_id))
        .unwrap_or_else(|| panic!("expected draw op for id={target_id}"));
    (
        op.bounds.x + op.bounds.w / 2,
        op.bounds.y + op.bounds.h / 2,
    )
}

/// 从当前帧引擎绘制指令中查找包含指定文案的 `TextRun` 中心逻辑坐标 `(cx, cy)`。
fn text_center(engine: &VeloxEngine, label: &str) -> (i16, i16) {
    let op = engine
        .cached_draw_ops()
        .iter()
        .rev()
        .find(|op| matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == label))
        .unwrap_or_else(|| panic!("expected TextRun for label={label:?}"));
    (
        op.bounds.x + op.bounds.w / 2,
        op.bounds.y + op.bounds.h / 2,
    )
}

#[test]
fn test_gallery_winit_and_wayland_platforms_and_idle_sleep() {
    let (mut w_engine, _w_root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let mut winit_plat = WinitPlatform::new(
        "Velox Base Gallery Test",
        DEFAULT_DESKTOP_WIDTH as usize,
        DEFAULT_DESKTOP_HEIGHT as usize,
    )
    .with_dpi_scale(1);
    assert_eq!(
        winit_plat.surface_size(),
        (DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT)
    );
    assert_eq!(
        winit_plat.step_platform(&mut w_engine, 16),
        PlatformControlFlow::Poll
    );
    assert_eq!(
        winit_plat.step_platform(&mut w_engine, 16),
        PlatformControlFlow::Wait
    );

    let (mut wl_engine, _wl_root) =
        create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let mut wl_plat = WaylandPlatform::new_xdg_toplevel(
        "Velox Base Gallery",
        "dev.velox.gallery",
        DEFAULT_DESKTOP_WIDTH as usize,
        DEFAULT_DESKTOP_HEIGHT as usize,
    );
    assert_eq!(
        wl_plat.surface_size(),
        (DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT)
    );
    assert_eq!(
        wl_plat.step_platform(&mut wl_engine, 16),
        PlatformControlFlow::Poll
    );
    assert_eq!(
        wl_plat.step_platform(&mut wl_engine, 16),
        PlatformControlFlow::Wait
    );
    assert_eq!(
        wl_plat.front_pixels().len(),
        (DEFAULT_DESKTOP_WIDTH as usize) * (DEFAULT_DESKTOP_HEIGHT as usize)
    );
}

#[test]
fn test_gallery_toolbar_theme_toast_dialog_occlusion_and_escape() {
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame();
    assert_eq!(engine.view(root).unwrap().theme_mode, ThemeMode::Dark);
    assert!(!engine.view(root).unwrap().notification_open);
    assert!(!engine.view(root).unwrap().about_dialog_open);

    // 1. 点击 "Light Mode" (id=102) 切换明暗主题
    let (tx, ty) = hit_center(&engine, 102);
    assert!(engine.dispatch_click(tx, ty));
    assert_eq!(engine.view(root).unwrap().theme_mode, ThemeMode::Light);
    let d_theme = engine.step_frame();
    assert!(!d_theme.is_empty());

    // 2. 点击 "Toast" (id=101) 弹出右上角 Notification
    let (bx, by) = hit_center(&engine, 101);
    assert!(engine.dispatch_click(bx, by));
    assert!(engine.view(root).unwrap().notification_open);
    let _ = engine.step_frame();

    // 点击 Notification 右上角关闭按钮关闭通知 (Notification width=300, pr(16), pos(0,52))
    assert!(engine.dispatch_click(DEFAULT_DESKTOP_WIDTH - 36, 72));
    assert!(!engine.view(root).unwrap().notification_open);
    let _ = engine.step_frame();

    // 3. 点击 "About" (id=103) 打开模态 Dialog
    let (ax, ay) = hit_center(&engine, 103);
    assert!(engine.dispatch_click(ax, ay));
    assert!(engine.view(root).unwrap().about_dialog_open);
    let _ = engine.step_frame();

    // 模态打开时，点击底层左侧栏条目必须被遮罩拦截
    assert!(!engine.dispatch_click(100, 160));
    assert_eq!(engine.view(root).unwrap().active_story, GalleryStory::Overview);

    // 按 Escape 键直接关闭模态 Dialog！
    assert!(engine.dispatch_key(KeyCode::Escape));
    assert!(!engine.view(root).unwrap().about_dialog_open);
    let _ = engine.step_frame();

    // 4. 侧边栏折叠与展开（折叠按钮位于侧边栏头部右上角）
    assert!(engine.dispatch_click(200, 68));
    assert!(engine.view(root).unwrap().sidebar_collapsed);
    let _ = engine.step_frame();

    assert!(engine.dispatch_click(26, 68));
    assert!(!engine.view(root).unwrap().sidebar_collapsed);
    let _ = engine.step_frame();
}

#[test]
fn test_gallery_all_11_stories_navigation_and_interactive_controls() {
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame();

    let all_stories = [
        (GalleryStory::Overview, "Overview"),
        (GalleryStory::Buttons, "Button & Toggle"),
        (GalleryStory::Selection, "Checkbox & Radio"),
        (GalleryStory::Sliders, "Switch & Slider"),
        (GalleryStory::Inputs, "Inputs & Select"),
        (GalleryStory::Navigation, "Tabs & Accordion"),
        (GalleryStory::Editor, "Code Editor"),
        (GalleryStory::Overlays, "Dialog & Modal"),
        (GalleryStory::Feedback, "Alerts & Badges"),
        (GalleryStory::Data, "Table & List"),
        (GalleryStory::SettingsIcons, "Settings & Icons"),
    ];

    // 1. 通过真实点击左侧 Sidebar 遍历切换全部 11 个 Story，确保无 panic 且每页产出绘制指令
    for &(story, label) in &all_stories {
        let (sx, sy) = text_center(&engine, label);
        assert!(engine.dispatch_click(sx, sy), "failed clicking sidebar item {label}");
        assert_eq!(engine.view(root).unwrap().active_story, story);
        let _ = engine.step_frame();
        assert!(!engine.cached_draw_ops().is_empty());
    }

    // 2. 切回 Buttons Story 验证按钮点击
    let (sx, sy) = text_center(&engine, "Button & Toggle");
    assert!(engine.dispatch_click(sx, sy));
    let _ = engine.step_frame();
    let (px, py) = hit_center(&engine, 1101);
    assert!(engine.dispatch_click(px, py));
    assert_eq!(engine.view(root).unwrap().button_clicks, 1);

    // 3. 切到 Editor Story 验证键盘输入与 Undo/Redo
    let (sx, sy) = text_center(&engine, "Code Editor");
    assert!(engine.dispatch_click(sx, sy));
    let _ = engine.step_frame();

    let (ex, ey) = hit_center(&engine, 320);
    assert!(engine.dispatch_pointer_down(ex, ey));
    let _ = engine.dispatch_pointer_up(ex, ey);
    let _ = engine.step_frame();

    let prev_len = engine.view(root).unwrap().editor_buf.len();
    assert!(engine.dispatch_key(KeyCode::Enter));
    assert!(engine.dispatch_key(KeyCode::Char('o')));
    assert!(engine.dispatch_key(KeyCode::Char('k')));
    assert!(engine.view(root).unwrap().editor_buf.len() > prev_len);
    let after_type = String::from(
        engine
            .view(root)
            .unwrap()
            .editor_buf
            .to_flat_string::<4096>()
            .as_str(),
    );
    let _ = engine.step_frame();

    let (ux, uy) = hit_center(&engine, 310);
    assert!(engine.dispatch_click(ux, uy));
    assert_ne!(
        engine
            .view(root)
            .unwrap()
            .editor_buf
            .to_flat_string::<4096>()
            .as_str(),
        after_type
    );
    let _ = engine.step_frame();

    let (rx, ry) = hit_center(&engine, 311);
    assert!(engine.dispatch_click(rx, ry));
    assert_eq!(
        engine
            .view(root)
            .unwrap()
            .editor_buf
            .to_flat_string::<4096>()
            .as_str(),
        after_type
    );

    // 验证软折行开关与 Select All 按钮交互正常
    let (wx, wy) = hit_center(&engine, 313);
    assert!(engine.dispatch_click(wx, wy));
    let _ = engine.step_frame();
    let (sax, say) = hit_center(&engine, 314);
    assert!(engine.dispatch_click(sax, say));
    assert!(engine.view(root).unwrap().editor_buf.selected_byte_len() > 0);
    let _ = engine.step_frame();

    // 4. 切到 Overlays Story：打开 Destructive AlertDialog 并验证 Escape 关闭
    let (sx, sy) = text_center(&engine, "Dialog & Modal");
    assert!(engine.dispatch_click(sx, sy));
    let _ = engine.step_frame();

    let (dx, dy) = hit_center(&engine, 1602);
    assert!(engine.dispatch_click(dx, dy));
    assert!(engine.view(root).unwrap().demo_alert_dialog_open);
    let _ = engine.step_frame();
    assert!(engine.dispatch_key(KeyCode::Escape));
    assert!(!engine.view(root).unwrap().demo_alert_dialog_open);
    let _ = engine.step_frame();

    // 5. 切到 Data Story：验证 DataTable 表头排序切换与动态 resize
    let (sx, sy) = text_center(&engine, "Table & List");
    assert!(engine.dispatch_click(sx, sy));
    let _ = engine.step_frame();

    assert_eq!(engine.view(root).unwrap().table_sort_dir, SortDirection::Asc);
    let (hx, hy) = text_center(&engine, "Crate");
    assert!(engine.dispatch_click(hx, hy));
    assert_eq!(engine.view(root).unwrap().table_sort_dir, SortDirection::Desc);

    engine.resize(1280, 800);
    let resize_damage = engine.step_frame();
    assert_eq!(resize_damage, &[Region::new(0, 0, 1280, 800)]);
}

#[test]
fn test_gallery_overview_animation_settles_to_idle() {
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame_dt(16);
    assert!(!engine.needs_redraw(), "initial frame should settle");

    let (px, py) = hit_center(&engine, 305);
    assert!(engine.dispatch_click(px, py));
    assert_eq!(engine.view(root).unwrap().current_cpu_load_pct(), 52);

    let mut animated_frames = 0usize;
    let mut frames_needed = 0usize;
    for i in 0..60 {
        let dmg_len = engine.step_frame_dt(16).len();
        if dmg_len > 0 {
            animated_frames += 1;
        }
        if !engine.needs_redraw() {
            frames_needed = i + 1;
            break;
        }
    }
    assert!(
        animated_frames >= 2,
        "ProgressBar bounds transition should animate across multiple frames (got {animated_frames})"
    );
    assert!(
        frames_needed > 0 && frames_needed <= 20 && !engine.needs_redraw(),
        "Gallery must return to 0% CPU idle sleep after 200ms animation finishes (frames={frames_needed})"
    );
}

#[test]
fn test_gallery_odd_hidpi_resize_no_stride_shear() {
    use velox_kit::platform::winit::{SoftwareSurfaceDriver, WlSurface};

    let (mut engine, _root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    engine.set_dpi_scale(2);

    let mut driver = SoftwareSurfaceDriver::new(
        (DEFAULT_DESKTOP_WIDTH as usize) * 2,
        (DEFAULT_DESKTOP_HEIGHT as usize) * 2,
    );
    let mut wl = WlSurface::new();
    assert!(driver.flush_frame_dt(&mut engine, &mut wl, 16));

    for &(phys_w, phys_h) in &[(961usize, 641usize), (777usize, 513usize)] {
        let logical_w = phys_w.div_ceil(2) as i16;
        let logical_h = phys_h.div_ceil(2) as i16;
        engine.resize(logical_w, logical_h);
        driver.resize(phys_w, phys_h);
        wl = WlSurface::new();
        assert!(driver.flush_frame_dt(&mut engine, &mut wl, 16));

        let px = driver.front_pixels();
        assert_eq!(px.len(), phys_w * phys_h);

        let right_sample_top = px[120 * phys_w + (phys_w - 1)];
        let right_sample_mid = px[280 * phys_w + (phys_w - 1)];
        let right_sample_bot = px[440 * phys_w + (phys_w - 1)];
        assert_eq!(right_sample_top, right_sample_mid);
        assert_eq!(right_sample_mid, right_sample_bot);
        assert_ne!(right_sample_mid.0, 0xFF00_0000);
    }
}

#[test]
fn test_gallery_i18n_live_locale_switch() {
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame();
    assert_eq!(engine.view(root).unwrap().locale, Locale::EN_US);

    let (lx, ly) = hit_center(&engine, 104);
    assert!(engine.dispatch_click(lx, ly));
    assert_eq!(engine.view(root).unwrap().locale, Locale::ZH_CN);
    let dmg_zh = engine.step_frame();
    assert!(!dmg_zh.is_empty());
    let has_zh_title = engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "Velox Base 组件画廊总览")
    });
    assert!(has_zh_title, "expected Chinese gallery overview title");

    let (lx2, ly2) = hit_center(&engine, 104);
    assert!(engine.dispatch_click(lx2, ly2));
    assert_eq!(engine.view(root).unwrap().locale, Locale::JA_JP);
    let _ = engine.step_frame();
    let has_ja_title = engine.cached_draw_ops().iter().any(|op| {
        matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "Velox Base コンポーネントギャラリー")
    });
    assert!(has_ja_title, "expected Japanese gallery overview title");

    let (lx3, ly3) = hit_center(&engine, 104);
    assert!(engine.dispatch_click(lx3, ly3));
    assert_eq!(engine.view(root).unwrap().locale, Locale::EN_US);
    let _ = engine.step_frame();
}

#[test]
fn test_gallery_150_dpi_status_bar_text_no_clipping() {
    let (mut engine, _root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    engine.set_scale_factor(1.5);
    let _ = engine.step_frame();
    let mut checked = 0usize;
    for op in engine.cached_draw_ops() {
        if let DrawPayload::TextRun {
            text, font_size, ..
        } = op.payload
        {
            let s = text.as_str();
            if s.contains("MPL-2.0")
                || s == "Story: Overview"
                || s.contains("Theme: Dark")
            {
                let fs15 = (((font_size as f32) * 1.5).round() as u32) as u8;
                let (w15, _) = velox_kit::core::display::measure_text_px(s, fs15);
                let clip_w_px = ((op.clip.w as f32) * 1.5).round() as i16;
                assert!(
                    clip_w_px >= w15,
                    "1.5x parent clip for {s:?} must fit full scaled text width (clip_w_px={clip_w_px}, w15={w15})"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 3, "expected all 3 status bar text runs verified");
}

#[test]
fn test_gallery_code_editor_cosmic_text_zero_jitter_and_wrap() {
    use velox_kit::base::{Action, Cursor, Edit, EditSelection, Wrap};

    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    engine.view_mut(root).unwrap().active_story = GalleryStory::Editor;
    let _ = engine.step_frame();

    // 1. 光标移动前后，所有文本坐标必须 100% 纹丝不动（0 抖动）
    let pos_a: Vec<(i16, i16)> = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| matches!(op.payload, DrawPayload::TextRun { .. }))
        .map(|op| (op.bounds.x, op.bounds.y))
        .collect();
    assert!(pos_a.len() > 25);

    engine
        .view_mut(root)
        .unwrap()
        .editor_buf
        .set_cursor(Cursor::new(8, 12));
    let _ = engine.step_frame();
    let pos_b: Vec<(i16, i16)> = engine
        .cached_draw_ops()
        .iter()
        .filter(|op| matches!(op.payload, DrawPayload::TextRun { .. }))
        .map(|op| (op.bounds.x, op.bounds.y))
        .collect();
    assert_eq!(pos_a, pos_b, "cursor movement must not shift text positions");

    // 2. 双击第 8 行 "GalleryCounter" 验证字形级精准选区矩形
    let gc_x = (velox_kit::core::display::measure_line_width_px("pub struct Gal", 13) as i32) + 2;
    let gc_y = 8 * 21 + 10;
    engine
        .view_mut(root)
        .unwrap()
        .editor_buf
        .action(Action::DoubleClick { x: gc_x, y: gc_y });
    assert_eq!(
        engine
            .view(root)
            .unwrap()
            .editor_buf
            .copy_selection()
            .unwrap()
            .as_str(),
        "GalleryCounter"
    );
    let _ = engine.step_frame();
    let sel_bg = CoreTheme::dark().primary.scale_alpha(78);
    let sel_quads: Vec<Region> = engine
        .cached_draw_ops()
        .iter()
        .filter_map(|op| match op.payload {
            DrawPayload::Quad { bg, .. } if bg == sel_bg => Some(op.bounds),
            _ => None,
        })
        .collect();
    assert_eq!(sel_quads.len(), 1);
    assert!(sel_quads[0].w > 50 && sel_quads[0].w < 160);

    // 3. 开启软折行后，超长行自动折为多条视觉行并保持选区渲染正常
    engine
        .view_mut(root)
        .unwrap()
        .editor_buf
        .buffer_mut()
        .set_wrap(Wrap::WordOrGlyph);
    engine
        .view_mut(root)
        .unwrap()
        .editor_buf
        .buffer_mut()
        .set_size(Some(420.0), None);
    engine
        .view_mut(root)
        .unwrap()
        .editor_buf
        .set_selection(EditSelection::Normal(Cursor::new(1, 3)));
    engine
        .view_mut(root)
        .unwrap()
        .editor_buf
        .set_cursor(Cursor::new(2, 24));
    let _ = engine.step_frame();
    assert!(
        engine
            .view_mut(root)
            .unwrap()
            .editor_buf
            .buffer_mut()
            .total_visual_lines()
            > engine.view(root).unwrap().editor_buf.line_count()
    );
}

#[test]
fn test_gallery_text_input_blink_alignment_drag_select_and_context_menu() {
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    engine.view_mut(root).unwrap().active_story = GalleryStory::Inputs;
    let _ = engine.step_frame_dt(16);

    // 1. 严格对齐验证：Clearable Input (1401)、Password Masked (1402)、NumberInput (1403)、Disabled (1404) 顶部与底部 100% 齐平
    let box_bounds = |eng: &VeloxEngine, id: u32| -> Region {
        eng.cached_draw_ops()
            .iter()
            .find(|op| op.id == Some(id))
            .unwrap_or_else(|| panic!("missing draw op id={id}"))
            .bounds
    };
    let b1 = box_bounds(&engine, 1401);
    let b2 = box_bounds(&engine, 1402);
    let b3 = box_bounds(&engine, 1403);
    let b4 = box_bounds(&engine, 1404);
    assert_eq!(b1.y, b2.y, "Clearable Input and Password Masked top must align");
    assert_eq!(b1.h, b2.h, "Clearable Input and Password Masked height must align");
    assert_eq!(b1.y, b3.y, "TextInput and NumberInput top must align");
    assert_eq!(b1.y, b4.y, "Active and Disabled TextInput top must align");

    // 2. 光标零假缝隙验证：光标 X 精确紧贴 "velox-base" 末尾（误差 <= 1px）
    let text_bounds = engine
        .cached_draw_ops()
        .iter()
        .find(|op| matches!(op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == "velox-base"))
        .expect("expected velox-base text op")
        .bounds;
    let caret_id = 1401u32 ^ 0x4355_5253;
    let caret_bounds = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(caret_id))
        .expect("expected blinking caret visible at t=16ms")
        .bounds;
    let expected_caret_x = text_bounds.x
        + velox_kit::core::display::measure_line_width_px("velox-base", 13);
    assert!(
        (caret_bounds.x - expected_caret_x).abs() <= 1,
        "caret x={} must sit flush after text end x={}",
        caret_bounds.x,
        expected_caret_x
    );

    // 3. 光标 530ms 闪烁周期验证：亮 -> 灭 -> 亮
    let has_caret = |eng: &VeloxEngine| -> bool {
        eng.cached_draw_ops()
            .iter()
            .any(|op| op.id == Some(caret_id))
    };
    assert!(has_caret(&engine), "caret should be visible initially");
    // 推进 540ms 进入暗相位
    for _ in 0..6 {
        let _ = engine.step_frame_dt(90);
    }
    assert!(!has_caret(&engine), "caret should blink off after 530ms");
    // 再推进 540ms 回到亮相位
    for _ in 0..6 {
        let _ = engine.step_frame_dt(90);
    }
    assert!(has_caret(&engine), "caret should blink back on after 1060ms");

    // 4. 鼠标任意拖拽选择验证：从 "ve" 后拖到 "velox" 末尾，精确选中 "lox"
    let drag_start_x =
        text_bounds.x + velox_kit::core::display::measure_line_width_px("ve", 13);
    let drag_end_x =
        text_bounds.x + velox_kit::core::display::measure_line_width_px("velox", 13);
    let mid_y = b1.y + b1.h / 2;
    assert!(engine.dispatch_pointer_down(drag_start_x, mid_y));
    assert!(engine.dispatch_pointer_move(drag_end_x, mid_y));
    let _ = engine.dispatch_pointer_up(drag_end_x, mid_y);
    assert_eq!(
        engine.view(root).unwrap().text_input_name.selected_text(),
        "lox"
    );
    let _ = engine.step_frame_dt(16);

    // 5. 鼠标右击弹出上下文菜单（保留选区），点击 Copy 再点击末尾右键 Paste
    assert!(engine.dispatch_pointer_secondary_down(drag_end_x, mid_y));
    assert!(engine.view(root).unwrap().input_ctx_menu.is_some());
    assert_eq!(
        engine.view(root).unwrap().text_input_name.selected_text(),
        "lox",
        "right-click must preserve active selection for copy/cut"
    );
    let _ = engine.step_frame_dt(16);

    let (copy_x, copy_y) = text_center(&engine, "Copy");
    assert!(engine.dispatch_click(copy_x, copy_y));
    assert!(engine.view(root).unwrap().input_ctx_menu.is_none());
    let _ = engine.step_frame_dt(16);

    // 点到末尾后右键 Paste，文本应精确追加 "lox" -> "velox-baselox"
    let end_x = b1.x + b1.w - 36;
    assert!(engine.dispatch_pointer_down(end_x, mid_y));
    let _ = engine.dispatch_pointer_up(end_x, mid_y);
    let _ = engine.step_frame_dt(16);

    assert!(engine.dispatch_pointer_secondary_down(end_x, mid_y));
    let _ = engine.step_frame_dt(16);
    let (paste_x, paste_y) = text_center(&engine, "Paste");
    assert!(engine.dispatch_click(paste_x, paste_y));
    assert_eq!(
        engine.view(root).unwrap().text_input_name.as_str(),
        "velox-baselox"
    );

    // 6. 快捷键验证（右键粘贴后焦点保持不丢失）：Ctrl+A + Ctrl+X 剪切整串后 Ctrl+V 还原
    let _ = engine.step_frame_dt(16);
    let _ = engine.dispatch_event(InputEvent::ModifiersChanged(KeyModifiers::CTRL));
    assert!(engine.dispatch_key(KeyCode::Char('a')));
    assert_eq!(
        engine.view(root).unwrap().text_input_name.selected_text(),
        "velox-baselox"
    );
    assert!(engine.dispatch_key(KeyCode::Char('x')));
    assert_eq!(engine.view(root).unwrap().text_input_name.as_str(), "");
    assert!(engine.dispatch_key(KeyCode::Char('v')));
    assert_eq!(
        engine.view(root).unwrap().text_input_name.as_str(),
        "velox-baselox"
    );
    let _ = engine.dispatch_event(InputEvent::ModifiersChanged(KeyModifiers::NONE));
}

#[test]
fn test_gallery_150dpi_caret_precision_and_zero_drift() {
    use velox_kit::core::display::measure_line_width_f32;
    use velox_kit::core::renderers::wgpu::WgpuOffscreenRenderer;

    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    engine.view_mut(root).unwrap().active_story = GalleryStory::Inputs;
    let _ = engine.step_frame_dt(16);

    let b1 = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(1401))
        .expect("input 1401")
        .bounds;

    let sf: f32 = 1.5;
    let pw = ((DEFAULT_DESKTOP_WIDTH as f32) * sf).round() as usize;
    let ph = ((DEFAULT_DESKTOP_HEIGHT as f32) * sf).round() as usize;
    let mut offscreen = WgpuOffscreenRenderer::new(pw, ph);
    if !offscreen.ensure_gpu() {
        return;
    }

    let x_min = ((b1.x + 6) as f32 * sf).round() as usize;
    let x_max = ((b1.x + b1.w - 30) as f32 * sf).round() as usize;
    let y_min = ((b1.y + 4) as f32 * sf).round() as usize;
    let y_max = ((b1.y + b1.h - 4) as f32 * sf).round() as usize;

    let measure_frame = |eng: &mut VeloxEngine,
                         off: &mut WgpuOffscreenRenderer,
                         txt: &str,
                         cur: usize|
     -> (usize, usize, usize) {
        {
            let v = eng.view_mut(root).unwrap();
            v.active_input_field = 0;
            v.text_input_name = velox_kit::base::EditableText::from_str(txt);
            v.text_input_name.set_selection(cur, cur);
            v.text_input_name.reset_blink(0);
        }
        let _ = eng.step_frame_dt(16);
        assert!(off.render_draw_ops_scaled(eng.cached_draw_ops(), eng.viewport_region(), sf));
        let fb = &off.rgba_buffer;
        let mut text_max_x = 0usize;
        let mut caret_min_x = usize::MAX;
        let mut caret_max_x = 0usize;
        for y in y_min..y_max {
            for x in x_min..x_max {
                let o = (y * pw + x) * 4;
                let (r, g, b) = (fb[o], fb[o + 1], fb[o + 2]);
                if r > 170 && g > 170 && b > 170 {
                    text_max_x = text_max_x.max(x);
                }
                if b > 180 && r < 95 && g > 100 {
                    caret_min_x = caret_min_x.min(x);
                    caret_max_x = caret_max_x.max(x);
                }
            }
        }
        assert!(caret_min_x <= caret_max_x, "caret pixels must be visible");
        (text_max_x, caret_min_x, caret_max_x)
    };

    // 1. 短串与 19 字符长串末尾光标：150% DPI 下 0.25px 精确字号零累积漂移，且光标恒定 2px 物理宽
    for txt in ["velox-base", "velox-base-gui-rust"] {
        let (text_max_x, cmin, cmax) =
            measure_frame(&mut engine, &mut offscreen, txt, txt.len());
        let caret_op = engine
            .cached_draw_ops()
            .iter()
            .find(|op| op.id == Some(1401u32 ^ 0x4355_5253))
            .expect("caret op");
        let caret_w = cmax - cmin + 1;
        assert_eq!(
            caret_w, 2,
            "1px logical caret at 150% DPI must be stable 2px (txt={txt:?}, op={:?}, clip={:?}, cmin={cmin}, cmax={cmax})",
            caret_op.bounds, caret_op.clip
        );
        assert!(
            cmin >= text_max_x && cmin - text_max_x <= 5,
            "150% DPI caret drifted after {:?}: text_max_x={text_max_x}, caret=[{cmin}..={cmax}]",
            txt
        );
    }

    // 2. 中间光标 `velox-ba|se`：光标中心精确贴合逻辑浮点位置 * 1.5（误差 <= 2px，绝不劈中 's'）
    let (_, cmin, cmax) = measure_frame(&mut engine, &mut offscreen, "velox-base", 8);
    let content_x0 = ((b1.x + 10) as f32) * sf;
    let expected_px = content_x0 + measure_line_width_f32("velox-ba", 13) * sf;
    let caret_center = 0.5 * ((cmin + cmax) as f32);
    assert!(
        (caret_center - expected_px).abs() <= 2.5,
        "150% DPI mid-word caret misaligned: center={caret_center:.1}, expected={expected_px:.1}"
    );
}

#[test]
fn test_gallery_inputs_gpui_base_calendar_slots_and_view_switching() {
    use velox_kit::base::{CalendarViewMode, DateSelection, SimpleDate};
    use velox_kit::core::motion::DrawPayload;

    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, 1100);
    engine.view_mut(root).unwrap().active_story = GalleryStory::Inputs;
    let _ = engine.step_frame_dt(16);

    let find_text_center = |eng: &velox_kit::core::engine::VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle))
            .unwrap_or_else(|| panic!("expected text run {needle:?}"));
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    // 1. 验证 gpui-kit 风格无样式 Calendar 外层尺寸 250px 与自定义文案 ("Nov" / "2026" / "‹" / "›" / "Su")
    let cal_op = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(1420))
        .expect("gpui-base Calendar outer box 1420 must exist");
    assert_eq!(cal_op.bounds.w, 250);
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, font_size: 11, .. } if text.as_str() == "Su"
    )));
    assert!(engine.cached_draw_ops().iter().any(|op| matches!(
        &op.payload,
        DrawPayload::TextRun { text, .. } if text.as_str() == "‹"
    )));

    // 2. 点击日期 20 -> 选中 2026-11-20
    let (d20x, d20y) = find_text_center(&engine, "20");
    assert!(engine.dispatch_click(d20x, d20y));
    assert_eq!(
        engine.view(root).unwrap().calendar_state.selection,
        DateSelection::Single(SimpleDate::new(2026, 11, 20))
    );
    let _ = engine.step_frame_dt(16);

    // 3. 点击月份标题 "Nov" 切到 Months 视图，再点击 "Jan" 切回 Days 且 view_month == 1
    let (mx, my) = find_text_center(&engine, "Nov");
    assert!(engine.dispatch_click(mx, my));
    assert_eq!(
        engine.view(root).unwrap().calendar_state.view_mode,
        CalendarViewMode::Months
    );
    let _ = engine.step_frame_dt(16);

    let (jx, jy) = find_text_center(&engine, "Jan");
    assert!(engine.dispatch_click(jx, jy));
    assert_eq!(
        engine.view(root).unwrap().calendar_state.view_mode,
        CalendarViewMode::Days
    );
    assert_eq!(engine.view(root).unwrap().calendar_state.view_month, 1);
    let _ = engine.step_frame_dt(16);

    // 4. 点击年份标题 "2026" 切到 Years 视图，再点击 "2025" 更新年份并切回 Days
    let (yx, yy) = find_text_center(&engine, "2026");
    assert!(engine.dispatch_click(yx, yy));
    assert_eq!(
        engine.view(root).unwrap().calendar_state.view_mode,
        CalendarViewMode::Years
    );
    let _ = engine.step_frame_dt(16);

    let (y25x, y25y) = find_text_center(&engine, "2025");
    assert!(engine.dispatch_click(y25x, y25y));
    assert_eq!(engine.view(root).unwrap().calendar_state.view_year, 2025);
    assert_eq!(
        engine.view(root).unwrap().calendar_state.view_mode,
        CalendarViewMode::Days
    );
}

#[test]
fn test_gallery_reactive_subviews_hit_l2_partial_relayout_without_root_rerender() {
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame_dt(16);

    let status_h = engine.view(root).unwrap().status_bar_view.expect("StatusBarView mounted");
    let pg_h = engine.view(root).unwrap().overview_playground.expect("OverviewPlaygroundView mounted");
    let signals = engine.view(root).unwrap().signals;

    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(status_h), 1);
    assert_eq!(engine.pod_render_count(pg_h), 1);

    // 1. 点击 Overview 体验台的 +10% 按钮（通过 batch 同时更新 cpu_load_pct 与 event_count 信号）
    let (inc_x, inc_y) = hit_center(&engine, 305);
    assert!(engine.dispatch_click(inc_x, inc_y));
    assert_eq!(signals.cpu_load_pct.get_untracked(), 52);
    assert_eq!(signals.event_count.get_untracked(), 1);
    assert!(!signals.high_load_alert.get());
    let dmg1 = engine.step_frame_dt(16);
    assert!(!dmg1.is_empty());

    // 核心断言：根视图 GalleryApp 0 次重跑 render()，仅 Playground 与 StatusBar 命中 L2 原地局部重排！
    assert_eq!(
        engine.pod_render_count(root),
        1,
        "Root GalleryApp must NOT re-render when OverviewPlaygroundView signal updates"
    );
    assert_eq!(engine.pod_render_count(pg_h), 2);
    assert_eq!(engine.pod_render_count(status_h), 2);
    assert!(
        engine.partial_relayout_count() >= 1,
        "Expected L2 in-place partial relayout hit"
    );
    assert!(engine.subview_cache_hits() >= 1);

    // 推进 ProgressBar 过渡动画至结束，验证 L1 Pass-4-Only 快路径且最终回到 L0 0% CPU 休眠
    let fast_before = engine.fast_paint_pass_count();
    for _ in 0..25 {
        let _ = engine.step_frame_dt(16);
        if !engine.needs_redraw() {
            break;
        }
    }
    assert!(!engine.needs_redraw(), "must settle to L0 idle sleep");
    assert!(
        engine.fast_paint_pass_count() > fast_before,
        "ProgressBar transition should run on L1 Pass-4-Only fast path"
    );
    assert_eq!(engine.pod_render_count(root), 1);

    // 2. 点击 Switch::bind (id=306) 切换 VSync：仅 Playground 子视图重渲染，Root 与 StatusBar 均为 0 重跑！
    let (sw_x, sw_y) = text_center(&engine, "VSync");
    assert!(engine.dispatch_click(sw_x, sw_y));
    assert!(!signals.vsync_enabled.get_untracked());
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(status_h), 2);
    assert_eq!(engine.pod_render_count(pg_h), 3);

    // 3. 点击 Checkbox::bind (id=307) 切换 Damage Tracking：同样 0 次重跑 Root 与 StatusBar
    let (cb_x, cb_y) = text_center(&engine, "Damage Tracking");
    assert!(engine.dispatch_click(cb_x, cb_y));
    assert!(!signals.damage_overlay_hint.get_untracked());
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(status_h), 2);
    assert_eq!(engine.pod_render_count(pg_h), 4);
    assert!(engine.partial_relayout_count() >= 3);

    // 4. 点击 Slider::bind (id=303) 调整帧率预算：验证双向绑定与零根视图重渲染
    let sl_region = engine
        .hit_regions()
        .iter()
        .find(|h| h.id == Some(303))
        .expect("expected slider hit region id=303")
        .bounds;
    let sl_x = sl_region.x + sl_region.w - 20;
    let sl_y = sl_region.y + sl_region.h / 2;
    assert!(engine.dispatch_pointer_down(sl_x, sl_y));
    let _ = engine.dispatch_pointer_up(sl_x, sl_y);
    assert_ne!(signals.frame_budget_pct.get_untracked(), 65);
    let _ = engine.step_frame_dt(16);
    assert_eq!(engine.pod_render_count(root), 1);
    assert_eq!(engine.pod_render_count(pg_h), 5);
    assert!(engine.partial_relayout_count() >= 4);
}

#[test]
fn test_software_vs_gpu_first_paint_pixel_parity() {
    use velox_kit::core::display::Xrgb8888;
    use velox_kit::core::renderers::wgpu::WgpuOffscreenRenderer;

    let mut offscreen = WgpuOffscreenRenderer::new(100, 100);
    if !offscreen.ensure_gpu() {
        return;
    }

    let assert_scene_parity = |label: &str, engine: &mut VeloxEngine, off: &mut WgpuOffscreenRenderer, sf: f32| {
        engine.set_scale_factor(sf);
        let vp = engine.viewport_region();
        let pw = ((vp.w as f32) * sf).round() as usize;
        let ph = ((vp.h as f32) * sf).round() as usize;

        let mut sw_buf = vec![Xrgb8888(0xFF00_0000); pw * ph];
        engine.rasterize_region(vp, &mut sw_buf);

        let ops = engine.cached_draw_ops();
        assert!(off.render_draw_ops_scaled(ops, vp, sf));
        let gpu_rgba = &off.rgba_buffer;

        let mut diff_gt_4 = 0usize;
        let mut max_diff = 0u8;
        for idx in 0..(pw * ph) {
            let sc = sw_buf[idx].to_color();
            let o = idx * 4;
            let d = sc
                .r()
                .abs_diff(gpu_rgba[o])
                .max(sc.g().abs_diff(gpu_rgba[o + 1]))
                .max(sc.b().abs_diff(gpu_rgba[o + 2]));
            if d > max_diff {
                max_diff = d;
            }
            if d > 4 {
                diff_gt_4 += 1;
            }
        }
        assert_eq!(
            diff_gt_4, 0,
            "[{label} @ sf={sf:.2}] software first-paint and GPU must match pixel-for-pixel (diff_gt_4={diff_gt_4}, max_diff={max_diff})"
        );
    };

    // 1. Overview 首屏在 1.0x / 1.25x / 1.5x / 1.75x / 2.0x 全档位 DPI 下逐像素零跳变
    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame_dt(16);
    for sf in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
        assert_scene_parity("Overview-Dark", &mut engine, &mut offscreen, sf);
    }

    // 2. Overlays + 亮色主题 + 弹出带高斯软阴影的 Dialog（1.5x 分数 DPI）
    engine.set_theme(ThemeMode::Light.to_theme());
    {
        let v = engine.view_mut(root).unwrap();
        v.theme_mode = ThemeMode::Light;
        v.active_story = GalleryStory::Overlays;
        v.demo_dialog_open = true;
    }
    let _ = engine.step_frame_dt(16);
    assert_scene_parity("Overlays-Light-DialogShadow", &mut engine, &mut offscreen, 1.5);

    // 3. Code Editor（多行代码 + 1px 缩进参考线 + 光标，1.5x 分数 DPI）
    engine.set_theme(ThemeMode::Dark.to_theme());
    {
        let v = engine.view_mut(root).unwrap();
        v.theme_mode = ThemeMode::Dark;
        v.active_story = GalleryStory::Editor;
        v.demo_dialog_open = false;
    }
    let _ = engine.step_frame_dt(16);
    assert_scene_parity("Editor-Dark", &mut engine, &mut offscreen, 1.5);
}

#[test]
fn test_gallery_async_task_subview_lifecycle_cancel_and_l2_isolation() {
    use desktop_app::{AsyncTaskStatus, DEFAULT_DESKTOP_HEIGHT, DEFAULT_DESKTOP_WIDTH};
    use velox_kit::core::motion::DrawPayload;

    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    let _ = engine.step_frame_dt(16);

    let async_h = engine
        .view(root)
        .unwrap()
        .async_task_view
        .expect("AsyncTaskView must be mounted");
    let status_h = engine
        .view(root)
        .unwrap()
        .status_bar_view
        .expect("StatusBarView must be mounted");

    engine
        .view_mut(async_h)
        .unwrap()
        .set_step_delay(Duration::from_millis(12));
    assert_eq!(engine.pod_render_count(root), 1);

    let text_center = |eng: &VeloxEngine, needle: &str| -> (i16, i16) {
        let op = eng
            .cached_draw_ops()
            .iter()
            .find(|op| {
                matches!(&op.payload, DrawPayload::TextRun { text, .. } if text.as_str() == needle)
            })
            .unwrap_or_else(|| panic!("expected visible text {needle:?}"));
        (op.bounds.x + op.bounds.w / 2, op.bounds.y + op.bounds.h / 2)
    };

    // 1. 点击 "Run Async Sync" 启动后台 async/await 流水线
    let (run_x, run_y) = text_center(&engine, "Run Async Sync");
    assert!(engine.dispatch_click(run_x, run_y));
    assert!(matches!(
        engine.view(async_h).unwrap().status,
        AsyncTaskStatus::Running { .. }
    ));

    // 2. 持续步进帧直至后台 async 任务完成并完成进度条过渡、自动回归 0% CPU 休眠
    let mut settled = false;
    for _ in 0..200 {
        let _ = engine.step_frame_dt(16);
        if matches!(
            engine.view(async_h).unwrap().status,
            AsyncTaskStatus::Completed { .. }
        ) && !engine.needs_redraw()
        {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(settled, "async task must complete and settle back to 0% CPU idle");
    assert_eq!(engine.view(async_h).unwrap().completed_runs, 1);
    // 整个异步执行与完成通知期间，根视图 GalleryApp 0 次重跑，仅叶子 AsyncTaskView 与 StatusBarView 局部刷新
    assert_eq!(engine.pod_render_count(root), 1);
    assert!(engine.pod_render_count(status_h) >= 2);
    assert!(engine.partial_relayout_count() >= 1);

    // 3. 再次启动后立刻点击 "Cancel Task" 验证协作式取消并回归静止休眠
    engine
        .view_mut(async_h)
        .unwrap()
        .set_step_delay(Duration::from_millis(120));
    assert!(engine.dispatch_click(run_x, run_y));
    let _ = engine.step_frame_dt(16);
    assert!(matches!(
        engine.view(async_h).unwrap().status,
        AsyncTaskStatus::Running { .. }
    ));

    let (cancel_x, cancel_y) = text_center(&engine, "Cancel Task");
    assert!(engine.dispatch_click(cancel_x, cancel_y));
    assert_eq!(engine.view(async_h).unwrap().status, AsyncTaskStatus::Cancelled);
    assert_eq!(
        engine.active_async_task_count(),
        0,
        "RAII Drop of Task must immediately cancel and reclaim foreground coroutine"
    );

    for _ in 0..30 {
        let _ = engine.step_frame_dt(16);
        if !engine.needs_redraw() {
            break;
        }
    }
    assert!(!engine.needs_redraw(), "cancelled async task must return to idle sleep");
    assert_eq!(engine.active_async_task_count(), 0);
    assert_eq!(engine.pod_render_count(root), 1);
}

#[test]
fn test_gallery_async_platform_driver_demo() {
    use desktop_app::{block_on, run_async_platform_demo};
    use velox_kit::platform::PlatformControlFlow;

    let report = block_on(run_async_platform_demo(Duration::from_millis(10)));
    assert_eq!(report.completed_runs, 1);
    assert_eq!(report.root_render_count, 1);
    assert_eq!(report.final_control_flow, PlatformControlFlow::Wait);
    assert!(report.frames_polled >= 2);
}

#[test]
fn test_gallery_async_task_view_runs_deterministically_on_manual_scheduler() {
    use desktop_app::{
        mount_desktop_app, AsyncTaskStatus, GalleryApp, DEFAULT_DESKTOP_HEIGHT,
        DEFAULT_DESKTOP_WIDTH,
    };

    // 同一份 GalleryApp / AsyncTaskView 业务代码，零修改切换至确定性虚拟时钟 ManualScheduler！
    let manual = ManualScheduler::new();
    let mut engine = VeloxEngine::new(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT)
        .with_scheduler(manual.clone());
    assert_eq!(engine.scheduler_name(), "velox-manual");

    let root = mount_desktop_app(&mut engine, GalleryApp::new());
    let async_h = engine
        .view(root)
        .unwrap()
        .async_task_view
        .expect("AsyncTaskView mounted");
    let _ = engine.step_frame_dt(16);

    // 点击 "Run Async Sync"（默认每阶段 180ms × 3 = 540ms），用虚拟时钟瞬间零真睡推进完成
    let run_btn = engine
        .cached_draw_ops()
        .iter()
        .find(|op| op.id == Some(351))
        .expect("Run Async Sync button")
        .bounds;
    assert!(engine.dispatch_click(run_btn.x + run_btn.w / 2, run_btn.y + run_btn.h / 2));

    for expected_pct in [25u8, 55, 85] {
        manual.drive_engine(&mut engine);
        assert!(matches!(
            engine.view(async_h).unwrap().status,
            AsyncTaskStatus::Running { progress_pct, .. } if progress_pct == expected_pct
        ));
        let _ = manual.advance(Duration::from_millis(180));
    }

    manual.drive_engine(&mut engine);
    assert!(matches!(
        engine.view(async_h).unwrap().status,
        AsyncTaskStatus::Completed {
            crates_synced: 4,
            ..
        }
    ));
    assert_eq!(engine.view(async_h).unwrap().completed_runs, 1);
    assert_eq!(engine.active_async_task_count(), 0);
    assert_eq!(engine.pod_render_count(root), 1);
}

#[test]
fn test_gallery_tokio_scheduler_and_reqwest_http_get_loopback() {
    use desktop_app::{block_on, run_tokio_http_demo};
    use velox_kit::platform::PlatformControlFlow;

    let report = block_on(run_tokio_http_demo());
    assert_eq!(report.scheduler_name, "tokio");
    assert_eq!(report.status_code, 200);
    assert!(report.bytes > 20);
    assert!(report.preview.contains("velox"));
    assert_eq!(
        report.root_render_count, 1,
        "Tokio + reqwest HTTP response update must hit L2 partial relayout without re-rendering root GalleryApp"
    );
    assert_eq!(report.final_control_flow, PlatformControlFlow::Wait);
}

#[test]
fn test_gallery_tokio_reqwest_in_flight_cancel_by_task_drop() {
    use desktop_app::async_demo::spawn_loopback_http_server;
    use desktop_app::{AsyncTaskStatus, TokioScheduler};

    let tokio_sched = TokioScheduler::global();
    // 故意延迟 3 秒的本地服务端，验证飞行中的 reqwest HTTP 请求可被 RAII Task::drop 立即取消
    let slow_url = spawn_loopback_http_server(
        &tokio_sched,
        r#"{"slow":true}"#,
        Duration::from_secs(3),
    );

    let (mut engine, root) = create_desktop_app(DEFAULT_DESKTOP_WIDTH, DEFAULT_DESKTOP_HEIGHT);
    assert_eq!(engine.scheduler_name(), "tokio");
    let async_h = engine
        .view(root)
        .unwrap()
        .async_task_view
        .expect("AsyncTaskView mounted");
    engine.view_mut(async_h).unwrap().set_http_url(slow_url);
    let _ = engine.step_frame_dt(16);

    let started = std::time::Instant::now();
    // 1. 点击 "HTTP GET" (id = 355) 发起飞行中请求
    let (hx, hy) = hit_center(&engine, 355);
    assert!(engine.dispatch_click(hx, hy));
    assert!(matches!(
        engine.view(async_h).unwrap().status,
        AsyncTaskStatus::HttpPending { .. }
    ));
    let _ = engine.step_frame_dt(16);

    // 2. 立即点击 "Cancel Task" (id = 352) 触发 self.sync_task = None RAII 取消
    let (cx, cy) = hit_center(&engine, 352);
    assert!(engine.dispatch_click(cx, cy));
    assert_eq!(engine.view(async_h).unwrap().status, AsyncTaskStatus::Cancelled);
    assert_eq!(engine.active_async_task_count(), 0);

    for _ in 0..30 {
        let _ = engine.step_frame_dt(16);
        if !engine.needs_redraw() {
            break;
        }
    }
    assert!(!engine.needs_redraw(), "cancelled HTTP task must return to 0% CPU Wait");
    assert_eq!(engine.pod_render_count(root), 1);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "RAII cancellation must abort immediately instead of waiting for slow 3s HTTP server"
    );
}
