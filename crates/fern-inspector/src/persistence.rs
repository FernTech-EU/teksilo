//! Inspector state persistence via `fern_settings::SettingsStore`.
//!
//! Wired by `state::install`'s post-root closure on each window. The
//! whole module is a no-op when no `SettingsStore` is registered (the
//! example apps that don't call `app_paths(...)` keep working without
//! changes).
//!
//! Keys live under the framework-reserved `__fern_inspector.*`
//! namespace so the inspector's preferences don't collide with app
//! keys and don't show up in the user-facing rebind UI.

use fern_core::signal::Signal;
use fern_settings::SettingsStore;

use crate::state::{InspectorState, MAX_PANEL_HEIGHT, MIN_PANEL_HEIGHT, NUM_TABS, OverlayMode};

const KEY_OPEN: &str = "__fern_inspector.open";
const KEY_BOUNDS_MODE: &str = "__fern_inspector.bounds_mode";
const KEY_OVERLAY_OPACITY: &str = "__fern_inspector.overlay_opacity";
const KEY_ACTIVE_TAB: &str = "__fern_inspector.active_tab";
const KEY_PANEL_HEIGHT: &str = "__fern_inspector.panel_height";

/// Bridge `InspectorState` signals to their persistent counterparts
/// in `SettingsStore`. Idempotent — calling more than once on the
/// same state simply re-applies the persisted seed values.
pub(crate) fn wire(state: &InspectorState, store: &SettingsStore) {
    bridge_bool(&state.open, store, KEY_OPEN);
    bridge_overlay_mode(&state.overlay_mode, store);
    bridge_f32(&state.overlay_opacity, store, KEY_OVERLAY_OPACITY);
    bridge_active_tab(&state.active_tab, store);
    bridge_panel_height(&state.panel_height, store);
}

fn bridge_bool(state_sig: &Signal<bool>, store: &SettingsStore, key: &str) {
    let persisted = store.signal::<bool>(key, state_sig.get());
    // Apply persisted seed to state.
    if persisted.get() != state_sig.get() {
        state_sig.set(persisted.get());
    }
    // Bridge state → persisted on changes.
    let pers_clone = persisted.clone();
    let h = state_sig.observe(move |v| {
        if pers_clone.get() != *v {
            pers_clone.set(*v);
        }
    });
    state_sig.attach_keepalive(h);
}

fn bridge_f32(state_sig: &Signal<f32>, store: &SettingsStore, key: &str) {
    // SettingsStore stores f32 as f64-equivalent via TOML. We read/write f32 directly.
    let persisted = store.signal::<f32>(key, state_sig.get());
    if (persisted.get() - state_sig.get()).abs() > f32::EPSILON {
        state_sig.set(persisted.get());
    }
    let pers_clone = persisted.clone();
    let h = state_sig.observe(move |v| {
        if (pers_clone.get() - *v).abs() > f32::EPSILON {
            pers_clone.set(*v);
        }
    });
    state_sig.attach_keepalive(h);
}

fn bridge_overlay_mode(state_sig: &Signal<OverlayMode>, store: &SettingsStore) {
    // Persist as a string so the on-disk file is human-readable.
    let initial = mode_to_str(state_sig.get()).to_string();
    let persisted = store.signal::<String>(KEY_BOUNDS_MODE, initial);
    let parsed = str_to_mode(&persisted.get()).unwrap_or(state_sig.get());
    if parsed != state_sig.get() {
        state_sig.set(parsed);
    }
    let pers_clone = persisted.clone();
    let h = state_sig.observe(move |mode| {
        let s = mode_to_str(*mode).to_string();
        if pers_clone.get() != s {
            pers_clone.set(s);
        }
    });
    state_sig.attach_keepalive(h);
}

fn bridge_active_tab(state_sig: &Signal<usize>, store: &SettingsStore) {
    // Stored as i64 because TOML lacks unsigned integers and `usize`'s
    // width varies by target. Negative or out-of-range values seed at
    // 0 — TabWidget gracefully no-ops on a stale index, but the panel
    // would render blank, which is worse than starting on the Tree
    // tab.
    let initial = state_sig.get() as i64;
    let persisted = store.signal::<i64>(KEY_ACTIVE_TAB, initial);
    let parsed = usize::try_from(persisted.get())
        .ok()
        .filter(|i| *i < NUM_TABS)
        .unwrap_or(0);
    if parsed != state_sig.get() {
        state_sig.set(parsed);
    }
    let pers_clone = persisted.clone();
    let h = state_sig.observe(move |idx| {
        let v = *idx as i64;
        if pers_clone.get() != v {
            pers_clone.set(v);
        }
    });
    state_sig.attach_keepalive(h);
}

fn bridge_panel_height(state_sig: &Signal<f32>, store: &SettingsStore) {
    // Same shape as `bridge_f32` but with an extra clamp so a stale
    // or hand-edited value can't shrink the panel below the toolbar
    // or grow it past the user-root.
    let initial = state_sig.get().clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT);
    let persisted = store.signal::<f32>(KEY_PANEL_HEIGHT, initial);
    let parsed = persisted.get().clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT);
    if (parsed - state_sig.get()).abs() > f32::EPSILON {
        state_sig.set(parsed);
    }
    let pers_clone = persisted.clone();
    let h = state_sig.observe(move |v| {
        let clamped = v.clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT);
        if (pers_clone.get() - clamped).abs() > f32::EPSILON {
            pers_clone.set(clamped);
        }
    });
    state_sig.attach_keepalive(h);
}

fn mode_to_str(mode: OverlayMode) -> &'static str {
    match mode {
        OverlayMode::Off => "off",
        OverlayMode::SelectionOnly => "selection",
        OverlayMode::AllBounds => "all",
    }
}

fn str_to_mode(s: &str) -> Option<OverlayMode> {
    match s {
        "off" => Some(OverlayMode::Off),
        "selection" => Some(OverlayMode::SelectionOnly),
        "all" => Some(OverlayMode::AllBounds),
        _ => None,
    }
}
