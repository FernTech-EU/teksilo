// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `install_toast` — wire the Toast system into a `TeksiloAppBuilder`.
//!
//! Mirrors the `TeksiloAppBuilderInspectorExt` pattern (see
//! [`teksilo_inspector`]) — the extension trait declaration and its
//! impl on `TeksiloAppBuilder` live in the same crate (`teksilo`),
//! which depends on both `teksilo-app` and `teksilo-widgets`, so the
//! orphan rule is satisfied.
//!
//! Apps wire it in one line:
//!
//! ```ignore
//! use teksilo::prelude::*;
//!
//! TeksiloAppBuilder::new()
//!     .theme(intui::light())
//!     .app_paths(AppPaths::new("eu", "FernTech", "MyApp").unwrap())
//!     .install_toast_default()                      // ← the install
//!     .initial_window(WindowConfig::new()
//!         .root(|tree, _state| tree.add(MyRoot::new())))
//!     .run();
//! ```
//!
//! Internally the install:
//!
//! 1. Opens the `NotificationArchiveModel` from
//!    `options.archive` (resolving the persistent path through
//!    `configured_app_paths()` if `Persistent`).
//! 2. Constructs a `ToastRegistry` bound to the archive.
//! 3. Registers `Rc<NotificationArchiveModel>` + `ToastRegistry`
//!    into `app_state`. `NotificationLog` / `NotificationCenterButton`
//!    consume the archive handle; `EventContextToastExt::show_toast`
//!    consumes the registry.
//! 4. Registers a `DefaultPostRoot` hook that wraps every window's
//!    root with a `ZStack` of `[user_root, ToastHost]`. The
//!    `DefaultPostRoot` fires for every window the app opens (initial
//!    + runtime-opened) so the host installs everywhere automatically.

use std::rc::Rc;

use teksilo_app::{DefaultPostRoot, TeksiloAppBuilder};
use teksilo_core::app_event::AppEvent;
use teksilo_widgets::notification::{NotificationArchive, NotificationArchiveModel};
use teksilo_widgets::primitives::{Expand, ZStack};
use teksilo_widgets::toast::{ToastHost, ToastInstallOptions, ToastRegistry};

/// Extension trait on [`TeksiloAppBuilder`] that wires up the Toast
/// system. The two methods are no-ops without the `toast` feature
/// on `teksilo`; with the feature on (the default), they install
/// the registry + archive + per-window `ToastHost` wrapper.
pub trait TeksiloAppBuilderToastExt {
    /// Install the Toast subsystem with the given options.
    ///
    /// # Panics
    ///
    /// Panics if `options.archive` is `Some(NotificationArchive::Persistent { … })`
    /// but no [`AppPaths`](teksilo_settings::AppPaths) was configured
    /// on the builder first. Use one of:
    /// - `.app_paths(AppPaths::new("qual", "org", "app").unwrap())`
    /// - `.application("qual", "org", "app")`
    /// - explicitly override to `NotificationArchive::in_memory()`
    ///   or `None` for tests / sandboxed builds.
    fn install_toast(self, options: ToastInstallOptions) -> Self;

    /// Convenience: install with [`ToastInstallOptions::default()`]
    /// (bottom-trailing corner, persistent archive at
    /// `<config>/notifications.toml`).
    fn install_toast_default(self) -> Self;
}

