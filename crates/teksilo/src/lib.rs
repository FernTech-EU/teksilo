// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub use teksilo_app as app;
pub use teksilo_canvas as canvas;
pub use teksilo_core as core;
pub use teksilo_data as data;
pub use teksilo_platform as platform;
pub use teksilo_settings as settings;
pub use teksilo_tokens as tokens;

// Theme presets (`intui::light()` / `intui::dark()` etc.) live in
// teksilo-core. Re-exported here so examples can call
// `teksilo::presets::intui::light()` without depending on teksilo-core
// directly.
pub use teksilo_core::presets;

/// The `teksu!` DSL macro. See `docs/teksu-language-spec-v3.md` for the
/// surface language. Re-exported from `teksilo-macros` so consuming
/// crates only need `teksilo` in `[dependencies]`.
pub use teksilo_macros::teksu;

/// `#[derive(IntentKind)]` — generates the typed DTO bridge between
/// an app's intent enum and the framework's runtime `Intent`. Each
/// variant declares its intent name via `#[name = "..."]`.
pub use teksilo_macros::IntentKind;

/// Re-export the `res!` macro so consuming crates only need `teksilo`
/// in their `[dependencies]` — same pattern as `serde` re-exporting
/// `serde_derive`.
pub use teksilo_resources::res;

/// Application developer guide — the curated, source-verified reference for
/// building apps with Teksilo: entry point, the unified [`Widget`](core::Widget)
/// trait, the layout model, `Signal`/`Prop` reactivity, attached event handlers,
/// Actions/Intents/Shortcuts, theming, settings, i18n, the widget catalog, and
/// headless testing. Rendered on docs.rs from `app_guide.md`. The same guide
/// backs the `teksilo-app` Claude Code skill.
pub mod app_guide {
    // The guide is hand-authored Markdown with type notation like `Signal<T>` in
    // prose and repo-relative links; suppress rustdoc lints for the included page
    // so the crate's `-D warnings` doc build stays green.
    #![allow(rustdoc::all)]
    #![doc = include_str!("app_guide.md")]
}

#[cfg(feature = "widgets")]
pub use teksilo_widgets as widgets;

#[cfg(feature = "text")]
pub use teksilo_text as text;

/// Re-export of `text_document`, so applications can access the rich
/// document model through the umbrella crate without adding a second
/// direct workspace dependency. `teksilo-text` depends on
/// `text-document` and re-exports it; this line just forwards the
/// re-export one more level up. Available whenever the `text` feature
/// is enabled (on by default) — `teksilo_text` is a direct dependency
/// of the umbrella only under that feature.
#[cfg(feature = "text")]
pub use teksilo_text::text_document;

#[cfg(feature = "i18n")]
pub use teksilo_i18n as i18n;

/// Optional main-thread async executor. Off by default — enable the `async`
/// feature. Re-exported as `teksilo::async_rt` (the bare name `async` is a
/// reserved keyword). The spawn extension traits are also in the [`prelude`].
#[cfg(feature = "async")]
pub use teksilo_async as async_rt;

/// Debug-only in-app inspector. Apps wire it in with one line:
///
/// ```ignore
/// use teksilo::prelude::*;
///
/// TeksiloAppBuilder::new()
///     .install_inspector_in_debug()   // no-op in release
///     .initial_window(WindowConfig::new()...)
///     .run();
/// ```
///
/// `TeksiloAppBuilderInspectorExt` is also re-exported from
/// [`prelude`] so the umbrella import (`use teksilo::prelude::*;`)
/// makes `install_inspector_in_debug()` callable directly.
#[cfg(feature = "inspector")]
pub use teksilo_inspector as inspector;

/// Toast notification install hook. Apps wire it in one line:
///
/// ```ignore
/// use teksilo::prelude::*;
///
/// TeksiloAppBuilder::new()
///     .theme(intui::light())
///     .app_paths(AppPaths::new("eu", "FernTech", "MyApp").unwrap())
///     .install_toast_default()
///     .initial_window(WindowConfig::new()...)
///     .run();
/// ```
///
/// The `install_toast(…)` and `install_toast_default()` methods come
/// from [`TeksiloAppBuilderToastExt`](toast_install::TeksiloAppBuilderToastExt),
/// re-exported through [`prelude`] so the umbrella import makes them
/// callable directly.
#[cfg(feature = "toast")]
pub mod toast_install;

