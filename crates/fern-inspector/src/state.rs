//! Inspector state and installation logic — debug builds only.

use fern_app::{DefaultPostRoot, FernAppBuilder};
use fern_canvas::{Point, Rect};
use fern_core::event::{Key, Modifiers};
use fern_core::intent::Intent;
use fern_core::shortcut::{KeyStroke, Shortcut};
use fern_core::signal::Signal;
use fern_core::widget_id::WidgetId;

use crate::shell::InspectorShell;

/// Number of tabs registered by `build_panel` in `shell.rs`. Single
/// source of truth so persistence can clamp loaded values that would
/// otherwise leave the panel showing nothing.
pub(crate) const NUM_TABS: usize = 9;

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
    /// Post-root ids of every InspectorShell currently mounted (one
    /// per window). Pushed by `state::install`'s post_root closure
    /// when each window's shell is created. Used as the multi-window
    /// `exclude` set so the picker / hover tooltip never resolve to
    /// the inspector's own UI in any window.
    pub shell_root_ids: Signal<Vec<WidgetId>>,
    /// Set by the tree tab's tap handler with the widget-local y
    /// coordinate of the click; the tab's own `layout_response`
    /// reads this on the next pass, divides by row height, and updates
    /// `selected_id`.
    pub pending_tree_click_y: Signal<Option<f32>>,
    /// Reactive mirror of `WidgetTree::hovered()`. Bridged once per
    /// process by `state::install`'s post-root closure: an observer on
    /// the tree's hover signal forwards every change here. None until
    /// the bridge is wired (apps without inspector wiring see no
    /// behavior change).
    pub hover_id: Signal<Option<WidgetId>>,
    /// Resolved hover descriptor — type name + bounds — for the
    /// widget under the cursor. Populated by `BoundsTracker` from
    /// `hover_id` during AllBounds layout passes; consumed by
    /// `HighlightLayer` to paint a floating tooltip near the cursor.
    /// `None` outside AllBounds mode or when the cursor is over the
    /// inspector's own subtree.
    pub(crate) hover_info: Signal<Option<crate::highlight::HoverInfo>>,
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
    /// Index of the active panel tab. Hoisted out of `build_panel`'s
    /// local state so the tab survives panel rebuilds and persists via
    /// `__fern_inspector.active_tab`. Indexes the same tab list the
    /// panel registers (Tree / Properties / Accessibility / Theme /
    /// Locale / Focus / Shortcuts / Overlays / Models — slots 0..9).
    pub active_tab: Signal<usize>,
    /// Currently selected row in the Data Models tab. Drives which
    /// registered model's contents are shown in the dump area. `None`
    /// falls back to the most recently registered model.
    pub selected_model_index: Signal<Option<usize>>,
    /// Set by the Models tab's tap handler with the widget-local y
    /// coordinate of the click; the tab's own `layout_response` reads
    /// this on the next pass, divides by row height, and updates
    /// `selected_model_index`. Mirrors `pending_tree_click_y`.
    pub pending_models_click_y: Signal<Option<f32>>,
    /// Mirror of `WidgetTree::focused()`. Bridged in `state::install`
    /// post_root closure so the Focus tab can bind to it for reactive
    /// repaint instead of polling.
    pub focus_id: Signal<Option<WidgetId>>,
    /// Mirror of `ShortcutRegistry::version()`. Bridged once per
    /// process; the Shortcuts tab binds to it for reactive repaint
    /// when shortcuts are registered or rebound.
    pub shortcut_version: Signal<u64>,
    /// Mirror of `OverlayManager::version()`. Bridged once per
    /// process; the Overlays tab binds to it for reactive repaint
    /// whenever an overlay is shown or dismissed.
    pub overlay_version: Signal<u64>,
    /// Padding insets and stack gaps captured by `BoundsTracker` in
    /// AllBounds mode. Painted as filled tinted bands by
    /// `HighlightLayer` to make per-axis spacing visible. Empty
    /// outside AllBounds.
    pub(crate) band_snapshot: Signal<Vec<crate::highlight::BandEntry>>,
    /// Current panel height in logical pixels. Drives the panel slot's
    /// `FixedSize::bind_height`. Mutated by the panel resize handle
    /// (top-edge drag) and persisted via `__fern_inspector.panel_height`.
    /// Clamped to a sensible range — see `MIN_PANEL_HEIGHT` /
    /// `MAX_PANEL_HEIGHT`.
    pub panel_height: Signal<f32>,
    /// Substring filter for the Tree tab. Empty string disables the
    /// filter. The tab compares each widget's last-segment type name
    /// case-insensitively against this string.
    pub tree_filter: Signal<String>,
    /// Panel-resize drag anchor: the widget-local y-coordinate inside
    /// the resize handle where the user originally pressed. The handle
    /// uses this as a fixed reference so its top-edge tracks the
    /// cursor exactly under live layout — `delta = anchor - position.y`
    /// applied to `panel_height` each `PointerMove`. `None` when no
    /// drag is in flight.
    pub(crate) panel_drag_anchor_y: Signal<Option<f32>>,
    /// Pre-formatted dump of the Properties tab's current rows
    /// (including the full multi-line Debug repr). Refreshed by the
    /// Properties leaf in `layout_response`; consumed by the Copy
    /// button to write to the clipboard. Empty when no widget is
    /// selected.
    pub(crate) properties_dump: Signal<String>,
    /// Value of the row the user just right-clicked in the Properties
    /// tab. Set by the leaf's secondary-click handler before opening
    /// the context menu; consumed by the menu's `Copy value`
    /// activation handler. Empty when no row was clicked.
    pub(crate) properties_context_value: Signal<String>,
    /// Key (column-name) of the row the user just right-clicked.
    /// Drives the dynamic menu label, e.g. `Copy "bounds"`.
    pub(crate) properties_context_key: Signal<String>,
}

