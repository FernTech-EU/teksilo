// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared keyboard handler for `TableView` and `TreeTableView`.
//!
//! The handler is generic over `RowNavigator` so flat and tree
//! navigation reuse the same key matrix. Tree-specific arrow-left /
//! arrow-right collapse/expand semantics fall through automatically
//! because the trait's default `is_expanded` / `has_children` /
//! `toggle_expanded` methods are no-ops on a flat table.

use std::rc::Rc;
use std::time::Duration;

use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;
use teksilo_data::SelectionMode;

use super::PaneBoundaries;
use super::body::SharedColumnWidths;
use super::column::{EditTriggers, TabTraversal};
use super::row_navigator::RowNavigator;
use super::selection::{CellSelectionModel, TableSelectionMode};
use crate::common::list_nav;
use crate::common::row_metrics::SharedRowMetrics;
use crate::common::type_ahead::TypeAheadState;
use crate::data_views::RowSelection;

/// Configuration captured from the table at build time and threaded
/// into the on_key handler. Cheap to clone (signals + Rcs).
#[derive(Clone)]
pub(crate) struct KeyHandlerConfig {
    pub navigator: Rc<dyn RowNavigator>,
    pub col_count: usize,
    /// Display position of the tree column — the one hosting the twist and
    /// indent gutter, and therefore the only column where ArrowLeft/ArrowRight
    /// collapse/expand instead of moving the cursor.
    ///
    /// Resolved per rebuild by the owning widget, because
    /// [`TreeTableView::tree_column`](crate::TreeTableView::tree_column) names
    /// a column *id* while user drag-reorder moves its *display* position —
    /// the two diverge the moment either is used. `TableView` passes `0`: its `FlatNavigator` reports
    /// `has_children`/`is_expanded` as false and `toggle_expanded` as a no-op,
    /// so the comparison can never lead anywhere.
    pub tree_column_display_pos: usize,
    pub focused_cell: Signal<Option<(usize, usize)>>,
    /// `(row, col)` → realized cell id. `Space` asks the focused cell whether
    /// it publishes a keyboard toggle before falling back to the selection —
    /// scoped to the *cell*, not the row, because a table can carry more than
    /// one checkbox column and "the row's checkbox" would be arbitrary.
    #[allow(clippy::type_complexity)]
    pub cell_map: Rc<std::cell::RefCell<Vec<((usize, usize), teksilo_core::widget_id::WidgetId)>>>,
    pub selection_mode: TableSelectionMode,
    pub selection: Option<RowSelection>,
    pub cell_selection: Option<CellSelectionModel>,
    pub scroll_y: Signal<f32>,
    pub max_scroll_y: Signal<f32>,
    pub viewport_height: Rc<std::cell::Cell<f32>>,
    /// The row-area's absolute (window) rect: row 0's top sits at
    /// `body_bounds.y` when `scroll_y == 0`. Read to chase the keyboard-focused
    /// row into any *enclosing* scroll area via
    /// [`EventContext::ensure_visible`](teksilo_core::widget::EventContext::ensure_visible)
    /// — the table's own viewport follow is handled by `scroll_y`. Rows are not
    /// distinct focusable nodes, so the framework's focus-driven follow never
    /// reveals the selected row in an outer scroller. Populated by each widget's
    /// `place_children`.
    pub body_bounds: Rc<std::cell::Cell<teksilo_canvas::Rect>>,
    /// Row geometry (uniform / exact / auto-measure) — drives the
    /// PageUp/PageDown focus-row math.
    pub row_metrics: SharedRowMetrics,
    pub tab_traversal: TabTraversal,
    pub editing_cell: Signal<Option<(usize, usize)>>,
    /// `(row, col_id)` → invoke user's edit hook. The closure resolves
    /// `col_id` from a display position; we keep it generic over
    /// `&str` so the keyboard module doesn't need a `Column<T>`
    /// reference.
    pub display_col_to_id: Rc<dyn Fn(usize) -> Option<String>>,
    /// The [`EditTriggers`] in force for the column at a display position —
    /// the view's set, overridden by the column's own, and `NONE` for a
    /// non-editable column. Per column rather than one set for the table
    /// because that is what the caller declares: entering edit mode on a
    /// column whose delegate has no editor would only confuse the focus /
    /// dispatch state, and a tree column usually wants different gestures from
    /// the value columns beside it.
    pub display_col_triggers: Rc<dyn Fn(usize) -> EditTriggers>,
    /// Optional: user callback fired when an edit trigger matches.
    #[allow(clippy::type_complexity)]
    pub on_cell_edit_request:
        Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    /// Optional: row-activate (Enter) callback.
    #[allow(clippy::type_complexity)]
    pub on_row_activate: Option<Rc<dyn Fn(usize, &mut teksilo_core::widget::EventContext)>>,
    /// Persistent type-ahead buffer (survives the per-keystroke rebuild).
    pub type_ahead: Rc<TypeAheadState>,
    /// Type-ahead label resolver: `row -> Some(text)` for a resident row.
    /// `None` (the option) disables type-ahead.
    #[allow(clippy::type_complexity)]
    pub type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>>,
    /// Reset window for the type-ahead search term.
    pub type_ahead_timeout: Duration,

