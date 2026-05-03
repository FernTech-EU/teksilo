//! Inspector state and installation logic — debug builds only.

use fern_app::{DefaultPostRoot, FernAppBuilder};
use fern_canvas::{Point, Rect};
use fern_core::event::{Key, Modifiers};
use fern_core::intent::Intent;
use fern_core::shortcut::{KeyStroke, Shortcut};
use fern_core::signal::Signal;
use fern_core::widget_id::WidgetId;

use crate::shell::InspectorShell;

/// How the bounds-overlay layer renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayMode {
    /// No overlay drawing at all.
    Off,
    /// Draw a stroke around the currently selected widget only.
    SelectionOnly,
    /// Draw every active widget's outline. Layout primitives in cyan,
    /// content widgets in magenta. Useful for visualizing layout
    /// structure.
    AllBounds,
}

impl OverlayMode {
    /// Cycle Off → Selection → All → Off. Used by the toolbar's
    /// keyboard-friendly Tab cycle.
    pub fn next(self) -> Self {
        match self {
            OverlayMode::Off => OverlayMode::SelectionOnly,
            OverlayMode::SelectionOnly => OverlayMode::AllBounds,
            OverlayMode::AllBounds => OverlayMode::Off,
        }
    }
}

/// Shared, app-wide state for the debug inspector.
///
/// A single instance is registered into `app_state` by
/// `install_inspector_in_debug` and shared across every window.
#[derive(Clone)]
pub struct InspectorState {
    /// Whether the inspector panel is currently visible. Toggled by
    /// F12, the toolbar Close button, and the `--fern-inspector` /
    /// `FERN_INSPECTOR` boot flags.
    pub open: Signal<bool>,
    /// The currently selected widget (clicked in the tree tab or
    /// picked via the picker tool). Drives the Properties and
    /// Accessibility tabs and the on-canvas highlight border.
    pub selected_id: Signal<Option<WidgetId>>,
    /// Live bounds of the selected widget, in window-local
    /// coordinates. Synced from the selection by `BoundsTracker` on
    /// every layout pass. The `HighlightLayer` reads this directly.
    pub selected_bounds: Signal<Option<Rect>>,
    /// Whether the picker tool is currently active. While `true`,
    /// `PickerOverlay` covers the user-root subregion and steals
    /// pointer events. Toggled by the toolbar Pick button and
    /// auto-cleared when a widget is picked.
    pub picker_mode: Signal<bool>,
    /// Latest pointer position recorded by the picker overlay,
    /// pending hit-test resolution by `PickResolver` in the next
    /// layout pass. `None` between picks.
    pub pending_pick_point: Signal<Option<Point>>,
    /// The post-root id (the inspector shell's wrapped root) — set by
    /// `InspectorShell::build` once it knows its own id, and used by
    /// the picker resolver as the `exclude` argument to hit-test so
    /// the picker doesn't pick its own overlay.
    pub shell_root_id: Signal<Option<WidgetId>>,
    /// Set by the tree tab's tap handler with the widget-local y
    /// coordinate of the click; the tab's own `layout_response`
    /// reads this on the next pass, divides by row height, and updates
    /// `selected_id`.
    pub pending_tree_click_y: Signal<Option<f32>>,
    /// Bounds-overlay rendering mode. Drives `HighlightLayer`. Toggled
    /// by the toolbar's `SegmentedControl`.
    pub overlay_mode: Signal<OverlayMode>,
    /// Opacity multiplier (0.1..1.0) applied to bounds-overlay strokes
    /// and tints. Lets the user dim a busy overlay on dense UIs.
    pub overlay_opacity: Signal<f32>,
    /// Snapshot of every widget's bounds + layout/content
    /// classification. Repopulated by `BoundsTracker` on every layout
    /// pass when `overlay_mode == AllBounds`; consumed by
    /// `HighlightLayer::paint`.
    pub(crate) bounds_snapshot: Signal<Vec<crate::highlight::BoundsEntry>>,
}

impl InspectorState {
    pub(crate) fn new(initial_open: bool) -> Self {
        Self {
            open: Signal::new(initial_open),
            selected_id: Signal::new(None),
            selected_bounds: Signal::new(None),
            picker_mode: Signal::new(false),
            pending_pick_point: Signal::new(None),
            shell_root_id: Signal::new(None),
            pending_tree_click_y: Signal::new(None),
            overlay_mode: Signal::new(OverlayMode::SelectionOnly),
            overlay_opacity: Signal::new(0.7),
            bounds_snapshot: Signal::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for InspectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorState")
            .field("open", &self.open.get())
            .field("selected_id", &self.selected_id.get())
            .field("picker_mode", &self.picker_mode.get())
            .finish()
    }
}

/// Decide whether the inspector should be open at startup, based on
/// `--fern-inspector` and `FERN_INSPECTOR` (`1` / `true`).
fn initial_open_from_env() -> bool {
    let flag = std::env::args().any(|a| a == "--fern-inspector");
    let env = std::env::var("FERN_INSPECTOR")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    flag || env
}

/// Wire up the inspector. See
/// [`crate::FernAppBuilderInspectorExt::install_inspector_in_debug`].
pub(crate) fn install(builder: FernAppBuilder) -> FernAppBuilder {
    let state = InspectorState::new(initial_open_from_env());

    let toggle_for_post_root = state.open.clone();
    let state_for_post_root = state.clone();

    let post_root = DefaultPostRoot::new(move |tree, root_id| {
        // Register F12 toggle. Owner is the user's root widget so the
        // shortcut is automatically removed when the window closes.
        let toggle = toggle_for_post_root.clone();
        let shortcut = Shortcut::new("__fern_inspector.toggle")
            .name("Toggle Inspector")
            .primary(KeyStroke::new(Key::F12, Modifiers::empty()))
            .on_activate(move |_ks, _ctx| {
                let next = !toggle.get();
                toggle.set(next);
                Intent::new("__fern_inspector.toggle")
            })
            .build();
        tree.shortcut_registry_mut().register_owned(shortcut, root_id);

        // Wrap the user root in an InspectorShell. The shell owns the
        // panel, the toolbar, the highlight overlay, the picker
        // overlay, and the bounds-tracker / pick-resolver helpers.
        let shell_id = tree.add(InspectorShell::new(root_id, state_for_post_root.clone()));
        state_for_post_root.shell_root_id.set(Some(shell_id));
        shell_id
    });

    builder.app_state(state).app_state(post_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_state_signals_are_clonable() {
        let s = InspectorState::new(false);
        let clone = s.clone();
        s.open.set(true);
        assert!(clone.open.get(), "Signal handles share the same backing");
    }

    #[test]
    fn inspector_state_starts_with_provided_value() {
        let off = InspectorState::new(false);
        assert!(!off.open.get());
        let on = InspectorState::new(true);
        assert!(on.open.get());
    }

    #[test]
    fn selection_starts_unset() {
        let s = InspectorState::new(false);
        assert!(s.selected_id.get().is_none());
        assert!(s.selected_bounds.get().is_none());
        assert!(!s.picker_mode.get());
    }
}
