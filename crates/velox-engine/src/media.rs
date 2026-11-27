//! 零拷贝视频帧表面（`VideoSurface`）与原生 WebView 宿主裁剪同步模型（`WebViewHostSync`）。
//!
//! 全模块核心保持 100% `#![no_std]` 零堆兼容：
//! - **GPU 零拷贝视频通道**：直接复用 [`crate::paint::ExternalTextureId`]，外部硬件解码器（MediaFoundation /
//!   VideoToolbox / VA-API / MediaCodec）产出的 GPU 纹理零拷贝直出合成；
//! - **CPU YUV 回退转换**：内置定点 BT.709 `convert_nv12_to_rgba8` 与 `convert_i420_to_rgba8`，在无硬件直出纹理时
//!   极速转码为 RGBA8 供双后端统一显示；
//! - **原生 WebView 宿主同步**：[`WebViewHostSync`] 每帧基于布局结果做几何 Diff，仅当位置/尺寸/裁剪/可见性变化时
//!   产出 [`WebViewHostUpdate`]，供 Windows WebView2 / macOS WKWebView / Linux WebKitGTK 零闪烁洞穿跟随。

use crate::image_pipeline::ObjectFit;
use crate::paint::ExternalTextureId;
use crate::solver::Region;

/// 视频帧像素格式。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum VideoPixelFormat {
    /// 半平面 YUV 4:2:0（Y 平面 + 交错 UV 平面，硬件解码器最常用格式）。
    Nv12,
    /// 三平面 YUV 4:2:0（Y + U + V 独立平面）。
    I420,
    /// 交错 RGBA8888。
    Rgba8,
    /// 交错 BGRA8888。
    Bgra8,
}

/// 零拷贝视频帧表面规格（同时兼容 GPU 外部纹理句柄与软件转码尺寸描述）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct VideoSurfaceSpec {
    pub texture_id: Option<ExternalTextureId>,
    pub video_width: u32,
    pub video_height: u32,
    pub format: VideoPixelFormat,
    pub fit: ObjectFit,
    pub rounded: u8,
    pub opacity: u8,
}

impl VideoSurfaceSpec {
    #[inline(always)]
    pub const fn new(video_width: u32, video_height: u32, format: VideoPixelFormat) -> Self {
        Self {
            texture_id: None,
            video_width,
            video_height,
            format,
            fit: ObjectFit::Contain,
            rounded: 8,
            opacity: 255,
        }
    }

    #[inline(always)]
    pub const fn external_texture(mut self, id: ExternalTextureId) -> Self {
        self.texture_id = Some(id);
        self
    }

    #[inline(always)]
    pub const fn fit(mut self, fit: ObjectFit) -> Self {
        self.fit = fit;
        self
    }

    #[inline(always)]
    pub const fn rounded(mut self, r: u8) -> Self {
        self.rounded = r;
        self
    }
}

#[inline(always)]
fn yuv_to_rgb_bt709(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let c = (y as i32 - 16).max(0);
    let d = u as i32 - 128;
    let e = v as i32 - 128;
    let r = (298 * c + 459 * e + 128) >> 8;
    let g = (298 * c - 55 * d - 136 * e + 128) >> 8;
    let b = (298 * c + 541 * d + 128) >> 8;
    (
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
    )
}

/// BT.709 定点快速转换 NV12（Y 平面 + 交错 UV 平面）至 RGBA8888。
/// 返回成功写入的像素数（`width * height`）；输入/输出缓冲区尺寸不足时返回 `0`。
pub fn convert_nv12_to_rgba8(
    y_plane: &[u8],
    uv_plane: &[u8],
    width: usize,
    height: usize,
    out_rgba: &mut [u8],
) -> usize {
    let pixels = width.saturating_mul(height);
    let uv_w = (width + 1) / 2;
    let uv_h = (height + 1) / 2;
    if pixels == 0
        || y_plane.len() < pixels
        || uv_plane.len() < uv_w.saturating_mul(uv_h).saturating_mul(2)
        || out_rgba.len() < pixels.saturating_mul(4)
    {
        return 0;
    }
    for row in 0..height {
        let y_row = row * width;
        let uv_row = (row >> 1) * uv_w * 2;
        let out_row = row * width * 4;
        for col in 0..width {
            let y = y_plane[y_row + col];
            let uv_idx = uv_row + (col >> 1) * 2;
            let u = uv_plane[uv_idx];
            let v = uv_plane[uv_idx + 1];
            let (r, g, b) = yuv_to_rgb_bt709(y, u, v);
            let o = out_row + col * 4;
            out_rgba[o] = r;
            out_rgba[o + 1] = g;
            out_rgba[o + 2] = b;
            out_rgba[o + 3] = 255;
        }
    }
    pixels
}

