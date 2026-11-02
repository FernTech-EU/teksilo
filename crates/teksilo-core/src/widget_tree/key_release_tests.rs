// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A key release goes to the widget that received its press, not to whatever
//! focus moved to while the key was down; a lone Enter or Space release is
//! dropped.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;

use super::WidgetTree;
use crate::event::{EventResponse, Key, Modifiers, WidgetEvent};
use crate::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;

type Log = Rc<RefCell<Vec<(&'static str, &'static str)>>>;

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// A focusable field named `name` logging the key events it receives. On an
/// Enter press, it hands focus to `next` when one is set.
fn field(
    tree: &mut WidgetTree,
    parent: WidgetId,
    name: &'static str,
    log: &Log,
    next: Rc<RefCell<Option<WidgetId>>>,
) -> WidgetId {
    let log = log.clone();
    tree.add_child(
        parent,
        FillWidget::new().focusable().on_key(move |event, ctx| {
            match event {
                WidgetEvent::KeyDown { key, .. } => {
                    log.borrow_mut().push((name, "down"));
                    if *key == Key::Enter
                        && let Some(next) = *next.borrow()
                    {
                        ctx.request_focus(next);
                    }
                }
                WidgetEvent::KeyUp { .. } => log.borrow_mut().push((name, "up")),
                _ => return EventResponse::Ignored,
            }
            EventResponse::Handled
        }),
    )
}

/// Fields A and B; A's Enter press moves focus to B.
fn a_hands_focus_to_b() -> (WidgetTree, WidgetId, WidgetId, Log) {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let next = Rc::new(RefCell::new(None));
    let a = field(&mut tree, root, "A", &log, next.clone());
    let b = field(&mut tree, root, "B", &log, Rc::new(RefCell::new(None)));
    *next.borrow_mut() = Some(b);
    lay_out(&mut tree);
    tree.focus(a);
    (tree, a, b, log)
}

fn down(tree: &mut WidgetTree, key: Key) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key,
        modifiers: Modifiers::NONE,
        text: None,
    });
}

fn up(tree: &mut WidgetTree, key: Key) {
    tree.dispatch_event(WidgetEvent::KeyUp {
        key,
        modifiers: Modifiers::NONE,
    });
}

fn with_window(tree: &mut WidgetTree) -> crate::window::WindowState {
    use crate::window::state::WindowStateInit;
    use crate::window::{TeksiloWindowId, WindowPlacement, WindowState};
    let state = WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: None,
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (400, 300),
        position: (0, 0),
        focused: true,
        resizable: true,
        always_on_top: false,
    });
    tree.set_window_state(state.clone());
    state
}

#[test]
fn a_release_goes_to_the_widget_that_got_the_press() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    down(&mut tree, Key::Enter);
    assert_eq!(tree.focused(), Some(b), "the press moved focus on");
    up(&mut tree, Key::Enter);
    assert_eq!(*log.borrow(), [("A", "down"), ("A", "up")]);
}

#[test]
fn a_press_and_release_on_one_widget_is_delivered_once() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    tree.focus(b);
    down(&mut tree, Key::Enter);
    up(&mut tree, Key::Enter);
    assert_eq!(*log.borrow(), [("B", "down"), ("B", "up")]);
}

#[test]
fn auto_repeated_presses_end_in_one_release() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    tree.focus(b);
    for _ in 0..3 {
        down(&mut tree, Key::ArrowDown);
    }
    up(&mut tree, Key::ArrowDown);
    up(&mut tree, Key::ArrowDown);
    let ups = log
        .borrow()
        .iter()
        .filter(|(_, what)| *what == "up")
        .count();
    assert_eq!(ups, 2, "the second release, lone, still reaches focus");
}

/// A lone Enter or Space release (its press was never delivered here) is
/// dropped; any other lone release still reaches focus.
#[test]
fn a_lone_activation_release_is_dropped() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    tree.focus(b);
    up(&mut tree, Key::Enter);
    up(&mut tree, Key::Space);
    assert!(log.borrow().is_empty());
    up(&mut tree, Key::Character('x'));
    assert_eq!(*log.borrow(), [("B", "up")]);
}

