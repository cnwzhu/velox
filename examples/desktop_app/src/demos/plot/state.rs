//! 高频波形 / K 线视窗与交互状态机（缩放、平移、实时推流、尖峰注入与十字准星精确读数）。

use velox_kit::style::Color;

use super::series::{OhlcCandle, RingSeries};

/// 高频波形/K 线画布状态（支持 X 轴时间窗缩放平移与十字准星悬停）。
pub struct RealtimePlotState {
    pub series: Vec<RingSeries>,
    pub candles: Vec<OhlcCandle>,
    /// X 轴可见归一化视窗 `[start_frac, end_frac] ⊆ [0.0, 1.0]`。
    pub x_window: (f64, f64),
    pub crosshair_local: Option<(i16, i16)>,
}

impl Default for RealtimePlotState {
    fn default() -> Self {
        Self::demo_100k_telemetry()
    }
}

impl RealtimePlotState {
    /// 生成 100,000 点高频双通道遥测波形 + 64 根金融 K 线用于演示与性能基准。
    pub fn demo_100k_telemetry() -> Self {
        let mut ch1 = RingSeries::new("Frame Latency (us)", Color::rgb(59, 130, 246), 100_000);
        let mut ch2 = RingSeries::new("GPU Utilization (%)", Color::rgb(16, 185, 129), 100_000);
        for i in 0..100_000usize {
            let t = i as f64 * 0.001;
            let spike = if i % 4096 == 1024 { 24.0 } else { 0.0 };
            let v1 = 50.0 + (t * 2.4).sin() * 18.0 + (t * 19.0).cos() * 4.5 + spike;
            let v2 = 42.0 + (t * 1.1).cos() * 14.0 + (t * 7.3).sin() * 6.0;
            ch1.push(t, v1);
            ch2.push(t, v2);
        }
        let mut candles = Vec::with_capacity(64);
        let mut price = 142.0f64;
        for i in 0..64 {
            let delta = ((i as f64 * 0.45).sin() * 3.2) + ((i as f64 * 1.7).cos() * 1.4);
            let open = price;
            let close = (open + delta).max(20.0);
            let high = open.max(close) + 1.8 + (i % 3) as f64 * 0.6;
            let low = open.min(close) - 1.6 - (i % 4) as f64 * 0.4;
            candles.push(OhlcCandle {
                t: i as f64,
                open,
                high,
                low,
                close,
            });
            price = close;
        }
        Self {
            series: vec![ch1, ch2],
            candles,
            x_window: (0.0, 1.0),
            crosshair_local: None,
        }
    }

    /// 滚轮围绕光标比例缩放 X 轴时间窗。
    pub fn zoom_x(&mut self, anchor_frac: f64, delta_y: i16) {
        let af = anchor_frac.clamp(0.0, 1.0);
        let (s, e) = self.x_window;
        let span = (e - s).max(0.002);
        let factor = if delta_y < 0 { 0.82 } else { 1.22 };
        let new_span = (span * factor).clamp(0.002, 1.0);
        let center = s + span * af;
        let mut ns = center - new_span * af;
        let mut ne = ns + new_span;
        if ns < 0.0 {
            ns = 0.0;
            ne = new_span;
        }
        if ne > 1.0 {
            ne = 1.0;
            ns = (1.0 - new_span).max(0.0);
        }
        self.x_window = (ns, ne);
    }

    /// 鼠标水平拖拽平移 X 轴可见时间窗（放大细节后自由浏览 100,000 点数据）。
    pub fn pan_x(&mut self, delta_px: f32, plot_width_px: f32) {
        let (s, e) = self.x_window;
        let span = (e - s).max(0.002);
        if span >= 0.999 {
            return;
        }
        let d_frac = -(delta_px as f64 / plot_width_px.max(16.0) as f64) * span;
        let mut ns = (s + d_frac).clamp(0.0, 1.0 - span);
        let ne = (ns + span).min(1.0);
        ns = (ne - span).max(0.0);
        self.x_window = (ns, ne);
    }

