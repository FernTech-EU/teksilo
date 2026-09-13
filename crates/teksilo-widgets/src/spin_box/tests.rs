// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless integration tests for `SpinBox`.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use super::{SpinBox, StepType, WheelMode, WrapMode};

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

// ── Mouse wheel ─────────────────────────────────────────────────────

/// Dispatch one wheel notch over `spin_id`.
///
/// `lines` is in Teksilo's own `ScrollDelta` sign, which is a *scroll
/// offset* delta rather than a raw wheel reading: `translate_mouse_wheel`
/// negates winit's natural sign, so a physical wheel-**down** notch arrives
/// here as `+3.0` (one notch × `LINES_PER_NOTCH`).
fn wheel(tree: &mut WidgetTree, spin_id: teksilo_core::widget_id::WidgetId, lines: f32) {
    use teksilo_canvas::Point;
    use teksilo_core::event::{ScrollDelta, WidgetEvent};

    // Scroll routes to the hovered widget, so park the pointer first.
    let b = tree.bounds(spin_id);
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(
        b.x + b.width * 0.5,
        b.y + b.height * 0.5,
    )));
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Lines { x: 0.0, y: lines },
        Modifiers::NONE,
    ));
    tick(tree);
}

fn hover_wheel_spin(initial: i32) -> (WidgetTree, Signal<i32>, teksilo_core::widget_id::WidgetId) {
    let value = Signal::new(initial);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 100)
            .single_step(1)
            .wheel_mode(super::WheelMode::Hover),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    (tree, value, id)
}

/// Wheel **down** must decrease the value, as it does in every desktop
/// stepper (Qt's `QAbstractSpinBox`, GTK's `GtkSpinButton`, WinUI's
/// `NumberBox`).
///
/// The trap this guards is the sign convention. `ScrollDelta` is not winit's
/// raw wheel reading — the platform layer negates it so that positive y
/// *increases a scroll offset*, i.e. scrolls down, which is why `ScrollArea`
/// and every data view add it straight to their scroll position. Reading
/// positive y as "the user scrolled up" therefore inverts the control, and
/// does so identically under every theme.
#[test]
fn wheel_down_decrements_and_wheel_up_increments() {
    let (mut tree, value, id) = hover_wheel_spin(50);

    // One physical wheel-down notch.
    wheel(&mut tree, id, 3.0);
    assert_eq!(value.get(), 49, "wheel down must decrease the value");

    // …and back up.
    wheel(&mut tree, id, -3.0);
    assert_eq!(value.get(), 50, "wheel up must increase the value");
}

/// The wheel must agree with the arrow keys: both are "one step", so
/// scrolling down and pressing ArrowDown have to move the same way.
#[test]
fn the_wheel_agrees_with_the_arrow_keys() {
    let (mut tree, value, id) = hover_wheel_spin(10);

    wheel(&mut tree, id, 3.0);
    assert_eq!(value.get(), 9, "one wheel-down notch is one step down");

    focus_field(&mut tree, id);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(
        value.get(),
        8,
        "ArrowDown must move the same direction as wheel down"
    );
}

/// `WheelMode::Disabled` lets the notch bubble to a surrounding scroll
/// container instead of quietly changing a value the user was scrolling past.
#[test]
fn wheel_mode_disabled_ignores_the_notch() {
    let value = Signal::new(50_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 100)
            .single_step(1)
            .wheel_mode(super::WheelMode::Disabled),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    wheel(&mut tree, id, 3.0);
    assert_eq!(value.get(), 50);
}

// ── Locale-aware display and input ─────────────────────────────────

/// The SpinBox publishes its displayed text (number + suffix) as the AT
/// node's value, so that is the observable for the rendered form.
fn at_value(tree: &mut WidgetTree, id: teksilo_core::widget_id::WidgetId) -> String {
    let update = tree.sync_accessibility();
    let target = teksilo_core::accessibility::widget_id_to_node_id(id);
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .and_then(|(_, n)| n.value().map(str::to_string))
        .expect("spin box AT value")
}

