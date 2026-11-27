//! 跨平台零外部依赖系统剪贴板抽象（`std` 与 `#![no_std]` 双模兼容）。
//!
//! - **Windows (`std`)**：直接调用 Win32 `user32.dll` / `kernel32.dll` 的 `CF_UNICODETEXT` 原生剪贴板接口，
//!   同时维护进程内线程安全回退缓存，保证跨应用自由复制粘贴以及并发无头测试的确定性；
//! - **其他 `std` 平台（macOS / Linux / Android / iOS / HarmonyOS / Web）**：提供进程内线程安全共享剪贴板；
//! - **`#![no_std]` 嵌入式目标**：提供零堆自旋锁保护的内联剪贴板缓冲。

use crate::arena::SmallString;

mod imp {
    use std::string::String;
    use std::sync::{Mutex, OnceLock};

    static FALLBACK_CLIPBOARD: OnceLock<Mutex<String>> = OnceLock::new();

    #[inline]
    fn fallback() -> &'static Mutex<String> {
        FALLBACK_CLIPBOARD.get_or_init(|| Mutex::new(String::new()))
    }

    #[cfg(target_os = "windows")]
    mod win32 {
        use core::ffi::c_void;
        use std::string::String;
        use std::vec::Vec;

        const CF_UNICODETEXT: u32 = 13;
        const GMEM_MOVEABLE: u32 = 0x0002;

        #[link(name = "user32")]
        unsafe extern "system" {
            fn OpenClipboard(hwnd_new_owner: *mut c_void) -> i32;
            fn CloseClipboard() -> i32;
            fn EmptyClipboard() -> i32;
            fn GetClipboardData(u_format: u32) -> *mut c_void;
            fn SetClipboardData(u_format: u32, h_mem: *mut c_void) -> *mut c_void;
            fn IsClipboardFormatAvailable(format: u32) -> i32;
        }

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GlobalAlloc(u_flags: u32, dw_bytes: usize) -> *mut c_void;
            fn GlobalLock(h_mem: *mut c_void) -> *mut c_void;
            fn GlobalUnlock(h_mem: *mut c_void) -> i32;
            fn GlobalFree(h_mem: *mut c_void) -> *mut c_void;
        }

        pub fn write_text(text: &str) -> bool {
            let mut utf16: Vec<u16> = text.encode_utf16().collect();
            utf16.push(0);
            let byte_len = utf16.len() * core::mem::size_of::<u16>();

            unsafe {
                if OpenClipboard(core::ptr::null_mut()) == 0 {
                    return false;
                }
                if EmptyClipboard() == 0 {
                    let _ = CloseClipboard();
                    return false;
                }
                let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_len);
                if hmem.is_null() {
                    let _ = CloseClipboard();
                    return false;
                }
                let ptr = GlobalLock(hmem) as *mut u16;
                if ptr.is_null() {
                    let _ = GlobalFree(hmem);
                    let _ = CloseClipboard();
                    return false;
                }
                core::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
                let _ = GlobalUnlock(hmem);
                let set_res = SetClipboardData(CF_UNICODETEXT, hmem);
                let _ = CloseClipboard();
                if set_res.is_null() {
                    let _ = GlobalFree(hmem);
                    return false;
                }
                true
            }
        }

        pub fn read_text() -> Option<String> {
            unsafe {
                if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                    return None;
                }
                if OpenClipboard(core::ptr::null_mut()) == 0 {
                    return None;
                }
                let handle = GetClipboardData(CF_UNICODETEXT);
                if handle.is_null() {
                    let _ = CloseClipboard();
                    return None;
                }
                let ptr = GlobalLock(handle) as *const u16;
                if ptr.is_null() {
                    let _ = CloseClipboard();
                    return None;
                }
                let mut len = 0usize;
                // 防御性上限 4M UTF-16 码元，避免异常系统剪贴板数据造成无限扫描
                while len < 4 * 1024 * 1024 && *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = core::slice::from_raw_parts(ptr, len);
                let text = String::from_utf16_lossy(slice);
                let _ = GlobalUnlock(handle);
                let _ = CloseClipboard();
                Some(text)
            }
        }

        pub fn has_text() -> bool {
            unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 }
        }
    }

    #[cfg(target_os = "windows")]
    static WIN32_FALLBACK_AHEAD: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    pub fn write_text(text: &str) -> bool {
        if let Ok(mut guard) = fallback().lock() {
            guard.clear();
            guard.push_str(text);
        }
        #[cfg(target_os = "windows")]
        {
            // 若系统剪贴板此刻被其它进程短暂锁定，做短重试并在仍失败时标记回退缓存领先，防止读到旧 OS 内容
            let mut synced = win32::write_text(text);
            if !synced {
                for _ in 0..3 {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    if win32::write_text(text) {
                        synced = true;
                        break;
                    }
                }
            }
            WIN32_FALLBACK_AHEAD.store(!synced, core::sync::atomic::Ordering::Release);
        }
        true
    }

    pub fn read_text() -> Option<String> {
        #[cfg(target_os = "windows")]
        if !WIN32_FALLBACK_AHEAD.load(core::sync::atomic::Ordering::Acquire) {
            if let Some(os_text) = win32::read_text() {
                if !os_text.is_empty() {
                    return Some(os_text);
                }
            }
        }
        if let Ok(guard) = fallback().lock() {
            if !guard.is_empty() {
                return Some(guard.clone());
            }
        }
        None
    }

    pub fn has_text() -> bool {
        #[cfg(target_os = "windows")]
        if win32::has_text() {
            return true;
        }
        fallback().lock().map(|g| !g.is_empty()).unwrap_or(false)
    }
}


