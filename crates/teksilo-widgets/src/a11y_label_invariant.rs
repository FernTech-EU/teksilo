// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The label invariants, for the widgets no registry reaches.
//!
//! The previewer walks every `WidgetCatalog` entry and every documentation
//! snippet and runs the same three checks over each — that is the census.
//! Fourteen widget modules are in neither registry, though, and two of
//! them held a leak the audit found: the calendar's zoom cell, which only
//! exists after a header click, and the privacy-settings rows, which
//! painted their label twice. A widget nothing renders is a widget nothing
//! checks, so those are constructed here by hand. The rest of the
//! unregistered set — the toast surface, the modal bodies, the menu and
//! popover surfaces — are covered by tests in their own modules, where
//! their private construction paths are reachable.
//!
//! The three checks are the ones in `teksilo_core::accessibility::audit`:
//! no label repeats a strict ancestor's own name; every visible label
//! carries text ranges; and what a reader reviews is what the node
//! announces, with geometry that intersects the label's own box.

#![cfg(test)]

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, SizeProposal, TextBackend};
use teksilo_core::accessibility::audit;
use teksilo_core::widget::Widget;
use teksilo_core::widget_tree::WidgetTree;

/// Build `widget` in a tree with a measuring backend and run every
/// invariant over the emitted accessibility tree.
///
/// The backend is not optional: without one a label reports no geometry,
/// and the geometry assertions would pass by being unmeasurable rather
/// than by being right.
#[track_caller]
fn assert_invariants(what: &str, widget: impl Widget + 'static) {
    assert_invariants_in(what, WidgetTree::new(), widget);
}

/// [`assert_invariants`] in a tree the caller prepared, for a widget that
/// builds what is under test only from something in its app state.
#[track_caller]
fn assert_invariants_in(what: &str, tree: WidgetTree, widget: impl Widget + 'static) {
    let backend: Rc<RefCell<dyn TextBackend>> = Rc::new(RefCell::new(MockTextBackend::new()));
    let mut tree = tree
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(backend);
    tree.add(widget);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let update = tree.sync_accessibility();

    let leaks = audit::duplicate_label_leaks(&update);
    assert!(
        leaks.is_empty(),
        "{what}: a label repeats an ancestor's accessible name: {leaks:?}"
    );
    let unreviewable = audit::labels_without_text_ranges(&update);
    assert!(
        unreviewable.is_empty(),
        "{what}: these labels carry no text ranges, so a reader can reach them \
         but cannot review them by character: {unreviewable:?}"
    );
    let divergences = audit::text_range_divergences(&update);
    assert!(
        divergences.is_empty(),
        "{what}: what a reader reviews is not what the node announces: {divergences:?}"
    );
}

#[test]
fn a_calendar_holds_the_invariants() {
    // The zoom cell only exists after a header click, so no registry ever
    // renders it — and it is where the audit found a leak. The day grid
    // reaches it through the same style, so building the calendar at all
    // exercises the label the leak was on.
    use crate::calendar::Calendar;
    let date = teksilo_core::signal::Signal::new(None);
    assert_invariants("calendar", Calendar::single(date));
}

#[cfg(feature = "telemetry")]
#[test]
fn a_privacy_settings_row_holds_the_invariants() {
    // These rows painted their label twice: once as the row's own text,
    // once as the toggle's. The screen showed it as well as the reader.
    // The rows exist only with telemetry configured; without it this built
    // the placeholder, and checked no row.
    let dir = tempfile::tempdir().unwrap();
    assert_invariants_in(
        "privacy settings",
        crate::privacy_settings::tree_with_telemetry(dir.path()),
        crate::privacy_settings::PrivacySettings::new(),
    );
}
