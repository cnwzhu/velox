//! 垂直滚轮选择器组件 `Roller`（定义可见行窗口、居中高亮选中、点击滚动行为与 `Theme` 按需回退）。

use core::time::Duration;
use velox_engine::prelude::*;

/// 滚轮选择器组件（`Roller`）。
pub struct Roller<'a> {
    pub id: u32,
    pub options: &'a [&'a str],
    pub selected: usize,
    pub width: i16,
    pub row_height: i16,
    pub visible_rows: u8,
    pub bg: Color,
    pub border_color: Color,
    pub center_bg: Color,
    pub center_border_color: Color,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub rounded: u8,
    pub animated: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_change: Option<&'a dyn Fn(usize, &mut ActionScope)>,
}

impl<'a> Roller<'a> {
    /// 创建一个默认无样式的滚轮选择器。
    #[inline(always)]
    pub const fn new(id: u32, options: &'a [&'a str], selected: usize) -> Self {
        Self {
            id,
            options,
            selected,
            width: 100,
            row_height: 28,
            visible_rows: 3,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            center_bg: Color::TRANSPARENT,
            center_border_color: Color::TRANSPARENT,
            active_fg: Color::TRANSPARENT,
            inactive_fg: Color::TRANSPARENT,
            rounded: 0,
            animated: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_change: None,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<usize>`]：渲染期自动建立细粒度订阅，滚动选定新项时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, options: &'a [&'a str], sig: Signal<usize>) -> Self {
        let cb = cx.arena().alloc(move |idx: usize, _: &mut ActionScope| {
            sig.set_if_changed(idx);
        });
        Self::new(id, options, sig.get()).on_change(cb)
    }

    /// 设置滚轮总宽度（像素，默认 `100px`）。
    #[inline(always)]
    pub const fn w(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 设置单行高度（像素，默认 `28px`）。
    #[inline(always)]
    pub const fn row_h(mut self, row_height: i16) -> Self {
        self.row_height = if row_height < 16 { 16 } else { row_height };
        self
    }

    /// 设置可见行数（建议奇数如 `3` 或 `5`，默认 `3`）。
    #[inline(always)]
    pub const fn visible_rows(mut self, rows: u8) -> Self {
        self.visible_rows = if rows < 3 { 3 } else { rows | 1 };
        self
    }

    /// 绑定选中项变更回调。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(usize, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Roller<'a>);

impl<'a> Widget<'a> for Roller<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg_base = cx.theme().foreground;
        let active_fg = if self.active_fg.a() > 0 {
            self.active_fg
        } else {
            fg_base
        };
        let inactive_fg = if self.inactive_fg.a() > 0 {
            self.inactive_fg
        } else {
            fg_base.scale_alpha(140)
        };

        let n = self.options.len();
        let sel = if n > 0 { self.selected.min(n - 1) } else { 0 };
        let half = (self.visible_rows / 2) as isize;
        let total_h = self.row_height.saturating_mul(self.visible_rows as i16);

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut container = div()
            .id(self.id)
            .flex_col()
            .w(width)
            .h(total_h)
            .rounded(self.rounded)
            .bg(self.bg);
        if self.border_color.a() > 0 {
            container = container.border(1, self.border_color);
        }

        for offset in -half..=half {
            let target_idx = (sel as isize) + offset;
            let is_center = offset == 0;
            let row_id = crate::scoped_child_id(self.id, 0x00F3_2429, ((offset + half + 1) as u32) * 17);

            let mut row = div()
                .id(row_id)
                .flex_row()
                .w_full()
                .h(self.row_height)
                .items_center()
                .justify_center();

            if is_center {
                row = row
                    .bg(self.center_bg)
                    .rounded(self.rounded.saturating_sub(2));
                if self.center_border_color.a() > 0 {
                    row = row.border(1, self.center_border_color);
                }
                if self.animated {
                    row = row.animate_color(Duration::from_millis(120));
                }
            }

            if target_idx >= 0 && (target_idx as usize) < n {
                let idx_usize = target_idx as usize;
                let label = self.options[idx_usize];
                let fg = if is_center {
                    active_fg
                } else {
                    inactive_fg
                };
                let fsize = if is_center { 15 } else { 12 };

                if !is_center {
                    if let Some(cb) = self.on_change {
                        let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                            cb(idx_usize, acx);
                        });
                        row = row.on_click_raw(raw_cb);
                    }
                }

                row = row.child(cx, text(label).size(fsize).color(fg));
            }

            container = container.child(cx, row);
        }

        container = self.style_ext.apply_to_div(container, self.style);
        container.into_element(cx)
    }
}