/// Shift released before the letter: pressed as "A", released as "a", one
/// key.
#[test]
fn a_shifted_character_pairs_with_its_unshifted_release() {
    let (mut tree, a, b, log) = a_hands_focus_to_b();
    down(&mut tree, Key::Character('A'));
    tree.focus(b);
    up(&mut tree, Key::Character('a'));
    assert_eq!(*log.borrow(), [("A", "down"), ("A", "up")]);
    assert_ne!(tree.focused(), Some(a));
}

/// With the physical key noted by the window manager, a release whose
/// logical key differs from its press still pairs: Option+Shift+2 pressed as
/// "€", released as "2" after Shift.
#[test]
fn the_physical_key_pairs_a_release_whose_logical_key_changed() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    let window = with_window(&mut tree);
    window.note_key_identity_from_os(42, false);
    down(&mut tree, Key::Character('€'));
    tree.focus(b);
    window.note_key_identity_from_os(42, false);
    up(&mut tree, Key::Character('2'));
    assert_eq!(*log.borrow(), [("A", "down"), ("A", "up")]);
}

/// A record whose release was lost does not take the release of the next
/// fresh press of that key, made on another widget; an auto-repeat keeps it.
#[test]
fn a_fresh_press_replaces_a_stale_record() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    let window = with_window(&mut tree);
    window.note_key_identity_from_os(7, false);
    down(&mut tree, Key::Character('q'));
    tree.focus(b);
    window.note_key_identity_from_os(7, true);
    down(&mut tree, Key::Character('q'));
    window.note_key_identity_from_os(7, false);
    down(&mut tree, Key::Character('q'));
    window.note_key_identity_from_os(7, false);
    up(&mut tree, Key::Character('q'));
    assert_eq!(log.borrow().last(), Some(&("B", "up")));
}

/// A press made before the window lost the focus is forgotten when it gets
/// the focus back; a release synthesised as it loses the focus still reaches
/// the widget that had the press.
#[test]
fn window_focus_bounds_a_press_record() {
    let (mut tree, a, b, log) = a_hands_focus_to_b();
    down(&mut tree, Key::Character('z'));
    tree.set_window_active(false);
    up(&mut tree, Key::Character('z'));
    assert_eq!(log.borrow().last(), Some(&("A", "up")), "the blur release");

    tree.focus(a);
    log.borrow_mut().clear();
    down(&mut tree, Key::Character('z'));
    tree.set_window_active(false);
    tree.set_window_active(true);
    tree.focus(b);
    up(&mut tree, Key::Character('z'));
    assert_eq!(
        *log.borrow(),
        [("A", "down"), ("B", "up")],
        "the record is gone"
    );
}

/// A press that opened a modal over its widget: the release is not sent
/// behind the modal, nor to the modal's field.
#[test]
fn a_release_is_not_sent_behind_a_modal_opened_by_its_press() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let opener = field(&mut tree, root, "opener", &log, Rc::new(RefCell::new(None)));
    let content = tree.add(StackWidget::new());
    let inside = field(
        &mut tree,
        content,
        "inside",
        &log,
        Rc::new(RefCell::new(None)),
    );
    lay_out(&mut tree);
    tree.focus(opener);
    down(&mut tree, Key::Enter);
    tree.activate(content);
    tree.show_overlay(OverlayRequest::new(
        content,
        opener,
        OverlayPlacement::Centered,
        DismissBehavior::EscapeKey,
    ));
    lay_out(&mut tree);
    tree.focus(inside);
    up(&mut tree, Key::Enter);
    assert_eq!(*log.borrow(), [("opener", "down")]);
}

/// A widget that captures the keyboard still gets the release of a press
/// that opened a modal over it.
#[test]
fn a_keyboard_capture_surface_gets_its_release_behind_a_modal() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let surface = {
        let log = log.clone();
        tree.add_child(
            root,
            FillWidget::new()
                .focusable()
                .keyboard_capture(true)
                .on_key(move |event, _ctx| {
                    match event {
                        WidgetEvent::KeyDown { .. } => log.borrow_mut().push(("surface", "down")),
                        WidgetEvent::KeyUp { .. } => log.borrow_mut().push(("surface", "up")),
                        _ => return EventResponse::Ignored,
                    }
                    EventResponse::Handled
                }),
        )
    };
    let content = tree.add(StackWidget::new());
    let inside = field(
        &mut tree,
        content,
        "inside",
        &log,
        Rc::new(RefCell::new(None)),
    );
    lay_out(&mut tree);
    tree.focus(surface);
    down(&mut tree, Key::Enter);
    tree.activate(content);
    tree.show_overlay(OverlayRequest::new(
        content,
        surface,
        OverlayPlacement::Centered,
        DismissBehavior::EscapeKey,
    ));
    lay_out(&mut tree);
    tree.focus(inside);
    up(&mut tree, Key::Enter);
    assert_eq!(*log.borrow(), [("surface", "down"), ("surface", "up")]);
}

