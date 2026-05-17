pub use fern_app as app;
pub use fern_canvas as canvas;
pub use fern_core as core;
pub use fern_data as data;
pub use fern_platform as platform;
pub use fern_settings as settings;
pub use fern_tokens as tokens;

// Theme presets (`intui::light()` / `intui::dark()` etc.) live in
// fern-core. Re-exported here so examples can call
// `fern_ui::presets::intui::light()` without depending on fern-core
// directly.
pub use fern_core::presets;

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

/// Debug-only in-app inspector. Apps wire it in with one line:
///
/// ```ignore
/// use fern_ui::prelude::*;
///
/// FernAppBuilder::new()
///     .install_inspector_in_debug()   // no-op in release
///     .initial_window(WindowConfig::new()...)
///     .run();
/// ```
///
/// `FernAppBuilderInspectorExt` is also re-exported from
/// [`prelude`] so the umbrella import (`use fern_ui::prelude::*;`)
/// makes `install_inspector_in_debug()` callable directly.
#[cfg(feature = "inspector")]
pub use fern_inspector as inspector;

/// Toast notification install hook. Apps wire it in one line:
///
/// ```ignore
/// use fern_ui::prelude::*;
///
/// FernAppBuilder::new()
///     .theme(intui::light())
///     .app_paths(AppPaths::new("com", "FernTech", "MyApp").unwrap())
///     .install_toast_default()
///     .initial_window(WindowConfig::new()...)
///     .run();
/// ```
///
/// The `install_toast(…)` and `install_toast_default()` methods come
/// from [`FernAppBuilderToastExt`](toast_install::FernAppBuilderToastExt),
/// re-exported through [`prelude`] so the umbrella import makes them
/// callable directly. See `docs/plans/widgets-plan.md §3.9` and
/// `.claude/plans/plan-for-the-creation-clever-stearns.md` for the
/// full design.
#[cfg(feature = "toast")]
pub mod toast_install;

pub mod prelude {
    // DSL entry point
    pub use fern_ui_macros::fern;

    // Core widget types
    pub use fern_core::{
        AccessNodeBuilder, AccessSubtreeMode, AccessibilityOverrides, Action, AnimationSpec,
        BuildContext, ButtonMask, CursorIcon, EventContext, EventResponse, FernBranch, FernBranch3,
        FernBranch4, FocusPolicy, Intent, IntentKind, IntentResponse, IntoFernChild,
        IntoFernCondition, Key, KeyStroke, LayoutContext, LayoutResponse, ModalCloseBehavior,
        ModalPresentation, Modifiers, PaintContext, PointerButton, Prop, Shortcut,
        ShortcutRegistry, ShortcutScope, Signal, TapEvent, Widget, WidgetBuilder, WidgetEvent,
        WidgetId,
    };

    // Geometry (lives in fern-canvas)
    pub use fern_canvas::{Point, Rect, Size, SizeProposal, Vec2};

    // Canvas and rendering
    pub use fern_canvas::{Canvas, EllipsisMode, Paint, Path, RenderFrame, TextOverflow};

    // Tokens
    pub use fern_tokens::{BorderRole, Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole};

    // Theme + appearance + extensions live in fern-core (so they can
    // co-locate with the per-widget style trait protocols and the typed
    // `Arc<dyn FooStyle>` slots).
    pub use fern_core::{Theme, ThemeAppearance, ThemeExtensions};

    // Theme presets — apps explicitly pick one (no Theme::default()):
    //   let theme = intui::light();
    pub use fern_core::presets::intui;

    // Sibling preset crates — opt-in alternatives to the bundled
    // IntUI preset. Each is a stub today returning an IntUI-shaped
    // baseline; per-tier customisation lands incrementally as the
    // four-tier styling refactor finishes the remaining widget
    // migrations. Apps opting into a feature get a stable import
    // path (`material3::light()`, `macos::dark()`, …).
    #[cfg(feature = "theme-material3")]
    pub use fern_theme_material3 as material3;

    #[cfg(feature = "theme-macos")]
    pub use fern_theme_macos as macos;

    #[cfg(feature = "theme-fluent")]
    pub use fern_theme_fluent as fluent;

    // Reactive color / style props — unified input types for widget builders.
    pub use fern_core::color_prop::{ColorProp, TextStyleProp};

    // App
    pub use fern_app::{FernAppBuilder, ThemeMode};

    // Settings (persistence layer)
    pub use fern_settings::{
        AppPaths, MruEntry, MruList, PerWindowState, SettingsBundle, SettingsExt, SettingsFile,
        SettingsKey, SettingsStore, WindowStateService,
    };

    // Multi-window API
    pub use fern_core::{
        DecorationsMode, FernWindowId, ModalConfig, UserAttentionKind, WindowCommand, WindowConfig,
        WindowPlacement, WindowState,
    };

    // i18n (architecture §12)
    #[cfg(feature = "i18n")]
    pub use fern_i18n::{
        I18nConfig, LanguageIdentifier, LocalizedString, localized, tr, tr_widget,
    };

    // Debug inspector — the extension trait that adds
    // `install_inspector_in_debug()` to `FernAppBuilder`. The trait is
    // always present (release builds get a no-op shim); only the
    // re-export is gated so apps that disable the `inspector` feature
    // don't pull in the dep.
    #[cfg(feature = "inspector")]
    pub use fern_inspector::FernAppBuilderInspectorExt;

    // Toast notification install hook + the public types apps work
    // with. The extension trait adds `install_toast(...)` /
    // `install_toast_default()` to `FernAppBuilder`. Public types
    // (`Toast`, `ToastAction`, `ToastSeverity`, `ToastPriority`,
    // `ToastHandle`, `ToastInstallOptions`, `NotificationArchive`,
    // `EventContextToastExt::show_toast`, the log widgets) are
    // re-exported so `use fern_ui::prelude::*` brings the entire
    // toast surface into scope.
    #[cfg(feature = "toast")]
    pub use crate::toast_install::FernAppBuilderToastExt;
    #[cfg(feature = "toast")]
    pub use fern_widgets::{
        EventContextToastExt, NotificationArchive, NotificationArchiveModel,
        NotificationCenterButton, NotificationEntry, NotificationLog, NotificationLogDialog, Toast,
        ToastAction, ToastActionStyle, ToastDismissCause, ToastHandle, ToastHost,
        ToastInstallOptions, ToastPriority, ToastRegistry, ToastSeverity,
    };

    // Native file dialogs. The extension trait brings
    // `ctx.pick_file(...)`, `ctx.save_file(...)`, etc. into scope.
    #[cfg(any(feature = "file-dialog", feature = "file-dialog-trait"))]
    pub use fern_platform::file_dialog::{
        EventContextFileDialogExt, FileDialogHandle, FileDialogRequest, FileDialogResult,
    };
}