impl TeksiloAppBuilderToastExt for TeksiloAppBuilder {
    fn install_toast(self, options: ToastInstallOptions) -> Self {
        // 1. Open the archive (if configured) — resolves persistent
        //    file paths through the configured `AppPaths`.
        let archive: Option<Rc<NotificationArchiveModel>> = match &options.archive {
            None => None,
            Some(NotificationArchive::InMemory { .. }) => Some(Rc::new(
                NotificationArchiveModel::open(
                    options.archive.as_ref().unwrap(),
                    // InMemory doesn't read paths; pass an empty test
                    // AppPaths to avoid an unwrap for the simple case.
                    &teksilo_settings::AppPaths::for_testing(std::path::Path::new("")),
                    std::time::Duration::from_millis(0),
                )
                .expect("in-memory archive open never fails"),
            )),
            Some(NotificationArchive::Persistent { .. }) => {
                let paths = self.configured_app_paths().cloned().expect(
                    "install_toast(Persistent) requires app_paths() (or application(...)) to be \
                     set on the builder first. For tests / sandboxed builds, override to \
                     `NotificationArchive::in_memory()` or `None`.",
                );
                Some(Rc::new(
                    NotificationArchiveModel::open(
                        options.archive.as_ref().unwrap(),
                        &paths,
                        // The framework's standard settings debounce
                        // matches `teksilo_settings::DEFAULT_DEBOUNCE`;
                        // archives use the same so writes coalesce
                        // with other settings writes.
                        teksilo_settings::DEFAULT_DEBOUNCE,
                    )
                    .expect("notification archive: file open failed"),
                ))
            }
        };

        // 2. Build the shared `ToastRegistry` (with or without an
        //    archive handle attached).
        let registry = match archive.clone() {
            Some(a) => ToastRegistry::with_archive(options.clone(), a),
            None => ToastRegistry::new(options.clone()),
        };

        // 3. The DefaultPostRoot closure runs once per window after
        //    the user's root_builder returns. It wraps `user_root`
        //    in `ZStack { user_root, ToastHost::new(registry.clone(),
        //    options.clone()) }`. ZStack hit-tests prefer later
        //    children, so toast surfaces correctly catch clicks
        //    that land inside their bounds.
        let registry_for_hook = registry.clone();
        let options_for_hook = options.clone();
        let post_root = DefaultPostRoot::new(move |tree, root_id| {
            let host = ToastHost::new(registry_for_hook.clone(), options_for_hook.clone());
            let host_id = tree.add(host);
            // The framework force-fills a *window root* to the window
            // bounds, but inside this ZStack the user's root becomes a
            // child sized to its own `layout_response`. A non-flex root
            // (a plain `VStack`, the common case) reports its content
            // height, so it would collapse to the top of the window and
            // leave the lower area blank. Wrap it in an `Expand` so it
            // fills the ZStack exactly as it filled the window when it
            // was the bare root — the wrapping stays layout-transparent.
            let filled_root = tree.add(Expand::new().respect_intrinsic().child_id(root_id));
            let stack = ZStack::new().add_child(filled_root).add_child(host_id);
            tree.add(stack)
        });

        // 3b. F3: turn a permanently-discarded `teksilo-settings` write
        //     (`AppEvent::SettingsWriteFailed`) into a persistent error
        //     toast. This is the one place in the framework that sees
        //     both `teksilo-app`'s `AppEvent` and `teksilo-widgets`'
        //     `Toast`/`ToastRegistry`, so every app that installs toast
        //     gets this automatically — no per-app wiring, no
        //     application-specific special case. `register_app_event_observer`
        //     composes (see its doc comment on `TeksiloAppBuilder`), so
        //     this coexists with the app's own `on_app_event` handler and
        //     any other extension's observer regardless of install order.
        let registry_for_write_failure = registry.clone();
        let write_failure_observer = move |event: &AppEvent| {
            if let AppEvent::SettingsWriteFailed {
                path,
                attempts,
                dropped_patches,
                message,
            } = event
            {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                registry_for_write_failure.show_settings_write_failed(
                    &file_name,
                    *attempts,
                    *dropped_patches,
                    message,
                );
            }
        };

        // 4. Register everything in app_state.
        let mut builder = self
            .app_state(registry)
            .register_app_event_observer(write_failure_observer);
        if let Some(a) = archive {
            builder = builder.app_state(a);
        }
        // Compose (don't clobber) the app-wide post-root chain, so the
        // toast host coexists with the inspector shell or any other
        // post-root chrome regardless of install order.
        builder.register_post_root(post_root)
    }

    fn install_toast_default(self) -> Self {
        self.install_toast(ToastInstallOptions::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_widgets::notification::NotificationArchive;

    #[test]
    #[should_panic(expected = "app_paths() (or application(...)) to be set")]
    fn install_persistent_without_app_paths_panics_with_helpful_message() {
        let opts = ToastInstallOptions {
            archive: Some(NotificationArchive::persistent("notif_test_panic")),
            ..ToastInstallOptions::default()
        };
        let _builder = TeksiloAppBuilder::new().install_toast(opts);
    }

    #[test]
    fn settings_write_failed_event_reaching_the_observer_enqueues_a_toast() {
        // F3 end-to-end, exercised at the one layer that can see both
        // sides of the join: `install_toast` registers an
        // `AppEvent`-observer (via `register_app_event_observer`) that
        // turns `AppEvent::SettingsWriteFailed` into a toast in the
        // shared `ToastRegistry`. Pull both back out of the tree's
        // `app_context` — the same `app_state` lookup any widget or
        // handler uses — synthesize the event, and assert on registry
        // state (not pixels) that it landed.
        use std::path::PathBuf;
        use teksilo_app::AppEventObservers;
        use teksilo_core::app_event::AppEvent;

        let app = TeksiloAppBuilder::new()
            .install_toast(ToastInstallOptions {
                archive: None,
                ..ToastInstallOptions::default()
            })
            .build_headless();

        let ctx = app.tree.app_context();
        let registry = ctx
            .app_state::<ToastRegistry>()
            .expect("install_toast registers a ToastRegistry in app_state")
            .clone();
        let observers = ctx
            .app_state::<AppEventObservers>()
            .expect("install_toast registers an AppEvent observer in app_state")
            .clone();

        assert_eq!(registry.live_count(), 0, "no toast before the event fires");

        let event = AppEvent::SettingsWriteFailed {
            path: PathBuf::from("/tmp/does-not-exist/window_state.toml"),
            attempts: 5,
            dropped_patches: 2,
            message: "disk full".to_string(),
        };
        (observers.0)(&event);

        assert_eq!(
            registry.live_count(),
            1,
            "the observer must enqueue exactly one toast for the failed write"
        );
    }

    #[test]
    fn unrelated_app_events_do_not_enqueue_a_toast() {
        // The observer must pattern-match specifically on
        // `SettingsWriteFailed` — every other `AppEvent` variant passing
        // through must be a no-op for the toast registry.
        use teksilo_app::AppEventObservers;
        use teksilo_core::app_event::AppEvent;

        let app = TeksiloAppBuilder::new()
            .install_toast(ToastInstallOptions {
                archive: None,
                ..ToastInstallOptions::default()
            })
            .build_headless();

        let ctx = app.tree.app_context();
        let registry = ctx
            .app_state::<ToastRegistry>()
            .expect("install_toast registers a ToastRegistry in app_state")
            .clone();
        let observers = ctx
            .app_state::<AppEventObservers>()
            .expect("install_toast registers an AppEvent observer in app_state")
            .clone();

        (observers.0)(&AppEvent::BackgroundComplete {
            operation_id: "unrelated".to_string(),
        });

        assert_eq!(
            registry.live_count(),
            0,
            "an unrelated AppEvent must not enqueue a toast"
        );
    }
}
