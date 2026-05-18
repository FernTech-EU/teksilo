pub use bastyde_app as app;
pub use bastyde_canvas as canvas;
pub use bastyde_core as core;
pub use bastyde_data as data;
pub use bastyde_platform as platform;
pub use bastyde_settings as settings;
pub use bastyde_tokens as tokens;

// Theme presets (`intui::light()` / `intui::dark()` etc.) live in
// bastyde-core. Re-exported here so examples can call
// `bastyde::presets::intui::light()` without depending on bastyde-core
// directly.
pub use bastyde_core::presets;

/// The `bati!` DSL macro. See `docs/bati-language-spec-v3.md` for the
/// surface language. Re-exported from `bastyde-macros` so consuming
/// crates only need `bastyde` in `[dependencies]`.
pub use bastyde_macros::bati;

/// `#[derive(IntentKind)]` — generates the typed DTO bridge between
/// an app's intent enum and the framework's runtime `Intent`. Each
/// variant declares its intent name via `#[name = "..."]`.
pub use bastyde_macros::IntentKind;

/// Re-export the `res!` macro so consuming crates only need `bastyde`
/// in their `[dependencies]` — same pattern as `serde` re-exporting
/// `serde_derive`.
pub use bastyde_resources::res;

#[cfg(feature = "widgets")]
pub use bastyde_widgets as widgets;

#[cfg(feature = "text")]
pub use bastyde_text as text;

/// Re-export of `text_document` when the `rich-text` feature is
/// enabled, so applications can access the rich document model
/// through the umbrella crate without adding a second direct
/// workspace dependency. `bastyde-text` pulls in `text-document`
/// under its own `rich-text` feature and re-exports it; this line
/// just forwards the re-export one more level up.
#[cfg(feature = "rich-text")]
pub use bastyde_text::text_document;

#[cfg(feature = "i18n")]
pub use bastyde_i18n as i18n;

/// Debug-only in-app inspector. Apps wire it in with one line:
///
/// ```ignore
/// use bastyde::prelude::*;
///
/// BastydeAppBuilder::new()
///     .install_inspector_in_debug()   // no-op in release
///     .initial_window(WindowConfig::new()...)
///     .run();
/// ```
///
/// `BastydeAppBuilderInspectorExt` is also re-exported from
/// [`prelude`] so the umbrella import (`use bastyde::prelude::*;`)
/// makes `install_inspector_in_debug()` callable directly.
#[cfg(feature = "inspector")]
pub use bastyde_inspector as inspector;

/// Toast notification install hook. Apps wire it in one line:
///
/// ```ignore
/// use bastyde::prelude::*;
///
/// BastydeAppBuilder::new()
///     .theme(intui::light())
///     .app_paths(AppPaths::new("com", "FernTech", "MyApp").unwrap())
///     .install_toast_default()
///     .initial_window(WindowConfig::new()...)
///     .run();
/// ```
///
/// The `install_toast(…)` and `install_toast_default()` methods come
/// from [`BastydeAppBuilderToastExt`](toast_install::BastydeAppBuilderToastExt),
/// re-exported through [`prelude`] so the umbrella import makes them
/// callable directly.
#[cfg(feature = "toast")]
pub mod toast_install;

pub mod prelude {
    // DSL entry point
    pub use bastyde_macros::bati;

    // Core widget types
    pub use bastyde_core::{
        AccessNodeBuilder, AccessSubtreeMode, AccessibilityOverrides, Action, AnimationSpec,
        BuildContext, ButtonMask, CursorIcon, EventContext, EventResponse, BatiBranch, BatiBranch3,
        BatiBranch4, FocusPolicy, Intent, IntentKind, IntentResponse, IntoBatiChild,
        IntoBatiCondition, Key, KeyStroke, LayoutContext, LayoutResponse, ModalCloseBehavior,
        ModalPresentation, Modifiers, PaintContext, PointerButton, Prop, Shortcut,
        ShortcutRegistry, ShortcutScope, Signal, TapEvent, Widget, WidgetBuilder, WidgetEvent,
        WidgetId,
    };

