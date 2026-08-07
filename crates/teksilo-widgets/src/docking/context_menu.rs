// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared context menus for dock activities (rail items + tab-strip tabs), the
//! activity-bar background, and the per-dock header **⋮ options** button.
//!
//! The per-activity menu (right-click a rail item or a dock tab) is:
//!
//! ```text
//! Hide "<activity>"
//! ──────────────
//! Move to              ▸  <enabled other sides>
//! ──────────────
//! ☑ <activity>            (one checkable row per activity in this side)
//! ☑ <activity>
//! ──────────────
//! Activity bar size    ▸  Default / Compact / Icon + Label   (rail item)
//!   – or –
//! Tab size             ▸  Text / Icon / Icon + Text   (dock tab)
//! ```
//!
//! The activity-bar **background** menu (right-click empty rail space) offers
//! just the checkable activity list + Activity bar size — the affordance to
//! restore an activity once every item has been hidden.
//!
//! The dock-header **⋮ options** menu ([`dock_options_menu`]) is the
//! always-visible counterpart (the VS Code "More actions" pattern). For a dock
//! that is one of several panes in a grouped activity it offers dock-level
//! relocation ("Move to new activity", "Move to side ▸") + Close; for a
//! sole-pane dock (which *is* its activity) it offers the activity-level Hide /
//! Move to / Close.
//!
//! Every "Move to" surface lists only [enabled sides](DockingModel::enabled_move_targets);
//! a disabled side is never offered (it would be silently rejected).

use teksilo_i18n::{LocalizedString, lit};

use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;

use super::geometry::DockSide;
use super::model::{DockOpenLocation, DockTabId, DockTabView, DockWidgetId, DockingModel};

/// Which trailing submenu the per-activity menu ends with.
#[derive(Clone, Copy)]
pub(crate) enum DockMenuKind {
    /// A rail item — ends with "Activity bar size".
    Rail,
    /// A dock tab — ends with "Tab size".
    Strip,
}

/// Friendly side name for the "Move to" submenu.
fn side_name(side: DockSide) -> LocalizedString {
    match side {
        DockSide::Leading => lit!("Leading"),
        DockSide::Trailing => lit!("Trailing"),
        DockSide::Top => lit!("Top"),
        DockSide::Bottom => lit!("Bottom"),
    }
}

/// The full per-activity context menu for a rail item or a dock tab.
pub(crate) fn activity_context_menu(
    model: &DockingModel,
    side: DockSide,
    tab_id: DockTabId,
    kind: DockMenuKind,
) -> MenuList {
    let tabs = model.side_tabs(side);
    let this_label = tabs
        .iter()
        .find(|v| v.id == tab_id)
        .map(|v| model.activity_label(v))
        .unwrap_or_else(|| lit!("Panel"));

    // Policy gates the user affordances: hiding an activity (Hide item +
    // checklist) and relocating one (Move to). The size / display submenu is an
    // appearance pref and always shown. `has_items` keeps separators tidy as
    // sections drop out.
    let policy = model.policy();
    let mut list = MenuList::new();
    let mut has_items = false;

    // Hide "<activity>".
    if policy.allow_activity_hide {
        let m = model.clone();
        let hide_label = lit!(format!("Hide \"{}\"", this_label.resolve_now()));
        list = list.item(
            MenuItem::new(hide_label).on_activate_fn(move |_| m.set_tab_hidden(tab_id, true)),
        );
        has_items = true;
    }

    // Move to ▸ <enabled other sides>. Omitted entirely when no enabled side is
    // available (a disabled side would only be a silently-rejected dead end).
    if policy.allow_activity_drag && !model.enabled_move_targets(side).is_empty() {
        if has_items {
            list = list.separator();
        }
        let m = model.clone();
        list = list.item(MenuItem::submenu(lit!("Move to"), move || {
            Box::new(move_to_submenu(&m, side, tab_id))
        }));
        has_items = true;
    }

    // Checkable list of this side's activities (hide / restore).
    if policy.allow_activity_hide {
        if has_items {
            list = list.separator();
        }
        list = activity_checkitems(list, model, side, &tabs);
        has_items = true;
    }

    if has_items {
        list = list.separator();
    }

    // Activity bar size (rail) / Tab size (strip).
    list = match kind {
        DockMenuKind::Rail => {
            let m = model.clone();
            list.item(MenuItem::submenu(lit!("Activity bar size"), move || {
                Box::new(rail_size_submenu(&m, side))
            }))
        }
        DockMenuKind::Strip => {
            let m = model.clone();
            list.item(MenuItem::submenu(lit!("Tab size"), move || {
                Box::new(tab_size_submenu(&m, side))
            }))
        }
    };

    list
}

