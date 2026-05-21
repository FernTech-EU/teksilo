//! [`OverscrollBehavior`] — boundary scroll-chaining policy.
//!
//! Shared by every scrollable surface (the `bastyde-widgets` scrollables —
//! `ScrollArea` / `ListView` / `TreeView` / `TableView` — and the
//! `bastyde-scene` `SceneView` pan handler). It lives in `bastyde-core` so
//! both tiers can name it without `bastyde-scene` depending on
//! `bastyde-widgets`. `bastyde-widgets` re-exports it as
//! `bastyde_widgets::OverscrollBehavior` for backwards compatibility.

/// Controls whether a scrollable surface chains scroll events to its ancestor
/// when it reaches a boundary — the equivalent of CSS `overscroll-behavior`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverscrollBehavior {
    /// At a boundary, decline the event (`Ignored`) so it propagates to the
    /// next ancestor scrollable. The default (`overscroll-behavior: auto`).
    #[default]
    Chain,
    /// Always absorb the event (`Handled`), even at the boundary — no
    /// chaining. Equivalent to `overscroll-behavior: contain`.
    Contain,
}
