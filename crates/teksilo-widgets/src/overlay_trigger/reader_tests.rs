// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A custom overlay trigger, and the popover it opens, as a screen reader
//! meets them.
//!
//! Each assertion reads the tree the way the platform adapters do: through
//! `accesskit_consumer`, which all three are built on, following its focus
//! and walking its filtered children (`common_filter`). `tools/reader/`
//! found, in `dialogs-and-popovers` and the widget catalog:
//!
//! * a `PopoverWidget<OverlayTrigger>` that was no Tab stop at all, and that
//!   an AT-SPI `grab_focus` could not focus either: the role, the name and
//!   `Click` sat on the trigger's own node and nothing made that node
//!   focusable;
//! * a `Dialog` with a custom trigger that put focus on the wrapped `Panel`,
//!   an unnamed group ("panel."), while the name sat on its parent;
//! * a `Snackbar` whose custom trigger was a `Button`: the reader's own
//!   activation of that focused button did nothing, and the working button
//!   around it was named with the snackbar's message;
//! * a popover with nothing to focus that parked focus on an unnamed,
//!   role-less host, where Orca 46.1 found nothing to say ("Results for
//!   [unknown] are pauses only"), and from which Tab went to the first
//!   control of the window.

use accesskit_consumer::{NodeRef, Tree, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::accesskit::{Action, HasPopup, Role};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::button::Button;
use crate::common::heard_test::{Heard, Listener};
use crate::dialog::Dialog;
use crate::overlay_trigger::OverlayTrigger;
use crate::panel::Panel;
use crate::popover_widget::{PopoverButton, PopoverWidget};
use crate::primitives::{TextWidget, VStack};
use crate::snackbar::Snackbar;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn laid_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

/// A trigger's face as the example draws it: a padded group with text in it,
/// and nothing that takes focus.
fn panel_face(text: &'static str) -> Panel {
    Panel::new()
        .padding(12.0)
        .child(TextWidget::new(lit!(text)))
}

/// A panel with nothing a keyboard can stop on, like the example's.
fn text_only_content() -> impl Widget {
    VStack::new()
        .child(TextWidget::new(lit!("Popover")))
        .child(TextWidget::new(lit!(
            "Use popovers for compact contextual actions without leaving the current surface."
        )))
}

/// `Before`, the widget under test, then `After`, in one column; focus on
/// `Before`. Returns the ids of `Before` and `After`.
fn between(tree: &mut WidgetTree, subject: impl Widget + 'static) -> (WidgetId, WidgetId) {
    let before = tree.add(Button::new(lit!("Before")));
    let after = tree.add(Button::new(lit!("After")));
    tree.add(VStack::new().child(before).child(subject).child(after));
    laid_out(tree);
    tree.focus(before);
    laid_out(tree);
    (before, after)
}

/// The role and name of the node a screen reader is on, as an adapter reads
/// it (`accesskit_consumer`'s focus, which follows `active_descendant`).
fn reader_focus(tree: &mut WidgetTree) -> (Role, String) {
    let platform = Tree::new(tree.sync_accessibility(), true);
    let focus = platform.state().focus().expect("the tree names a focus");
    (focus.role(), focus.label().unwrap_or_default())
}

/// The name of every button a reader reaches by walking the tree (object
/// navigation, flat review), in reading order.
fn reader_buttons(tree: &mut WidgetTree) -> Vec<String> {
    fn walk(node: NodeRef<'_>, out: &mut Vec<String>) {
        if node.role() == Role::Button {
            out.push(node.label().unwrap_or_default());
        }
        for child in node.filtered_children(&common_filter) {
            walk(child, out);
        }
    }
    let platform = Tree::new(tree.sync_accessibility(), true);
    let mut out = Vec::new();
    walk(platform.state().root(), &mut out);
    out
}

/// A screen reader's own activation of the node it is on (AT-SPI `click`,
/// UIA Invoke, `AXPress`), dispatched through the adapters' entry point.
fn reader_clicks_focus(tree: &mut WidgetTree) -> bool {
    let focused = tree.focused().expect("something holds focus");
    tree.dispatch_access_action(
        widget_id_to_node_id(focused),
        Action::Click,
        None,
        &mut teksilo_core::NoopWindowOps,
    )
}

/// The same activation on the button a reader found by name, wherever focus
/// is: what the sweep did from 'Show snackbar' to open the popover.
fn reader_clicks(tree: &mut WidgetTree, name: &str) -> bool {
    let target = tree.find_by_label(name).expect("a node of that name");
    tree.dispatch_access_action(
        widget_id_to_node_id(target),
        Action::Click,
        None,
        &mut teksilo_core::NoopWindowOps,
    )
}

/// The example's popover: a panel face, a name, and a panel with only text in it.
fn show_popover() -> impl Widget {
    PopoverWidget::new(OverlayTrigger::around(panel_face("Popover actions")).named("Show popover"))
        .content(text_only_content())
}

#[test]
fn a_custom_popover_trigger_is_a_tab_stop_heard_as_its_name() {
    let mut tree = light_tree();
    between(&mut tree, show_popover());
    let mut listener = Listener::attach(&mut tree);

    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Show popover".to_string()),
        "Tab must land on the trigger, and the node it lands on must be the named button"
    );
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("Show popover".to_string())]
    );

    tree.press_key(Key::Enter, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "Enter on the trigger opens the popover"
    );
}

