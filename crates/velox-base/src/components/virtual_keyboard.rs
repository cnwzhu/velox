//! 屏幕虚拟软键盘组件 `VirtualKeyboard`（定义 QWERTY 与数字小键盘模式、键位动作与按需 `Theme` 样式回退）。

use velox_engine::prelude::*;

/// 虚拟软键盘按键动作枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeyAction {
    /// 输入单个 ASCII 字符。
    Char(char),
    /// 退格删除。
    Backspace,
    /// 确认/回车提交。
    Enter,
}

/// 虚拟软键盘布局模式枚举。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum KeyboardMode {
    /// 标准英文全键盘（QWERTY 3 行）。
    #[default]
    Qwerty,
    /// 紧凑数字小键盘（`0..9`、退格、确认）。
    Numeric,
}

/// 触控虚拟软键盘组件（`VirtualKeyboard`）。
pub struct VirtualKeyboard<'a> {
    pub id: u32,
    pub mode: KeyboardMode,
    pub width: i16,
    pub height: i16,
    pub board_bg: Color,
    pub board_border_color: Color,
    pub key_bg: Color,
    pub key_fg: Color,
    pub enter_bg: Color,
    pub enter_fg: Color,
    pub active_bg: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_key: Option<&'a dyn Fn(KeyAction, &mut ActionScope)>,
}

const NUMERIC_KEYS: [(&str, KeyAction); 12] = [
    ("1", KeyAction::Char('1')),
    ("2", KeyAction::Char('2')),
    ("3", KeyAction::Char('3')),
    ("4", KeyAction::Char('4')),
    ("5", KeyAction::Char('5')),
    ("6", KeyAction::Char('6')),
    ("7", KeyAction::Char('7')),
    ("8", KeyAction::Char('8')),
    ("9", KeyAction::Char('9')),
    ("DEL", KeyAction::Backspace),
    ("0", KeyAction::Char('0')),
    ("OK", KeyAction::Enter),
];

const QWERTY_ROW1: [(&str, KeyAction); 10] = [
    ("Q", KeyAction::Char('Q')),
    ("W", KeyAction::Char('W')),
    ("E", KeyAction::Char('E')),
    ("R", KeyAction::Char('R')),
    ("T", KeyAction::Char('T')),
    ("Y", KeyAction::Char('Y')),
    ("U", KeyAction::Char('U')),
    ("I", KeyAction::Char('I')),
    ("O", KeyAction::Char('O')),
    ("P", KeyAction::Char('P')),
];

const QWERTY_ROW2: [(&str, KeyAction); 9] = [
    ("A", KeyAction::Char('A')),
    ("S", KeyAction::Char('S')),
    ("D", KeyAction::Char('D')),
    ("F", KeyAction::Char('F')),
    ("G", KeyAction::Char('G')),
    ("H", KeyAction::Char('H')),
    ("J", KeyAction::Char('J')),
    ("K", KeyAction::Char('K')),
    ("L", KeyAction::Char('L')),
];

const QWERTY_ROW3: [(&str, KeyAction); 9] = [
    ("Z", KeyAction::Char('Z')),
    ("X", KeyAction::Char('X')),
    ("C", KeyAction::Char('C')),
    ("V", KeyAction::Char('V')),
    ("B", KeyAction::Char('B')),
    ("N", KeyAction::Char('N')),
    ("M", KeyAction::Char('M')),
    ("DEL", KeyAction::Backspace),
    ("OK", KeyAction::Enter),
];

impl<'a> VirtualKeyboard<'a> {
    /// 创建一个指定布局模式的虚拟软键盘（未显式指定的颜色与圆角将在渲染时按需回退 `Theme`）。
    #[inline(always)]
    pub const fn new(id: u32, mode: KeyboardMode) -> Self {
        Self {
            id,
            mode,
            width: 300,
            height: 120,
            board_bg: Color::TRANSPARENT,
            board_border_color: Color::TRANSPARENT,
            key_bg: Color::TRANSPARENT,
            key_fg: Color::TRANSPARENT,
            enter_bg: Color::TRANSPARENT,
            enter_fg: Color::TRANSPARENT,
            active_bg: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_key: None,
        }
    }