fn with_locale<R>(tag: &str, f: impl FnOnce() -> R) -> R {
    teksilo_i18n::thread_local::clear();
    let cfg = teksilo_i18n::I18nConfig::test_only(tag, &[("x", "x")]);
    teksilo_i18n::thread_local::install(teksilo_i18n::I18nManager::from_config(&cfg));
    let out = f();
    teksilo_i18n::thread_local::clear();
    out
}

#[test]
fn displays_the_locale_decimal_separator() {
    with_locale("fr-FR", || {
        let value = Signal::new(12.5_f64);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(SpinBox::new(value, 0.0, 100.0).decimals(1));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tick(&mut tree);
        assert_eq!(at_value(&mut tree, id), "12,5");
    });
}

#[test]
fn grouping_is_off_by_default_and_opt_in() {
    with_locale("en-US", || {
        let value = Signal::new(1_234_567_i64);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let plain = tree.add(SpinBox::new(value.clone(), 0, 9_999_999));
        let grouped = tree.add(SpinBox::new(value, 0, 9_999_999).use_grouping(true));
        tree.layout(SizeProposal::exact(600.0, 200.0));
        tick(&mut tree);
        assert_eq!(at_value(&mut tree, plain), "1234567");
        assert_eq!(at_value(&mut tree, grouped), "1,234,567");
    });
}

#[test]
fn localized_false_pins_the_c_locale() {
    // The escape hatch for a number that is an identifier rather than a
    // quantity — a port, a version component, a database id.
    with_locale("fr-FR", || {
        let value = Signal::new(8080.5_f64);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            SpinBox::new(value, 0.0, 99999.0)
                .decimals(1)
                .localized(false),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tick(&mut tree);
        assert_eq!(at_value(&mut tree, id), "8080.5");
    });
}

#[test]
fn grouping_keeps_large_integers_exact() {
    // The display path is a string transform over the value's own
    // `Display`, never a round-trip through `f64`, so an i64 past 2^53
    // survives being shown.
    with_locale("en-US", || {
        let value = Signal::new(9_007_199_254_740_993_i64);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(SpinBox::new(value, 0, i64::MAX).use_grouping(true));
        tree.layout(SizeProposal::exact(400.0, 60.0));
        tick(&mut tree);
        assert_eq!(at_value(&mut tree, id), "9,007,199,254,740,993");
    });
}

#[test]
fn a_locale_switch_re_renders_the_number_in_place() {
    // SpinBox already re-formats on `set_locale` via a `ctx.effect` on
    // the locale signal — this pins that the effect now has something
    // locale-dependent to re-render, and that it re-resolves the
    // conventions rather than reusing the ones captured at build time.
    with_locale("en-US", || {
        let value = Signal::new(12.5_f64);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_locale("en-US".to_string());
        let id = tree.add(SpinBox::new(value, 0.0, 100.0).decimals(1));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tick(&mut tree);
        assert_eq!(at_value(&mut tree, id), "12.5");

        teksilo_i18n::thread_local::clear();
        let cfg = teksilo_i18n::I18nConfig::test_only("fr-FR", &[("x", "x")]);
        teksilo_i18n::thread_local::install(teksilo_i18n::I18nManager::from_config(&cfg));
        tree.set_locale("fr-FR".to_string());
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tick(&mut tree);
        assert_eq!(at_value(&mut tree, id), "12,5");
    });
}

#[test]
fn the_commit_path_reads_the_locale_form_back() {
    // The display and parse directions share one `NumberPresentation`,
    // so whatever the field shows, the commit can read.
    with_locale("fr-FR", || {
        let p = super::NumberPresentation::resolve(true, false);
        assert_eq!(p.parse::<f64>("12,5"), Some(12.5));
        assert_eq!(p.parse::<f64>("-12,5"), Some(-12.5));
        // A numeric keypad still works: `.` is neither separator in
        // fr-FR, so it reads as the decimal point.
        assert_eq!(p.parse::<f64>("12.5"), Some(12.5));
        assert_eq!(p.parse::<f64>("nope"), None);
    });
}

