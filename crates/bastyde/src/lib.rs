// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

/// Application developer guide — the curated, source-verified reference for
/// building apps with Bastyde: entry point, the unified [`Widget`](core::Widget)
/// trait, the layout model, `Signal`/`Prop` reactivity, attached event handlers,
/// Actions/Intents/Shortcuts, theming, settings, i18n, the widget catalog, and
/// headless testing. Rendered on docs.rs from `app_guide.md`. The same guide
/// backs the `bastyde-app` Claude Code skill.
pub mod app_guide {
    // The guide is hand-authored Markdown with type notation like `Signal<T>` in
    // prose and repo-relative links; suppress rustdoc lints for the included page
    // so the crate's `-D warnings` doc build stays green.
    #![allow(rustdoc::all)]
    #![doc = include_str!("app_guide.md")]
}

#[cfg(feature = "widgets")]
pub use bastyde_widgets as widgets;

#[cfg(feature = "text")]
pub use bastyde_text as text;

/// Re-export of `text_document`, so applications can access the rich
/// document model through the umbrella crate without adding a second
/// direct workspace dependency. `bastyde-text` depends on
/// `text-document` and re-exports it; this line just forwards the
/// re-export one more level up. Available whenever the `text` feature
/// is enabled (on by default) — `bastyde_text` is a direct dependency
/// of the umbrella only under that feature.
#[cfg(feature = "text")]
pub use bastyde_text::text_document;

#[cfg(feature = "i18n")]
pub use bastyde_i18n as i18n;

/// Optional main-thread async executor. Off by default — enable the `async`
/// feature. Re-exported as `bastyde::async_rt` (the bare name `async` is a
/// reserved keyword). The spawn extension traits are also in the [`prelude`].
#[cfg(feature = "async")]
pub use bastyde_async as async_rt;

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
///     .app_paths(AppPaths::new("eu", "FernTech", "MyApp").unwrap())
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

/// Embeddable [`WebView`](bastyde_webview::WebView) widget. Re-exported as
/// `bastyde::web_view`. The `install_web_view{,_default}` methods come from
/// [`BastydeAppBuilderWebViewExt`](webview_install::BastydeAppBuilderWebViewExt),
/// re-exported through [`prelude`]. Present for any web-view feature —
/// engine-backed (`web-view` = wry, `web-view-servo` = +Servo) or
/// `web-view-headless` (no engine).
#[cfg(any(feature = "web-view-headless", feature = "web-view"))]
pub use bastyde_webview as web_view;

/// WebView install hook (extension trait on `BastydeAppBuilder`).
#[cfg(any(feature = "web-view-headless", feature = "web-view"))]
pub mod webview_install;

/// GUI-free runtime-introspection & automation toolkit
/// ([`bastyde_automation`]). Re-exported as `bastyde::automation`. The
/// `install_automation_bridge_in_debug()` method comes from
/// [`BastydeAppBuilderAutomationExt`](automation_install::BastydeAppBuilderAutomationExt),
/// re-exported through [`prelude`].
#[cfg(feature = "automation")]
pub use bastyde_automation as automation;

/// Automation bridge install hook (extension trait on `BastydeAppBuilder`).
#[cfg(feature = "automation")]
pub mod automation_install;

pub mod prelude {
    // DSL entry point
    pub use bastyde_macros::bati;

    // Core widget types
    pub use bastyde_core::{
        AccessNodeBuilder, AccessSubtreeMode, AccessibilityOverrides, Action, AnimationSpec,
        BatiBranch, BatiBranch3, BatiBranch4, BuildContext, ButtonMask, CursorIcon, EventContext,
        EventResponse, ImeContext, ImePurpose, Intent, IntentKind, IntentResponse, IntoBatiChild,
        IntoBatiCondition, Key, KeyStroke, LayoutContext, LayoutResponse, ModalCloseBehavior,
        ModalPresentation, Modifiers, OverscrollBehavior, PaintContext, PointerButton, Prop,
        Shortcut, ShortcutRegistry, ShortcutScope, Signal, TapEvent, TraversalScopePolicy, Widget,
        WidgetBuilder, WidgetEvent, WidgetId,
    };

    // Geometry (lives in bastyde-canvas)
    pub use bastyde_canvas::{Point, Rect, Size, SizeProposal, Vec2};

    // Canvas and rendering
    pub use bastyde_canvas::{Canvas, EllipsisMode, Paint, Path, RenderFrame, TextOverflow};

    // Tokens
    pub use bastyde_tokens::{
        BorderRole, Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole,
    };

    // Theme + appearance + extensions live in bastyde-core (so they can
    // co-locate with the per-widget style trait protocols and the typed
    // `Arc<dyn FooStyle>` slots).
    pub use bastyde_core::{Theme, ThemeAppearance, ThemeExtensions, ThemeId};

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
        SettingsKey, SettingsStore, TEXT_SCALE_KEY, WindowStateService,
    };

    // Multi-window API
    pub use bastyde_core::{
        BastydeWindowId, CloseResponse, DecorationsMode, ModalConfig, UserAttentionKind,
        WindowCommand, WindowConfig, WindowPlacement, WindowState,
    };

    // i18n (architecture §12)
    #[cfg(feature = "i18n")]
    pub use bastyde_i18n::{
        I18nConfig, LanguageIdentifier, LocalizedString, lit, localized, tr, tr_widget,
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

    // WebView. The extension trait adds `install_web_view{,_default}()` to
    // `BastydeAppBuilder`; the `WebView` widget + its public backend surface
    // come into scope so apps `use bastyde::prelude::*` and embed web content.
    #[cfg(any(feature = "web-view-headless", feature = "web-view"))]
    pub use crate::webview_install::BastydeAppBuilderWebViewExt;
    #[cfg(any(feature = "web-view-headless", feature = "web-view"))]
    pub use bastyde_webview::{
        WebSource, WebView, WebViewBackend, WebViewEvent, WebViewHandle, WebViewId,
        WebViewRegistry, WebViewStyle,
    };

    // Automation bridge (debug-only). The extension trait adds
    // `install_automation_bridge_in_debug()` to `BastydeAppBuilder`.
    #[cfg(feature = "automation")]
    pub use crate::automation_install::BastydeAppBuilderAutomationExt;

    // Native file dialogs. The extension trait brings
    // `ctx.pick_file(...)`, `ctx.save_file(...)`, etc. into scope.
    #[cfg(any(feature = "file-dialog", feature = "file-dialog-trait"))]
    pub use bastyde_platform::file_dialog::{
        EventContextFileDialogExt, FileDialogHandle, FileDialogRequest, FileDialogResult,
    };

    // Optional async executor: the install hook, the `ctx.spawn_local(...)`
    // extension trait, and the `spawn_blocking` offload helper.
    #[cfg(feature = "async")]
    pub use bastyde_async::{
        AsyncRuntimeHandle, BastydeAppBuilderAsyncExt, BlockingError, EventContextAsyncExt,
        TaskHandle, spawn_blocking,
    };

    // Reactor adapters — install hooks for awaiting native runtime futures.
    #[cfg(feature = "async-std")]
    pub use bastyde_async_std::BastydeAppBuilderAsyncStdExt;
    #[cfg(feature = "tokio")]
    pub use bastyde_tokio::{BastydeAppBuilderTokioExt, TokioHandle};
}
