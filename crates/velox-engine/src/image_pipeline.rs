//! 通用位图资产管线：纯 Rust PNG/BMP 解码器、显存/内存预算 LRU 缓存、异步后台加载队列、`ObjectFit` 与九宫格 `NineSlice`。
//!
//! - 核心数据结构（[`ImageAssetId`]、[`ObjectFit`]、[`NineSliceInsets`]、[`DecodedImageSpec`]、[`compute_object_fit`]）
//!   100% 无堆兼容，可在所有硬件与桌面目标上直接构建与排版；
//! - `std` 环境下提供纯 Rust 零外部依赖 PNG（完整 RFC 1951 Inflate + 5 种滤波器）/ BMP 解码、
//!   按字节预算自动淘汰的 [`LruImageCache`] 与后台线程池 [`AsyncImageLoader`]。

use crate::element::{Element, IntoElement};
use crate::paint::CornerRadii;
use crate::pod::FrameScope;
use crate::solver::Region;
use crate::style::Color;

/// 图像资产稳定句柄 ID（用于在 `LruImageCache` 与 GPU 纹理缓存中 $O(1)$ 查找）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct ImageAssetId(pub u32);

/// 图像内容在目标盒子中的缩放适配模式（对应 CSS `object-fit`）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum ObjectFit {
    /// 拉伸填满整个目标区域（不保持纵横比）。
    Fill = 0,
    /// 等比缩放使整张图完整显示在盒子内（可能留边）。
    #[default]
    Contain = 1,
    /// 等比缩放填满整个盒子并裁切溢出边缘（最常用于头像/卡片封面）。
    Cover = 2,
    /// 仅当原图大于目标盒子时等比缩小，否则按原尺寸居中显示。
    ScaleDown = 3,
    /// 按原尺寸居中显示，不做缩放。
    None = 4,
}

/// 九宫格（9-Slice）边框保持内边距（单位：源图像素，四角不拉伸、四边单轴拉伸、中心双轴拉伸）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct NineSliceInsets {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl NineSliceInsets {
    pub const ZERO: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    #[inline(always)]
    pub const fn all(v: u16) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    #[inline(always)]
    pub const fn new(left: u16, top: u16, right: u16, bottom: u16) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[inline(always)]
    pub const fn is_active(self) -> bool {
        self.left > 0 || self.top > 0 || self.right > 0 || self.bottom > 0
    }
}

/// 已解码图像绘制规格（分配于 `ScratchArena`，通过 8B 引用挂入 `DrawPayload::DecodedImage`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct DecodedImageSpec {
    pub id: ImageAssetId,
    pub width: i16,
    pub height: i16,
    pub fit: ObjectFit,
    pub nine_slice: NineSliceInsets,
    pub radii: CornerRadii,
    pub opacity: u8,
    pub tint: Color,
    pub fallback_bg: Color,
    pub content_revision: u64,
}

impl DecodedImageSpec {
    #[inline(always)]
    pub const fn new(id: ImageAssetId, width: i16, height: i16) -> Self {
        Self {
            id,
            width,
            height,
            fit: ObjectFit::Contain,
            nine_slice: NineSliceInsets::ZERO,
            radii: CornerRadii::ZERO,
            opacity: 255,
            tint: Color::WHITE,
            fallback_bg: Color::rgba(36, 42, 54, 255),
            content_revision: 0,
        }
    }

    #[inline]
    pub fn mix_hash(&self, h: &mut u32) {
        let mut mix = |b: u8| {
            *h ^= b as u32;
            *h = h.wrapping_mul(0x0100_0193);
        };
        for b in self.id.0.to_le_bytes() {
            mix(b);
        }
        mix(self.fit as u8);
        for b in self.nine_slice.left.to_le_bytes() {
            mix(b);
        }
        for b in self.nine_slice.top.to_le_bytes() {
            mix(b);
        }
        for b in self.nine_slice.right.to_le_bytes() {
            mix(b);
        }
        for b in self.nine_slice.bottom.to_le_bytes() {
            mix(b);
        }
        mix(self.radii.tl);
        mix(self.radii.tr);
        mix(self.radii.br);
        mix(self.radii.bl);
        mix(self.opacity);
        for b in self.tint.0.to_le_bytes() {
            mix(b);
        }
        for b in self.fallback_bg.0.to_le_bytes() {
            mix(b);
        }
        for b in self.content_revision.to_le_bytes() {
            mix(b);
        }
    }
}

/// 声明式解码图像构建器。
#[derive(Copy, Clone, Debug)]
pub struct DecodedImage {
    pub spec: DecodedImageSpec,
}

/// 构造一个支持 `ObjectFit`、`NineSlice` 与异步 LRU 纹理缓存的通用图像元素。
#[inline(always)]
pub const fn decoded_image(id: ImageAssetId, width: i16, height: i16) -> DecodedImage {
    DecodedImage {
        spec: DecodedImageSpec::new(id, width, height),
    }
}

impl DecodedImage {
    #[inline(always)]
    pub const fn fit(mut self, fit: ObjectFit) -> Self {
        self.spec.fit = fit;
        self
    }

    #[inline(always)]
    pub const fn nine_slice(mut self, insets: NineSliceInsets) -> Self {
        self.spec.nine_slice = insets;
        self
    }

    #[inline(always)]
    pub const fn rounded(mut self, r: u8) -> Self {
        self.spec.radii = CornerRadii::all(r);
        self
    }