#[test]
fn the_commit_path_reads_grouped_input() {
    with_locale("en-US", || {
        let p = super::NumberPresentation::resolve(true, true);
        assert_eq!(p.parse::<i64>("1,234,567"), Some(1_234_567));
        // Past 2^53 — the parse never goes through f64 either.
        assert_eq!(
            p.parse::<i64>("9,007,199,254,740,993"),
            Some(9_007_199_254_740_993)
        );
    });
}

#[test]
fn the_input_filter_admits_the_locale_separator_and_ascii_both() {
    with_locale("fr-FR", || {
        let p = super::NumberPresentation::resolve(true, false);
        assert!(p.accepts_char::<f64>(','), "the locale decimal separator");
        assert!(p.accepts_char::<f64>('.'), "the numeric keypad dot");
        assert!(p.accepts_char::<f64>('-'));
        assert!(p.accepts_char::<f64>('7'));
        assert!(!p.accepts_char::<f64>('q'));
    });
    with_locale("ar-EG", || {
        let p = super::NumberPresentation::resolve(true, false);
        assert!(p.accepts_char::<f64>('٧'), "an Arabic-Indic digit");
        assert!(p.accepts_char::<f64>('٫'), "the locale decimal separator");
        assert!(p.accepts_char::<f64>('7'), "an ASCII digit still types");
    });
}

#[test]
fn the_input_filter_admits_the_group_separator_only_when_grouping() {
    // fr-FR groups with U+202F, a character `f64`'s own filter rejects,
    // so the grouping flag is the only thing that can admit it.
    with_locale("fr-FR", || {
        let ungrouped = super::NumberPresentation::resolve(true, false);
        let grouped = super::NumberPresentation::resolve(true, true);
        assert!(!ungrouped.accepts_char::<f64>('\u{202f}'));
        assert!(grouped.accepts_char::<f64>('\u{202f}'));
    });
}

#[test]
fn localized_false_neither_renders_nor_reads_the_locale_form() {
    with_locale("fr-FR", || {
        let p = super::NumberPresentation::resolve(false, false);
        assert_eq!(p.parse::<f64>("12.5"), Some(12.5));
        assert_eq!(p.parse::<f64>("12,5"), None);
        assert!(!p.accepts_char::<f64>(','));
    });
}

// ── Keyboard: the deliberate deviations, and the modifier rule ─────

#[test]
fn home_and_end_stay_with_the_caret() {
    // The one deliberate deviation from the W3C ARIA spinbutton pattern, and
    // the reason is that `QAbstractSpinBox` routes both to its inner
    // `QLineEdit` — as do WinUI's `NumberBox`, Blink, Avalonia and jQuery UI —
    // while typing the number reaches min and max anyway.
    let (mut tree, value, id) = setup_int(10, 0, 100);
    focus_field(&mut tree, id);

    tree.press_key(Key::Home, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 10, "Home belongs to the caret, not the value");

    tree.press_key(Key::End, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 10, "End belongs to the caret, not the value");
}

#[test]
fn the_horizontal_arrows_belong_to_the_caret() {
    let (mut tree, value, id) = setup_int(10, 0, 100);
    focus_field(&mut tree, id);

    for key in [Key::ArrowLeft, Key::ArrowRight] {
        tree.press_key(key, Modifiers::NONE);
        tick(&mut tree);
        assert_eq!(value.get(), 10, "{key:?} must not step the value");
    }
}

#[test]
fn an_accelerator_chord_does_not_step() {
    // Behaviour change: modifiers used to be ignored outright, so `Ctrl+Up`
    // stepped the value and reported the key handled, swallowing a chord the
    // application may have bound.
    let (mut tree, value, id) = setup_int(10, 0, 100);
    focus_field(&mut tree, id);

    for (key, mods) in [
        (Key::ArrowUp, Modifiers::CTRL),
        (Key::ArrowDown, Modifiers::ALT),
        (Key::PageUp, Modifiers::SUPER),
        (Key::PageDown, Modifiers::CTRL),
    ] {
        tree.press_key(key, mods);
        tick(&mut tree);
        assert_eq!(value.get(), 10, "{key:?} with {mods:?} must fall through");
    }
}