/// Default panel height in logical pixels. Used as the initial value
/// of `panel_height` and as the persistence default.
pub(crate) const DEFAULT_PANEL_HEIGHT: f32 = 280.0;
/// Lower clamp for `panel_height`. Anything smaller doesn't fit the
/// toolbar + one row of tab content.
pub(crate) const MIN_PANEL_HEIGHT: f32 = 120.0;
/// Upper clamp for `panel_height`. Past this the panel starts hiding
/// the user-root area entirely on small windows.
pub(crate) const MAX_PANEL_HEIGHT: f32 = 720.0;

impl InspectorState {
    pub(crate) fn new(initial_open: bool) -> Self {
        Self {
            open: Signal::new(initial_open),
            selected_id: Signal::new(None),
            selected_bounds: Signal::new(None),
            picker_mode: Signal::new(false),
            pending_pick_point: Signal::new(None),
            shell_root_ids: Signal::new(Vec::new()),
            pending_tree_click_y: Signal::new(None),
            overlay_mode: Signal::new(OverlayMode::SelectionOnly),
            overlay_opacity: Signal::new(0.7),
            bounds_snapshot: Signal::new(Vec::new()),
            active_tab: Signal::new(0),
            selected_model_index: Signal::new(None),
            pending_models_click_y: Signal::new(None),
            hover_id: Signal::new(None),
            hover_info: Signal::new(None),
            focus_id: Signal::new(None),
            shortcut_version: Signal::new(0),
            overlay_version: Signal::new(0),
            band_snapshot: Signal::new(Vec::new()),
            panel_height: Signal::new(DEFAULT_PANEL_HEIGHT),
            tree_filter: Signal::new(String::new()),
            panel_drag_anchor_y: Signal::new(None),
            properties_dump: Signal::new(String::new()),
            properties_context_value: Signal::new(String::new()),
            properties_context_key: Signal::new(String::new()),
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
    let persistence_wired = std::rc::Rc::new(std::cell::Cell::new(false));
    let hover_bridge_wired = std::rc::Rc::new(std::cell::Cell::new(false));

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

        // First time only: bridge state signals to SettingsStore (if
        // the app has wired one). Idempotent guard via Cell so
        // multi-window apps don't bridge twice.
        if !persistence_wired.replace(true) {
            if let Some(store) = tree
                .app_context()
                .app_state::<fern_settings::SettingsStore>()
            {
                crate::persistence::wire(&state_for_post_root, store);
            }
        }

        // First time only: bridge a handful of `tree.*_signal()`
        // sources → InspectorState mirrors so the panel tabs can react
        // via signal bindings rather than polling. Same idempotent
        // guard pattern as persistence; the keepalive lives on the
        // tree's own signal so it dies with the tree.
        if !hover_bridge_wired.replace(true) {
            let hover_target = state_for_post_root.hover_id.clone();
            let tree_hover = tree.hovered_signal();
            let h = tree_hover.observe(move |id| {
                if hover_target.get() != *id {
                    hover_target.set(*id);
                }
            });
            tree_hover.attach_keepalive(h);

            let focus_target = state_for_post_root.focus_id.clone();
            let tree_focus = tree.focused_signal();
            let h = tree_focus.observe(move |id| {
                if focus_target.get() != *id {
                    focus_target.set(*id);
                }
            });
            tree_focus.attach_keepalive(h);

            let sc_target = state_for_post_root.shortcut_version.clone();
            let sc_version = tree.shortcut_registry().version().clone();
            let h = sc_version.observe(move |v| {
                if sc_target.get() != *v {
                    sc_target.set(*v);
                }
            });
            sc_version.attach_keepalive(h);

            let ov_target = state_for_post_root.overlay_version.clone();
            let ov_version = tree.overlay_manager().version().clone();
            let h = ov_version.observe(move |v| {
                if ov_target.get() != *v {
                    ov_target.set(*v);
                }
            });
            ov_version.attach_keepalive(h);
        }

        // Wrap the user root in an InspectorShell. The shell owns the
        // panel, the toolbar, the highlight overlay, the picker
        // overlay, and the bounds-tracker / pick-resolver helpers.
        let shell_id = tree.add(InspectorShell::new(root_id, state_for_post_root.clone()));
        // Multi-window: every window's shell adds its id to the
        // exclusion vec so neither the picker nor the hover tooltip
        // ever resolves to inspector UI in any window.
        let mut ids = state_for_post_root.shell_root_ids.get();
        if !ids.contains(&shell_id) {
            ids.push(shell_id);
            state_for_post_root.shell_root_ids.set(ids);
        }
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