    /// Resolved column widths in display order — shared with the row/header
    /// layout. Read to compute a display column's horizontal extent for
    /// ensure-column-visible.
    pub column_widths: SharedColumnWidths,
    /// Pane partition (Leading/Middle/Trailing) — pinned columns never
    /// trigger horizontal scrolling, since they're always visible by
    /// definition. Snapshotted at build like `col_count` (a pinning/order
    /// change rebuilds the whole table anyway).
    pub pane_boundaries: PaneBoundaries,
    pub scroll_x: Signal<f32>,
    pub max_scroll_x: Signal<f32>,
    /// Middle-pane viewport width, populated by `place_children` — the
    /// horizontal analogue of `viewport_height`.
    pub middle_viewport_width: Rc<std::cell::Cell<f32>>,
}

/// Build the on_key closure. Captures config by value; the closure is
/// `'static` and ready to slot into a `HandlerSet`.
pub(crate) fn build_key_handler(
    cfg: KeyHandlerConfig,
) -> impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static {
    move |event, ctx: &mut EventContext| {
        let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
            return EventResponse::Ignored;
        };

        let row_count = cfg.navigator.row_count();
        if row_count == 0 || cfg.col_count == 0 {
            return EventResponse::Ignored;
        }
        // The keyboard cursor: the focused cell once the user has navigated or
        // clicked, else the selected row (a table can be handed a selection
        // before it is ever focused — a restored last position, a preselected
        // entry). `None` means "no cursor yet", which is deliberately NOT the
        // same as `Some((0, 0))`: the directional keys below land ON the near
        // end cell rather than stepping past it. Collapsing the two is what made
        // the first ArrowDown skip row 0, the first ArrowUp a dead key
        // (`prev_row(0)` is `None`), and the first ArrowRight skip column 0.
        let raw = cfg.focused_cell.get().or_else(|| {
            cfg.selection
                .as_ref()
                .and_then(|s| s.selected_indices().first().copied())
                .map(|r| (r, 0))
        });
        let cursor = raw.map(|(r, c)| (r.min(row_count - 1), c.min(cfg.col_count - 1)));
        // Anchor for the keys that compute *from* a cell (expand / collapse,
        // paging, Home/End, activation, editing) rather than step in a
        // direction.
        let (row, col) = cursor.unwrap_or((0, 0));
        // Persist the clamp: if the stored focus was out of range (e.g. rows or
        // columns were removed since it was set), write the in-bounds cell back
        // so the focus ring and any later reader don't keep the stale position.
        if raw.is_some() && raw != Some((row, col)) {
            cfg.focused_cell.set(Some((row, col)));
        }

        // Read layout direction live from the dispatch context (a runtime
        // locale switch dirties the tree but does not rebuild, so a
        // build-time capture would go stale).
        let rtl = ctx.is_rtl();

        // Tree-aware collapse / expand (flat impls are no-ops, so this is
        // safe to evaluate eagerly). The keys follow the visual chevron:
        // under LTR the collapsed chevron points right (ArrowRight
        // expands, ArrowLeft collapses); under RTL it points left, so the
        // two arrows swap.
        let on_tree_column = col == cfg.tree_column_display_pos;
        let is_collapse_key = if rtl {
            matches!(key, Key::ArrowRight)
        } else {
            matches!(key, Key::ArrowLeft)
        };
        let is_expand_key = if rtl {
            matches!(key, Key::ArrowLeft)
        } else {
            matches!(key, Key::ArrowRight)
        };
        // Only the unmodified arrows expand: with Shift the key belongs to the
        // range extension, and with the accelerator to the cursor-only move.
        // ⌘ is excluded too, or a macOS ⌘←/⌘→ — which the platform spends on
        // history, never on an outline — would silently open and close rows.
        let plain_arrow =
            !modifiers.shift() && !modifiers.ctrl() && !modifiers.alt() && !modifiers.super_key();
        if plain_arrow && is_collapse_key && on_tree_column {
            if cfg.navigator.is_expanded(row) {
                cfg.navigator.toggle_expanded(row);
                return EventResponse::Handled;
            }
            // Nothing to collapse: ascend. The ARIA tree pattern, Explorer,
            // Nautilus and Dolphin all read Left this way, and `TreeView` has
            // done so since it shipped — this is the half `TreeTableView` was
            // missing, which left Left a dead key on every leaf.
            if let Some(parent) = cfg.navigator.parent_row(row) {
                cfg.focused_cell.set(Some((parent, col)));
                apply_selection_extension(&cfg, parent, col, false);
                ensure_row_visible(&cfg, parent, row_count, ctx);
                return EventResponse::Handled;
            }
        }
        if plain_arrow
            && is_expand_key
            && on_tree_column
            && cfg.navigator.has_children(row)
            && !cfg.navigator.is_expanded(row)
        {
            cfg.navigator.toggle_expanded(row);
            return EventResponse::Handled;
        }

        // `*` expands the whole subtree, `+` / `-` one level — read before
        // type-ahead, since `Key::to_char` answers for all three.
        //
        // Gated on the navigator actually being hierarchical (the same
        // discriminator `corner_column` uses). A flat `TableView` can never
        // act on these, and claiming them there returned `Ignored` *before*
        // the type-to-edit and type-ahead arms below — so on a US board,
        // where `-` is unshifted, typing a minus to start a negative number
        // in an `EditTriggers::ANY_KEY` column stopped opening the editor.
        if is_hierarchical(&cfg)
            && let Some(chord) = list_nav::tree_chord(*key, *modifiers)
        {
            let handled = with_navigator_subtree(&cfg, |ops| match chord {
                list_nav::TreeChord::ExpandSubtree => {
                    crate::common::tree_expand::expand_subtree(ops, row)
                }
                list_nav::TreeChord::CollapseSubtree => {
                    crate::common::tree_expand::collapse_subtree(ops, row)
                }
                list_nav::TreeChord::ExpandOne => {
                    if cfg.navigator.has_children(row) && !cfg.navigator.is_expanded(row) {
                        cfg.navigator.toggle_expanded(row);
                        true
                    } else {
                        false
                    }
                }
                list_nav::TreeChord::CollapseOne => {
                    if cfg.navigator.is_expanded(row) {
                        cfg.navigator.toggle_expanded(row);
                        true
                    } else {
                        false
                    }
                }
            });
            return if handled {
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            };
        }

        // macOS reads ⌘↓ as open, ⌘↑ as collapse-or-ascend and ⌥→/⌥← as a
        // recursive expand in an outline. All four are dead here otherwise.
        if let Some(alias) = list_nav::mac_alias(*key, *modifiers, rtl) {
            let handled = match alias {
                list_nav::MacAlias::Activate => {
                    if let Some(ref f) = cfg.on_row_activate {
                        f(row, ctx);
                        true
                    } else {
                        false
                    }
                }
                list_nav::MacAlias::CollapseOrParent => {
                    if cfg.navigator.is_expanded(row) {
                        cfg.navigator.toggle_expanded(row);
                        true
                    } else if let Some(parent) = cfg.navigator.parent_row(row) {
                        cfg.focused_cell.set(Some((parent, col)));
                        apply_selection_extension(&cfg, parent, col, false);
                        ensure_row_visible(&cfg, parent, row_count, ctx);
                        true
                    } else {
                        false
                    }
                }
                list_nav::MacAlias::ExpandSubtree => with_navigator_subtree(&cfg, |ops| {
                    crate::common::tree_expand::expand_subtree(ops, row)
                }),
                list_nav::MacAlias::CollapseSubtree => with_navigator_subtree(&cfg, |ops| {
                    crate::common::tree_expand::collapse_subtree(ops, row)
                }),
            };
            return if handled {
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            };
        }

        // While a cell editor is open the container must stop claiming the
        // keys the editor needs. Rows are not focusable nodes, so a key the
        // editor ignores bubbles straight to this handler: a single-line
        // editor does nothing with PageDown, and the table would page the
        // cursor out from under the edit in progress. Escape and the commit /
        // advance keys stay here, since they are about the *edit*, not the
        // text. The ARIA grid pattern states the rule outright — everything
        // else "passes the key event to the focused widget".
        if cfg.editing_cell.get().is_some()
            && !matches!(key, Key::Escape | Key::Enter | Key::Tab | Key::F2)
        {
            return EventResponse::Ignored;
        }

        let viewport_h = cfg.viewport_height.get();
        // A table whose cells are the navigable unit reads Home as the start
        // of the row; one that selects whole rows has no cell cursor for that
        // to mean anything against, so its Home is the first row — which is
        // also what Explorer's details view and every list control do.
        let view_kind = if cfg.selection_mode.is_cell_mode() {
            list_nav::ViewKind::CellGrid
        } else {
            list_nav::ViewKind::Linear
        };
        let nav = list_nav::nav_chord(*key, *modifiers, view_kind);

        // Each directional key, with NO cursor yet, lands ON the end cell it
        // would have entered from — it does not step past it (see `cursor`
        // above, and the same rule in `ListView` / `TreeView` / `GridView`).
        // `first_row` / `last_row` (not raw 0 / row_count-1) so a hierarchical
        // navigator — `TreeTableView` plugs its own in here — enters at a row
        // that is actually visible.
        let new_pos: Option<(usize, usize)> = match key {
            Key::ArrowUp => match cursor {
                None => cfg.navigator.last_row().map(|r| (r, col)),
                Some(_) => cfg.navigator.prev_row(row).map(|r| (r, col)),
            },
            Key::ArrowDown => match cursor {
                None => cfg.navigator.first_row().map(|r| (r, col)),
                Some(_) => cfg.navigator.next_row(row).map(|r| (r, col)),
            },
            // Visual-left moves to a higher display index under RTL
            // (columns run right-to-left), so the two arrows swap their
            // index delta. The clamps stay tied to the physical edge each
            // arrow points at. Column 0 is the leading column in both
            // directions, so a cursor-less entry lands on the column the key
            // points *away* from: the "next" key on the first column, the
            // "previous" key on the last.
            Key::ArrowLeft => {
                if rtl {
                    match cursor {
                        None => Some((row, 0)),
                        Some(_) => (col + 1 < cfg.col_count).then_some((row, col + 1)),
                    }
                } else {
                    match cursor {
                        None => Some((row, cfg.col_count - 1)),
                        // `.then` (lazy) — `col - 1` must not be evaluated at col 0.
                        Some(_) => (col > 0).then(|| (row, col - 1)),
                    }
                }
            }
            Key::ArrowRight => {
                if rtl {
                    match cursor {
                        None => Some((row, cfg.col_count - 1)),
                        Some(_) => (col > 0).then(|| (row, col - 1)),
                    }
                } else {
                    match cursor {
                        None => Some((row, 0)),
                        Some(_) => (col + 1 < cfg.col_count).then_some((row, col + 1)),
                    }
                }
            }
            // The edge-and-page family, resolved once in `common::list_nav`.
            // `first_row` / `last_row` rather than raw 0 / count-1 so a
            // hierarchical navigator lands on a row that is actually visible.
            Key::Home | Key::End | Key::PageUp | Key::PageDown => {
                let Some(chord) = nav else {
                    return EventResponse::Ignored;
                };
                match chord.movement {
                    list_nav::NavMove::RowFirst => Some((row, 0)),
                    list_nav::NavMove::RowLast => Some((row, cfg.col_count - 1)),
                    // Where the accelerator lands depends on which ARIA
                    // pattern this table *is*, and the two disagree on
                    // purpose. A flat cell grid escalates to the corner —
                    // "moves focus to the first cell in the first row" (the
                    // grid pattern, and Excel and Qt's `QTableView`). A
                    // treegrid keeps the column: "moves focus to the cell in
                    // the first row in the same column as the cell that had
                    // focus". A row-selection table has no column cursor to
                    // move at all, so the column simply stays put.
                    list_nav::NavMove::First => cfg
                        .navigator
                        .first_row()
                        .map(|r| (r, corner_column(&cfg, view_kind, col, false))),
                    list_nav::NavMove::Last => cfg
                        .navigator
                        .last_row()
                        .map(|r| (r, corner_column(&cfg, view_kind, col, true))),
                    // Scroll one page, then move focus to the row one viewport
                    // away from the current row's top — offset-table-driven,
                    // so variable heights page by visual distance rather than
                    // by a fixed row count. Guarantees progress even when a
                    // single row is taller than the viewport.
                    list_nav::NavMove::Page { down } => {
                        let new_y = if down {
                            (cfg.scroll_y.get() + viewport_h).min(cfg.max_scroll_y.get())
                        } else {
                            (cfg.scroll_y.get() - viewport_h).max(0.0)
                        };
                        cfg.scroll_y.set(new_y);
                        let r = {
                            let mut m = cfg.row_metrics.borrow_mut();
                            m.resize(row_count);
                            let target_y = if down {
                                m.row_top(row) + viewport_h
                            } else {
                                (m.row_top(row) - viewport_h).max(0.0)
                            };
                            m.row_at(target_y)
                        };
                        let r = if r == row && down {
                            (row + 1).min(row_count - 1)
                        } else if r == row {
                            row.saturating_sub(1)
                        } else {
                            r.min(row_count - 1)
                        };
                        Some((r, col))
                    }
                }
            }
            // Ctrl+Tab / Ctrl+Shift+Tab escape the cell grid: return Ignored so
            // the framework's focus cycling moves to the next / previous widget.
            // Plain Tab still navigates cells (the `CellsThenRows` trap), but
            // this gives keyboard users a reliable way out — the same un-trap
            // affordance `RichTextEditor` leaves to OS focus navigation.
            //
            // Literal `ctrl()`, not `command()`: Ctrl+Tab is Ctrl+Tab on macOS
            // too — ⌘⇥ belongs to the application switcher and never reaches an
            // app at all.
            Key::Tab if modifiers.ctrl() => return EventResponse::Ignored,
            Key::Tab => {
                if modifiers.shift() {
                    if col > 0 {
                        Some((row, col - 1))
                    } else if let Some(prev) = cfg.navigator.prev_row(row) {
                        Some((prev, cfg.col_count - 1))
                    } else if cfg.tab_traversal == TabTraversal::OutOfTable {
                        return EventResponse::Ignored;
                    } else {
                        Some((row, col))
                    }
                } else {
                    if col + 1 < cfg.col_count {
                        Some((row, col + 1))
                    } else if let Some(next) = cfg.navigator.next_row(row) {
                        Some((next, 0))
                    } else if cfg.tab_traversal == TabTraversal::OutOfTable {
                        return EventResponse::Ignored;
                    } else {
                        Some((row, col))
                    }
                }
            }
            // Space and its two modified forms. In a **row**-selection table
            // this is the Explorer trio: Space toggles the focused row, and
            // Ctrl+Space does the same after a Ctrl+Arrow walk away from the
            // selection.
            //
            // In a **multi**-cell grid the same two chords are already spoken
            // for, and mean the opposite way round: the ARIA grid pattern and
            // Excel both read Ctrl+Space as "select this column" and
            // Shift+Space as "select this row". Reading them as a toggle there
            // would take a spreadsheet user's two most-used selection chords
            // and do something else with them.
            //
            // `MultiCell` only, not every cell mode: a `SingleCell` table
            // holds one cell by definition, so "select the column" has no
            // reading there — it would quietly break the mode's own
            // invariant. It falls through to the plain toggle below, the same
            // way `select_all` already declines outside the multi modes.
            Key::Space
                if cfg.selection_mode == TableSelectionMode::MultiCell && modifiers.ctrl() =>
            {
                select_column(&cfg, col, row_count);
                cfg.focused_cell.set(Some((row, col)));
                return EventResponse::Handled;
            }
            Key::Space
                if cfg.selection_mode == TableSelectionMode::MultiCell && modifiers.shift() =>
            {
                select_row_cells(&cfg, row);
                cfg.focused_cell.set(Some((row, col)));
                return EventResponse::Handled;
            }
            Key::Space => {
                // A cell carrying a checkbox reads Space as "check this" — its
                // control is out of the Tab order, so this is the only
                // keyboard route to it. A cell that publishes no toggle falls
                // back to the selection, exactly as before.
                if let Some(cell_id) = cfg
                    .cell_map
                    .borrow()
                    .iter()
                    .find(|(pos, _)| *pos == (row, col))
                    .map(|(_, id)| *id)
                {
                    let fallback_cfg = cfg.clone();
                    ctx.row_space_activate(
                        cell_id,
                        std::rc::Rc::new(move || {
                            toggle_selection(&fallback_cfg, row, col);
                        }),
                    );
                    cfg.focused_cell.set(Some((row, col)));
                    return EventResponse::Handled;
                }
                toggle_selection(&cfg, row, col);
                cfg.focused_cell.set(Some((row, col)));
                return EventResponse::Handled;
            }
            Key::Enter => {
                if let Some(ref f) = cfg.on_row_activate {
                    f(row, ctx);
                } else {
                    toggle_selection(&cfg, row, col);
                }
                return EventResponse::Handled;
            }
            Key::F2 if (cfg.display_col_triggers)(col).contains(EditTriggers::F2) => {
                if let Some(col_id) = (cfg.display_col_to_id)(col) {
                    cfg.editing_cell.set(Some((row, col)));
                    if let Some(ref f) = cfg.on_cell_edit_request {
                        f(row, &col_id, ctx);
                    }
                    return EventResponse::Handled;
                }
                return EventResponse::Ignored;
            }
            // Type-to-edit. `Key::Character` only fires for non-letter
            // printable chars on this platform; letters arrive as the
            // dedicated `Key::A`..`Key::Z` variants. Match any key that
            // has a printable char form via `Key::to_char()`. Gated on
            // the column's `editable` flag so non-editable columns
            // don't enter edit mode (which would set `editing_cell`
            // without any actual editor in the cell to receive focus
            // and follow-up keystrokes).
            k if (cfg.display_col_triggers)(col).contains(EditTriggers::ANY_KEY)
                && !modifiers.ctrl()
                && !modifiers.alt()
                && !modifiers.super_key()
                && k.to_char().is_some() =>
            {
                if let Some(col_id) = (cfg.display_col_to_id)(col) {
                    cfg.editing_cell.set(Some((row, col)));
                    if let Some(ref f) = cfg.on_cell_edit_request {
                        f(row, &col_id, ctx);
                    }
                    // Don't claim Handled — the typed character should
                    // propagate to the editor that the cell delegate
                    // swaps in.
                    return EventResponse::Ignored;
                }
                return EventResponse::Ignored;
            }
            // Type-ahead: a printable char (no Ctrl/Alt/Super) jumps the
            // focused row to the next row whose label starts with the
            // accumulated term. Reached only when the type-to-edit arm above
            // didn't consume the char (no editor on this column / edit off).
            k if cfg.type_ahead_label.is_some()
                && !modifiers.ctrl()
                && !modifiers.alt()
                && !modifiers.super_key()
                && k.to_char().is_some() =>
            {
                let c = k.to_char().unwrap();
                let label = cfg.type_ahead_label.as_ref().unwrap();
                if let Some(nr) =
                    cfg.type_ahead
                        .search(c, row, row_count, cfg.type_ahead_timeout, |i| label(i))
                {
                    cfg.focused_cell.set(Some((nr, col)));
                    apply_selection_extension(&cfg, nr, col, false);
                    ensure_row_visible(&cfg, nr, row_count, ctx);
                    ensure_col_visible(&cfg, col);
                    return EventResponse::Handled;
                }
                return EventResponse::Ignored;
            }
            // Select all — Ctrl+A, ⌘A on macOS; Ctrl+Shift+A deselects (GTK's
            // chord, unspent by every other toolkit).
            // `Ignored` outside the multi modes, so the chord reaches the
            // application rather than being swallowed by a table that has
            // nothing to deselect — the shape `ListView`, `TreeView` and
            // `GridView` already use for the same pair.
            Key::A if modifiers.command() && modifiers.shift() => {
                if !cfg.selection_mode.is_multi() {
                    return EventResponse::Ignored;
                }
                clear_selection(&cfg);
                return EventResponse::Handled;
            }
            Key::A if modifiers.command() => {
                select_all(&cfg, row_count);
                return EventResponse::Handled;
            }
            Key::Escape => {
                if cfg.editing_cell.get().is_some() {
                    cfg.editing_cell.set(None);
                } else {
                    cfg.focused_cell.set(None);
                }
                return EventResponse::Handled;
            }
            _ => None,
        };

        if let Some((nr, nc)) = new_pos {
            cfg.focused_cell.set(Some((nr, nc)));
            // Explorer/Finder convention: Ctrl+Arrow (no Shift) repositions
            // the keyboard cursor without touching selection — the followed
            // "select the row you land on" behavior is opt-out only via
            // Ctrl, exactly like plain Arrow's select-follow is opt-in via
            // nothing (default) and Shift+Arrow's extend is opt-in via
            // Shift. `Ctrl+Space` (below, `Key::Space`'s `toggle_selection`
            // already ignores modifiers) then toggles just the cell the
            // cursor moved to.
            let is_arrow = matches!(
                key,
                Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
            );
            // Literal `ctrl()`, macOS included: ⌘↑/⌘↓ already mean something
            // else in a Finder list, and this Explorer-style cursor pair has no
            // ⌘ counterpart — Control keeps it reachable and out of the way.
            let move_cursor_only = is_arrow && modifiers.ctrl() && !modifiers.shift();
            // The edge-and-page keys carry their own verb from `list_nav`,
            // where the accelerator means "move the cursor, leave the
            // selection alone" — the rule GTK4 and Qt apply to every
            // navigation key, and the one the arrows above already follow.
            match nav.map(|c| c.selection) {
                Some(list_nav::SelectionOp::Suppress) => {}
                Some(list_nav::SelectionOp::Extend) => {
                    apply_selection_extension(&cfg, nr, nc, true)
                }
                Some(list_nav::SelectionOp::ExtendAdditive) => {
                    apply_additive_extension(&cfg, nr, nc)
                }
                Some(list_nav::SelectionOp::Replace) => {
                    apply_selection_extension(&cfg, nr, nc, false)
                }
                None if !move_cursor_only => {
                    apply_selection_extension(&cfg, nr, nc, modifiers.shift())
                }
                None => {}
            }
            ensure_row_visible(&cfg, nr, row_count, ctx);
            ensure_col_visible(&cfg, nc);
            return EventResponse::Handled;
        }

        EventResponse::Ignored
    }
}

