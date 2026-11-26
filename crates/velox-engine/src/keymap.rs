//! 作用域感知快捷键与命令路由系统（`Keymap` / `KeyStroke` / `KeyChord` / `ActionId`，100% `#![no_std]` 零堆）。
//!
//! 对标 GPUI / VS Code 的声明式键盘命令派发模型：
//! - 支持单键组合（如 `Ctrl+S`、`Ctrl+Shift+P`）与两段弦键（如 `Ctrl+K, Ctrl+S`）；
//! - 支持按焦点上下文栈（如 `&["Editor", "Workspace", "Global"]`）自内向外精准匹配命令；
//! - 弦键等待态内置自动超时复位，避免误按前缀键卡死后续输入。

use crate::arena::SmallVec;
use crate::pod::{KeyCode, KeyModifiers};

/// 强类型业务命令/动作 ID（例如 `const SAVE: ActionId = ActionId(1);`）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct ActionId(pub u32);

/// 单个按键组合（键值 + 修饰键位掩码）。
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct KeyStroke {
    pub key: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyStroke {
    #[inline(always)]
    pub const fn new(key: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }

    #[inline(always)]
    pub const fn plain(key: KeyCode) -> Self {
        Self::new(key, KeyModifiers::NONE)
    }

    #[inline(always)]
    pub const fn ctrl(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::CTRL)
    }

    #[inline(always)]
    pub const fn ctrl_shift(ch: char) -> Self {
        Self::new(
            KeyCode::Char(ch),
            KeyModifiers(KeyModifiers::CTRL.0 | KeyModifiers::SHIFT.0),
        )
    }

    #[inline(always)]
    pub const fn alt(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::ALT)
    }
}

/// 单键或两段弦键快捷键序列（如 `Ctrl+K` 后接 `Ctrl+S`）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct KeyChord {
    pub first: KeyStroke,
    pub second: Option<KeyStroke>,
}

impl KeyChord {
    #[inline(always)]
    pub const fn single(stroke: KeyStroke) -> Self {
        Self {
            first: stroke,
            second: None,
        }
    }

    #[inline(always)]
    pub const fn pair(first: KeyStroke, second: KeyStroke) -> Self {
        Self {
            first,
            second: Some(second),
        }
    }
}

/// 单条作用域快捷键绑定记录（`context == None` 表示全局兜底生效）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct KeyBinding {
    pub context: Option<&'static str>,
    pub chord: KeyChord,
    pub action: ActionId,
}

impl KeyBinding {
    /// 创建全局生效的快捷键绑定。
    #[inline(always)]
    pub const fn global(chord: KeyChord, action: ActionId) -> Self {
        Self {
            context: None,
            chord,
            action,
        }
    }

    /// 创建限定在特定焦点上下文作用域（如 `"CodeEditor"` / `"Dialog"`）内的快捷键绑定。
    #[inline(always)]
    pub const fn scoped(context: &'static str, chord: KeyChord, action: ActionId) -> Self {
        Self {
            context: Some(context),
            chord,
            action,
        }
    }
}

/// 快捷键解析结果。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeymapMatch {
    /// 精确命中某条绑定命令。
    Matched(ActionId),
    /// 命中了某条两段弦键的前缀键，正等待第二键输入（调用方应吞掉本次事件避免误输入）。
    PendingChord,
    /// 未命中任何绑定。
    NoMatch,
}

/// 定长无堆作用域快捷键路由表与弦键状态机（默认最多 64 条绑定，支持 `#![no_std]`）。
#[derive(Clone, Debug)]
pub struct Keymap<const CAP: usize = 64> {
    bindings: SmallVec<KeyBinding, CAP>,
    pending_first: Option<KeyStroke>,
    pending_since_ms: u32,
    chord_timeout_ms: u32,
}

impl<const CAP: usize> Default for Keymap<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> Keymap<CAP> {
    #[inline]
    pub const fn new() -> Self {
        Self {
            bindings: SmallVec::new(),
            pending_first: None,
            pending_since_ms: 0,
            chord_timeout_ms: 1500,
        }
    }