    // Geometry (lives in bastyde-canvas)
    pub use bastyde_canvas::{Point, Rect, Size, SizeProposal, Vec2};

    // Canvas and rendering
    pub use bastyde_canvas::{Canvas, EllipsisMode, Paint, Path, RenderFrame, TextOverflow};

    // Tokens
    pub use bastyde_tokens::{BorderRole, Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole};

    // Theme + appearance + extensions live in bastyde-core (so they can
    // co-locate with the per-widget style trait protocols and the typed
    // `Arc<dyn FooStyle>` slots).
    pub use bastyde_core::{Theme, ThemeAppearance, ThemeExtensions};

    // Theme presets — apps explicitly pick one (no Theme::default()):
    //   let theme = intui::light();
    pub use bastyde_core::presets::intui;

    // Sibling preset crates — opt-in alternatives to the bundled
    // IntUI preset. Each is a stub today returning an IntUI-shaped
    // baseline; per-tier customisation lands incrementally as the
    // four-tier styling refactor finishes the remaining widget
    // migrations. Apps opting into a feature get a stable import
    // path (`material3::light()`, `macos::dark()`, …).
    #[cfg(feature = "theme-material3")]
    pub use bastyde_theme_material3 as material3;

    #[cfg(feature = "theme-macos")]
    pub use bastyde_theme_macos as macos;

    #[cfg(feature = "theme-fluent")]
    pub use bastyde_theme_fluent as fluent;

    // Reactive color / style props — unified input types for widget builders.
    pub use bastyde_core::color_prop::{ColorProp, TextStyleProp};

    // App
    pub use bastyde_app::{BastydeAppBuilder, ThemeMode};

    // Settings (persistence layer)
    pub use bastyde_settings::{
        AppPaths, MruEntry, MruList, PerWindowState, SettingsBundle, SettingsExt, SettingsFile,
        SettingsKey, SettingsStore, WindowStateService,
    };

    // Multi-window API
    pub use bastyde_core::{
        DecorationsMode, BastydeWindowId, ModalConfig, UserAttentionKind, WindowCommand, WindowConfig,
        WindowPlacement, WindowState,
    };

    // i18n (architecture §12)
    #[cfg(feature = "i18n")]
    pub use bastyde_i18n::{
        I18nConfig, LanguageIdentifier, LocalizedString, localized, tr, tr_widget,
    };

    // Debug inspector — the extension trait that adds
    // `install_inspector_in_debug()` to `BastydeAppBuilder`. The trait is
    // always present (release builds get a no-op shim); only the
    // re-export is gated so apps that disable the `inspector` feature
    // don't pull in the dep.
    #[cfg(feature = "inspector")]
    pub use bastyde_inspector::BastydeAppBuilderInspectorExt;

    // Toast notification install hook + the public types apps work
    // with. The extension trait adds `install_toast(...)` /
    // `install_toast_default()` to `BastydeAppBuilder`. Public types
    // (`Toast`, `ToastAction`, `ToastSeverity`, `ToastPriority`,
    // `ToastHandle`, `ToastInstallOptions`, `NotificationArchive`,
    // `EventContextToastExt::show_toast`, the log widgets) are
    // re-exported so `use bastyde::prelude::*` brings the entire
    // toast surface into scope.
    #[cfg(feature = "toast")]
    pub use crate::toast_install::BastydeAppBuilderToastExt;
    #[cfg(feature = "toast")]
    pub use bastyde_widgets::{
        EventContextToastExt, NotificationArchive, NotificationArchiveModel,
        NotificationCenterButton, NotificationEntry, NotificationLog, NotificationLogDialog, Toast,
        ToastAction, ToastActionStyle, ToastDismissCause, ToastHandle, ToastHost,
        ToastInstallOptions, ToastPriority, ToastRegistry, ToastSeverity,
    };

    // Native file dialogs. The extension trait brings
    // `ctx.pick_file(...)`, `ctx.save_file(...)`, etc. into scope.
    #[cfg(any(feature = "file-dialog", feature = "file-dialog-trait"))]
    pub use bastyde_platform::file_dialog::{
        EventContextFileDialogExt, FileDialogHandle, FileDialogRequest, FileDialogResult,
    };
}
