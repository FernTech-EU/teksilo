// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What `Home` / `End` / `PageUp` / `PageDown` mean in a data view, and the
//! handful of item-view chords that genuinely are platform-specific.
//!
//! The five data views — `ListView`, `TreeView`, `TableView`, `TreeTableView`,
//! `GridView` — each own their arrow keys, because an arrow means something
//! different in each (±1 row, ±one grid row, one cell across). What they must
//! *not* each invent is the edge-and-page family: those keys behave the same
//! everywhere the cursor topology is the same, and three separate hand-rolled
//! answers is how they drifted apart.
//!
//! ## Why navigation carries no platform branch
//!
//! Unlike [`text_nav`](super::text_nav), where macOS really does lay the
//! motions out differently, every surveyed cross-platform toolkit binds
//! `Home` / `End` / `PageUp` / `PageDown` **identically on all three
//! platforms**: GTK4's list key-binding block has exactly one `#ifdef
//! __APPLE__` and it wraps *select-all*; Qt's item views switch on the raw key
//! with no platform guard at all; wxWidgets, Slint and VS Code do the same.
//! So [`nav_chord`] deliberately takes no convention — a parameter that
//! provably never changes the answer would only suggest a branch exists.
//!
//! AppKit itself does differ: `NSTableView` implements
//! `scrollToBeginningOfDocument:` and *not* `moveToBeginningOfDocument:`, so on
//! macOS `Home` scrolls and no key selects the first row, and `Shift+Home` is a
//! dead chord. Teksilo does not reproduce that, for the same reason no other
//! cross-platform toolkit does: it would ship a list in which no key reaches
//! the first row. The deviation is deliberate and documented in
//! `docs/data-view-keyboard.md`.
//!
//! What *is* platform-specific is a short list of chords that are dead on one
//! platform and meaningful on another — [`mac_alias`] — and those follow the
//! `_for`-twin pattern so both branches stay reachable from one host's test
//! run, exactly like [`text_nav`](super::text_nav) and
//! [`Modifiers::with_command_convention`](teksilo_core::event::Modifiers::with_command_convention).

use teksilo_core::event::{Key, Modifiers};

/// The desktop item-view convention a chord is read against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ListNavConvention {
    /// macOS, where a list carries a few chords the other desktops spend
    /// elsewhere: `⌘↓` opens, `⌘↑` ascends, `⌥→`/`⌥←` expand a whole subtree.
    Mac,
    /// Windows and Linux.
    Desktop,
}

impl ListNavConvention {
    /// The convention this build targets.
    pub(crate) const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::Desktop
    };
}

/// The cursor topology of the view asking — which is what decides whether
/// `Home` is scoped to a row or to the whole collection.
///
/// The discriminator is **not** the widget's name but whether `←` / `→` move a
/// persistent cell cursor. Every stack that has one makes `Home` row-scoped
/// (Windows data grids, Qt's `QTableView`, the ARIA grid pattern); every stack
/// that does not makes it collection-scoped (all lists, all trees, Qt's
/// `QTreeView`, the ARIA listbox and tree patterns).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ViewKind {
    /// One-dimensional: `ListView`, `TreeView`. No column cursor.
    Linear,
    /// A wrapped two-dimensional tile grid: `GridView`. Its rows are a reflow
    /// artifact — the same `Home` press lands somewhere else after the window
    /// is resized — so they are *not* a `Home` target. `GtkGridView` and Qt's
    /// `QListView` in icon mode both resolve `Home` to the absolute first item
    /// for this reason.
    TileGrid,
    /// A semantic grid with a column cursor: `TableView` / `TreeTableView` in a
    /// cell-selection mode, where a row really is a navigable unit.
    CellGrid,
}

/// Where a nav key sends the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NavMove {
    /// First item of the collection — in a tree, the first *visible* row.
    First,
    /// Last item of the collection — in a tree, the last *visible* row.
    Last,
    /// First cell of the row holding the cursor. Only produced for
    /// [`ViewKind::CellGrid`].
    RowFirst,
    /// Last cell of the row holding the cursor. Only produced for
    /// [`ViewKind::CellGrid`].
    RowLast,
    /// One viewport of content, in the direction pressed. The caller resolves
    /// the distance geometrically — from the row-offset table, never as
    /// `index ± page_size`, or variable row heights page to the wrong row.
    Page { down: bool },
}