/// Scroll the viewport so `row` is fully visible — a no-op when it
/// already is. Gives `TableView` / `TreeTableView` the same
/// "keyboard-focused row stays on screen" behavior that `ListView` /
/// `TreeView` already have: every Arrow / Home / End / Ctrl+Home/End /
/// Tab move that lands on a new row keeps it visible.
///
/// `PageUp` / `PageDown` already set `scroll_y` to a page boundary and
/// pick a focus row at the new viewport edge, so calling this for them
/// only refines the offset (the chosen row is visible by construction —
/// no extra jump).
fn ensure_row_visible(
    cfg: &KeyHandlerConfig,
    row: usize,
    row_count: usize,
    ctx: &mut EventContext,
) {
    let scroll = cfg.scroll_y.get();
    let new_scroll = {
        let mut m = cfg.row_metrics.borrow_mut();
        m.resize(row_count);
        m.scroll_for_ensure_visible(
            row,
            scroll,
            cfg.viewport_height.get(),
            cfg.max_scroll_y.get(),
        )
    };
    if (new_scroll - scroll).abs() > f32::EPSILON {
        cfg.scroll_y.set(new_scroll);
    }
    // After keeping the row in the table's OWN viewport, chain the reveal to
    // any enclosing scroll area (a form/page the table is embedded in).
    crate::common::row_metrics::chase_row_into_outer_view(
        ctx,
        &cfg.row_metrics,
        cfg.body_bounds.get(),
        row,
        new_scroll,
    );
}

