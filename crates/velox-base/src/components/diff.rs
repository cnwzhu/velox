//! 只读代码补丁与变更预览组件 `Diff`（无样式基础行为定义）。

use velox_engine::prelude::*;

/// 代码差异行类型枚举（`DiffLineKind`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DiffLineKind {
    #[default]
    Context,
    Added,
    Removed,
    HunkHeader,
}

/// 代码差异视图布局模式（`DiffViewMode`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DiffViewMode {
    #[default]
    Unified,
    Split,
}

/// 单行代码差异描述（`DiffLine`）。
#[derive(Clone, Copy, Debug)]
pub struct DiffLine<'a> {
    pub kind: DiffLineKind,
    pub content: &'a str,
}

impl<'a> DiffLine<'a> {
    /// 创建一行代码差异条目。
    #[inline(always)]
    pub const fn new(kind: DiffLineKind, content: &'a str) -> Self {
        Self { kind, content }
    }
}

/// 只读代码补丁与变更预览组件（`Diff`，支持直接解析 Unified Patch 文本或传入结构化 `DiffLine` 切片）。
pub struct Diff<'a> {
    pub id: u32,
    pub title: &'a str,
    pub patch: &'a str,
    pub lines: &'a [DiffLine<'a>],
    pub mode: DiffViewMode,
    pub width: i16,
    pub bg: Color,
    pub header_bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub added_bg: Color,
    pub added_fg: Color,
    pub removed_bg: Color,
    pub removed_fg: Color,
    pub hunk_bg: Color,
    pub hunk_fg: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> Diff<'a> {
    /// 使用 Unified Diff 补丁文本（包含 `+`/`-`/`@@` 前缀行）创建 `Diff` 视图（未显式设置配色时回退 `Theme` 样式）。
    #[inline(always)]
    pub const fn from_patch(id: u32, title: &'a str, patch: &'a str) -> Self {
        Self {
            id,
            title,
            patch,
            lines: &[],
            mode: DiffViewMode::Unified,
            width: 420,
            bg: Color::TRANSPARENT,
            header_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            added_bg: Color::TRANSPARENT,
            added_fg: Color::TRANSPARENT,
            removed_bg: Color::TRANSPARENT,
            removed_fg: Color::TRANSPARENT,
            hunk_bg: Color::TRANSPARENT,
            hunk_fg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 使用结构化 [`DiffLine`] 切片创建 `Diff` 视图（未显式设置配色时回退 `Theme` 样式）。
    #[inline(always)]
    pub const fn new(id: u32, title: &'a str, lines: &'a [DiffLine<'a>]) -> Self {
        Self {
            id,
            title,
            patch: "",
            lines,
            mode: DiffViewMode::Unified,
            width: 420,
            bg: Color::TRANSPARENT,
            header_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            added_bg: Color::TRANSPARENT,
            added_fg: Color::TRANSPARENT,
            removed_bg: Color::TRANSPARENT,
            removed_fg: Color::TRANSPARENT,
            hunk_bg: Color::TRANSPARENT,
            hunk_fg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置视图模式（`Unified` 或 `Split`）。
    #[inline(always)]
    pub const fn mode(mut self, mode: DiffViewMode) -> Self {
        self.mode = mode;
        self
    }

    /// 设置组件宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    fn classify_line(raw: &'a str) -> (DiffLineKind, &'a str) {
        if raw.starts_with("@@") {
            (DiffLineKind::HunkHeader, raw)
        } else if raw.starts_with('+') && !raw.starts_with("+++") {
            (DiffLineKind::Added, raw)
        } else if raw.starts_with('-') && !raw.starts_with("---") {
            (DiffLineKind::Removed, raw)
        } else {
            (DiffLineKind::Context, raw)
        }
    }

    fn render_diff_row(&self, cx: &'a FrameScope, kind: DiffLineKind, content: &'a str) -> Div<'a> {
        let (bg, prefix, fg) = match kind {
            DiffLineKind::Added => (self.added_bg, "+", self.added_fg),
            DiffLineKind::Removed => (self.removed_bg, "-", self.removed_fg),
            DiffLineKind::HunkHeader => (self.hunk_bg, "@", self.hunk_fg),
            DiffLineKind::Context => (self.bg, " ", self.text_color),
        };

        div()
            .flex_row()
            .w(self.width)
            .h(20)
            .px(8)
            .gap(6)
            .bg(bg)
            .items_center()
            .child(cx, text(prefix).size(11).color(fg))
            .child(cx, text(content).size(11).color(fg).ellipsis())
    }
}

crate::impl_base_styled_box!(Diff<'a>);

impl<'a> Widget<'a> for Diff<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        self.width = StyleExt::resolve_width(self.style, self.width);
        let fg = cx.theme().foreground;
        if self.text_color.a() == 0 {
            self.text_color = fg;
        }
        if self.added_fg.a() == 0 {
            self.added_fg = fg;
        }
        if self.removed_fg.a() == 0 {
            self.removed_fg = fg;
        }
        if self.hunk_fg.a() == 0 {
            self.hunk_fg = fg;
        }

        let mut root = div()
            .id(self.id)
            .flex_col()
            .w(self.width)
            .rounded(self.rounded)
            .bg(self.bg)
            .overflow_hidden();
        if self.border_color.a() > 0 {
            root = root.border(1, self.border_color);
        }

        if !self.title.is_empty() {
            root = root.child(
                cx,
                div()
                    .w(self.width)
                    .h(28)
                    .px(10)
                    .bg(self.header_bg)
                    .items_center()
                    .child(cx, text(self.title).size(12).color(self.text_color)),
            );
        }

        if !self.lines.is_empty() {
            for l in self.lines {
                root = root.child(cx, self.render_diff_row(cx, l.kind, l.content));
            }
        } else {
            for raw in self.patch.split('\n') {
                let line = raw.strip_suffix('\r').unwrap_or(raw);
                let (kind, txt) = Self::classify_line(line);
                root = root.child(cx, self.render_diff_row(cx, kind, txt));
            }
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
