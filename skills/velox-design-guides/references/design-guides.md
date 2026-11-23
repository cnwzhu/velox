# Velox 界面设计指南（Normative Design Guides）

**Contents:** [设计主张（Design thesis）](#设计主张design-thesis) · [向现代组件系统取经](#向现代组件系统取经) · [从用户任务出发](#从用户任务出发) · [视觉语言](#视觉语言) · [常见布局骨架（Layout patterns）](#常见布局骨架layout-patterns) · [组件选型与组合边界](#组件选型与组合边界) · [交互状态与反馈](#交互状态与反馈) · [浮层、遮罩与打断级别](#浮层遮罩与打断级别) · [动效设计与空闲节能](#动效设计与空闲节能) · [数据密集型界面设计](#数据密集型界面设计) · [界面文案与术语](#界面文案与术语) · [多端平台适配与礼仪](#多端平台适配与礼仪) · [AI 生成 Velox 界面的常见禁忌](#ai-生成-velox-界面的常见禁忌) · [可访问性与键盘焦点检查清单](#可访问性与键盘焦点检查清单) · [设计走查清单（Review Checklist）](#设计走查清单review-checklist)

本指南是构建高质量 Velox 应用的强制设计规范。设计任何新界面、页面结构、卡片组件或交互反馈前，必须严格对照执行。

---

## 设计主张（Design thesis）

1. **原生响应与零算力浪费**：界面静止时应当彻底零消耗（`PlatformControlFlow::Wait`，0% CPU），只在状态变化或过渡期间局部标脏重绘；设计应尊重结构稳定性和清晰区块边界，让脏矩形局部更新天然高效，而不依赖无休止的背景特效动画。
2. **一维清晰秩序（Clarity via Axis Discipline）**：Velox 基于 `i16` 四步一维弹性布局求解器（`Two-Pass Axis Solver`），好的界面自然符合清晰的横/纵流骨架、合理的内边距梯度和严谨对齐，而不是靠脆弱的魔法硬编码坐标拼贴。
3. **以信息密度服务专业任务，以触控安全服务移动体验**：在桌面工作台上呈现高可读、结构紧凑的面板、代码区和表格；在移动端和 Web 画布上确保可触控区域、异形屏避让区（`SafeAreaInsets`）与软键盘留空清晰可靠。

---

## 向现代组件系统取经

Velox 借鉴 Shadcn UI 的“语义化令牌 + 可组合原语 + 低视觉噪音”理念，并适配零全局堆分配架构：

- **少装饰、强层次**：用 `1px` 细边框（`.border(1, theme.border)`）与背景表面明度反差（`theme.background` vs `theme.card` vs `theme.secondary`）建立层级，而不是沉重浓黑大阴影。
- **可控的视觉变体**：命令只有 5 档标准语义（`Primary`、`Secondary`、`Outline`、`Warning`、`Ghost`），避免五颜六色的自定义按钮破坏主次焦点。
- **默认即可用，显式可覆写**：基础组件自动回退 `cx.theme()` 语义配色与标准字号，同时允许调用方通过 `Styled`、`TailwindStyled` 或显式构建器字段精准定制。

---

## 从用户任务出发

设计每个屏幕前先回答三件事：

1. **主要动作是什么？**：同一工具栏或表单 footer 最多只放一个 `.primary()` 按钮；次要或切换操作用 `.secondary()` / `.outline()`；低干扰辅助操作用 `.ghost()`。
2. **常驻状态与按需状态怎么分层？**：全局状态与导航（工作台切页、分支、Checks 计数、主题状态）放进常驻壳层；详细信息或系统说明按需进入 `Dialog`、`Sheet` 或切换页签，避免一屏塞满所有表单项。
3. **空状态、报错状态长什么样？**：表格无选中行、输入框空占位（`placeholder`）、表单字段错误（`FormField::error(...)`）必须在首次构图时一并定义。

---

## 视觉语言

### 语义颜色令牌（`CoreTheme` / `ThemeMode`）

严禁在应用组件内直接散落硬编码 `Color::rgb(...)` 来替代主前景、底色、边框与主色调。必须从 `let theme = cx.theme();`（配合 `self.theme_mode.to_theme()` + `cx.override_theme(theme)`）取值：

| 语义字段 | 典型角色 | `Dark` (Shadcn Zinc Dark) | `Light` (Shadcn Zinc Light) |
| --- | --- | --- | --- |
| `theme.background` | 页面最底层背景 | Zinc 950 `(9, 9, 11)` | Pure White `(255, 255, 255)` |
| `theme.foreground` | 主正文、主标题、首要标签 | Zinc 50 `(250, 250, 250)` | Zinc 950 `(9, 9, 11)` |
| `theme.card` / `card_foreground` | 工具栏、卡片、对话框表面与文本 | Zinc 925 `(20, 20, 23)` | Pure White `(255, 255, 255)` |
| `theme.popover` / `popover_foreground` | 下拉菜单、浮窗、气泡面板表面 | Zinc 900 `(24, 24, 27)` | Pure White `(255, 255, 255)` |
| `theme.primary` / `primary_foreground` | 主操作按钮、激活项、勾选态 | Blue 500 `(59, 130, 246)` | Blue 600 `(37, 99, 235)` |
| `theme.secondary` / `secondary_foreground` | 次要按钮、选中悬停、斑马纹 | Zinc 800 `(39, 39, 42)` | Zinc 100 `(244, 244, 245)` |
| `theme.muted` / `muted_foreground` | 辅助说明、占位符、状态栏次要文字 | Zinc 400 `(161, 161, 170)` | Zinc 500 `(113, 113, 122)` |
| `theme.accent` / `accent_foreground` | 柔和高亮交互表面与悬停底色 | Zinc 800 `(39, 39, 42)` | Zinc 100 `(244, 244, 245)` |
| `theme.danger` / `danger_foreground` | 危险操作、失败/报错文本与警示（注意不是 `destructive`） | Red 500 `(239, 68, 68)` | Red 600 `(220, 38, 38)` |
| `theme.success` / `success_foreground` | 成功完成状态、HTTP 2xx、检查通过标记 | Green 500 `(34, 197, 94)` | Green 600 `(22, 163, 74)` |
| `theme.warning` / `warning_foreground` | 警告提醒、已取消提示、必填星号 | Amber 500 `(245, 158, 11)` | Amber 600 `(217, 119, 6)` |
| `theme.info` / `info_foreground` | 信息提示横幅、进行中状态标识 | Blue 500 `(59, 130, 246)` | Blue 600 `(37, 99, 235)` |
| `theme.border` / `input` / `ring` | `1px` 容器边框 / 输入框边框 / 柔和键盘焦点环 | Zinc 800 / Zinc 700 / Steel Blue | Zinc 200 / Zinc 300 / Sky Blue |
| `theme.sidebar` / `list_hover` / `list_active` | 侧边栏表面、列表悬停态与选中态 | Zinc 940 / Zinc 825 / Zinc 775 | Zinc 50 / Zinc 100 / Zinc 200 |

> 说明：仅模态全屏遮罩背景允许使用带透明度的中性暗幕 `Color::rgba(0, 0, 0, 140)`（或 `168`），其它视觉颜色应尽量源自 `CoreTheme`。

### 字体、间距与圆角标尺

由于 `Style` 内的 `padding` / `gap` 为 `i8`，`rounded` 为 `u8`，推荐采用统一离散几何标尺：

- **字号梯度 (`text(...).size(px)`)**：
  - `18px`：主工作区页面大标题；
  - `14px–15px`：顶部应用标题、对话框标题、区域标题；
  - `13px`：正文、卡片标题、输入框内容、列表标题；
  - `12px`：按钮默认小号文字、描述文字、表格内容、副标题；
  - `11px`：状态栏（`StatusBar`）、角标（`Badge`）、帮助/错误小字、`Tooltip`。
- **间距梯度 (`.p(...)` / `.px(...)` / `.py(...)` / `.gap(...)`)**：
  - 紧凑元素间：`4px` / `6px` / `8px`（按钮行 `gap(8)`、徽标内边距 `px(8).py(2)`）；
  - 面板内容区：`12px` / `14px` / `16px` / `20px`（卡片 `p(16).gap(12)`，主内容区 `p(20).gap(14)`）。
- **圆角梯度 (`.rounded(u8)`)**：
  - 徽标/小胶囊：`4px`；
  - 按钮、输入框、表格、`Tabs` 内项：`6px`–`8px`；
  - 卡片容器：`8px`；
  - 对话框与模态弹窗：`10px`–`12px`。

---

## 常见布局骨架（Layout patterns）

### 1. 桌面工作台壳层（Desktop Workbench Shell）

参考 `examples/desktop_app/src/lib.rs` 验证结构：

1. **根容器**：`div().id(1).size_full().flex_col().bg(theme.background)`；
2. **顶部工具栏**：固定逻辑高度 `h(44)`、`px(16)`、`justify_between()`、`bg(theme.card)`、`border(1, theme.border)`；左侧放产品标题 + 当前页面徽章，右侧放主次动作按钮组（高度统一 `h(28)`）；
3. **中间主工作区 (`body_row`)**：`w_full().flex_grow(1).flex_row()`：
   - 左侧导航栏 `Sidebar`：展开 `200px`、折叠 `52px`、`h_full()`；
   - 右侧主内容区：`flex_grow(1).h_full().p(20)`，使桌面拖拽缩放（`Resized`）时主面板平滑填充全部剩余宽高；
4. **底部状态栏**：全宽 `w_full().h(26).px(12)`、`justify_between()`、`11px` 文字，呈现分支、协议、激活页与实时统计。

### 2. 移动端应用壳层（Mobile Shell）

参考 `examples/android_app/src/lib.rs` 验证结构：

1. **根容器**：`div().size_full().flex_col().justify_between().bg(cx.theme().background)`；
2. **顶部 `NavBar`**：默认开启 `.safe_top(true)`，自动在 `44px` 导航条上方叠加 `cx.safe_area().top`（状态栏/灵动岛/挖孔区）；
3. **内容区**：`.flex_grow(1)` 配合页面路由栈 `Navigator<Route, N>`，支持边缘右滑（`on_swipe`）与物理返回导航；
4. **底部 `TabBar`**：默认开启 `.safe_bottom(true)`，避让底部手势横条 / Home Indicator，可触摸按钮高度建议 `>= 38px`。

### 3. 表单与设置页卡

- 使用 `Card::new()` 或 `Form::new(id, w)` + `FormField::new(id, label)`；
- 成对设置的开关项使用单行 `.flex_row().w_full().items_center().justify_between()`（左侧文字说明 `13px`，右侧 `Switch`）；
- 多列仪表盘瓦片采用 `.flex_row().w_full().gap(16)` 下子卡片各设 `.flex_grow(1)` 或用 `.grid(cols)`。

---

## 组件选型与组合边界

- **优先复用 `velox-base` / `velox-component` 现成组件**：布尔配置用 `Switch` / `Checkbox`，单选小集合（`2–4` 项）用 `RadioGroup` 或 `Tabs`，`5+` 项用 `Select` / `Dropdown`，长列表用 `VirtualList` / `UniformList`，遥测比值用 `ArcGauge` + `Progress`，代码展示与编辑用 `CodeEditor`。
- **何时写独立 `Widget<'a>`**：某一段卡片/统计行在本应用中多处复用，但不需要自己独立保存跨帧状态。
- **何时拆分为独立 `View` (`Handle<T>`)**：包含高频局部数据刷新（如独立每秒跳动的遥测表、长编辑区），需要独立局部重绘与重排隔离边界以免牵连整窗重排。

---

## 交互状态与反馈

- **可见交互态**：所有自定义可点击 `div()` 建议提供 `.hover_bg(...)`、`.active_bg(...)` 与 `.focusable()` / `.focus_border(2, theme.primary)`（内置 `Button` 已自动派生 `hover_bg` 与 `active_bg`）；
- **禁用状态 `.disabled(true)`**：禁用态降低不透明度，且不绑定点击与键盘输入句柄；
- **Button vs Link**：触发应用内部行为（保存、运行检查、切换页签、打开关于对话框）一律使用 `Button`；`Link` 仅用于外部链接/邮件/文档跳转到外部资源语义；
- **表单提交与校验**：必填字段标记 `.required(true)`，校验失败通过 `FormField::error("...")` 就近展示，不要把所有校验错误丢进一个大弹窗；
- **异步任务反馈（加载 / 取消 / 错误）**：发起网络请求或耗时后台计算（`acx.spawn` + `cx.background_spawn`）时，用进度条（`Progress`）或旋转器（`Spinner`，仅在 `busy` 期间 `cx.keep_animating(true)`）呈现活跃状态，并始终提供明确的 `Cancel` 入口（释放 `Task<T>` 即刻取消）；成功态用 `theme.success`，失败态用 `theme.danger` 就近显示简洁摘要，完成后立即停帧恢复 0% CPU `Wait`。

---

## 浮层、遮罩与打断级别

按打断用户工作流的程度从低到高选择浮层组件：

1. **轻量即时提示 —— `Tooltip`**：悬停/短提示，只含 1 行紧凑文案（`11px`），不承载按钮。
2. **非阻塞反馈 —— `Notification` / `Alert`**：操作完成或检查通过（如 `"Workspace Check Passed"`）使用右上角 `Notification`（置于 `.pos(0, 52).w_full().overlay()` 容器中并 `.justify_end().pr(16)`），不打断用户当前键盘输入或点击流程。
3. **上下文工具浮窗 —— `Popover` / `Menu` / `ContextMenu` / `Select`**：展开浮层必须开启 `.overlay().occlude_hit()`，使点击菜单本身不会穿透。
4. **半模态侧滑工作区 —— `Sheet` / `BottomSheet`**：桌面从左右滑出属性检查器、移动从底部滑出快捷菜单，点击背景或按返回即可关闭。
5. **强阻塞模态弹窗 —— `Dialog` / `AlertDialog` / `Modal`**：
   - 外层全屏遮罩 **必须** 写 `.pos(0, 0).size_full().overlay().occlude_hit().bg(Color::rgba(0, 0, 0, 140))`；
   - `AlertDialog` 专用于高风险不可逆或破坏性确认操作（如删除项目、清空缓存），明确区分取消与破坏性确认动作。

---

## 动效设计与空闲节能

- **时长准则**：微交互色变（`Button`、`Switch`、`Tabs`）使用 `120–160ms`；抽屉/弹窗进退场（`Dialog`、`BottomSheet`、`Presence`）使用 `180–240ms`；
- **曲线准则**：通用平滑移动与透明度用 `Easing::EaseOut`；弹出卡片入场可用 `Easing::EaseOutBack` 或 `Easing::Spring`；
- **生命周期节流**：使用 `Presence` 管理弹窗/浮层退场动画时，务必在 `render` 中以 `cx.keep_animating(self.popup.is_animating(now));` 驱动，动画完成后 `VeloxEngine::needs_redraw()` 自动降回 `false`，回到 0% CPU 休眠。

---

## 数据密集型界面设计

- **固定对齐列宽的数据表格**：为每列合理分配明确逻辑宽度 `TableColumn::new(title, width)`，开启斑马纹区分行边界，点击行后在下方详情卡联动显示该项详述；
- **代码编辑器 (`CodeEditor`)**：保持 2 空格或 4 空格 `TabSize` 提示、开启行号与缩进参考线、为语法令牌赋予明确对比度色值，并在工具栏明确提供 `Undo` / `Redo` 操作按钮；
- **万级滚动数据**：严格用 `VirtualList` / `UniformList` 分页窗口渲染，避免整棵树撑爆 `ScratchArena`。

---

## 界面文案与术语

1. **命令动作“动词 + 名词”**：`Run Check`、`Open Analytics`、`Insert Fn`、`Save Profile`、`Delete Project`，禁止含糊的 `OK`、`Yes`、`Do it`；
2. **确认弹窗说清影响范围**：标题明确操作对象（`Remove "Roadmap"?`），描述说明后果（`Files on disk are not deleted.`），主按钮复述动作结果（`Remove`）；
3. **中英文自然惯例**：
   - 中文文案遵循规范全角标点，名词术语前后可留半角空格或按产品统一；禁止逐字直译英文语序；
   - 英文遵循 Title Case（重要标签/标题/短按钮）或 Sentence case（描述段落与帮助提示），不在单个句子内无故切换语种；
4. **数值自带单位**：`60 FPS`、`42%`、`1280×800`、`2× HiDPI`，让数字自身可读。

---

## 多端平台适配与礼仪

- **Windows / macOS / Pure Wayland 桌面**：支持自由拖拽 resize（所有主要区域必须依托 `size_full()` + `flex_grow(1)` 自适应而非写死 960×640）、支持 1×/2× HiDPI 与清晰的悬停/焦点态反馈；桌面鼠标（`PointerKind::Mouse`）滚动由滚轮与滚动条驱动，**严禁在普通表单或内容页擅自打开 `GestureConfig::mouse_drag_scroll`**（避免鼠标点选误拖整个页面；仅地图/画布编辑器按需开启）；
- **Android / iOS / 鸿蒙移动端**：全屏页面必须避开刘海屏/灵动岛、底部手势栏与输入法软键盘高度；重要二级页同时支持左上角显式返回按钮与系统边缘/物理返回手势（`PointerKind::Touch` 自动支持跟手拖滑与惯性滚动）；
- **WebAssembly `<canvas>`**：不主动依赖 DOM 滚动条，自动按浏览器 `e.pointerType` 区分鼠标与触控行为，重要操作保持清晰的画布边界与浏览器 `popstate` 返回同步。

---

## AI 生成 Velox 界面的常见禁忌

1. **漏写 `.flex_grow(1)` 导致放大窗口时留白孤岛**：主体容器或卡片组必须用 `.flex_grow(1)` 或 `.w_full()` / `.h_full()` 参与窗口自适应。
2. **强行堆砌多余徽章与空白装饰卡片**：如果信息可以用简单两列或一行状态栏展示，不要拆成十个嵌套卡片。
3. **弹窗没有半透明全屏遮罩与 `.occlude_hit()`**：直接把 `Dialog` 当普通 `.overlay()` 放在屏幕角落会导致底层按钮可点且视觉层次崩塌。
4. **硬编码暗色 RGB 后忘记切换亮色主题**：未经过 `ThemeMode::Light` 切换检验就上线，导致白底白字或看不见边框。
5. **在 `render` 中不断请求下一帧 (`cx.request_frame()`)**：直接毁掉 Velox 0% 空闲 CPU 特性。

---

## 可访问性与键盘焦点检查清单

- [ ] 所有自定义可点击/可编辑 `div()` 都调用了 `.focusable()` 或 `.focus_border(width, theme.primary)`；
- [ ] `Tab` / `BackTab` / `Enter` 焦点链顺序符合从上到下、从左到右阅读流；
- [ ] 文本与背景色、主色调与前景字均出自 `CoreTheme` 规范配色，在 `Dark` 与 `Light` 双模式下皆保持足够可读对比度；
- [ ] 使用 `engine.build_a11y_tree::<128>()` 抽检主要交互按钮、输入框、滑块均具有正确的 `A11yRole` 与非空可读 `label`。

---

## 设计走查清单（Review Checklist）

- [ ] **全局主题 Token**：零散硬编码裸色已被替换为 `cx.theme()` 语义颜色，`ThemeMode::Dark` 与 `ThemeMode::Light` 皆正常可读。
- [ ] **壳层响应式**：根容器使用 `.size_full()`，工作区主面板使用 `.flex_grow(1)`，`engine.resize(1280, 800)` 后无裁切或错位。
- [ ] **视觉密度与留白**：遵守 `4/8/12/16/20` 间距标尺与 `11/12/13/14/18` 字号层级。
- [ ] **浮层隔离**：所有模态 `Dialog` / `Modal` / `BottomSheet` 均带 `.overlay().occlude_hit()`，点击底层区域返回 `false`；非阻塞反馈采用 `Notification` / `Alert`。
- [ ] **文案与状态**：按钮采用精确动宾短语，交互控件具备明确的 hover / active / focus / disabled 状态区分。
- [ ] **0% 空闲 CPU**：静止状态下次帧 `step_platform` 返回 `PlatformControlFlow::Wait`。
