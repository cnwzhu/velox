//! 统一跨端输入事件与基础几何/手势/键盘/IME 类型定义。

/// 按钮点击事件参数。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct ClickEvent {
    pub x: i16,
    pub y: i16,
}

/// 鼠标按键标识（左键、右键、中键）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum PointerButton {
    /// 左键 / 主触摸点。
    #[default]
    Left,
    /// 右键（上下文菜单）。
    Right,
    /// 中键（滚轮按下）。
    Middle,
}

/// 指针输入设备来源类型（用于隔离桌面鼠标与触控屏的拖拽滚动/惯性/边缘返回手势）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum PointerKind {
    /// 触摸屏 / 手指触控（默认启用跟手拖拽滚动、松手惯性滚动与左边缘右滑返回手势）。
    #[default]
    Touch = 0,
    /// 桌面鼠标 / 触控板指针（默认仅触发显式 `.on_drag(...)` 控件，禁止在空白可滚动页面上按住左键拖动画布）。
    Mouse = 1,
    /// 手写笔 / 压感笔（按精确指针处理，禁止在空白滚动区误触发拖拽滚屏）。
    Pen = 2,
}

/// 鼠标光标视觉形状（对应 `wp-cursor-shape-v1`、CSS Cursor 与桌面平台标准光标形态）。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CursorShape {
    /// 默认箭头光标。
    #[default]
    Default = 0,
    /// 手型点击光标（链接/按钮/复选框/开关/页签悬停）。
    Pointer = 1,
    /// I 型文本编辑光标（输入框/代码编辑器）。
    Text = 2,
    /// 可抓取手型光标（滑块/拖拽把手悬停）。
    Grab = 3,
    /// 抓取拖拽中光标（滑块/把手按住拖拽）。
    Grabbing = 4,
    /// 禁止/禁用不可交互光标（`disabled` 控件悬停）。
    NotAllowed = 5,
    /// 加载/后台处理进度光标（`loading` 按钮悬停）。
    Progress = 6,
    /// 四向移动光标。
    Move = 7,
    /// 十字准星光标（取色器/画布区域选择）。
    Crosshair = 8,
    /// 垂直双向调整尺寸光标（上下分隔条/上下边框）。
    ResizeNS = 9,
    /// 水平双向调整尺寸光标（左右分隔条/左右边框）。
    ResizeEW = 10,
    /// 对角线调整尺寸光标（左上-右下）。
    ResizeNWSE = 11,
    /// 对角线调整尺寸光标（右上-左下）。
    ResizeNESW = 12,
}

impl CursorShape {
    /// 返回标准 CSS `cursor` 属性名（便于 WebAssembly `<canvas>` 直接同步浏览器光标）。
    #[inline(always)]
    pub const fn as_css_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Pointer => "pointer",
            Self::Text => "text",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::NotAllowed => "not-allowed",
            Self::Progress => "progress",
            Self::Move => "move",
            Self::Crosshair => "crosshair",
            Self::ResizeNS => "ns-resize",
            Self::ResizeEW => "ew-resize",
            Self::ResizeNWSE => "nwse-resize",
            Self::ResizeNESW => "nesw-resize",
        }
    }
}

/// 触摸/鼠标快速滑动（Swipe）手势方向。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SwipeDir {
    /// 向左快速滑动（如进入下一页）。
    Left,
    /// 向右快速滑动（如边缘返回上一页）。
    Right,
    /// 向上快速滑动（如呼出底部通知栏）。
    Up,
    /// 向下快速滑动（如拉出顶部控制中心）。
    Down,
}

/// 触摸/鼠标拖拽事件参数。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct DragEvent {
    /// 当前指针屏幕坐标 X。
    pub x: i16,
    /// 当前指针屏幕坐标 Y。
    pub y: i16,
    /// 相对目标元素左边界的局部坐标 X。
    pub local_x: i16,
    /// 相对目标元素上边界的局部坐标 Y。
    pub local_y: i16,
    /// 目标元素宽度（便于计算百分比进度，如 `Slider`）。
    pub width: i16,
    /// 目标元素高度。
    pub height: i16,
    /// 相对上一帧的位移增量 X。
    pub delta_x: i16,
    /// 相对上一帧的位移增量 Y。
    pub delta_y: i16,
}

/// 物理按键与旋转编码器导航键值（面向无触摸屏 MCU 与桌面键盘）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeyCode {
    /// 切换焦点至下一个可聚焦元素（`Tab` 或旋钮顺时针）。
    Tab,
    /// 切换焦点至上一个可聚焦元素（`Shift+Tab` 或旋钮逆时针）。
    BackTab,
    /// 方向键：上。
    Up,
    /// 方向键：下。
    Down,
    /// 方向键：左。
    Left,
    /// 方向键：右。
    Right,
    /// 确认/触发当前焦点元素（`Enter` 或旋钮按下）。
    Enter,
    /// 空格触发。
    Space,
    /// 返回/取消。
    Escape,
    /// 退格键（删除光标前字符）。
    Backspace,
    /// 删除键（删除光标后字符）。
    Delete,
    /// 移至行首。
    Home,
    /// 移至行尾。
    End,
    /// 向上翻页（`PageUp`）。
    PageUp,
    /// 向下翻页（`PageDown`）。
    PageDown,
    /// 字符输入。
    Char(char),
}