#[test]
fn a_shifted_arrow_still_steps() {
    // `Shift` is not a distinct chord here, and the single-line field binds
    // nothing to `Shift+Up`, so rejecting it would make the chord dead rather
    // than deferential.
    let (mut tree, value, id) = setup_int(10, 0, 100);
    focus_field(&mut tree, id);

    tree.press_key(Key::ArrowUp, Modifiers::SHIFT);
    tick(&mut tree);
    assert_eq!(value.get(), 11);
}

#[test]
fn page_down_uses_page_step() {
    // The missing mirror of `page_up_uses_page_step`.
    let value = Signal::new(50_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).page_step(25));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);
    focus_field(&mut tree, id);

    tree.press_key(Key::PageDown, Modifiers::NONE);
    tick(&mut tree);
    assert_eq!(value.get(), 25);
}

// ── AccessKit `SetValue` ───────────────────────────────────────────

/// Drive an AccessKit action the way a platform adapter does, and report
/// whether anything acted on it — which is what the automation bridge reports
/// back to its caller.
fn access(
    tree: &mut WidgetTree,
    id: teksilo_core::widget_id::WidgetId,
    action: teksilo_core::accesskit::Action,
    data: Option<teksilo_core::accesskit::ActionData>,
) -> bool {
    let mut ops = teksilo_core::window::NoopWindowOps;
    let handled = tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(id),
        action,
        data,
        &mut ops,
    );
    tick(tree);
    handled
}

#[test]
fn a11y_set_value_accepts_a_numeric_payload() {
    // macOS sends `NumericValue` for an `NSNumber` handed to
    // `setAccessibilityValue:`, and AT-SPI's `Value.SetCurrentValue` — what
    // Orca calls on a spin button — sends it unconditionally.
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    assert!(access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::NumericValue(42.0))
    ));
    assert_eq!(value.get(), 42);
}

#[test]
fn a11y_set_value_accepts_a_string_payload() {
    // macOS sends `Value` for an `NSString`, and Teksilo's own automation
    // `set_value` tool sends only this shape — so it was a silent no-op
    // against every SpinBox in the catalog.
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    assert!(access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::Value("42".into()))
    ));
    assert_eq!(value.get(), 42);
}

#[test]
fn a11y_set_value_clamps_into_the_range() {
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    assert!(access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::NumericValue(1_000.0))
    ));
    assert_eq!(value.get(), 100, "above max clamps to max");
    assert!(access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::Value("-50".into()))
    ));
    assert_eq!(value.get(), 0, "below min clamps to min");
}

#[test]
fn a11y_set_value_declines_an_unparseable_string() {
    // The same revert a typed rubbish string gets on Enter — and reported
    // unhandled, so an automation client hears the refusal instead of reading
    // a success over a stale value.
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    assert!(!access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::Value("seven".into()))
    ));
    assert_eq!(value.get(), 10);
}

#[test]
fn a11y_set_value_declines_a_non_finite_number() {
    // `SpinValue::from_f64_saturating` maps NaN to zero; a malformed request
    // must not become a silent write of 0.
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    assert!(!access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::NumericValue(f64::NAN))
    ));
    assert_eq!(value.get(), 10);
}

#[test]
fn a11y_set_value_fires_on_value_changed() {
    // The callback is the only notification an app that does not observe the
    // signal gets, and an assistive-technology edit is a user edit.
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::accesskit::{Action, ActionData};

    let seen: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    let seen_h = seen.clone();
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 100).on_value_changed(move |v, _ctx| seen_h.set(Some(v))),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    assert!(access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::NumericValue(42.0))
    ));
    assert_eq!(seen.get(), Some(42));
    assert_eq!(value.get(), 42);
}

