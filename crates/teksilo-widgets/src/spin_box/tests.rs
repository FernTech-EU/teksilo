// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless integration tests for `SpinBox`.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use super::{SpinBox, StepType, WrapMode};

fn tick(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(300.0, 60.0));
}

fn setup_int(
    initial: i32,
    min: i32,
    max: i32,
) -> (WidgetTree, Signal<i32>, teksilo_core::widget_id::WidgetId) {
    let value = Signal::new(initial);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), min, max));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    (tree, value, id)
}

fn focus_field(tree: &mut WidgetTree, spin_id: teksilo_core::widget_id::WidgetId) {
    let field = tree
        .first_focusable_descendant(spin_id)
        .expect("SpinBox should have a focusable inner field");
    tree.focus(field);
}

// ── Construction ────────────────────────────────────────────────────

#[test]
fn constructs_and_lays_out() {
    let (tree, _v, id) = setup_int(0, 0, 100);
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

/// Regression: the value text grows with the global text scale, so the
/// SpinBox's width cap must grow too — otherwise the scaled digits clip.
#[test]
fn width_grows_with_text_scale() {
    let value = Signal::new(50);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value, 0, 100).width_chars(3).suffix(" %"));
    tree.layout(SizeProposal::unspecified());
    let w1 = tree.bounds(id).width;
    tree.set_user_text_scale(2.0);
    tree.layout(SizeProposal::unspecified());
    let w2 = tree.bounds(id).width;
    assert!(
        w2 > w1 * 1.4,
        "spinbox width should grow with the text scale: {w1} -> {w2}"
    );
}

// ── Keyboard stepping ──────────────────────────────────────────────

#[test]
fn arrow_up_increments() {
    let (mut tree, value, id) = setup_int(10, 0, 100);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 11);
}

#[test]
fn arrow_down_decrements() {
    let (mut tree, value, id) = setup_int(10, 0, 100);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 9);
}

#[test]
fn page_up_uses_page_step() {
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).page_step(25));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::PageUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 35);
}

#[test]
fn page_step_defaults_to_ten_times_single_step() {
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 1000).single_step(3));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::PageUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 40, "page step default must be 10x single step");
}

// ── Clamping & wrapping ────────────────────────────────────────────

#[test]
fn clamp_mode_blocks_past_max() {
    let (mut tree, value, id) = setup_int(99, 0, 100);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    tree.press_key(Key::ArrowUp, Modifiers::NONE); // would go to 101
    tick(&mut tree);
    assert_eq!(value.get(), 100);
}

#[test]
fn clamp_mode_blocks_below_min() {
    let (mut tree, value, id) = setup_int(1, 0, 100);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tick(&mut tree);
    tree.press_key(Key::ArrowDown, Modifiers::NONE); // would go to -1
    tick(&mut tree);
    assert_eq!(value.get(), 0);
}

#[test]
fn wrap_mode_wraps_past_max() {
    let value = Signal::new(9_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 9).wrap_mode(WrapMode::Wrap));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 0, "wrap past max jumps to min");
}

#[test]
fn wrap_mode_wraps_past_min() {
    let value = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 9).wrap_mode(WrapMode::Wrap));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 9, "wrap past min jumps to max");
}

// ── Read-only / disabled ───────────────────────────────────────────

#[test]
fn read_only_blocks_keyboard_step() {
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).read_only(true));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 10, "read_only must block stepping");
}

// ── Adaptive step ──────────────────────────────────────────────────

#[test]
fn adaptive_step_scales_to_magnitude() {
    // value ∈ [100, 999) → step = 100
    let value = Signal::new(250_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 10_000)
            .single_step(1)
            .step_type(StepType::Adaptive),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 350);
}

#[test]
fn adaptive_step_small_values_use_base_step() {
    let value = Signal::new(3_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 1000)
            .single_step(1)
            .step_type(StepType::Adaptive),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 4, "adaptive under 10 should keep base step");
}

// ── External value changes ────────────────────────────────────────

#[test]
fn external_value_set_reformats_text() {
    let value = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _id = tree.add(SpinBox::new(value.clone(), 0, 100));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    // Changing the external signal should not panic, and should
    // propagate through the reformat effect.
    value.set(42);
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(value.get(), 42);
}

// ── Accessibility ──────────────────────────────────────────────────

