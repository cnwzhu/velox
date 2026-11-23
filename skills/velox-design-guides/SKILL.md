---
name: velox-design-guides
description: >-
  Normative Design Guides for Velox GUI applications (`velox-kit` across Desktop,
  Mobile, and WebAssembly targets). Read in full before designing
  or changing any screen, shell layout, spacing, color token usage (`CoreTheme` /
  `ThemeMode`), visual hierarchy, density, component selection, interaction states,
  overlays (`.overlay().occlude_hit()`), motion (`Tween` / `Presence` / `Easing`),
  data-heavy view, or interface copy, and before reviewing UI quality.
  Velox GUI 应用界面设计与评审规范指南：在设计或修改页面结构、布局留白、明暗主题令牌、
  交互反馈、模态浮层、动效曲线、数据密集型视图与界面文案前务必阅读。
---

# Velox 设计指南（Velox Design Guides）

详细设计规范见 [references/design-guides.md](references/design-guides.md)。它是工程实施前的**前置约束**，而非可选灵感。设计或修改可见界面前，请直接读取该文档，不要仅凭本摘要、单个现有示例或通用前端直觉作答。

## 章节阅读地图

全新界面或大型重构请通读全文；局部调整可先读前两节“设计主张”与“从任务出发”，再按关键字检索对应章节（`rg -n '^## ' references/design-guides.md`）：

| 章节                               | 何时必读                                                             |
| ---------------------------------- | -------------------------------------------------------------------- |
| 设计主张（Design thesis）          | 永远必读；原生响应、零无效像素与零空闲能耗的设计原则                 |
| 向现代组件系统取经                 | 借鉴 Shadcn 语义化 Token 与可读性纪律                                |
| 从用户任务出发                     | 永远必读；信息层级、主次动作区分、渐进式信息披露                     |
| 视觉语言                           | 颜色令牌（`CoreTheme`）、字号层级、间距与圆角标尺、卡片与边框        |
| 常见布局骨架（Layout patterns）    | 桌面工作台壳层、移动端安全区壳层、表单设置卡与分栏                   |
| 组件选型与组合边界                 | 选择现成组件、组合 `div()`、编写 `Widget` 还是升级为独立 `View`      |
| 交互状态与反馈                     | 悬停、按下、聚焦环、选中态、禁用态、验证与警告状态                   |
| 浮层、遮罩与打断级别               | `Dialog` / `AlertDialog` / `Modal` / `Sheet` / `Notification` / `Tooltip` |
| 动效设计与空闲节能                 | `120–220ms` 微过渡、`Easing` 选型、`Presence` 退场与零空闲重绘       |
| 数据密集型界面设计                 | `Table` / `DataTable`、`VirtualList`、`Tree`、`CodeEditor`、遥测卡片 |
| 界面文案与术语                     | 按钮“动词 + 名词”格式、中英文排版与空白页/报错文案规范               |
| 多端平台适配与礼仪                 | Windows / macOS / Pure Wayland 桌面；Android / iOS / 鸿蒙 / Web 端   |
| AI 生成 Velox 界面的常见禁忌       | 防止堆砌模板卡片、缺少 `flex_grow(1)` 溢出或遗漏 `.occlude_hit()`    |
| 可访问性与键盘焦点检查清单         | `.focusable()`、`focus_border`、Tab 焦点链与对比度审查               |
| 设计走查清单（Review Checklist）   | 交付前逐项核对所有红线                                               |

## 不可妥协的设计底线

- **严禁硬编码裸色值替代语义令牌**：主界面、卡片、文本与边框配色必须通过 `cx.theme()`（`background`、`foreground`、`card`、`popover`、`primary`、`secondary`、`muted_foreground`、`accent`、`danger`、`success`、`warning`、`info`、`border`、`input`、`ring`、`sidebar`）读取，确保 `ThemeMode::Dark` 与 `ThemeMode::Light` 实时切换一致可用（注意危险/错误令牌为 `theme.danger`，而非 `destructive`）。
- **平台形态各司其职**：
  - 桌面端追求高信息密度、稳定持久化导航（可折叠 `Sidebar`、`Toolbar`、`StatusBar`）、响应式 `flex_grow(1)` 主区与 `2× HiDPI` 锐利边框；
  - 移动端优先单手拇指可达区（`NavBar` + `TabBar` + `BottomSheet`）与 `.safe_area(cx)` / `keyboard_inset` 自动避让，保证至少 `38px+` 触控高度。
- **`Button` 用于应用内命令，`Link` 仅用于外部资源/跳转**：应用内所有操作与表单提交统一使用 `Button`（靠 `.primary()` / `.secondary()` / `.outline()` / `.ghost()` / `.warning()` 区分权重）。
- **状态变化必须肉眼可辨**：所有可交互控件必须提供明确的 `hover_bg`、`active_bg`、`focus_border`（用于键盘导航）与 `disabled` 降透明度反馈。
- **模态浮层必须视觉与命中双重隔离**：模态弹窗必须拥有明确遮罩、关闭入口，并挂载 `.overlay().occlude_hit()` 彻底阻断对底层列表或导航栏的误触；非阻塞状态反馈优先用右上角 `Notification` 或内联 `Alert`。
- **动效克制且绝不阻止空闲休眠**：常规状态切换控制在 `120–220ms`，且过渡结束后不得再触发 `cx.request_frame()`，保证界面静止时回归 `PlatformControlFlow::Wait`（0% CPU）。

完成界面修改后，请对照 [references/design-guides.md](references/design-guides.md) 末尾的“设计走查清单”逐项验证。
