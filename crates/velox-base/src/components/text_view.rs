//! Markdown 与 HTML 富文本渲染视图基础组件 `TextView`、只读可选中文本 `SelectableText` 与轻量级富文本预览 `MarkdownView`（默认无样式）。

use velox_core::prelude::*;

/// 富文本解析格式枚举（`TextViewFormat`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextViewFormat {
    #[default]
    Markdown,
    Html,
    Plain,
}

/// 支持范围选区高亮的只读可选中文本基础组件（`SelectableText`，默认无样式）。
pub struct SelectableText<'a> {
    pub id: u32,
    pub content: &'a str,
    pub selection: Option<(usize, usize)>,
    pub font_size: u8,
    pub text_color: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> SelectableText<'a> {
    /// 创建一个默认无样式的只读可选中文本组件。
    #[inline(always)]
    pub const fn new(id: u32, content: &'a str) -> Self {
        Self {
            id,
            content,
            selection: None,
            font_size: 13,
            text_color: Color::TRANSPARENT,
            selection_bg: Color::TRANSPARENT,
            selection_fg: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置选中的字节范围 `(start, end)`。
    #[inline(always)]
    pub const fn selection(mut self, selection: Option<(usize, usize)>) -> Self {
        self.selection = selection;
        self
    }
}

crate::impl_base_styled_box!(SelectableText<'a>);

impl<'a> Widget<'a> for SelectableText<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let selection_bg = if self.selection_bg.a() > 0 {
            self.selection_bg
        } else {
            fg.scale_alpha(64)
        };
        let selection_fg = if self.selection_fg.a() > 0 {
            self.selection_fg
        } else {
            fg
        };

        let mut row = div().id(self.id).flex_row().items_center().focusable();
        if !self.focus_ring {
            row = row.no_focus_border();
        }

        if let Some((s, e)) = self.selection {
            let start = s.min(self.content.len());
            let end = e.min(self.content.len());
            if start < end
                && self.content.is_char_boundary(start)
                && self.content.is_char_boundary(end)
            {
                let prefix = &self.content[..start];
                let mid = &self.content[start..end];
                let suffix = &self.content[end..];
                if !prefix.is_empty() {
                    row = row.child(cx, text(prefix).size(self.font_size).color(text_color));
                }
                row = row.child(
                    cx,
                    div()
                        .bg(selection_bg)
                        .rounded(self.rounded)
                        .child(cx, text(mid).size(self.font_size).color(selection_fg)),
                );
                if !suffix.is_empty() {
                    row = row.child(cx, text(suffix).size(self.font_size).color(text_color));
                }
                return self.style_ext.apply_to_div(row, self.style).into_element(cx);
            }
        }

        row = row.child(
            cx,
            text(self.content).size(self.font_size).color(text_color),
        );
        self.style_ext.apply_to_div(row, self.style).into_element(cx)
    }
}

/// 轻量级 Markdown 文档渲染视图基础组件（`MarkdownView`，默认无样式，支持 `#`/`##`/`###` 标题、`- ` 列表、`> ` 引用与代码块渲染）。
pub struct MarkdownView<'a> {
    pub id: u32,
    pub source: &'a str,
    pub width: i16,
    pub h1_color: Color,
    pub h2_color: Color,
    pub h3_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub accent_color: Color,
    pub code_bg: Color,
    pub code_fg: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> MarkdownView<'a> {
    /// 创建一个默认无样式的 Markdown 视图组件。
    #[inline(always)]
    pub const fn new(id: u32, source: &'a str) -> Self {
        Self {
            id,
            source,
            width: 360,
            h1_color: Color::TRANSPARENT,
            h2_color: Color::TRANSPARENT,
            h3_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            accent_color: Color::TRANSPARENT,
            code_bg: Color::TRANSPARENT,
            code_fg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 设置容器宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }
}

crate::impl_base_styled_box!(MarkdownView<'a>);

impl<'a> Widget<'a> for MarkdownView<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let h1_color = if self.h1_color.a() > 0 { self.h1_color } else { fg };
        let h2_color = if self.h2_color.a() > 0 { self.h2_color } else { fg };
        let h3_color = if self.h3_color.a() > 0 { self.h3_color } else { fg };
        let text_color = if self.text_color.a() > 0 { self.text_color } else { fg };
        let muted_color = if self.muted_color.a() > 0 {
            self.muted_color
        } else {
            fg.scale_alpha(150)
        };
        let accent_color = if self.accent_color.a() > 0 {
            self.accent_color
        } else {
            fg.scale_alpha(96)
        };
        let code_fg = if self.code_fg.a() > 0 { self.code_fg } else { fg };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut root = div().id(self.id).flex_col().w(width).gap(4);
        let mut in_code_block = false;

        for raw_line in self.source.split('\n') {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }
            if in_code_block {
                root = root.child(
                    cx,
                    div()
                        .w(width)
                        .px(8)
                        .py(2)
                        .rounded(self.rounded)
                        .bg(self.code_bg)
                        .child(cx, text(line).size(12).color(code_fg)),
                );
            } else if let Some(h1) = line.strip_prefix("# ") {
                root = root.child(cx, text(h1).size(18).color(h1_color));
            } else if let Some(h2) = line.strip_prefix("## ") {
                root = root.child(cx, text(h2).size(15).color(h2_color));
            } else if let Some(h3) = line.strip_prefix("### ") {
                root = root.child(cx, text(h3).size(14).color(h3_color));
            } else if let Some(quote) = line.strip_prefix("> ") {
                root = root.child(
                    cx,
                    div()
                        .pl(8)
                        .border(1, accent_color)
                        .child(cx, text(quote).size(12).color(muted_color)),
                );
            } else if let Some(bullet) = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
            {
                root = root.child(
                    cx,
                    div()
                        .flex_row()
                        .gap(6)
                        .items_center()
                        .child(cx, text("•").size(12).color(accent_color))
                        .child(cx, text(bullet).size(12).color(text_color)),
                );
            } else if !line.is_empty() {
                root = root.child(cx, text(line).size(12).color(text_color));
            }
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}

