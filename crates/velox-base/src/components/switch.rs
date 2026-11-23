//! 无样式拨动开关行为原语 `Switch`（标准 `28×16` / `36×20` / `44×24` 几何尺寸与左右标签交互；
//! 默认零胶囊圆角、零主题蓝填充、零阴影与零过渡动画，预设主题外观由 `velox-component::Switch` 提供）。

use crate::traits::ComponentSize;
use core::time::Duration;
use velox_engine::prelude::*;

const FLAG_GEOMETRY: u8 = 1 << 0;
const FLAG_TRACK_BG: u8 = 1 << 1;
const FLAG_THUMB_BG: u8 = 1 << 2;
const FLAG_BORDER: u8 = 1 << 3;
const FLAG_ACTIVE_COLOR: u8 = 1 << 4;

/// 开关附属标签的位置方向（`label_side` 规格）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum SwitchLabelSide {
    Left,
    #[default]
    Right,
}

/// 传递给 `Switch::track(...)` / `Switch::thumb(...)` 插槽回调的开关状态。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SwitchState {
    pub checked: bool,
    pub disabled: bool,
}

impl SwitchState {
    #[inline(always)] pub const fn is_checked(&self) -> bool { self.checked }
    #[inline(always)] pub const fn is_disabled(&self) -> bool { self.disabled }
}

type SwitchPartRenderer<'a> = &'a dyn Fn(Div<'a>, SwitchState, &'a FrameScope) -> Element<'a>;
type SwitchTrackRenderer<'a> = &'a dyn Fn(Div<'a>, SwitchState) -> Div<'a>;

/// 无样式拨动开关行为原语（`Switch`）。
pub struct Switch<'a> {
    pub id: u32,
    pub checked: bool,
    pub disabled: bool,
    pub focus_ring: bool,
    pub label: Option<StrRef<'a>>,
    pub label_side: SwitchLabelSide,
    pub size: ComponentSize,
    pub style: Style,
    pub style_ext: StyleExt,
    pub track_width: i16,
    pub track_height: i16,
    pub thumb_size: i16,
    pub padding: i8,
    pub track_bg: Color,
    pub active_color: Color,
    pub thumb_bg: Color,
    pub label_color: Color,
    pub border_width: u8,
    pub border_color: Color,
    pub track_rounded: Option<u8>,
    pub thumb_rounded: Option<u8>,
    pub hover_bg: Option<Color>,
    pub active_bg: Option<Color>,
    pub thumb_shadow: Option<(u8, i8, i8, Color)>,
    pub animated: bool,
    pub on_change: Option<&'a dyn Fn(bool, &mut ActionScope)>,
    track_renderer: Option<SwitchTrackRenderer<'a>>,
    thumb_renderer: Option<SwitchPartRenderer<'a>>,
    custom_flags: u8,
}

impl<'a> Switch<'a> {
    /// 创建一个无样式拨动开关原语（默认直角透明轨道 + `1px` 中性边框 + 直角滑块，无阴影/动画/主题蓝）。
    #[inline(always)]
    pub const fn new(id: u32, checked: bool) -> Self {
        Self {
            id,
            checked,
            disabled: false,
            focus_ring: false,
            label: None,
            label_side: SwitchLabelSide::Right,
            size: ComponentSize::Medium,
            style: Style {
                width: Length::Auto,
                height: Length::Auto,
                padding: Edges::all(0),
                bg: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                gap: 0,
                rounded: 0,
                border_width: 0,
                flex_flags: 0,
            },
            style_ext: StyleExt::DEFAULT,
            track_width: 36,
            track_height: 20,
            thumb_size: 16,
            padding: 2,
            track_bg: Color::TRANSPARENT,
            active_color: Color::TRANSPARENT,
            thumb_bg: Color::TRANSPARENT,
            label_color: Color::TRANSPARENT,
            border_width: 0,
            border_color: Color::TRANSPARENT,
            track_rounded: None,
            thumb_rounded: None,
            hover_bg: None,
            active_bg: None,
            thumb_shadow: None,
            animated: false,
            on_change: None,
            track_renderer: None,
            thumb_renderer: None,
            custom_flags: 0,
        }
    }

    /// 零堆分配双向绑定至 [`Signal<bool>`]：渲染期自动建立细粒度订阅，切换时通过 `set_if_changed` 精准更新。
    #[inline]
    pub fn bind(cx: &'a FrameScope, id: u32, sig: Signal<bool>) -> Self {
        let cb = cx.arena().alloc(move |v: bool, _: &mut ActionScope| {
            sig.set_if_changed(v);
        });
        Self::new(id, sig.get()).on_change(cb)
    }

