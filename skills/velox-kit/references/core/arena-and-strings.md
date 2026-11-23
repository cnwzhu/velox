# 帧内存池与零堆字符串（ScratchArena / SmallVec / StrRef / fmt!）

**Contents:** [瞬态帧内存池 ScratchArena](#瞬态帧内存池-scratcharena) · [零堆文本引用 StrRef 与 fmt!](#零堆文本引用-strref-与-fmt) · [自适应序列容器 SmallVec](#自适应序列容器-smallvec) · [常见生命周期陷阱与解决方案](#常见生命周期陷阱与解决方案)

---

## 瞬态帧内存池 ScratchArena

`velox_engine::arena::ScratchArena` 是 Velox 实现“渲染期零全局堆分配”的基础设施：

- 底层基于 `bumpalo::Bump` 实现连续页块弹性分配，在每帧或每次重建通过 `rewind()` O(1) 整体复用内存；
- 提供 `alloc` / `alloc_bytes` / `alloc_fmt` / `alloc_iter` 存放本帧文本、回调闭包与高亮切片，以及 `alloc_temp` / `alloc_temp_iter_exact` 存放瞬态树与布局求解节点。

| API | 返回类型 | 用途 |
| --- | --- | --- |
| `cx.arena().alloc(val)` | `&'a mut T` | 在帧池底端分配单个对象或闭包（如自定义事件包装闭包） |
| `cx.arena().alloc_temp(val)` | `&'a mut T` | 分配瞬态树节点，供求解器阶段使用 |
| `cx.arena().alloc_bytes(len)` | `&'a mut [u8]` | 分配零初始化字节缓冲 |
| `cx.arena().alloc_iter(iter)` | `&'a mut [T]` | 无需 `Vec` 中转，将迭代器直接展开为连续切片 `&'a mut [T]` |
| `cx.arena().alloc_fmt(args)` | `StrRef<'a>` | 格式化写入帧池（优先使用 `fmt!(cx, ...)` 宏） |
| `cx.arena().used_bytes()` | `usize` | 读取当前帧竞技场逻辑占用字节数（配合 `engine.last_arena_used_bytes()`） |

---

## 零堆文本引用 StrRef 与 fmt!

```rust
pub enum StrRef<'a> {
    Static(&'static str),
    Scratch(&'a str),
}
```

- 字面量 `"Hello"` 与 `fmt!(cx, "CPU: {}%", load)` 的结果均可隐式转换为 `StrRef<'a>`，直接传给 `text(...)`、`Button::new(...)`、`NavBar::new(...)`、`ArcGauge::label(...)`；
- 如果某个组件构造器签名接受 `&'a str`（如 `Table`、`FormField::help`、`Dialog::description`），请调用 `.as_str()`：`fmt!(cx, "Preview: {}", s).as_str()`；
- 对已存在的 `EditableText<CAP>` 或 `TextBuffer<CAP>`，调用 `self.buf.as_arena_str(cx)` 会把最新文本零堆分配复制到当前 `ScratchArena` 并返回 `&'a str`，解除 `&mut self` 与 `&self` 的借用冲突。

---

## 自适应序列容器 SmallVec

`SmallVec<T, N>` 为统一容量提示序列抽象（内部基于 `Vec<T>` 自动按需扩容，`push(&mut self, value: T) -> bool` 保持一致接口）：

- 当需要在 `render` 中构造数量动态的切片传入组件时，常规做法是先用 `SmallVec<T, N>` 收集，再调用 `cx.arena().alloc_iter(items.as_slice().iter().copied())` 转成 `&'a [T]`（参见 `examples/desktop_app/src/lib.rs` 中 `tokenize_rust_highlights`）。

---

## 常见生命周期陷阱与解决方案

1. **局部变量切片导致 `E0515` / `E0716`**：
   - 错误写法：`let items = &[SidebarItem::new(...)]; Sidebar::new(200, "App", items)`；
   - 正确写法：使用 `const ITEMS: &[SidebarItem<'static>] = &[...];`，或通过 `cx.arena().alloc_iter(...)` 将切片提升至 `'a` 生命周期。
2. **临时字符串超出作用域**：
   - 错误写法：在 `std` 下使用 `format!(...)` 生成 `String` 再传入 `&str`（`String` 在 `render` 返回时被 Drop）；
   - 正确写法：永远使用 `fmt!(cx, ...)` 或 `cx.arena().alloc_iter(...)`。