/// What the same chord does to the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SelectionOp {
    /// Collapse the selection onto the cursor, and move the anchor with it.
    Replace,
    /// Leave the selection and the anchor exactly as they are — move only the
    /// cursor.
    ///
    /// This is what the accelerator means on a navigation key. GTK4 registers
    /// every nav key with a `(select, modify, extend)` triple whose `modify`
    /// variant skips the selection call outright; Qt's
    /// `extendedSelectionCommand` returns `NoUpdate` for any navigation key
    /// with Control held. It is the same rule the arrows already follow, which
    /// is why the arrows are the *general* case and not an exception.
    Suppress,
    /// Select the anchor-to-cursor range, replacing whatever fell outside it —
    /// so reversing a `Shift` gesture shrinks the range instead of growing it.
    Extend,
    /// Select the anchor-to-cursor range *unioned with* the selection as it
    /// stood when the gesture began, so a second range can be built without
    /// losing the first.
    ExtendAdditive,
}

/// A resolved navigation chord: where to go, and what that does to the
/// selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NavChord {
    pub(crate) movement: NavMove,
    pub(crate) selection: SelectionOp,
}

/// Which expand/collapse a non-arrow tree chord asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TreeChord {
    /// Expand the cursor row one level.
    ExpandOne,
    /// Collapse the cursor row.
    CollapseOne,
    /// Expand the cursor row and every descendant.
    ExpandSubtree,
    /// Collapse the cursor row and every descendant.
    CollapseSubtree,
}

/// A chord that means something on macOS and nothing on the other desktops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MacAlias {
    /// `⌘↓` — open the focused row, the Mac counterpart of `Enter`. Finder
    /// documents "Command–Down Arrow: Open the selected item", and VS Code
    /// binds it as `list.select`'s macOS secondary.
    Activate,
    /// `⌘↑` — collapse the focused row, or move to its parent. Finder's
    /// "open the folder that contains the current folder"; VS Code's
    /// `list.collapse` macOS secondary.
    CollapseOrParent,
    /// `⌥→` — expand the focused row and every descendant. AppKit's
    /// `expandItem:expandChildren:`, documented as the Option-click twin.
    ExpandSubtree,
    /// `⌥←` — collapse the focused row and every descendant.
    CollapseSubtree,
}

/// What `key` means for the edge-and-page family, with `modifiers` held, in a
/// view of this `kind` — or `None` if it is not one of those keys.
///
/// Takes no convention on purpose; see the module documentation.
pub(crate) fn nav_chord(key: Key, modifiers: Modifiers, kind: ViewKind) -> Option<NavChord> {
    let movement = match key {
        // In a cell grid the accelerator escalates the scope: bare Home is the
        // start of the row, ⌘/Ctrl+Home the first cell of the whole table —
        // Qt's `QTableView` and every Windows data grid agree. A linear or
        // tile view has no row to be scoped to, so both spellings mean the
        // collection's first item and the accelerator only suppresses the
        // selection.
        Key::Home => match kind {
            ViewKind::CellGrid if !modifiers.command() => NavMove::RowFirst,
            _ => NavMove::First,
        },
        Key::End => match kind {
            ViewKind::CellGrid if !modifiers.command() => NavMove::RowLast,
            _ => NavMove::Last,
        },
        Key::PageUp => NavMove::Page { down: false },
        Key::PageDown => NavMove::Page { down: true },
        _ => return None,
    };
    Some(NavChord {
        movement,
        selection: selection_op(modifiers),
    })
}

/// What `modifiers` ask a navigation key to do to the selection.
///
/// The accelerator suppresses; `Shift` extends; the two together extend
/// additively. Split out so the four cases are stated once rather than
/// re-derived at each of the five call sites.
pub(crate) fn selection_op(modifiers: Modifiers) -> SelectionOp {
    match (modifiers.command(), modifiers.shift()) {
        (true, true) => SelectionOp::ExtendAdditive,
        (true, false) => SelectionOp::Suppress,
        (false, true) => SelectionOp::Extend,
        (false, false) => SelectionOp::Replace,
    }
}