    /// 设置开关旁显示的文字标签（点击标签亦可切换开关）。
    #[inline(always)]
    pub fn label(mut self, text: impl Into<StrRef<'a>>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// 设置标签位于轨道左侧或右侧（默认右侧）。
    #[inline(always)]
    pub const fn label_side(mut self, side: SwitchLabelSide) -> Self {
        self.label_side = side;
        self
    }

    /// 自定义开启态轨道强调色。
    #[inline(always)]
    pub const fn color(mut self, color: Color) -> Self {
        self.active_color = color;
        self.custom_flags |= FLAG_ACTIVE_COLOR;
        self
    }

    /// 启用或禁用键盘焦点环（无样式原语默认关闭）。
    #[inline(always)]
    pub const fn focus_ring(mut self, enabled: bool) -> Self {
        self.focus_ring = enabled;
        self
    }

    /// 设置轨道与滑块几何尺寸 `(track_width, track_height, thumb_size, padding)`。
    #[inline(always)]
    pub const fn geometry(
        mut self,
        track_width: i16,
        track_height: i16,
        thumb_size: i16,
        padding: i8,
    ) -> Self {
        self.track_width = track_width;
        self.track_height = track_height;
        self.thumb_size = thumb_size;
        self.padding = padding;
        self.custom_flags |= FLAG_GEOMETRY;
        self
    }

    /// 设置轨道背景色（覆盖开启与关闭态默认轨道色）。
    #[inline(always)]
    pub const fn track_bg(mut self, color: Color) -> Self {
        self.track_bg = color;
        self.custom_flags |= FLAG_TRACK_BG;
        self
    }

    /// 设置滑块背景色。
    #[inline(always)]
    pub const fn thumb_bg(mut self, color: Color) -> Self {
        self.thumb_bg = color;
        self.custom_flags |= FLAG_THUMB_BG;
        self
    }

    /// 设置轨道边框宽度与颜色。
    #[inline(always)]
    pub const fn border(mut self, width: u8, color: Color) -> Self {
        self.border_width = width;
        self.border_color = color;
        self.custom_flags |= FLAG_BORDER;
        self
    }

    /// 启用或禁用颜色与位置过渡动画。
    #[inline(always)]
    pub const fn animated(mut self, enabled: bool) -> Self {
        self.animated = enabled;
        self
    }

    /// 绑定开关状态变更回调闭包（传入切换后的新 `bool` 状态）。
    #[inline(always)]
    pub fn on_change(mut self, handler: &'a dyn Fn(bool, &mut ActionScope)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// 是否已显式设置开启态强调色。
    #[inline(always)]
    pub const fn has_custom_active_color(&self) -> bool {
        (self.custom_flags & FLAG_ACTIVE_COLOR) != 0
    }

    /// 是否已显式设置轨道背景色。
    #[inline(always)]
    pub const fn has_custom_track_bg(&self) -> bool {
        (self.custom_flags & FLAG_TRACK_BG) != 0
    }

    /// 是否已显式设置滑块背景色。
    #[inline(always)]
    pub const fn has_custom_thumb_bg(&self) -> bool {
        (self.custom_flags & FLAG_THUMB_BG) != 0
    }

    /// `gpui-base` 风格轨道容器插槽定制回调（零堆分配）。
    #[inline]
    pub fn track<F>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SwitchState) -> Div<'a> + 'a,
    {
        self.track_renderer = Some(cx.arena().alloc(f));
        self
    }

    /// `gpui-base` 风格滑块插槽定制回调（零堆分配）。
    #[inline]
    pub fn thumb<F, R>(mut self, cx: &'a FrameScope, f: F) -> Self
    where
        F: Fn(Div<'a>, SwitchState) -> R + 'a,
        R: IntoElement<'a>,
    {
        let cb = cx.arena().alloc(
            move |thumb: Div<'a>, state: SwitchState, frame_cx: &'a FrameScope| {
                f(thumb, state).into_element(frame_cx)
            },
        );
        self.thumb_renderer = Some(cb);
        self
    }

    /// 是否已显式设置轨道边框。
    #[inline(always)]
    pub const fn has_custom_border(&self) -> bool {
        (self.custom_flags & FLAG_BORDER) != 0
    }
}

crate::impl_base_styled_box!(Switch<'a>);
crate::impl_base_sizable!(Switch<'a>);
crate::impl_base_disableable!(Switch<'a>);

impl<'a> Widget<'a> for Switch<'a> {
    fn render_once(self, cx: &'a FrameScope) -> Element<'a> {
        let def_fg = cx.theme().foreground;
        let has_field_geom = self.track_width != 36
            || self.track_height != 20
            || self.thumb_size != 16
            || self.padding != 2;
        let (track_w, track_h, thumb_sz, pad, font_sz) =
            if (self.custom_flags & FLAG_GEOMETRY) != 0 || has_field_geom {
                (
                    self.track_width,
                    self.track_height,
                    self.thumb_size,
                    self.padding,
                    13u8,
                )
            } else {
                match self.size {
                    ComponentSize::Small => (28i16, 16i16, 12i16, 2i8, 12u8),
                    ComponentSize::Medium => (36i16, 20i16, 16i16, 2i8, 13u8),
                    ComponentSize::Large => (44i16, 24i16, 20i16, 2i8, 14u8),
                }
            };

        let track_bg = if (self.custom_flags & FLAG_TRACK_BG) != 0
            || self.track_bg != Color::TRANSPARENT
        {
            self.track_bg
        } else if self.checked && ((self.custom_flags & FLAG_ACTIVE_COLOR) != 0 || self.active_color != Color::TRANSPARENT) {
            self.active_color
        } else {
            Color::TRANSPARENT
        };

        let thumb_bg = if (self.custom_flags & FLAG_THUMB_BG) != 0
            || self.thumb_bg != Color::TRANSPARENT
        {
            self.thumb_bg
        } else {
            def_fg
        };

        let (border_width, border_color) = if (self.custom_flags & FLAG_BORDER) != 0
            || self.border_color != Color::TRANSPARENT
        {
            (self.border_width, self.border_color)
        } else if track_bg == Color::TRANSPARENT {
            (1, def_fg)
        } else {
            (0, Color::TRANSPARENT)
        };

        let track_r = self.track_rounded.unwrap_or(0);
        let thumb_r = self.thumb_rounded.unwrap_or(0);
        let label_col = if self.label_color == Color::TRANSPARENT {
            def_fg
        } else {
            self.label_color
        };

        let has_label = self.label.is_some();
        let track_id = if has_label {
            self.id ^ 0x4000_0000
        } else {
            self.id
        };

        let sw_state = SwitchState {
            checked: self.checked,
            disabled: self.disabled,
        };

        let mut track = if let Some(track_fn) = self.track_renderer {
            let mut base = div().id(track_id).flex_row().items_center();
            if self.checked {
                base.style_mut().set_justify(Justify::End);
            } else {
                base.style_mut().set_justify(Justify::Start);
            }
            track_fn(base, sw_state)
        } else {
            let mut t = div()
                .id(track_id)
                .flex_row()
                .items_center()
                .w(track_w)
                .h(track_h)
                .p(pad)
                .rounded(track_r)
                .bg(track_bg);

            if border_width > 0 && border_color != Color::TRANSPARENT {
                t = t.border(border_width, border_color);
            }
            if !self.disabled {
                if let Some(h) = self.hover_bg {
                    t = t.hover_bg(h);
                }
                if let Some(a) = self.active_bg {
                    t = t.active_bg(a);
                }
            }

            if self.animated {
                t = t.animate_color(Duration::from_millis(180));
            }

            if self.checked {
                t.style_mut().set_justify(Justify::End);
            } else {
                t.style_mut().set_justify(Justify::Start);
            }
            t
        };

        let thumb_el = if let Some(thumb_fn) = self.thumb_renderer {
            thumb_fn(div().id(self.id ^ 0x8000_0000), sw_state, cx)
        } else {
            let mut thumb = div()
                .id(self.id ^ 0x8000_0000)
                .w(thumb_sz)
                .h(thumb_sz)
                .rounded(thumb_r)
                .bg(thumb_bg);

            if let Some((blur, ox, oy, col)) = self.thumb_shadow {
                thumb = thumb.shadow(blur, ox, oy, col);
            }

            if self.animated {
                thumb = thumb.animate_bounds(Duration::from_millis(180));
            }
            thumb.into_element(cx)
        };
        track = track.child(cx, thumb_el);

        if let Some(lbl) = self.label {
            let mut row = div().id(self.id).flex_row().items_center().gap(8);
            if self.disabled {
                row = row.opacity(115).cursor(CursorShape::NotAllowed);
            } else {
                row = row.focusable().no_focus_border();
                if let Some(cb) = self.on_change {
                    let next_state = !self.checked;
                    let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                        cb(next_state, acx);
                    });
                    row = row.on_click_raw(raw_cb);
                }
            }
            let eff_fs = self.style_ext.text_size.unwrap_or(font_sz);
            let eff_col = self.style_ext.text_color.unwrap_or(label_col);
            let lbl_el = text(lbl).size(eff_fs).color(eff_col);
            let row = match self.label_side {
                SwitchLabelSide::Left => row.child(cx, lbl_el).child(cx, track),
                SwitchLabelSide::Right => row.child(cx, track).child(cx, lbl_el),
            };
            self.style_ext.apply_to_div(row, self.style).into_element(cx)
        } else {
            if self.disabled {
                track = track.opacity(115).cursor(CursorShape::NotAllowed);
            } else {
                track = track.focusable();
                if !self.focus_ring {
                    track = track.no_focus_border();
                }
                if let Some(cb) = self.on_change {
                    let next_state = !self.checked;
                    let raw_cb = cx.arena().alloc(move |acx: &mut ActionScope| {
                        cb(next_state, acx);
                    });
                    track = track.on_click_raw(raw_cb);
                }
            }
            self.style_ext.apply_to_div(track, self.style).into_element(cx)
        }
    }
}
