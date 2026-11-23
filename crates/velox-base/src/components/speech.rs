//! 语音听写与语音转文字输入组件 `Speech`（支持按需回退 `Theme` 样式）。

use velox_engine::prelude::*;

/// 语音听写引擎工作状态枚举（`SpeechStatus`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpeechStatus {
    #[default]
    Idle,
    Listening,
    Processing,
    Error,
}

/// 语音听写状态结构（`SpeechState`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechState {
    pub status: SpeechStatus,
    /// 当前麦克风音量强度百分比（`0..=100`）。
    pub audio_level: u8,
}

impl Default for SpeechState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechState {
    /// 创建一个默认空闲态的语音听写状态。
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            status: SpeechStatus::Idle,
            audio_level: 0,
        }
    }

    /// 切换开始/停止录音状态。
    #[inline]
    pub fn toggle_listening(&mut self) {
        self.status = match self.status {
            SpeechStatus::Listening => SpeechStatus::Idle,
            _ => SpeechStatus::Listening,
        };
    }
}

/// 语音听写与语音转文字输入组件（`Speech`）。
pub struct Speech<'a> {
    pub id: u32,
    pub state: SpeechState,
    pub transcript: &'a str,
    pub placeholder: &'a str,
    pub width: i16,
    pub bg: Color,
    pub border_color: Color,
    pub active_border_color: Color,
    pub text_color: Color,
    pub muted_color: Color,
    pub idle_mic_bg: Color,
    pub listening_mic_bg: Color,
    pub processing_mic_bg: Color,
    pub error_mic_bg: Color,
    pub mic_fg: Color,
    pub meter_color: Color,
    pub rounded: u8,
    pub style: Style,
    pub style_ext: StyleExt,
    pub on_toggle_listen: Option<&'a dyn Fn(&mut ActionScope)>,
    pub on_clear: Option<&'a dyn Fn(&mut ActionScope)>,
}

impl<'a> Speech<'a> {
    /// 创建一个语音听写输入组件（未显式指定的颜色与圆角将在渲染时按需回退 `Theme`）。
    #[inline(always)]
    pub const fn new(id: u32, state: SpeechState, transcript: &'a str) -> Self {
        Self {
            id,
            state,
            transcript,
            placeholder: "Click mic to dictate...",
            width: 260,
            bg: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            active_border_color: Color::TRANSPARENT,
            text_color: Color::TRANSPARENT,
            muted_color: Color::TRANSPARENT,
            idle_mic_bg: Color::TRANSPARENT,
            listening_mic_bg: Color::TRANSPARENT,
            processing_mic_bg: Color::TRANSPARENT,
            error_mic_bg: Color::TRANSPARENT,
            mic_fg: Color::TRANSPARENT,
            meter_color: Color::TRANSPARENT,
            rounded: 0,
            style: Style::DEFAULT,
            style_ext: StyleExt::DEFAULT,
            on_toggle_listen: None,
            on_clear: None,
        }
    }

    /// 设置容器宽度。
    #[inline(always)]
    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// 绑定点击麦克风开始/停止听写回调。
    #[inline(always)]
    pub fn on_toggle_listen(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_toggle_listen = Some(handler);
        self
    }

    /// 绑定清空转写文本回调。
    #[inline(always)]
    pub fn on_clear(mut self, handler: &'a dyn Fn(&mut ActionScope)) -> Self {
        self.on_clear = Some(handler);
        self
    }
}

crate::impl_base_styled_box!(Speech<'a>);

impl<'a> Widget<'a> for Speech<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let bg = self.bg;
        let border_color = self.border_color;
        let active_border_color = if self.active_border_color != Color::TRANSPARENT {
            self.active_border_color
        } else {
            border_color
        };
        let text_color = if self.text_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.text_color
        };
        let muted_color = if self.muted_color == Color::TRANSPARENT {
            def_fg.scale_alpha(140)
        } else {
            self.muted_color
        };
        let mic_bg = match self.state.status {
            SpeechStatus::Listening => self.listening_mic_bg,
            SpeechStatus::Processing => self.processing_mic_bg,
            SpeechStatus::Error => self.error_mic_bg,
            SpeechStatus::Idle => self.idle_mic_bg,
        };
        let mic_fg = if self.mic_fg == Color::TRANSPARENT {
            def_fg
        } else {
            self.mic_fg
        };
        let meter_color = if self.meter_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.meter_color
        };
        let rounded = self.rounded;

        let is_listening = self.state.status == SpeechStatus::Listening;

        let mut mic_btn = div()
            .id(crate::scoped_child_id(self.id, 0x5DCC_0B17, 1))
            .w(28)
            .h(28)
            .rounded(if rounded > 0 { 14 } else { 0 })
            .bg(mic_bg)
            .items_center()
            .justify_center()
            .focusable()
            .no_focus_border()
            .child(
                cx,
                text(if is_listening { "■" } else { "🎤" })
                    .size(12)
                    .color(mic_fg),
            );
        if let Some(cb) = self.on_toggle_listen {
            mic_btn = mic_btn.on_click_raw(cb);
        }

        let display_str = if !self.transcript.is_empty() {
            self.transcript
        } else {
            self.placeholder
        };
        let txt_col = if !self.transcript.is_empty() {
            text_color
        } else {
            muted_color
        };

        let bar_border = if is_listening {
            active_border_color
        } else {
            border_color
        };

        let width = StyleExt::resolve_width(self.style, self.width);
        let mut bar = div()
            .id(self.id)
            .flex_row()
            .w(width)
            .h(38)
            .px(8)
            .gap(8)
            .rounded(rounded)
            .bg(bg)
            .items_center()
            .focusable()
            .no_focus_border()
            .child(cx, mic_btn)
            .child(
                cx,
                div()
                    .w((width - 80).max(60))
                    .child(cx, text(display_str).size(12).color(txt_col).ellipsis()),
            );
        if bar_border != Color::TRANSPARENT {
            bar = bar.border(1, bar_border);
        }

        if is_listening {
            let h1 = (4 + (self.state.audio_level as i16 * 16) / 100).clamp(4, 20);
            let h2 = (6 + (self.state.audio_level as i16 * 12) / 100).clamp(4, 20);
            let bar_r = if rounded > 0 { 1 } else { 0 };
            let meter = div()
                .flex_row()
                .gap(2)
                .items_center()
                .child(cx, div().w(3).h(h1).rounded(bar_r).bg(meter_color))
                .child(cx, div().w(3).h(h2).rounded(bar_r).bg(meter_color));
            bar = bar.child(cx, meter);
        } else if let Some(clear_cb) = self.on_clear {
            let clr = div()
                .id(crate::scoped_child_id(self.id, 0x5DCC_0B17, 2))
                .w(20)
                .h(20)
                .items_center()
                .justify_center()
                .focusable()
                .on_click_raw(clear_cb)
                .child(cx, text("×").size(12).color(muted_color));
            bar = bar.child(cx, clr);
        }

        bar = self.style_ext.apply_to_div(bar, self.style);
        bar.into_element(cx)
    }
}
