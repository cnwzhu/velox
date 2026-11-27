//! 桌面操作系统深度集成原语（`os_integration`）：
//! 统一封装原生顶部菜单栏（`NativeMenuBar`）、系统托盘（`SystemTraySpec`）、
//! 原生文件对话框协议（`FileDialogRequest`）与跨应用拖放幽灵预览（`DragPreviewGhost`）。

use std::boxed::Box;
use std::path::PathBuf;
use std::string::String;
use std::sync::{Mutex, OnceLock};
use std::vec::Vec;

use crate::solver::Region;

/// 原生菜单项定义（对应 macOS NSMenu / Windows HMENU / Linux DBusMenu）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NativeMenuItem {
    Action {
        id: u32,
        label: String,
        accelerator: Option<String>,
        enabled: bool,
        checked: Option<bool>,
    },
    Separator,
    Submenu {
        label: String,
        items: Vec<NativeMenuItem>,
    },
}

impl NativeMenuItem {
    pub fn action(id: u32, label: impl Into<String>, accelerator: Option<&str>) -> Self {
        Self::Action {
            id,
            label: label.into(),
            accelerator: accelerator.map(String::from),
            enabled: true,
            checked: None,
        }
    }

    pub fn checkbox(id: u32, label: impl Into<String>, checked: bool) -> Self {
        Self::Action {
            id,
            label: label.into(),
            accelerator: None,
            enabled: true,
            checked: Some(checked),
        }
    }
}

/// 顶层原生菜单栏分组（如 `File` / `Edit` / `View` / `Window`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NativeMenuSection {
    pub title: String,
    pub items: Vec<NativeMenuItem>,
}

/// 跨平台声明式原生菜单栏模型。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NativeMenuBar {
    pub sections: Vec<NativeMenuSection>,
}

impl NativeMenuBar {
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    pub fn add_section(mut self, title: impl Into<String>, items: Vec<NativeMenuItem>) -> Self {
        self.sections.push(NativeMenuSection {
            title: title.into(),
            items,
        });
        self
    }

    /// 按动作 ID 检索菜单项名称（用于快捷键回显与自动化校验）。
    pub fn find_action_label(&self, target_id: u32) -> Option<&str> {
        fn search(items: &[NativeMenuItem], id: u32) -> Option<&str> {
            for item in items {
                match item {
                    NativeMenuItem::Action {
                        id: item_id, label, ..
                    } if *item_id == id => return Some(label.as_str()),
                    NativeMenuItem::Submenu { items: sub, .. } => {
                        if let Some(found) = search(sub, id) {
                            return Some(found);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        for sec in &self.sections {
            if let Some(found) = search(&sec.items, target_id) {
                return Some(found);
            }
        }
        None
    }
}

/// 系统托盘图标与菜单描述（Windows NotifyIcon / macOS NSStatusItem / Linux StatusNotifierItem）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SystemTraySpec {
    pub id: String,
    pub tooltip: String,
    pub badge_count: u32,
    pub menu: Vec<NativeMenuItem>,
}

impl SystemTraySpec {
    pub fn new(id: impl Into<String>, tooltip: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            tooltip: tooltip.into(),
            badge_count: 0,
            menu: Vec::new(),
        }
    }

    pub fn with_menu(mut self, items: Vec<NativeMenuItem>) -> Self {
        self.menu = items;
        self
    }
}

/// 系统文件对话框模式。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FileDialogMode {
    OpenFile,
    OpenMultiple,
    SaveFile,
    PickFolder,
}

/// 文件扩展名过滤器。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// 跨平台原生文件对话框请求。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileDialogRequest {
    pub mode: FileDialogMode,
    pub title: String,
    pub default_name: Option<String>,
    pub filters: Vec<FileFilter>,
}

impl FileDialogRequest {
    pub fn open_file(title: impl Into<String>) -> Self {
        Self {
            mode: FileDialogMode::OpenFile,
            title: title.into(),
            default_name: None,
            filters: Vec::new(),
        }
    }

    pub fn save_file(title: impl Into<String>, default_name: impl Into<String>) -> Self {
        Self {
            mode: FileDialogMode::SaveFile,
            title: title.into(),
            default_name: Some(default_name.into()),
            filters: Vec::new(),
        }
    }

    pub fn add_filter(mut self, name: impl Into<String>, exts: &[&str]) -> Self {
        self.filters.push(FileFilter {
            name: name.into(),
            extensions: exts.iter().map(|s| String::from(*s)).collect(),
        });
        self
    }
}

/// 文件对话框执行结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FileDialogResult {
    Selected(Vec<PathBuf>),
    Cancelled,
}

type MockDialogFn = Box<dyn Fn(&FileDialogRequest) -> FileDialogResult + Send + Sync>;

static MOCK_FILE_DIALOG: OnceLock<Mutex<Option<MockDialogFn>>> = OnceLock::new();

/// 注册确定性文件对话框拦截器（供无头 CI 自动化测试或自定义沙箱文件系统宿主使用）。
pub fn set_mock_file_dialog_handler<F>(handler: Option<F>)
where
    F: Fn(&FileDialogRequest) -> FileDialogResult + Send + Sync + 'static,
{
    let lock = MOCK_FILE_DIALOG.get_or_init(|| Mutex::new(None));
    if let Ok(mut g) = lock.lock() {
        *g = handler.map(|f| Box::new(f) as MockDialogFn);
    }
}

/// 触发文件对话框请求（优先命中已注册测试/宿主处理器；否则返回 `Cancelled` 避免无头阻塞）。
pub fn run_file_dialog(req: &FileDialogRequest) -> FileDialogResult {
    let lock = MOCK_FILE_DIALOG.get_or_init(|| Mutex::new(None));
    if let Ok(g) = lock.lock() {
        if let Some(ref f) = *g {
            return f(req);
        }
    }
    FileDialogResult::Cancelled
}

/// 跨应用拖拽跟随幽灵预览浮层（`DragPreviewGhost`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DragPreviewGhost {
    pub title: String,
    pub subtitle: String,
    pub badge_count: u32,
    pub pointer_x: i16,
    pub pointer_y: i16,
    pub width: i16,
    pub height: i16,
}

impl DragPreviewGhost {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>, px: i16, py: i16) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            badge_count: 1,
            pointer_x: px,
            pointer_y: py,
            width: 196,
            height: 44,
        }
    }

    /// 计算不越出主视口边界的跟随屏幕矩形。
    #[inline]
    pub fn clamped_bounds(&self, viewport: Region) -> Region {
        let x = (self.pointer_x + 14).clamp(
            viewport.x,
            (viewport.right() - self.width).max(viewport.x),
        );
        let y = (self.pointer_y + 14).clamp(
            viewport.y,
            (viewport.bottom() - self.height).max(viewport.y),
        );
        Region::new(x, y, self.width, self.height)
    }
}
