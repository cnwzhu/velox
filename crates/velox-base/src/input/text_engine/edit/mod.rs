//! 统一文本编辑动作协议（[`Action`]）、语义选区模式（[`EditSelection`]）、
//! 原子双向变更事务（[`Change`] / [`ChangeItem`]）与编辑器抽象特征（[`Edit`]）。
//!
//! 参照 `pop-os/cosmic-text/src/edit/mod.rs` 设计，完整兼容 `#![no_std]`。

pub mod editor;
pub mod syntax;

use velox_engine::arena::{SmallString, SmallVec};

use super::attrs::AttrsList;
use super::buffer::TextDocBuffer;
use super::cursor::{Cursor, Motion};

pub use editor::CosmicEditor;
pub use syntax::{
    PlainSyntaxHighlighter, RustSyntaxHighlighter, SyntaxEditor, SyntaxHighlighter, SyntaxTheme,
};

/// 编辑器标准交互动作枚举（对应 `cosmic-text` `Action`）。
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Action {
    /// 执行光标移动导航（自动遵循当前选区或折叠选区）。
    Motion(Motion),
    /// 取消当前选区（`Escape`）。
    Escape,
    /// 在当前位置插入单个字符（若有选区则先替换选区）。
    Insert(char),
    /// 插入换行符并按需执行自动缩进（`Enter`）。
    Enter,
    /// 向前删除字符或选区（`Backspace`）。
    Backspace,
    /// 向后删除字符或选区（`Delete`）。
    Delete,
    /// 缩进当前行或选区覆盖的全部逻辑行（`Tab`）。
    Indent,
    /// 反缩进当前行或选区覆盖的全部逻辑行（`Shift+Tab`）。
    Unindent,
    /// 在内容区局部像素坐标 `(x, y)` 处单击定位光标。
    Click { x: i32, y: i32 },
    /// 在内容区局部像素坐标 `(x, y)` 处双击选中整词。
    DoubleClick { x: i32, y: i32 },
    /// 在内容区局部像素坐标 `(x, y)` 处三击选中整行。
    TripleClick { x: i32, y: i32 },
    /// 拖拽至内容区局部像素坐标 `(x, y)` 处按当前选区模式扩展选区。
    Drag { x: i32, y: i32 },
    /// 垂直滚动指定视觉行数（正值向下看后文，负值向上看前文）。
    Scroll { lines: i32 },
    /// 垂直滚动指定像素增量（正值向下，负值向上）。
    ScrollPixels { pixels: f32 },
}

/// 编辑器语义选区模式（对应 `cosmic-text` `Selection`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum EditSelection {
    /// 无活动选区（折叠光标）。
    #[default]
    None,
    /// 普通字符级选区：以给定锚点 `Cursor` 到当前活动 `cursor` 为界。
    Normal(Cursor),
    /// 整行级选区（三击触发）：拖拽时以完整逻辑行为粒度扩展。
    Line(Cursor),
    /// 整词级选区（双击触发）：拖拽时以完整单词边界为粒度扩展。
    Word(Cursor),
}

/// 单条原子文本变更条目（对应 `cosmic-text` `ChangeItem`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeItem {
    /// 变更起始逻辑光标（总是满足 `start <= end`）。
    pub start: Cursor,
    /// 变更结束逻辑光标。
    pub end: Cursor,
    /// 插入或被删除的完整 UTF-8 文本（含行尾换行符）。
    pub text: SmallString<128>,
    /// `true` 表示插入操作；`false` 表示删除操作。
    pub insert: bool,
}

/// 一次可双向撤销/重做的原子编辑事务（对应 `cosmic-text` `Change`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub now: SmallVec<ChangeItem, 6>,
}

