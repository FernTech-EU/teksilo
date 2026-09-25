// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader and a keyboard user get from the toast stack.
//!
//! Each test mounts a real [`ToastHost`] the way `install_toast` does and
//! listens to the tree through `accesskit_consumer`, as the platform adapters
//! do (see `crate::common::heard_test`). The findings they pin came from a
//! screen-reader sweep of `toast-demo` under Orca: a host that rebuilt every
//! toast on every queue change re-announced each toast still on screen, and
//! threw keyboard focus onto the oldest toast whenever the focused control was
//! replaced.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::button::Button;
use crate::common::heard_test::{Heard, Listener};
use crate::primitives::{Expand, ZStack};
use crate::toast::{Toast, ToastAction, ToastHost, ToastInstallOptions, ToastRegistry};

const WINDOW: SizeProposal = SizeProposal {
    width: Some(900.0),
    height: Some(600.0),
};

fn registry() -> ToastRegistry {
    ToastRegistry::new(options())
}

fn options() -> ToastInstallOptions {
    ToastInstallOptions {
        archive: None,
        ..ToastInstallOptions::default()
    }
}

/// `ZStack { Expand(root), host }`, as `install_toast` wraps every window,
/// with one focusable button standing in for the application.
fn host_tree(registry: &ToastRegistry) -> (WidgetTree, WidgetId, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let app = tree.add(Button::new(lit!("Start")));
    let host = tree.add(ToastHost::new(registry.clone(), options()));
    let filled = tree.add(Expand::new().respect_intrinsic().child(app));
    tree.add(ZStack::new().child(filled).child(host));
    tree.layout(WINDOW);
    (tree, host, app)
}

fn live(text: &str) -> Heard {
    Heard::Live(text.to_string())
}

/// Focus `id` as the keyboard does (the reader Tabbed to it).
fn tab_to(tree: &mut WidgetTree, id: WidgetId) {
    tree.focus_with_origin(id, FocusOrigin::Keyboard);
    tree.layout(WINDOW);
}

/// A toast shown while others are up is heard, and nothing already on screen
/// is heard again: each adapter announces a live node when it *enters* the
/// tree, so a host that replaced every toast's node on every change announced
/// every toast still up (toast-01, catalog-c-13).
#[test]
fn a_new_toast_is_heard_and_the_toasts_already_shown_are_not() {
    let registry = registry();
    let (mut tree, _host, _app) = host_tree(&registry);
    let mut reader = Listener::attach(&mut tree);

    registry.enqueue(Toast::info(lit!("Info notice #1")));
    tree.layout(WINDOW);
    assert_eq!(reader.heard(&mut tree), vec![live("Info notice #1")]);

    registry.enqueue(Toast::success(lit!("Saved #2")));
    tree.layout(WINDOW);
    assert_eq!(
        reader.heard(&mut tree),
        vec![live("Saved #2")],
        "only the toast that just appeared is heard"
    );

    registry.enqueue(Toast::error(lit!("Build #3 failed")).body(lit!("Three errors.")));
    tree.layout(WINDOW);
    assert_eq!(reader.heard(&mut tree), vec![live("Build #3 failed")]);
}

/// A toast that expires leaves quietly, and the one still up is not announced
/// again as if it had just arrived.
#[test]
fn an_expiring_toast_leaves_the_others_unannounced() {
    let registry = registry();
    let (mut tree, _host, _app) = host_tree(&registry);
    let mut reader = Listener::attach(&mut tree);

    registry.enqueue(Toast::error(lit!("Sticky error #1")).persistent());
    registry.enqueue(
        Toast::info(lit!("Info notice #2")).auto_dismiss_after(Duration::from_millis(500)),
    );
    tree.layout(WINDOW);
    let _ = reader.heard(&mut tree);

    assert!(registry.tick_timers(Duration::from_millis(600), false));
    tree.layout(WINDOW);
    assert_eq!(
        reader.heard(&mut tree),
        Vec::<Heard>::new(),
        "the sticky error is still on screen and must not be read again"
    );
}

/// A toast arriving while the reader is inside another toast leaves focus
/// where it is (toast-04, "focus-second").
#[test]
fn a_new_toast_leaves_focus_on_the_toast_the_reader_is_in() {
    let registry = registry();
    let (mut tree, _host, _app) = host_tree(&registry);
    registry.enqueue(Toast::error(lit!("Sticky error #1")).persistent());
    registry.enqueue(Toast::warning(lit!("Warning #2")).persistent());
    tree.layout(WINDOW);

    let warning = tree.find_by_label("Warning #2").expect("the warning toast");
    tab_to(&mut tree, warning);
    let mut reader = Listener::attach(&mut tree);

    registry.enqueue(Toast::success(lit!("Saved #3")));
    tree.layout(WINDOW);

    assert_eq!(
        tree.focused(),
        Some(warning),
        "focus stays on the warning toast"
    );
    assert_eq!(
        reader.heard(&mut tree),
        vec![live("Saved #3")],
        "the new toast is heard, and no focus move"
    );
}

