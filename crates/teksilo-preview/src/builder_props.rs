// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The shared `WidgetBuilder` property surface, as grouped [`KnobSpec`]s.
//!
//! Every widget shares the framework-level value setters from `WidgetBuilder`
//! (visibility, focus, accessibility, …). A design tool lists them once, in a
//! dedicated panel, grouped into collapsible sections — separate from each
//! widget's own [`knobs()`](crate::WidgetCatalog::knobs).
//!
//! These are **optional overrides**: a knob's declared default is "unset"
//! (`None` / empty), meaning "inherit the widget's built-in behavior"; the tool
//! emits a `name: value` property only when the user sets one. The knob `id`
//! is the exact `WidgetBuilder` method name, so it lowers correctly in `teksu!`.
//! Closure handlers (`on_tap`, …) and slot setters are intentionally absent —
//! they are not value properties. Types the schema can't model as a typed
//! editor (the `accesskit::*` enums, button masks) are declared as text.

use crate::knob::KnobSpec;

/// The shared `WidgetBuilder` properties, grouped for a sectioned (ToolBox)
/// presentation. Each entry is `(group label, the group's knobs)`.
pub fn builder_property_groups() -> Vec<(&'static str, KnobSpec)> {
    vec![
        ("Framework", framework()),
        ("Gestures", gestures()),
        ("Accessibility", accessibility()),
    ]
}

/// Node-level framework properties.
fn framework() -> KnobSpec {
    KnobSpec::new()
        // A reactive `Prop<bool>` — usually a `#{ signal }`; edited as text.
        .opt_text("visible_when", "Visible when", None)
        .opt_bool("focusable", "Focusable", None)
        .opt_i32("tab_index", "Tab index", None, 0, 999)
        // `CursorIcon` is an external (winit) enum → text.
        .opt_text("cursor", "Cursor", None)
        .opt_bool("clips_children_on", "Clips children", None)
        .opt_bool("event_pass_through", "Event pass-through", None)
        .opt_bool("hit_transparent", "Hit transparent", None)
}

/// Which pointer buttons each gesture accepts (`ButtonMask` → text).
fn gestures() -> KnobSpec {
    KnobSpec::new()
        .opt_text("accept_tap_buttons", "Tap buttons", None)
        .opt_text("accept_double_tap_buttons", "Double-tap buttons", None)
        .opt_text("accept_triple_tap_buttons", "Triple-tap buttons", None)
        .opt_text("accept_long_press_buttons", "Long-press buttons", None)
}

/// Accessibility (AccessKit) properties. The `accesskit::*` enums are external,
/// so role/live/current/has-popup/orientation are declared as text.
fn accessibility() -> KnobSpec {
    KnobSpec::new()
        .opt_text("access_label", "Label", None)
        .opt_text("access_description", "Description", None)
        .opt_text("access_hint", "Hint", None)
        .opt_text("access_value", "Value", None)
        .opt_text("access_identifier", "Identifier", None)
        .opt_bool("access_hidden", "Hidden", None)
        .opt_bool("access_disabled", "Disabled", None)
        .opt_text("access_role", "Role", None)
        .opt_text("access_live", "Live", None)
        .opt_text("access_current", "Current", None)
        .opt_text("access_has_popup", "Has popup", None)
        .opt_text("access_orientation", "Orientation", None)
        // AccessKit numeric values are domain-specific (a slider could be 0–100);
        // these bounds are only nominal hints — the value is text-edited.
        .opt_f32("access_numeric_value", "Numeric value", None, -1.0e6, 1.0e6)
        .opt_f32("access_numeric_step", "Numeric step", None, -1.0e6, 1.0e6)
        .opt_text("access_shortcut_literal", "Shortcut", None)
        .opt_text("access_shortcut_id", "Shortcut id", None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_non_empty_and_named() {
        let groups = builder_property_groups();
        assert_eq!(groups.len(), 3);
        for (label, spec) in &groups {
            assert!(!label.is_empty());
            assert!(
                !spec.declarations().is_empty(),
                "group {label} has no knobs"
            );
        }
    }

    #[test]
    fn knob_ids_are_widgetbuilder_method_names() {
        // The id must be the exact builder method name so `name: value` lowers
        // to `.name(value)` in teksu!. Spot-check a few.
        let fw = framework();
        assert!(fw.get("focusable").is_some());
        assert!(fw.get("tab_index").is_some());
        assert!(fw.get("visible_when").is_some());
        assert!(accessibility().get("access_label").is_some());
    }
}