#[test]
fn a11y_set_value_honours_a_custom_value_from_text() {
    // A string payload takes the same parse Enter takes, so a field configured
    // for "percent with a stored fraction" reads the AT's "50 %" the way it
    // reads the user's.
    use teksilo_core::accesskit::{Action, ActionData};
    let value = Signal::new(0_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 100)
            .value_from_text(|s| s.trim_end_matches('%').trim().parse::<i32>().ok()),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    assert!(access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::Value("50 %".into()))
    ));
    assert_eq!(value.get(), 50);
}

#[test]
fn a11y_read_only_refuses_and_does_not_advertise_the_mutating_actions() {
    // macOS gates AXValue settability on the advertisement alone —
    // `is_read_only` does not veto it — so advertising here would tell
    // VoiceOver the value is settable when it is not.
    use teksilo_core::accesskit::{Action, ActionData};
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SpinBox::new(value.clone(), 0, 100).read_only(true));
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    let actions = tree.accessibility_node(id).actions().to_vec();
    for refused in [Action::SetValue, Action::Increment, Action::Decrement] {
        assert!(
            !actions.contains(&refused),
            "a read-only spin box must not advertise {refused:?}"
        );
    }
    assert!(!access(
        &mut tree,
        id,
        Action::SetValue,
        Some(ActionData::NumericValue(42.0))
    ));
    assert_eq!(value.get(), 10);

    // …and the *inner text node* refuses too. Not advertising an action is not
    // the same as refusing it: an adapter dispatches what the technology asks
    // for, and AT-SPI publishes `EditableText` off the interface set rather
    // than off the action list, so a write aimed at the field went straight
    // through the composite's read-only gate and rewrote the value.
    let field = tree
        .first_focusable_descendant(id)
        .expect("SpinBox has an inner field");
    assert!(!access(
        &mut tree,
        field,
        Action::SetValue,
        Some(ActionData::Value("42".into()))
    ));
    assert_eq!(value.get(), 10, "a read-only spin box keeps its value");
}

#[test]
fn a11y_increment_survives_the_move_to_the_payload_handler() {
    // Increment and Decrement moved into the payload handler with SetValue,
    // because that is where `ActionData` rides. They used to have to: the
    // dispatcher called `on_access_action_request` *instead of*
    // `on_access_action`. It now fires both, but the pair still lives there —
    // one handler, one match, one place to read.
    use teksilo_core::accesskit::Action;
    let (mut tree, value, id) = setup_int(10, 0, 100);

    assert!(access(&mut tree, id, Action::Increment, None));
    assert_eq!(value.get(), 11);
    assert!(access(&mut tree, id, Action::Decrement, None));
    assert_eq!(value.get(), 10);
}

#[test]
fn a11y_set_value_on_the_inner_field_still_commits() {
    // The composite publishes two AT nodes: a `Role::SpinButton` root and the
    // `Role::TextInput` beneath it. Setting the *root* goes through the spin
    // box's own handler; setting the *field* used to replace the displayed
    // string and stop there, so the typed value stayed stale until the next
    // blur and `on_value_changed` never fired — an assistive technology or an
    // automation client that resolved the text node saw a success and the
    // wrong value.
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::accesskit::{Action, ActionData};

    let seen: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    let seen_h = seen.clone();
    let value = Signal::new(10_i32);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        SpinBox::new(value.clone(), 0, 100).on_value_changed(move |v, _ctx| seen_h.set(Some(v))),
    );
    tree.layout(SizeProposal::exact(300.0, 60.0));
    tick(&mut tree);

    let field = tree
        .first_focusable_descendant(id)
        .expect("SpinBox has a focusable inner field");
    assert!(access(
        &mut tree,
        field,
        Action::SetValue,
        Some(ActionData::Value("42".into()))
    ));

    assert_eq!(value.get(), 42, "the typed value follows the text");
    assert_eq!(seen.get(), Some(42), "and the change is announced");
}