    /// 恢复全景 0%..100% 缩放。
    pub fn reset_zoom(&mut self) {
        self.x_window = (0.0, 1.0);
    }

    /// 实时推流：每次推进 64 个高频采样点到 100,000 环形缓冲（自动滑动覆写最旧数据）。
    pub fn push_live_batch(&mut self, seed: usize) {
        let base_t = 100.0 + seed as f64 * 0.064;
        for i in 0..64usize {
            let t = base_t + i as f64 * 0.001;
            let spike = if (seed + i) % 19 == 0 { 26.0 } else { 0.0 };
            let v1 = 50.0 + (t * 2.7).sin() * 19.0 + (t * 21.0).cos() * 5.0 + spike;
            let v2 = 44.0 + (t * 1.3).cos() * 14.0 + (t * 8.1).sin() * 5.5;
            if let Some(ch1) = self.series.get_mut(0) {
                ch1.push(t, v1);
            }
            if let Some(ch2) = self.series.get_mut(1) {
                ch2.push(t, v2);
            }
        }
    }

    /// 一键注入一批高频脉冲尖峰点，便于观察 LTTB 算法保留尖峰特征的效果。
    pub fn inject_spike_burst(&mut self) {
        let n = self.total_points();
        let base_t = 120.0 + (n as f64) * 0.0001;
        for i in 0..512usize {
            let t = base_t + i as f64 * 0.001;
            let spike = if i % 32 == 16 { 38.0 } else { 0.0 };
            let v1 = 52.0 + (t * 3.5).sin() * 15.0 + spike;
            let v2 = 46.0 + (t * 1.8).cos() * 12.0;
            if let Some(ch1) = self.series.get_mut(0) {
                ch1.push(t, v1);
            }
            if let Some(ch2) = self.series.get_mut(1) {
                ch2.push(t, v2);
            }
        }
    }

    /// 根据十字准星局部 X 坐标计算精确读数摘要（波形模式显示通道数值，K 线模式显示 OHLC）。
    pub fn crosshair_readout(&self, plot_w: i16, show_candles: bool) -> Option<String> {
        let (mx, _) = self.crosshair_local?;
        let pad_l = 12.0f32;
        let pw = (plot_w as f32 - 24.0).max(16.0);
        let frac_in_view = ((mx as f32 - pad_l) / pw).clamp(0.0, 1.0) as f64;
        let global_frac = self.x_window.0 + (self.x_window.1 - self.x_window.0) * frac_in_view;

        if show_candles && !self.candles.is_empty() {
            let idx = ((global_frac * self.candles.len() as f64).floor() as usize)
                .min(self.candles.len() - 1);
            let c = self.candles[idx];
            return Some(format!(
                "Bar #{} | O:{:.1} H:{:.1} L:{:.1} C:{:.1}",
                idx + 1,
                c.open,
                c.high,
                c.low,
                c.close
            ));
        }

        let ch1 = self.series.first()?;
        if ch1.is_empty() {
            return None;
        }
        let mut buf1 = Vec::new();
        ch1.copy_ordered_into(&mut buf1);
        let idx = ((global_frac * buf1.len() as f64).floor() as usize).min(buf1.len() - 1);
        let (t, v1) = buf1[idx];
        let v2 = self
            .series
            .get(1)
            .and_then(|s| {
                let mut b = Vec::new();
                s.copy_ordered_into(&mut b);
                b.get(idx).map(|p| p.1)
            })
            .unwrap_or(0.0);
        Some(format!(
            "Sample #{idx} (t={t:.2}s) | Latency: {v1:.1}us | GPU: {v2:.1}%"
        ))
    }

    pub fn total_points(&self) -> usize {
        self.series.iter().map(|s| s.len()).sum()
    }
}
