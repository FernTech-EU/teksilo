//! In-app debug inspector for Bastyde.
//!
//! See `docs/inspector.md` for a user-facing reference. This crate is
//! gated by `cfg(debug_assertions)` — in release builds the
//! [`BastydeAppBuilderInspectorExt::install_inspector_in_debug`] entry
//! point compiles to a no-op, so app code calling it stays ergonomic
//! without `#[cfg]` lines.

#[cfg(debug_assertions)]
mod highlight;
#[cfg(all(debug_assertions, test))]
mod integration_tests;
#[cfg(debug_assertions)]
mod keyboard;
#[cfg(debug_assertions)]
mod persistence;
#[cfg(debug_assertions)]
mod picker;
#[cfg(debug_assertions)]
mod resize_handle;
#[cfg(debug_assertions)]
mod shell;
#[cfg(debug_assertions)]
mod state;
#[cfg(debug_assertions)]
mod tabs;

#[cfg(debug_assertions)]
pub use state::InspectorState;

use bastyde_app::BastydeAppBuilder;

/// Extension trait on [`BastydeAppBuilder`] that wires in the debug
/// inspector. The `install_inspector_in_debug` method is a no-op in
/// release builds so apps can call it unconditionally.
pub trait BastydeAppBuilderInspectorExt {
    /// Install the debug inspector. In debug builds, this:
    /// - parses `--bastyde-inspector` from `std::env::args()` and
    ///   `BASTYDE_INSPECTOR=1` from the environment to seed the initial
    ///   visibility,
    /// - registers an [`InspectorState`] in the app-state registry so
    ///   widgets can read the inspector's signals,
    /// - registers a default `post_root` hook that wraps every
    ///   window's root with the inspector shell (currently a no-op
    ///   wrapper — slice 1).
    ///
    /// In release (`!cfg(debug_assertions)`), this is a no-op.
    fn install_inspector_in_debug(self) -> Self;
}

impl BastydeAppBuilderInspectorExt for BastydeAppBuilder {
    #[cfg(debug_assertions)]
    fn install_inspector_in_debug(self) -> Self {
        state::install(self)
    }

    #[cfg(not(debug_assertions))]
    fn install_inspector_in_debug(self) -> Self {
        self
    }
}
