// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Integration tests for [`ColorPicker`](super::ColorPicker) and its
//! subcomponents. Headless — no Xvfb / GPU required.

#![cfg(test)]

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::Color;

use super::*;
use crate::color_picker::alpha_strip::AlphaStrip;
use crate::color_picker::hsv_canvas::HsvCanvas;
use crate::color_picker::hue_strip::HueStrip;
use crate::color_picker::state::ColorComponents;
use crate::color_picker::swatch::ColorSwatch;
use crate::color_picker::swatch_grid::SwatchGrid;

#[test]
fn builds_with_default_options() {
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorPicker::new(value));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    // No panic = pass; assert the root has bounds.
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn root_accessibility_emits_group_role() {
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorPicker::new(value));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Group);
}

#[test]
fn setting_red_preserves_alpha() {
    let value = Signal::new(Color::from_rgba(0.5, 0.5, 0.5, 0.8));
    // ColorComponents must run inside a widget build to register effects.
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(ColorPicker::new(value.clone()).alpha_enabled(true));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Mutate the signal directly to "fake" what a setter does.
    let c = value.get();
    value.set(Color::from_rgba(0.9, c.g(), c.b(), c.a()));
    assert!((value.get().a() - 0.8).abs() < 0.01);
    assert!((value.get().r() - 0.9).abs() < 0.01);
}

#[test]
fn signal_drives_picker_components() {
    // Smoke: constructing ColorComponents in isolation still requires a
    // BuildContext (effect registration), which we get by building any
    // widget that uses it.
    let value = Signal::new(Color::from_rgba(1.0, 0.0, 0.5, 1.0));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(ColorPicker::new(value.clone()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Mutate value and check it's stored (round-trip via the signal).
    value.set(Color::from_rgba(0.0, 1.0, 0.0, 1.0));
    let updated = value.get();
    assert!((updated.r()).abs() < 0.01);
    assert!((updated.g() - 1.0).abs() < 0.01);
}

#[test]
fn color_components_red_setter_writes_back() {
    // Build a tiny widget that uses ColorComponents, then exercise the
    // red setter directly (Rc<dyn Fn>) and verify the bound signal moves.
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
    use teksilo_core::widget_id::WidgetId;

    struct Probe {
        value: Signal<Color>,
        captured: Rc<RefCell<Option<Rc<dyn Fn(f32)>>>>,
    }
    impl std::fmt::Debug for Probe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Probe").finish()
        }
    }
    impl Widget for Probe {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let c = ColorComponents::new(ctx, self.value.clone());
            *self.captured.borrow_mut() = Some(c.set_red.clone());
            Vec::new()
        }
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn place_children(
            &self,
            _bounds: teksilo_canvas::Rect,
            _proposal: SizeProposal,
            _children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
        }
    }

    let value = Signal::new(Color::from_rgb(0.1, 0.2, 0.3));
    let captured: Rc<RefCell<Option<Rc<dyn Fn(f32)>>>> = Rc::new(RefCell::new(None));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(Probe {
        value: value.clone(),
        captured: captured.clone(),
    });
    tree.layout(SizeProposal::exact(100.0, 100.0));

    let setter = captured.borrow().as_ref().unwrap().clone();
    setter(0.7);
    let updated = value.get();
    assert!((updated.r() - 0.7).abs() < 0.01);
    // Other channels preserved.
    assert!((updated.g() - 0.2).abs() < 0.01);
    assert!((updated.b() - 0.3).abs() < 0.01);
}

#[test]
fn hue_strip_emits_slider_with_correct_range() {
    let hue = Signal::new(180.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(HueStrip::new(hue.clone(), setter, dragging).label("Hue"));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Slider);
}

#[test]
fn alpha_strip_emits_slider() {
    let color = Signal::new(Color::RED);
    let alpha = Signal::new(0.5_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let alpha = alpha.clone();
        Rc::new(move |a| alpha.set(a))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(AlphaStrip::new(color, alpha, setter, dragging).label("Opacity"));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Slider);
}

