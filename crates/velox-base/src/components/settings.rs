//! 结构化应用设置面板基础组件 `Settings`（默认无样式）。

use velox_core::prelude::*;

/// 设置项控件值类型枚举（`SettingsValue`）。
#[derive(Clone, Copy, Debug)]
pub enum SettingsValue<'a> {
    Bool(bool),
    Text(&'a str),
    Action(&'a str),
}

/// 单行设置项定义（`SettingsItem`）。
#[derive(Clone, Copy, Debug)]
pub struct SettingsItem<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub value: SettingsValue<'a>,
}

impl<'a> SettingsItem<'a> {
    /// 创建一个设置项。
    #[inline(always)]
    pub const fn new(
        title: &'a str,
        description: &'a str,
        value: SettingsValue<'a>,
    ) -> Self {
        Self {
            title,
            description,
            value,
        }
    }
}

/// 设置分组定义（`SettingsGroup`）。
#[derive(Clone, Copy, Debug)]
pub struct SettingsGroup<'a> {
    pub title: &'a str,
    pub items: &'a [SettingsItem<'a>],
}

impl<'a> SettingsGroup<'a> {
    /// 创建一个设置分组。
    #[inline(always)]
    pub const fn new(title: &'a str, items: &'a [SettingsItem<'a>]) -> Self {
        Self { title, items }
    }
}

/// 结构化应用设置面板基础组件（`Settings`，默认无样式）。
pub struct Settings<'a> {
    pub id: u32,
    pub groups: &'a [SettingsGroup<'a>],
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub control_bg: Color,
    pub active_bg: Color,
    pub active_fg: Color,
    pub switch_off_bg: Color,
    pub switch_thumb_bg: Color,
    pub switch_rounded: u8,
    pub switch_thumb_shadow: Option<(u8, i8, i8, Color)>,
    pub row_hover_bg: Option<Color>,
    pub row_active_bg: Option<Color>,
    pub action_hover_bg: Option<Color>,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_item_click: Option<&'a dyn Fn(usize, usize, &mut ActionScope)>,
}

impl<'a> Settings<'a> {
    /// 创建一个默认无样式的设置界面组件。
    #[inline(always)]
    pub const fn new(id: u32, groups: &'a [SettingsGroup<'a>]) -> Self {
        Self {
            id,
            groups,
            width: 380,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            control_bg: Color::TRANSPARENT,
            active_bg: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            switch_off_bg: Color::TRANSPARENT,
            switch_thumb_bg: Color::TRANSPARENT,
            switch_rounded: 0,
            switch_thumb_shadow: None,
            row_hover_bg: None,
            row_active_bg: None,
            action_hover_bg: None,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_item_click: None,
        }
    }

    /// 设置面板宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定点击或切换某个设置项 `(group_idx, item_idx)` 的回调。
    #[inline(always)]
    pub fn on_item_click(mut self, handler: &'a dyn Fn(usize, usize, &mut ActionScope)) -> Self {
        self.on_item_click = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Settings<'a>);

impl<'a> Widget<'a> for Settings<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
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
        let active_fg = if self.active_fg.a() > 0 {
            self.active_fg
        } else {
            fg
        };
        let thumb_bg = if self.switch_thumb_bg.a() > 0 {
            self.switch_thumb_bg
        } else {
            fg
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut root = div().id(self.id).flex_col().w(width).gap(12);
        let on_click = self.on_item_click;
        let ctrl_rounded = self.rounded.saturating_sub(2);
        let thumb_rounded = self.switch_rounded.saturating_sub(2);
        let mut seq = 1u32;

        for (g_idx, grp) in self.groups.iter().enumerate() {
            let mut grp_box = div()
                .flex_col()
                .w(width)
                .p(10)
                .gap(8)
                .rounded(self.rounded)
                .bg(self.bg)
                .child(cx, text(grp.title).size(13).color(text_color));
            if self.border_color.a() > 0 {
                grp_box = grp_box.border(1, self.border_color);
            }

            for (i_idx, item) in grp.items.iter().enumerate() {
                let mut left_col = div()
                    .flex_col()
                    .gap(2)
                    .child(cx, text(item.title).size(12).color(text_color));
                if !item.description.is_empty() {
                    left_col = left_col.child(
                        cx,
                        text(item.description).size(11).color(muted_color),
                    );
                }

                let ctrl = match item.value {
                    SettingsValue::Bool(on) => {
                        let track_bg = if on {
                            self.active_bg
                        } else {
                            self.switch_off_bg
                        };
                        let mut track = div()
                            .flex_row()
                            .items_center()
                            .w(32)
                            .h(18)
                            .p(2)
                            .rounded(self.switch_rounded)
                            .bg(track_bg);
                        if track_bg.a() == 0 {
                            track = track.border(1, fg);
                        }
                        if on {
                            track.style_mut().set_justify(Justify::End);
                        } else {
                            track.style_mut().set_justify(Justify::Start);
                        }
                        let mut thumb = div()
                            .w(14)
                            .h(14)
                            .rounded(thumb_rounded)
                            .bg(thumb_bg);
                        if let Some((blur, ox, oy, col)) = self.switch_thumb_shadow {
                            thumb = thumb.shadow(blur, ox, oy, col);
                        }
                        track.child(cx, thumb)
                    }
                    SettingsValue::Text(val) => {
                        let mut badge = div()
                            .px(8)
                            .py(3)
                            .rounded(ctrl_rounded)
                            .bg(self.control_bg)
                            .child(cx, text(val).size(11).color(text_color));
                        if self.border_color.a() > 0 {
                            badge = badge.border(1, self.border_color);
                        }
                        badge
                    }
                    SettingsValue::Action(lbl) => {
                        let mut act = div()
                            .px(10)
                            .py(4)
                            .rounded(ctrl_rounded)
                            .bg(self.active_bg)
                            .child(cx, text(lbl).size(11).color(active_fg));
                        if let Some(h) = self.action_hover_bg {
                            act = act.hover_bg(h);
                        }
                        act
                    }
                };

                let mut row = div()
                    .id(crate::scoped_child_id(self.id, 0x6806_7B08, seq))
                    .flex_row()
                    .w(width - 20)
                    .px(6)
                    .py(4)
                    .rounded(ctrl_rounded)
                    .items_center()
                    .justify_between()
                    .focusable();
                if let Some(h) = self.row_hover_bg {
                    row = row.hover_bg(h);
                }
                if let Some(a) = self.row_active_bg {
                    row = row.active_bg(a);
                }
                if !self.focus_ring {
                    row = row.no_focus_border();
                }
                row = row.child(cx, left_col).child(cx, ctrl);
                seq += 1;

                if let Some(cb) = on_click {
                    let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                        cb(g_idx, i_idx, scope);
                    });
                    row = row.on_click_raw(click_cb);
                }
                grp_box = grp_box.child(cx, row);
            }
            root = root.child(cx, grp_box);
        }

        root = self.style_ext.apply_to_div(root, self.style);
        root.into_element(cx)
    }
}
