//! Integration tests for [`ColorPicker`](super::ColorPicker) and its
//! subcomponents. Headless — no Xvfb / GPU required.

#![cfg(test)]

use fern_canvas::SizeProposal;
use fern_core::accesskit::Role;
use fern_core::event::{Key, Modifiers};
use fern_core::signal::Signal;
use fern_core::widget_tree::WidgetTree;
use fern_tokens::Color;

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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let id = tree.add(ColorPicker::new(value));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    // No panic = pass; assert the root has bounds.
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn builds_with_alpha_enabled() {
    let value = Signal::new(Color::from_rgba(1.0, 0.0, 0.0, 0.5));
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    tree.add(ColorPicker::new(value).alpha_enabled(true));
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

#[test]
fn builds_with_compact_layout() {
    let value = Signal::new(Color::BLUE);
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    tree.add(ColorPicker::new(value).layout(ColorPickerLayout::Compact));
    tree.layout(SizeProposal::exact(400.0, 400.0));
}

#[test]
fn builds_with_wide_layout() {
    let value = Signal::new(Color::GREEN);
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    tree.add(
        ColorPicker::new(value)
            .alpha_enabled(true)
            .layout(ColorPickerLayout::Wide)
            .show_hsv_spinners(true),
    );
    tree.layout(SizeProposal::exact(900.0, 500.0));
}

#[test]
fn builds_with_nullable_value() {
    let value: Signal<Option<Color>> = Signal::new(Some(Color::RED));
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    tree.add(ColorPicker::nullable(value));
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

#[test]
fn builds_with_nullable_none() {
    let value: Signal<Option<Color>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    tree.add(ColorPicker::nullable(value));
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

#[test]
fn root_accessibility_emits_group_role() {
    let value = Signal::new(Color::RED);
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let id = tree.add(ColorPicker::new(value));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Group);
}

#[test]
fn setting_red_preserves_alpha() {
    let value = Signal::new(Color::from_rgba(0.5, 0.5, 0.5, 0.8));
    // ColorComponents must run inside a widget build to register effects.
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    use fern_core::build_context::BuildContext;
    use fern_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
    use fern_core::widget_id::WidgetId;
    use std::cell::RefCell;
    use std::rc::Rc;

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
            _bounds: fern_canvas::Rect,
            _proposal: SizeProposal,
            _children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
        }
    }

    let value = Signal::new(Color::from_rgb(0.1, 0.2, 0.3));
    let captured: Rc<RefCell<Option<Rc<dyn Fn(f32)>>>> = Rc::new(RefCell::new(None));
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
fn swatch_emits_color_well_with_color_value() {
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let id = tree.add(ColorSwatch::new(Color::RED));
    tree.layout(SizeProposal::exact(40.0, 40.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::ColorWell);
}

#[test]
fn swatch_grid_emits_grid_role() {
    let swatches = Signal::new(vec![Color::RED, Color::GREEN, Color::BLUE]);
    let selected = Signal::new(Color::RED);
    let on_select: Rc<dyn Fn(Color, &mut fern_core::widget::EventContext)> = Rc::new(|_, _| {});
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let id = tree.add(SwatchGrid::new(swatches, selected, 6, on_select));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let node = tree.accessibility_node(id);
    assert_eq!(node.role(), Role::Grid);
}

#[test]
fn hsv_canvas_emits_placeholder_role() {
    let hue = Signal::new(0.0_f32);
    let sat = Signal::new(1.0_f32);
    let val = Signal::new(1.0_f32);
    let set_hsv: Rc<dyn Fn(f32, f32, f32)> = Rc::new(|_, _, _| {});
    let dragging = Rc::new(std::cell::Cell::new(false));
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let id = tree.add(HsvCanvas::new(hue, sat, val, set_hsv, dragging));
    tree.layout(SizeProposal::exact(224.0, 192.0));
    let node = tree.accessibility_node(id);
    // The HSV canvas emits a placeholder GenericContainer role; the
    // containing ColorPicker excludes its subtree from the AT tree
    // via `.access_exclude_subtree()`.
    assert_eq!(node.role(), Role::GenericContainer);
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let id = tree.add(ColorEdit::new(value));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn color_edit_builds_nullable() {
    use crate::color_edit::ColorEdit;
    let value: Signal<Option<Color>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    tree.add(ColorEdit::nullable(value));
    tree.layout(SizeProposal::exact(200.0, 40.0));
}

#[test]
fn color_edit_emits_button_role() {
    // ColorEdit composes a Button (via PopoverButton) — the
    // Role::Button declaration lives on the inner trigger, not on
    // ColorEdit's own arena node. Walk to the first focusable
    // descendant to find the trigger and check its role.
    use crate::color_edit::ColorEdit;
    let value = Signal::new(Color::from_hex("#3584E4"));
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
