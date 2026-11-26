//! 基础实时数据可视化图表组件 `Chart`（支持 `Line`、`Bar`、`Area`、`Pie`、`Candlestick` 五种图表类型，
//! 纯整数 SDF 抗锯齿绘制，支持 `gpui-kit` 风格入场渐进展开、数据平滑变形 Morphing、
//! 鼠标悬停列高亮、点击/拖拽刷选十字准星平滑追踪、饼图扇区弹性弹出与浮动数值 Tooltip）。

use velox_engine::prelude::*;
use velox_engine::sdf::dist_to_segment;

extern crate alloc;
use alloc::vec::Vec;
use core::cell::RefCell;
use std::collections::HashMap;

/// 图表展示类型枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ChartKind {
    /// 连续抗锯齿折线图（默认）。
    #[default]
    Line,
    /// 垂直柱状图。
    Bar,
    /// 面积填充折线图（垂直渐变衰减）。
    Area,
    /// 饼图 / 环形占比图（支持 `donut_ratio` 内径镂空与点击扇区弹性弹出）。
    Pie,
    /// K 线图（蜡烛图，OHLC）。
    Candlestick,
}

/// K 线图（`Candlestick`）单根蜡烛的开高低收（OHLC）数据点。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CandlestickBar {
    pub open: i16,
    pub high: i16,
    pub low: i16,
    pub close: i16,
}

impl CandlestickBar {
    /// 创建一根 OHLC 蜡烛数据点。
    #[inline(always)]
    pub const fn new(open: i16, high: i16, low: i16, close: i16) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }
}

/// 图表动画模式（入场渐进展开 / 数据切换平滑变形 Morphing）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum AnimMode {
    /// 入场展开：柱状图错峰生长、折线/面积从左到右描边、饼图顺时针扫掠、K 线错峰展开。
    Entrance,
    /// 数据变形：长度不变时在旧值与新值之间平滑插值过渡。
    Morph,
}

struct ChartAnimEntry {
    kind: ChartKind,
    data_hash: u64,
    last_epoch: Option<u32>,
    start_ms: u32,
    duration_ms: u32,
    mode: AnimMode,
    prev_data: Vec<i16>,
    curr_data: Vec<i16>,
    // 交互高亮状态：悬停实时追踪 + 点击锁定，支持 180ms 缓出焦点追踪动画
    hover_idx: Option<usize>,
    locked_idx: Option<usize>,
    prev_focus_idx: Option<usize>,
    focus_start_ms: u32,
}

std::thread_local! {
    static CHART_ANIM_CACHE: RefCell<HashMap<u32, ChartAnimEntry>> = RefCell::new(HashMap::new());
}

#[inline]
fn set_chart_hover_idx(chart_id: u32, idx: Option<usize>, now_ms: u32) -> bool {
    CHART_ANIM_CACHE.with(|cell| {
        let mut map = cell.borrow_mut();
        if let Some(entry) = map.get_mut(&chart_id) {
            let old_eff = entry.hover_idx.or(entry.locked_idx);
            if entry.hover_idx == idx {
                return false;
            }
            entry.hover_idx = idx;
            let new_eff = entry.hover_idx.or(entry.locked_idx);
            if old_eff != new_eff {
                entry.prev_focus_idx = old_eff;
                entry.focus_start_ms = now_ms;
                return true;
            }
        }
        false
    })
}

#[inline]
fn set_chart_locked_idx(chart_id: u32, idx: Option<usize>, toggle: bool, now_ms: u32) -> bool {
    CHART_ANIM_CACHE.with(|cell| {
        let mut map = cell.borrow_mut();
        if let Some(entry) = map.get_mut(&chart_id) {
            let old_eff = entry.hover_idx.or(entry.locked_idx);
            let next_locked = if toggle && entry.locked_idx == idx {
                None
            } else {
                idx
            };
            entry.locked_idx = next_locked;
            entry.hover_idx = next_locked;
            let new_eff = entry.hover_idx.or(entry.locked_idx);
            if old_eff != new_eff {
                entry.prev_focus_idx = old_eff;
                entry.focus_start_ms = now_ms;
            }
            return true;
        }
        false
    })
}