/// 支持 Markdown 与轻量级 HTML 渲染的富文本视图基础组件（`TextView`，默认无样式）。
pub struct TextView<'a> {
    pub id: u32,
    pub source: &'a str,
    pub format: TextViewFormat,
    pub width: i16,
    pub h1_color: Color,
    pub h2_color: Color,
    pub h3_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub accent_color: Color,
    pub code_bg: Color,
    pub code_fg: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
}

impl<'a> TextView<'a> {
    /// 创建一个默认无样式 Markdown 模式的 `TextView` 组件。
    #[inline(always)]
    pub const fn new(id: u32, source: &'a str) -> Self {
        Self {
            id,
            source,
            format: TextViewFormat::Markdown,
            width: 360,
            h1_color: Color::TRANSPARENT,
            h2_color: Color::TRANSPARENT,
            h3_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            accent_color: Color::TRANSPARENT,
            code_bg: Color::TRANSPARENT,
            code_fg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
        }
    }

    /// 创建一个 HTML 渲染模式的 `TextView` 组件。
    #[inline(always)]
    pub const fn html(id: u32, source: &'a str) -> Self {
        Self::new(id, source).format(TextViewFormat::Html)
    }

    /// 设置富文本渲染格式。
    #[inline(always)]
    pub const fn format(mut self, format: TextViewFormat) -> Self {
        self.format = format;
        self
    }

    /// 设置容器宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    fn strip_html_tags<'b>(&self, line: &'b str) -> (u8, Color, &'b str) {
        let trimmed = line.trim();
        if let Some(inner) = trimmed.strip_prefix("<h1>").and_then(|s| s.strip_suffix("</h1>")) {
            return (18, self.h1_color, inner);
        }
        if let Some(inner) = trimmed.strip_prefix("<h2>").and_then(|s| s.strip_suffix("</h2>")) {
            return (15, self.h2_color, inner);
        }
        if let Some(inner) = trimmed.strip_prefix("<h3>").and_then(|s| s.strip_suffix("</h3>")) {
            return (14, self.h3_color, inner);
        }
        if let Some(inner) = trimmed
            .strip_prefix("<code>")
            .and_then(|s| s.strip_suffix("</code>"))
        {
            return (12, self.code_fg, inner);
        }
        if let Some(inner) = trimmed.strip_prefix("<p>").and_then(|s| s.strip_suffix("</p>")) {
            return (12, self.text_color, inner);
        }
        if let Some(inner) = trimmed.strip_prefix("<li>").and_then(|s| s.strip_suffix("</li>")) {
            return (12, self.text_color, inner);
        }
        (12, self.text_color, trimmed)
    }
}

crate::impl_base_styled_box!(TextView<'a>);

impl<'a> Widget<'a> for TextView<'a> {
    fn render_once(mut self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        if self.h1_color.a() == 0 {
            self.h1_color = fg;
        }
        if self.h2_color.a() == 0 {
            self.h2_color = fg;
        }
        if self.h3_color.a() == 0 {
            self.h3_color = fg;
        }
        if self.text_color.a() == 0 {
            self.text_color = fg;
        }
        if self.muted_color.a() == 0 {
            self.muted_color = fg.scale_alpha(150);
        }
        if self.accent_color.a() == 0 {
            self.accent_color = fg.scale_alpha(96);
        }
        if self.code_fg.a() == 0 {
            self.code_fg = fg;
        }

        let width = StyleExt::resolve_width(self.style, self.width);
        match self.format {
            TextViewFormat::Markdown => {
                let mut md = MarkdownView::new(self.id, self.source).width(width);
                md.h1_color = self.h1_color;
                md.h2_color = self.h2_color;
                md.h3_color = self.h3_color;
                md.text_color = self.text_color;
                md.muted_color = self.muted_color;
                md.accent_color = self.accent_color;
                md.code_bg = self.code_bg;
                md.code_fg = self.code_fg;
                md.rounded = self.rounded;
                md.style = self.style;
                md.style_ext = self.style_ext;
                md.render_once(cx)
            }
            TextViewFormat::Plain => {
                let mut root = div().id(self.id).flex_col().w(width).gap(4);
                for raw_line in self.source.split('\n') {
                    let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
                    root = root.child(cx, text(line).size(12).color(self.text_color));
                }
                self.style_ext.apply_to_div(root, self.style).into_element(cx)
            }
            TextViewFormat::Html => {
                let mut root = div().id(self.id).flex_col().w(width).gap(4);
                for raw_line in self.source.split('\n') {
                    let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
                    if line.trim().is_empty() {
                        continue;
                    }
                    let (fsize, col, content) = self.strip_html_tags(line);
                    root = root.child(cx, text(content).size(fsize).color(col));
                }
                self.style_ext.apply_to_div(root, self.style).into_element(cx)
            }
        }
    }
}
