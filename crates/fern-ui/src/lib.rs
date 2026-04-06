pub use fern_app as app;
pub use fern_canvas as canvas;
pub use fern_core as core;
pub use fern_tokens as tokens;

#[cfg(feature = "widgets")]
pub use fern_widgets as widgets;

#[cfg(feature = "text")]
pub use fern_text as text;

#[cfg(feature = "i18n")]
pub use fern_i18n as i18n;

pub mod prelude {
    // Core widget types
    pub use fern_core::{
        AccessNodeBuilder, AppCommand, BuildContext, CursorIcon, DerivedState, EventContext,
        EventResponse, FocusPolicy, Key, LayoutContext, Modifiers, PaintContext, Shortcut,
        ShortcutMap, State, Widget, WidgetEvent, WidgetId,
    };

    // Geometry (lives in fern-canvas)
    pub use fern_canvas::{Point, Rect, Size, SizeProposal, Vec2};

    // Canvas and rendering
    pub use fern_canvas::{Canvas, Paint, Path, RenderFrame};

    // Tokens
    pub use fern_tokens::{Color, CornerRadius, Theme};

    // App
    pub use fern_app::FernAppBuilder;
}