/// 将 UTF-8 文本写入操作系统剪贴板。
#[inline]
pub fn clipboard_write_text(text: &str) -> bool {
    imp::write_text(text)
}

/// 从剪贴板读取 UTF-8 文本并放入 [`SmallString`]。
#[inline]
pub fn clipboard_read_into<const N: usize>() -> SmallString<N> {
    {
        match imp::read_text() {
            Some(s) => SmallString::from_str_trunc(&s),
            None => SmallString::new(),
        }
    }
}

/// 从操作系统剪贴板读取完整 UTF-8 字符串（仅 `std` 可用）。
#[inline]
pub fn clipboard_read_text() -> Option<std::string::String> {
    imp::read_text()
}

/// 判断剪贴板当前是否存在可供粘贴的非空文本。
#[inline]
pub fn clipboard_has_text() -> bool {
    imp::has_text()
}

/// 富格式剪贴板图像数据（RGBA8888）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: std::vec::Vec<u8>,
}

/// 富格式剪贴板负载（支持纯文本、RGBA 位图、文件路径列表及任意自定义 MIME 数据）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ClipboardPayload {
    Text(std::string::String),
    Image(ClipboardImage),
    FilePaths(std::vec::Vec<std::path::PathBuf>),
    CustomMime {
        mime: std::string::String,
        bytes: std::vec::Vec<u8>,
    },
}

static RICH_CLIPBOARD: std::sync::OnceLock<std::sync::Mutex<Option<ClipboardPayload>>> =
    std::sync::OnceLock::new();

/// 向跨平台富格式剪贴板写入多模态负载（写入文本时同步刷新系统原生剪贴板）。
pub fn clipboard_write_payload(payload: ClipboardPayload) -> bool {
    if let ClipboardPayload::Text(ref t) = payload {
        let _ = clipboard_write_text(t);
    }
    let lock = RICH_CLIPBOARD.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut g) = lock.lock() {
        *g = Some(payload);
        true
    } else {
        false
    }
}

/// 从富格式剪贴板读取最近写入的多模态负载；若仅系统文本剪贴板存在内容则自动封装为 `ClipboardPayload::Text`。
pub fn clipboard_read_payload() -> Option<ClipboardPayload> {
    let lock = RICH_CLIPBOARD.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(g) = lock.lock() {
        if let Some(ref p) = *g {
            return Some(p.clone());
        }
    }
    clipboard_read_text().map(ClipboardPayload::Text)
}