/// The dock-header **⋮ options** menu. `multi_pane` is whether this dock shares
/// its activity with other docks (a grouped, split activity):
///
/// * **multi-pane** → dock-level relocation: `Move to new activity`
///   ([`promote_to_tab`](DockingModel::promote_to_tab)) + `Move to side ▸`
///   ([`dock_move_to_submenu`]).
/// * **sole-pane** (the dock *is* the activity) → activity-level `Hide` +
///   `Move to ▸`.
///
/// All gated by [`DockPolicy`](super::model::DockPolicy). There is no `Close`:
/// a dock can only be **hidden** (and restored from the activity checklist) —
/// closing would leave no way to bring it back.
pub(crate) fn dock_options_menu(
    model: &DockingModel,
    side: DockSide,
    tab_id: DockTabId,
    dock_id: DockWidgetId,
    multi_pane: bool,
) -> MenuList {
    let policy = model.policy();
    let has_targets = !model.enabled_move_targets(side).is_empty();
    let mut list = MenuList::new();

    if multi_pane {
        if policy.allow_activity_drag {
            // Pull this dock out of the group into its own new activity.
            let m = model.clone();
            list = list.item(MenuItem::new(lit!("Move to new activity")).on_activate_fn(
                move |_| {
                    let at = m.side_append_index(side);
                    m.promote_to_tab(dock_id, side, at);
                },
            ));
            // Move just this dock to another side (as its own new activity).
            if has_targets {
                let m = model.clone();
                list = list.item(MenuItem::submenu(lit!("Move to side"), move || {
                    Box::new(dock_move_to_submenu(&m, dock_id, side))
                }));
            }
        }
    } else {
        // Sole-pane dock == its activity: the activity-level actions belong here.
        let mut has_items = false;
        if policy.allow_activity_hide {
            let this_label = model
                .side_tabs(side)
                .iter()
                .find(|v| v.id == tab_id)
                .map(|v| model.activity_label(v))
                .unwrap_or_else(|| lit!("Panel"));
            let m = model.clone();
            let hide_label = lit!(format!("Hide \"{}\"", this_label.resolve_now()));
            list = list.item(
                MenuItem::new(hide_label).on_activate_fn(move |_| m.set_tab_hidden(tab_id, true)),
            );
            has_items = true;
        }
        if policy.allow_activity_drag && has_targets {
            if has_items {
                list = list.separator();
            }
            let m = model.clone();
            list = list.item(MenuItem::submenu(lit!("Move to"), move || {
                Box::new(move_to_submenu(&m, side, tab_id))
            }));
        }
    }

    list
}

/// Whether [`dock_options_menu`] would produce at least one item — so the
/// header `⋮` button can be omitted when it would only open an empty menu (a
/// dock under a fully-locked policy).
pub(crate) fn dock_has_options(model: &DockingModel, side: DockSide, multi_pane: bool) -> bool {
    let policy = model.policy();
    if multi_pane {
        // "Move to new activity" is always present when activity-drag is allowed.
        policy.allow_activity_drag
    } else {
        policy.allow_activity_hide
            || (policy.allow_activity_drag && !model.enabled_move_targets(side).is_empty())
    }
}

/// The background menu (right-click empty rail / side chrome): the checkable
/// activities list + the kind-appropriate size submenu. This is the affordance
/// to restore an activity after every item has been hidden.
pub(crate) fn background_menu(
    model: &DockingModel,
    side: DockSide,
    kind: DockMenuKind,
) -> MenuList {
    let tabs = model.side_tabs(side);
    // The checklist is the hide / restore affordance — omit it when activity
    // hiding is locked (leaving just the appearance submenu).
    let mut list = MenuList::new();
    if model.policy().allow_activity_hide {
        list = activity_checkitems(list, model, side, &tabs);
        list = list.separator();
    }
    let m = model.clone();
    match kind {
        DockMenuKind::Rail => list.item(MenuItem::submenu(lit!("Activity bar size"), move || {
            Box::new(rail_size_submenu(&m, side))
        })),
        DockMenuKind::Strip => list.item(MenuItem::submenu(lit!("Tab size"), move || {
            Box::new(tab_size_submenu(&m, side))
        })),
    }
}

