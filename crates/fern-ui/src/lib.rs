pub use fern_app as app;
pub use fern_canvas as canvas;
pub use fern_core as core;
pub use fern_data as data;
pub use fern_platform as platform;
pub use fern_tokens as tokens;

/// The `fern!` DSL macro. See `docs/fern-language-spec-v3.md` for the
/// surface language. Re-exported from `fern-ui-macros` so consuming
/// crates only need `fern-ui` in `[dependencies]`.
pub use fern_ui_macros::fern;

/// `#[derive(IntentKind)]` — generates the typed DTO bridge between
/// an app's intent enum and the framework's runtime `Intent`. Each
/// variant declares its intent name via `#[name = "..."]`.
pub use fern_ui_macros::IntentKind;

/// Re-export the `res!` macro so consuming crates only need `fern-ui`
/// in their `[dependencies]` — same pattern as `serde` re-exporting
/// `serde_derive`.
pub use fern_resources::res;

#[cfg(feature = "widgets")]
pub use fern_widgets as widgets;

#[cfg(feature = "text")]
pub use fern_text as text;

/// Re-export of `text_document` when the `rich-text` feature is
/// enabled, so applications can access the rich document model
/// through the umbrella crate without adding a second direct
/// workspace dependency. `fern-text` pulls in `text-document`
/// under its own `rich-text` feature and re-exports it; this line
/// just forwards the re-export one more level up.
#[cfg(feature = "rich-text")]
pub use fern_text::text_document;

#[cfg(feature = "i18n")]
pub use fern_i18n as i18n;

pub mod prelude {
    // DSL entry point
    pub use fern_ui_macros::fern;

    // Core widget types
    pub use fern_core::{
        AccessNodeBuilder, Action, BuildContext, CursorIcon, EventContext, EventResponse,
        FernBranch, FernBranch3, FernBranch4, FocusPolicy, Intent, IntentKind, IntentResponse,
        IntoFernChild, IntoFernCondition, Key, KeyStroke, LayoutContext, ModalCloseBehavior,
        ModalPresentation, Modifiers, PaintContext, Prop, Shortcut, ShortcutRegistry,
        ShortcutScope, Signal, Widget, WidgetBuilder, WidgetEvent, WidgetId,
    };

    // Geometry (lives in fern-canvas)
    pub use fern_canvas::{Point, Rect, Size, SizeProposal, Vec2};

    // Canvas and rendering
    pub use fern_canvas::{Canvas, EllipsisMode, Paint, Path, RenderFrame, TextOverflow};

    // Tokens
    pub use fern_tokens::{
        BorderRole, Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole, Theme,
    };

    // Reactive color / style props — unified input types for widget builders.
    pub use fern_core::color_prop::{ColorProp, TextStyleProp};

    // App
    pub use fern_app::{FernAppBuilder, ThemeMode};

    // Multi-window API
    pub use fern_core::{
        DecorationsMode, FernWindowId, ModalConfig, UserAttentionKind, WindowCommand,
        WindowConfig, WindowPlacement, WindowState,
    };

    // i18n (architecture §12)
    #[cfg(feature = "i18n")]
    pub use fern_i18n::{
        I18nConfig, LanguageIdentifier, LocalizedString, localized, tr, tr_widget,
    };
}