/// A progress toast updated in place keeps its node and its Cancel: a reader
/// who Tabs to Cancel and listens can still press it at the next step, and
/// the title is not announced again at every step (toast-04, toast-02).
///
/// When the job ends and Cancel goes away, focus stays in the job's own toast;
/// it is not thrown onto the oldest toast of the stack.
#[test]
fn a_progress_toast_keeps_its_cancel_under_focus_and_says_its_title_once() {
    let registry = registry();
    let (mut tree, _host, _app) = host_tree(&registry);
    // An older toast, so "the first focusable toast" and "the job's toast"
    // are different places.
    registry.enqueue(Toast::error(lit!("Sticky error #1")).persistent());
    tree.layout(WINDOW);
    let mut reader = Listener::attach(&mut tree);

    let pressed_at = Rc::new(Cell::new(None::<u32>));
    let job = |step: u32| {
        let pressed_at = pressed_at.clone();
        Toast::loading(lit!("Background job"))
            .id("job")
            .body(lit!(format!("{}% done", step * 5)))
            .action(
                ToastAction::destructive(lit!("Cancel"), move |_| pressed_at.set(Some(step)))
                    .closes_toast(false),
            )
    };

    registry.enqueue(job(0));
    tree.layout(WINDOW);
    // Whether the older toast is heard again here is the first test's
    // business; this one is about the job.
    assert!(reader.heard(&mut tree).contains(&live("Background job")));

    let cancel = tree.find_by_label("Cancel").expect("the job's Cancel");
    tab_to(&mut tree, cancel);
    let _ = reader.heard(&mut tree);

    for step in 1..=5 {
        registry.enqueue(job(step));
        tree.layout(WINDOW);
    }
    assert_eq!(
        tree.focused(),
        Some(cancel),
        "each step keeps the Cancel the reader is on"
    );
    assert_eq!(
        reader.heard(&mut tree),
        Vec::<Heard>::new(),
        "no step moves focus or announces the unchanged title again"
    );

    tree.press_key(Key::Space, Modifiers::NONE);
    assert_eq!(
        pressed_at.get(),
        Some(5),
        "Space on Cancel runs the action of the latest step"
    );

    registry.enqueue(
        Toast::success(lit!("Background job complete"))
            .id("job")
            .auto_dismiss_after(Duration::from_secs(5)),
    );
    tree.layout(WINDOW);
    let job_toast = tree
        .find_by_label("Background job complete")
        .expect("the completed job's toast");
    let focused = tree.focused().expect("focus is not dropped with Cancel");
    assert!(
        focused == job_toast || tree.is_descendant_of(focused, job_toast),
        "focus stays in the job's toast, not the oldest toast of the stack"
    );
    assert!(
        reader
            .heard(&mut tree)
            .contains(&live("Background job complete")),
        "the completion is heard"
    );
}

/// While keyboard focus is inside a toast its timer stands still, as it does
/// under a resting pointer (toast-07, WCAG 2.2.1).
#[test]
fn a_toast_does_not_expire_while_focus_is_inside_it() {
    let registry = registry();
    let (mut tree, _host, app) = host_tree(&registry);
    let (handle, _) = registry.enqueue(
        Toast::error(lit!("Build #1 failed"))
            .action(ToastAction::primary(lit!("Show errors"), |_| {})),
    );
    tree.layout(WINDOW);

    let action = tree
        .find_by_label("Show errors")
        .expect("the toast's action");
    tab_to(&mut tree, action);

    registry.tick_timers(Duration::from_secs(11), false);
    tree.layout(WINDOW);
    assert!(
        registry.is_entry_alive(handle.entry_id()),
        "the toast does not time out under keyboard focus"
    );
    assert_eq!(tree.focused(), Some(action));

    tab_to(&mut tree, app);
    registry.tick_timers(Duration::from_secs(11), false);
    assert!(
        !registry.is_entry_alive(handle.entry_id()),
        "once focus has left, the timer runs again"
    );
}