/// Embeddable [`WebView`](teksilo_webview::WebView) widget. Re-exported as
/// `teksilo::web_view`. The `install_web_view{,_default}` methods come from
/// [`TeksiloAppBuilderWebViewExt`](webview_install::TeksiloAppBuilderWebViewExt),
/// re-exported through [`prelude`]. Present for any web-view feature —
/// engine-backed (`web-view` = wry, `web-view-servo` = +Servo) or
/// `web-view-headless` (no engine).
#[cfg(any(feature = "web-view-headless", feature = "web-view"))]
pub use teksilo_webview as web_view;

/// Embeddable terminal-emulator [`Terminal`](teksilo_terminal::Terminal)
/// widget. Re-exported as `teksilo::terminal`. The view is self-contained (it
/// renders into the wgpu surface and needs no app wiring); the PTY + VT model
/// are the default `portable-pty` + `alacritty_terminal` engine.
#[cfg(feature = "terminal")]
pub use teksilo_terminal as terminal;

/// WebView install hook (extension trait on `TeksiloAppBuilder`).
#[cfg(any(feature = "web-view-headless", feature = "web-view"))]
pub mod webview_install;

/// GUI-free runtime-introspection & automation toolkit
/// ([`teksilo_automation`]). Re-exported as `teksilo::automation`. The
/// `install_automation_bridge_in_debug()` method comes from
/// [`TeksiloAppBuilderAutomationExt`](automation_install::TeksiloAppBuilderAutomationExt),
/// re-exported through [`prelude`].
#[cfg(feature = "automation")]
pub use teksilo_automation as automation;

/// Automation bridge install hook (extension trait on `TeksiloAppBuilder`).
#[cfg(feature = "automation")]
pub mod automation_install;

pub mod prelude {
    // DSL entry point
    pub use teksilo_macros::teksu;

    // Core widget types
    pub use teksilo_core::{
        AccessNodeBuilder, AccessSubtreeMode, AccessibilityOverrides, Action, AnimationSpec,
        BuildContext, ButtonMask, CursorIcon, EventContext, EventResponse, ImeContext, ImePurpose,
        Intent, IntentKind, IntentResponse, IntoTeksiChild, IntoTeksiCondition, Key, KeyStroke,
        LayoutContext, LayoutResponse, ModalCloseBehavior, ModalPresentation, Modifiers,
        OverscrollBehavior, PaintContext, PointerButton, Prop, Shortcut, ShortcutRegistry,
        ShortcutScope, Signal, TapEvent, TeksiBranch, TeksiBranch3, TeksiBranch4,
        TraversalScopePolicy, Widget, WidgetBuilder, WidgetEvent, WidgetId,
    };

    // Window-active appearance: the opt-in per-widget dim wrapper. The
    // automatic layers (caret hide, selection desaturation) need no import;
    // `.dim_when_inactive(..)` rides in via `WidgetBuilder` above.
    pub use teksilo_core::dim_when_inactive::DimWhenInactive;

    // Geometry (lives in teksilo-canvas)
    pub use teksilo_canvas::{Point, Rect, Size, SizeProposal, Vec2};

    // Canvas and rendering
    pub use teksilo_canvas::{Canvas, EllipsisMode, Paint, Path, RenderFrame, TextOverflow};

    // Tokens
    pub use teksilo_tokens::{
        BorderRole, Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole,
    };

    // Theme + appearance + extensions live in teksilo-core (so they can
    // co-locate with the per-widget style trait protocols and the typed
    // `Arc<dyn FooStyle>` slots).
    pub use teksilo_core::{Theme, ThemeAppearance, ThemeExtensions, ThemeId};

    // Theme presets — apps explicitly pick one (no Theme::default()):
    //   let theme = intui::light();
    pub use teksilo_core::presets::intui;

    // Sibling preset crates — opt-in alternatives to the bundled IntUI
    // preset, each behind its own Cargo feature and reachable by a stable
    // path (`material3::light()`, `fluent::dark()`, …). `material3` and
    // `fluent` are complete design languages (tokens + Tier-3 widget
    // chrome); `macos` is still an IntUI-shaped baseline.
    #[cfg(feature = "theme-material3")]
    pub use teksilo_theme_material3 as material3;

    #[cfg(feature = "theme-macos")]
    pub use teksilo_theme_macos as macos;

    #[cfg(feature = "theme-fluent")]
    pub use teksilo_theme_fluent as fluent;

    // Reactive color / style props — unified input types for widget builders.
    pub use teksilo_core::color_prop::{ColorProp, TextStyleProp};

    // App
    pub use teksilo_app::{TeksiloAppBuilder, ThemeMode};

    // Settings (persistence layer)
    pub use teksilo_settings::{
        AppPaths, MruEntry, MruList, PerWindowState, SettingsBundle, SettingsExt, SettingsFile,
        SettingsKey, SettingsStore, TEXT_SCALE_KEY, WindowStateService,
    };