/// Append one checkable row per activity (checked = visible). The checkmark is
/// bound to the model's **live** per-activity hidden signal (so an external
/// `set_tab_hidden` updates it), and activation flips the *current* model state.
fn activity_checkitems(
    mut list: MenuList,
    model: &DockingModel,
    _side: DockSide,
    tabs: &[DockTabView],
) -> MenuList {
    for view in tabs {
        let tab_id = view.id;
        let label = model.activity_label(view);
        // Reflect-only: checked = visible = !hidden, tracked live from the model
        // (NOT a frozen snapshot). The activation owns the write.
        let visible_sig = model.tab_hidden_signal(tab_id).map(|h| !*h);
        let m = model.clone();
        list = list.item(
            MenuItem::new(label)
                .reflect_checked(visible_sig)
                .on_activate_fn(move |_| m.set_tab_hidden(tab_id, !m.is_tab_hidden(tab_id))),
        );
    }
    list
}

/// "Move to" submenu: one row per *enabled* other side; relocates the whole tab
/// there, shows that side, and selects the moved tab.
fn move_to_submenu(model: &DockingModel, from: DockSide, tab_id: DockTabId) -> MenuList {
    let mut list = MenuList::new();
    for target in model.enabled_move_targets(from) {
        let m = model.clone();
        list = list.item(MenuItem::new(side_name(target)).on_activate_fn(move |_| {
            let at = m.side_append_index(target);
            m.move_tab(tab_id, target, at);
            m.set_side_visible(target, true);
            m.select_tab_by_id(target, tab_id);
        }));
    }
    list
}

/// Per-dock "Move to side" submenu: relocate a single dock to an *enabled* other
/// side as its own new activity.
fn dock_move_to_submenu(model: &DockingModel, dock_id: DockWidgetId, from: DockSide) -> MenuList {
    let mut list = MenuList::new();
    for target in model.enabled_move_targets(from) {
        let m = model.clone();
        list = list.item(MenuItem::new(side_name(target)).on_activate_fn(move |_| {
            m.move_dock(dock_id, DockOpenLocation::side(target).new_tab());
            m.set_side_visible(target, true);
        }));
    }
    list
}

/// Activity-bar size radio submenu, bound straight to the side's reactive
/// rail-size selector (the rail rebuilds when it flips).
fn rail_size_submenu(model: &DockingModel, side: DockSide) -> MenuList {
    let sig = model.rail_size_signal(side);
    MenuList::new()
        .item(MenuItem::new(lit!("Default")).radio(0, sig.clone()))
        .item(MenuItem::new(lit!("Compact")).radio(1, sig.clone()))
        .item(MenuItem::new(lit!("Icon + Label")).radio(2, sig))
}

/// Tab-display radio submenu, bound straight to the side's reactive tab-display
/// selector (the strip rebuilds when it flips).
fn tab_size_submenu(model: &DockingModel, side: DockSide) -> MenuList {
    let sig = model.tab_display_signal(side);
    MenuList::new()
        .item(MenuItem::new(lit!("Text")).radio(0, sig.clone()))
        .item(MenuItem::new(lit!("Icon")).radio(1, sig.clone()))
        .item(MenuItem::new(lit!("Icon + Text")).radio(2, sig))
}

#[cfg(test)]
mod tests {
    use super::super::geometry::DockSide;
    use super::super::model::{
        DockOpenLocation, DockPolicy, DockWidgetId, DockWidgetMeta, DockingModel,
    };
    use super::dock_has_options;
    use teksilo_i18n::lit;

    fn open(m: &DockingModel) {
        let id = DockWidgetId::fresh();
        m.register_meta(
            id,
            DockWidgetMeta {
                title: lit!("Dock"),
                icon: None,
                min_size: None,
                default: DockOpenLocation::side(DockSide::Leading),
                header_actions: None,
                show_header: false,
            },
        );
        m.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    }

    #[test]
    fn dock_has_options_under_default_policy() {
        // Multi-pane → "Move to new activity"; sole-pane → "Hide" / "Move to".
        let m = DockingModel::new();
        open(&m);
        assert!(dock_has_options(&m, DockSide::Leading, true));
        assert!(dock_has_options(&m, DockSide::Leading, false));
    }

    #[test]
    fn locked_dock_has_no_options() {
        // A fully-locked dock would open an empty menu — so the `⋮` button is
        // omitted.
        let m = DockingModel::new();
        open(&m);
        m.set_policy(DockPolicy::locked());
        assert!(!dock_has_options(&m, DockSide::Leading, true));
        assert!(!dock_has_options(&m, DockSide::Leading, false));
    }
}