/// 键盘修饰键状态位掩码（紧凑 1 字节表示，支持 Shift / Ctrl / Alt / Super）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct KeyModifiers(pub u8);

impl KeyModifiers {
    /// 无修饰键按下。
    pub const NONE: Self = Self(0);
    /// `Shift` 修饰键。
    pub const SHIFT: Self = Self(1 << 0);
    /// `Ctrl` 修饰键。
    pub const CTRL: Self = Self(1 << 1);
    /// `Alt` 修饰键。
    pub const ALT: Self = Self(1 << 2);
    /// `Super` / `Logo` / `Meta` 修饰键（窗口管理器默认主修饰键）。
    pub const LOGO: Self = Self(1 << 3);

    /// 是否没有任何修饰键按下。
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 是否按下了 `Shift`。
    #[inline(always)]
    pub const fn shift(self) -> bool {
        (self.0 & Self::SHIFT.0) != 0
    }

    /// 是否按下了 `Ctrl`。
    #[inline(always)]
    pub const fn ctrl(self) -> bool {
        (self.0 & Self::CTRL.0) != 0
    }

    /// 是否按下了 `Alt`。
    #[inline(always)]
    pub const fn alt(self) -> bool {
        (self.0 & Self::ALT.0) != 0
    }

    /// 是否按下了 `Super` / `Logo`。
    #[inline(always)]
    pub const fn logo(self) -> bool {
        (self.0 & Self::LOGO.0) != 0
    }

    /// 合并修饰键集合。
    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// 添加指定修饰键标志（等价于 [`Self::union`]）。
    #[inline(always)]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// 移除指定修饰键。
    #[inline(always)]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// 是否包含命令快捷键修饰键（`Super`/`Logo`、`Alt` 或 `Ctrl`），用于区分普通打字输入与全局快捷键。
    #[inline(always)]
    pub const fn has_shortcut_modifier(self) -> bool {
        (self.0 & (Self::LOGO.0 | Self::ALT.0 | Self::CTRL.0)) != 0
    }
}

/// 移动端与异形屏安全区域内边距（状态栏、刘海屏/灵动岛、圆角边缘与底部 Home Indicator）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SafeAreaInsets {
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
    pub left: i16,
}

impl SafeAreaInsets {
    /// 全零安全区域常量（适用于 `const fn` 零栈初始化）。
    pub const ZERO: Self = Self {
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
    };

    /// 创建四向安全区域内边距 `(top, right, bottom, left)`。
    #[inline(always)]
    pub const fn new(top: i16, right: i16, bottom: i16, left: i16) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// 常见全面屏手机竖屏默认安全区域（顶部状态栏/灵动岛 `44px`，底部 Home Indicator `34px`）。
    #[inline(always)]
    pub const fn phone_portrait() -> Self {
        Self {
            top: 44,
            right: 0,
            bottom: 34,
            left: 0,
        }
    }

    /// 常见全面屏手机横屏默认安全区域（左右异形区 `44px`，底部 Home Indicator `21px`）。
    #[inline(always)]
    pub const fn phone_landscape() -> Self {
        Self {
            top: 0,
            right: 44,
            bottom: 21,
            left: 44,
        }
    }

    /// Android 全面屏手机竖屏默认安全区域（状态栏/挖孔屏 `48px`，底部手势导航条 `34px`）。
    #[inline(always)]
    pub const fn android_portrait() -> Self {
        Self {
            top: 48,
            right: 0,
            bottom: 34,
            left: 0,
        }
    }

    /// Android 全面屏手机横屏默认安全区域（左侧挖孔区 `48px`，底部手势横条 `24px`）。
    #[inline(always)]
    pub const fn android_landscape() -> Self {
        Self {
            top: 0,
            right: 0,
            bottom: 24,
            left: 48,
        }
    }

    /// iOS (iPhone 灵动岛/全面屏) 竖屏默认安全区域（顶部灵动岛 `59pt`，底部 Home Indicator `34pt`）。
    #[inline(always)]
    pub const fn ios_portrait() -> Self {
        Self {
            top: 59,
            right: 0,
            bottom: 34,
            left: 0,
        }
    }

    /// iOS (iPhone 灵动岛/全面屏) 横屏默认安全区域（左右安全区 `59pt`，底部 Home Indicator `21pt`）。
    #[inline(always)]
    pub const fn ios_landscape() -> Self {
        Self {
            top: 0,
            right: 59,
            bottom: 21,
            left: 59,
        }
    }

    /// 鸿蒙 (HarmonyOS NEXT / OpenHarmony) 手机竖屏默认避让区域（顶部状态栏 `38vp`，底部导航条 `28vp`）。
    #[inline(always)]
    pub const fn harmony_portrait() -> Self {
        Self {
            top: 38,
            right: 0,
            bottom: 28,
            left: 0,
        }
    }