/// Scroll the Middle pane horizontally so `display_col` is fully visible —
/// the horizontal analogue of [`ensure_row_visible`]. A no-op for a
/// Leading/Trailing-pinned column: pinning already guarantees visibility, so
/// the column can never trigger horizontal scrolling. Unlike rows (via
/// `RowMetrics`, virtualized over thousands of entries), the column count is
/// small and already fully resolved in `column_widths`, so a plain linear
/// scan suffices — no shared "ColumnMetrics" abstraction needed.
fn ensure_col_visible(cfg: &KeyHandlerConfig, display_col: usize) {
    let b = cfg.pane_boundaries;
    if display_col < b.leading_count || display_col >= b.middle_end {
        return;
    }
    let widths = cfg.column_widths.borrow();
    let Some(w) = widths.get(display_col).copied() else {
        return;
    };
    // Logical x of `display_col` within the *unscrolled* Middle content
    // strip (offset from the Middle pane's own leading edge) — i.e.
    // `column_logical_x` with `scroll_x = 0`, restricted to the Middle
    // pane's own local space (band_width is irrelevant here since Trailing
    // never enters this branch).
    let x: f32 = widths[b.leading_count..display_col].iter().sum();
    drop(widths);

    let viewport_w = cfg.middle_viewport_width.get();
    let scroll = cfg.scroll_x.get();
    let max = cfg.max_scroll_x.get();
    let new_scroll = if x < scroll {
        x
    } else if x + w > scroll + viewport_w {
        (x + w - viewport_w).max(0.0)
    } else {
        scroll
    }
    .clamp(0.0, max.max(0.0));
    if (new_scroll - scroll).abs() > f32::EPSILON {
        cfg.scroll_x.set(new_scroll);
    }
}