/// BT.709 定点快速转换 I420（Y + U + V 三平面）至 RGBA8888。
pub fn convert_i420_to_rgba8(
    y_plane: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    width: usize,
    height: usize,
    out_rgba: &mut [u8],
) -> usize {
    let pixels = width.saturating_mul(height);
    let cw = (width + 1) / 2;
    let ch = (height + 1) / 2;
    let c_len = cw.saturating_mul(ch);
    if pixels == 0
        || y_plane.len() < pixels
        || u_plane.len() < c_len
        || v_plane.len() < c_len
        || out_rgba.len() < pixels.saturating_mul(4)
    {
        return 0;
    }
    for row in 0..height {
        let y_row = row * width;
        let c_row = (row >> 1) * cw;
        let out_row = row * width * 4;
        for col in 0..width {
            let y = y_plane[y_row + col];
            let c_idx = c_row + (col >> 1);
            let (r, g, b) = yuv_to_rgb_bt709(y, u_plane[c_idx], v_plane[c_idx]);
            let o = out_row + col * 4;
            out_rgba[o] = r;
            out_rgba[o + 1] = g;
            out_rgba[o + 2] = b;
            out_rgba[o + 3] = 255;
        }
    }
    pixels
}

/// 原生子窗口 WebView 宿主区域描述（供 WebView2 / WKWebView / WebKitGTK 洞穿定位）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct WebViewHostRegion {
    pub id: u32,
    pub logical_bounds: Region,
    pub clip_bounds: Region,
    pub visible: bool,
    pub z_order: i16,
}

impl WebViewHostRegion {
    /// 计算裁剪后的有效逻辑可见框。
    #[inline]
    pub fn effective_bounds(&self) -> Region {
        if !self.visible {
            return Region::EMPTY;
        }
        self.logical_bounds.intersect(self.clip_bounds)
    }

    /// 换算为物理像素矩形 `(x, y, w, h)`（适配分数 HiDPI 缩放）。
    #[inline]
    pub fn physical_rect(&self, scale_factor: f32) -> (i32, i32, u32, u32) {
        let eff = self.effective_bounds();
        if eff.is_empty() {
            return (0, 0, 0, 0);
        }
        let sf = scale_factor.clamp(0.5, 4.0);
        let x = ((eff.x as f32) * sf) as i32;
        let y = ((eff.y as f32) * sf) as i32;
        let w = ((eff.w.max(0) as f32) * sf) as u32;
        let h = ((eff.h.max(0) as f32) * sf) as u32;
        (x, y, w, h)
    }
}

/// 单帧 WebView 宿主几何变更通知（零冗余 IPC 同步）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WebViewHostUpdate {
    pub id: u32,
    pub physical_rect: (i32, i32, u32, u32),
    pub visible: bool,
    pub z_order: i16,
}

/// 跨帧 WebView 宿主裁剪同步器（最多追踪 8 个嵌入式 WebView，静息帧 0 同步开销）。
#[derive(Clone, Debug, Default)]
pub struct WebViewHostSync {
    last: [Option<WebViewHostUpdate>; 8],
}

impl WebViewHostSync {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { last: [None; 8] }
    }

    /// 对比本帧布局算出的 [`WebViewHostRegion`]；仅当物理坐标、可见性或 Z 序发生变化时返回 `Some(update)`。
    pub fn diff_region(
        &mut self,
        region: WebViewHostRegion,
        scale_factor: f32,
    ) -> Option<WebViewHostUpdate> {
        let phys = region.physical_rect(scale_factor);
        let vis = region.visible && phys.2 > 0 && phys.3 > 0;
        let next = WebViewHostUpdate {
            id: region.id,
            physical_rect: phys,
            visible: vis,
            z_order: region.z_order,
        };
        for slot in &mut self.last {
            if let Some(prev) = slot {
                if prev.id == region.id {
                    if *prev == next {
                        return None;
                    }
                    *prev = next;
                    return Some(next);
                }
            }
        }
        for slot in &mut self.last {
            if slot.is_none() {
                *slot = Some(next);
                return Some(next);
            }
        }
        Some(next)
    }
}
