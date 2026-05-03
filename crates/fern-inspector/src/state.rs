//! Inspector state and installation logic — debug builds only.

use fern_app::{DefaultPostRoot, FernAppBuilder};
use fern_core::event::{Key, Modifiers};
use fern_core::intent::Intent;
use fern_core::shortcut::{KeyStroke, Shortcut};
use fern_core::signal::Signal;

/// Shared, app-wide state for the debug inspector.
///
/// A single instance is registered into `app_state` by
/// `install_inspector_in_debug` and shared across every window. Future
/// slices will extend it with selected-widget id, picker mode, panel
/// height, theme draft, etc.
#[derive(Clone)]
pub struct InspectorState {
    /// Whether the inspector panel is currently visible. Toggled by
    /// F12, the toolbar Close button, the Esc key, and the
    /// `--fern-inspector` / `FERN_INSPECTOR` boot flags.
    pub open: Signal<bool>,
}

impl InspectorState {
    fn new(initial_open: bool) -> Self {
        Self {
            open: Signal::new(initial_open),
        }
    }
}

impl std::fmt::Debug for InspectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorState")
            .field("open", &self.open.get())
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

    // F12 wiring lives inside the post-root closure so each window
    // gets its own shortcut owned by that window's user root.
    let toggle_for_post_root = state.open.clone();

    let post_root = DefaultPostRoot::new(move |tree, root_id| {
        let toggle = toggle_for_post_root.clone();
        let shortcut = Shortcut::new("__fern_inspector.toggle")
            .name("Toggle Inspector")
            .primary(KeyStroke::new(Key::F12, Modifiers::empty()))
            .on_activate(move |_ks, _ctx| {
                let next = !toggle.get();
                toggle.set(next);
                eprintln!("[fern-inspector] toggle = {next}");
                // No Action consumes this intent — it's a side-effect-only
                // shortcut. The intent dispatch dissipates harmlessly.
                Intent::new("__fern_inspector.toggle")
            })
            .build();
        tree.shortcut_registry_mut().register_owned(shortcut, root_id);
        // Slice 1: no visible wrapper yet. Wrapping ships in slice 2.
        root_id
    });

    builder
        .app_state(state)
        .app_state(post_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_state_signal_is_clonable() {
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
}