    /// 鸿蒙 (HarmonyOS NEXT / OpenHarmony) 手机横屏默认避让区域（左右避让区 `38vp`，底部导航条 `20vp`）。
    #[inline(always)]
    pub const fn harmony_landscape() -> Self {
        Self {
            top: 0,
            right: 38,
            bottom: 20,
            left: 38,
        }
    }
}

/// 零堆内联 IME 组合输入预编辑（Preedit）负载（支持最多 64 字节 UTF-8 组合串与选区光标区间）。
///
/// 100% 兼容 `#![no_std]` 与 `Copy`，用于中文拼音、日文假名、韩文等输入法实时预编辑展示。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ImePreedit {
    pub buf: [u8; 64],
    pub len: u8,
    pub cursor_start: u8,
    pub cursor_end: u8,
    pub has_cursor: bool,
}

impl Default for ImePreedit {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl ImePreedit {
    /// 空预编辑状态（表示当前未处于组合输入中）。
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            buf: [0; 64],
            len: 0,
            cursor_start: 0,
            cursor_end: 0,
            has_cursor: false,
        }
    }

    /// 从 UTF-8 字符串与可选字节级光标选区构造内联预编辑状态（超长时按 UTF-8 字符边界安全截断）。
    pub fn from_str(text: &str, cursor: Option<(usize, usize)>) -> Self {
        let mut out = Self::empty();
        let bytes = text.as_bytes();
        let mut copy_len = bytes.len().min(64);
        while copy_len > 0 && !text.is_char_boundary(copy_len) {
            copy_len -= 1;
        }
        out.buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
        out.len = copy_len as u8;
        if let Some((s, e)) = cursor {
            out.cursor_start = s.min(copy_len) as u8;
            out.cursor_end = e.min(copy_len) as u8;
            out.has_cursor = true;
        }
        out
    }

    /// 返回当前预编辑 UTF-8 字符串切片。
    #[inline]
    pub fn as_str(&self) -> &str {
        let n = (self.len as usize).min(64);
        core::str::from_utf8(&self.buf[..n]).unwrap_or("")
    }

    /// 当前是否无正在组合的预编辑文本。
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 返回预编辑串内的可选光标/高亮字节区间 `(start, end)`。
    #[inline(always)]
    pub const fn cursor(&self) -> Option<(usize, usize)> {
        if self.has_cursor {
            Some((self.cursor_start as usize, self.cursor_end as usize))
        } else {
            None
        }
    }
}

/// 统一输入事件枚举（支持完整触摸手势生命周期、滚轮、物理按键焦点导航、原生 IME 组合输入、移动端安全区与系统返回）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum InputEvent {
    /// 指针/手指按下。
    PointerDown { x: i16, y: i16 },
    /// 指针/手指移动或拖拽。
    PointerMove { x: i16, y: i16 },
    /// 指针/手指抬起。
    PointerUp { x: i16, y: i16 },
    /// 鼠标右键（次键）按下。
    PointerSecondaryDown { x: i16, y: i16 },
    /// 鼠标右键（次键）抬起。
    PointerSecondaryUp { x: i16, y: i16 },
    /// 指针移出当前窗口/表面区域（清除悬停状态）。
    PointerLeave,
    /// 指针/手势取消。
    PointerCancel,
    /// 更新当前指针输入设备来源类型（区分桌面 `Mouse` / `Pen` 与触控屏 `Touch`）。
    PointerKindChanged(PointerKind),
    /// 鼠标滚轮或编码器滚动。
    Scroll {
        x: i16,
        y: i16,
        delta_y: i16,
    },
    /// 水平滚轮滚动。
    ScrollX {
        x: i16,
        y: i16,
        delta_x: i16,
    },
    /// 物理按键或旋钮按键按下。
    KeyDown(KeyCode),
    /// 物理按键释放。
    KeyUp(KeyCode),
    /// 键盘修饰键状态变更（Ctrl / Alt / Shift / Super）。
    ModifiersChanged(KeyModifiers),
    /// 退格删除一个字符（面向处于聚焦态的文本输入框）。
    Backspace,
    /// 删除光标后一个字符（`Delete` 键）。
    Delete,
    /// 输入法（IME）提交单个字符。
    TextCommit(char),
    /// 系统输入法（IME）会话激活。
    ImeEnabled,
    /// 系统输入法（IME）会话关闭/失焦（清空预编辑态）。
    ImeDisabled,
    /// 系统输入法（IME）组合输入预编辑文本更新（如拼音 `"zhongwen"` 及选区光标）。
    ImePreedit(ImePreedit),
    /// 系统级返回导航请求（Android 返回键 / 预测性返回手势 / iOS 左边缘右滑返回 / Web 浏览器 `popstate`）。
    NavigateBack,
    /// 移动端异形屏安全区域发生变更（如屏幕旋转、状态栏显隐切换）。
    SafeAreaChanged(SafeAreaInsets),
    /// 移动端屏幕软键盘弹出高度发生变更（`0` 表示软键盘收起）。
    KeyboardInsetChanged(i16),
}

