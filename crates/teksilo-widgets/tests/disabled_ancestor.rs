// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A control inside a **disabled ancestor** must look disabled.
//!
//! This used to be false for every interactive widget in the catalog. A widget
//! derives its disabled state from `ctx.effective_enabled_signal(self_id)` in
//! its own `build()`, and that used to walk the ancestor chain at call time —
//! but `insert_widget` inserts a node with `parent: None` and wires the parent
//! link only *after* `build()` returns. The walk therefore saw an empty chain
//! and captured the widget's OWN `enabled` prop as the whole answer, for the
//! rest of its mounted life. A Button in a disabled form painted its live
//! accent fill forever: inert to clicks (the arena blocks events) and correct
//! to a screen reader (AccessKit reads the live tree), but visually *lit*.
//!
//! The signal is now node-resident and refreshed by the framework from the live
//! arena (`WidgetTree::flush_effective_enabled_signals`), so these hold.
//!
//! The test renders each widget three ways and compares the painted quads:
//!
//! * `E` — enabled
//! * `S` — disabled via its own `.enabled(false)` builder
//! * `A` — the widget itself untouched, but its parent `VStack` disabled
//!
//! `A` must equal `S`, and must differ from `E`. Comparing against BOTH matters:
//! an earlier version of this test disabled `S` with `tree.enabled_when(id, ..)`
//! *after* the widget had already built, which left `S` just as stale as `A` —
//! the two matched, and the test passed while the bug was wide open.

use teksilo_canvas::{RenderFrame, SizeProposal};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::button::{Button, ButtonVariant};
use teksilo_widgets::checkbox::Checkbox;
use teksilo_widgets::combo_box::ComboBox;
use teksilo_widgets::primitives::VStack;
use teksilo_widgets::slider::Slider;
use teksilo_widgets::spin_box::SpinBox;
use teksilo_widgets::toggle::Toggle;

/// Every fill/stroke colour in the frame. SDF shapes and tier-1 decoration
/// rects both, since which pipeline a `RectWidget` lands in depends on its
/// corner radius.
fn colors(frame: &RenderFrame) -> Vec<[f32; 4]> {
    frame
        .shapes
        .iter()
        .map(|s| s.color)
        .chain(frame.decorations.iter().map(|d| d.color))
        .collect()
}

fn same(a: &[[f32; 4]], b: &[[f32; 4]]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.iter().zip(y.iter()).all(|(p, q)| (p - q).abs() < 1e-4))
}

fn render(w: impl Widget + 'static) -> Vec<[f32; 4]> {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(w);
    tree.layout(SizeProposal::exact(300.0, 80.0));
    colors(&tree.render())
}

/// Render the widget with its own `enabled` untouched, inside a `VStack` that
/// is disabled. Uses the ordinary `.child(w)` builder idiom — the one that
/// inserts the child parentless, and so is the path that was broken.
fn render_in_disabled_parent(w: impl Widget + 'static) -> Vec<[f32; 4]> {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let form = tree.add(VStack::new().child(w));
    tree.enabled_when(form, false);
    tree.layout(SizeProposal::exact(300.0, 80.0));
    colors(&tree.render())
}

/// `enabled` / `self_disabled` build the same widget, differing only in
/// `.enabled(false)`.
fn assert_dims_from_ancestor<W: Widget + 'static>(
    name: &str,
    enabled: impl Fn() -> W,
    self_disabled: impl Fn() -> W,
) {
    let e = render(enabled());
    let s = render(self_disabled());
    let a = render_in_disabled_parent(enabled());

    assert!(
        !same(&e, &s),
        "{name}: test is vacuous — enabled and self-disabled render identically, \
         so it cannot detect a failure to dim"
    );
    assert!(
        !same(&a, &e),
        "{name}: disabled only by an ancestor, it still renders exactly like an \
         ENABLED widget — it looks clickable but is inert"
    );
    assert!(
        same(&a, &s),
        "{name}: disabled by an ancestor, it renders neither like an enabled nor \
         like a self-disabled widget"
    );
}

#[test]
fn button_dims_inside_a_disabled_ancestor() {
    assert_dims_from_ancestor(
        "Button(Filled)",
        || Button::new(lit!("Save")).variant(ButtonVariant::Filled),
        || {
            Button::new(lit!("Save"))
                .variant(ButtonVariant::Filled)
                .enabled(false)
        },
    );
}

#[test]
fn toggle_dims_inside_a_disabled_ancestor() {
    assert_dims_from_ancestor(
        "Toggle",
        || Toggle::new(Signal::new(true)),
        || Toggle::new(Signal::new(true)).enabled(false),
    );
}

#[test]
fn checkbox_dims_inside_a_disabled_ancestor() {
    assert_dims_from_ancestor(
        "Checkbox",
        || Checkbox::new(Signal::new(true)),
        || Checkbox::new(Signal::new(true)).enabled(false),
    );
}

#[test]
fn slider_dims_inside_a_disabled_ancestor() {
    assert_dims_from_ancestor(
        "Slider",
        || Slider::new(Signal::new(0.5_f32), 0.0, 1.0),
        || Slider::new(Signal::new(0.5_f32), 0.0, 1.0).enabled(false),
    );
}

#[test]
fn combo_box_dims_inside_a_disabled_ancestor() {
    assert_dims_from_ancestor(
        "ComboBox",
        || ComboBox::new(vec![lit!("a")], Signal::new(Some("a".to_string()))),
        || ComboBox::new(vec![lit!("a")], Signal::new(Some("a".to_string()))).enabled(false),
    );
}

#[test]
fn spin_box_dims_inside_a_disabled_ancestor() {
    assert_dims_from_ancestor(
        "SpinBox",
        || SpinBox::new(Signal::new(5_i32), 0, 100),
        || SpinBox::new(Signal::new(5_i32), 0, 100).enabled(false),
    );
}

/// The live path: the form's `enabled` signal flips after everything is
/// mounted. The button must dim with no rebuild, and light up again on re-enable.
#[test]
fn button_follows_an_ancestor_toggled_after_mount() {
    let enabled = Signal::new(true);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let form =
        tree.add(VStack::new().child(Button::new(lit!("Save")).variant(ButtonVariant::Filled)));
    tree.enabled_when(form, enabled.clone());
    tree.layout(SizeProposal::exact(300.0, 80.0));
    let lit_up = colors(&tree.render());

    enabled.set(false);
    tree.layout(SizeProposal::exact(300.0, 80.0));
    let dimmed = colors(&tree.render());
    assert!(
        !same(&lit_up, &dimmed),
        "flipping the form's enabled signal must dim the button"
    );

    enabled.set(true);
    tree.layout(SizeProposal::exact(300.0, 80.0));
    assert!(
        same(&colors(&tree.render()), &lit_up),
        "re-enabling the form must restore the button exactly"
    );
}