    /// 设置软键盘宽高（像素）。
    #[inline(always)]
    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 绑定软键盘按键触发回调。
    #[inline(always)]
    pub fn on_key(mut self, handler: &'a dyn Fn(KeyAction, &mut ActionScope)) -> Self {
        self.on_key = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(VirtualKeyboard<'a>);

impl<'a> Widget<'a> for VirtualKeyboard<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let width = StyleExt::resolve_width(self.style, self.width);
        let height = StyleExt::resolve_height(self.style, self.height);
        let def_fg = cx.theme().foreground;
        let board_bg = self.board_bg;
        let board_border_color = self.board_border_color;
        let key_bg_base = self.key_bg;
        let key_fg_base = if self.key_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.key_fg
        };
        let enter_bg = self.enter_bg;
        let enter_fg = if self.enter_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.enter_fg
        };
        let active_bg = self.active_bg;
        let rounded = self.rounded;

        match self.mode {
            KeyboardMode::Numeric => {
                let row_h = ((height - 20) / 4).max(20);
                let mut root = div()
                    .id(self.id)
                    .grid(3)
                    .w(width)
                    .h(height)
                    .p(4)
                    .gap(4)
                    .rounded(rounded)
                    .bg(board_bg);
                if board_border_color != Color::TRANSPARENT {
                    root = root.border(1, board_border_color);
                }

                for (i, &(label, act)) in NUMERIC_KEYS.iter().enumerate() {
                    let is_ok = matches!(act, KeyAction::Enter);
                    let key_bg = if is_ok { enter_bg } else { key_bg_base };
                    let key_fg = if is_ok { enter_fg } else { key_fg_base };
                    let mut key_btn = div()
                        .id(crate::scoped_child_id(self.id, 0x14BD_8090, (i as u32) + 1))
                        .flex_row()
                        .h(row_h)
                        .items_center()
                        .justify_center()
                        .rounded(rounded.saturating_sub(2))
                        .bg(key_bg);
                    if active_bg != Color::TRANSPARENT {
                        key_btn = key_btn.active_bg(active_bg);
                    }

                    if let Some(cb) = self.on_key {
                        let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                            cb(act, acx);
                        });
                        key_btn = key_btn.on_click_raw(raw_cb);
                    }

                    root = root.child(
                        cx,
                        key_btn.child(cx, text(label).size(13).color(key_fg)),
                    );
                }
                self.style_ext.apply_to_div(root, self.style).into_element(cx)
            }
            KeyboardMode::Qwerty => {
                let rows: [&[(&str, KeyAction)]; 3] = [&QWERTY_ROW1, &QWERTY_ROW2, &QWERTY_ROW3];
                let mut root = div()
                    .id(self.id)
                    .flex_col()
                    .w(width)
                    .h(height)
                    .p(4)
                    .gap(4)
                    .rounded(rounded)
                    .bg(board_bg);
                if board_border_color != Color::TRANSPARENT {
                    root = root.border(1, board_border_color);
                }

                let mut seq = 1u32;
                for (r_idx, row_keys) in rows.iter().enumerate() {
                    let mut row_div = div()
                        .id(crate::scoped_child_id(self.id, 0x14BD_8090, 200 + r_idx as u32))
                        .flex_row()
                        .w_full()
                        .flex_grow(1)
                        .gap(3);

                    for &(label, act) in *row_keys {
                        let is_special = matches!(act, KeyAction::Enter | KeyAction::Backspace);
                        let key_bg = if matches!(act, KeyAction::Enter) {
                            enter_bg
                        } else {
                            key_bg_base
                        };
                        let key_fg = if matches!(act, KeyAction::Enter) {
                            enter_fg
                        } else {
                            key_fg_base
                        };
                        let mut key_btn = div()
                            .id(crate::scoped_child_id(self.id, 0x14BD_8090, seq))
                            .flex_row()
                            .flex_grow(if is_special { 2 } else { 1 })
                            .h_full()
                            .items_center()
                            .justify_center()
                            .rounded(rounded.saturating_sub(4))
                            .bg(key_bg);
                        if active_bg != Color::TRANSPARENT {
                            key_btn = key_btn.active_bg(active_bg);
                        }
                        seq += 1;

                        if let Some(cb) = self.on_key {
                            let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                                cb(act, acx);
                            });
                            key_btn = key_btn.on_click_raw(raw_cb);
                        }

                        row_div = row_div.child(
                            cx,
                            key_btn.child(cx, text(label).size(11).color(key_fg)),
                        );
                    }
                    root = root.child(cx, row_div);
                }
                self.style_ext.apply_to_div(root, self.style).into_element(cx)
            }
        }
    }
}
