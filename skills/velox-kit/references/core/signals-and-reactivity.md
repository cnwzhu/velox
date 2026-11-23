# 细粒度响应式信号图（Signal / Computed / Effect）

**Contents:** [核心概念与零堆内存模型](#核心概念与零堆内存模型) · [Signal\<T\> 与自动依赖追踪](#signalt-与自动依赖追踪) · [Computed\<T\> 记忆化派生与 Effect 观察器](#computedt-记忆化派生与-effect-观察器) · [生命周期清理与使用注意事项](#生命周期清理与使用注意事项)

---

## 核心概念与零堆内存模型

`velox_core::signal` 提供无需宏观状态树 Diff 的细粒度响应式原语（由 `velox_kit::prelude::*` 全量导出）：

- **`Signal<T>`、`Computed<T>`、`Effect`** 均为 **4 字节 `Copy` 强类型句柄**（`id: u16` + `generation: u16` 防 ABA）；
- 存储约束：**内联值存储要求 `T: Copy + 'static`，`size_of::<T>() <= 16` 字节、`align_of::<T>() <= 8` 字节**（在编译后运行写入时检查断言，覆盖 `i32`、`u64`、`bool`、`Color`、`SimpleDate`、`Region`、轻量状态枚举等）；
- 动态扩容：线程局部信号表按需自动扩容 `Signal` 与 `Computed` 槽位。

---

## Signal\<T\> 与自动依赖追踪

```rust
use velox_kit::prelude::*;

struct MetricCard {
    load: Signal<u8>,
}

impl View for MetricCard {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        // 调用 self.load.get() 会自动将当前渲染的 PodId 注册为订阅者
        let current = self.load.get();
        div()
            .id(10)
            .w(180)
            .h(48)
            .child(cx, text(fmt!(cx, "Load: {}%", current)))
            .into_element(cx)
    }
}
```

### 常用 API 速查

| 方法 | 说明 |
| --- | --- |
| `signal(initial)` / `Signal::new(initial)` | 分配一个新信号句柄 |
| `.get()` | 读取值；若位于 `View::render`、`Computed` 或 `Effect` 期间，自动注册当前追踪订阅者 |
| `.get_in(cx)` | 显式以给定 `cx.current_pod()` 注册订阅并读取 |
| `.get_untracked()` | 纯只读读取当前值，不建立订阅依赖 |
| `.set(val)` / `.set_in(acx, val)` | 写入新值、递增版本号，并仅将读取过它的订阅 `PodId` 标记为脏 |
| `.set_if_changed(val)` | 仅当 `T: PartialEq` 且值确实改变时写入并标脏 |
| `.update(\|v\| *v += 1)` / `.update_in(acx, ...)` | 重入安全原地更新（执行闭包时不持有全局表借用） |
| `batch(\|\| { s1.set(a); s2.set(b); })` | 批量事务更新：合并多个信号变更，退出 `batch` 时一次性触发关联 `Effect` 与视图标脏 |
| `+=`, `-=`, `\|=`, `^=`, `==`, `Display` | 已直接为 `Signal<T>` 实现运算符与格式化特征（`fmt!(cx, "{}", sig)` 自动触发追踪读） |
| `Switch::bind(cx, id, sig)` 等 | 13 类常用受控组件内置零堆双向绑定构造器（自动 `sig.get()` + `sig.set_if_changed(v)`） |
| `.dispose()` | 释放信号槽位并递增 `generation` |

---

## Computed\<T\> 记忆化派生与 Effect 观察器

```rust
let count = signal(2i32);
let doubled = computed(move || count.get() * 2);

assert_eq!(doubled.get(), 4);
assert_eq!(doubled.get(), 4);
assert_eq!(doubled.eval_count(), 1); // 依赖未变，O(1) 返回缓存值

count.set(5);
assert_eq!(doubled.get(), 10);
assert_eq!(doubled.eval_count(), 2); // 源版本号递增后才重新求值
```

- **`Computed::new` / `computed(f)` / `computed_eq(f)`**：要求 `F: Fn() -> T + Copy + 'static`（可直接 `move` 捕获 4 字节 `Copy` 的 `Signal<T>` / `Computed<T>` 句柄），单个计算节点最多自动追踪 `MAX_DEPS_PER_NODE = 8` 个依赖源信号，禁止循环依赖；`computed_eq` 额外在派生值未改变时抑制下游无效重算；在 `View::render` 中读取 `doubled.get()` 命中缓存时，也会透传注册源 `Signal` 到当前 `PodId` 以便后续唤醒视图；
- **`Effect::new` / `effect(f)`**：创建时立即执行一次以采集依赖，之后任一被追踪 `Signal` 变更时自动同步触发；适合非渲染副作用（如硬件标志、统计累加器或调试日志），不再需要时调用 `eff.dispose()`。

---

## 生命周期清理与使用注意事项

- `PodPool` 回收或移除某个 `PodId` 时会自动清理它在信号表上的订阅掩码；每次进入 `View::render` 之前也会先清空该 `PodId` 上一帧旧依赖并按本帧实际执行路径重新采样（动态分支切换依赖零残留）；
- **UI 主线程独占约束（异步铁律）**：每个 `PodPool` 分配唯一非零 `pool_id`；`std` 模式下信号表存储在 `thread_local!` 中，**严禁在 `cx.background_spawn(...)` 后台工作线程内读写 `Signal<T>`**。异步任务必须 `.await` 回到前景 `acx.spawn` 协程后，再于 `this.update(&mut cx, |view, _| { view.sig.set(...); })` 内更新信号（`ForegroundExecutor::poll_ready` 会自动刷入脏 `PodId`）；
- **不要把大型结构体放进 `Signal<T>`**：大于 16 字节的数据（如长文本缓冲区、大数组）应作为普通字段存放在宿主 `View` 中，由 `PodPool` + `View` + `SubView` 边界管理刷新。