    #[inline(always)]
    pub const fn rounded_corners(mut self, radii: CornerRadii) -> Self {
        self.spec.radii = radii;
        self
    }

    #[inline(always)]
    pub const fn opacity(mut self, opacity: u8) -> Self {
        self.spec.opacity = opacity;
        self
    }

    #[inline(always)]
    pub const fn tint(mut self, tint: Color) -> Self {
        self.spec.tint = tint;
        self
    }

    #[inline(always)]
    pub const fn fallback_bg(mut self, color: Color) -> Self {
        self.spec.fallback_bg = color;
        self
    }

    #[inline(always)]
    pub const fn revision(mut self, rev: u64) -> Self {
        self.spec.content_revision = rev;
        self
    }
}

impl<'a> IntoElement<'a> for DecodedImage {
    #[inline]
    fn into_element(self, cx: &'a FrameScope) -> Element<'a> {
        let spec_ref = cx.arena().alloc_temp(self.spec);
        Element::DecodedImage(spec_ref)
    }
}

/// 根据源图像尺寸 `(src_w, src_h)`、目标绘制区域 `dst` 与 [`ObjectFit`] 计算实际绘制子矩形与采样 UV `[u0, v0, u1, v1]`。
pub fn compute_object_fit(
    src_w: u32,
    src_h: u32,
    dst: Region,
    fit: ObjectFit,
) -> (Region, [f32; 4]) {
    if src_w == 0 || src_h == 0 || dst.w <= 0 || dst.h <= 0 {
        return (dst, [0.0, 0.0, 1.0, 1.0]);
    }
    let sw = src_w as f32;
    let sh = src_h as f32;
    let dw = dst.w as f32;
    let dh = dst.h as f32;

    match fit {
        ObjectFit::Fill => (dst, [0.0, 0.0, 1.0, 1.0]),
        ObjectFit::Contain => {
            let scale = (dw / sw).min(dh / sh);
            let rw = (sw * scale).max(1.0);
            let rh = (sh * scale).max(1.0);
            let rx = dst.x as f32 + (dw - rw) * 0.5;
            let ry = dst.y as f32 + (dh - rh) * 0.5;
            (
                Region::new(rx as i16, ry as i16, rw as i16, rh as i16),
                [0.0, 0.0, 1.0, 1.0],
            )
        }
        ObjectFit::Cover => {
            let scale = (dw / sw).max(dh / sh);
            let vis_w = (dw / scale) / sw;
            let vis_h = (dh / scale) / sh;
            let u0 = ((1.0 - vis_w) * 0.5).clamp(0.0, 0.5);
            let v0 = ((1.0 - vis_h) * 0.5).clamp(0.0, 0.5);
            (dst, [u0, v0, 1.0 - u0, 1.0 - v0])
        }
        ObjectFit::ScaleDown => {
            if sw <= dw && sh <= dh {
                compute_object_fit(src_w, src_h, dst, ObjectFit::None)
            } else {
                compute_object_fit(src_w, src_h, dst, ObjectFit::Contain)
            }
        }
        ObjectFit::None => {
            let rw = sw.min(dw);
            let rh = sh.min(dh);
            let rx = dst.x as f32 + (dw - rw) * 0.5;
            let ry = dst.y as f32 + (dh - rh) * 0.5;
            let u_span = (rw / sw).min(1.0);
            let v_span = (rh / sh).min(1.0);
            let u0 = (1.0 - u_span) * 0.5;
            let v0 = (1.0 - v_span) * 0.5;
            (
                Region::new(rx as i16, ry as i16, rw as i16, rh as i16),
                [u0, v0, u0 + u_span, v0 + v_span],
            )
        }
    }
}

/// 九宫格坐标映射：将目标局部像素位置 `pos`（区间 `[0, dst_len)`）映射到源图浮点采样坐标（区间 `[0, src_len)`）。
#[inline]
pub fn map_nine_slice_coord(pos: f32, dst_len: f32, src_len: f32, start_inset: u16, end_inset: u16) -> f32 {
    if dst_len <= 0.0 || src_len <= 0.0 {
        return 0.0;
    }
    let si = (start_inset as f32).min(src_len * 0.5).min(dst_len * 0.5);
    let ei = (end_inset as f32).min(src_len * 0.5).min(dst_len * 0.5);
    if pos < si {
        pos
    } else if pos >= dst_len - ei {
        src_len - (dst_len - pos)
    } else {
        let dst_mid = (dst_len - si - ei).max(1.0);
        let src_mid = (src_len - si - ei).max(0.0);
        si + ((pos - si) / dst_mid) * src_mid
    }
}

// ============================================================================
// `std` 通用图像解码（PNG / BMP / 魔数探测）、预算感知 LRU 缓存与异步后台解码管线
// ============================================================================

mod std_pipeline {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
    use std::vec;
    use std::vec::Vec;

    /// 支持探测的图像容器格式。
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    pub enum ImageFormat {
        Png,
        Jpeg,
        WebP,
        Bmp,
        Unknown,
    }

