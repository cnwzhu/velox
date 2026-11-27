//! 工业遥测与金融高频实时波形 / OHLC K 线绘图模块（`velox_base::plot`）。
//!
//! 模块职责划分：
//! - [`series`]：固定容量零重分配环形缓冲 [`RingSeries`] 与金融 [`OhlcCandle`]；
//! - [`downsample`]：Sveinn Steinarsson 正宗 LTTB 保形降采样算法 [`lttb_downsample`]（便于后续扩展 M4 / Min-Max / 均线指标）；
//! - [`state`]：时间窗缩放/平移/推流/尖峰注入与十字准星精确读数状态机 [`RealtimePlotState`]；
//! - [`widget`]：声明式高频波形与 K 线组件 [`RealtimePlot`]。

pub mod downsample;
pub mod series;
pub mod state;
pub mod widget;

pub use downsample::lttb_downsample;
pub use series::{OhlcCandle, RingSeries};
pub use state::RealtimePlotState;
pub use widget::RealtimePlot;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_realtime_plot_100k_ring_lttb_pan_stream_and_readout() {
        let mut plot = RealtimePlotState::demo_100k_telemetry();
        assert_eq!(plot.total_points(), 200_000);
        let mut raw = Vec::new();
        plot.series[0].copy_ordered_into(&mut raw);
        assert_eq!(raw.len(), 100_000);
        let mut down = Vec::new();
        lttb_downsample(&raw, 400, &mut down);
        assert_eq!(down.len(), 400);
        let max_y = down.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        assert!(
            max_y > 85.0,
            "LTTB should preserve high-frequency spikes, got max_y={max_y}"
        );

        plot.zoom_x(0.5, -20);
        let before = plot.x_window.0;
        plot.pan_x(-80.0, 760.0);
        assert!(plot.x_window.0 > before);
        plot.push_live_batch(1);
        plot.crosshair_local = Some((300, 120));
        let readout = plot.crosshair_readout(780, false).unwrap();
        assert!(readout.contains("Latency:") && readout.contains("GPU:"));
    }
}
