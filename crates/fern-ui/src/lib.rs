pub use fern_tokens as tokens;
pub use fern_canvas as canvas;
pub use fern_core as core;
pub use fern_app as app;

#[cfg(feature = "widgets")]
pub use fern_widgets as widgets;

#[cfg(feature = "text")]
pub use fern_text as text;

#[cfg(feature = "i18n")]
pub use fern_i18n as i18n;

pub mod prelude {
    // Core widget types
    pub use fern_core::{
        Widget, BuildContext, EventContext, PaintContext, LayoutContext,
        WidgetId, WidgetEvent, EventResponse, FocusPolicy,
        State, DerivedState,
        AppCommand, ShortcutMap, Shortcut, Modifiers, Key,
        AccessNodeBuilder, CursorIcon,
    };
    #[allow(deprecated)]
    pub use fern_core::CompositeWidget;

    // Geometry (lives in fern-canvas)
    pub use fern_canvas::{
        SizeProposal, Size, Rect, Point, Vec2,
    };

    // Canvas and rendering
    pub use fern_canvas::{Canvas, RenderFrame, Path, Paint};

    // Tokens
    pub use fern_tokens::{Theme, Color, CornerRadius};

    // App
    pub use fern_app::FernAppBuilder;
}
