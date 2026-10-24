// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared test helper for composites built disabled and enabled later.
//!
//! A composite that reads its `enabled` prop once in `build()` and hands the
//! `bool` to its parts freezes them: nothing rebuilds the composite when a
//! bound `Signal<bool>` flips, so parts built disabled stay disabled after it
//! is enabled. The composite's own `enabled_when` already disables its whole
//! subtree, so a part needs no copy of the value.

use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

/// Every node at or under `root` the arena reports as disabled.
pub(crate) fn disabled_nodes_under(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    fn collect(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        if !tree.is_enabled(id) {
            out.push(id);
        }
        for child in tree.children(id) {
            collect(tree, child, out);
        }
    }
    let mut out = Vec::new();
    collect(tree, root, &mut out);
    out
}