    /// 设置弦键前缀等待超时毫秒数（默认 `1500ms`）。
    #[inline(always)]
    pub fn set_chord_timeout_ms(&mut self, timeout_ms: u32) {
        self.chord_timeout_ms = timeout_ms.max(100);
    }

    /// 注册一条快捷键绑定（后注册的同作用域绑定具有更高优先级，允许用户覆盖默认键位）。
    #[inline]
    pub fn bind(&mut self, binding: KeyBinding) -> bool {
        self.bindings.push(binding)
    }

    /// 清空全部绑定与待定弦键状态。
    #[inline]
    pub fn clear(&mut self) {
        self.bindings.clear();
        self.pending_first = None;
    }

    /// 复位进行中的弦键前缀状态。
    #[inline(always)]
    pub fn reset_pending(&mut self) {
        self.pending_first = None;
    }

    /// 返回当前是否正在等待两段弦键的第二个按键。
    #[inline(always)]
    pub const fn has_pending_chord(&self) -> bool {
        self.pending_first.is_some()
    }

    /// 归一化字符大小写（例如按下 `Ctrl+Shift+p` 与 `Ctrl+Shift+P` 视为同一快捷键）。
    #[inline]
    fn normalize_stroke(mut stroke: KeyStroke) -> KeyStroke {
        if let KeyCode::Char(c) = stroke.key {
            if c.is_ascii_uppercase() {
                stroke.key = KeyCode::Char(c.to_ascii_lowercase());
            }
        }
        stroke
    }

    fn find_in_context(
        &self,
        target_ctx: Option<&str>,
        first: KeyStroke,
        second: Option<KeyStroke>,
    ) -> Option<ActionId> {
        let nf = Self::normalize_stroke(first);
        let ns = second.map(Self::normalize_stroke);
        for b in self.bindings.as_slice().iter().rev() {
            if b.context != target_ctx {
                continue;
            }
            let bf = Self::normalize_stroke(b.chord.first);
            let bs = b.chord.second.map(Self::normalize_stroke);
            if bf == nf && bs == ns {
                return Some(b.action);
            }
        }
        None
    }

    fn has_chord_prefix_in_context(&self, target_ctx: Option<&str>, first: KeyStroke) -> bool {
        let nf = Self::normalize_stroke(first);
        for b in self.bindings.as_slice() {
            if b.context == target_ctx
                && b.chord.second.is_some()
                && Self::normalize_stroke(b.chord.first) == nf
            {
                return true;
            }
        }
        false
    }

    /// 按焦点作用域链（由最内层焦点控件向外排列，例如 `&["CodeEditor", "Workspace"]`）解析按键。
    pub fn resolve(
        &mut self,
        context_chain: &[&str],
        stroke: KeyStroke,
        now_ms: u32,
    ) -> KeymapMatch {
        if let Some(prev) = self.pending_first {
            if now_ms.wrapping_sub(self.pending_since_ms) > self.chord_timeout_ms {
                self.pending_first = None;
            } else {
                self.pending_first = None;
                for &ctx in context_chain {
                    if let Some(act) = self.find_in_context(Some(ctx), prev, Some(stroke)) {
                        return KeymapMatch::Matched(act);
                    }
                }
                if let Some(act) = self.find_in_context(None, prev, Some(stroke)) {
                    return KeymapMatch::Matched(act);
                }
            }
        }

        // 先检查是否命中某个两段弦键前缀（最内层作用域优先）
        for &ctx in context_chain {
            if self.has_chord_prefix_in_context(Some(ctx), stroke) {
                self.pending_first = Some(stroke);
                self.pending_since_ms = now_ms;
                return KeymapMatch::PendingChord;
            }
            if let Some(act) = self.find_in_context(Some(ctx), stroke, None) {
                return KeymapMatch::Matched(act);
            }
        }
        if self.has_chord_prefix_in_context(None, stroke) {
            self.pending_first = Some(stroke);
            self.pending_since_ms = now_ms;
            return KeymapMatch::PendingChord;
        }
        if let Some(act) = self.find_in_context(None, stroke, None) {
            return KeymapMatch::Matched(act);
        }
        KeymapMatch::NoMatch
    }
}