    /// 识别字节流的图像文件格式魔数。
    pub fn detect_image_format(bytes: &[u8]) -> ImageFormat {
        if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
            ImageFormat::Png
        } else if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            ImageFormat::Jpeg
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            ImageFormat::WebP
        } else if bytes.len() >= 2 && &bytes[..2] == b"BM" {
            ImageFormat::Bmp
        } else {
            ImageFormat::Unknown
        }
    }

    /// 已解码的直通 RGBA8888 图像像素缓冲（线程安全共享）。
    #[derive(Clone, Debug)]
    pub struct DecodedPixels {
        pub width: u32,
        pub height: u32,
        pub rgba: Arc<[u8]>,
        pub revision: u64,
    }

    impl DecodedPixels {
        #[inline]
        pub fn byte_size(&self) -> usize {
            self.rgba.len()
        }
    }

    /// 图像资产的轻量元数据（不持有像素 `Arc` 强引用，像素被 CPU LRU 淘汰后仍可查询尺寸与版本号）。
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct ImageMetadata {
        pub width: u32,
        pub height: u32,
        pub revision: u64,
    }

    impl ImageMetadata {
        /// 展开为 RGBA8888 后的字节大小。
        #[inline]
        pub const fn byte_size(&self) -> usize {
            (self.width as usize)
                .saturating_mul(self.height as usize)
                .saturating_mul(4)
        }
    }

    /// 异步图像加载状态（`Ready` 仅持有轻量 [`ImageMetadata`]，绝不钉死像素缓存内存）。
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum ImageLoadState {
        Loading,
        Ready(ImageMetadata),
        Failed,
    }

    /// 按显存/内存字节预算严格淘汰最久未用条目的 LRU 图像缓存。
    pub struct LruImageCache {
        max_bytes: usize,
        cur_bytes: usize,
        clock: u64,
        entries: HashMap<ImageAssetId, (DecodedPixels, u64)>,
    }

    impl LruImageCache {
        pub fn new(max_bytes: usize) -> Self {
            Self {
                max_bytes,
                cur_bytes: 0,
                clock: 0,
                entries: HashMap::new(),
            }
        }

        #[inline]
        pub fn current_bytes(&self) -> usize {
            self.cur_bytes
        }

        #[inline]
        pub fn max_bytes(&self) -> usize {
            self.max_bytes
        }

        #[inline]
        pub fn set_max_bytes(&mut self, max_bytes: usize) {
            self.max_bytes = max_bytes;
            self.evict_if_needed();
        }

        #[inline]
        pub fn len(&self) -> usize {
            self.entries.len()
        }

        #[inline]
        pub fn is_empty(&self) -> bool {
            self.entries.is_empty()
        }

        /// 将解码像素写入 LRU 缓存；若单张图像字节数已超过总预算 `max_bytes`，则不留驻常驻缓存以严格守住预算上限。
        pub fn insert(&mut self, id: ImageAssetId, pixels: DecodedPixels) {
            let _ = self.insert_checked(id, pixels);
        }

        /// 将解码像素写入 LRU 缓存，返回是否成功留驻在预算范围内（单图超出 `max_bytes` 时返回 `false`）。
        pub fn insert_checked(&mut self, id: ImageAssetId, pixels: DecodedPixels) -> bool {
            if let Some((old, _)) = self.entries.remove(&id) {
                self.cur_bytes = self.cur_bytes.saturating_sub(old.byte_size());
            }
            let sz = pixels.byte_size();
            if sz > self.max_bytes {
                // 单图即超总预算：不留驻缓存，严格确保 `cur_bytes <= max_bytes`。
                return false;
            }
            self.clock = self.clock.saturating_add(1);
            self.cur_bytes = self.cur_bytes.saturating_add(sz);
            self.entries.insert(id, (pixels, self.clock));
            self.evict_if_needed();
            true
        }

        pub fn get(&mut self, id: ImageAssetId) -> Option<DecodedPixels> {
            self.clock = self.clock.saturating_add(1);
            let clk = self.clock;
            if let Some((px, stamp)) = self.entries.get_mut(&id) {
                *stamp = clk;
                return Some(px.clone());
            }
            None
        }

        pub fn remove(&mut self, id: ImageAssetId) -> bool {
            if let Some((old, _)) = self.entries.remove(&id) {
                self.cur_bytes = self.cur_bytes.saturating_sub(old.byte_size());
                true
            } else {
                false
            }
        }

        pub fn clear(&mut self) {
            self.entries.clear();
            self.cur_bytes = 0;
        }

        fn evict_if_needed(&mut self) {
            while self.cur_bytes > self.max_bytes {
                let Some((&victim_id, _)) = self.entries.iter().min_by_key(|(_, (_, ts))| *ts) else {
                    break;
                };
                if let Some((old, _)) = self.entries.remove(&victim_id) {
                    self.cur_bytes = self.cur_bytes.saturating_sub(old.byte_size());
                }
            }
        }
    }

    type CustomDecoderFn = fn(&[u8]) -> Option<(u32, u32, Vec<u8>)>;
    type ReadyCallback = Arc<dyn Fn(ImageAssetId, bool) + Send + Sync + 'static>;

    const DEFAULT_IMAGE_CACHE_BUDGET: usize = 64 * 1024 * 1024;
    const MAX_DECODE_QUEUE_LEN: usize = 256;
    const DEFAULT_MAX_IN_FLIGHT_DECODE_BYTES: usize = 32 * 1024 * 1024;

    static CUSTOM_DECODERS: OnceLock<RwLock<Vec<CustomDecoderFn>>> = OnceLock::new();
    static GLOBAL_IMAGE_CACHE: OnceLock<Mutex<LruImageCache>> = OnceLock::new();
    static GLOBAL_IMAGE_STATES: OnceLock<Mutex<HashMap<ImageAssetId, ImageLoadState>>> = OnceLock::new();
    static NEXT_IMAGE_REV: AtomicU64 = AtomicU64::new(1);

    struct DecodeJob {
        id: ImageAssetId,
        bytes: Vec<u8>,
        estimated_rgba_bytes: usize,
        on_ready: Option<ReadyCallback>,
    }

    struct DecodePoolState {
        queue: VecDeque<DecodeJob>,
        in_flight_bytes: usize,
        max_in_flight_bytes: usize,
        worker_count: usize,
    }

    struct DecodePool {
        state: Mutex<DecodePoolState>,
        cv: Condvar,
    }

    static DECODE_POOL: OnceLock<Arc<DecodePool>> = OnceLock::new();

    fn estimate_decoded_rgba_bytes(bytes: &[u8]) -> usize {
        // 尽量从 PNG IHDR / BMP BITMAPINFOHEADER 读取真实展开尺寸，精准限制并发展开像素内存。
        if bytes.len() >= 24 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" && &bytes[12..16] == b"IHDR" {
            let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
            let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize;
            if w > 0 && h > 0 {
                return w.saturating_mul(h).saturating_mul(4).max(4096);
            }
        } else if bytes.len() >= 26 && &bytes[..2] == b"BM" {
            let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
            let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).unsigned_abs();
            if w > 0 && h > 0 {
                return (w as usize).saturating_mul(h as usize).saturating_mul(4).max(4096);
            }
        }
        bytes.len().saturating_mul(4).max(4096)
    }

    fn decode_pool() -> &'static Arc<DecodePool> {
        DECODE_POOL.get_or_init(|| {
            let workers = std::thread::available_parallelism()
                .map(|n| n.get().clamp(2, 4))
                .unwrap_or(2);
            let pool = Arc::new(DecodePool {
                state: Mutex::new(DecodePoolState {
                    queue: VecDeque::with_capacity(32),
                    in_flight_bytes: 0,
                    max_in_flight_bytes: DEFAULT_MAX_IN_FLIGHT_DECODE_BYTES,
                    worker_count: workers,
                }),
                cv: Condvar::new(),
            });
            for idx in 0..workers {
                let p = Arc::clone(&pool);
                let _ = std::thread::Builder::new()
                    .name(std::format!("velox-img-decode-{idx}"))
                    .spawn(move || decode_worker_loop(p));
            }
            pool
        })
    }

    fn decode_worker_loop(pool: Arc<DecodePool>) {
        loop {
            let (job, reserved_bytes) = {
                let Ok(mut st) = pool.state.lock() else {
                    return;
                };
                loop {
                    if let Some(front) = st.queue.front() {
                        // 无在途任务时放行单张大图避免饥饿死锁；并发时受 `max_in_flight_bytes` 字节配额节流。
                        if st.in_flight_bytes == 0
                            || st.in_flight_bytes.saturating_add(front.estimated_rgba_bytes)
                                <= st.max_in_flight_bytes
                        {
                            let job = st.queue.pop_front().unwrap();
                            let est = job.estimated_rgba_bytes;
                            st.in_flight_bytes = st.in_flight_bytes.saturating_add(est);
                            break (job, est);
                        }
                    }
                    let Ok(guard) = pool.cv.wait(st) else {
                        return;
                    };
                    st = guard;
                }
            };

            let ok = decode_and_commit_async(job.id, &job.bytes);

            {
                if let Ok(mut st) = pool.state.lock() {
                    st.in_flight_bytes = st.in_flight_bytes.saturating_sub(reserved_bytes);
                    pool.cv.notify_all();
                }
            }

            if let Some(cb) = job.on_ready {
                cb(job.id, ok);
            }
        }
    }

    fn decode_and_commit_async(id: ImageAssetId, bytes: &[u8]) -> bool {
        let decoded = decode_image_bytes(bytes);
        let states = GLOBAL_IMAGE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
        // 若解码期间资产已被显式 `unload_image` 或已被更新的同步注册覆盖，避免回写陈旧状态。
        let still_loading = states
            .lock()
            .ok()
            .is_some_and(|s| matches!(s.get(&id), Some(ImageLoadState::Loading)));
        if !still_loading {
            return false;
        }
        if let Some((w, h, rgba)) = decoded {
            register_image_rgba(id, w, h, rgba).is_some()
        } else {
            if let Ok(mut s) = states.lock() {
                if matches!(s.get(&id), Some(ImageLoadState::Loading)) {
                    s.insert(id, ImageLoadState::Failed);
                }
            }
            false
        }
    }

    /// 配置后台异步图像解码池允许同时展开的最大像素字节配额（默认 32 MiB）。
    pub fn set_async_decode_in_flight_budget(max_bytes: usize) {
        let pool = decode_pool();
        if let Ok(mut st) = pool.state.lock() {
            st.max_in_flight_bytes = max_bytes.max(64 * 1024);
            pool.cv.notify_all();
        }
    }

    /// 返回后台异步解码工作池的常驻复用线程数。
    pub fn async_decode_worker_count() -> usize {
        decode_pool()
            .state
            .lock()
            .map(|st| st.worker_count)
            .unwrap_or(0)
    }

    /// 注册外部格式解码器（例如项目外部接入 `zune-jpeg` / `webp` 时无缝挂载到统一管线）。
    pub fn register_custom_image_decoder(decoder: CustomDecoderFn) {
        let list = CUSTOM_DECODERS.get_or_init(|| RwLock::new(Vec::new()));
        if let Ok(mut w) = list.write() {
            w.push(decoder);
        }
    }

    /// 设置全局图像 LRU 缓存字节上限（默认 64 MiB），并立即淘汰超出新预算的旧条目。
    pub fn set_global_image_cache_budget(max_bytes: usize) {
        let cache = GLOBAL_IMAGE_CACHE.get_or_init(|| Mutex::new(LruImageCache::new(DEFAULT_IMAGE_CACHE_BUDGET)));
        if let Ok(mut c) = cache.lock() {
            c.set_max_bytes(max_bytes);
        }
    }

    /// 返回全局图像 CPU 像素缓存实时统计：`(current_bytes, max_bytes, entry_count)`。
    pub fn global_image_cache_stats() -> (usize, usize, usize) {
        let cache = GLOBAL_IMAGE_CACHE.get_or_init(|| Mutex::new(LruImageCache::new(DEFAULT_IMAGE_CACHE_BUDGET)));
        cache
            .lock()
            .map(|c| (c.current_bytes(), c.max_bytes(), c.len()))
            .unwrap_or((0, DEFAULT_IMAGE_CACHE_BUDGET, 0))
    }

    /// 显式释放指定图像在全局 CPU LRU 缓存中的像素数据，但保留轻量元数据（尺寸与版本号）以防 GPU 纹理版本回退。
    pub fn release_cached_image(id: ImageAssetId) -> bool {
        let cache = GLOBAL_IMAGE_CACHE.get_or_init(|| Mutex::new(LruImageCache::new(DEFAULT_IMAGE_CACHE_BUDGET)));
        cache.lock().map(|mut c| c.remove(id)).unwrap_or(false)
    }

    /// 清空全局 CPU 像素 LRU 缓存中的所有像素数据（保留元数据状态）。
    pub fn clear_global_image_cache() {
        let cache = GLOBAL_IMAGE_CACHE.get_or_init(|| Mutex::new(LruImageCache::new(DEFAULT_IMAGE_CACHE_BUDGET)));
        if let Ok(mut c) = cache.lock() {
            c.clear();
        }
    }

    /// 彻底卸载指定图像资产：同时移除 CPU LRU 像素缓存与加载状态/元数据记录。
    pub fn unload_image(id: ImageAssetId) -> bool {
        let removed_px = release_cached_image(id);
        let states = GLOBAL_IMAGE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
        let removed_st = states
            .lock()
            .map(|mut s| s.remove(&id).is_some())
            .unwrap_or(false);
        removed_px || removed_st
    }

    /// 直接向全局 LRU 缓存注册已解码的 RGBA8888 像素（返回递增版本号）。
    ///
    /// 像素内存仅由全局 LRU 缓存与当前使用中的 [`DecodedPixels`] 句柄持有；
    /// `GLOBAL_IMAGE_STATES` 仅记录轻量 [`ImageMetadata`]，被 LRU 淘汰后底层像素会被立即释放。
    pub fn register_image_rgba(id: ImageAssetId, width: u32, height: u32, rgba: Vec<u8>) -> Option<u64> {
        let expected_len = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        if width == 0 || height == 0 || rgba.len() != expected_len {
            return None;
        }
        let rev = NEXT_IMAGE_REV.fetch_add(1, Ordering::Relaxed);
        let px = DecodedPixels {
            width,
            height,
            rgba: Arc::from(rgba.into_boxed_slice()),
            revision: rev,
        };
        let meta = ImageMetadata {
            width,
            height,
            revision: rev,
        };
        let cache = GLOBAL_IMAGE_CACHE.get_or_init(|| Mutex::new(LruImageCache::new(DEFAULT_IMAGE_CACHE_BUDGET)));
        if let Ok(mut c) = cache.lock() {
            c.insert(id, px);
        }
        let states = GLOBAL_IMAGE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut s) = states.lock() {
            s.insert(id, ImageLoadState::Ready(meta));
        }
        Some(rev)
    }

    /// 同步解码 PNG / BMP / 自定义格式字节流并写入全局 LRU 缓存。
    pub fn load_image_bytes(id: ImageAssetId, bytes: &[u8]) -> Option<u64> {
        let (w, h, rgba) = decode_image_bytes(bytes)?;
        register_image_rgba(id, w, h, rgba)
    }

    /// 在有界复用后台解码工作池中异步解码图像字节流，完成后写入全局 LRU 缓存并触发 `on_ready` 唤醒回调。
    ///
    /// 工作池固定复用 `2..=4` 个工作线程并按展开后 RGBA 字节配额节流，避免批量加载时无限开线程或瞬时内存峰值暴涨。
    pub fn load_image_bytes_async(
        id: ImageAssetId,
        bytes: Vec<u8>,
        on_ready: Option<ReadyCallback>,
    ) {
        let states = GLOBAL_IMAGE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut s) = states.lock() {
            s.insert(id, ImageLoadState::Loading);
        }
        let estimated_rgba_bytes = estimate_decoded_rgba_bytes(&bytes);
        let pool = decode_pool();
        let rejected = {
            let Ok(mut st) = pool.state.lock() else {
                return;
            };
            if st.queue.len() >= MAX_DECODE_QUEUE_LEN {
                true
            } else {
                st.queue.push_back(DecodeJob {
                    id,
                    bytes,
                    estimated_rgba_bytes,
                    on_ready: on_ready.clone(),
                });
                pool.cv.notify_one();
                false
            }
        };
        if rejected {
            if let Ok(mut s) = states.lock() {
                if matches!(s.get(&id), Some(ImageLoadState::Loading)) {
                    s.insert(id, ImageLoadState::Failed);
                }
            }
            if let Some(cb) = on_ready {
                cb(id, false);
            }
        }
    }

    /// 查询指定图像 ID 的缓存像素（同时刷新其 LRU 活跃时间戳）。
    pub fn get_cached_image(id: ImageAssetId) -> Option<DecodedPixels> {
        let cache = GLOBAL_IMAGE_CACHE.get_or_init(|| Mutex::new(LruImageCache::new(DEFAULT_IMAGE_CACHE_BUDGET)));
        cache.lock().ok().and_then(|mut c| c.get(id))
    }

    /// 查询指定图像 ID 的异步加载状态（不含像素强引用）。
    pub fn image_load_state(id: ImageAssetId) -> Option<ImageLoadState> {
        let states = GLOBAL_IMAGE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
        states.lock().ok().and_then(|s| s.get(&id).copied())
    }

    /// 直接读取指定图像 ID 的轻量元数据（即使 CPU 像素已被 LRU 淘汰，仍可准确获取宽高与版本号）。
    pub fn image_metadata(id: ImageAssetId) -> Option<ImageMetadata> {
        match image_load_state(id) {
            Some(ImageLoadState::Ready(meta)) => Some(meta),
            _ => None,
        }
    }

    /// 直接读取指定图像 ID 的资产版本号（供 GPU 精灵图集缓存键读取，不受 CPU 像素淘汰影响）。
    pub fn image_revision(id: ImageAssetId) -> Option<u64> {
        image_metadata(id).map(|m| m.revision)
    }

    /// 统一解码入口：内置纯 Rust PNG + BMP 解码器，未知或 JPEG/WebP 格式自动尝试已注册外部解码器。
    pub fn decode_image_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        match detect_image_format(bytes) {
            ImageFormat::Png => decode_png(bytes),
            ImageFormat::Bmp => decode_bmp(bytes),
            _ => {
                if let Some(list) = CUSTOM_DECODERS.get() {
                    if let Ok(r) = list.read() {
                        for dec in r.iter() {
                            if let Some(res) = dec(bytes) {
                                return Some(res);
                            }
                        }
                    }
                }
                None
            }
        }
    }

    // ------------------------------------------------------------------------
    // 纯 Rust BMP 解码器（24-bit BGR / 32-bit BGRA）
    // ------------------------------------------------------------------------

    fn decode_bmp(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        if bytes.len() < 54 || &bytes[..2] != b"BM" {
            return None;
        }
        let data_off = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
        let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let h_signed = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
        let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
        let comp = u32::from_le_bytes([bytes[30], bytes[31], bytes[32], bytes[33]]);
        if w <= 0 || h_signed == 0 || (bpp != 24 && bpp != 32) || comp != 0 {
            return None;
        }
        let width = w as u32;
        let top_down = h_signed < 0;
        let height = h_signed.unsigned_abs();
        let bytes_per_px = (bpp / 8) as usize;
        let row_stride = ((width as usize) * bytes_per_px + 3) & !3;
        if data_off + row_stride * (height as usize) > bytes.len() {
            return None;
        }
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..(height as usize) {
            let src_y = if top_down { y } else { (height as usize) - 1 - y };
            let row = &bytes[data_off + src_y * row_stride..data_off + (src_y + 1) * row_stride];
            for x in 0..(width as usize) {
                let si = x * bytes_per_px;
                let di = (y * (width as usize) + x) * 4;
                rgba[di] = row[si + 2];
                rgba[di + 1] = row[si + 1];
                rgba[di + 2] = row[si];
                rgba[di + 3] = if bytes_per_px == 4 { row[si + 3] } else { 255 };
            }
        }
        Some((width, height, rgba))
    }

    // ------------------------------------------------------------------------
    // 纯 Rust PNG 解码器（RFC 2083 + RFC 1951 Inflate + 5 种滤波器）
    // ------------------------------------------------------------------------

    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,
        bit_buf: u32,
        bit_cnt: u32,
    }

    impl<'a> BitReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self {
                data,
                pos: 0,
                bit_buf: 0,
                bit_cnt: 0,
            }
        }

        #[inline]
        fn read_bits(&mut self, n: u32) -> Option<u32> {
            while self.bit_cnt < n {
                if self.pos >= self.data.len() {
                    return None;
                }
                self.bit_buf |= (self.data[self.pos] as u32) << self.bit_cnt;
                self.pos += 1;
                self.bit_cnt += 8;
            }
            let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
            let val = self.bit_buf & mask;
            self.bit_buf >>= n;
            self.bit_cnt -= n;
            Some(val)
        }

        fn align_byte(&mut self) {
            let drop_bits = self.bit_cnt & 7;
            self.bit_buf >>= drop_bits;
            self.bit_cnt -= drop_bits;
        }

        fn read_u16_le_aligned(&mut self) -> Option<u16> {
            self.align_byte();
            let lo = self.read_bits(8)? as u16;
            let hi = self.read_bits(8)? as u16;
            Some(lo | (hi << 8))
        }
    }

    struct HuffTree {
        counts: [u16; 16],
        symbols: Vec<u16>,
    }

    impl HuffTree {
        fn from_lengths(lengths: &[u8]) -> Option<Self> {
            let mut counts = [0u16; 16];
            for &len in lengths {
                if len as usize >= 16 {
                    return None;
                }
                if len > 0 {
                    counts[len as usize] += 1;
                }
            }
            let mut offsets = [0u16; 16];
            let mut sum = 0u16;
            for bits in 1..16 {
                offsets[bits] = sum;
                sum = sum.checked_add(counts[bits])?;
            }
            let mut symbols = vec![0u16; sum as usize];
            for (sym, &len) in lengths.iter().enumerate() {
                if len > 0 {
                    let off = offsets[len as usize] as usize;
                    symbols[off] = sym as u16;
                    offsets[len as usize] += 1;
                }
            }
            Some(Self { counts, symbols })
        }

        fn decode(&self, br: &mut BitReader<'_>) -> Option<u16> {
            let mut code = 0i32;
            let mut first = 0i32;
            let mut index = 0i32;
            for len in 1..16 {
                code |= br.read_bits(1)? as i32;
                let cnt = self.counts[len] as i32;
                if code - cnt < first {
                    return self.symbols.get((index + (code - first)) as usize).copied();
                }
                index += cnt;
                first = (first + cnt) << 1;
                code <<= 1;
            }
            None
        }
    }

    fn inflate_zlib(zlib: &[u8]) -> Option<Vec<u8>> {
        if zlib.len() < 6 {
            return None;
        }
        let cmf = zlib[0];
        let flg = zlib[1];
        if (cmf & 0x0F) != 8 || (((cmf as u16) << 8 | flg as u16) % 31) != 0 {
            return None;
        }
        let start = if (flg & 0x20) != 0 { 6 } else { 2 };
        let mut br = BitReader::new(&zlib[start..]);
        let mut out = Vec::new();

        const LEN_BASE: [u16; 29] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227, 258,
        ];
        const LEN_EXTRA: [u8; 29] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
        ];
        const DIST_BASE: [u16; 30] = [
            1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
            1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
        ];
        const DIST_EXTRA: [u8; 30] = [
            0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
            12, 13, 13,
        ];
        const CL_ORDER: [usize; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];

        loop {
            let bfinal = br.read_bits(1)?;
            let btype = br.read_bits(2)?;
            match btype {
                0 => {
                    let len = br.read_u16_le_aligned()?;
                    let nlen = br.read_u16_le_aligned()?;
                    if len != !nlen {
                        return None;
                    }
                    for _ in 0..len {
                        out.push(br.read_bits(8)? as u8);
                    }
                }
                1 | 2 => {
                    let (lit_tree, dist_tree) = if btype == 1 {
                        let mut ll = [0u8; 288];
                        for l in &mut ll[0..144] {
                            *l = 8;
                        }
                        for l in &mut ll[144..256] {
                            *l = 9;
                        }
                        for l in &mut ll[256..280] {
                            *l = 7;
                        }
                        for l in &mut ll[280..288] {
                            *l = 8;
                        }
                        let dl = [5u8; 32];
                        (HuffTree::from_lengths(&ll)?, HuffTree::from_lengths(&dl)?)
                    } else {
                        let hlit = br.read_bits(5)? as usize + 257;
                        let hdist = br.read_bits(5)? as usize + 1;
                        let hclen = br.read_bits(4)? as usize + 4;
                        let mut cl_lens = [0u8; 19];
                        for i in 0..hclen {
                            cl_lens[CL_ORDER[i]] = br.read_bits(3)? as u8;
                        }
                        let cl_tree = HuffTree::from_lengths(&cl_lens)?;
                        let mut lens = Vec::with_capacity(hlit + hdist);
                        while lens.len() < hlit + hdist {
                            let sym = cl_tree.decode(&mut br)?;
                            match sym {
                                0..=15 => lens.push(sym as u8),
                                16 => {
                                    let prev = *lens.last()?;
                                    let rep = br.read_bits(2)? as usize + 3;
                                    for _ in 0..rep {
                                        lens.push(prev);
                                    }
                                }
                                17 => {
                                    let rep = br.read_bits(3)? as usize + 3;
                                    for _ in 0..rep {
                                        lens.push(0);
                                    }
                                }
                                18 => {
                                    let rep = br.read_bits(7)? as usize + 11;
                                    for _ in 0..rep {
                                        lens.push(0);
                                    }
                                }
                                _ => return None,
                            }
                        }
                        if lens.len() < hlit + hdist {
                            return None;
                        }
                        (
                            HuffTree::from_lengths(&lens[..hlit])?,
                            HuffTree::from_lengths(&lens[hlit..hlit + hdist])?,
                        )
                    };

                    loop {
                        let sym = lit_tree.decode(&mut br)?;
                        if sym < 256 {
                            out.push(sym as u8);
                        } else if sym == 256 {
                            break;
                        } else {
                            let lidx = (sym as usize).checked_sub(257)?;
                            if lidx >= 29 {
                                return None;
                            }
                            let length = LEN_BASE[lidx] as usize
                                + br.read_bits(LEN_EXTRA[lidx] as u32)? as usize;
                            let dsym = dist_tree.decode(&mut br)? as usize;
                            if dsym >= 30 {
                                return None;
                            }
                            let dist = DIST_BASE[dsym] as usize
                                + br.read_bits(DIST_EXTRA[dsym] as u32)? as usize;
                            if dist == 0 || dist > out.len() {
                                return None;
                            }
                            for _ in 0..length {
                                let b = out[out.len() - dist];
                                out.push(b);
                            }
                        }
                    }
                }
                _ => return None,
            }
            if bfinal != 0 {
                break;
            }
        }
        Some(out)
    }

    #[inline]
    fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
        let ai = a as i32;
        let bi = b as i32;
        let ci = c as i32;
        let p = ai + bi - ci;
        let pa = (p - ai).abs();
        let pb = (p - bi).abs();
        let pc = (p - ci).abs();
        if pa <= pb && pa <= pc {
            a
        } else if pb <= pc {
            b
        } else {
            c
        }
    }

    fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        if bytes.len() < 33 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
            return None;
        }
        let mut pos = 8usize;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut bit_depth = 0u8;
        let mut color_type = 0u8;
        let mut interlace = 0u8;
        let mut palette: Vec<[u8; 3]> = Vec::new();
        let mut trns: Vec<u8> = Vec::new();
        let mut idat: Vec<u8> = Vec::new();

        while pos + 12 <= bytes.len() {
            let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
            let ctype = &bytes[pos + 4..pos + 8];
            let data_start = pos + 8;
            let data_end = data_start.checked_add(len)?;
            if data_end + 4 > bytes.len() {
                return None;
            }
            let chunk = &bytes[data_start..data_end];
            match ctype {
                b"IHDR" => {
                    if chunk.len() < 13 {
                        return None;
                    }
                    width = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    height = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                    bit_depth = chunk[8];
                    color_type = chunk[9];
                    interlace = chunk[12];
                }
                b"PLTE" => {
                    for rgb in chunk.chunks_exact(3) {
                        palette.push([rgb[0], rgb[1], rgb[2]]);
                    }
                }
                b"tRNS" => {
                    trns.extend_from_slice(chunk);
                }
                b"IDAT" => {
                    idat.extend_from_slice(chunk);
                }
                b"IEND" => break,
                _ => {}
            }
            pos = data_end + 4;
        }

        if width == 0 || height == 0 || bit_depth != 8 || interlace != 0 {
            return None;
        }
        let bpp = match color_type {
            0 => 1usize, // Gray
            2 => 3usize, // RGB
            3 => 1usize, // Indexed
            4 => 2usize, // GrayAlpha
            6 => 4usize, // RGBA
            _ => return None,
        };
        let raw = inflate_zlib(&idat)?;
        let row_bytes = (width as usize) * bpp;
        if raw.len() < (height as usize) * (row_bytes + 1) {
            return None;
        }

        let mut prev_row = vec![0u8; row_bytes];
        let mut cur_row = vec![0u8; row_bytes];
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];

        for y in 0..(height as usize) {
            let line = &raw[y * (row_bytes + 1)..(y + 1) * (row_bytes + 1)];
            let filter = line[0];
            let src = &line[1..];
            for i in 0..row_bytes {
                let left = if i >= bpp { cur_row[i - bpp] } else { 0 };
                let up = prev_row[i];
                let up_left = if i >= bpp { prev_row[i - bpp] } else { 0 };
                cur_row[i] = match filter {
                    0 => src[i],
                    1 => src[i].wrapping_add(left),
                    2 => src[i].wrapping_add(up),
                    3 => src[i].wrapping_add((((left as u16) + (up as u16)) >> 1) as u8),
                    4 => src[i].wrapping_add(paeth_predictor(left, up, up_left)),
                    _ => return None,
                };
            }

            let dst_row = &mut rgba[y * (width as usize) * 4..(y + 1) * (width as usize) * 4];
            match color_type {
                0 => {
                    for x in 0..(width as usize) {
                        let g = cur_row[x];
                        let d = x * 4;
                        dst_row[d] = g;
                        dst_row[d + 1] = g;
                        dst_row[d + 2] = g;
                        dst_row[d + 3] = 255;
                    }
                }
                2 => {
                    for x in 0..(width as usize) {
                        let s = x * 3;
                        let d = x * 4;
                        dst_row[d] = cur_row[s];
                        dst_row[d + 1] = cur_row[s + 1];
                        dst_row[d + 2] = cur_row[s + 2];
                        dst_row[d + 3] = 255;
                    }
                }
                3 => {
                    for x in 0..(width as usize) {
                        let idx = cur_row[x] as usize;
                        let [r, g, b] = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                        let a = trns.get(idx).copied().unwrap_or(255);
                        let d = x * 4;
                        dst_row[d] = r;
                        dst_row[d + 1] = g;
                        dst_row[d + 2] = b;
                        dst_row[d + 3] = a;
                    }
                }
                4 => {
                    for x in 0..(width as usize) {
                        let s = x * 2;
                        let d = x * 4;
                        let g = cur_row[s];
                        dst_row[d] = g;
                        dst_row[d + 1] = g;
                        dst_row[d + 2] = g;
                        dst_row[d + 3] = cur_row[s + 1];
                    }
                }
                6 => {
                    dst_row.copy_from_slice(&cur_row);
                }
                _ => return None,
            }
            core::mem::swap(&mut prev_row, &mut cur_row);
        }

        Some((width, height, rgba))
    }
}

pub use std_pipeline::{
    async_decode_worker_count, clear_global_image_cache, decode_image_bytes, detect_image_format,
    get_cached_image, global_image_cache_stats, image_load_state, image_metadata, image_revision,
    load_image_bytes, load_image_bytes_async, register_custom_image_decoder, register_image_rgba,
    release_cached_image, set_async_decode_in_flight_budget, set_global_image_cache_budget,
    unload_image, DecodedPixels, ImageFormat, ImageLoadState, ImageMetadata, LruImageCache,
};