/// The non-arrow tree chords: `*` expands a whole subtree, `+` expands one
/// level, `-` collapses.
///
/// Windows, Qt's `QTreeView` (`expandRecursively`) and GTK3's `GtkTreeView`
/// agree on all three. The ARIA tree pattern reads `*` differently — "expands
/// all siblings at the same level" — so this is the desktop meaning and the
/// widgets must not claim APG conformance for it.
///
/// The keys are matched by character, without distinguishing the numeric
/// keypad: winit reports both locations as the same logical character, and
/// Microsoft's own interaction guidelines say not to bind keypad-only keys
/// anyway.
///
/// Returns `None` when `Ctrl` / `Alt` / `Super` is held, so `Ctrl+-` and
/// friends stay available to the application. `Shift` is **not** rejected:
/// on every Latin layout `*` and `+` are shifted keys (`Shift+8` and
/// `Shift+=` on a US board, `Shift++`/`Shift+=` on a German one), and winit
/// reports the shifted logical character with `SHIFT` still set in the
/// modifier state. Rejecting it would leave both expand chords reachable
/// only from a numeric keypad, while `-` — unshifted on the same boards —
/// kept working, so the feature would collapse but never open.
///
/// Callers must consult this **before** their type-ahead arm —
/// `Key::to_char` answers `Some('*')`, so type-ahead would otherwise swallow
/// the chord.
pub(crate) fn tree_chord(key: Key, modifiers: Modifiers) -> Option<TreeChord> {
    if modifiers.ctrl() || modifiers.alt() || modifiers.super_key() {
        return None;
    }
    match key {
        Key::Character('*') => Some(TreeChord::ExpandSubtree),
        Key::Character('+') => Some(TreeChord::ExpandOne),
        Key::Character('-') => Some(TreeChord::CollapseOne),
        _ => None,
    }
}

/// The macOS-only item-view chords, on this platform.
///
/// `rtl` mirrors the horizontal pair, the way the tree's own expand/collapse
/// arrows already are.
pub(crate) fn mac_alias(key: Key, modifiers: Modifiers, rtl: bool) -> Option<MacAlias> {
    mac_alias_for(ListNavConvention::CURRENT, key, modifiers, rtl)
}

