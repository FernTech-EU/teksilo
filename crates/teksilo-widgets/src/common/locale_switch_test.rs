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
//! publish the date in full on their own AT node, written for a listener,
//! so only the `Role::TextInput` descendant shows the editable pattern.
//!
//! It also holds [`speaking`], the setup every test of what a date widget
//! *says* shares: the framework's own strings in one language, and a tree in
//! the same locale.

use std::rc::Rc;

use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::I18nManager;

/// A tree speaking `tag`: the framework's own strings installed on this
/// thread the way an application that registers `framework_locales()`
/// installs them, and the tree given the same locale, as the app layer gives
/// it. The caller clears the thread-local when it is done.
pub(crate) fn speaking(tag: &str) -> (Rc<I18nManager>, WidgetTree) {
    use teksilo_i18n::{I18nConfig, thread_local};
    thread_local::clear();
    let cfg = I18nConfig::new()
        .supported_locales([
            "en-US".parse().unwrap(),
            "fr-FR".parse().unwrap(),
            "ja-JP".parse().unwrap(),
        ])
        .auto_detect_os_locale(false)
        .framework_locales(crate::framework_locales());
    let mgr = I18nManager::from_config(&cfg);
    mgr.set_locale(tag.parse().unwrap());
    thread_local::install(mgr.clone());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.set_locale(tag.to_string());
    (mgr, tree)
}

/// Name and value of the one calendar grid published in `tree`, which is
/// what a screen reader says of it.
pub(crate) fn spoken_grid(tree: &mut WidgetTree) -> (String, String) {
    let update = tree.sync_accessibility();
    let grids: Vec<(String, String)> = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == teksilo_core::accesskit::Role::Grid)
        .map(|(_, node)| {
            (
                node.label().unwrap_or_default().to_string(),
                node.value().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(grids.len(), 1, "one calendar grid published: {grids:?}");
    grids.into_iter().next().unwrap()
}

/// Every displayed date/time text at or below `root`, in tree order.
///
/// Descendants only: these widgets publish the date in full on their *own*
/// AT node, which is not the pattern. One level down is where the rendered
/// pattern shows: as a `TextInput` for a standalone field, or as the
/// `DateInput` / `TimeInput` sub-fields of a composed one like
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