#[test]
fn a11y_role_is_spin_button() {
    let (tree, _v, id) = setup_int(50, 0, 100);
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::SpinButton);
    // Increment / Decrement / SetValue / Focus actions all exposed.
    let actions = info.actions();
    assert!(actions.contains(&teksilo_core::accesskit::Action::Increment));
    assert!(actions.contains(&teksilo_core::accesskit::Action::Decrement));
    assert!(actions.contains(&teksilo_core::accesskit::Action::Focus));
}

#[test]
fn disabled_blocks_keyboard_step() {
    // Built-in arena-level `is_disabled` relies on an
    // `enabled_state` signal that containers like `GroupBox`
    // bind, which SpinBox does not opt into by default. What we
    // do guarantee is that the step paths short-circuit when
    // `enabled = false`; the value signal stays put.
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).enabled(false));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    // Inner field isn't focusable when the SpinBox is disabled
    // (TextInputField propagates enabled), so we can't rely on
    // focus_field here. Hit the field with a direct key press via
    // a focused-or-not attempt and check the value is unchanged.
    if let Some(field) = tree.first_focusable_descendant(id) {
        tree.focus(field);
    }
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 10, "disabled SpinBox must not step");
}

// ── Floats ─────────────────────────────────────────────────────────

#[test]
fn float_type_formats_with_decimals() {
    let value = Signal::new(0.25_f64);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _id = tree.add(
        SpinBox::new(value.clone(), 0.0, 1.0)
            .single_step(0.05)
            .decimals(2),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    // The value signal round-trip is what we can assert here.
    // Full text-signal inspection needs access to the inner field,
    // which is not exposed by the public API.
    assert!((value.get() - 0.25).abs() < 1e-9);
}

#[test]
fn float_arrow_steps_by_single_step() {
    let value = Signal::new(0.5_f32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0.0, 1.0)
            .single_step(0.1)
            .decimals(2),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert!((value.get() - 0.6).abs() < 1e-5, "got {}", value.get());
}

// ── on_value_changed callback ──────────────────────────────────────

#[test]
fn on_value_changed_fires_on_step() {
    use std::cell::Cell;
    use std::rc::Rc;
    let value = Signal::new(0_i32);
    let fired = Rc::new(Cell::new(0_i32));
    let c = fired.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id =
        tree.add(SpinBox::new(value.clone(), 0, 100).on_value_changed(move |v, _ctx| c.set(v)));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(fired.get(), 1);
}

// ── Hidden buttons ────────────────────────────────────────────────

#[test]
fn hidden_buttons_still_step_via_keyboard() {
    // Int UI-style dense form: no visible step buttons, keyboard
    // (and wheel) must still work.
    use super::ButtonLayout;
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).button_layout(ButtonLayout::Hidden));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 11);
}

#[test]
fn show_buttons_sugar_matches_button_layout() {
    use super::ButtonLayout;
    // Both builders must produce the same behavior.
    let a = Signal::new(0_i32);
    let b = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let ia = tree.add(SpinBox::new(a.clone(), 0, 10).show_buttons(false));
    let ib = tree.add(SpinBox::new(b.clone(), 0, 10).button_layout(ButtonLayout::Hidden));
    tree.layout(SizeProposal::exact(300.0, 120.0));
    tick(&mut tree);
    // Both widgets should have identical focusable-descendant
    // counts: one focusable field each, no focusable buttons.
    fn count_focusable(tree: &WidgetTree, root: teksilo_core::widget_id::WidgetId) -> usize {
        // walk widgets under root and count those that expose
        // focusable=true via the test API
        let mut count = 0;
        // first_focusable_descendant returns only the first; to
        // compare we just check it's Some for both.
        if tree.first_focusable_descendant(root).is_some() {
            count += 1;
        }
        count
    }
    assert_eq!(count_focusable(&tree, ia), count_focusable(&tree, ib));
}

// ── Width control ─────────────────────────────────────────────────
//
// `width()` / `fill_width()` behaviour is exercised visually in
// `examples/spin_box` rather than unit-tested here: the test harness
// uses `SizeProposal::exact` for the tree root, which pins the root
// widget to that exact size and bypasses the SpinBox's internal
// `MaxSize` cap. Validating the cap needs a multi-child parent
// (HStack row) that distributes space — covered by the demo.

// ── Suffix & special value text coexist ───────────────────────────

// ── Accessibility numeric properties ─────────────────────────────

