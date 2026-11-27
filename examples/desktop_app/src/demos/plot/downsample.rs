//! 时序保形降采样算法模块（LTTB 等），独立成文件便于后续扩展 Min-Max / M4 / 均线指标计算。

/// Sveinn Steinarsson **Largest-Triangle-Three-Buckets (LTTB)** 保形降采样算法：
/// 将海量时序点压缩到 `threshold` 个代表点，严格保留首尾点与每个桶内面积最大的视觉极值点。
pub fn lttb_downsample(data: &[(f64, f64)], threshold: usize, out: &mut Vec<(f64, f64)>) {
    out.clear();
    let n = data.len();
    if n == 0 || threshold == 0 {
        return;
    }
    if threshold >= n || threshold <= 2 {
        if threshold == 1 {
            out.push(data[0]);
        } else if threshold == 2 && n >= 2 {
            out.push(data[0]);
            out.push(data[n - 1]);
        } else {
            out.extend_from_slice(data);
        }
        return;
    }

    out.reserve(threshold);
    let every = (n - 2) as f64 / (threshold - 2) as f64;
    let mut a = 0usize;
    out.push(data[a]);

    for i in 0..(threshold - 2) {
        // 下一个桶的均值点作为三角形第三顶点
        let avg_start = (((i + 1) as f64 * every).floor() as usize + 1).min(n - 1);
        let avg_end = (((i + 2) as f64 * every).floor() as usize + 1).min(n);
        let avg_len = (avg_end.saturating_sub(avg_start)).max(1) as f64;
        let mut avg_x = 0.0f64;
        let mut avg_y = 0.0f64;
        for &(px, py) in &data[avg_start..avg_end.max(avg_start + 1).min(n)] {
            avg_x += px;
            avg_y += py;
        }
        avg_x /= avg_len;
        avg_y /= avg_len;

        // 当前桶内寻找与 point_a、avg 构成最大三角形面积的点
        let range_offs = ((i as f64 * every).floor() as usize + 1).min(n - 1);
        let range_to = (((i + 1) as f64 * every).floor() as usize + 1).min(n - 1);
        let (ax, ay) = data[a];
        let mut max_area = -1.0f64;
        let mut next_a = range_offs;
        for idx in range_offs..range_to.max(range_offs + 1) {
            let (px, py) = data[idx];
            let area = ((ax - avg_x) * (py - ay) - (ax - px) * (avg_y - ay)).abs() * 0.5;
            if area > max_area {
                max_area = area;
                next_a = idx;
            }
        }
        out.push(data[next_a]);
        a = next_a;
    }

    out.push(data[n - 1]);
}