/// [`mac_alias`] against an explicit convention.
///
/// All four chords are dead in Teksilo today, and all four are claimed by the
/// platform on macOS, so binding them adds reach without taking anything away.
/// None of them may be bound off macOS: `Alt+Right` is history-forward on
/// Windows, and `Ctrl+↑`/`Ctrl+↓` already mean cursor-only movement there.
pub(crate) fn mac_alias_for(
    convention: ListNavConvention,
    key: Key,
    modifiers: Modifiers,
    rtl: bool,
) -> Option<MacAlias> {
    if convention != ListNavConvention::Mac {
        return None;
    }
    // Shift is left alone: ⇧⌘↓ is not one of these chords, and swallowing it
    // here would shadow whatever the view does with a shifted arrow.
    if modifiers.shift() || modifiers.ctrl() {
        return None;
    }
    let expand_key = if rtl { Key::ArrowLeft } else { Key::ArrowRight };
    let collapse_key = if rtl { Key::ArrowRight } else { Key::ArrowLeft };
    match key {
        Key::ArrowDown if modifiers.super_key() && !modifiers.alt() => Some(MacAlias::Activate),
        Key::ArrowUp if modifiers.super_key() && !modifiers.alt() => {
            Some(MacAlias::CollapseOrParent)
        }
        k if k == expand_key && modifiers.alt() && !modifiers.super_key() => {
            Some(MacAlias::ExpandSubtree)
        }
        k if k == collapse_key && modifiers.alt() && !modifiers.super_key() => {
            Some(MacAlias::CollapseSubtree)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::ListNavConvention::{Desktop, Mac};
    use super::ViewKind::{CellGrid, Linear, TileGrid};
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const ALT: Modifiers = Modifiers::ALT;
    const SUPER: Modifiers = Modifiers::SUPER;
    const CTRL: Modifiers = Modifiers::CTRL;
    /// The platform accelerator — ⌘ on macOS, Ctrl elsewhere. Written this way
    /// so the assertions below hold on every host.
    const CMD: Modifiers = Modifiers::COMMAND;

    fn movement(key: Key, mods: Modifiers, kind: ViewKind) -> NavMove {
        nav_chord(key, mods, kind).expect("a nav key").movement
    }

    fn selection(key: Key, mods: Modifiers) -> SelectionOp {
        nav_chord(key, mods, Linear).expect("a nav key").selection
    }

    #[test]
    fn a_linear_view_sends_home_and_end_to_the_ends_of_the_collection() {
        assert_eq!(movement(Key::Home, NONE, Linear), NavMove::First);
        assert_eq!(movement(Key::End, NONE, Linear), NavMove::Last);
        // The accelerator has nowhere further to escalate to here, so it
        // changes only what happens to the selection.
        assert_eq!(movement(Key::Home, CMD, Linear), NavMove::First);
        assert_eq!(movement(Key::End, CMD, Linear), NavMove::Last);
    }

    #[test]
    fn a_tile_grid_ignores_its_reflow_rows() {
        // A wrapped grid's rows change with the window width, so "first tile in
        // this row" is not a target a user can form a model of. GtkGridView and
        // QListView in icon mode both resolve Home to the absolute first item.
        for mods in [NONE, CMD, SHIFT] {
            assert_eq!(movement(Key::Home, mods, TileGrid), NavMove::First);
            assert_eq!(movement(Key::End, mods, TileGrid), NavMove::Last);
        }
    }

    #[test]
    fn a_cell_grid_scopes_home_to_the_row_until_the_accelerator_escalates_it() {
        assert_eq!(movement(Key::Home, NONE, CellGrid), NavMove::RowFirst);
        assert_eq!(movement(Key::End, NONE, CellGrid), NavMove::RowLast);
        assert_eq!(movement(Key::Home, CMD, CellGrid), NavMove::First);
        assert_eq!(movement(Key::End, CMD, CellGrid), NavMove::Last);
        // Shift alone extends within the row; it does not escalate the scope.
        assert_eq!(movement(Key::Home, SHIFT, CellGrid), NavMove::RowFirst);
        assert_eq!(movement(Key::Home, CMD | SHIFT, CellGrid), NavMove::First);
    }

    #[test]
    fn paging_is_the_same_move_in_every_kind_of_view() {
        for kind in [Linear, TileGrid, CellGrid] {
            assert_eq!(
                movement(Key::PageUp, NONE, kind),
                NavMove::Page { down: false }
            );
            assert_eq!(
                movement(Key::PageDown, NONE, kind),
                NavMove::Page { down: true }
            );
        }
    }

    #[test]
    fn the_accelerator_moves_the_cursor_without_touching_the_selection() {
        for key in [Key::Home, Key::End, Key::PageUp, Key::PageDown] {
            assert_eq!(selection(key, NONE), SelectionOp::Replace);
            assert_eq!(selection(key, CMD), SelectionOp::Suppress);
            assert_eq!(selection(key, SHIFT), SelectionOp::Extend);
            assert_eq!(selection(key, CMD | SHIFT), SelectionOp::ExtendAdditive);
        }
    }

    #[test]
    fn keys_outside_the_edge_and_page_family_are_not_ours() {
        // The arrows mean something different in each view — ±1 row, ±one grid
        // row, one cell across — so each view keeps them.
        for key in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::Space,
            Key::Enter,
            Key::Escape,
            Key::Tab,
            Key::A,
        ] {
            assert!(nav_chord(key, NONE, Linear).is_none());
        }
    }

    #[test]
    fn tree_chords_take_the_desktop_reading_of_asterisk() {
        // Windows, QTreeView::expandRecursively and GtkTreeView all expand the
        // whole subtree. The ARIA tree pattern means "all siblings at this
        // level" by the same key; we ship the desktop meaning.
        assert_eq!(
            tree_chord(Key::Character('*'), NONE),
            Some(TreeChord::ExpandSubtree)
        );
        assert_eq!(
            tree_chord(Key::Character('+'), NONE),
            Some(TreeChord::ExpandOne)
        );
        assert_eq!(
            tree_chord(Key::Character('-'), NONE),
            Some(TreeChord::CollapseOne)
        );
    }

    #[test]
    fn a_modified_tree_chord_belongs_to_the_application() {
        for mods in [CTRL, ALT, SUPER] {
            for ch in ['*', '+', '-'] {
                assert_eq!(tree_chord(Key::Character(ch), mods), None);
            }
        }
    }

    #[test]
    fn a_shifted_tree_chord_is_still_the_chord() {
        // `*` is Shift+8 and `+` is Shift+= on a US board, and winit reports
        // the shifted logical character with SHIFT still set. Rejecting Shift
        // left both expand chords reachable only from a numeric keypad while
        // `-`, unshifted, kept working — a tree that collapsed but never
        // opened.
        assert_eq!(
            tree_chord(Key::Character('*'), SHIFT),
            Some(TreeChord::ExpandSubtree)
        );
        assert_eq!(
            tree_chord(Key::Character('+'), SHIFT),
            Some(TreeChord::ExpandOne)
        );
        assert_eq!(
            tree_chord(Key::Character('-'), SHIFT),
            Some(TreeChord::CollapseOne)
        );
    }

    #[test]
    fn tree_chords_must_be_read_before_type_ahead() {
        // `Key::to_char` answers for these, so a view that runs its type-ahead
        // arm first would swallow the chord and search for "*" instead.
        for ch in ['*', '+', '-'] {
            assert!(Key::Character(ch).to_char().is_some());
            assert!(tree_chord(Key::Character(ch), NONE).is_some());
        }
    }

    #[test]
    fn the_mac_aliases_exist_only_on_mac() {
        for key in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
        ] {
            for mods in [SUPER, ALT, SUPER | ALT] {
                assert_eq!(
                    mac_alias_for(Desktop, key, mods, false),
                    None,
                    "Alt+Right is history-forward on Windows and Ctrl+arrow is \
                     already cursor-only movement; neither may be reused"
                );
            }
        }
    }

    #[test]
    fn mac_reads_command_arrows_the_way_finder_does() {
        // ⌘↓ opens and ⌘↑ ascends — NOT first/last, which is the *text*
        // idiom. Binding them to the ends of the collection would collide with
        // the platform rather than conform to it.
        assert_eq!(
            mac_alias_for(Mac, Key::ArrowDown, SUPER, false),
            Some(MacAlias::Activate)
        );
        assert_eq!(
            mac_alias_for(Mac, Key::ArrowUp, SUPER, false),
            Some(MacAlias::CollapseOrParent)
        );
    }

    #[test]
    fn mac_option_arrows_expand_a_whole_subtree_and_mirror_under_rtl() {
        assert_eq!(
            mac_alias_for(Mac, Key::ArrowRight, ALT, false),
            Some(MacAlias::ExpandSubtree)
        );
        assert_eq!(
            mac_alias_for(Mac, Key::ArrowLeft, ALT, false),
            Some(MacAlias::CollapseSubtree)
        );
        assert_eq!(
            mac_alias_for(Mac, Key::ArrowLeft, ALT, true),
            Some(MacAlias::ExpandSubtree)
        );
        assert_eq!(
            mac_alias_for(Mac, Key::ArrowRight, ALT, true),
            Some(MacAlias::CollapseSubtree)
        );
    }

    #[test]
    fn a_shifted_mac_alias_falls_through_to_the_view() {
        // ⇧⌘↓ is not one of these chords; claiming it would shadow whatever the
        // view does with a shifted arrow.
        for key in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
        ] {
            assert_eq!(mac_alias_for(Mac, key, SUPER | SHIFT, false), None);
            assert_eq!(mac_alias_for(Mac, key, ALT | SHIFT, false), None);
        }
    }

    #[test]
    fn physical_control_is_not_a_mac_item_view_chord() {
        // ⌃↑ belongs to Mission Control; the cursor-only pair stays on literal
        // Control precisely because it has no ⌘ counterpart to move to.
        for key in [Key::ArrowUp, Key::ArrowDown] {
            assert_eq!(mac_alias_for(Mac, key, CTRL, false), None);
        }
    }

    #[test]
    fn the_current_convention_matches_the_build_target() {
        if cfg!(target_os = "macos") {
            assert_eq!(ListNavConvention::CURRENT, Mac);
            assert_eq!(
                mac_alias(Key::ArrowDown, SUPER, false),
                Some(MacAlias::Activate)
            );
        } else {
            assert_eq!(ListNavConvention::CURRENT, Desktop);
            assert_eq!(mac_alias(Key::ArrowDown, SUPER, false), None);
        }
    }
}