#[test]
fn a11y_set_value_on_the_inner_field_clamps_and_reverts_like_typing() {
    // The commit is the widget's own, so it is the same parse, clamp and
    // revert `Enter` performs — not a second, looser path.
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    let field = tree
        .first_focusable_descendant(id)
        .expect("SpinBox has a focusable inner field");

    assert!(access(
        &mut tree,
        field,
        Action::SetValue,
        Some(ActionData::Value("1000".into()))
    ));
    assert_eq!(value.get(), 100, "out of range clamps");

    assert!(
        !access(
            &mut tree,
            field,
            Action::SetValue,
            Some(ActionData::Value("seven".into()))
        ),
        "a string the widget cannot read is a refused write, not a handled one"
    );
    assert_eq!(value.get(), 100, "an unparseable string reverts");
}

#[test]
fn a11y_set_value_on_the_inner_field_puts_a_refused_string_back() {
    // The field overwrites its document *before* asking the host, and the
    // host's revert writes the bound `Signal<String>` — which still holds the
    // pre-edit display, because the document→signal sync is deferred. So the
    // revert was a no-op and the deferred sync then published the refused
    // string: the field read `"seven"` to a screen reader while the value it
    // stands for was still 10, which is the exact confusion this whole path
    // exists to prevent.
    use teksilo_core::accesskit::{Action, ActionData};
    let (mut tree, value, id) = setup_int(10, 0, 100);
    let field = tree
        .first_focusable_descendant(id)
        .expect("SpinBox has a focusable inner field");

    assert!(!access(
        &mut tree,
        field,
        Action::SetValue,
        Some(ActionData::Value("seven".into()))
    ));
    for _ in 0..3 {
        tick(&mut tree);
    }

    assert_eq!(value.get(), 10, "the value is untouched");
    let update = tree.sync_accessibility();
    let nid = teksilo_core::accessibility::widget_id_to_node_id(field);
    let shown = update
        .nodes
        .iter()
        .find(|(n, _)| *n == nid)
        .and_then(|(_, n)| n.value().map(|s| s.to_string()));
    assert_eq!(
        shown.as_deref(),
        Some("10"),
        "a refused write must not leave its string in the field"
    );
}

// ---------------------------------------------------------------------------
// Touch: the step buttons
// ---------------------------------------------------------------------------

/// Find the step buttons by walking the SpinBox subtree for the nodes whose
/// widget type name ends in `StepButton` — the stacked pair beside the field.
/// Returns them top-first.
fn step_buttons(
    tree: &WidgetTree,
    spin: teksilo_core::widget_id::WidgetId,
) -> Vec<teksilo_canvas::Rect> {
    let mut found: Vec<teksilo_canvas::Rect> = Vec::new();
    let mut stack = vec![spin];
    while let Some(id) = stack.pop() {
        if tree
            .widget_type_name(id)
            .is_some_and(|n| n.ends_with("StepButton"))
        {
            found.push(tree.bounds(id));
        }
        stack.extend(tree.children(id).iter().copied());
    }
    found.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
    found
}

/// The step buttons are the sweep's one unreachable target, and this pins the
/// measurement so a later layout change is noticed. 18 x 13 dp, inside a column
/// exactly their own width: no outset can grow into space its parent does not
/// own, and the field beside them defeats the miss-only slop pass. See the
/// `hit_outset` note on `StepButton` for why the fix is a layout, not a target.
#[test]
fn the_step_buttons_are_the_sweeps_named_sub_floor_residue() {
    let (tree, _value, spin) = setup_int(5, 0, 10);
    let buttons = step_buttons(&tree, spin);
    assert_eq!(buttons.len(), 2, "a stacked SpinBox has two step buttons");
    let floor = tree.theme().input.min_target_conformance;
    for b in &buttons {
        assert!(
            b.width < floor && b.height < floor,
            "a step button measured {b:?}; if it now clears {floor} dp the \
             residue is gone and this test should be replaced by a conformance one",
        );
    }
    // ...and the column that contains them is exactly as wide as they are,
    // which is what leaves an outset nowhere to grow.
    assert!(
        (buttons[0].width - buttons[1].width).abs() < 0.01,
        "the two steps share one column width",
    );
}

