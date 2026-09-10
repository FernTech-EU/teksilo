// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
use super::*;
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

#[test]
fn field_selection_color_swaps_on_window_active() {
    let colors = teksilo_core::presets::intui::light().colors;
    assert_eq!(
        field_selection_color(&colors, true, true),
        colors.selection_bg_active.to_array(),
        "active window uses the vivid selection colour"
    );
    assert_eq!(
        field_selection_color(&colors, false, true),
        colors.selection_bg_inactive.to_array(),
        "inactive window uses the muted selection colour"
    );
    assert_ne!(
        field_selection_color(&colors, true, true),
        field_selection_color(&colors, false, true)
    );
}

/// **A field that is not focused paints no selection at all**, in an
/// active window or a background one.
///
/// Dimming it was not enough: tab across a form of `SpinBox`es — each of
/// which selects all on keyboard focus — and every field left behind kept
/// a grey band, so the form read as a column of half-lit selections with
/// no way to tell which one the keystrokes went to. Native single-line
/// fields hide it outright (Win32 without `ES_NOHIDESEL`,
/// `TextBoxBase.HideSelection = true`, `QLineEdit`'s `deselect()` on
/// focus-out, AppKit detaching the field editor).
#[test]
fn field_selection_color_vanishes_when_the_field_is_not_focused() {
    let colors = teksilo_core::presets::intui::light().colors;
    assert_eq!(
        field_selection_color(&colors, true, false),
        [0.0; 4],
        "an unfocused field must paint no selection, even in an active window"
    );
    assert_eq!(field_selection_color(&colors, false, false), [0.0; 4]);
}

/// ...and the live field re-tints as focus comes and goes, rather than
/// keeping whatever colour it was built with.
#[test]
fn a_field_re_tints_its_selection_when_focus_leaves_it() {
    let colors = teksilo_core::presets::intui::light().colors;
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let a = tree.add(TextInputField::new(Signal::new("hello".to_string())));
    let b = tree.add(TextInputField::new(Signal::new("world".to_string())));
    tree.layout(SizeProposal::exact(200.0, 40.0));

    let tint = |tree: &WidgetTree, id| {
        tree.widget_as_any(id)
            .and_then(|w| w.downcast_ref::<TextInputField>())
            .and_then(|f| f.state.as_ref())
            .map(|st| st.borrow().selection_tint)
            .expect("a built field")
    };

    tree.focus(a);
    assert_eq!(
        tint(&tree, a),
        colors.selection_bg_active.to_array(),
        "the focused field paints its selection live"
    );

    tree.focus(b);
    assert_eq!(
        tint(&tree, a),
        [0.0; 4],
        "focus moved to another field and the first kept a visible selection"
    );
    assert_eq!(tint(&tree, b), colors.selection_bg_active.to_array());
}

#[test]
fn caret_hidden_when_window_inactive() {
    let text = Signal::new("hello".to_string());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInputField::new(text));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    let _ = tree.render();

    // Reach the built field's shared state (created lazily in build()) to
    // observe the caret-gate inputs directly — the caret paints as an
    // engine-internal fill, not a top-level decoration.
    let state = tree
        .widget_as_any(id)
        .and_then(|a| a.downcast_ref::<TextInputField>())
        .map(|f| f.state().clone())
        .expect("built TextInputField is reachable via as_any");

    // Focus the field by clicking its centre.
    let b = tree.bounds(id);
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // One frame so the blink turns the caret on (on_focus sets it on; the
    // 500 ms interval hasn't elapsed after a single 16 ms tick).
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(200.0, 40.0));

    assert!(state.borrow().has_focus, "field took focus");
    assert!(state.borrow().window_active);
    assert!(
        state.borrow().caret_visible.get(),
        "caret visible when focused in an active window"
    );

    // Window blur: caret hidden (effect clears it synchronously).
    tree.set_window_active(false);
    assert!(!state.borrow().window_active);
    assert!(
        !state.borrow().caret_visible.get(),
        "caret hidden while the window is inactive"
    );

    // Reactivate: caret returns immediately (field still holds focus).
    tree.set_window_active(true);
    assert!(
        state.borrow().caret_visible.get(),
        "caret restored on window reactivate"
    );
}
