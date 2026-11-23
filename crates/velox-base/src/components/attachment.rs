//! 文件与多媒体附件表面组件 `Attachment`。

use velox_engine::prelude::*;

/// 附件媒体类型枚举（`AttachmentKind`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AttachmentKind {
    #[default]
    File,
    Image,
    Audio,
    Video,
    Archive,
    Code,
}

impl AttachmentKind {
    /// 获取附件类型对应的短标签。
    #[inline(always)]
    pub const fn badge_str(self) -> &'static str {
        match self {
            Self::File => "FILE",
            Self::Image => "IMG",
            Self::Audio => "AUD",
            Self::Video => "VID",
            Self::Archive => "ZIP",
            Self::Code => "CODE",
        }
    }
}

/// 文件与多媒体附件卡片基础组件（`Attachment`，默认无样式）。
#[derive(Clone, Copy)]
pub struct Attachment<'a> {
    pub id: u32,
    pub name: &'a str,
    pub meta: &'a str,
    pub kind: AttachmentKind,
    pub progress: Option<u8>,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub badge_bg: Color,
    pub badge_border_color: Color,
    pub badge_text_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub track_bg: Color,
    pub accent_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_remove: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_click: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Attachment<'a> {
    /// 创建一个默认无样式的文件或媒体附件表面组件。
    #[inline(always)]
    pub const fn new(id: u32, name: &'a str, meta: &'a str) -> Self {
        Self {
            id,
            name,
            meta,
            kind: AttachmentKind::File,
            progress: None,
            width: 220,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            badge_bg: Color::TRANSPARENT,
            badge_border_color: Color::TRANSPARENT,
            badge_text_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            track_bg: Color::TRANSPARENT,
            accent_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_remove: None,
            on_click: None,
        }
    }

    /// 设置附件媒体类型。
    #[inline(always)]
    pub const fn kind(mut self, kind: AttachmentKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置上传/下载进度百分比（`0..=100`）。
    #[inline(always)]
    pub const fn progress(mut self, percent: u8) -> Self {
        self.progress = Some(if percent > 100 { 100 } else { percent });
        self
    }

    /// 设置附件卡片宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定移除附件按钮回调。
    #[inline(always)]
    pub fn on_remove(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_remove = Some(handler);
        self
    }

    /// 绑定点击附件卡片回调。
    #[inline(always)]
    pub fn on_click(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Attachment<'a>);

impl<'a> Widget<'a> for Attachment<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let badge_text_color = if self.badge_text_color.a() > 0 {
            self.badge_text_color
        } else {
            fg
        };
        let text_color = if self.text_color.a() > 0 {
            self.text_color
        } else {
            fg
        };
        let muted_color = if self.muted_color.a() > 0 {
            self.muted_color
        } else {
            fg.scale_alpha(150)
        };
        let track_bg = if self.track_bg.a() > 0 {
            self.track_bg
        } else {
            fg.scale_alpha(48)
        };
        let accent_color = if self.accent_color.a() > 0 {
            self.accent_color
        } else {
            fg
        };

        let mut icon_box = div()
            .w(32)
            .h(32)
            .rounded(self.rounded.saturating_sub(2))
            .bg(self.badge_bg)
            .items_center()
            .justify_center()
            .child(
                cx,
                text(self.kind.badge_str())
                    .size(10)
                    .color(badge_text_color),
            );
        if self.badge_border_color.a() > 0 {
            icon_box = icon_box.border(1, self.badge_border_color);
        }

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut info_col = div()
            .flex_col()
            .w((width - 76).max(60))
            .gap(2)
            .child(
                cx,
                text(self.name)
                    .size(12)
                    .color(text_color)
                    .ellipsis(),
            )
            .child(
                cx,
                text(self.meta)
                    .size(10)
                    .color(muted_color)
                    .ellipsis(),
            );

        if let Some(pct) = self.progress {
            let bar_w = (width - 80).max(40);
            let fill_w = ((bar_w as i32 * pct as i32) / 100) as i16;
            let bar_r = if self.rounded > 0 { 2 } else { 0 };
            info_col = info_col.child(
                cx,
                div()
                    .w(bar_w)
                    .h(4)
                    .rounded(bar_r)
                    .bg(track_bg)
                    .overflow_hidden()
                    .child(
                        cx,
                        div()
                            .w(fill_w)
                            .h(4)
                            .rounded(bar_r)
                            .bg(accent_color),
                    ),
            );
        }

        let mut card = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .p(8)
            .gap(8)
            .rounded(self.rounded)
            .bg(self.bg)
            .items_center()
            .focusable();
        if self.border_color.a() > 0 {
            card = card.border(1, self.border_color);
        }
        if !self.focus_ring {
            card = card.no_focus_border();
        }
        card = card.child(cx, icon_box).child(cx, info_col);

        if let Some(cb) = self.on_click {
            card = card.on_click_raw(cb);
        }

        if let Some(rm_cb) = self.on_remove {
            let mut rm_btn = div()
                .id(crate::scoped_child_id(self.id, 0xA5CB_7C68, 1))
                .w(20)
                .h(20)
                .rounded(self.rounded.saturating_sub(4))
                .items_center()
                .justify_center()
                .focusable()
                .on_click_raw(rm_cb)
                .child(cx, text("×").size(12).color(muted_color));
            if !self.focus_ring {
                rm_btn = rm_btn.no_focus_border();
            }
            card = card.child(cx, rm_btn);
        }

        card = self.style_ext.apply_to_div(card, self.style);
        card.into_element(cx)
    }
}
