//! Shared keyboard handler for `TableView` and `TreeTable`.
//!
//! The handler is generic over [`RowNavigator`] so flat and tree
//! navigation reuse the same key matrix. Tree-specific arrow-left /
//! arrow-right collapse/expand semantics fall through automatically
//! because the trait's default `is_expanded` / `has_children` /
//! `toggle_expanded` methods are no-ops on a flat table.

use std::rc::Rc;

use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::EventContext;
use bastyde_data::{SelectionMode, SelectionModel};

use super::column::{EditTrigger, TabTraversal};
use super::row_navigator::RowNavigator;
use super::selection::{CellSelectionModel, TableSelectionMode};

/// Configuration captured from the table at build time and threaded
/// into the on_key handler. Cheap to clone (signals + Rcs).
#[derive(Clone)]
pub(crate) struct KeyHandlerConfig {
    pub navigator: Rc<dyn RowNavigator>,
    pub col_count: usize,
    pub focused_cell: Signal<Option<(usize, usize)>>,
    pub selection_mode: TableSelectionMode,
    pub selection: Option<SelectionModel>,
    pub cell_selection: Option<CellSelectionModel>,
    pub scroll_y: Signal<f32>,
    pub max_scroll_y: Signal<f32>,
    pub viewport_height: Rc<std::cell::Cell<f32>>,
    pub row_height: f32,
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
    pub on_cell_edit_request: Option<Rc<dyn Fn(usize, &str, &mut bastyde_core::widget::EventContext)>>,
    /// Optional: row-activate (Enter) callback.
    #[allow(clippy::type_complexity)]
    pub on_row_activate: Option<Rc<dyn Fn(usize, &mut bastyde_core::widget::EventContext)>>,
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
        let (row, col) = cfg.focused_cell.get().unwrap_or((0, 0));
        let row = row.min(row_count - 1);
        let col = col.min(cfg.col_count - 1);

        // Tree-aware ArrowLeft / ArrowRight (flat impls are
        // no-ops, so this is safe to evaluate eagerly).
        let on_tree_column = col == 0; // tree column is leftmost in
        // current TreeTable scope
        if matches!(key, Key::ArrowLeft) && on_tree_column && cfg.navigator.is_expanded(row) {
            cfg.navigator.toggle_expanded(row);
            return EventResponse::Handled;
        }
        if matches!(key, Key::ArrowRight)
            && on_tree_column
            && cfg.navigator.has_children(row)
            && !cfg.navigator.is_expanded(row)
        {
            cfg.navigator.toggle_expanded(row);
            return EventResponse::Handled;
        }

        let viewport_h = cfg.viewport_height.get();
        let rows_per_page = ((viewport_h / cfg.row_height).floor() as usize).max(1);

        let new_pos: Option<(usize, usize)> = match key {
            Key::ArrowUp => cfg.navigator.prev_row(row).map(|r| (r, col)),
            Key::ArrowDown => cfg.navigator.next_row(row).map(|r| (r, col)),
            Key::ArrowLeft => {
                if col == 0 {
                    None
                } else {
                    Some((row, col - 1))
                }
            }
            Key::ArrowRight => {
                if col + 1 >= cfg.col_count {
                    None
                } else {
                    Some((row, col + 1))
                }
            }
            Key::Home if !modifiers.ctrl() => Some((row, 0)),
            Key::End if !modifiers.ctrl() => Some((row, cfg.col_count - 1)),
            Key::Home if modifiers.ctrl() => cfg.navigator.first_row().map(|r| (r, 0)),
            Key::End if modifiers.ctrl() => {
                cfg.navigator.last_row().map(|r| (r, cfg.col_count - 1))
            }
            Key::PageUp => {
                // Scroll one page; move focus the same number of rows up.
                let new_y = (cfg.scroll_y.get() - viewport_h).max(0.0);
                cfg.scroll_y.set(new_y);
                let r = row.saturating_sub(rows_per_page);
                Some((r, col))
            }
            Key::PageDown => {
                let new_y = (cfg.scroll_y.get() + viewport_h).min(cfg.max_scroll_y.get());
                cfg.scroll_y.set(new_y);
                let r = (row + rows_per_page).min(row_count - 1);
                Some((r, col))
            }
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
            return EventResponse::Handled;
        }

        EventResponse::Ignored
    }
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