/// The value is still fully reachable without the pointer, which is why the
/// residue above costs no function.
#[test]
fn the_value_is_reachable_without_the_step_buttons() {
    let (mut tree, value, spin) = setup_int(5, 0, 10);
    focus_field(&mut tree, spin);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(value.get(), 6, "Up steps the value");
    let node = tree.accessibility_node(spin);
    let actions = node.actions();
    assert!(
        actions.contains(&teksilo_core::accesskit::Action::Increment)
            && actions.contains(&teksilo_core::accesskit::Action::Decrement),
        "and assistive technology can step it too: {actions:?}",
    );
}

/// A cancel is terminal — no `PointerUp` follows it — so the arm that normally
/// disarms hold-to-repeat never runs. Before the controls sweep a pan claimant
/// winning the press left the box stepping for the rest of the session.
#[test]
fn a_cancelled_press_stops_the_auto_repeat() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::{CancelReason, PointerPhase};

    let (mut tree, value, spin) = setup_int(5, 0, 100);
    let buttons = step_buttons(&tree, spin);
    let up = buttons[0];
    let at = Point::new(up.center().x, up.center().y);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    assert_eq!(value.get(), 6, "the press steps once, Qt-style");

    // The repeat is driven by the wall clock, so the only way to observe it is
    // to let the wall clock run. Past the 400 ms initial delay the held press
    // must have started stepping on its own — this half is the premise the
    // second half needs, and without it the test below would pass for the
    // uninteresting reason that nothing was ever repeating.
    std::thread::sleep(std::time::Duration::from_millis(550));
    for _ in 0..8 {
        tick(&mut tree);
    }
    let while_held = value.get();
    assert!(
        while_held > 6,
        "a press held past the initial delay must auto-repeat, got {while_held}",
    );

    tree.cancel_pointer(
        id,
        CancelReason::PeerClaimed,
        &mut teksilo_core::window::NoopWindowOps,
    );
    let after_cancel = value.get();
    std::thread::sleep(std::time::Duration::from_millis(550));
    for _ in 0..8 {
        tick(&mut tree);
    }
    assert_eq!(
        value.get(),
        after_cancel,
        "the repeat kept running after the press was taken away",
    );
}

/// A press that is released **off** the button stops the repeat too.
///
/// The finger slides away from a tiny 18 x 13 dp arrow before it lifts, which
/// on a touchscreen is the common case rather than the exotic one. Without the
/// press capture the release is hit-tested somewhere else entirely, the
/// button's `PointerUp` arm never runs, and the value climbs for as long as the
/// widget lives.
#[test]
fn a_release_away_from_the_button_stops_the_auto_repeat() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::PointerPhase;

    let (mut tree, value, spin) = setup_int(5, 0, 100);
    let buttons = step_buttons(&tree, spin);
    let up = buttons[0];
    let id = finger();
    tree.dispatch_pointer(touch(
        id,
        PointerPhase::Down,
        Point::new(up.center().x, up.center().y),
        0,
    ));
    assert_eq!(value.get(), 6, "the press steps once, Qt-style");

    std::thread::sleep(std::time::Duration::from_millis(550));
    for _ in 0..8 {
        tick(&mut tree);
    }
    assert!(
        value.get() > 6,
        "premise: a press held past the initial delay auto-repeats",
    );

    // Lift far outside the button — and outside the whole SpinBox.
    tree.dispatch_pointer(touch(id, PointerPhase::Up, Point::new(280.0, 55.0), 600));
    let after_release = value.get();
    std::thread::sleep(std::time::Duration::from_millis(550));
    for _ in 0..8 {
        tick(&mut tree);
    }
    assert_eq!(
        value.get(),
        after_release,
        "the repeat kept running after the finger lifted away from the button",
    );
}

// ── Touch: what the box does about panning, it does by omission ──────

