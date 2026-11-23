# 布局求解与紧凑样式（24B Style / Styled / TailwindStyled / Solver / Grid / SafeArea）

**Contents:** [24 字节紧凑 Style 位域与值域边界](#24-字节紧凑-style-位域与值域边界) · [Styled 与 Div 扩展链式 API](#styled-与-div-扩展链式-api) · [TailwindStyled 主题预设扩展](#tailwindstyled-主题预设扩展) · [四步一维弹性布局求解器（Two-Pass Axis Solver）](#四步一维弹性布局求解器two-pass-axis-solver) · [Grid 二维网格与 SafeAreaInsets](#grid-二维网格与-safeareainsets)

---

## 24 字节紧凑 Style 位域与值域边界

`velox_core::style::Style` 使用 `#[repr(C)]` 并通过编译期断言保证严格 **`size_of::<Style>() == 24`**：

| 字段 | 类型 | 说明与有效范围 |
| --- | --- | --- |
| `width` / `height` | `Length`（各 4B） | `Auto`、`Px(i16)`、`Full`（100%）、`Pct(i16)`（`0..=100`） |
| `padding` | `Edges<i8>`（4B） | `-128..=127px`（超过 127px 请用 `Div::safe_area` / `extra_padding`） |
| `bg` / `border_color` | `Color`（各 4B） | ARGB8888 (`0xAARRGGBB`)，提供 `.lerp()` 与 `.scale_alpha()` |
| `gap` | `i8`（1B） | 子节点间距（`-128..=127px`） |
| `rounded` | `u8`（1B） | 低 7 位圆角 `0..=127px`，最高位（bit 7）为 `flex_wrap` 折行开关 |
| `border_width` | `u8`（1B） | 低 4 位边框宽 `0..=15px`，高 4 位为紧凑统一外边距 `.m(0..=15)` |
| `flex_flags` | `u8`（1B） | `flex_dir(1b) \| justify(3b) \| items_align(2b) \| flex_grow(2b: 0..=3)` |

---

## Styled 与 Div 扩展链式 API

### `Styled` Trait（所有 `Div` 与包装组件通用）

- **布局方向与换行**：`.flex_row()`（默认）、`.flex_col()`、`.flex_wrap()`
- **尺寸策略**：`.size_full()`、`.w_full()`、`.h_full()`、`.w(i16)`、`.h(i16)`、`.w_pct(0..=100)`、`.h_pct(0..=100)`
- **弹性配比**：`.flex_grow(0..=3)`（注意：**Velox 没有 `.flex_1()`**，请使用 `.flex_grow(1)`）
- **内边距与紧凑外边距**：`.p(i8)`、`.px(i8)`、`.py(i8)`、`.pt(i8)`、`.pb(i8)`、`.pl(i8)`、`.pr(i8)`、`.m(0..=15)`、`.gap(i8)`
- **主/交叉轴对齐**：`.items_center()`、`.items_start()`、`.items_end()`、`.items_stretch()`（默认 Stretch）、`.justify_center()`、`.justify_between()`、`.justify_end()`、`.justify_around()`
- **外观**：`.bg(Color)`、`.rounded(0..=127)`、`.border(0..=15, Color)`

### `Div` 独占高级外观与定位方法（按需分配于 `ScratchArena`）

- **宽容大边距与最小/最大约束**：`.mx(i16)` / `.my(i16)` / `.mt(i16)` / `.mb(i16)` / `.ml(i16)` / `.mr(i16)`、`.min_w(i16)` / `.max_w(i16)` / `.min_h(i16)` / `.max_h(i16)`
- **自身交叉轴对齐与基线**：`.self_start()`、`.self_center()`、`.self_end()`、`.self_stretch()`、`.align_self(AlignItems)`、`.items_baseline()`
- **绝对叠层定位与浮层裁剪**：`.pos(x, y)`（脱离父线性流相对父内容区偏移）、`.overlay()`（提升至顶层 Z-Order 渲染）、`.occlude_hit()`（阻断点击穿透）、`.z_index(i8)`、`.overflow_hidden()`
- **滚动视口**：`.scrollable(max_scroll)` / `.scrollable_auto()`
- **高级视觉**：`.opacity(0..=255)`、`.hover_bg(Color)`、`.active_bg(Color)`、`.bg_gradient(Color)` / `.bg_gradient_x(Color)`、`.shadow(blur, ox, oy, Color)`

---

## TailwindStyled 主题预设扩展

在 `velox-component`（或开启 `component` feature，默认启用）中为所有实现 `Styled` 的类型提供 `TailwindStyled` 扩展方法：

- `.tw_card_surface(theme)`：仅在尚未设置自定义背景/边框/圆角时填充 `theme.card` + `border(1, theme.border)` + `rounded(8)`；
- `.tw_button_size(size)`：按 `ComponentSize` 应用预设高度与水平内边距；
- `.tw_button_variant(theme, variant)`：按 `ComponentVariant` 应用按钮语义背景、边框与 `rounded(8)`。

---

## 四步一维弹性布局求解器（Two-Pass Axis Solver）

`velox_core::solver` 放弃依赖硬件浮点的二维重载约束，采用纯 `i16` 整数“水平先行、垂直跟进（Height-for-Width）”四步法：

1. **Pass 1 (Horizontal Min/Pref Measure)**：自底向上测量每个节点的最小/首选宽度（含 `Text` 自动测宽）；
2. **Pass 2 (Horizontal Solve & Line Wrapping)**：自顶向下把可用宽度按固定像素 / 百分比 / `flex_grow` 分配给子项；若开启 `.flex_wrap()` 或 `.grid(cols)`，在此阶段完成折行与列宽分配；
3. **Pass 3 (Vertical Height-for-Width)**：拿到确定的横向宽度后，自底向上折算多行 `Text` 高度与弹性行高；
4. **Pass 4 (Assign Regions & Flatten)**：自顶向下落实屏幕 `Region { x, y, w, h }`，处理主/交叉轴对齐、`pos` 偏移与滚动平移，同时直接拍平为一维 `DrawOp` 与 `HitRegion`（支持增量缓存 `Pass-4-Only` 快路径）。

---

## Grid 二维网格与 SafeAreaInsets

- **二维网格 `.grid(cols)`**：在任意 `div()` 上调用 `.grid(3).gap(12)` 即可把子节点按 3 列等分网格排列；或直接复用高层组件 `GridLayout` + `GridTile`。
- **移动端安全区避让**：
  - `div().safe_area(cx)` 自动累加 `cx.safe_area()` 的四边与 `cx.keyboard_inset()` 底部软键盘高度（存储于 `extra_padding: Edges<i16>`，不会被 `i8` 截断）；
  - `div().pt_safe(cx)` / `div().pb_safe(cx)` 分别用于顶部导航与底部工具条。
