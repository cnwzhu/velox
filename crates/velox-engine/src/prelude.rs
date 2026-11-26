//! 开发者常用导出集合（`use velox_engine::prelude::*;`）。

pub use core::time::Duration;

pub use crate::a11y::{
    children_of, collect_a11y_nodes, diff_a11y_snapshot, diff_a11y_trees, snapshot_a11y_tree,
    A11yNode, A11yNodeSnapshot, A11yRole, A11yTreeUpdate,
};
pub use crate::app::{
    application, init, open_window, AppContext, AppError, Application, BuiltWindow, WindowContext,
    WindowHandle, WindowOptions,
};
pub use crate::arena::{ScratchArena, SmallString, SmallVec, StrRef};
pub use crate::async_rt::{
    run_async_event_loop, AsyncDriver, AsyncLoopConfig, AsyncLoopReport, SchedulerCapabilities,
    SchedulerClock, YieldNow,
};
pub use crate::async_rt::{
    block_on, AsyncApp, BackgroundExecutor, BoxFuture, ForegroundExecutor, ManualScheduler,
    NativeStdScheduler, SchedulerBackend, StdAsyncDriver, Task, Timer, UiWakeFuture, UiWakeSignal,
};
pub use crate::damage::DamageTracker;
pub use crate::display::{DisplaySurface, PixelFormat, Rgb565, Rgba8888, SpriteFormat, Xrgb8888};
pub use crate::element::{
    div, img, img_arc, img_rotated_rgb565, img_shader, svg, svg_a4, svg_icon, svg_path, svg_str,
    text, Div, DivInteractivity, DivNode, DivVisual, Element, ElementNode, EnterFrom, Img,
    IntoElement, SpriteSource, Text, TransitionSpec, View, Widget,
};
pub use crate::engine::{GestureConfig, VeloxEngine, MAX_DRAW_OPS, MAX_HIT_REGIONS};
pub use crate::fmt;
pub use crate::i18n::{detect_system_locale, DynamicCatalog};
pub use crate::i18n::{
    format_template, I18nArg, I18nContext, Locale, MessageEntry, PluralCategory, StaticCatalog,
    StaticLocaleTable, TextDirection,
};
pub use crate::motion::{DrawOp, DrawPayload, MotionCache, MotionState, Presence, Tween};
pub use crate::paint::{
    BackdropFilter, BlendMode, ColorMatrix, ColorStop, CornerRadii, DropShadow, ExtendMode,
    ExternalTextureId, ExternalTextureSpec, GradientSpace, LayerFilter, LayerSpec, Paint,
    PaintEffects, StyledQuad,
};
pub use crate::platform::{
    clipboard_has_text, clipboard_read_into, clipboard_write_text, AsyncPlatform,
    AsyncPlatformSurface, DesktopRenderBackend, FlushReport, Platform, PlatformControlFlow,
    PlatformSurface,
};
pub use crate::platform::clipboard_read_text;
pub use crate::pod::{
    derive_scoped_id, ActionScope, ClickEvent, CoreTheme, CursorShape, DragEvent, FrameScope,
    Handle, ImePreedit, InputEvent, KeyCode, KeyModifiers, PodId, PodPool, PointerButton,
    PointerKind, SafeAreaInsets, Scope, SwipeDir,
};
pub use crate::renderers::software::OffscreenCompositor;
pub use crate::renderers::software::{
    blit_translated_layer, compute_sdf_coverage, compute_unoccluded_clip,
    rasterize_parallel_threads, rasterize_region, rasterize_region_parallel, SoftwareRenderer,
};
pub use crate::renderers::{PixelBufferRenderer, RepaintBufferType, Renderer};
pub use crate::sdf::SurfacePainter;
pub use crate::signal::{batch, computed, computed_eq, effect, signal, Computed, Effect, Signal};
pub use crate::solver::Region;
pub use crate::style::{
    px, rgb, rgba, AlignItems, Color, Easing, Edges, FlexDir, FluentBuilder, Justify, Length,
    LoopMode, Style, StyleExt, StyleRefinement, Styled, StyledExt, TextAlign,
};
pub use crate::svg::{
    parse_svg_color, parse_svg_viewbox, SvgFillRule, SvgLineCap, SvgLineJoin, SvgRenderOptions,
    SvgViewBox, Transform2D,
};
