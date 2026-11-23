//! 一次性验证码输入框组件 `OtpInput` 与固定容量验证码状态机 `OtpState`。

use velox_core::prelude::*;

/// 面向 [`OtpInput`] 的固定位数无堆分配验证码输入状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtpState<const N: usize = 6> {
    digits: [u8; N],
    len: usize,
    active_slot: usize,
}

impl<const N: usize> Default for OtpState<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> OtpState<N> {
    /// 创建一个空的 `N` 位验证码状态。
    pub const fn new() -> Self {
        Self {
            digits: [0u8; N],
            len: 0,
            active_slot: 0,
        }
    }

    /// 当前已输入的验证码长度。
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否已填满所有 `N` 位。
    #[inline]
    pub const fn is_complete(&self) -> bool {
        self.len == N
    }

    /// 当前聚焦的槽位索引（`0..N`）。
    #[inline]
    pub const fn active_slot(&self) -> usize {
        self.active_slot
    }

    /// 获取当前已输入的 ASCII 字符串切片。
    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.digits[..self.len]) }
    }

    /// 获取第 `index` 个槽位的字符（未输入时返回 `None`）。
    #[inline]
    pub fn slot_char(&self, index: usize) -> Option<char> {
        if index < self.len {
            Some(self.digits[index] as char)
        } else {
            None
        }
    }

    /// 输入单个 ASCII 数字或字母，自动推进焦点槽位。
    pub fn push_char(&mut self, ch: char) -> bool {
        if !ch.is_ascii_alphanumeric() || self.len >= N {
            return false;
        }
        self.digits[self.len] = ch as u8;
        self.len += 1;
        self.active_slot = self.len.min(N.saturating_sub(1));
        true
    }

    /// 退格删除最后一位字符并回退焦点槽位。
    pub fn backspace(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        self.len -= 1;
        self.digits[self.len] = 0;
        self.active_slot = self.len;
        true
    }

    /// 清空全部输入。
    pub fn clear(&mut self) {
        self.digits = [0u8; N];
        self.len = 0;
        self.active_slot = 0;
    }

    /// 处理键盘按键事件。
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char(ch) => self.push_char(ch),
            KeyCode::Backspace | KeyCode::Delete => self.backspace(),
            _ => false,
        }
    }
}

/// 面向 [`OtpInput::slot`] 插槽回调的单个验证码格状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtpSlotState {
    pub index: usize,
    pub is_active: bool,
    pub is_filled: bool,
    pub is_masked: bool,
    pub ch: Option<char>,
}

impl OtpSlotState {
    #[inline(always)]
    pub const fn index(&self) -> usize {
        self.index
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }

    #[inline(always)]
    pub const fn is_filled(&self) -> bool {
        self.is_filled
    }

    #[inline(always)]
    pub const fn is_masked(&self) -> bool {
        self.is_masked
    }

    #[inline(always)]
    pub const fn ch(&self) -> Option<char> {
        self.ch
    }
}

/// 一次性密码 / 验证码分格输入框基础组件（`OtpInput`）。
pub struct OtpInput<'a> {
    pub id: u32,
    pub value: &'a str,
    pub length: usize,
    pub masked: bool,
    pub focused: bool,
    pub group_size: usize,
    pub slot_size: i16,
    pub bg: Color,
    pub border_color: Color,
    pub active_border_color: Color,
    pub text_color: Color,
    pub separator_color: Color,
    pub rounded: u8,
    pub focus_ring: bool,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_key_input: Option<&'a dyn Fn(KeyCode, &mut ActionScope)>,
    pub slot_renderer: Option<&'a dyn Fn(Div<'a>, OtpSlotState, &'a FrameScope) -> Element<'a>>,
}

impl<'a> OtpInput<'a> {
    /// 创建一个默认 6 位无样式验证码分格输入框原语。
    #[inline(always)]
    pub const fn new(id: u32, value: &'a str, length: usize) -> Self {
        Self {
            id,
            value,
            length: if length == 0 { 6 } else { length },
            masked: false,
            focused: false,
            group_size: 3,
            slot_size: 36,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            separator_color: Color::TRANSPARENT,
            rounded: 0,
            focus_ring: false,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_key_input: None,
            slot_renderer: None,
        }
    }