/// Two keys held at once, the second moving focus: each release goes to the
/// widget that had its press.
#[test]
fn two_held_keys_each_release_where_they_were_pressed() {
    let (mut tree, _a, b, log) = a_hands_focus_to_b();
    down(&mut tree, Key::Character('x'));
    down(&mut tree, Key::Enter);
    assert_eq!(tree.focused(), Some(b));
    down(&mut tree, Key::Character('y'));
    up(&mut tree, Key::Character('x'));
    up(&mut tree, Key::Enter);
    up(&mut tree, Key::Character('y'));
    let ups: Vec<_> = log
        .borrow()
        .iter()
        .filter(|(_, what)| *what == "up")
        .map(|(who, _)| *who)
        .collect();
    assert_eq!(ups, ["A", "A", "B"]);
}

/// A redirected release bubbles from the press target, so an ancestor that
/// tracks held keys hears it.
#[test]
fn a_redirected_release_bubbles_to_the_press_targets_ancestors() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let parent_log = log.clone();
    let parent = tree.add(StackWidget::new().on_key(move |event, _ctx| {
        if matches!(event, WidgetEvent::KeyUp { .. }) {
            parent_log.borrow_mut().push(("parent", "up"));
        }
        EventResponse::Ignored
    }));
    let next = Rc::new(RefCell::new(None));
    let a = {
        let next = next.clone();
        tree.add_child(
            parent,
            FillWidget::new()
                .focusable()
                .on_key(move |event, ctx| match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } => {
                        if let Some(next) = *next.borrow() {
                            ctx.request_focus(next);
                        }
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }),
        )
    };
    let other_root = tree.add(StackWidget::new());
    let b = field(
        &mut tree,
        other_root,
        "B",
        &log,
        Rc::new(RefCell::new(None)),
    );
    *next.borrow_mut() = Some(b);
    lay_out(&mut tree);
    tree.focus(a);
    down(&mut tree, Key::Enter);
    assert_eq!(tree.focused(), Some(b));
    up(&mut tree, Key::Enter);
    assert_eq!(*log.borrow(), [("parent", "up")]);
}

/// A widget that asked for every release gets it behind a modal, alone: the
/// release does not bubble on to its ancestors, which are behind the modal
/// too.
#[test]
fn a_release_behind_a_modal_does_not_bubble_past_its_target() {
    #[derive(Debug)]
    struct Forwarder;
    impl crate::widget::Widget for Forwarder {
        fn build(&mut self, ctx: &mut crate::BuildContext) -> Vec<WidgetId> {
            ctx.receive_every_key_release();
            vec![]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let parent_log = log.clone();
    let parent = tree.add(StackWidget::new().on_key(move |event, _ctx| {
        if matches!(event, WidgetEvent::KeyUp { .. }) {
            parent_log.borrow_mut().push(("parent", "up"));
        }
        EventResponse::Ignored
    }));
    let child_log = log.clone();
    let forwarder = tree.add_child(
        parent,
        Forwarder.focusable(true).on_key(move |event, _ctx| {
            if matches!(event, WidgetEvent::KeyUp { .. }) {
                child_log.borrow_mut().push(("forwarder", "up"));
            }
            EventResponse::Ignored
        }),
    );
    let content = tree.add(StackWidget::new());
    let inside = field(
        &mut tree,
        content,
        "inside",
        &log,
        Rc::new(RefCell::new(None)),
    );
    lay_out(&mut tree);
    tree.focus(forwarder);
    down(&mut tree, Key::Enter);
    tree.activate(content);
    tree.show_overlay(OverlayRequest::new(
        content,
        forwarder,
        OverlayPlacement::Centered,
        DismissBehavior::EscapeKey,
    ));
    lay_out(&mut tree);
    tree.focus(inside);
    up(&mut tree, Key::Enter);
    assert_eq!(*log.borrow(), [("forwarder", "up")]);
}
