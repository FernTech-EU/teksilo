//! Shared context menus for dock activities (rail items + tab-strip tabs) and
//! the activity-bar background.
//!
//! The per-activity menu (right-click a rail item or a dock tab) is:
//!
//! ```text
//! Hide "<activity>"
//! ──────────────
//! Move to              ▸  <other sides>
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

use bastyde_i18n::{LocalizedString, lit};

use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;

use super::geometry::DockSide;
use super::model::{DockTabId, DockTabView, DockingModel};

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

/// The display label for one tab (explicit title → active dock title → "Panel").
fn tab_label(model: &DockingModel, view: &DockTabView) -> LocalizedString {
    view.title
        .clone()
        .or_else(|| view.panes.first().and_then(|d| model.dock_title(*d)))
        .unwrap_or_else(|| lit!("Panel"))
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
        .map(|v| tab_label(model, v))
        .unwrap_or_else(|| lit!("Panel"));

    let mut list = MenuList::new();

    // Hide "<activity>".
    {
        let m = model.clone();
        let hide_label = lit!(format!("Hide \"{}\"", this_label.resolve_now()));
        list = list.item(
            MenuItem::new(hide_label).on_activate_fn(move |_| m.set_tab_hidden(tab_id, true)),
        );
    }

    list = list.separator();

    // Move to ▸ <other sides>.
    {
        let m = model.clone();
        list = list.item(MenuItem::submenu(lit!("Move to"), move || {
            Box::new(move_to_submenu(&m, side, tab_id))
        }));
    }

    list = list.separator();

    // Checkable list of this side's activities.
    list = activity_checkitems(list, model, side, &tabs);

    list = list.separator();

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

/// The background menu (right-click empty rail / side chrome): the checkable
/// activities list + the kind-appropriate size submenu. This is the affordance
/// to restore an activity after every item has been hidden.
pub(crate) fn background_menu(model: &DockingModel, side: DockSide, kind: DockMenuKind) -> MenuList {
    let tabs = model.side_tabs(side);
    let mut list = activity_checkitems(MenuList::new(), model, side, &tabs);
    list = list.separator();
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

/// Append one checkable row per activity (checked = visible). Toggling drives
/// `set_tab_hidden`.
fn activity_checkitems(
    mut list: MenuList,
    model: &DockingModel,
    _side: DockSide,
    tabs: &[DockTabView],
) -> MenuList {
    use bastyde_core::signal::Signal;
    for view in tabs {
        let tab_id = view.id;
        let hidden = view.hidden;
        let label = tab_label(model, view);
        let m = model.clone();
        list = list.item(
            MenuItem::new(label)
                // Checkmark reflects current visibility; the flip is cosmetic
                // (the menu dismisses on click) — the model drive is below.
                .bind_checked(Signal::new(!hidden))
                .on_activate_fn(move |_| m.set_tab_hidden(tab_id, !hidden)),
        );
    }
    list
}

/// "Move to" submenu: one row per *other* side; relocates the whole tab there,
/// shows that side, and selects the moved tab.
fn move_to_submenu(model: &DockingModel, from: DockSide, tab_id: DockTabId) -> MenuList {
    let mut list = MenuList::new();
    for target in DockSide::ALL {
        if target == from {
            continue;
        }
        let m = model.clone();
        list = list.item(MenuItem::new(side_name(target)).on_activate_fn(move |_| {
            let at = m.tab_count(target);
            m.move_tab(tab_id, target, at);
            m.set_side_visible(target, true);
            m.select_tab_by_id(target, tab_id);
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
