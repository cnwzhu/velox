# 组件家族约定（Component-Family Conventions）

**Contents:** [按任务选择组件家族](#按任务选择组件家族) · [事件请求与命令回调的差异](#事件请求与命令回调的差异) · [Signal 双向绑定简写 `.bind()`](#signal-双向绑定简写-bind) · [长生命周期切片 `'a` 约束](#长生命周期切片-a-约束) · [`Base*` 与 `Themed*` 双别名约定](#base-与-themed-双别名约定) · [显式导入能力 Trait](#显式导入能力-trait)

在查阅单个组件源码细节前，先使用本页预测同类组件的构造器形参、状态所有权与回调签名。切勿把其他 GUI 框架（如 React/Web/其他 Rust GUI）的约定套用到 Velox 上。

## 按任务选择组件家族

| 任务场景 | 家族代表与真实构造签名 | 状态归属 | 回调与子元素组合 |
| --- | --- | --- | --- |
| 执行单次命令 | `Button::new(id, label)` | 应用持有操作逻辑 | `.on_click(cx.listener(\|this, _: ClickEvent, _acx\| ...))` |
| 切换布尔开关 | `Checkbox::new(id, checked)`、`Switch::new(id, checked)`、`Toggle::new(id, label, checked)` | 宿主 `View` 持有 `bool`，渲染期传入值 | `.on_change(cx.listener(\|this, next: bool, _acx\| { this.flag = next; }))` |
| 多选一索引切换 | `RadioGroup::new(id, options, selected)`、`Tabs::new(id, labels, active)`、`TabBar::new(id, labels, active)` | 宿主 `View` 持有 `Option<usize>` 或 `usize` | `RadioGroup` 用 `.on_change`；`Tabs` / `TabBar` 用 `.on_select` 接收 `usize` |
| 单行文本编辑 | `TextInput::new(id, value_str, focused)`（别名 `Input`） | 宿主 `View` 持有 `EditableText<CAP>` 与 `focused: bool` | `.on_focus(cx.action(...))`、`.on_key_input(cx.listener(\|this, k: KeyCode, _\| ...))` |
| 多行代码/文本编辑 | `CodeEditor::new(id, value_str, focused)`（别名 `Editor`）、`Textarea::new(...)` | 宿主 `View` 持有 `TextBuffer<CAP>` | `.highlights(spans)`、`.selection(sel, cur_pt)`、`.on_key_input(...)`、`.on_pointer_select(...)` |
| 下拉选择列表 | `Select::new(id, options, selected, open)`、`Dropdown::new(id, options, selected, expanded)` | 宿主 `View` 持有 `selected` 与 `open` | `.on_toggle(cx.action(...))` + `.on_select(cx.listener(\|this, idx: usize, _\| ...))` |
| 表单容器与字段 | `Form::new(id, width)`、`FormField::new(id, label)` | 字段控件各自保持受控状态 | `Form::content(cx, ...)` / `.on_submit(cx.action(...))`；`FormField::control(cx, ...)` |
| 模态确认/对话框 | `Dialog::new(id, title, open)`、`AlertDialog::new(id, title, desc)`、`Modal::new(id, title)` | 宿主 `View` 持有 `open: bool` | `.content(cx, ...)`、`.footer(cx, ...)`、`.on_close(cx.action(...))` |
| 数据表格 / 侧边栏 | `Table::new(id, columns, rows)`、`DataTable::new(...)`、`Sidebar::new(id, title, sections)` | 宿主 `View` 持有当前选中行/页签/折叠状态 | `Table` 用 `.on_row_click`；`Sidebar` 用 `.on_select` (`u32`) + `.on_toggle_collapse` |

**构造约定要点**：

1. **绝大多数组件首参为稳定 `id: u32`**：用于在局部刷新、过渡动画、滚动偏移与焦点链中跨帧识别组件，切勿使用随机或易变索引作为列表组件 `id`（`Badge::new(label)`、`Card::new()`、`div()` 等轻量无 ID 容器除外）。
2. **`Widget<'a>` 在每帧 `render` 中按值消费**：绝大多数组件实现 `Widget<'a>`（无保留堆对象），构造器传入的切片或状态引用均为只读快照；事件触发后通过 `cx.listener` / `cx.action` 写回宿主 `View`，下一帧引擎自动重建轻量化元素树。

---

## 事件请求与命令回调的差异

Velox 在 `Scope<Self>` 上提供两个核心回调分配器（均分配于帧级 `ScratchArena`，零 `Box` 堆分配）：

1. **`cx.listener(|this, event: E, acx: &mut ActionScope| { ... })`**：
   - 返回 `&'a dyn Fn(E, &mut ActionScope)`。
   - 用于传递值或事件载荷：`Button::on_click`（`ClickEvent`）、`Switch::on_change` / `Checkbox::on_change`（`bool`）、`Slider::on_change`（`i32`）、`Tabs::on_select` / `RadioGroup::on_change` / `Table::on_row_click` / `Select::on_select`（`usize`）、`Sidebar::on_select`（`u32`）、`TextInput::on_key_input`（`KeyCode`）、`Div::on_swipe`（`SwipeDir`）。
2. **`cx.action(|this, acx: &mut ActionScope| { ... })`**：
   - 返回 `&'a dyn Fn(&mut ActionScope)`。
   - 用于无事件参数动作：`Dialog::on_close`、`Notification::on_dismiss`、`BottomSheet::on_dismiss`、`TextInput::on_focus`、`Select::on_toggle`、`Sidebar::on_toggle_collapse`、`Form::on_submit`、`Div::on_click_raw`。

```rust
// 受控值变更（Switch / Checkbox 收到的是请求切换后的目标 bool）
Switch::new(501, self.vsync_enabled)
    .on_change(cx.listener(|this, next: bool, _acx| {
        this.vsync_enabled = next;
    }))

// 无参数动作关闭（Notification / Dialog）
Notification::new(800, "Saved", "Settings written.")
    .variant(NotificationVariant::Success)
    .on_dismiss(cx.action(|this, _acx| {
        this.notification_open = false;
    }))
```

**自动标脏与异步派发**：无论 `cx.listener` 还是 `cx.action`，回调返回后引擎都会**默认标记当前 `PodId` 为脏**（无需再写重复的 `acx.notify()`；若回调内检查条件未变更任何状态，可调用 `acx.skip_notify()` 避免无效重绘）。需要在回调内发起异步任务时，直接调用 `acx.spawn(async move |this: Handle<Self>, mut cx: AsyncApp| ...)` 并将返回的 `Task<()>` 存入 `View` 字段（详见 [core/async.md](core/async.md)）。

---

## Signal 双向绑定简写 `.bind()`

当受控组件直接绑定到细粒度 `Signal<T>` 时，可使用零堆分配的 `Component::bind(cx, ...)` 构造器（在 `velox-base` 与 `velox-component` 两侧均提供）：渲染期自动读取 `sig.get()` 建立订阅，交互时自动在 `ScratchArena` 分配回调并通过 `sig.set_if_changed(next)` 精准回写，无需手写 `on_change` / `on_select` 样板代码：

| 绑定信号类型 | 支持 `.bind(...)` 的组件与签名 |
| --- | --- |
| `Signal<bool>` | `Switch::bind(cx, id, sig)`、`Checkbox::bind(cx, id, sig)`、`Toggle::bind(cx, id, label, sig)`、`Collapsible::bind(cx, id, title, sig)`、`Accordion::bind(cx, id, title, sig)` |
| `Signal<i32>` / `Signal<i64>` / `Signal<u8>` | `Slider::bind(cx, id, sig)`、`NumberInput::bind(cx, id, sig)`、`Rating::bind(cx, id, sig)` |
| `Signal<usize>` | `Tabs::bind(cx, id, labels, sig)`、`TabBar::bind(cx, id, labels, sig)`、`RadioGroup::bind(cx, id, options, sig)`、`Roller::bind(cx, id, options, sig)`、`Pagination::bind(cx, id, total_pages, sig)` |

---

## 长生命周期切片 `'a` 约束

许多组件构造器接受借用切片（如 `&'a [&'a str]`、`&'a [SidebarSection<'a>]`、`&'a [TableColumn<'a>]`、`&'a [SelectOption<'a>]`、`&'a [HighlightSpan]`），而 `View::render<'a>` 返回 `Element<'a>`：

- **静态配置选项使用 `const`**：如 `const TABS: &[&str] = &["Overview", "Settings"];` 或 `const COLS: &[TableColumn<'static>] = &[...];`，避免局部变量 `let cols = &[...]` 在 `render` 函数返回时触发 `E0515` / `E0716` 悬垂借用错误。
- **动态文本与动态列表使用 `cx` 帧内存池**：
  - 字符串格式化：`fmt!(cx, "Checks: {}", self.count)` 生成 `StrRef<'a>`，或调用 `.as_str()` 取 `&'a str`；
  - 文本编辑器文档：`self.editor_buf.as_arena_str(cx)` 零堆分配复制到帧内 `&'a str`；
  - 动态切片：`cx.arena().alloc_iter(iter)` 返回 `&'a mut [T]`。

---

## `Base*` 与 `Themed*` 双别名约定

`velox-kit` 支持严格可加的 Feature 设计：

- 启用 `base`（默认随 `component` 开启）时，顶层组件（如 `Button`）始终指向 `velox_base::Button`；
- 启用 `component`（默认开启）时，`velox-component` 通过 `define_themed_component!` 提供带默认 Tailwind 语义预置的包装类型；
- 若库作者需要保证无论下游开启哪些 Feature 类型永不变化，可直接导入 `velox_kit::{BaseButton, BaseCard, ...}` 或（开启 `component` 时）`velox_kit::{ThemedButton, ThemedCard, ...}`。

**边界原则**：`velox-base` 中的组件只负责交互逻辑与在**未显式指定配色时**按需回退 `cx.theme()`；应用层通过 `Styled`、`TailwindStyled` 或显式颜色方法覆盖时，始终保持调用方定制优先级最高。

---

## 显式导入能力 Trait

`use velox_kit::prelude::*;` 已导出核心布局/引擎 Trait，部分组件修饰符 Trait 由 `velox_kit` 顶层及 `prelude`（启用 `base` 时）导出，按需留意具体组件是否实现对应 Trait：

- `Styled`（所有 `Div` 与核心包装组件）：`.flex_row()`、`.flex_col()`、`.w()` / `.h()` / `.w_full()` / `.h_full()` / `.size_full()`、`.flex_grow(1..=3)`、`.p()` / `.px()` / `.py()`、`.gap()`、`.bg()`、`.border()`、`.rounded()`、`.items_center()`、`.justify_between()`。
- `TailwindStyled`（`velox_kit::component::TailwindStyled`）：`.tw_card_surface(theme)`、`.tw_button_size(size)`、`.tw_button_variant(theme, variant)`。
- `Sizable`：`.small()`、`.medium()`、`.large()`、`.with_size(ComponentSize)`（常见于 `Button`、`Switch`、`Checkbox`、`Radio`、`RadioGroup`、`Tabs`、`Icon`、`Badge`、`Slider` 等）。
- `Variants`：`.primary()`、`.secondary()`、`.outline()`、`.warning()`、`.ghost()`、`.with_variant(ComponentVariant)`（常见于 `Button`、`Badge` 等）。
- `Disableable`：`.disabled(bool)`（常见于 `Button`、`Switch`、`Checkbox`、`Radio`、`RadioGroup`、`TextInput`、`Slider`、`Toggle` 等）。