/// Which column `Ctrl+Home` / `Ctrl+End` land on.
///
/// The ARIA grid and treegrid patterns differ here deliberately: a flat cell
/// grid escalates to the table's corner, a treegrid keeps the column the
/// cursor was already in. A row-selection table has no column cursor to move.
///
/// The navigator is the discriminator — only a hierarchical one reports a
/// depth — so this costs no extra config field and cannot fall out of step
/// with the widget it describes.
fn corner_column(
    cfg: &KeyHandlerConfig,
    view_kind: list_nav::ViewKind,
    col: usize,
    last: bool,
) -> usize {
    if view_kind != list_nav::ViewKind::CellGrid {
        return col;
    }
    if is_hierarchical(cfg) {
        col
    } else if last {
        cfg.col_count - 1
    } else {
        0
    }
}

/// Whether the navigator reports a hierarchy at all.
///
/// Only `TreeNavigator` answers `depth`, so this is what separates a
/// `TreeTableView` from a `TableView` without a second config field that
/// could fall out of step with the widget it describes.
fn is_hierarchical(cfg: &KeyHandlerConfig) -> bool {
    cfg.navigator
        .first_row()
        .and_then(|r| cfg.navigator.depth(r))
        .is_some()
}

/// Run `f` against a [`SubtreeOps`](crate::common::tree_expand::SubtreeOps)
/// view of the navigator.
///
/// A flat table's navigator reports no depth and no children, so every
/// recursive expand over one is a no-op — which is why `TableView` can share
/// this path without a second branch.
fn with_navigator_subtree<R>(
    cfg: &KeyHandlerConfig,
    f: impl FnOnce(&crate::common::tree_expand::SubtreeOps) -> R,
) -> R {
    let nav = &cfg.navigator;
    let count = || nav.row_count();
    let row = |i: usize| {
        nav.depth(i)
            .map(|d| (d, nav.has_children(i), nav.is_expanded(i)))
    };
    let set = |i: usize, on: bool| {
        if nav.is_expanded(i) != on {
            nav.toggle_expanded(i);
        }
    };
    f(&crate::common::tree_expand::SubtreeOps {
        visible_count: &count,
        row: &row,
        set_expanded: &set,
    })
}

