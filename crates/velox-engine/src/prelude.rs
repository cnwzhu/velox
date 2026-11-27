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
pub use crate::automation::{
    compute_engine_frame_checksum, AutomationResult, AutomationSession, EventRecorder,
    EventReplayer, RecordedEvent, RecordedStep, ReplayReport, SemanticAction, SemanticQuery,
};
pub use crate::damage::DamageTracker;
pub use crate::devtools::{
    build_inspector_overlay, build_live_devtools_panel, capture_telemetry, FrameBudgetRing,
    FrameTelemetry, InspectorConfig, LiveInspectorState, LiveStyleOverrides, StyleOverride,
};
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
pub use crate::keymap::{ActionId, KeyBinding, KeyChord, KeyStroke, Keymap, KeymapMatch};
pub use crate::media::{
    convert_i420_to_rgba8, convert_nv12_to_rgba8, VideoPixelFormat, VideoSurfaceSpec,
    WebViewHostRegion, WebViewHostSync, WebViewHostUpdate,
};
pub use crate::motion::{
    DrawOp, DrawPayload, MotionCache, MotionState, Presence, Spring, SpringConfig, Tween,
};
pub use crate::paint::{
    BackdropFilter, BlendMode, ColorMatrix, ColorStop, CornerRadii, DropShadow, ExtendMode,
    ExternalTextureId, ExternalTextureSpec, GradientSpace, LayerFilter, LayerSpec, LayoutExtras,
    Paint, PaintEffects, StyledQuad,
};
pub use crate::platform::{
    clipboard_has_text, clipboard_read_into, clipboard_write_text, AsyncPlatform,
    AsyncPlatformSurface, DesktopRenderBackend, FlushReport, Platform, PlatformControlFlow,
    PlatformSurface,
};
pub use crate::platform::clipboard::{
    clipboard_read_payload, clipboard_read_text, clipboard_write_payload, ClipboardImage,
    ClipboardPayload,
};
pub use crate::platform::os_integration::{
    run_file_dialog, set_mock_file_dialog_handler, DragPreviewGhost, FileDialogMode,
    FileDialogRequest, FileDialogResult, FileFilter, NativeMenuBar, NativeMenuItem,
    NativeMenuSection, SystemTraySpec,
};
pub use crate::platform::winit::{DetachedDockPanel, DockZone};
pub use crate::pod::{
    derive_scoped_id, ActionScope, ClickEvent, CoreTheme, CursorShape, DragEvent, DragPayload,
    EventPhase, FrameScope, Handle, ImePreedit, InputEvent, KeyCode, KeyModifiers, PinchEvent,
    PodId, PodPool, PointerButton, PointerKind, SafeAreaInsets, Scope, SwipeDir, WheelEvent,
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
pub use crate::text_rich::{
    classify_script, contains_rtl, inline_flow, is_emoji_char, is_rtl_char, measure_rich_text_box,
    mirror_bidi_char, reorder_bidi_visual, rich_text, shape_text_run, split_script_runs, BidiClass,
    BidiVisualRun, FontFallbackChain, FontFamily, FontSpec, FontStyle as RichFontStyle,
    FontWeight as RichFontWeight, InlineFlow, RichText, RichTextSpec, ScriptKind, ScriptRun,
    ShapedGlyph, TextDecoration as RichTextDecoration, TextSpan,
};
pub use crate::image_pipeline::{
    compute_object_fit, decoded_image, map_nine_slice_coord, DecodedImage, DecodedImageSpec,
    ImageAssetId, NineSliceInsets, ObjectFit,
};
pub use crate::image_pipeline::{
    decode_image_bytes, detect_image_format, get_cached_image, image_load_state,
    load_image_bytes, load_image_bytes_async, register_custom_image_decoder, register_image_rgba,
    set_global_image_cache_budget, DecodedPixels, ImageFormat, ImageLoadState, LruImageCache,
};
pub use crate::display::{
    register_font_bytes, register_font_vec, registered_custom_font_count,
};
pub use crate::vector_canvas::{
    rasterize_vector_spec, vector_canvas, VecPoint, VectorCanvas, VectorDrawSpec, VectorPrimitive,
};
#[cfg(feature = "custom-shader")]
pub use crate::renderers::wgpu::{
    register_custom_shader, unregister_custom_shader, CustomShaderPrimitive, CustomShaderViewport,
};
pub use crate::canvas_scene::{
    rasterize_scene_canvas, scene_canvas, CanvasAsciiLabel, CanvasItemState, CanvasMeshVertex,
    CanvasPrimitiveKind, CanvasStaticGeom, CustomShaderCall, CustomShaderId, CustomShaderLayer,
    SceneCanvas, SceneCanvasSpec, StaticSceneId, Viewport2D, CANVAS_STATE_DISABLED,
    CANVAS_STATE_FOCUSED, CANVAS_STATE_HOVERED, CANVAS_STATE_LOCKED, CANVAS_STATE_SELECTED,
};