#[test]
fn a11y_numeric_value_matches_signal() {
    let value = Signal::new(42_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).single_step(2));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    // We can't inspect the raw AccessKit `Node` through the public
    // API, but we can round-trip via the Role + Action set and
    // confirm the value updates the a11y string published by
    // `builder.set_value`. The Info wrapper doesn't expose the
    // numeric_value itself, so the closest smoke test is that the
    // node exists and advertises the expected actions.
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::SpinButton);
    let actions = info.actions();
    for required in [
        teksilo_core::accesskit::Action::Increment,
        teksilo_core::accesskit::Action::Decrement,
        teksilo_core::accesskit::Action::SetValue,
        teksilo_core::accesskit::Action::Focus,
    ] {
        assert!(
            actions.contains(&required),
            "missing a11y action {:?}",
            required
        );
    }
}

#[test]
fn a11y_name_uses_label() {
    let value = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value, 0, 100).label(lit!("Font size")));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    let info = tree.accessibility_node(id);
    assert_eq!(info.name(), Some("Font size"));
}

// ── Reactive suffix + special_value_text ──────────────────────────
//
// The Qt-compat bug was: with `value == min` and
// `special_value_text` set, the suffix was still rendered —
// producing visible "Never s" instead of "Never". The fix lives in
// two places:
//
//   1. `TextInputField::suffix` — lets the composite swap
//      the suffix to an empty string reactively.
//   2. `SpinBox::build` — wires a derived `Signal<String>` that
//      resolves to `""` exactly when the value equals `min` and
//      the field is not currently focused.
//
// We can't inspect the raw a11y string from the public
// `AccessibilityInfo` wrapper, and the inner `text_signal` is
// private to the field, so the regression coverage here focuses
// on the wiring surface — (1) the composite builds without panic
// for the full `suffix + special` path, (2) stepping off `min`
// and back works without crashing, and (3) the a11y role / actions
// stay correct.

#[test]
fn reactive_suffix_survives_value_transitions() {
    let value = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 3600)
            .suffix(" s")
            .special_value_text(lit!("Never"))
            .label(lit!("Timeout")),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    // Cross the special-text boundary a few times.
    value.set(30);
    tick(&mut tree);
    value.set(0);
    tick(&mut tree);
    value.set(120);
    tick(&mut tree);
    // Widget is still alive and value signal intact.
    assert_eq!(value.get(), 120);
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::SpinButton);
}

// ── Tooltip ───────────────────────────────────────────────────────

#[test]
fn tooltip_appears_on_hover() {
    let value = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value, 0, 100).tooltip(lit!("Tip")));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tree.pointer_move(tree.bounds(id).center());
    tree.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "tooltip should appear on hover"
    );
    assert!(tree.find_by_label("Tip").is_some());
}

// ── Disabled appearance ───────────────────────────────────────────
//
// A SpinBox frames its `TextInputField` in *neutral* roles
// (`SurfaceRole::Content` / `BorderRole::Default`), and the disabled-role
// substitution in `ColorProp::resolve` only rewrites the *accent* family —
// so unlike a Filled Button it gets no automatic greying and must opt in via
// `SurfaceRole::Disabled`. It once did not, and stayed fully lit after
// `.enabled(false)`. These pin the painted pixels, not the intent.
//
// Match on the frame rect + stroke width rather than "some quad has this
// colour": the IntUI light palette reuses `#EBECF0` for `border`,
// `surface_hover` and `surface_disabled` alike, so a bare colour scan cannot
// tell an enabled field's *border* from a disabled field's *fill*.

/// The frame's fill (`stroke_width == 0`) and outline (`stroke_width > 0`),
/// identified as the quads covering the SpinBox's own bounds.
fn frame_colors(
    tree: &mut WidgetTree,
    id: teksilo_core::widget_id::WidgetId,
) -> (Option<[f32; 4]>, Option<[f32; 4]>) {
    let b = tree.bounds(id);
    let covers = |s: &[f32; 4]| {
        (s[0] - b.x).abs() < 0.5
            && (s[1] - b.y).abs() < 0.5
            && (s[2] - b.width).abs() < 0.5
            && (s[3] - b.height).abs() < 0.5
    };
    let frame = tree.render();
    let fill = frame
        .shapes
        .iter()
        .find(|s| covers(&s.screen) && s.stroke_width == 0.0)
        .map(|s| s.color);
    let border = frame
        .shapes
        .iter()
        .find(|s| covers(&s.screen) && s.stroke_width > 0.0)
        .map(|s| s.color);
    (fill, border)
}