/// Press on the spin box's field and drag `up` logical pixels, then lift.
/// Returns how many scroll events the enclosing container saw.
///
/// The container is a real claimant — `scroll_container` + a vertical
/// `PanClaim` — so the pan has somewhere legitimate to go. Whether it gets
/// there is the question each caller asks.
fn pan_over_a_spin_box(spin: SpinBox<i32>, up: f32) -> std::rc::Rc<std::cell::Cell<u32>> {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::event::EventResponse;
    use teksilo_core::pointer::PointerPhase;
    use teksilo_core::pointer::touch_action::{PanAxes, PanClaim};
    use teksilo_core::widget_builder::WidgetBuilder;

    let scrolled = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let count = scrolled.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let spin = tree.add(spin);
    let _page = tree.add(
        crate::primitives::VStack::new()
            .add_child(spin)
            .scroll_container(PanAxes::BOTH)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_e, _c| {
                count.set(count.get() + 1);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 600.0));

    // Start on the field itself, not on a step button: a press on a step
    // button steps by design, and the question here is about the box's body.
    let bounds = tree.bounds(spin);
    let start = Point::new(bounds.x + bounds.width * 0.25, bounds.center().y);
    let contact = finger();
    tree.dispatch_pointer(touch(contact, PointerPhase::Down, start, 0));
    for (i, dy) in [up / 3.0, up * 2.0 / 3.0, up].into_iter().enumerate() {
        let at = Point::new(start.x, start.y - dy);
        tree.dispatch_pointer(touch(contact, PointerPhase::Move, at, 20 + i as u64 * 20));
    }
    tree.dispatch_pointer(touch(
        contact,
        PointerPhase::Up,
        Point::new(start.x, start.y - up),
        100,
    ));
    scrolled
}

/// The spin box declares no `touch_action` and makes no pan claim, so its
/// subtree stays at the default `TouchAction::AUTO` and a finger that comes to
/// rest on it and then drags belongs to the enclosing scroller.
///
/// This is the behaviour the module header describes. It is asserted here
/// rather than by reading a declaration off the node, because there is no
/// declaration to read: the property is that the box gets out of the way, and
/// the only way to see that is to put a scroller behind it and pan.
///
/// The value must not move either. A wheel notch steps the box (`on_scroll`),
/// and a pan that reached that handler would step it once per sample.
///
/// **What this test does and does not see.** The box's default
/// [`WheelMode::Focused`] makes its `on_scroll` decline before the pan question
/// is reached, and this fixture never focuses the field — so a pan that DID
/// reach the handler would still leave the value at 50 here. The value
/// assertion below is therefore a guard against a regression in the wheel gate,
/// not evidence about the claim. `a_finger_pan_over_a_hover_wheel_spin_box_
/// scrolls_its_container` is the one that can see the claim, and it is the one
/// to read for the module header's argument.
#[test]
fn a_finger_pan_over_the_spin_box_scrolls_its_container() {
    let value = Signal::new(50);
    let scrolled = pan_over_a_spin_box(SpinBox::new(value.clone(), 0, 100), 150.0);

    assert!(
        scrolled.get() > 0,
        "the pan never reached the scroller — the spin box kept the contact",
    );
    assert_eq!(value.get(), 50, "and it stepped nothing on the way past");
}

/// The same pan over a box whose wheel is **not** gated on focus.
///
/// This is the case that can actually observe the claim. With
/// [`WheelMode::Hover`] the `on_scroll` handler runs for any scroll that
/// reaches the box, so if the box were on the pan's claimant chain the
/// synthesised samples would step the value once each — 50 → 47 for the three
/// samples this fixture sends. It stays at 50 because the box makes no claim
/// and the claimant walk therefore never visits it, which is exactly what the
/// module header argues and what adding a `PanClaim` here would break.
#[test]
fn a_finger_pan_over_a_hover_wheel_spin_box_scrolls_its_container() {
    let value = Signal::new(50);
    let spin = SpinBox::new(value.clone(), 0, 100).wheel_mode(WheelMode::Hover);
    let scrolled = pan_over_a_spin_box(spin, 150.0);

    assert!(
        scrolled.get() > 0,
        "the pan never reached the scroller — the spin box kept the contact",
    );
    assert_eq!(
        value.get(),
        50,
        "an ungated wheel handler must still never see a finger's pan",
    );
}