impl Change {
    #[inline]
    pub const fn new() -> Self {
        Self {
            now: SmallVec::new(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.now.is_empty()
    }

    /// 原地反转该事务中所有原子变更的顺序与插入/删除方向（`undo` 即 `reverse()` 后 `apply_change()`）。
    pub fn reverse(&mut self) {
        self.now.as_mut_slice().reverse();
        for item in self.now.as_mut_slice() {
            item.insert = !item.insert;
        }
    }

    /// 返回该事务反转后的副本（不修改原事务）。
    pub fn reversed(&self) -> Self {
        let mut cloned = self.clone();
        cloned.reverse();
        cloned
    }
}

/// 统一文本编辑器抽象特征（对应 `cosmic-text` `Edit` trait）。
///
/// 使得 [`CosmicEditor`]、增量语法高亮装饰器 [`SyntaxEditor`] 以及未来可扩展的 Vi/Emacs 模态包装器
/// 均可无缝复用同一套 `CodeEditor` 渲染组件与事件派发契约。
pub trait Edit {
    /// 获取底层多行排版缓冲区的只读引用。
    fn buffer_ref(&self) -> &TextDocBuffer;

    /// 获取底层多行排版缓冲区的可变引用。
    fn buffer_mut(&mut self) -> &mut TextDocBuffer;

    /// 获取当前活动光标位置。
    fn cursor(&self) -> Cursor;

    /// 设置当前活动光标位置。
    fn set_cursor(&mut self, cursor: Cursor);

    /// 获取当前语义选区模式。
    fn selection(&self) -> EditSelection;

    /// 设置当前语义选区模式。
    fn set_selection(&mut self, selection: EditSelection);

    /// 获取规范有序的选区起止边界 `(start, end)`（若无活动选区则返回 `None`）。
    fn selection_bounds(&self) -> Option<(Cursor, Cursor)>;

    /// 是否启用回车自动缩进。
    fn auto_indent(&self) -> bool;

    /// 设置是否启用回车自动缩进。
    fn set_auto_indent(&mut self, auto_indent: bool);

    /// 获取制表符宽度（列数）。
    fn tab_width(&self) -> u16;

    /// 设置制表符宽度（列数）。
    fn set_tab_width(&mut self, tab_width: u16);

    /// 按需执行增量语法高亮与视口内可见行排版。
    fn shape_as_needed(&mut self, prune: bool);

    /// 删除指定逻辑区间 `[start, end)` 内的文本（并记录到活跃 [`Change`] 事务中）。
    fn delete_range(&mut self, start: Cursor, end: Cursor);

    /// 在指定逻辑光标 `cursor` 处插入字符串（可跨多行），返回插入结束后的新光标位置。
    fn insert_at(
        &mut self,
        cursor: Cursor,
        data: &str,
        attrs_list: Option<AttrsList>,
    ) -> Cursor;

    /// 复制当前选区文本（无选区返回 `None`）。
    fn copy_selection(&self) -> Option<SmallString<1024>>;

    /// 删除当前活动选区文本（若存在选区并成功删除返回 `true`）。
    fn delete_selection(&mut self) -> bool;

    /// 应用一条双向变更事务 [`Change`]（供撤销/重做与外部协同补丁使用）。
    fn apply_change(&mut self, change: &Change) -> bool;

    /// 开启一条变更记录事务。
    fn start_change(&mut self);

    /// 结束并取出当前变更记录事务（空事务返回 `None`）。
    fn finish_change(&mut self) -> Option<Change>;

    /// 执行一条标准编辑器动作 [`Action`]。
    fn action(&mut self, action: Action);

    /// 查询当前光标相对内容视口左上角的整数像素坐标 `(x, y)`。
    fn cursor_position(&mut self) -> Option<(i32, i32)>;

    /// 在当前光标或选区位置插入字符串（若存在选区则先删除选区再插入，自动作为单条可撤销事务）。
    fn insert_string(&mut self, data: &str, attrs_list: Option<AttrsList>);

    /// 全选整个文档。
    fn select_all(&mut self);
}
