// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Inspector state and installation logic — debug builds only.

use bastyde_app::{BastydeAppBuilder, DefaultPostRoot};
use bastyde_canvas::{Point, Rect};
use bastyde_core::event::{Key, Modifiers};
use bastyde_core::intent::Intent;
use bastyde_core::shortcut::{KeyStroke, Shortcut};
use bastyde_core::signal::Signal;
use bastyde_core::widget_id::WidgetId;

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

/// One row in the picker's chain menu — a widget id paired with the
/// last segment of its type name (e.g. `Button`, `Padding`) so the
/// menu's row labels can be derived without re-walking the arena
/// from the click handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickChainEntry {
    pub id: WidgetId,
    pub label: String,
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
    /// F12, the toolbar Close button, and the `--bastyde-inspector` /
    /// `BASTYDE_INSPECTOR` boot flags.
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
    /// Ancestor chain produced by the picker. Index 0 is the deepest
    /// widget under the click point; subsequent entries walk up the
    /// arena tree (parent → grandparent → …) and stop at the
    /// containing user-root id (inclusive) or after 10 entries —
    /// whichever comes first. Each entry carries the resolved type
    /// label (last segment of `Widget::type_name()`) alongside the
    /// id so the menu rows can display a stable string without
    /// re-walking the arena. Cleared back to empty once the user
    /// commits a selection from the menu or dismisses it. While
    /// non-empty, the picker shows a context menu listing each entry
    /// so the developer can choose the right level of a composite
    /// (e.g. the `Button` ancestor instead of the deepest-hit
    /// `TextWidget` inside it). See `PickResolver::layout_response`
    /// for the producer side and `PickerOverlay`'s pointer handler
    /// for the consumer side.
    pub pending_pick_chain: Signal<Vec<PickChainEntry>>,
    /// Window-local point at which to anchor the picker chain menu.
    /// Set together with `pending_pick_chain` by `PickResolver` and
    /// passed to `OverlayPlacement::AtPointer`. Cleared once the
    /// menu is shown.
    pub pick_menu_anchor: Signal<Option<Point>>,
    /// Pre-registered menu panel id. Created once by `InspectorShell`
    /// in `build()` and parked dormant. The picker activates it +
    /// shows it as an overlay, with each row's `Button::label`
    /// reading from `pending_pick_chain`. Same orphan-dormant pattern
    /// the `PropertiesRows` "Copy value" context menu uses.
    pub(crate) pick_menu_id: Signal<Option<WidgetId>>,
    /// User-root widget ids — one per InspectorShell (so one per
    /// window). Pushed by `state::install`'s post_root closure with
    /// the `root_id` argument it receives. Used as the **starting
    /// points** for tree walks (Tree tab, BoundsTracker) and for
    /// scoped hit-tests (PickResolver) so the inspector's picker /
    /// listings only see the user app and never resolve into the
    /// inspector's own chrome. Note: previous slices misnamed this
    /// `shell_root_ids` and stored the InspectorShell id itself —
    /// that was over-aggressive (after wrapping, the shell IS the
    /// only root, so excluding it left nothing to walk) and broke the
    /// picker.
    pub user_root_ids: Signal<Vec<WidgetId>>,
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
    /// `__bastyde_inspector.active_tab`. Indexes the same tab list the
    /// panel registers (Tree / Properties / Accessibility / Theme /
    /// Locale / Focus / Shortcuts / Overlays / Models — slots 0..9).
    pub active_tab: Signal<usize>,
    /// Stable [`TabId`](bastyde_widgets::TabId) for each of the
    /// `NUM_TABS` panel tabs, allocated once at construction. The
    /// `TabWidget` selection signal speaks in `Option<TabId>`; this
    /// vector is the index ↔ id translation table for keyboard
    /// navigation, persistence, and the `active_tab` ↔
    /// `active_tab_id` bridge.
    pub tab_ids: Vec<bastyde_widgets::TabId>,
    /// `TabWidget`-shaped projection of [`active_tab`](Self::active_tab).
    /// Bridged to `active_tab` by paired observers installed in
    /// `InspectorState::new`; reads / writes flow either way and
    /// the bridge stays alive for the lifetime of the
    /// [`InspectorState`] (the observer handles are stored in
    /// `_active_tab_bridge`).
    pub active_tab_id: Signal<Option<bastyde_widgets::TabId>>,
    /// Lifetime anchor for the `active_tab` ↔ `active_tab_id`
    /// observers. Wrapped in `Rc<Vec<...>>` so [`InspectorState`]
    /// stays `Clone` (the inspector duplicates the state across the
    /// many tab widgets that read it). All clones share the same
    /// observer registrations; the registrations detach when the
    /// last clone is dropped.
    _active_tab_bridge: std::rc::Rc<Vec<bastyde_core::ObserverHandle>>,
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
    /// Whether the overflow overlay is active. **On by default** in debug
    /// builds (Flutter-style "loud" overflow): wherever a distributing
    /// container's children exceed its bounds, hazard stripes are painted —
    /// independent of [`overlay_mode`](Self::overlay_mode), so it works with
    /// the inspector panel closed. Toggled from the toolbar and persisted via
    /// `__bastyde_inspector.overflow_overlay`.
    pub overflow_overlay: Signal<bool>,
    /// Overhang strips (the regions where children spill past their
    /// distributing parent) captured by `BoundsTracker` when
    /// [`overflow_overlay`](Self::overflow_overlay) is on; painted as hazard
    /// stripes by `HighlightLayer`. Empty when off or when nothing overflows.
    pub(crate) overflow_snapshot: Signal<Vec<bastyde_canvas::Rect>>,
    /// Current panel height in logical pixels. Drives the panel slot's
    /// `FixedSize::height`. Mutated by the panel resize handle
    /// (top-edge drag) and persisted via `__bastyde_inspector.panel_height`.
    /// Clamped to a sensible range — see `MIN_PANEL_HEIGHT` /
    /// `MAX_PANEL_HEIGHT`.
    pub panel_height: Signal<f32>,
    /// Substring filter for the Tree tab. Empty string disables the
    /// filter. The tab compares each widget's last-segment type name
    /// case-insensitively against this string.
    pub tree_filter: Signal<String>,
    /// Panel-resize drag anchor — `(anchor_window_y, start_height)`
    /// captured at PointerDown. The handler sets `panel_height =
    /// start_height + (anchor_window_y - position.y)` on every
    /// `PointerMove` so total height tracks the cursor's total
    /// displacement from the initial click. Storing only the anchor
    /// (without `start_height`) is wrong because `position` in
    /// `WidgetEvent::PointerMove` is window-local — repeated moves
    /// accumulate against the *live* height instead of the start
    /// height, multiplying the user's drag.
    pub(crate) panel_drag_anchor: Signal<Option<(f32, f32)>>,
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
        let active_tab: Signal<usize> = Signal::new(0);
        let tab_ids: Vec<bastyde_widgets::TabId> = (0..NUM_TABS)
            .map(|_| bastyde_widgets::TabId::fresh())
            .collect();
        let active_tab_id: Signal<Option<bastyde_widgets::TabId>> = Signal::new(Some(tab_ids[0]));

        // Index → id observer: when the index signal changes (keyboard
        // nav / persistence load), update the id signal so the
        // `TabWidget` adopts the new selection.
        let bridge1 = {
            let id_sig = active_tab_id.clone();
            let ids = tab_ids.clone();
            active_tab.observe(move |i| {
                if let Some(&id) = ids.get(*i)
                    && id_sig.get() != Some(id)
                {
                    id_sig.set(Some(id));
                }
            })
        };

        // Id → index observer: when the user clicks a tab in the bar,
        // the `TabWidget` writes a new id; reflect that into the
        // index signal so keyboard nav / persistence stay in sync.
        let bridge2 = {
            let idx_sig = active_tab.clone();
            let ids = tab_ids.clone();
            active_tab_id.observe(move |maybe_id| {
                if let Some(id) = maybe_id
                    && let Some(i) = ids.iter().position(|x| x == id)
                    && idx_sig.get() != i
                {
                    idx_sig.set(i);
                }
            })
        };

        Self {
            open: Signal::new(initial_open),
            selected_id: Signal::new(None),
            selected_bounds: Signal::new(None),
            picker_mode: Signal::new(false),
            pending_pick_point: Signal::new(None),
            pending_pick_chain: Signal::new(Vec::new()),
            pick_menu_anchor: Signal::new(None),
            pick_menu_id: Signal::new(None),
            user_root_ids: Signal::new(Vec::new()),
            pending_tree_click_y: Signal::new(None),
            overlay_mode: Signal::new(OverlayMode::SelectionOnly),
            overlay_opacity: Signal::new(0.7),
            bounds_snapshot: Signal::new(Vec::new()),
            active_tab,
            tab_ids,
            active_tab_id,
            _active_tab_bridge: std::rc::Rc::new(vec![bridge1, bridge2]),
            selected_model_index: Signal::new(None),
            pending_models_click_y: Signal::new(None),
            hover_id: Signal::new(None),
            hover_info: Signal::new(None),
            focus_id: Signal::new(None),
            shortcut_version: Signal::new(0),
            overlay_version: Signal::new(0),
            band_snapshot: Signal::new(Vec::new()),
            overflow_overlay: Signal::new(true),
            overflow_snapshot: Signal::new(Vec::new()),
            panel_height: Signal::new(DEFAULT_PANEL_HEIGHT),
            tree_filter: Signal::new(String::new()),
            panel_drag_anchor: Signal::new(None),
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
/// `--bastyde-inspector` and `BASTYDE_INSPECTOR` (`1` / `true`).
fn initial_open_from_env() -> bool {
    let flag = std::env::args().any(|a| a == "--bastyde-inspector");
    let env = std::env::var("BASTYDE_INSPECTOR")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    flag || env
}

/// Wire up the inspector. See
/// [`crate::BastydeAppBuilderInspectorExt::install_inspector_in_debug`].
pub(crate) fn install(builder: BastydeAppBuilder) -> BastydeAppBuilder {
    let state = InspectorState::new(initial_open_from_env());

    let toggle_for_post_root = state.open.clone();
    let state_for_post_root = state.clone();
    let persistence_wired = std::rc::Rc::new(std::cell::Cell::new(false));
    let hover_bridge_wired = std::rc::Rc::new(std::cell::Cell::new(false));

    let post_root = DefaultPostRoot::new(move |tree, root_id| {
        // Register F12 toggle. Owner is the user's root widget so the
        // shortcut is automatically removed when the window closes.
        let toggle = toggle_for_post_root.clone();
        let shortcut = Shortcut::new("__bastyde_inspector.toggle")
            .name("Toggle Inspector")
            .primary(KeyStroke::new(Key::F12, Modifiers::empty()))
            .on_activate(move |_ks, _ctx| {
                let next = !toggle.get();
                toggle.set(next);
                Intent::new("__bastyde_inspector.toggle")
            })
            .build();
        tree.shortcut_registry_mut()
            .register_owned(shortcut, root_id);

        // First time only: bridge state signals to SettingsStore (if
        // the app has wired one). Idempotent guard via Cell so
        // multi-window apps don't bridge twice.
        if !persistence_wired.replace(true)
            && let Some(store) = tree
                .app_context()
                .app_state::<bastyde_settings::SettingsStore>()
        {
            crate::persistence::wire(&state_for_post_root, store);
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
        // Multi-window: every window's user-root id is appended so
        // the inspector's tree-walking consumers (picker, Tree tab,
        // BoundsTracker) target the right per-window app subtree.
        let mut ids = state_for_post_root.user_root_ids.get();
        if !ids.contains(&root_id) {
            ids.push(root_id);
            state_for_post_root.user_root_ids.set(ids);
        }
        shell_id
    });

    // Compose (don't clobber) the app-wide post-root chain. Using
    // `register_post_root` instead of `app_state(post_root)` lets the
    // inspector shell coexist with the toast host (or any other
    // post-root chrome) no matter which was installed first — a plain
    // `app_state` insert is type-keyed and would silently overwrite the
    // other installer's `DefaultPostRoot`.
    builder.app_state(state).register_post_root(post_root)
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
