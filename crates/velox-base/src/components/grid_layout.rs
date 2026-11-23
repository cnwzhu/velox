//! 可拖拽、可缩放的仪表盘与看板网格布局基础组件 `GridLayout`（默认无样式）。

use velox_engine::prelude::*;

/// 仪表盘网格看板磁贴单元定义（`GridTile`）。
#[derive(Clone, Copy, Debug)]
pub struct GridTile<'a> {
    pub id: u32,
    pub col: u8,
    pub row: u8,
    pub col_span: u8,
    pub row_span: u8,
    pub title: &'a str,
    pub body: &'a str,
}

impl<'a> GridTile<'a> {
    /// 创建一个指定网格坐标 `(col, row)` 与跨度 `(col_span, row_span)` 的看板磁贴。
    #[inline(always)]
    pub const fn new(
        id: u32,
        col: u8,
        row: u8,
        col_span: u8,
        row_span: u8,
        title: &'a str,
        body: &'a str,
    ) -> Self {
        Self {
            id,
            col,
            row,
            col_span: if col_span == 0 { 1 } else { col_span },
            row_span: if row_span == 0 { 1 } else { row_span },
            title,
            body,
        }
    }
}

/// 可拖拽、可缩放的仪表盘与组件看板网格布局基础组件（`GridLayout`，默认无样式）。
pub struct GridLayout<'a> {
    pub id: u32,
    pub columns: u8,
    pub cell_width: i16,
    pub cell_height: i16,
    pub gap: i16,
    pub tiles: &'a [GridTile<'a>],
    pub active_tile_id: Option<u32>,
    pub tile_bg: Color,
    pub border_color: Color,
    pub active_border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_tile_click: Option<&'a dyn Fn(u32, &mut ActionScope)>,
}

impl<'a> GridLayout<'a> {
    /// 创建一个默认无样式的网格看板布局组件。
    #[inline(always)]
    pub const fn new(
        id: u32,
        columns: u8,
        cell_width: i16,
        cell_height: i16,
        tiles: &'a [GridTile<'a>],
    ) -> Self {
        Self {
            id,
            columns: if columns == 0 { 1 } else { columns },
            cell_width: if cell_width < 24 { 24 } else { cell_width },
            cell_height: if cell_height < 24 { 24 } else { cell_height },
            gap: 8,
            tiles,
            active_tile_id: None,
            tile_bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_tile_click: None,
        }
    }

    /// 设置当前激活或拖拽中的磁贴 ID。
    #[inline(always)]
    pub const fn active_tile(mut self, tile_id: Option<u32>) -> Self {
        self.active_tile_id = tile_id;
        self
    }

    /// 绑定点击某个磁贴的回调。
    #[inline(always)]
    pub fn on_tile_click(mut self, handler: &'a dyn Fn(u32, &mut ActionScope)) -> Self {
        self.on_tile_click = Some(handler);
        self
    }

    /// 计算单个磁贴在容器内的像素几何矩形 `(x, y, width, height)`。
    #[inline]
    pub const fn tile_bounds(&self, tile: &GridTile<'a>) -> (i16, i16, i16, i16) {
        let step_x = self.cell_width + self.gap;
        let step_y = self.cell_height + self.gap;
        let x = (tile.col as i16) * step_x;
        let y = (tile.row as i16) * step_y;
        let w = (tile.col_span as i16) * self.cell_width + ((tile.col_span as i16) - 1) * self.gap;
        let h = (tile.row_span as i16) * self.cell_height + ((tile.row_span as i16) - 1) * self.gap;
        (x, y, w, h)
    }
}

crate::impl_base_styled_box!(GridLayout<'a>);

impl<'a> Widget<'a> for GridLayout<'a> {
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

        let total_w = (self.columns as i16) * self.cell_width
            + ((self.columns as i16).saturating_sub(1)) * self.gap;
        let max_row = self
            .tiles
            .iter()
            .map(|t| (t.row + t.row_span) as i16)
            .max()
            .unwrap_or(1);
        let total_h = max_row * self.cell_height + (max_row.saturating_sub(1)) * self.gap;

        let mut board = div().id(self.id).w(total_w).h(total_h);
        let on_click = self.on_tile_click;

        for tile in self.tiles {
            let (tx, ty, tw, th) = self.tile_bounds(tile);
            let is_active = self.active_tile_id == Some(tile.id);
            let bcol = if is_active {
                self.active_border_color
            } else {
                self.border_color
            };

            let mut card = div()
                .id(crate::scoped_child_id(self.id, 0x2209_9D74, tile.id))
                .pos(tx, ty)
                .flex_col()
                .w(tw)
                .h(th)
                .p(10)
                .gap(4)
                .rounded(self.rounded)
                .bg(self.tile_bg)
                .focusable();
            if bcol.a() > 0 {
                card = card.border(1, bcol);
            }
            if !self.focus_ring {
                card = card.no_focus_border();
            }
            card = card
                .child(cx, text(tile.title).size(12).color(text_color))
                .child(cx, text(tile.body).size(11).color(muted_color));

            if let Some(cb) = on_click {
                let tid = tile.id;
                let click_cb = cx.arena().alloc(move |scope: &mut ActionScope| {
                    cb(tid, scope);
                });
                card = card.on_click_raw(click_cb);
            }
            board = board.child(cx, card);
        }

        board = self.style_ext.apply_to_div(board, self.style);
        board.into_element(cx)
    }
}