#[test]
fn a_custom_dialog_trigger_puts_focus_on_its_named_button() {
    let mut tree = light_tree();
    between(
        &mut tree,
        Dialog::new(lit!("Open dialog"))
            .content(|| TextWidget::new(lit!("Review Changes")))
            .trigger(panel_face("Review changes")),
    );

    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Open dialog".to_string()),
        "the focused node must be the one carrying the trigger's role and name, \
         not the unnamed panel it wraps"
    );

    tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        tree.drain_pending_modal_requests().len(),
        1,
        "Enter opens the dialog"
    );
}

#[test]
fn a_custom_button_trigger_opens_its_snackbar_from_the_readers_click() {
    let mut tree = light_tree();
    between(
        &mut tree,
        Snackbar::new(lit!("File saved successfully"))
            .content(TextWidget::new(lit!("File saved successfully")))
            .trigger(Button::new(lit!("Show snackbar"))),
    );

    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Show snackbar".to_string())
    );

    assert!(
        reader_clicks_focus(&mut tree),
        "the reader's click must be answered"
    );
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "activating the focused trigger the way a screen reader does presents the snackbar"
    );
    assert_eq!(
        reader_buttons(&mut tree),
        vec!["Before", "Show snackbar", "After"],
        "one button for the trigger, named by its own text: not a second one around it \
         named with the snackbar's message"
    );
}

/// A control given as a trigger takes the popup state onto the node a reader
/// is on, where the wrapper used to keep it for itself.
#[test]
fn a_control_as_a_dialog_trigger_carries_the_popup_state() {
    let mut tree = light_tree();
    between(
        &mut tree,
        Dialog::new(lit!("Open dialog"))
            .content(|| TextWidget::new(lit!("Review Changes")))
            .trigger(Button::new(lit!("Review"))),
    );
    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    let platform = Tree::new(tree.sync_accessibility(), true);
    let focus = platform.state().focus().expect("the tree names a focus");
    assert_eq!(
        (focus.role(), focus.label().unwrap_or_default()),
        (Role::Button, "Review".to_string())
    );
    assert_eq!(focus.data().has_popup(), Some(HasPopup::Dialog));
    assert_eq!(focus.data().is_expanded(), Some(false));
}

#[test]
fn a_popover_with_nothing_to_focus_puts_focus_on_its_named_dialog() {
    let mut tree = light_tree();
    between(&mut tree, show_popover());
    let mut listener = Listener::attach(&mut tree);

    assert!(reader_clicks(&mut tree, "Show popover"));
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "precondition: the popover is open"
    );
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Dialog, "Show popover".to_string()),
        "focus moves onto the popover itself, a dialog named after its trigger"
    );
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("Show popover".to_string())]
    );
}

/// A control that is disabled as it mounts is still the control. Taking it for
/// a widget with no focus of its own would give the trigger a Tab stop beside
/// it, an enabled button that opens what the disabled one withholds.
#[test]
fn a_control_disabled_as_it_mounts_is_still_the_trigger() {
    let mut tree = light_tree();
    let enabled = Signal::new(false);
    let (before, _) = between(
        &mut tree,
        Dialog::new(lit!("Open dialog"))
            .content(|| TextWidget::new(lit!("Review Changes")))
            .trigger(Button::new(lit!("Review")).enabled(enabled.clone())),
    );

    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "After".to_string()),
        "a disabled trigger is no Tab stop, and nothing stands in for it"
    );
    assert_eq!(reader_buttons(&mut tree), vec!["Before", "Review", "After"]);

    enabled.set(true);
    tree.focus(before);
    laid_out(&mut tree);
    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Review".to_string()),
        "once enabled, the button is the one Tab stop"
    );
    tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(tree.drain_pending_modal_requests().len(), 1);
}

