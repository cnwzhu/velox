# 输入事件、焦点与文本编辑引擎（Events / Focus / EditableText / TextBuffer / Navigator）

**Contents:** [统一输入事件与手势参数](#统一输入事件与手势参数) · [事件绑定：`cx.listener` 与 `cx.action`](#事件绑定cxlistener-与-cxaction) · [焦点链与按键导航](#焦点链与按键导航) · [文本编辑状态机：EditableText 与 TextBuffer](#文本编辑状态机editabletext-与-textbuffer) · [页面路由栈 Navigator](#页面路由栈-navigator)

---

## 统一输入事件与手势参数

`velox_engine::pod::InputEvent` 抽象全平台输入来源（鼠标、触摸屏、手写笔、滚轮、实体键盘、IME 与系统返回导航）：

- **指针与触控**：`PointerDown { x, y }`、`PointerMove { x, y }`、`PointerUp { x, y }`、`PointerSecondaryDown/Up`、`PointerLeave`、`PointerCancel`、`PointerKindChanged(PointerKind)`
- **滚动**：`Scroll { x, y, delta_y }`、`ScrollX { x, y, delta_x }`
- **键盘与文本**：`KeyDown(KeyCode)`、`KeyUp(KeyCode)`、`ModifiersChanged(KeyModifiers)`、`Backspace`、`Delete`、`TextCommit(char)`
- **系统导航与移动端视图区**：`NavigateBack`、`SafeAreaChanged(SafeAreaInsets)`、`KeyboardInsetChanged(i16)`

### 指针设备来源隔离（`PointerKind`）与 `GestureConfig`

引擎通过 `PointerKind`（`Touch` / `Mouse` / `Pen`）区分不同输入设备的交互习惯，并配合 `GestureConfig` 参数（默认：`drag_slop_px: 8`、`swipe_threshold_px: 16`、`edge_back_width_px: 24`、`long_press_ms: 500`、`repeat_delay_ms: 400`、`repeat_interval_ms: 100`、`mouse_drag_scroll: false`）控制手势识别，可通过 `engine.set_gesture_config(cfg)` 自定义：

- **触摸屏（`PointerKind::Touch`）**：默认启用空白滚动区跟手拖拽滚动、松手惯性滚动以及左边缘右滑返回（`NavigateBack`）；`VeloxEngine::new(w, h)` 出于移动端向后兼容默认处于 `PointerKind::Touch`；
- **桌面鼠标（`PointerKind::Mouse`）**：`WinitPlatform` 与 `velox_kit::application().run(...)` 自动对鼠标事件标记 `PointerKind::Mouse`（同时支持桌面触屏 `WindowEvent::Touch` 动态切换为 `Touch`），Web 宿主自动桥接浏览器 `e.pointerType`。因为 `GestureConfig::mouse_drag_scroll` **默认为 `false`**，鼠标按住空白处拖动**不会**把整个页面拖走（滚轮、滚动条及显式注册了 `.on_drag(...)` 的控件如 `Slider` / `ResizableSplit` / 文本选区照常工作）；仅在地图、白板等需鼠标拖动画布的场景中可显式开启 `cfg.mouse_drag_scroll = true`；
- **引擎显式派发辅助方法**：测试或自定义宿主可直接调用 `engine.dispatch_mouse_down(x, y)`、`engine.dispatch_touch_down(x, y)` 或 `engine.dispatch_pointer_down_with_kind(x, y, kind)`。

---

## 事件绑定：`cx.listener` 与 `cx.action`

在 `View::render` 内为组件或 `Div` 绑定事件闭包，全部分配在 `ScratchArena`：

```rust
// 接收事件参数 E 的回调（ClickEvent, DragEvent, SwipeDir, KeyCode, usize, bool, i32 ...）
let on_drag = cx.listener(|this, ev: DragEvent, acx| {
    let ratio = (ev.local_x.max(0) as i32 * 100) / ev.width.max(1) as i32;
    this.progress = ratio.clamp(0, 100);
    acx.stop_propagation();
});

// 无事件参数的动作回调（on_dismiss / on_close / on_focus / on_toggle / on_click_raw）
let raw_back = cx.action(|this, _acx| {
    this.nav.pop(200);
});
```

在 `Div` 上直接支持：`.on_click(cx, ...)`、`.on_click_raw(...)`、`.on_long_press(cx, ...)`、`.on_long_press_raw(...)`、`.on_drag(cx, ...)`、`.on_secondary_click(cx, ...)`、`.on_swipe(cx, ...)`、`.on_key_input(...)`、`.on_navigate_back(...)`、`.auto_repeat()`。

---

## 焦点链与按键导航

- 调用 `div().focusable()` 或 `.focus_border(width, color)` 会把当前容器纳入键盘/实体按键导航顺序（绑定了 `.on_click` / `.on_key_input` / `.on_long_press` 时也会自动开启 `focusable`）；
- 用户按 `Tab` / `BackTab`（或对应方向按键）时，引擎更新 `focused_index` 并自动触发 `Pass-4` 焦点环渲染；按 `Enter` / `Space` 激活当前聚焦元素；
- 文本类组件聚焦期间收到的 `KeyCode` 会优先派发到 `.on_key_input(...)`。

---

## 文本编辑状态机：EditableText 与 TextBuffer

Velox 将**状态持有**与**无状态渲染**解耦，全部基于定长零堆状态机实现：

1. **单行输入 `EditableText<const CAP: usize = 64>`**（搭配 `TextInput` / `Input`）：
   - 支持光标字节偏移、像素测距 `cursor_pixel_offset(font_size)`、`hit_test_cursor_byte`、`selected_range()`、单词边界移动 (`move_word_left/right`)、IME 预编辑 `set_preedit` / `commit_preedit` 与 `handle_key(key)`；
   - 在 `render` 中通过 `self.text.as_arena_str(cx)` 零堆分配传入 `TextInput::new(id, s, focused)`。
2. **多行代码编辑 `TextBuffer<const CAP: usize = 1024>`**（搭配 `CodeEditor` / `Textarea`）：
   - 内置 `CursorSelection`、`BufferPoint`（含 `column_anchor` 垂直导航保持列位）、`UndoHistory<TextSnapshot<256>, 16, 2>`、`AutoClosedPairs<16>`、`SearchMatcher<32>` 与 `TabSize`；
   - 支持 `handle_key(key)`、`undo()` / `redo()`、`insert_newline()`、`insert_str(...)`、`select_to_point(anchor, pt)`、`set_cursor_point(pt)`、`as_arena_str(cx)`；
   - 配合词法高亮 `HighlightSpan`（`HighlightKind`: Keyword / Type / String / Comment / Number / Function / Operator）与诊断 `DiagnosticSpan`（`DiagnosticSeverity`）传给 `CodeEditor`。

---

## 页面路由栈 Navigator

`velox_base::navigator::Navigator<S: Copy, const DEPTH: usize = 8>` 是定长容量、无堆分配页面栈与双页面转场控制器：

- **入栈 / 出栈 / 替换**：`nav.push(route, PageTransition::SlideLeft, 200)`、`nav.pop(200)`、`nav.replace(...)`、`nav.current()`、`nav.can_pop()`；
- **系统返回键绑定**：在根节点配合 `if nav.can_pop() || sheet_open { root = root.on_swipe(cx, ...); }` 并在 `NavBar` 中按 `nav.can_pop()` 条件渲染返回按钮；
- **转场步进**：`step_transition` 与 `step_dual_transition` 使用定点二次缓出（`EaseOutQuad`）输出旧/新页面偏移量 `(offset_x, offset_y, alpha)`，并支持 `composite_transition_buffers` 直接对离屏缓冲平移合成。