#[inline]
fn hash_chart_data(data: &[i16]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &v in data {
        h ^= (v as u16) as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 三次方缓出（`0..=256` -> `0..=256`），用于平滑图表入场、数据变形与交互焦点追踪。
#[inline(always)]
fn ease_out_cubic_256(t_256: i32) -> i32 {
    let t = t_256.clamp(0, 256);
    let inv = 256 - t;
    256 - ((inv * inv * inv) >> 16)
}

/// 错峰进度计算：在总体进度 `prog_256`（`0..=256`）下为第 `idx / count` 项分配交错起止窗。
#[inline]
fn staggered_progress_256(prog_256: i32, idx: usize, count: usize) -> i32 {
    if prog_256 >= 256 || count <= 1 {
        return ease_out_cubic_256(prog_256);
    }
    let max_delay = 104i32;
    let window = 256 - max_delay;
    let delay = ((idx as i32) * max_delay) / ((count - 1) as i32).max(1);
    let local = (((prog_256 - delay) * 256) / window).clamp(0, 256);
    ease_out_cubic_256(local)
}

/// 整数近似 1024 刻度顺时针极角（0 = 12 点钟，256 = 3 点钟，512 = 6 点钟，768 = 9 点钟）。
#[inline]
fn clockwise_angle_1024(dx: i32, dy: i32) -> i32 {
    let ux = dx.abs();
    let uy = dy.abs();
    if ux == 0 && uy == 0 {
        0
    } else if dx >= 0 && dy < 0 {
        if uy >= ux {
            (128 * ux) / uy.max(1)
        } else {
            256 - (128 * uy) / ux
        }
    } else if dx > 0 && dy >= 0 {
        if ux >= uy {
            256 + (128 * uy) / ux.max(1)
        } else {
            512 - (128 * ux) / uy
        }
    } else if dx <= 0 && dy > 0 {
        if uy >= ux {
            512 + (128 * ux) / uy.max(1)
        } else {
            768 - (128 * uy) / ux
        }
    } else if ux >= uy {
        768 + (128 * uy) / ux.max(1)
    } else {
        1024 - (128 * ux) / uy
    }
    .clamp(0, 1023)
}

/// 由 1024 刻度顺时针角度计算 `(sin_q8, -cos_q8)` 单位方向向量（用于饼图激活扇区弹性外移）。
#[inline]
fn angle_1024_dir_q8(angle_1024: i32) -> (i32, i32) {
    let deg10 = ((angle_1024.rem_euclid(1024) as i64 * 3600) / 1024) as i32;
    let (sin_q10, cos_q10) = velox_engine::sdf::sin_cos_deg10(deg10);
    // 12 点钟为 (0, -1)，顺时针转角 θ 对应 (sin θ, -cos θ)
    (sin_q10 >> 2, -(cos_q10 >> 2))
}

/// 基础实时数据图表组件（`Chart`）。
pub struct Chart<'a> {
    pub id: u32,
    pub width: i16,
    pub height: i16,
    pub data: &'a [i16],
    pub labels: &'a [&'a str],
    pub unit: &'a str,
    pub min_val: i16,
    pub max_val: i16,
    pub kind: ChartKind,
    pub series_color: Option<Color>,
    pub grid_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub grid_lines: u8,
    /// 是否启用 `gpui-kit` 风格入场渐进展开与数据平滑变形动画（`velox-component` `std` 下默认开启）。
    pub animated: bool,
    /// 是否启用鼠标悬停列高亮、点击/拖拽刷选十字准星追踪、饼图扇区弹出与顶部数值 Tooltip。
    pub interactive: bool,
    /// 动画总时长（毫秒，默认 `520ms`）。
    pub duration_ms: u16,
    /// 是否在折线/面积图数据节点处绘制抗锯齿高亮圆点。
    pub show_dots: bool,
    /// 是否为柱状图启用顶部柔和圆角。
    pub rounded_bars: bool,
    /// 饼图内圆镂空半径占比（`0` 为实心饼图，`45..=65` 为现代 Donut 环形图）。
    pub donut_ratio: u8,
    /// 可选受控高亮数据索引（`None` 时在 `std` 交互模式下由图表内置状态自动维护）。
    pub active_index: Option<usize>,
    /// 可选点击/拖拽刷选数据点时的回调。
    pub on_select: Option<&'a dyn Fn(usize, &mut ActionScope)>,
    /// 可选手动动画重播代数（变更该值时立即重播入场展开动画）。
    pub anim_epoch: Option<u32>,
    /// 可选显式绘制进度（`0..=255`；用于 `#![no_std]` 手动驱动或确定性快照）。
    pub manual_progress: Option<u8>,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Chart<'a> {
    /// 创建一个实时数据图表，传入组件 `id`、宽高、数据点切片 `data` 与数值量程 `(min_val, max_val)`。
    #[inline(always)]
    pub const fn new(
        id: u32,
        width: i16,
        height: i16,
        data: &'a [i16],
        min_val: i16,
        max_val: i16,
    ) -> Self {
        Self {
            id,
            width: if width < 24 { 24 } else { width },
            height: if height < 24 { 24 } else { height },
            data,
            labels: &[],
            unit: "",
            min_val,
            max_val: if max_val <= min_val {
                min_val.saturating_add(1)
            } else {
                max_val
            },
            kind: ChartKind::Line,
            series_color: None,
            grid_color: None,
            bg_color: None,
            grid_lines: 3,
            animated: false,
            interactive: false,
            duration_ms: 520,
            show_dots: false,
            rounded_bars: false,
            donut_ratio: 0,
            active_index: None,
            on_select: None,
            anim_epoch: None,
            manual_progress: None,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置图表类型（[`ChartKind::Line`]、[`ChartKind::Bar`]、[`ChartKind::Area`]、[`ChartKind::Pie`] 或 [`ChartKind::Candlestick`]）。
    #[inline(always)]
    pub const fn kind(mut self, kind: ChartKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置 K 线图（[`ChartKind::Candlestick`]）的 OHLC 数据序列并切换为蜡烛图模式。
    #[inline(always)]
    pub const fn candles(mut self, candles: &'a [CandlestickBar]) -> Self {
        self.data = unsafe {
            core::slice::from_raw_parts(candles.as_ptr() as *const i16, candles.len() * 4)
        };
        self.kind = ChartKind::Candlestick;
        self
    }

    /// 设置每个数据点/扇区的可选标签切片（用于交互悬浮 Tooltip 展示）。
    #[inline(always)]
    pub const fn labels(mut self, labels: &'a [&'a str]) -> Self {
        self.labels = labels;
        self
    }

    /// 设置数值显示后缀单位（如 `" FPS"`、`" KB"`、`"% "`）。
    #[inline(always)]
    pub const fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    /// 设置数据序列颜色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.series_color = Some(color);
        self
    }

    /// 设置背景与网格线颜色。
    #[inline(always)]
    pub const fn background_and_grid(mut self, bg: Color, grid: Color) -> Self {
        self.bg_color = Some(bg);
        self.grid_color = Some(grid);
        self
    }

    /// 设置水平参考网格线数量（默认 `3` 条）。
    #[inline(always)]
    pub const fn grid_lines(mut self, count: u8) -> Self {
        self.grid_lines = count;
        self
    }

    /// 启用或禁用 `gpui-kit` 风格入场渐进展开与数据平滑变形动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 启用或禁用鼠标悬停高亮列、点击/拖拽刷选十字准星追踪与浮动 Tooltip。
    #[inline(always)]
    pub const fn interactive(mut self, enabled: bool) -> Self {
        self.interactive = enabled;
        self
    }

    /// 设置动画持续时长（毫秒，默认 `520ms`）。
    #[inline(always)]
    pub const fn duration_ms(mut self, ms: u16) -> Self {
        self.duration_ms = if ms < 60 { 60 } else { ms };
        self
    }

    /// 是否在折线图/面积图数据节点处绘制抗锯齿高亮圆点。
    #[inline(always)]
    pub const fn show_dots(mut self, show: bool) -> Self {
        self.show_dots = show;
        self
    }

    /// 是否为柱状图启用顶部柔和圆角与高光渐变。
    #[inline(always)]
    pub const fn rounded_bars(mut self, rounded: bool) -> Self {
        self.rounded_bars = rounded;
        self
    }

    /// 设置饼图内圆镂空半径百分比（`0` 为实心饼图，`50` 为环形 Donut 图）。
    #[inline(always)]
    pub const fn donut_ratio(mut self, pct: u8) -> Self {
        self.donut_ratio = if pct > 80 { 80 } else { pct };
        self
    }

    /// 设置受控激活数据点索引。
    #[inline(always)]
    pub const fn active_index(mut self, idx: Option<usize>) -> Self {
        self.active_index = idx;
        self
    }

    /// 绑定点击/拖拽刷选数据点时的回调。
    #[inline(always)]
    pub fn on_select(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// 设置动画重播代数（变更该数值可立即重播入场展开动画）。
    #[inline(always)]
    pub const fn anim_epoch(mut self, epoch: u32) -> Self {
        self.anim_epoch = Some(epoch);
        self
    }

    /// 手动指定动画进度（`0..=255`），覆盖自动时钟推演。
    #[inline(always)]
    pub const fn progress(mut self, progress_u8: u8) -> Self {
        self.manual_progress = Some(progress_u8);
        self
    }
}

crate::impl_base_styled_box!(Chart<'a>);

impl<'a> Widget<'a> for Chart<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let theme = cx.theme();
        let fg = theme.foreground;
        let line_col = self.series_color.unwrap_or(fg);
        let grid_col = self.grid_color.unwrap_or_else(|| fg.scale_alpha(48));
        let bg_col = self.bg_color.unwrap_or(Color::TRANSPARENT);

        let w = StyleExt::resolve_width(self.style, self.width).max(24);
        let h = StyleExt::resolve_height(self.style, self.height).max(24);
        let min_v = self.min_val as i32;
        let range_v = ((self.max_val as i32) - min_v).max(1);
        let kind = self.kind;
        let grids = self.grid_lines;
        let show_dots = self.show_dots;
        let rounded_bars = self.rounded_bars;
        let donut_ratio = self.donut_ratio;
        let item_count = match kind {
            ChartKind::Candlestick => self.data.len() / 4,
            _ => self.data.len(),
        };

        // 推演入场/变形动画与交互焦点追踪动画状态
        let (eff_data, anim_mode, raw_prog_256, active_idx, prev_focus_idx, focus_t_256): (
            &'a [i16],
            AnimMode,
            i32,
            Option<usize>,
            Option<usize>,
            i32,
        ) = if let Some(mp) = self.manual_progress {
            let p = if mp == 255 { 256 } else { mp as i32 };
            (self.data, AnimMode::Entrance, p, self.active_index, self.active_index, 256)
        } else if self.animated || self.interactive {
            {
                let now_ms = cx.now_ms();
                let dur_ms = (self.duration_ms.max(60)) as u32;
                let dhash = hash_chart_data(self.data);
                let mut mode_out = AnimMode::Entrance;
                let mut prog_out = 256i32;
                let mut morphed_buf: Option<Vec<i16>> = None;
                let mut active_out = self.active_index;
                let mut prev_focus_out = self.active_index;
                let mut focus_t_out = 256i32;

                CHART_ANIM_CACHE.with(|cell| {
                    let mut map = cell.borrow_mut();
                    if map.len() > 128 && !map.contains_key(&self.id) {
                        map.clear();
                    }
                    let entry = map.entry(self.id).or_insert_with(|| ChartAnimEntry {
                        kind,
                        data_hash: dhash,
                        last_epoch: self.anim_epoch,
                        start_ms: now_ms,
                        duration_ms: if self.animated { dur_ms } else { 0 },
                        mode: AnimMode::Entrance,
                        prev_data: self.data.to_vec(),
                        curr_data: self.data.to_vec(),
                        hover_idx: None,
                        locked_idx: self.active_index,
                        prev_focus_idx: self.active_index,
                        focus_start_ms: now_ms,
                    });

                    if let Some(ctrl_idx) = self.active_index {
                        let old_eff = entry.hover_idx.or(entry.locked_idx);
                        if old_eff != Some(ctrl_idx) {
                            entry.prev_focus_idx = old_eff;
                            entry.locked_idx = Some(ctrl_idx);
                            entry.hover_idx = None;
                            entry.focus_start_ms = now_ms;
                        }
                    } else {
                        if entry.hover_idx.is_some_and(|idx| idx >= item_count) {
                            entry.hover_idx = None;
                        }
                        if entry.locked_idx.is_some_and(|idx| idx >= item_count) {
                            entry.locked_idx = None;
                        }
                    }

                    let epoch_changed = self.anim_epoch != entry.last_epoch;
                    let kind_changed = entry.kind != kind;
                    let data_changed = entry.data_hash != dhash;

                    if epoch_changed || kind_changed {
                        entry.kind = kind;
                        entry.data_hash = dhash;
                        entry.last_epoch = self.anim_epoch;
                        entry.start_ms = now_ms;
                        entry.duration_ms = if self.animated { dur_ms } else { 0 };
                        entry.mode = AnimMode::Entrance;
                        entry.prev_data.clear();
                        entry.prev_data.extend_from_slice(self.data);
                        entry.curr_data.clear();
                        entry.curr_data.extend_from_slice(self.data);
                    } else if data_changed && self.animated {
                        if entry.curr_data.len() == self.data.len() && !self.data.is_empty() {
                            let prev_elapsed = now_ms.wrapping_sub(entry.start_ms);
                            let prev_t = if entry.duration_ms == 0 || prev_elapsed >= entry.duration_ms {
                                256
                            } else {
                                ease_out_cubic_256(
                                    ((prev_elapsed as u64 * 256) / entry.duration_ms.max(1) as u64)
                                        as i32,
                                )
                            };
                            let mut snap = Vec::with_capacity(self.data.len());
                            for i in 0..self.data.len() {
                                let a = entry.prev_data.get(i).copied().unwrap_or(entry.curr_data[i]) as i32;
                                let b = entry.curr_data[i] as i32;
                                let cur = if entry.mode == AnimMode::Morph {
                                    a + (((b - a) * prev_t) >> 8)
                                } else {
                                    b
                                };
                                snap.push(cur.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
                            }
                            entry.prev_data = snap;
                            entry.curr_data.clear();
                            entry.curr_data.extend_from_slice(self.data);
                            entry.mode = AnimMode::Morph;
                            entry.duration_ms = (dur_ms * 3 / 4).max(240);
                        } else {
                            entry.prev_data.clear();
                            entry.prev_data.extend_from_slice(self.data);
                            entry.curr_data.clear();
                            entry.curr_data.extend_from_slice(self.data);
                            entry.mode = AnimMode::Entrance;
                            entry.duration_ms = dur_ms;
                        }
                        entry.data_hash = dhash;
                        entry.start_ms = now_ms;
                    } else if data_changed {
                        entry.data_hash = dhash;
                        entry.curr_data.clear();
                        entry.curr_data.extend_from_slice(self.data);
                    }

                    let elapsed = now_ms.wrapping_sub(entry.start_ms);
                    let active = self.animated && entry.duration_ms > 0 && elapsed < entry.duration_ms;
                    let p = if !active {
                        256
                    } else {
                        ((elapsed as u64 * 256) / entry.duration_ms.max(1) as u64).min(256) as i32
                    };
                    mode_out = entry.mode;
                    prog_out = p;

                    if active && entry.mode == AnimMode::Morph && entry.prev_data.len() == entry.curr_data.len() {
                        let eased = ease_out_cubic_256(p);
                        let mut interp = Vec::with_capacity(entry.curr_data.len());
                        for i in 0..entry.curr_data.len() {
                            let a = entry.prev_data[i] as i32;
                            let b = entry.curr_data[i] as i32;
                            let v = a + (((b - a) * eased) >> 8);
                            interp.push(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
                        }
                        morphed_buf = Some(interp);
                    }

                    // 交互焦点切换平滑追踪（180ms 缓出：十字准星滑行 / 饼图扇区弹性弹出）
                    const FOCUS_DUR_MS: u32 = 180;
                    active_out = self.active_index.or(entry.hover_idx).or(entry.locked_idx);
                    prev_focus_out = entry.prev_focus_idx;
                    let f_elapsed = now_ms.wrapping_sub(entry.focus_start_ms);
                    focus_t_out = if self.animated && f_elapsed < FOCUS_DUR_MS {
                        ease_out_cubic_256(((f_elapsed as u64 * 256) / FOCUS_DUR_MS as u64) as i32)
                    } else {
                        256
                    };
                });

                if prog_out < 256 || focus_t_out < 256 {
                    cx.request_frame();
                }
                let slice_ref: &'a [i16] = if let Some(buf) = morphed_buf {
                    cx.arena().alloc_iter(buf.into_iter())
                } else {
                    self.data
                };
                (slice_ref, mode_out, prog_out, active_out, prev_focus_out, focus_t_out)
            }
        } else {
            (self.data, AnimMode::Entrance, 256, self.active_index, self.active_index, 256)
        };

        let entrance_prog = if anim_mode == AnimMode::Entrance {
            raw_prog_256
        } else {
            256
        };
        let eased_prog = ease_out_cubic_256(entrance_prog);
        let data = eff_data;

        let mut hash = self.id
            ^ ((w as u32) << 16)
            ^ (h as u32)
            ^ ((kind as u32) << 8)
            ^ (grids as u32)
            ^ ((donut_ratio as u32) << 12)
            ^ (if show_dots { 0x1000_0000 } else { 0 })
            ^ (if rounded_bars { 0x2000_0000 } else { 0 });
        hash = hash.rotate_left(5) ^ line_col.0;
        hash = hash.rotate_left(5) ^ bg_col.0;
        hash = hash.rotate_left(5) ^ grid_col.0;
        hash = hash.rotate_left(5) ^ (entrance_prog as u32);
        hash = hash.rotate_left(5) ^ (active_idx.map(|i| i as u32 + 1).unwrap_or(0));
        hash = hash.rotate_left(5) ^ (prev_focus_idx.map(|i| i as u32 + 1).unwrap_or(0));
        hash = hash.rotate_left(5) ^ (focus_t_256 as u32);
        hash = hash.rotate_left(5)
            ^ (((self.min_val as u16 as u32) << 16) | (self.max_val as u16 as u32));
        for &v in data {
            hash = hash.rotate_left(5) ^ (v as u16 as u32);
        }

        let crosshair_col = theme.foreground.scale_alpha(120);
        let chart_img = img_shader(cx, w, h, hash, move |painter: &mut dyn SurfacePainter| {
            let b = painter.bounds();
            let c = painter.clip();
            if c.is_empty() || b.w <= 4 || b.h <= 4 {
                return;
            }

            let draw_bg = bg_col.a() > 0;
            for py in c.y..c.bottom() {
                let rel_y = py - b.y;
                let mut is_grid = false;
                if grids > 0 && kind != ChartKind::Pie {
                    for g in 1..=grids {
                        let gy = ((b.h as i32) * (g as i32)) / ((grids as i32) + 1);
                        if (rel_y as i32) == gy {
                            is_grid = true;
                            break;
                        }
                    }
                }
                for px in c.x..c.right() {
                    if is_grid && ((px - b.x) & 1) == 0 {
                        painter.blend_pixel(px, py, grid_col, 165);
                    } else if draw_bg {
                        painter.blend_pixel(px, py, bg_col, bg_col.a());
                    }
                }
            }

            let pad = 6i32;
            let plot_w = ((b.w as i32) - pad * 2).max(1);
            let plot_h = ((b.h as i32) - pad * 2).max(1);

            match kind {
                ChartKind::Bar => {
                    if data.is_empty() {
                        return;
                    }
                    let n = data.len();
                    let slot_w = (plot_w / (n as i32)).max(3);
                    let gap = if slot_w >= 8 { 3 } else { 1 };
                    let bar_w = (slot_w - gap).max(1);
                    let top_r = if rounded_bars { (bar_w / 3).clamp(1, 4) } else { 0 };
                    let bottom_y = (b.bottom() as i32) - pad;

                    for (i, &val) in data.iter().enumerate() {
                        let item_p = staggered_progress_256(entrance_prog, i, n);
                        if item_p <= 0 {
                            continue;
                        }
                        let is_active = active_idx == Some(i);
                        let has_any_active = active_idx.is_some();
                        let base_col = if is_active {
                            line_col.lerp(Color::WHITE, ((46 * focus_t_256) >> 8) as u16)
                        } else if has_any_active {
                            line_col.scale_alpha(155)
                        } else {
                            line_col
                        };
                        let top_hi = base_col.lerp(Color::WHITE, if is_active { 56 } else { 28 });

                        let full_h = (((val as i32) - min_v).clamp(0, range_v) * plot_h) / range_v;
                        // 激活柱子轻微向上弹起 2px，呈现 gpui-kit 弹性交互反馈
                        let boost = if is_active { (2 * focus_t_256) >> 8 } else { 0 };
                        let anim_h = (((full_h * item_p) >> 8) + boost).max(1);
                        let bx0 = (b.x as i32) + pad + (i as i32) * slot_w + gap / 2;
                        let bx1 = bx0 + bar_w;
                        let by1 = bottom_y;
                        let by0 = (by1 - anim_h).max((b.y as i32) + pad);

                        let x_start = (bx0 as i16).max(c.x);
                        let x_end = (bx1 as i16).min(c.right());
                        let y_start = (by0 as i16).max(c.y);
                        let y_end = (by1 as i16).min(c.bottom());
                        let span_h = (by1 - by0).max(1);

                        for py in y_start..y_end {
                            let dy_top = (py as i32) - by0;
                            let grad_t = ((dy_top * 256) / span_h).clamp(0, 256) as u16;
                            let row_col = top_hi.lerp(base_col, grad_t);
                            for px in x_start..x_end {
                                let mut alpha = base_col.a();
                                if top_r > 0 && dy_top < top_r {
                                    let dx_l = (px as i32) - bx0;
                                    let dx_r = (bx1 - 1) - (px as i32);
                                    let edge_dx = if dx_l < top_r {
                                        top_r - dx_l
                                    } else if dx_r < top_r {
                                        top_r - dx_r
                                    } else {
                                        0
                                    };
                                    if edge_dx > 0 {
                                        let dy_c = top_r - dy_top;
                                        let d2 = edge_dx * edge_dx + dy_c * dy_c;
                                        let r2 = top_r * top_r;
                                        if d2 > r2 + top_r {
                                            alpha = 0;
                                        } else if d2 > r2 {
                                            let aa = (((r2 + top_r - d2) * 255) / top_r.max(1))
                                                .clamp(0, 255) as u16;
                                            alpha = ((alpha as u16 * aa) / 255) as u8;
                                        }
                                    }
                                }
                                if alpha > 0 {
                                    painter.blend_pixel(px, py, row_col, alpha);
                                }
                            }
                        }
                    }
                }
                ChartKind::Line | ChartKind::Area => {
                    if data.len() < 2 {
                        return;
                    }
                    let last_idx = (data.len() - 1) as i32;
                    let bottom_y = (b.bottom() as i32) - pad;
                    let reveal_x = (b.x as i32) + pad + ((plot_w * eased_prog) >> 8);
                    let mut tip_pos: Option<(i32, i32)> = None;

                    for seg in 0..(data.len() - 1) {
                        let v0 = ((data[seg] as i32) - min_v).clamp(0, range_v);
                        let v1 = ((data[seg + 1] as i32) - min_v).clamp(0, range_v);

                        let x0 = (b.x as i32) + pad + ((seg as i32) * plot_w) / last_idx;
                        let x1 = (b.x as i32) + pad + (((seg + 1) as i32) * plot_w) / last_idx;
                        let y0 = bottom_y - (v0 * plot_h) / range_v;
                        let y1 = bottom_y - (v1 * plot_h) / range_v;

                        if x0 > reveal_x {
                            break;
                        }
                        let (eff_x1, eff_y1) = if x1 > reveal_x && x1 > x0 {
                            let ty = y0 + ((y1 - y0) * (reveal_x - x0)) / (x1 - x0);
                            tip_pos = Some((reveal_x, ty));
                            (reveal_x, ty)
                        } else {
                            tip_pos = Some((x1, y1));
                            (x1, y1)
                        };

                        if kind == ChartKind::Area && eff_x1 > x0 {
                            let fill_x0 = (x0 as i16).max(c.x);
                            let fill_x1 = (eff_x1 as i16).min(c.right());
                            let seg_dx = (eff_x1 - x0).max(1);
                            for px in fill_x0..fill_x1 {
                                let t = (px as i32) - x0;
                                let top_y = y0 + ((eff_y1 - y0) * t) / seg_dx;
                                let fy0 = (top_y as i16).max(c.y);
                                let fy1 = (bottom_y as i16).min(c.bottom());
                                let col_h = (bottom_y - top_y).max(1);
                                for py in fy0..fy1 {
                                    let dist = ((py as i32) - top_y).clamp(0, col_h);
                                    let a = (112 - ((dist * 104) / col_h)).clamp(8, 112) as u8;
                                    painter.blend_pixel(px, py, line_col, a);
                                }
                            }
                        }

                        let min_x = (x0.min(eff_x1) - 3).max(c.x as i32) as i16;
                        let max_x = (x0.max(eff_x1) + 4).min(c.right() as i32) as i16;
                        let min_y = (y0.min(eff_y1) - 3).max(c.y as i32) as i16;
                        let max_y = (y0.max(eff_y1) + 4).min(c.bottom() as i32) as i16;

                        for py in min_y..max_y {
                            for px in min_x..max_x {
                                let d16 = dist_to_segment(
                                    (px as i32) * 16,
                                    (py as i32) * 16,
                                    x0 * 16,
                                    y0 * 16,
                                    eff_x1 * 16,
                                    eff_y1 * 16,
                                );
                                if d16 < 28 {
                                    let alpha = if d16 <= 12 {
                                        255u8
                                    } else {
                                        (((28 - d16) * 255) / 16) as u8
                                    };
                                    painter.blend_pixel(px, py, line_col, alpha);
                                }
                            }
                        }
                    }

                    let draw_dot = |painter: &mut dyn SurfacePainter,
                                    cx_p: i32,
                                    cy_p: i32,
                                    outer_r: i32,
                                    ring_col: Color,
                                    core_col: Color,
                                    halo_alpha: u8| {
                        let hr = if halo_alpha > 0 { outer_r + 4 } else { outer_r + 1 };
                        let x0 = (cx_p - hr).max(c.x as i32) as i16;
                        let x1 = (cx_p + hr + 1).min(c.right() as i32) as i16;
                        let y0 = (cy_p - hr).max(c.y as i32) as i16;
                        let y1 = (cy_p + hr + 1).min(c.bottom() as i32) as i16;
                        let r2 = outer_r * outer_r;
                        let halo_r2 = (outer_r + 4) * (outer_r + 4);
                        let core_r2 = (outer_r - 2).max(1) * (outer_r - 2).max(1);
                        for py in y0..y1 {
                            let dy = (py as i32) - cy_p;
                            for px in x0..x1 {
                                let dx = (px as i32) - cx_p;
                                let d2 = dx * dx + dy * dy;
                                if d2 <= r2 {
                                    let col = if d2 <= core_r2 { core_col } else { ring_col };
                                    painter.blend_pixel(px, py, col, 255);
                                } else if halo_alpha > 0 && d2 <= halo_r2 {
                                    let span = (halo_r2 - r2).max(1);
                                    let a = (((halo_r2 - d2) * halo_alpha as i32) / span)
                                        .clamp(0, halo_alpha as i32) as u8;
                                    painter.blend_pixel(px, py, ring_col, a);
                                } else if d2 <= r2 + outer_r * 2 {
                                    let a = (((r2 + outer_r * 2 - d2) * 200)
                                        / (outer_r * 2).max(1))
                                    .clamp(0, 200) as u8;
                                    painter.blend_pixel(px, py, ring_col, a);
                                }
                            }
                        }
                    };

                    if show_dots {
                        let core = if bg_col.a() > 0 { bg_col } else { Color::WHITE };
                        for (idx, &val) in data.iter().enumerate() {
                            let px_i = (b.x as i32) + pad + ((idx as i32) * plot_w) / last_idx;
                            if px_i > reveal_x {
                                break;
                            }
                            let v = ((val as i32) - min_v).clamp(0, range_v);
                            let py_i = bottom_y - (v * plot_h) / range_v;
                            draw_dot(painter, px_i, py_i, 3, line_col, core, 0);
                        }
                    }

                    // 交互激活点：平滑滑行十字准星 + 外发光脉冲圆点
                    if let Some(aidx) = active_idx {
                        if aidx < data.len() {
                            let target_x = (b.x as i32) + pad + ((aidx as i32) * plot_w) / last_idx;
                            let target_v = ((data[aidx] as i32) - min_v).clamp(0, range_v);
                            let target_y = bottom_y - (target_v * plot_h) / range_v;
                            let (cur_x, cur_y) = if let Some(pidx) = prev_focus_idx {
                                if pidx < data.len() && focus_t_256 < 256 {
                                    let px0 = (b.x as i32) + pad + ((pidx as i32) * plot_w) / last_idx;
                                    let pv0 = ((data[pidx] as i32) - min_v).clamp(0, range_v);
                                    let py0 = bottom_y - (pv0 * plot_h) / range_v;
                                    (
                                        px0 + (((target_x - px0) * focus_t_256) >> 8),
                                        py0 + (((target_y - py0) * focus_t_256) >> 8),
                                    )
                                } else {
                                    (target_x, target_y)
                                }
                            } else {
                                (target_x, target_y)
                            };

                            // 垂直虚线十字准星
                            let cx_i16 = cur_x as i16;
                            if cx_i16 >= c.x && cx_i16 < c.right() {
                                let y0 = ((b.y as i32 + pad) as i16).max(c.y);
                                let y1 = (bottom_y as i16).min(c.bottom());
                                for py in y0..y1 {
                                    if ((py - b.y) & 3) < 2 {
                                        painter.blend_pixel(cx_i16, py, crosshair_col, 175);
                                    }
                                }
                            }
                            draw_dot(
                                painter,
                                cur_x,
                                cur_y,
                                5,
                                line_col.lerp(Color::WHITE, 64),
                                Color::WHITE,
                                ((110 * focus_t_256) >> 8) as u8,
                            );
                        }
                    }

                    if entrance_prog < 256 {
                        if let Some((tx, ty)) = tip_pos {
                            draw_dot(
                                painter,
                                tx,
                                ty,
                                4,
                                line_col.lerp(Color::WHITE, 80),
                                Color::WHITE,
                                80,
                            );
                        }
                    }
                }
                ChartKind::Pie => {
                    if data.is_empty() {
                        return;
                    }
                    let base_cx = (b.x as i32) + (b.w as i32) / 2;
                    let base_cy = (b.y as i32) + (b.h as i32) / 2;
                    // 为激活扇区弹性外移留出 5px 呼吸空间
                    let radius = ((plot_w.min(plot_h) / 2) - 4).max(4);
                    let r_sq = radius * radius;
                    let inner_r_sq = (radius - 1).max(0) * (radius - 1).max(0);
                    let hole_r = ((radius * (donut_ratio as i32)) / 100)
                        .min(radius - 3)
                        .max(0);
                    let hole_r_sq = hole_r * hole_r;
                    let hole_aa_sq = (hole_r + 1) * (hole_r + 1);
                    let max_sweep_angle = (1024 * eased_prog) >> 8;

                    let palette = [
                        line_col,
                        Color::rgb(16, 185, 129),
                        Color::rgb(245, 158, 11),
                        Color::rgb(239, 68, 68),
                        Color::rgb(168, 85, 247),
                        Color::rgb(14, 165, 233),
                        Color::rgb(236, 72, 153),
                        Color::rgb(132, 204, 22),
                    ];
                    let total: i64 = data.iter().map(|&v| (v as i64).max(0)).sum();

                    // 计算激活扇区的中心角与平滑径向弹出位移（0..=5px）
                    let (exp_slice, exp_dx, exp_dy) = if let Some(aidx) = active_idx {
                        if aidx < data.len() && total > 0 {
                            let mut acc = 0i64;
                            for &v in &data[..aidx] {
                                acc += (v as i64).max(0);
                            }
                            let cur = (data[aidx] as i64).max(0);
                            let mid_ang = (((acc * 2 + cur) * 512) / total) as i32;
                            let (dir_x, dir_y) = angle_1024_dir_q8(mid_ang);
                            let dist = (5 * focus_t_256) >> 8;
                            (Some(aidx), (dir_x * dist) >> 8, (dir_y * dist) >> 8)
                        } else {
                            (None, 0, 0)
                        }
                    } else {
                        (None, 0, 0)
                    };

                    let slice_at = |ang: i32| -> usize {
                        if total > 0 {
                            let target = ((ang as i64) * total) / 1024;
                            let mut acc = 0i64;
                            let mut chosen = data.len() - 1;
                            for (idx, &v) in data.iter().enumerate() {
                                acc += (v as i64).max(0);
                                if target < acc {
                                    chosen = idx;
                                    break;
                                }
                            }
                            chosen
                        } else {
                            ((ang as usize) * data.len()) / 1024
                        }
                    };

                    for py in c.y..c.bottom() {
                        for px in c.x..c.right() {
                            // 优先检测是否命中弹出中的激活扇区
                            let mut drawn_active = false;
                            if let Some(es) = exp_slice {
                                let dx = (px as i32) - (base_cx + exp_dx);
                                let dy = (py as i32) - (base_cy + exp_dy);
                                let d2 = dx * dx + dy * dy;
                                if d2 <= r_sq && (hole_r == 0 || d2 >= hole_r_sq) {
                                    let ang = clockwise_angle_1024(dx, dy);
                                    if ang <= max_sweep_angle && slice_at(ang) == es {
                                        let col = palette[es % palette.len()]
                                            .lerp(Color::WHITE, ((36 * focus_t_256) >> 8) as u16);
                                        let mut alpha = if d2 <= inner_r_sq {
                                            255u8
                                        } else {
                                            let span = (r_sq - inner_r_sq).max(1);
                                            (((r_sq - d2) * 255) / span).clamp(0, 255) as u8
                                        };
                                        if hole_r > 0 && d2 < hole_aa_sq {
                                            let span = (hole_aa_sq - hole_r_sq).max(1);
                                            let ha = (((d2 - hole_r_sq) * 255) / span)
                                                .clamp(0, 255) as u8;
                                            alpha = ((alpha as u16 * ha as u16) / 255) as u8;
                                        }
                                        if alpha > 0 {
                                            painter.blend_pixel(px, py, col, alpha);
                                        }
                                        drawn_active = true;
                                    }
                                }
                            }
                            if drawn_active {
                                continue;
                            }

                            let dx = (px as i32) - base_cx;
                            let dy = (py as i32) - base_cy;
                            let d2 = dx * dx + dy * dy;
                            if d2 <= r_sq && (hole_r == 0 || d2 >= hole_r_sq) {
                                let ang = clockwise_angle_1024(dx, dy);
                                if ang > max_sweep_angle {
                                    continue;
                                }
                                let sidx = slice_at(ang);
                                if exp_slice == Some(sidx) {
                                    continue;
                                }
                                let col = if exp_slice.is_some() {
                                    palette[sidx % palette.len()].scale_alpha(165)
                                } else {
                                    palette[sidx % palette.len()]
                                };
                                let mut alpha = if d2 <= inner_r_sq {
                                    col.a()
                                } else {
                                    let span = (r_sq - inner_r_sq).max(1);
                                    let aa = (((r_sq - d2) * 255) / span).clamp(0, 255) as u16;
                                    ((col.a() as u16 * aa) / 255) as u8
                                };
                                if hole_r > 0 && d2 < hole_aa_sq {
                                    let span = (hole_aa_sq - hole_r_sq).max(1);
                                    let ha = (((d2 - hole_r_sq) * 255) / span).clamp(0, 255) as u16;
                                    alpha = ((alpha as u16 * ha) / 255) as u8;
                                }
                                if alpha > 0 {
                                    painter.blend_pixel(px, py, col, alpha);
                                }
                            }
                        }
                    }
                }
                ChartKind::Candlestick => {
                    let count = data.len() / 4;
                    if count == 0 {
                        return;
                    }
                    let slot_w = (plot_w / (count as i32)).max(4);
                    let body_w = (slot_w - 2).max(2);
                    let bottom_y = (b.bottom() as i32) - pad;
                    let bull_col = Color::rgb(34, 197, 94);
                    let bear_col = Color::rgb(239, 68, 68);

                    for i in 0..count {
                        let item_p = staggered_progress_256(entrance_prog, i, count);
                        if item_p <= 0 {
                            continue;
                        }
                        let bar = CandlestickBar::new(
                            data[i * 4],
                            data[i * 4 + 1],
                            data[i * 4 + 2],
                            data[i * 4 + 3],
                        );
                        let is_active = active_idx == Some(i);
                        let mut col = if bar.close >= bar.open {
                            bull_col
                        } else {
                            bear_col
                        };
                        if is_active {
                            col = col.lerp(Color::WHITE, ((48 * focus_t_256) >> 8) as u16);
                        } else if active_idx.is_some() {
                            col = col.scale_alpha(150);
                        }
                        let map_y = |v: i16| -> i32 {
                            let clamped = ((v as i32) - min_v).clamp(0, range_v);
                            bottom_y - (clamped * plot_h) / range_v
                        };
                        let y_open = map_y(bar.open);
                        let lerp_y = |target: i32| y_open + (((target - y_open) * item_p) >> 8);
                        let y_high = lerp_y(map_y(bar.high));
                        let y_low = lerp_y(map_y(bar.low));
                        let y_close = lerp_y(map_y(bar.close));

                        let bx0 = (b.x as i32) + pad + (i as i32) * slot_w;
                        let wick_x = (bx0 + body_w / 2) as i16;
                        if is_active && wick_x >= c.x && wick_x < c.right() {
                            let gy0 = ((b.y as i32 + pad) as i16).max(c.y);
                            let gy1 = (bottom_y as i16).min(c.bottom());
                            for py in gy0..gy1 {
                                if ((py - b.y) & 3) < 2 {
                                    painter.blend_pixel(wick_x, py, crosshair_col, 160);
                                }
                            }
                        }
                        if wick_x >= c.x && wick_x < c.right() {
                            let wy0 = (y_high.min(y_low) as i16).max(c.y);
                            let wy1 = (y_high.max(y_low) as i16).min(c.bottom());
                            for py in wy0..wy1 {
                                painter.blend_pixel(wick_x, py, col, col.a());
                            }
                        }

                        let by0 = (y_open.min(y_close) as i16).max(c.y);
                        let by1 = ((y_open.max(y_close) + 1) as i16).min(c.bottom());
                        let x0 = (bx0 as i16).max(c.x);
                        let x1 = ((bx0 + body_w) as i16).min(c.right());
                        for py in by0..by1 {
                            for px in x0..x1 {
                                painter.blend_pixel(px, py, col, col.a());
                            }
                        }
                    }
                }
            }
        });

        // 非交互模式保持最轻量的单节点直出（零额外包装）
        if !self.interactive || item_count == 0 {
            let elem = chart_img.into_element(cx);
            return self.style_ext.wrap_element(cx, elem, self.style);
        }

        // 交互模式（gpui-kit 风格）：
        // 1. 底层渲染抗锯齿动画图表；
        // 2. 鼠标悬停实时驱动十字准星滑行、发光脉冲点、柱子微弹、饼图扇区径向弹出与 120fps 列高亮；
        // 3. 点击锁定/解除数据点、拖拽支持触摸刷选；顶部弹性浮动 Tooltip 实时跟随当前激活项。
        let chart_id = self.id;
        let on_select_cb = self.on_select;
        let mut root = div()
            .id(chart_id)
            .w(w)
            .h(h)
            .bg(bg_col)
            .border(1, grid_col.scale_alpha(75))
            .rounded(6)
            .overflow_hidden()
            .child(cx, chart_img);

        let pad_i16 = 6i16;
        let plot_w_i16 = (w - pad_i16 * 2).max(1);

        if kind == ChartKind::Pie {
            // 饼图/环形图：鼠标悬停或拖拽按极角实时命中对应扇区并平滑径向弹出 5px
            let raw_data = self.data;
            let total_pie: i64 = raw_data.iter().map(|&v| (v as i64).max(0)).sum();
            let pick_pie_idx = move |lx: i16, ly: i16, bw: i16, bh: i16| -> usize {
                let dx = (lx as i32) - (bw as i32) / 2;
                let dy = (ly as i32) - (bh as i32) / 2;
                let ang = clockwise_angle_1024(dx, dy);
                if total_pie > 0 {
                    let target = ((ang as i64) * total_pie) / 1024;
                    let mut acc = 0i64;
                    let mut chosen = raw_data.len().saturating_sub(1);
                    for (idx, &v) in raw_data.iter().enumerate() {
                        acc += (v as i64).max(0);
                        if target < acc {
                            chosen = idx;
                            break;
                        }
                    }
                    chosen
                } else {
                    ((ang as usize) * raw_data.len()) / 1024
                }
            };
            let on_pie_move = cx.arena().alloc(move |ev: DragEvent, acx: &mut ActionScope| {
                let idx = pick_pie_idx(ev.local_x, ev.local_y, ev.width, ev.height);
                let _ = (idx, &acx);
                if set_chart_hover_idx(chart_id, Some(idx), acx.now_ms()) {
                    acx.notify();
                }
            });
            let on_pie_hover = cx.arena().alloc(move |entered: bool, acx: &mut ActionScope| {
                let _ = &acx;
                if !entered {
                    if set_chart_hover_idx(chart_id, None, acx.now_ms()) {
                        acx.notify();
                    }
                }
            });
            let on_pie_drag = cx.arena().alloc(move |ev: DragEvent, acx: &mut ActionScope| {
                let idx = pick_pie_idx(ev.local_x, ev.local_y, ev.width, ev.height);
                if set_chart_locked_idx(chart_id, Some(idx), false, acx.now_ms()) {
                    acx.notify();
                }
                if let Some(cb) = on_select_cb {
                    cb(idx, acx);
                }
            });
            let on_pie_click = cx.arena().alloc(move |acx: &mut ActionScope| {
                let next = active_idx.unwrap_or(0) % item_count;
                if set_chart_locked_idx(chart_id, Some(next), true, acx.now_ms()) {
                    acx.notify();
                }
                if let Some(cb) = on_select_cb {
                    cb(next, acx);
                }
            });
            root = root.child(
                cx,
                div()
                    .id(crate::scoped_child_id(chart_id, 0xC4A7_1001, 1))
                    .pos(0, 0)
                    .w(w)
                    .h(h)
                    .cursor(CursorShape::Pointer)
                    .hover_bg(line_col.scale_alpha(12))
                    .animate_color(Duration::from_millis(120))
                    .no_focus_border()
                    .on_hover(cx, on_pie_hover)
                    .on_pointer_move(cx, on_pie_move)
                    .on_drag(cx, on_pie_drag)
                    .on_click_raw(on_pie_click),
            );
        } else {
            // 柱状图 / 折线图 / 面积图 / K 线图：鼠标悬停实时追踪 + 120fps 列高亮 + 点击锁定 + 拖拽刷选
            let on_hover_move = cx.arena().alloc(move |ev: DragEvent, acx: &mut ActionScope| {
                let rel = ev.local_x.clamp(0, ev.width.max(1) - 1) as usize;
                let idx = ((rel * item_count) / (ev.width.max(1) as usize)).min(item_count - 1);
                let _ = (idx, &acx);
                if set_chart_hover_idx(chart_id, Some(idx), acx.now_ms()) {
                    acx.notify();
                }
            });
            let on_row_hover = cx.arena().alloc(move |entered: bool, acx: &mut ActionScope| {
                let _ = &acx;
                if !entered {
                    if set_chart_hover_idx(chart_id, None, acx.now_ms()) {
                        acx.notify();
                    }
                }
            });
            let on_scrub = cx.arena().alloc(move |ev: DragEvent, acx: &mut ActionScope| {
                let rel = ev.local_x.clamp(0, ev.width.max(1) - 1) as usize;
                let idx = ((rel * item_count) / (ev.width.max(1) as usize)).min(item_count - 1);
                if set_chart_locked_idx(chart_id, Some(idx), false, acx.now_ms()) {
                    acx.notify();
                }
                if let Some(cb) = on_select_cb {
                    cb(idx, acx);
                }
            });

            let mut hover_row = div()
                .id(crate::scoped_child_id(chart_id, 0xC4A7_1002, 1))
                .pos(pad_i16, 0)
                .w(plot_w_i16)
                .h(h)
                .flex_row()
                .on_hover(cx, on_row_hover)
                .on_pointer_move(cx, on_hover_move)
                .on_drag(cx, on_scrub);

            let slot_hover_col = line_col.scale_alpha(26);
            for idx in 0..item_count {
                let x0 = ((idx as i32) * (plot_w_i16 as i32)) / (item_count as i32);
                let x1 = (((idx + 1) as i32) * (plot_w_i16 as i32)) / (item_count as i32);
                let sw = (x1 - x0).max(1) as i16;
                let click_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                    if set_chart_locked_idx(chart_id, Some(idx), true, acx.now_ms()) {
                        acx.notify();
                    }
                    if let Some(cb) = on_select_cb {
                        cb(idx, acx);
                    }
                });
                let mut slot = div()
                    .id(crate::scoped_child_id(chart_id, 0xC4A7_2000, idx as u32 + 1))
                    .w(sw)
                    .h(h)
                    .cursor(CursorShape::Pointer)
                    .hover_bg(slot_hover_col)
                    .animate_color(Duration::from_millis(120))
                    .no_focus_border()
                    .on_click_raw(click_cb);
                if active_idx == Some(idx) {
                    slot = slot.bg(line_col.scale_alpha(18));
                }
                hover_row = hover_row.child(cx, slot);
            }
            root = root.child(cx, hover_row);
        }

        // 顶部弹性浮动 Tooltip 徽章（激活任意数据点/扇区时自动呈现并智能贴近指针列水平位置）
        if let Some(aidx) = active_idx {
            if aidx < item_count {
                let lbl = self.labels.get(aidx).copied().unwrap_or("");
                let tip_text = match kind {
                    ChartKind::Candlestick => {
                        let o = self.data[aidx * 4];
                        let hi = self.data[aidx * 4 + 1];
                        let lo = self.data[aidx * 4 + 2];
                        let cl = self.data[aidx * 4 + 3];
                        if lbl.is_empty() {
                            velox_engine::fmt!(cx, "O:{o} H:{hi} L:{lo} C:{cl}")
                        } else {
                            velox_engine::fmt!(cx, "{lbl} C:{cl} ({lo}-{hi})")
                        }
                    }
                    ChartKind::Pie => {
                        let val = self.data[aidx];
                        let total: i64 = self.data.iter().map(|&v| (v as i64).max(0)).sum();
                        let pct = if total > 0 {
                            ((val as i64).max(0) * 100) / total
                        } else {
                            0
                        };
                        if lbl.is_empty() {
                            velox_engine::fmt!(cx, "#{} · {}%", aidx + 1, pct)
                        } else {
                            velox_engine::fmt!(cx, "{} · {}%", lbl, pct)
                        }
                    }
                    _ => {
                        let val = self.data[aidx];
                        if lbl.is_empty() {
                            velox_engine::fmt!(cx, "#{} · {}{}", aidx + 1, val, self.unit)
                        } else {
                            velox_engine::fmt!(cx, "{} · {}{}", lbl, val, self.unit)
                        }
                    }
                };
                let est_w = ((tip_text.len() as i16) * 6 + 14).min(w.saturating_sub(12)).max(44);
                let tip_x = if kind == ChartKind::Pie {
                    6
                } else {
                    let denom = if kind == ChartKind::Line || kind == ChartKind::Area {
                        (item_count - 1).max(1) as i32
                    } else {
                        item_count.max(1) as i32
                    };
                    let center = pad_i16 as i32
                        + if kind == ChartKind::Line || kind == ChartKind::Area {
                            ((aidx as i32) * (plot_w_i16 as i32)) / denom
                        } else {
                            (((aidx as i32) * 2 + 1) * (plot_w_i16 as i32)) / (denom * 2)
                        };
                    (center as i16 - est_w / 2).clamp(6, (w - est_w - 6).max(6))
                };
                let tip_pill = div()
                    .id(crate::scoped_child_id(chart_id, 0xC4A7_9000, 1))
                    .pos(tip_x, 4)
                    .px(6)
                    .py(2)
                    .rounded(5)
                    .bg(theme.popover.scale_alpha(232))
                    .border(1, line_col.scale_alpha(180))
                    .animate_all(Duration::from_millis(140))
                    .enter_from(0, 0, -6, Duration::from_millis(160))
                    .ease(Easing::EaseOut)
                    .child(cx, text(tip_text).size(10).color(theme.popover_foreground).nowrap());
                root = root.child(cx, tip_pill);
            }
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