    /// 设置是否以掩码圆点（`•`）隐藏已输入的验证码。
    #[inline(always)]
    pub const fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// 设置是否处于聚焦态。
    #[inline(always)]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// 设置每组显示的格子数量（组间插入短横分隔符）。
    #[inline(always)]
    pub const fn groups(mut self, group_size: usize) -> Self {
        self.group_size = group_size;
        self
    }

    /// 绑定键盘按键输入回调。
    #[inline(always)]
    pub fn on_key_input(mut self, handler: &'a dyn Fn(KeyCode, &mut ActionScope)) -> Self {
        self.on_key_input = Some(handler);
        self
    }

    /// `gpui-base` 风格单格插槽回调：将每个验证码槽位外观定制权交由调用方。
    #[inline(always)]
    pub fn slot<F, E>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, OtpSlotState, &'a FrameScope) -> E + 'a,
        E: IntoElement<'a>,
    {
        let cb = cx
            .arena()
            .alloc(move |item: Div<'a>, state: OtpSlotState, scope: &'a FrameScope| {
                f(item, state, scope).into_element(scope)
            });
        self.slot_renderer = Some(cb);
        self
    }
}

crate::impl_base_styled_box!(OtpInput<'a>);

impl<'a> Widget<'a> for OtpInput<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let active_border_color = if self.active_border_color != Color::TRANSPARENT {
            self.active_border_color
        } else {
            border_color
        };
        let text_color = if self.text_color == Color::TRANSPARENT {
            fg
        } else {
            self.text_color
        };
        let separator_color = if self.separator_color == Color::TRANSPARENT {
            fg.scale_alpha(150)
        } else {
            self.separator_color
        };
        let rounded = self.rounded;

        let mut row = div()
            .id(self.id)
            .flex_row()
            .items_center()
            .gap(6)
            .focusable();
        if !self.focus_ring {
            row = row.no_focus_border();
        }

        if let Some(kcb) = self.on_key_input {
            row = row.on_key_input(kcb);
        }

        let bytes = self.value.as_bytes();
        let active_idx = bytes.len().min(self.length.saturating_sub(1));

        for idx in 0..self.length {
            if self.group_size > 0 && idx > 0 && idx % self.group_size == 0 {
                row = row.child(
                    cx,
                    text("-").size(14).color(separator_color),
                );
            }

            let is_active = self.focused && idx == active_idx;
            let is_filled = idx < bytes.len();
            let slot_id = crate::scoped_child_id(self.id, 0x3124_13FD, idx as u32 + 1);

            if let Some(renderer) = self.slot_renderer {
                let mut slot = div().id(slot_id).items_center().justify_center();
                if is_filled {
                    let cell_str = if self.masked {
                        "•"
                    } else {
                        let b = cx.arena().alloc_bytes(1);
                        b[0] = bytes[idx];
                        unsafe { core::str::from_utf8_unchecked(b) }
                    };
                    slot = slot.child(cx, text(cell_str));
                }
                let state = OtpSlotState {
                    index: idx,
                    is_active,
                    is_filled,
                    is_masked: self.masked,
                    ch: if is_filled { Some(bytes[idx] as char) } else { None },
                };
                row = row.child(cx, renderer(slot, state, cx));
            } else {
                let border_col = if is_active {
                    active_border_color
                } else {
                    border_color
                };

                let mut slot = div()
                    .id(slot_id)
                    .w(self.slot_size)
                    .h(self.slot_size)
                    .rounded(rounded)
                    .bg(bg)
                    .items_center()
                    .justify_center();
                if border_col != Color::TRANSPARENT {
                    slot = slot.border(if is_active { 2 } else { 1 }, border_col);
                }

                if is_filled {
                    let cell_str = if self.masked {
                        "•"
                    } else {
                        let b = cx.arena().alloc_bytes(1);
                        b[0] = bytes[idx];
                        unsafe { core::str::from_utf8_unchecked(b) }
                    };
                    slot = slot.child(cx, text(cell_str).size(15).color(text_color));
                }

                row = row.child(cx, slot);
            }
        }

        row = self.style_ext.apply_to_div(row, self.style);
        row.into_element(cx)
    }
}