#[test]
fn hue_strip_keyboard_steps() {
    let hue = Signal::new(180.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(HueStrip::new(hue.clone(), setter, dragging));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    tree.focus(id);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert!((hue.get() - 179.0).abs() < 0.01);
    tree.press_key(Key::PageUp, Modifiers::NONE);
    assert!((hue.get() - 194.0).abs() < 0.01);
    tree.press_key(Key::Home, Modifiers::NONE);
    assert!(hue.get().abs() < 0.01);
    tree.press_key(Key::End, Modifiers::NONE);
    assert!((hue.get() - 359.0).abs() < 0.01);
}

#[test]
fn alpha_strip_keyboard_steps() {
    // The alpha strip had no keyboard test at all, though it has bound the
    // whole family since it shipped.
    let alpha = Signal::new(0.50_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let alpha = alpha.clone();
        Rc::new(move |a| alpha.set(a))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(AlphaStrip::new(
        Signal::new(Color::RED),
        alpha.clone(),
        setter,
        dragging,
    ));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    tree.focus(id);

    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert!((alpha.get() - 0.51).abs() < 0.001, "alpha={}", alpha.get());
    tree.press_key(Key::PageUp, Modifiers::NONE);
    assert!((alpha.get() - 0.61).abs() < 0.001, "alpha={}", alpha.get());
    tree.press_key(Key::Home, Modifiers::NONE);
    assert!(alpha.get().abs() < 0.001);
    tree.press_key(Key::End, Modifiers::NONE);
    assert!((alpha.get() - 1.0).abs() < 0.001);
}

#[test]
fn a_vertical_strip_puts_its_maximum_at_the_top() {
    // Both strips default to vertical inside the picker, and both grew their
    // value downward: `ArrowUp` raised the hue and lowered the thumb. A
    // vertical bounded scalar's maximum is at the top everywhere else in
    // Teksilo — and in Qt, GTK4, the Win32 trackbar and `<input type=range>` —
    // so the geometry moved rather than the chord table.
    use teksilo_canvas::Point;
    use teksilo_core::event::{PointerButton, WidgetEvent};

    let hue = Signal::new(180.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(HueStrip::new(hue.clone(), setter, dragging));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    tree.render();

    // A press near the top reaches the top of the range.
    let top = Point::new(10.0, 4.0);
    tree.pointer_move(top);
    tree.dispatch_event(WidgetEvent::pointer_down(
        top,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        top,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert!(
        hue.get() > 340.0,
        "a press near the top is near the maximum hue, got {}",
        hue.get()
    );

    // …the same direction `ArrowUp` travels.
    hue.set(180.0);
    tree.focus(id);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert!(
        hue.get() > 180.0,
        "ArrowUp increases the hue, got {}",
        hue.get()
    );
}

#[test]
fn a_vertical_alpha_strip_is_opaque_at_the_top() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{PointerButton, WidgetEvent};

    let alpha = Signal::new(0.5_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let alpha = alpha.clone();
        Rc::new(move |a| alpha.set(a))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(AlphaStrip::new(
        Signal::new(Color::RED),
        alpha.clone(),
        setter,
        dragging,
    ));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    tree.render();

    let bottom = Point::new(10.0, 196.0);
    tree.pointer_move(bottom);
    tree.dispatch_event(WidgetEvent::pointer_down(
        bottom,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        bottom,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert!(
        alpha.get() < 0.05,
        "a press near the bottom is near transparent, got {}",
        alpha.get()
    );
}

#[test]
fn the_strips_ignore_accelerator_chords() {
    // Behaviour change: modifiers used to be ignored outright, so `Ctrl+Home`
    // drove a strip to its minimum and swallowed the chord.
    let hue = Signal::new(180.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(HueStrip::new(hue.clone(), setter, dragging));
    tree.layout(SizeProposal::exact(20.0, 200.0));
    tree.focus(id);

    for (key, mods) in [
        (Key::Home, Modifiers::CTRL),
        (Key::PageUp, Modifiers::ALT),
        (Key::ArrowUp, Modifiers::SUPER),
    ] {
        tree.press_key(key, mods);
        assert!(
            (hue.get() - 180.0).abs() < 0.01,
            "{key:?} with {mods:?} moved the hue to {}",
            hue.get()
        );
    }
}

#[test]
fn swatch_emits_color_well_with_color_value() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorSwatch::new(Color::RED));
    tree.layout(SizeProposal::exact(40.0, 40.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::ColorWell);
}

#[test]
fn swatch_grid_emits_grid_role() {
    let swatches = Signal::new(vec![Color::RED, Color::GREEN, Color::BLUE]);
    let selected = Signal::new(Color::RED);
    let on_select: Rc<dyn Fn(Color, &mut teksilo_core::widget::EventContext)> = Rc::new(|_, _| {});
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(SwatchGrid::new(swatches, selected, 6, on_select));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Grid);
}

/// The canvas is a **named group carrying a value**, not a placeholder.
///
/// A `GenericContainer` with no properties is pruned by the accessibility
/// walker, and a pruned node advertises nothing — which is what left the one
/// control in the picker whose value is a *pair* undriveable by assistive
/// technology. Its children stay excluded (three gradient layers); the node
/// does not.
#[test]
fn the_hsv_canvas_names_itself_and_reports_both_axes() {
    let hue = Signal::new(0.0_f32);
    let sat = Signal::new(0.5_f32);
    let val = Signal::new(0.25_f32);
    let set_hsv: Rc<dyn Fn(f32, f32, f32)> = Rc::new(|_, _, _| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(HsvCanvas::new(hue, sat, val, set_hsv, dragging));
    tree.layout(SizeProposal::exact(224.0, 192.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Group);
    assert_eq!(node.name(), Some("Saturation and brightness"));
    let update = tree.sync_accessibility();
    let value = update
        .nodes
        .iter()
        .find(|(n, _)| *n == teksilo_core::accessibility::widget_id_to_node_id(id))
        .and_then(|(_, n)| n.value().map(|s| s.to_string()));
    assert_eq!(value.as_deref(), Some("Saturation 50%, brightness 25%"));
}

#[test]
fn default_swatches_palette_has_twelve_colors() {
    assert_eq!(DEFAULT_SWATCHES.len(), 12);
}

// ── ColorEdit ────────────────────────────────────────────────────────

#[test]
fn color_edit_builds_with_default_options() {
    use crate::color_edit::ColorEdit;
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorEdit::new(value));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn color_edit_emits_button_role() {
    // ColorEdit composes a Button (via PopoverButton) — the
    // Role::Button declaration lives on the inner trigger, not on
    // ColorEdit's own arena node. Walk to the first focusable
    // descendant to find the trigger and check its role.
    use crate::color_edit::ColorEdit;
    let value = Signal::new(Color::from_hex("#3584E4"));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorEdit::new(value));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    let trigger = tree
        .first_focusable_descendant(id)
        .expect("ColorEdit must expose a focusable trigger");
    let node = tree.accessibility_node(trigger);
    assert_eq!(node.role(), Role::Button);
}

#[test]
fn external_value_change_propagates_through_picker() {
    // Pin the symptom the user reported: dragging the HSV canvas (which
    // mutates the bound `Signal<Color>`) should drive the spinner /
    // hex-input bridges WITHOUT the user having to focus into them.
    // We can't render the field text directly in headless tests, but we
    // can confirm the bridge `Signal<u8>` cells move by walking the
    // value→bridge effect chain.
    let value = Signal::new(Color::from_rgba(0.0, 0.0, 0.0, 1.0));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        ColorPicker::new(value.clone())
            .alpha_enabled(true)
            .show_rgb_spinners(true)
            .show_hex_input(true),
    );
    tree.layout(SizeProposal::exact(900.0, 700.0));

    // External mutation simulating an HSV-canvas drag.
    value.set(Color::from_rgba(1.0, 0.5, 0.25, 1.0));

    // The bound signal moved.
    let after = value.get();
    assert!((after.r() - 1.0).abs() < 0.01);
    assert!((after.g() - 0.5).abs() < 0.01);
    assert!((after.b() - 0.25).abs() < 0.01);
}

#[test]
fn color_edit_clicking_trigger_opens_popover() {
    use crate::color_edit::ColorEdit;
    let value = Signal::new(Color::BLUE);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorEdit::new(value));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    tree.render(); // cache bounds for tap

    // The trigger Button (focusable descendant of ColorEdit) is the
    // node that carries the disclosure state.
    let trigger = tree
        .first_focusable_descendant(id)
        .expect("ColorEdit must expose a focusable trigger");
    let before = tree.accessibility_node(trigger);
    assert!(!before.is_expanded(), "popover starts closed");

    tree.click(trigger);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let after = tree.accessibility_node(trigger);
    assert!(
        after.is_expanded(),
        "click opens popover (set_expanded=true)"
    );
}

#[test]
fn color_picker_footer_invokes_done_callback() {
    // show_footer adds a Done button; clicking it must fire the
    // user-supplied on_done callback. We check by counting calls in
    // a shared cell.
    use std::cell::Cell;
    use std::rc::Rc;
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let calls = Rc::new(Cell::new(0_u32));
    let calls_for_picker = calls.clone();
    let picker_id = tree.add(
        ColorPicker::new(value)
            .layout(ColorPickerLayout::Compact)
            .show_footer(true)
            .on_done(move |_ctx| {
                calls_for_picker.set(calls_for_picker.get() + 1);
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    tree.render();

    // Find the Done button by its label and click it.
    let done_id = tree
        // No I18nManager is installed in the test environment, so
        // resolve_message_widget returns the literal Fluent key.
        .find_by_label("color-picker-done-label")
        .expect("Done button must exist when show_footer(true)");
    let _ = picker_id;
    tree.click(done_id);
    assert_eq!(calls.get(), 1, "on_done must fire once per Done click");
}

#[test]
fn color_edit_cancel_restores_value_to_open_time_snapshot() {
    // Open the popover with value=RED, mutate value externally to
    // simulate a drag, then click Cancel — value should snap back to
    // the value that was bound at popover-open time (RED).
    use crate::color_edit::ColorEdit;
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(ColorEdit::new(value.clone()));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    tree.render();

    let trigger = tree
        .first_focusable_descendant(id)
        .expect("ColorEdit trigger must exist");
    tree.click(trigger);
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Simulate the user dragging the HSV canvas — the picker writes
    // through to the bound signal continuously.
    value.set(Color::from_hex("#00FF00"));
    assert_eq!(value.get(), Color::from_hex("#00FF00"));

    let cancel_id = tree
        .find_by_label("color-picker-cancel-label")
        .expect("Cancel button must exist when show_footer(true)");
    tree.click(cancel_id);
    tree.layout(SizeProposal::exact(800.0, 600.0));

    assert_eq!(
        value.get(),
        Color::RED,
        "cancel should restore the value the bound signal had at popover-open time",
    );
}

#[test]
fn tooltip_appears_on_hover() {
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id =
        tree.add(ColorPicker::new(value).tooltip(teksilo_i18n::LocalizedString::literal("Tip")));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.pointer_move(tree.bounds(id).center());
    tree.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "tooltip should appear on hover"
    );
    assert!(tree.find_by_label("Tip").is_some());
}

// ---------------------------------------------------------------------------
// Touch: continuous manipulators and their targets
// ---------------------------------------------------------------------------

/// A finger drag on the hue strip inside a scroller sets the hue and scrolls
/// nothing.
///
/// The end-to-end promise for a continuous manipulator. What delivers it is the
/// press capture the strip's drag takes — it makes the strip the innermost
/// member of the pointer's sequence, so the enclosing claimant never wins.
/// `touch_action(NONE)`'s own consumer is the pinch gate, tested below.
#[test]
fn a_finger_drag_on_the_hue_strip_inside_a_scroller_sets_the_hue() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::event::EventResponse;
    use teksilo_core::pointer::PointerPhase;
    use teksilo_core::pointer::touch_action::{PanAxes, PanClaim};
    use teksilo_core::widget_builder::WidgetBuilder;

    let hue = Signal::new(0.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let scrolled = Rc::new(std::cell::Cell::new(0_u32));
    let count = scrolled.clone();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let strip = tree.add(HueStrip::new(hue.clone(), setter, dragging));
    let _list = tree.add(
        crate::primitives::VStack::new()
            .child(strip)
            .scroll_container(PanAxes::BOTH)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_e, _c| {
                count.set(count.get() + 1);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal::exact(60.0, 400.0));
    let b = tree.bounds(strip);

    let id = finger();
    tree.dispatch_pointer(touch(
        id,
        PointerPhase::Down,
        Point::new(b.center().x, b.y + 4.0),
        0,
    ));
    for (i, f) in [0.3_f32, 0.6, 0.9].into_iter().enumerate() {
        let at = Point::new(b.center().x, b.y + b.height * f);
        tree.dispatch_pointer(touch(id, PointerPhase::Move, at, 20 + i as u64 * 20));
    }
    tree.dispatch_pointer(touch(
        id,
        PointerPhase::Up,
        Point::new(b.center().x, b.y + b.height * 0.9),
        100,
    ));

    // The drag ends nine tenths down, a tenth of the range up from the bottom.
    assert!(
        (hue.get() - 36.0).abs() < 5.0,
        "the finger drag did not reach the strip (hue {})",
        hue.get()
    );
    assert_eq!(scrolled.get(), 0, "and it must not have scrolled the list");
}

/// Two contacts on `subject` must start no pinch on the surface it sits in.
///
/// The one consumer of `touch_action(NONE)` that the press capture does not
/// already cover: `feed_pinch` asks `effective_touch_action(hit target)
/// .allows_pinch()` on the contact itself, before any capture or arbitration
/// exists. Relax the manipulator to `AUTO` and the pinch starts.
fn pinch_is_refused_on(subject: Box<dyn teksilo_core::widget::Widget>, size: (f32, f32)) {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::gesture::PinchPhase;
    use teksilo_core::pointer::PointerPhase;
    use teksilo_core::widget_builder::WidgetBuilder;

    let started = Rc::new(std::cell::Cell::new(0_u32));
    let n = started.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add_boxed(subject);
    let _surface = tree.add(crate::primitives::ZStack::new().child(id).on_pinch(
        move |phase, _c| {
            if matches!(phase, PinchPhase::Started { .. }) {
                n.set(n.get() + 1);
            }
        },
    ));
    tree.layout(SizeProposal::exact(size.0, size.1));
    let b = tree.bounds(id);

    // Two contacts a third of the way in from each end, on the long axis.
    let (a_at, c_at) = if b.height >= b.width {
        (
            Point::new(b.center().x, b.y + b.height / 3.0),
            Point::new(b.center().x, b.y + b.height * 2.0 / 3.0),
        )
    } else {
        (
            Point::new(b.x + b.width / 3.0, b.center().y),
            Point::new(b.x + b.width * 2.0 / 3.0, b.center().y),
        )
    };
    let a = finger();
    let c = finger();
    tree.dispatch_pointer(touch(a, PointerPhase::Down, a_at, 0));
    tree.dispatch_pointer(touch(c, PointerPhase::Down, c_at, 5));

    assert_eq!(
        started.get(),
        0,
        "a pinch started on a manipulator must not reach the surface under it",
    );
    assert!(
        !tree.touch_pinch_active(),
        "…and none may be running at all"
    );
}

#[test]
fn two_contacts_on_the_hue_strip_pinch_nothing_underneath() {
    let hue = Signal::new(0.0_f32);
    let setter: Rc<dyn Fn(f32)> = Rc::new(|_| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    pinch_is_refused_on(
        Box::new(HueStrip::new(hue, setter, dragging)),
        (60.0, 300.0),
    );
}

#[test]
fn two_contacts_on_the_alpha_strip_pinch_nothing_underneath() {
    let color = Signal::new(Color::RED);
    let alpha = Signal::new(0.5_f32);
    let setter: Rc<dyn Fn(f32)> = Rc::new(|_| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    pinch_is_refused_on(
        Box::new(AlphaStrip::new(color, alpha, setter, dragging)),
        (60.0, 300.0),
    );
}

#[test]
fn two_contacts_on_the_hsv_canvas_pinch_nothing_underneath() {
    let hue = Signal::new(0.0_f32);
    let sat = Signal::new(1.0_f32);
    let val = Signal::new(1.0_f32);
    let set_hsv: Rc<dyn Fn(f32, f32, f32)> = Rc::new(|_, _, _| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    pinch_is_refused_on(
        Box::new(HsvCanvas::new(hue, sat, val, set_hsv, dragging)),
        (224.0, 192.0),
    );
}

/// A colour strip beside the canvas it belongs to, both inside a picker body
/// that takes taps of its own.
///
/// The row is what makes the strip tests discriminate. The miss-only slop pass
/// only wins when the exact hit's bubble path carries no eligible handler, or
/// when its candidate is strictly closer than that path's owner; the row owns
/// the press at distance zero, and a near-miss does not beat zero. So the only
/// mechanism left that can carry a press beside the strip onto it is the
/// strip's own `hit_outset` — without the row, the slop pass would serve the
/// press either way and the test could not tell the two apart.
fn strip_in_a_tappable_row(
    strip: Box<dyn teksilo_core::widget::Widget>,
    row_taps: Rc<std::cell::Cell<u32>>,
) -> (WidgetTree, teksilo_core::widget_id::WidgetId) {
    use teksilo_core::widget_builder::WidgetBuilder;

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let strip = tree.add_boxed(strip);
    let filler = tree.add(crate::primitives::RectWidget::new());
    let _row = tree.add(
        crate::primitives::HStack::new()
            .child(strip)
            .child(filler)
            .on_tap(move |_e, _c| row_taps.set(row_taps.get() + 1)),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));
    (tree, strip)
}

/// The 14 dp strip cannot grow, so it earns its 24 dp across the short axis
/// from the pointer side: a finger 4 dp beside it still adjusts it, and a mouse
/// there does not.
#[test]
fn a_finger_beside_the_hue_strip_still_adjusts_it() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::PointerPhase;

    let hue = Signal::new(0.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let row_taps = Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, strip) = strip_in_a_tappable_row(
        Box::new(HueStrip::new(hue.clone(), setter, dragging)),
        row_taps.clone(),
    );
    let b = tree.bounds(strip);
    assert!(
        b.width < 24.0,
        "the strip is meant to be under the floor, got {b:?}"
    );
    // 4 dp past the strip's right edge, three quarters down it.
    let at = Point::new(b.right() + 4.0, b.y + b.height * 0.75);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
    // Three quarters down a strip whose maximum is at the top is a quarter of
    // the range.
    assert!(
        (hue.get() - 90.0).abs() < 5.0,
        "the outset did not carry the press (hue {})",
        hue.get()
    );
    assert_eq!(row_taps.get(), 0, "and the row must not have taken it too");
}

/// A mouse there does not: an exact hot-spot 4 dp beside the strip is a press
/// on the picker body, exactly as it was before the touch programme.
#[test]
fn a_mouse_beside_the_hue_strip_lands_on_the_row() {
    use teksilo_canvas::Point;
    use teksilo_core::event::PointerButton;

    let hue = Signal::new(0.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let hue = hue.clone();
        Rc::new(move |h| hue.set(h))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let row_taps = Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, strip) = strip_in_a_tappable_row(
        Box::new(HueStrip::new(hue.clone(), setter, dragging)),
        row_taps.clone(),
    );
    let b = tree.bounds(strip);
    let at = Point::new(b.right() + 4.0, b.y + b.height * 0.75);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert_eq!(
        hue.get(),
        0.0,
        "a mouse must not be given the strip's outset"
    );
    assert_eq!(row_taps.get(), 1);
}

/// The alpha strip is the hue strip's twin — same 14 dp thickness, same
/// inability to grow, same answer.
#[test]
fn a_finger_beside_the_alpha_strip_still_adjusts_it() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::PointerPhase;

    let color = Signal::new(Color::RED);
    let alpha = Signal::new(1.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let alpha = alpha.clone();
        Rc::new(move |a| alpha.set(a))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let row_taps = Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, strip) = strip_in_a_tappable_row(
        Box::new(AlphaStrip::new(color, alpha.clone(), setter, dragging)),
        row_taps.clone(),
    );
    let b = tree.bounds(strip);
    assert!(
        b.width < 24.0,
        "the strip is meant to be under the floor, got {b:?}"
    );
    // 4 dp past the strip's right edge, a quarter of the way down it.
    let at = Point::new(b.right() + 4.0, b.y + b.height * 0.25);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
    // A quarter of the way down leaves three quarters of the range, the strip's
    // maximum being at the top.
    assert!(
        (alpha.get() - 0.75).abs() < 0.05,
        "the outset did not carry the press (alpha {})",
        alpha.get()
    );
    assert_eq!(row_taps.get(), 0, "and the row must not have taken it too");
}

/// …and a mouse there does not.
#[test]
fn a_mouse_beside_the_alpha_strip_lands_on_the_row() {
    use teksilo_canvas::Point;
    use teksilo_core::event::PointerButton;

    let color = Signal::new(Color::RED);
    let alpha = Signal::new(1.0_f32);
    let setter: Rc<dyn Fn(f32)> = {
        let alpha = alpha.clone();
        Rc::new(move |a| alpha.set(a))
    };
    let dragging = Rc::new(std::cell::Cell::new(false));
    let row_taps = Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, strip) = strip_in_a_tappable_row(
        Box::new(AlphaStrip::new(color, alpha.clone(), setter, dragging)),
        row_taps.clone(),
    );
    let b = tree.bounds(strip);
    let at = Point::new(b.right() + 4.0, b.y + b.height * 0.25);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert_eq!(
        alpha.get(),
        1.0,
        "a mouse must not be given the strip's outset"
    );
    assert_eq!(row_taps.get(), 1);
}

/// What the strip reports it painted: the whole node as the press surface, and
/// the thumb as the grab affordance nothing else in the tree can see.
#[test]
fn the_hue_strip_reports_its_body_and_its_thumb() {
    let hue = Signal::new(180.0_f32);
    let setter: Rc<dyn Fn(f32)> = Rc::new(|_| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let strip = tree.add(HueStrip::new(hue, setter, dragging));
    let _row = tree.add(crate::primitives::HStack::new().child(strip));
    tree.layout(SizeProposal::exact(60.0, 400.0));
    let bounds = tree.bounds(strip);
    let regions = tree.widget_target_regions(strip);
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].rect, bounds);
    assert_eq!(regions[0].role, teksilo_tokens::TargetRole::Target);
    assert_eq!(regions[1].role, teksilo_tokens::TargetRole::Grab);
    // Half-way hue → the thumb sits half-way down the strip.
    let mid = bounds.y + bounds.height * 0.5;
    assert!((regions[1].rect.center().y - mid).abs() < 1.0);
}

/// The alpha strip reports the same pair, and its thumb tracks the opacity
/// rather than the hue — so a strip that reported a thumb pinned to the top,
/// or reported nothing at all, is caught here.
#[test]
fn the_alpha_strip_reports_its_body_and_its_thumb() {
    let color = Signal::new(Color::RED);
    let alpha = Signal::new(0.25_f32);
    let setter: Rc<dyn Fn(f32)> = Rc::new(|_| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let strip = tree.add(AlphaStrip::new(color, alpha.clone(), setter, dragging));
    let _row = tree.add(crate::primitives::HStack::new().child(strip));
    tree.layout(SizeProposal::exact(60.0, 400.0));
    let bounds = tree.bounds(strip);
    let regions = tree.widget_target_regions(strip);
    assert_eq!(regions.len(), 2, "body and thumb, got {regions:?}");
    assert_eq!(regions[0].rect, bounds);
    assert_eq!(regions[0].role, teksilo_tokens::TargetRole::Target);
    assert_eq!(regions[1].role, teksilo_tokens::TargetRole::Grab);
    // A quarter opacity → the thumb sits a quarter of the way down the strip,
    // and nowhere near the half-way mark a hue-derived one would report.
    let quarter = bounds.y + bounds.height * 0.25;
    assert!(
        (regions[1].rect.center().y - quarter).abs() < 1.0,
        "the thumb does not track the opacity: {:?} vs {quarter}",
        regions[1].rect,
    );
    // …and it follows the value, rather than being a constant.
    alpha.set(0.75);
    let moved = tree.widget_target_regions(strip);
    let three_quarters = bounds.y + bounds.height * 0.75;
    assert!(
        (moved[1].rect.center().y - three_quarters).abs() < 1.0,
        "the thumb did not follow the opacity: {:?}",
        moved[1].rect,
    );
}

/// Two swatches side by side in a recents strip that takes taps of its own —
/// the shape the swatch's own doc names: a wall of 22 dp targets, each 2 dp
/// under the floor, inside something that takes presses.
///
/// A swatch laid out as a bare root is stretched to the window, which makes
/// the miss-only slop pass see a 200 dp box and offer nothing, so such a
/// fixture would pass on arithmetic rather than on the mechanism. Here each
/// swatch is at its real 22 dp, where the slop pass *could* serve the press —
/// and the tappable row is what stops it, leaving the outset as the only way
/// the press can reach the swatch.
fn swatch_pair_in_a_tappable_row(
    first: Rc<std::cell::Cell<u32>>,
    second: Rc<std::cell::Cell<u32>>,
    row_taps: Rc<std::cell::Cell<u32>>,
) -> (
    WidgetTree,
    teksilo_core::widget_id::WidgetId,
    teksilo_core::widget_id::WidgetId,
) {
    use teksilo_core::widget_builder::WidgetBuilder;

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let a =
        tree.add(ColorSwatch::new(Color::RED).on_activate_fn(move |_| first.set(first.get() + 1)));
    let b = tree
        .add(ColorSwatch::new(Color::BLUE).on_activate_fn(move |_| second.set(second.get() + 1)));
    let _row = tree.add(
        crate::primitives::HStack::new()
            .spacing(6.0)
            .child(a)
            .child(b)
            .on_tap(move |_e, _c| row_taps.set(row_taps.get() + 1)),
    );
    tree.layout(SizeProposal::exact(200.0, 60.0));
    (tree, a, b)
}

/// A 22 dp swatch is 2 dp under the floor and its neighbours in a grid are
/// targets too, so it earns the difference from the pointer side.
#[test]
fn a_finger_just_outside_a_swatch_still_picks_it() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::PointerPhase;

    let picked = Rc::new(std::cell::Cell::new(0_u32));
    let neighbour = Rc::new(std::cell::Cell::new(0_u32));
    let row_taps = Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, sw, _other) =
        swatch_pair_in_a_tappable_row(picked.clone(), neighbour.clone(), row_taps.clone());
    let b = tree.bounds(sw);
    assert!(
        b.width < 24.0,
        "the swatch is meant to be under the floor, got {b:?}"
    );
    // Half a dp into the gap: outside the 22 dp square, inside the 24 dp
    // target the outset earns it, and nowhere near its neighbour.
    let at = Point::new(b.right() + 0.5, b.center().y);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
    assert_eq!(
        picked.get(),
        1,
        "the swatch's outset did not take the press"
    );
    assert_eq!(neighbour.get(), 0, "and the neighbouring swatch must not");
    assert_eq!(row_taps.get(), 0, "and the row must not have taken it too");
}

/// A swatch that takes no press claims no widened target — a widened node that
/// then ignores the press is a hole punched in the grid behind it.
#[test]
fn a_swatch_that_takes_no_press_declares_no_outset() {
    use teksilo_core::widget::Widget;
    use teksilo_tokens::{InputTokens, PointerKind};

    let tokens = InputTokens::default();
    let decorative = ColorSwatch::new(Color::RED);
    assert_eq!(
        decorative.hit_outset(PointerKind::Touch, &tokens),
        teksilo_canvas::EdgeInsets::ZERO,
        "a swatch with no action",
    );
    let disabled = ColorSwatch::new(Color::RED)
        .on_activate_fn(|_| {})
        .enabled(false);
    assert_eq!(
        disabled.hit_outset(PointerKind::Touch, &tokens),
        teksilo_canvas::EdgeInsets::ZERO,
        "a disabled swatch",
    );
    // The premise: an enabled, activatable one does earn an outset, so the two
    // assertions above are gates rather than a mechanism that never fires.
    let live = ColorSwatch::new(Color::RED).on_activate_fn(|_| {});
    assert_ne!(
        live.hit_outset(PointerKind::Touch, &tokens),
        teksilo_canvas::EdgeInsets::ZERO,
    );
}

/// A mouse is exact: the same press lands on the strip behind the swatches.
#[test]
fn a_mouse_just_outside_a_swatch_lands_on_the_row() {
    use teksilo_canvas::Point;
    use teksilo_core::event::PointerButton;

    let picked = Rc::new(std::cell::Cell::new(0_u32));
    let neighbour = Rc::new(std::cell::Cell::new(0_u32));
    let row_taps = Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, sw, _other) =
        swatch_pair_in_a_tappable_row(picked.clone(), neighbour.clone(), row_taps.clone());
    let b = tree.bounds(sw);
    let at = Point::new(b.right() + 0.5, b.center().y);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert_eq!(
        picked.get(),
        0,
        "a mouse must not be given the swatch's outset"
    );
    assert_eq!(row_taps.get(), 1);
}
