// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared test helper for the locale-switch regression suite.
//!
//! Every datetime editing widget derives a *display convention* — a
//! strftime-subset pattern, or a 12-vs-24-hour clock — from the locale at
//! `build()` time. `WidgetTree::set_locale` only calls `mark_all_dirty`
//! (layout + paint), so without a `Rebuild`-level binding on the locale
//! signal those conventions freeze at whatever locale was active when the
//! widget was first built. Each widget carries a test asserting it
//! re-derives; they all need the same observable.
//!
//! The observable has to be the *inner* editing surface: these widgets
//! deliberately publish an ISO value on their own AT node (locale-
//! independent by design), so only the `Role::TextInput` descendant shows
//! the rendered pattern.

use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

/// Every displayed date/time text at or below `root`, in tree order.
///
/// Descendants only: these widgets publish an ISO value on their *own* AT
/// node (locale-independent by design), so the root would report a string
/// that never changes with the locale. One level down is where the
/// rendered pattern shows — as a `TextInput` for a standalone field, or as
/// the `DateInput` / `TimeInput` sub-fields of a composed one like
/// `DateTimeEdit`.
pub(crate) fn displayed_texts(tree: &mut WidgetTree, root: WidgetId) -> Vec<String> {
    use teksilo_core::accesskit::Role;

    fn collect(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        out.push(id);
        for c in tree.children(id) {
            collect(tree, c, out);
        }
    }
    let mut ids = Vec::new();
    for c in tree.children(root) {
        collect(tree, c, &mut ids);
    }

    let update = tree.sync_accessibility();
    ids.iter()
        .filter_map(|id| {
            let target = teksilo_core::accessibility::widget_id_to_node_id(*id);
            update
                .nodes
                .iter()
                .find(|(nid, _)| *nid == target)
                .filter(|(_, n)| {
                    matches!(
                        n.role(),
                        Role::TextInput | Role::DateInput | Role::TimeInput
                    )
                })
                .and_then(|(_, n)| n.value().map(str::to_string))
        })
        .collect()
}

/// The first entry of [`displayed_texts`], for the single-field widgets.
pub(crate) fn displayed_text(tree: &mut WidgetTree, root: WidgetId) -> Option<String> {
    displayed_texts(tree, root).into_iter().next()
}