    // Multi-window API
    pub use teksilo_core::{
        CloseResponse, DecorationsMode, ModalConfig, SizeToContent, TeksiloWindowId,
        UserAttentionKind, WindowCommand, WindowConfig, WindowPlacement, WindowRemovedCallback,
        WindowRemovedEvent, WindowState,
    };

    // i18n (architecture §12)
    #[cfg(feature = "i18n")]
    pub use teksilo_i18n::{
        I18nConfig, LanguageIdentifier, LocalizedString, lit, localized, tr, tr_widget,
    };

    // Debug inspector — the extension trait that adds
    // `install_inspector_in_debug()` to `TeksiloAppBuilder`. The trait is
    // always present (release builds get a no-op shim); only the
    // re-export is gated so apps that disable the `inspector` feature
    // don't pull in the dep.
    #[cfg(feature = "inspector")]
    pub use teksilo_inspector::TeksiloAppBuilderInspectorExt;

    // Toast notification install hook + the public types apps work
    // with. The extension trait adds `install_toast(...)` /
    // `install_toast_default()` to `TeksiloAppBuilder`. Public types
    // (`Toast`, `ToastAction`, `ToastSeverity`, `ToastPriority`,
    // `ToastHandle`, `ToastInstallOptions`, `NotificationArchive`,
    // `EventContextToastExt::show_toast`, the log widgets) are
    // re-exported so `use teksilo::prelude::*` brings the entire
    // toast surface into scope.
    #[cfg(feature = "toast")]
    pub use crate::toast_install::TeksiloAppBuilderToastExt;
    #[cfg(feature = "toast")]
    pub use teksilo_widgets::{
        EventContextToastExt, NotificationArchive, NotificationArchiveModel,
        NotificationCenterButton, NotificationEntry, NotificationLog, NotificationLogDialog, Toast,
        ToastAction, ToastActionStyle, ToastAudience, ToastDismissCause, ToastHandle, ToastHost,
        ToastInstallOptions, ToastPriority, ToastRegistry, ToastRoute, ToastSeverity,
    };

    // WebView. The extension trait adds `install_web_view{,_default}()` to
    // `TeksiloAppBuilder`; the `WebView` widget + its public backend surface
    // come into scope so apps `use teksilo::prelude::*` and embed web content.
    #[cfg(any(feature = "web-view-headless", feature = "web-view"))]
    pub use crate::webview_install::TeksiloAppBuilderWebViewExt;
    #[cfg(any(feature = "web-view-headless", feature = "web-view"))]
    pub use teksilo_webview::{
        WebSource, WebView, WebViewBackend, WebViewEvent, WebViewHandle, WebViewId,
        WebViewRegistry, WebViewStyle,
    };

    // Terminal emulator (Console). The `Terminal` widget + its controller,
    // colour scheme, command and style types come into scope so apps
    // `use teksilo::prelude::*` and embed a shell.
    #[cfg(feature = "terminal")]
    pub use teksilo_terminal::{
        BellStyle, ColorScheme, CursorStyle, Terminal, TerminalClosePolicy, TerminalCommand,
        TerminalController, TerminalStyle,
    };

    // Automation bridge (debug-only). The extension trait adds
    // `install_automation_bridge_in_debug()` to `TeksiloAppBuilder`.
    #[cfg(feature = "automation")]
    pub use crate::automation_install::TeksiloAppBuilderAutomationExt;

    // Native file dialogs. The extension trait brings
    // `ctx.pick_file(...)`, `ctx.save_file(...)`, etc. into scope.
    #[cfg(any(feature = "file-dialog", feature = "file-dialog-trait"))]
    pub use teksilo_platform::file_dialog::{
        EventContextFileDialogExt, FileDialogHandle, FileDialogRequest, FileDialogResult,
    };

    // Optional async executor: the install hook, the `ctx.spawn_local(...)`
    // extension trait, and the `spawn_blocking` offload helper.
    #[cfg(feature = "async")]
    pub use teksilo_async::{
        AsyncRuntimeHandle, BlockingError, EventContextAsyncExt, TaskHandle,
        TeksiloAppBuilderAsyncExt, spawn_blocking,
    };

    // Reactor adapters — install hooks for awaiting native runtime futures.
    #[cfg(feature = "async-std")]
    pub use teksilo_async_std::TeksiloAppBuilderAsyncStdExt;
    #[cfg(feature = "tokio")]
    pub use teksilo_tokio::{TeksiloAppBuilderTokioExt, TokioHandle};
}
