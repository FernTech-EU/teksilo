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

use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::EventContext;
use bastyde_data::SelectionMode;

use super::column::{EditTrigger, TabTraversal};
use super::row_navigator::RowNavigator;
use super::selection::{CellSelectionModel, TableSelectionMode};
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
    pub selection_mode: TableSelectionMode,
    pub selection: Option<RowSelection>,
    pub cell_selection: Option<CellSelectionModel>,
    pub scroll_y: Signal<f32>,
    pub max_scroll_y: Signal<f32>,
    pub viewport_height: Rc<std::cell::Cell<f32>>,
    /// The row-area's absolute (window) rect: row 0's top sits at
    /// `body_bounds.y` when `scroll_y == 0`. Read to chase the keyboard-focused
    /// row into any *enclosing* scroll area via
    /// [`EventContext::ensure_visible`](bastyde_core::widget::EventContext::ensure_visible)
    /// — the table's own viewport follow is handled by `scroll_y`. Rows are not
    /// distinct focusable nodes, so the framework's focus-driven follow never
    /// reveals the selected row in an outer scroller. Populated by each widget's
    /// `place_children`.
    pub body_bounds: Rc<std::cell::Cell<bastyde_canvas::Rect>>,
    /// Row geometry (uniform / exact / auto-measure) — drives the
    /// PageUp/PageDown focus-row math.
    pub row_metrics: SharedRowMetrics,
    pub tab_traversal: TabTraversal,
    pub editing_cell: Signal<Option<(usize, usize)>>,
    pub edit_trigger: EditTrigger,
    /// `(row, col_id)` → invoke user's edit hook. The closure resolves
    /// `col_id` from a display position; we keep it generic over
    /// `&str` so the keyboard module doesn't need a `Column<T>`
    /// reference.
    pub display_col_to_id: Rc<dyn Fn(usize) -> Option<String>>,
    /// Whether the column at the given display position is editable.
    /// F2 and type-to-edit are no-ops on cells of non-editable
    /// columns — entering edit mode on a column whose delegate has no
    /// editor would only confuse the focus / dispatch state.
    pub display_col_editable: Rc<dyn Fn(usize) -> bool>,
    /// Optional: user callback fired when an edit trigger matches.
    #[allow(clippy::type_complexity)]
    pub on_cell_edit_request:
        Option<Rc<dyn Fn(usize, &str, &mut bastyde_core::widget::EventContext)>>,
    /// Optional: row-activate (Enter) callback.
    #[allow(clippy::type_complexity)]
    pub on_row_activate: Option<Rc<dyn Fn(usize, &mut bastyde_core::widget::EventContext)>>,
    /// Persistent type-ahead buffer (survives the per-keystroke rebuild).
    pub type_ahead: Rc<TypeAheadState>,
    /// Type-ahead label resolver: `row -> Some(text)` for a resident row.
    /// `None` (the option) disables type-ahead.
    #[allow(clippy::type_complexity)]
    pub type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>>,
    /// Reset window for the type-ahead search term.
    pub type_ahead_timeout: Duration,
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
        if is_collapse_key && on_tree_column && cfg.navigator.is_expanded(row) {
            cfg.navigator.toggle_expanded(row);
            return EventResponse::Handled;
        }
        if is_expand_key
            && on_tree_column
            && cfg.navigator.has_children(row)
            && !cfg.navigator.is_expanded(row)
        {
            cfg.navigator.toggle_expanded(row);
            return EventResponse::Handled;
        }

        let viewport_h = cfg.viewport_height.get();

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
            Key::Home if !modifiers.ctrl() => Some((row, 0)),
            Key::End if !modifiers.ctrl() => Some((row, cfg.col_count - 1)),
            Key::Home if modifiers.ctrl() => cfg.navigator.first_row().map(|r| (r, 0)),
            Key::End if modifiers.ctrl() => {
                cfg.navigator.last_row().map(|r| (r, cfg.col_count - 1))
            }
            Key::PageUp => {
                // Scroll one page; move focus to the row one viewport
                // above the current row's top (offset-table-driven, so
                // variable heights page by visual distance, not by a
                // fixed row count). Guarantee progress even when a
                // single row is taller than the viewport.
                let new_y = (cfg.scroll_y.get() - viewport_h).max(0.0);
                cfg.scroll_y.set(new_y);
                let r = {
                    let mut m = cfg.row_metrics.borrow_mut();
                    m.resize(row_count);
                    let target_y = (m.row_top(row) - viewport_h).max(0.0);
                    m.row_at(target_y)
                };
                let r = if r == row { row.saturating_sub(1) } else { r };
                Some((r, col))
            }
            Key::PageDown => {
                let new_y = (cfg.scroll_y.get() + viewport_h).min(cfg.max_scroll_y.get());
                cfg.scroll_y.set(new_y);
                let r = {
                    let mut m = cfg.row_metrics.borrow_mut();
                    m.resize(row_count);
                    let target_y = m.row_top(row) + viewport_h;
                    m.row_at(target_y)
                };
                let r = if r == row {
                    (row + 1).min(row_count - 1)
                } else {
                    r.min(row_count - 1)
                };
                Some((r, col))
            }
            // Ctrl+Tab / Ctrl+Shift+Tab escape the cell grid: return Ignored so
            // the framework's focus cycling moves to the next / previous widget.
            // Plain Tab still navigates cells (the `CellsThenRows` trap), but
            // this gives keyboard users a reliable way out — the same un-trap
            // affordance `RichTextEditor` leaves to OS focus navigation.
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
            Key::Space => {
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
            Key::F2
                if matches!(
                    cfg.edit_trigger,
                    EditTrigger::F2 | EditTrigger::F2OrType | EditTrigger::F2OrTypeOrDoubleClick
                ) && (cfg.display_col_editable)(col) =>
            {
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
            k if matches!(
                cfg.edit_trigger,
                EditTrigger::F2OrType | EditTrigger::F2OrTypeOrDoubleClick
            ) && !modifiers.ctrl()
                && !modifiers.alt()
                && !modifiers.super_key()
                && k.to_char().is_some()
                && (cfg.display_col_editable)(col) =>
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
                    return EventResponse::Handled;
                }
                return EventResponse::Ignored;
            }
            Key::A if modifiers.ctrl() => {
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
            apply_selection_extension(&cfg, nr, nc, modifiers.shift());
            ensure_row_visible(&cfg, nr, row_count, ctx);
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