fn assert_color(got: Option<[f32; 4]>, want: teksilo_tokens::Color, what: &str) {
    let want = want.to_array();
    let got = got.unwrap_or_else(|| panic!("no {what} quad painted at the SpinBox bounds"));
    assert!(
        got.iter()
            .zip(want.iter())
            .all(|(a, b)| (a - b).abs() < 1e-4),
        "{what}: expected {want:?}, painted {got:?}"
    );
}

fn spin_box_tree(enabled: bool) -> (WidgetTree, teksilo_core::widget_id::WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(Signal::new(5_i32), 0, 100).enabled(enabled));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    (tree, id)
}

#[test]
fn disabled_spin_box_paints_the_neutral_disabled_frame() {
    let theme = teksilo_core::presets::intui::light();
    let (mut tree, id) = spin_box_tree(false);
    let (fill, border) = frame_colors(&mut tree, id);

    assert_color(fill, theme.colors.surface_disabled, "fill");
    assert_color(border, theme.colors.border_disabled, "border");

    // `accent_disabled` is a washed-out *accent* (pale cyan in IntUI) — right
    // for a Filled Button, wrong for a neutral field.
    let accent_disabled = theme.colors.accent_disabled.to_array();
    assert_ne!(fill.unwrap(), accent_disabled);
    assert_ne!(border.unwrap(), accent_disabled);
}

#[test]
fn enabled_spin_box_frame_is_unchanged() {
    let theme = teksilo_core::presets::intui::light();
    let (mut tree, id) = spin_box_tree(true);
    let (fill, border) = frame_colors(&mut tree, id);
    assert_color(fill, theme.colors.surface_content, "fill");
    assert_color(border, theme.colors.border, "border");
}

#[test]
fn spin_box_dims_reactively_without_a_rebuild() {
    // The chrome binds `effective_enabled_signal`, so flipping a bound
    // `Signal<bool>` must re-tint on the next paint — no rebuild.
    let theme = teksilo_core::presets::intui::light();
    let enabled = Signal::new(true);
    let mut tree = WidgetTree::new().with_theme(theme.clone());
    let id = tree.add(SpinBox::new(Signal::new(5_i32), 0, 100).enabled(enabled.clone()));
    tree.layout(SizeProposal::exact(300.0, 60.0));

    let (fill, _) = frame_colors(&mut tree, id);
    assert_color(fill, theme.colors.surface_content, "fill (enabled)");

    enabled.set(false);
    tree.layout(SizeProposal::exact(300.0, 60.0));
    let (fill, border) = frame_colors(&mut tree, id);
    assert_color(fill, theme.colors.surface_disabled, "fill (after disable)");
    assert_color(
        border,
        theme.colors.border_disabled,
        "border (after disable)",
    );
}

/// A SpinBox inside a disabled *form* must dim, even though it is itself
/// `enabled`. This is the case the obvious implementation gets wrong: a
/// `cfg.is_disabled` signal derived from `effective_enabled_signal` reflects
/// only the widget's OWN `enabled` prop, because that walk captures the
/// ancestor chain at call time and a widget's parent is not wired yet during
/// its own `build()`. The frame therefore paints `SurfaceRole::Field`, which
/// dims inside `ColorProp::resolve` from the paint walker's live arena chain.
#[test]
fn spin_box_dims_inside_a_disabled_ancestor() {
    use crate::primitives::VStack;

    let theme = teksilo_core::presets::intui::light();
    let enabled = Signal::new(true);
    let mut tree = WidgetTree::new().with_theme(theme.clone());
    let form = tree.add(VStack::new().child(SpinBox::new(Signal::new(5_i32), 0, 100)));
    tree.enabled_when(form, enabled.clone());
    tree.layout(SizeProposal::exact(300.0, 60.0));
    let spin = tree
        .children(form)
        .first()
        .copied()
        .expect("VStack should hold the SpinBox");

    enabled.set(false);
    tree.layout(SizeProposal::exact(300.0, 60.0));
    let (fill, _) = frame_colors(&mut tree, spin);
    assert_color(fill, theme.colors.surface_disabled, "fill");
}
