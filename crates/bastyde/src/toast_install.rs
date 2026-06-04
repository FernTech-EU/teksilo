//! `install_toast` — wire the Toast system into a `BastydeAppBuilder`.
//!
//! Mirrors the `BastydeAppBuilderInspectorExt` pattern (see
//! [`bastyde_inspector`]) — the extension trait declaration and its
//! impl on `BastydeAppBuilder` live in the same crate (`bastyde`),
//! which depends on both `bastyde-app` and `bastyde-widgets`, so the
//! orphan rule is satisfied.
//!
//! Apps wire it in one line:
//!
//! ```ignore
//! use bastyde::prelude::*;
//!
//! BastydeAppBuilder::new()
//!     .theme(intui::light())
//!     .app_paths(AppPaths::new("com", "FernTech", "MyApp").unwrap())
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

use bastyde_app::{BastydeAppBuilder, DefaultPostRoot};
use bastyde_widgets::notification::{NotificationArchive, NotificationArchiveModel};
use bastyde_widgets::primitives::{Expand, ZStack};
use bastyde_widgets::toast::{ToastHost, ToastInstallOptions, ToastRegistry};

/// Extension trait on [`BastydeAppBuilder`] that wires up the Toast
/// system. The two methods are no-ops without the `toast` feature
/// on `bastyde`; with the feature on (the default), they install
/// the registry + archive + per-window `ToastHost` wrapper.
pub trait BastydeAppBuilderToastExt {
    /// Install the Toast subsystem with the given options.
    ///
    /// # Panics
    ///
    /// Panics if `options.archive` is `Some(NotificationArchive::Persistent { … })`
    /// but no [`AppPaths`](bastyde_settings::AppPaths) was configured
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

impl BastydeAppBuilderToastExt for BastydeAppBuilder {
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
                    &bastyde_settings::AppPaths::for_testing(std::path::Path::new("")),
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
                        // matches `bastyde_settings::DEFAULT_DEBOUNCE`;
                        // archives use the same so writes coalesce
                        // with other settings writes.
                        bastyde_settings::DEFAULT_DEBOUNCE,
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

        // 4. Register everything in app_state.
        let mut builder = self.app_state(registry);
        if let Some(a) = archive {
            builder = builder.app_state(a);
        }
        builder.app_state(post_root)
    }

    fn install_toast_default(self) -> Self {
        self.install_toast(ToastInstallOptions::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_widgets::notification::NotificationArchive;

    #[test]
    fn install_with_in_memory_archive_does_not_require_app_paths() {
        // No `.app_paths(...)` configured — install should still
        // succeed with the InMemory archive.
        let opts = ToastInstallOptions {
            archive: Some(NotificationArchive::in_memory()),
            ..ToastInstallOptions::default()
        };
        let _builder = BastydeAppBuilder::new().install_toast(opts);
    }

    #[test]
    fn install_with_no_archive_does_not_require_app_paths() {
        let opts = ToastInstallOptions {
            archive: None,
            ..ToastInstallOptions::default()
        };
        let _builder = BastydeAppBuilder::new().install_toast(opts);
    }

    #[test]
    #[should_panic(expected = "app_paths() (or application(...)) to be set")]
    fn install_persistent_without_app_paths_panics_with_helpful_message() {
        let opts = ToastInstallOptions {
            archive: Some(NotificationArchive::persistent("notif_test_panic")),
            ..ToastInstallOptions::default()
        };
        let _builder = BastydeAppBuilder::new().install_toast(opts);
    }

    #[test]
    fn install_persistent_with_app_paths_succeeds() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let opts = ToastInstallOptions {
            archive: Some(NotificationArchive::persistent("notif_test_success")),
            ..ToastInstallOptions::default()
        };
        let _builder = BastydeAppBuilder::new()
            .app_paths(bastyde_settings::AppPaths::for_testing(dir.path()))
            .install_toast(opts);
    }

    #[test]
    fn install_toast_default_uses_persistent_archive_by_default() {
        // Default is persistent — requires app_paths. Sanity check
        // that the default constructor wires the right shape.
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let _builder = BastydeAppBuilder::new()
            .app_paths(bastyde_settings::AppPaths::for_testing(dir.path()))
            .install_toast_default();
    }
}