fn toggle_selection(cfg: &KeyHandlerConfig, row: usize, col: usize) {
    match cfg.selection_mode {
        TableSelectionMode::SingleRow | TableSelectionMode::MultiRow => {
            if let Some(ref s) = cfg.selection {
                if s.is_selected(row) {
                    // Toggle off: a Multi selection model can have it
                    // both ways; Single mode replaces with empty.
                    if cfg.selection_mode == TableSelectionMode::MultiRow {
                        s.toggle(row);
                    } else {
                        s.clear();
                    }
                } else {
                    s.select(row);
                }
            }
        }
        TableSelectionMode::SingleCell | TableSelectionMode::MultiCell => {
            if let Some(ref cs) = cfg.cell_selection {
                if cs.is_selected(row, col) && cfg.selection_mode == TableSelectionMode::MultiCell {
                    cs.toggle(row, col);
                } else {
                    cs.select(row, col);
                }
            }
        }
        TableSelectionMode::None => {}
    }
}

fn apply_selection_extension(cfg: &KeyHandlerConfig, row: usize, col: usize, shift: bool) {
    match cfg.selection_mode {
        TableSelectionMode::MultiRow => {
            if let Some(ref s) = cfg.selection {
                if shift && s.mode() == SelectionMode::Multi {
                    s.extend_to(row);
                } else {
                    s.select(row);
                }
            }
        }
        TableSelectionMode::SingleRow => {
            if let Some(ref s) = cfg.selection {
                s.select(row);
            }
        }
        TableSelectionMode::MultiCell => {
            if let Some(ref cs) = cfg.cell_selection {
                if shift {
                    cs.extend_to(row, col);
                } else {
                    cs.select(row, col);
                }
            }
        }
        TableSelectionMode::SingleCell => {
            if let Some(ref cs) = cfg.cell_selection {
                cs.select(row, col);
            }
        }
        TableSelectionMode::None => {}
    }
}