/// A toast shown while another is up gets its own time. The host's tick used
/// to charge every toast for the whole interval since its last tick, so a toast
/// that arrived in that interval paid for time before it existed and left with
/// the older one (toast-06).
#[test]
fn a_toast_shown_beside_another_is_not_charged_for_time_before_it_existed() {
    let registry = registry();
    let (mut tree, _host, _app) = host_tree(&registry);
    registry.enqueue(Toast::info(lit!("Info notice #1")));
    tree.layout(WINDOW);

    std::thread::sleep(Duration::from_millis(400));
    let (late, _) = registry
        .enqueue(Toast::success(lit!("Saved #2")).auto_dismiss_after(Duration::from_millis(600)));
    tree.layout(WINDOW);

    // The next frame runs the host's timer.
    tree.request_frame();
    tree.layout(WINDOW);

    let left = registry
        .with_entry(late.entry_id(), |e| e.time_left)
        .flatten()
        .expect("the second toast is still up");
    assert!(
        left >= Duration::from_millis(450),
        "the second toast was charged for the 400 ms before it was shown: {left:?} left of 600 ms"
    );
}

/// A toast taken off screen while focus was inside it does not leave the
/// timers paused: taking the focused control down moves focus out of the
/// surface, and the surface reports it before it goes. A window whose content
/// is rebuilt, or closed, takes the surface with it while the toast lives on.
#[test]
fn a_surface_taken_down_with_focus_inside_does_not_hold_the_timers() {
    let registry = registry();
    let (mut tree, host, _app) = host_tree(&registry);
    let (handle, _) = registry.enqueue(
        Toast::error(lit!("Build #1 failed"))
            .action(ToastAction::primary(lit!("Show errors"), |_| {})),
    );
    tree.layout(WINDOW);
    let action = tree
        .find_by_label("Show errors")
        .expect("the toast's action");
    tab_to(&mut tree, action);

    tree.destroy_subtree_for_testing(host);
    registry.tick_timers(Duration::from_secs(11), false);
    assert!(
        !registry.is_entry_alive(handle.entry_id()),
        "with no surface left holding focus, the toast times out"
    );
}

/// A window closed with keyboard focus inside a toast does not leave the
/// timers paused. A closed window's tree is dropped whole, with no focus
/// change, so nothing tells the registry that focus left; the surface says it
/// as it is dropped. Otherwise no toast in any other window would ever time
/// out again.
#[test]
fn a_window_closed_with_focus_in_a_toast_does_not_hold_the_timers() {
    let registry = registry();
    let (mut first, _, _) = host_tree(&registry);
    let (mut second, _, _) = host_tree(&registry);
    let (handle, _) = registry.enqueue(
        Toast::error(lit!("Build #1 failed"))
            .action(ToastAction::primary(lit!("Show errors"), |_| {})),
    );
    first.layout(WINDOW);
    second.layout(WINDOW);
    let action = second
        .find_by_label("Show errors")
        .expect("the toast's action in the second window");
    tab_to(&mut second, action);
    registry.tick_timers(Duration::from_secs(11), false);
    assert!(
        registry.is_entry_alive(handle.entry_id()),
        "the premise: focus in the second window holds the toast"
    );

    drop(second);
    first.layout(WINDOW);
    registry.tick_timers(Duration::from_secs(11), false);
    assert!(
        !registry.is_entry_alive(handle.entry_id()),
        "once the window holding focus is closed, the toast times out"
    );
}

/// A toast shown in two windows stays held while focus is inside it in either
/// one. An update in place rebuilds its surface in both, and the window
/// without focus must not clear the pause the other one holds.
#[test]
fn focus_in_one_window_holds_a_toast_that_another_window_rebuilds() {
    let registry = registry();
    let (mut first, _, _) = host_tree(&registry);
    let (mut second, _, _) = host_tree(&registry);
    let job = |percent: u32| {
        Toast::info(lit!("Background job"))
            .id("job")
            .body(lit!(format!("{percent}% done")))
            .action(ToastAction::primary(lit!("Show"), |_| {}).closes_toast(false))
    };
    let (handle, _) = registry.enqueue(job(0));
    first.layout(WINDOW);
    second.layout(WINDOW);
    let action = second
        .find_by_label("Show")
        .expect("the toast's action in the second window");
    tab_to(&mut second, action);

    registry.enqueue(job(5));
    second.layout(WINDOW);
    first.layout(WINDOW);
    assert_eq!(second.focused(), Some(action), "focus stays on Show");
    registry.tick_timers(Duration::from_secs(11), false);
    assert!(
        registry.is_entry_alive(handle.entry_id()),
        "the first window's rebuild does not end the second window's pause"
    );
}
