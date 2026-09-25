// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a keyboard and screen-reader user gets from the bell and the log while
//! notifications keep arriving.
//!
//! Every toast is mirrored into the archive, and a background job mirrors each
//! of its progress steps. The bell and the log used to rebuild themselves on
//! every archive change, which replaced the control the reader was on: focus
//! was re-fired on a fresh bell, an open popover closed under the reader, and
//! the log threw focus back to its first button at every step.

use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::styles::{BannerSeverity, ToastPriority};
use teksilo_core::widget_tree::WidgetTree;

use crate::common::heard_test::{Heard, Listener};
use crate::notification::{
    NotificationArchiveModel, NotificationCenterButton, NotificationEntry, NotificationLog,
};
use crate::primitives::{FixedSize, Spacer, VStack};
use crate::toast::ToastRoute;

const WINDOW: SizeProposal = SizeProposal {
    width: Some(500.0),
    height: Some(700.0),
};

fn entry(title: &str) -> NotificationEntry {
    NotificationEntry {
        id: 0,
        severity: BannerSeverity::Info,
        priority: ToastPriority::Normal,
        title: title.to_string(),
        body: None,
        actions: Vec::new(),
        timestamp: jiff::Timestamp::now(),
        group: None,
        source: None,
        read: false,
        dedup_id: None,
        updates: Vec::new(),
        route: ToastRoute::Broadcast,
    }
}

/// One progress step of a job: every step merges into the same archive row.
fn job_step(step: u32) -> NotificationEntry {
    NotificationEntry {
        body: Some(format!("{}% done", step * 5)),
        dedup_id: Some("job".to_string()),
        ..entry("Background job")
    }
}

fn bell_label() -> String {
    teksilo_i18n::tr_widget!(a11y_builtin_bell()).resolve_now()
}

/// A status bar with the bell at the bottom of the window.
fn bell_tree(archive: &Rc<NotificationArchiveModel>) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let spacer = tree.add(FixedSize::new().height(600.0).child(Spacer::new()));
    let bell = tree.add(NotificationCenterButton::new(archive.clone()));
    tree.add(VStack::new().child(spacer).child(bell));
    tree.layout(WINDOW);
    tree
}

/// A notification arriving while focus is on the bell leaves the bell, and
/// focus on it, alone (catalog-c-14). The badge still follows the count.
#[test]
fn a_notification_leaves_the_focused_bell_in_place() {
    let archive = Rc::new(NotificationArchiveModel::in_memory());
    archive.push(entry("Info notice #1"));
    let mut tree = bell_tree(&archive);

    let bell = tree.find_by_label(&bell_label()).expect("the bell button");
    tree.focus(bell);
    tree.layout(WINDOW);
    let mut reader = Listener::attach(&mut tree);

    archive.push(entry("Saved #2"));
    tree.layout(WINDOW);

    assert_eq!(
        tree.focused(),
        Some(bell),
        "the focused bell is the same node"
    );
    assert!(
        !reader
            .heard(&mut tree)
            .iter()
            .any(|h| matches!(h, Heard::Focus(_))),
        "no focus event is fired on a replacement bell"
    );
    assert!(
        tree.find_by_label("2").is_some(),
        "the badge follows the unread count"
    );
}

/// The bell's popover stays open, with focus where the reader left it, while
/// notifications arrive and after Mark all read (toast-v1).
#[test]
fn the_bell_popover_stays_open_while_notifications_arrive() {
    let archive = Rc::new(NotificationArchiveModel::in_memory());
    archive.push(entry("Info notice #1"));
    let mut tree = bell_tree(&archive);

    let bell = tree.find_by_label(&bell_label()).expect("the bell button");
    tree.click(bell);
    tree.layout(WINDOW);
    assert_eq!(tree.active_overlays().len(), 1, "the popover is open");

    let mark_read_label = teksilo_i18n::tr_widget!(notifications_mark_all_read()).resolve_now();
    let mark_read = tree
        .find_by_label(&mark_read_label)
        .expect("Mark all read in the popover");
    tree.focus(mark_read);
    tree.layout(WINDOW);

    archive.push(entry("Warning #2"));
    tree.layout(WINDOW);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "a new notification does not close the popover"
    );
    assert_eq!(
        tree.focused(),
        Some(mark_read),
        "focus stays on Mark all read"
    );

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(WINDOW);
    assert_eq!(archive.unread_count().get(), 0, "Mark all read did its job");
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "and left the reader in the popover"
    );
    assert_eq!(tree.focused(), Some(mark_read));
}

fn log_tree(archive: &Rc<NotificationArchiveModel>) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(NotificationLog::new(archive.clone()));
    tree.layout(WINDOW);
    tree
}

/// A job archiving its progress behind the log does not take focus off the
/// log's buttons (toast-v2), and Clear all says what it did.
#[test]
fn the_log_keeps_focus_on_clear_all_while_a_job_archives_its_progress() {
    let archive = Rc::new(NotificationArchiveModel::in_memory());
    archive.push(entry("Info notice #1"));
    archive.push(job_step(0));
    let mut tree = log_tree(&archive);

    let clear_label = teksilo_i18n::tr_widget!(notifications_clear()).resolve_now();
    let clear = tree.find_by_label(&clear_label).expect("Clear all");
    tree.focus(clear);
    tree.layout(WINDOW);
    let mut reader = Listener::attach(&mut tree);

    for step in 1..=5 {
        archive.push(job_step(step));
        tree.layout(WINDOW);
    }
    assert_eq!(tree.focused(), Some(clear), "focus stays on Clear all");
    assert!(
        !reader
            .heard(&mut tree)
            .iter()
            .any(|h| matches!(h, Heard::Focus(_))),
        "no step moves focus"
    );

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(WINDOW);
    assert_eq!(archive.entries().len(), 0, "Clear all emptied the log");
    assert_eq!(tree.focused(), Some(clear), "focus stays on Clear all");
    let empty = teksilo_i18n::tr_widget!(notifications_empty()).resolve_now();
    assert!(
        reader
            .heard(&mut tree)
            .contains(&Heard::Live(empty.clone())),
        "the reader hears that the log is now empty ({empty:?})"
    );
}

/// A row whose notification did not change is the same row after a job
/// archives another step: a reader on it stays there.
#[test]
fn the_log_keeps_focus_on_an_unchanged_row_while_a_job_archives_its_progress() {
    let archive = Rc::new(NotificationArchiveModel::in_memory());
    archive.push(entry("Info notice #1"));
    archive.push(job_step(0));
    let mut tree = log_tree(&archive);

    let row = tree.find_by_label("Info notice #1").expect("the info row");
    let row = tree.first_focusable_descendant(row).unwrap_or(row);
    tree.focus(row);
    tree.layout(WINDOW);
    assert_eq!(
        tree.focused(),
        Some(row),
        "the row takes focus (the premise)"
    );

    for step in 1..=3 {
        archive.push(job_step(step));
        tree.layout(WINDOW);
    }
    assert_eq!(tree.focused(), Some(row), "focus stays on the info row");
}
