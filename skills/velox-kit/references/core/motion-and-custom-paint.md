# 动画过渡与过程化 SDF 绘制（Transitions / Tween / Presence / img_shader）

**Contents:** [声明式 Div 过渡动画](#声明式-div-过渡动画) · [命令式补间器 Tween 与退场状态机 Presence](#命令式补间器-tween-与退场状态机-presence) · [缓动曲线与循环模式](#缓动曲线与循环模式) · [零堆位图贴图与过程化 SurfacePainter](#零堆位图贴图与过程化-surfacepainter)

---

## 声明式 Div 过渡动画

当元素拥有稳定 `u32` ID（`.id(u32)` 或 `.key(...)`）时，`VeloxEngine` 会在 `MotionCache` 中跟踪上一帧状态并自动插值（纯整数定点数运算，完成后自动静止进入 0% CPU `Wait`）：

```rust
div()
    .id(200)
    .w(240)
    .h(44)
    .bg(if self.active { theme.primary } else { theme.secondary })
    .animate_color(Duration::from_millis(160))
    .animate_bounds(Duration::from_millis(200))
    .ease(Easing::EaseOut)
```

| 方法 | 作用 |
| --- | --- |
| `.animate_color(dur)` / `.animate_bg(dur)` | 平滑插值背景色与边框色变更 |
| `.animate_bounds(dur)` | 平滑插值元素屏幕坐标与尺寸变更 |
| `.animate_opacity(dur)` | 平滑插值整体不透明度 `.opacity(0..=255)` |
| `.animate_all(dur)` | 同时开启颜色、边界与不透明度过渡 |
| `.enter_from(opacity, dx, dy, dur)` | 首次挂载时从起始透明度与偏移滑入 |
| `.fade_in(dur)` / `.slide_in(dx, dy, dur)` | 入场淡入与滑入快捷方式 |
| `.ping_pong_bg(from_bg, dur)` | 在 `from_bg` 与当前 `.bg(...)` 之间无限往复呼吸闪烁 |
| `.ease(Easing)` / `.duration(dur)` / `.delay(dur)` | 指定缓动曲线、覆盖时长或启动延迟（支持列表交错 stagger） |
| `.repeat_infinite()` / `.repeat_ping_pong()` | 设循环模式为 `LoopMode::Infinite` 或 `LoopMode::PingPong` |

---

## 命令式补间器 Tween 与退场状态机 Presence

当元素需要在隐藏后“延迟卸载直到退场动画播完”，或需要动画化非布局属性（仪表盘角度、进度值、旋转角度）时，把 `Copy` 的 `Tween` / `Presence` 存在 `View` 字段中：

```rust
struct PopupView {
    popup: Presence,
}

// 1. 事件中用 acx.now_ms() 触发入场/退场：
let on_toggle = cx.action(|this, acx| {
    this.popup.toggle(acx.now_ms());
});

// 2. 渲染中用 cx.now_ms() 采样并按需请求下一帧：
let now = cx.now_ms();
if self.popup.is_mounted(now) {
    root = root.child(
        cx,
        div()
            .id(90)
            .opacity(self.popup.opacity(now))
            .mt(self.popup.offset(now, 16))
            .child(cx, text("Fading Card")),
    );
}
// 当动画自然结束时返回 false，自动停止请求帧并回归 Wait 空闲休眠
cx.keep_animating(self.popup.is_animating(now));
```

`Tween` 核心方法：`Tween::new(val)`、`.with_easing(Easing)`、`.with_duration(ms)`、`.with_delay(ms)`、`animate_to(now_ms, to)`（打断进行中动画时从当前采样值连续平滑衔接）、`start(now_ms, from, to)`、`jump_to(val)`、`value(now_ms)`、`progress_256(now_ms)`、`is_running(now_ms)`。

---

## 缓动曲线与循环模式

`Easing` 内置 8 种纯粹基于整数定点数求值的曲线（无硬件浮点依赖）：

- `Linear`、`EaseIn`、`EaseOut`（默认）、`EaseInOut`
- `Spring`（二阶欠阻尼弹簧，带 ~8–10% 物理回弹）
- `EaseOutBack`（过冲回落，适合卡片与弹出面板入场）
- `Bounce`（落地多次弹跳）、`Elastic`（指数衰减弹性振荡）

在无障碍“减少动态效果”或单元测试需要快进到底时，可调用 `engine.finish_animations()` 立即结束所有单次过渡。

---

## 零堆位图贴图与过程化 SurfacePainter

除了内置 `div()` 与 `text()`，图标、贴图与矢量自定义画布编译为 `DrawPayload::Sprite`：

- **静态/压缩位图原语**：
  - `img(w, h, bytes, SpriteFormat)`
  - `svg(w, h, alpha_mask)`（A8 预烘焙单色图标，配合 `.tint(color)` 着色） / `svg_a4(w, h, a4_mask)`（体积减半）
  - `img_rle_a8` / `img_rle_rgb565`（零解压缓冲直出）
  - `img_rotated_rgb565(cx, w, h, sw, sh, bytes, angle_deg10, scale_q8)`（任意 0.1° 旋转 + 定点缩放）
- **过程化矢量画布 `img_shader`**：

```rust
img_shader(cx, 120, 120, hash_of_state, move |surface: &mut dyn SurfacePainter| {
    let b = surface.bounds();
    let clip = surface.clip(); // 仅绘制落在脏矩形与裁剪窗口内的像素！
    // 使用 velox_core::sdf::{dist_to_segment, sin_cos_deg10, isqrt_i32} + surface.blend_pixel(...)
    // 或 surface.draw_text(b, "42%", theme.foreground, 14)
})
```

`state_hash: u32` 必须反映所有影响画面输出的输入参数；`DamageTracker` 用 `state_hash` 判定该帧精灵内容是否真正变化。
