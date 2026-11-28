//! 磁盘持久化二进制 `wgpu::PipelineCache` 管理器（跨进程零毫秒复用已编译 PSO 机器码）。
//!
//! - 仅在硬件后端声明支持 `wgpu::Features::PIPELINE_CACHE`（如 Vulkan）时自动激活；
//! - 文件路径自动绑定 `AdapterInfo`（`vendor` / `device` / `backend` / `driver_info` 指纹），显卡驱动升级自动安全失效；
//! - 写回采用「写临时文件 + 原子覆盖」策略，防多实例并发写坏缓存。

use ::wgpu;
use std::path::PathBuf;
use std::string::String;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::vec::Vec;

const CACHE_MAGIC: &[u8; 8] = b"VLXPSO01";

#[inline]
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("velox").join("pso_cache");
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
            return PathBuf::from(xdg).join("velox").join("pso_cache");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".cache").join("velox").join("pso_cache");
        }
    }
    std::env::temp_dir().join("velox_pso_cache")
}

fn adapter_fingerprint(info: &wgpu::AdapterInfo) -> (String, u64) {
    let mut sig = String::with_capacity(96);
    use core::fmt::Write;
    let _ = write!(
        sig,
        "{:04x}_{:04x}_{:?}_{}_{}",
        info.vendor, info.device, info.backend, info.driver, info.driver_info
    );
    let hash = fnv1a_64(sig.as_bytes());
    let filename = std::format!(
        "pso_{:04x}_{:04x}_{:?}_{:016x}.bin",
        info.vendor, info.device, info.backend, hash
    );
    (filename, hash)
}

/// 绑定特定显卡与驱动版本的持久化二进制 `wgpu::PipelineCache` 封装。
pub(crate) struct DiskPipelineCache {
    cache: wgpu::PipelineCache,
    path: PathBuf,
    fingerprint: u64,
    last_saved_hash: AtomicU64,
    loaded_from_disk: bool,
}

// SAFETY: `wgpu::PipelineCache`、`PathBuf` 与 `AtomicU64` 均跨线程安全。
unsafe impl Send for DiskPipelineCache {}
unsafe impl Sync for DiskPipelineCache {}

impl DiskPipelineCache {
    /// 若 `device` 已启用 `Features::PIPELINE_CACHE`，从磁盘载入匹配的二进制缓存并创建 `PipelineCache`。
    pub(crate) fn load(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Option<Arc<Self>> {
        if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return None;
        }
        let info = adapter.get_info();
        let (filename, fingerprint) = adapter_fingerprint(&info);
        let dir = cache_dir();
        let path = dir.join(filename);

        let mut loaded_payload: Option<Vec<u8>> = None;
        let mut initial_hash = 0u64;
        if let Ok(raw) = std::fs::read(&path) {
            if raw.len() > 16
                && &raw[0..8] == CACHE_MAGIC
                && u64::from_le_bytes(raw[8..16].try_into().unwrap_or([0; 8])) == fingerprint
            {
                let payload = raw[16..].to_vec();
                initial_hash = fnv1a_64(&payload);
                loaded_payload = Some(payload);
            }
        }
        let loaded_from_disk = loaded_payload.is_some();

        // SAFETY: `fallback: true` 保证遇到跨驱动/损坏数据时 wgpu 安全回退为空缓存而非 UB。
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("velox_disk_pipeline_cache"),
                data: loaded_payload.as_deref(),
                fallback: true,
            })
        };

        Some(Arc::new(Self {
            cache,
            path,
            fingerprint,
            last_saved_hash: AtomicU64::new(initial_hash),
            loaded_from_disk,
        }))
    }

    #[inline]
    pub(crate) fn wgpu_cache(&self) -> &wgpu::PipelineCache {
        &self.cache
    }

    #[inline]
    pub(crate) fn loaded_from_disk(&self) -> bool {
        self.loaded_from_disk
    }

    /// 将新增编译的二进制管线机器码增量原子写回磁盘缓存（无变更时零磁盘 I/O）。
    pub(crate) fn persist(&self) {
        let Some(data) = self.cache.get_data() else {
            return;
        };
        if data.is_empty() {
            return;
        }
        let hash = fnv1a_64(&data);
        if self.last_saved_hash.load(Ordering::Relaxed) == hash {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = Vec::with_capacity(16 + data.len());
        out.extend_from_slice(CACHE_MAGIC);
        out.extend_from_slice(&self.fingerprint.to_le_bytes());
        out.extend_from_slice(&data);

        let mut tmp_name = self.path.as_os_str().to_os_string();
        tmp_name.push(std::format!(".{}.tmp", std::process::id()));
        let tmp_path = PathBuf::from(tmp_name);
        if std::fs::write(&tmp_path, &out).is_ok() {
            if std::fs::rename(&tmp_path, &self.path).is_err() {
                let _ = std::fs::remove_file(&self.path);
                let _ = std::fs::rename(&tmp_path, &self.path);
            }
            self.last_saved_hash.store(hash, Ordering::Relaxed);
        } else {
            let _ = std::fs::remove_file(&tmp_path);
        }
    }
}

/// 请求 `wgpu::Device` 并在适配器支持时自动开启 `Features::PIPELINE_CACHE` 且加载磁盘缓存。
pub(crate) fn request_device_with_pso_cache(
    adapter: &wgpu::Adapter,
    label: &'static str,
    required_limits: wgpu::Limits,
) -> Option<(wgpu::Device, wgpu::Queue, Option<Arc<DiskPipelineCache>>)> {
    let want_cache = adapter.features().contains(wgpu::Features::PIPELINE_CACHE);
    let required_features = if want_cache {
        wgpu::Features::PIPELINE_CACHE
    } else {
        wgpu::Features::empty()
    };
    let (device, queue) = super::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        required_features,
        required_limits,
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
    }))
    .ok()?;
    let pso_cache = DiskPipelineCache::load(adapter, &device);
    Some((device, queue, pso_cache))
}
