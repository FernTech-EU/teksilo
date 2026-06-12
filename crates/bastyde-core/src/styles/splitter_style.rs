//! Tier-3 style protocol for `Splitter`. See `docs/styling-system.md`.
//!
//! The `Splitter` widget owns all input handling (drag, keyboard,
//! collapse, accessibility) and delegates only the *paint* of each
//! divider handle to a [`SplitterStyle`] impl. Layout-affecting
//! dimensions (gutter thickness, min pane sizes, keyboard step, snap
//! offset) live on the `SplitterModel`, not here — the styling-system
//! rule is that anything consumed by `place_children` must be resolved
//! before layout, whereas a style body is built lazily and paints at
//! paint time.
//!
//! The IntUI default ([`crate::styles`]'s `RecipeSplitterStyle`, in
//! `bastyde-widgets`) reproduces the thin-line-with-hover-dwell-focus
//! look out of the box; apps install a different look per-call via
//! `Splitter::style(...)` or theme-wide via `theme.style_slots.splitter`.

use std::rc::Rc;

use bastyde_tokens::Orientation;

use crate::build_context::BuildContext;
use crate::focus::FocusOrigin;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Reactive inputs handed to a [`SplitterStyle`] when it builds one
/// divider handle's visual body. Every field a default impl repaints on
/// is a `Signal`, so the body re-renders without a rebuild.
#[derive(Clone, Debug)]
pub struct SplitterStyleConfig {
    /// Orientation of the parent `Splitter`. Note: a *horizontal*
    /// splitter (panes side-by-side) draws a *vertical* handle bar, and
    /// vice versa — the recipe accounts for this when painting the line.
    pub orientation: Orientation,
    pub is_hovered: Signal<bool>,
    /// `true` while the user is drag-pressing this handle.
    pub is_dragging: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// `Some(FocusOrigin::Keyboard)` while the handle has keyboard
    /// focus; the IntUI default uses this to gate the full-strength
    /// focus indicator. `Some(Pointer)` and `None` fall back to the
    /// hover-dwell ramp.
    pub focus_origin: Signal<Option<FocusOrigin>>,
    /// Hover-dwell progress `0.0..=1.0` driving the focus-indicator
    /// fade-in (animated by the handle; the style maps it to alpha).
    pub hover_progress: Signal<f32>,
}

/// The visual contract for a `Splitter` divider handle.
///
/// `make_handle` returns the `WidgetId` of a leaf (or subtree) that
/// paints the divider chrome. The host `Splitter` sizes it to the
/// model's gutter thickness × the cross axis and routes all input
/// itself — the body is purely presentational and should mark itself
/// hidden from the accessibility tree (the splitter handle owns the
/// `Role::Splitter` node).
pub trait SplitterStyle: 'static {
    fn make_handle(&self, cfg: &SplitterStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedSplitterStyle = Rc<dyn SplitterStyle>;