/// Now that the trigger takes focus, a pointer's release lands focus on it as
/// well. The popover's own request must still win, or the popover would open
/// and close in one click: arriving on its anchor is leaving it.
#[test]
fn a_click_on_a_custom_trigger_opens_the_popover_onto_its_dialog() {
    let mut tree = light_tree();
    between(&mut tree, show_popover());
    let trigger = tree.find_by_label("Show popover").expect("the trigger");

    tree.click(trigger);
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the click opens the popover"
    );
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Dialog, "Show popover".to_string())
    );

    tree.click(trigger);
    laid_out(&mut tree);
    assert!(
        tree.active_overlays().is_empty(),
        "a second click closes it"
    );
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Show popover".to_string())
    );
}

#[test]
fn a_popover_buttons_panel_is_named_after_the_button() {
    let mut tree = light_tree();
    between(
        &mut tree,
        PopoverButton::new(Button::new(lit!("Open popover"))).content(text_only_content()),
    );
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.press_key(Key::Enter, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Dialog, "Open popover".to_string())
    );
}

#[test]
fn tab_from_a_popover_with_nothing_to_focus_goes_on_to_the_next_control() {
    let mut tree = light_tree();
    let (_, after) = between(&mut tree, show_popover());
    assert!(reader_clicks(&mut tree, "Show popover"));
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "precondition: the popover is open"
    );

    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        tree.focused(),
        Some(after),
        "Tab continues from the popover's place after its trigger, not from the top of the window"
    );
    assert!(
        tree.active_overlays().is_empty(),
        "and leaving closes the popover"
    );
}

#[test]
fn shift_tab_from_a_popover_with_nothing_to_focus_goes_back_to_its_trigger() {
    let mut tree = light_tree();
    between(&mut tree, show_popover());
    assert!(reader_clicks(&mut tree, "Show popover"));
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "precondition: the popover is open"
    );

    tree.press_key(Key::Tab, Modifiers::SHIFT);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Show popover".to_string())
    );
    assert!(tree.active_overlays().is_empty());
}

/// Around a control, the trigger's own node is structure, so the dialog is
/// named by the control that stands for it.
#[test]
fn a_popover_around_a_control_is_named_by_the_control() {
    let mut tree = light_tree();
    between(
        &mut tree,
        PopoverWidget::new(OverlayTrigger::around(Button::new(lit!("Filter"))))
            .content(text_only_content()),
    );
    tree.press_key(Key::Tab, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Filter".to_string())
    );

    tree.press_key(Key::Enter, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Dialog, "Filter".to_string())
    );
}

/// The dialog takes focus only when nothing inside it can: a panel with a
/// control still opens on that control, as it always has.
#[test]
fn a_popover_with_a_control_still_opens_on_the_control() {
    let mut tree = light_tree();
    between(
        &mut tree,
        PopoverWidget::new(
            OverlayTrigger::around(panel_face("Popover actions")).named("Show popover"),
        )
        .content(
            VStack::new()
                .child(TextWidget::new(lit!("Popover")))
                .child(Button::new(lit!("Inside"))),
        ),
    );
    assert!(reader_clicks(&mut tree, "Show popover"));
    laid_out(&mut tree);
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Inside".to_string())
    );
}

/// A bare panel is the caller's own chrome, with no dialog of the
/// framework's to focus. With nothing in it to focus either, focus stays on
/// the trigger rather than moving to a node no reader can name.
#[test]
fn a_bare_popover_with_nothing_to_focus_leaves_focus_on_its_trigger() {
    let mut tree = light_tree();
    between(
        &mut tree,
        PopoverButton::new(Button::new(lit!("Open popover")))
            .bare()
            .content(text_only_content()),
    );
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.press_key(Key::Enter, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "precondition: the popover is open"
    );
    assert_eq!(
        reader_focus(&mut tree),
        (Role::Button, "Open popover".to_string())
    );
}