/// Ctrl+Shift+`nav` — extend the range while keeping whatever the previous
/// gesture selected. Only the multi modes have a range to extend.
fn apply_additive_extension(cfg: &KeyHandlerConfig, row: usize, col: usize) {
    match cfg.selection_mode {
        TableSelectionMode::MultiRow => {
            if let Some(ref s) = cfg.selection {
                s.extend_to_additive(row);
            }
        }
        // `CellSelectionModel::extend_to` already unions with the set
        // committed at the last non-extending action, so its plain extend is
        // the additive one — the row model is the side that needed a second
        // entry point.
        TableSelectionMode::MultiCell => {
            if let Some(ref cs) = cfg.cell_selection {
                cs.extend_to(row, col);
            }
        }
        _ => apply_selection_extension(cfg, row, col, true),
    }
}

/// Ctrl+Space in a cell grid — select the whole column holding the cursor.
/// The ARIA grid pattern and Excel agree on this; it is *not* the file
/// manager's "toggle the focused item".
fn select_column(cfg: &KeyHandlerConfig, col: usize, row_count: usize) {
    if let Some(ref cs) = cfg.cell_selection {
        cs.select_cells((0..row_count).map(|r| (r, col)));
    }
}

/// Shift+Space in a cell grid — select the whole row holding the cursor.
fn select_row_cells(cfg: &KeyHandlerConfig, row: usize) {
    if let Some(ref cs) = cfg.cell_selection {
        cs.select_cells((0..cfg.col_count).map(|c| (row, c)));
    }
}

/// Ctrl+Shift+A — drop the selection whichever model is backing it.
fn clear_selection(cfg: &KeyHandlerConfig) {
    match cfg.selection_mode {
        TableSelectionMode::MultiRow => {
            if let Some(ref s) = cfg.selection {
                s.clear();
            }
        }
        TableSelectionMode::MultiCell => {
            if let Some(ref cs) = cfg.cell_selection {
                cs.clear();
            }
        }
        _ => {}
    }
}

fn select_all(cfg: &KeyHandlerConfig, row_count: usize) {
    match cfg.selection_mode {
        TableSelectionMode::MultiRow => {
            if let Some(ref s) = cfg.selection {
                s.select_all(row_count);
            }
        }
        TableSelectionMode::MultiCell => {
            if let Some(ref cs) = cfg.cell_selection {
                cs.select_all(row_count, cfg.col_count);
            }
        }
        _ => {}
    }
}
