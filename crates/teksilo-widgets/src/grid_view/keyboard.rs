// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! 2D keyboard navigation for `GridView`.
//!
//! Mirrors `table_view/keyboard.rs` but for a flat-model tile grid: arrow
//! keys move by ±1 (within a row) and ±columns (between rows), with RTL
//! horizontal swap, Home/End row ends, Ctrl+Home/End document ends,
//! PageUp/Down by a viewport of rows, Tab traversal, Space/Enter to
//! select, Escape to clear focus, and Ctrl+A to select-all. Shift + any
//! navigation extends the selection range (reading-order, Finder/Explorer
//! style). Every navigation scrolls the new focus into view.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_core::drag_payload::DragPayload;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;
use teksilo_data::{DropPosition, SelectionModel};

use super::layout::{GridLayoutStrategy, ScrollAnchor};
use crate::common::list_nav;
use crate::common::type_ahead::TypeAheadState;
use crate::data_views::ViewId;

/// How Tab moves out of (or within) the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridTabTraversal {
    /// Tab releases focus to the next focusable widget in the window.
    #[default]
    OutOfGrid,
    /// Tab advances to the next tile (wrapping rows); Shift+Tab the previous.
    WithinGrid,
}

/// Everything the key handler needs, captured at build time. `col_count`
/// and `row_height` are shared `Cell`s updated by the layout pass so the
/// handler always reads the live column count.
pub(crate) struct GridKeyConfig {
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    pub(crate) col_count: Signal<usize>,
    pub(crate) focused_index: Signal<Option<usize>>,
    pub(crate) selection: Option<SelectionModel>,
    pub(crate) scroll_y: Signal<f32>,
    pub(crate) max_scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    pub(crate) viewport_width: Rc<Cell<f32>>,
    /// The grid body pane's absolute (window) origin, published each layout
    /// pass by `GridBodyPane::place_children` (`None` until the pane has laid
    /// out at least once). Lets the handler compute the focused tile's absolute
    /// rect (`origin + tile_rect - scroll`) and chase it into any *enclosing*
    /// scroll area via
    /// [`EventContext::ensure_visible`](teksilo_core::widget::EventContext::ensure_visible).
    /// Tiles are virtualized and not focusable, so the focus-driven follow
    /// never reveals the focused tile in an outer scroller. Left as `None`, the
    /// chase is skipped so a nav dispatched before the first layout can't anchor
    /// the rect at (0, 0).
    pub(crate) viewport_origin: Rc<Cell<Option<teksilo_canvas::Point>>>,
    pub(crate) strategy: Rc<dyn GridLayoutStrategy>,
    pub(crate) wrap_navigation: bool,
    pub(crate) tab_traversal: GridTabTraversal,
    /// Activation (Enter / double-click) — index only; the app looks up the
    /// item from its own model handle.
    #[allow(clippy::type_complexity)]
    pub(crate) on_tile_activate: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    pub(crate) reorderable: bool,
    /// Source-owned reorder commit (erased from the backing `ListDataSource`).
    /// Alt+Arrow synthesizes a same-view `RowDragData<T>` (via
    /// `make_reorder_payload`, below) and routes it through the exact same
    /// path a pointer drop takes. `(payload, target, position, view_id) ->
    /// applied`.
    #[allow(clippy::type_complexity)]
    pub(crate) accept_drop_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, ViewId) -> bool>,
    /// This grid's id, stamped into the synthetic drag payload so the source
    /// recognizes the move as same-view.
    pub(crate) view_id: ViewId,
    /// Builds the synthetic same-view reorder payload
    /// (`DragPayload::typed(RowDragData::<T> { .. })`) for a given source
    /// index. Erases the grid's item type `T` so this (non-generic) module
    /// doesn't need a type parameter — mirrors the `DndLazy` erasure pattern.
    pub(crate) make_reorder_payload: Rc<dyn Fn(usize) -> DragPayload>,
    pub(crate) type_ahead_timeout: Duration,
    /// Persistent across rebuilds — see `GridView::type_ahead`.
    pub(crate) type_ahead: Rc<TypeAheadState>,
    /// Index → realized tile id. `Space` asks the focused tile whether it
    /// publishes a keyboard toggle before falling back to the selection.
    #[allow(clippy::type_complexity)]
    pub(crate) tile_map: Rc<std::cell::RefCell<Vec<(usize, teksilo_core::widget_id::WidgetId)>>>,
    /// `None` when a row isn't resident yet (lazy/windowed source) — skipped
    /// during the search rather than matched against whatever the label
    /// closure happens to compute for an absent row.
    #[allow(clippy::type_complexity)]
    pub(crate) type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>>,
}

/// Build the `on_key` closure for a `GridView`.
pub(crate) fn build_grid_key_handler(
    cfg: GridKeyConfig,
) -> impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static {
    let ta_state = cfg.type_ahead.clone();
    move |event, ctx| {
        let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
            return EventResponse::Ignored;
        };
        let n = (cfg.len_fn)();
        if n == 0 {
            return EventResponse::Ignored;
        }
        let cols = cfg.col_count.get().max(1);
        let rtl = ctx.is_rtl();
        // The keyboard cursor: `focused_index` once the user has navigated or
        // clicked, else the current selection (a grid can be handed a selected
        // tile before it is ever focused). `None` = "no cursor yet", which is
        // NOT "cursor on tile 0" — the directional keys below land ON an end
        // tile rather than stepping past it.
        let cursor = cfg
            .focused_index
            .get()
            .or_else(|| {
                cfg.selection
                    .as_ref()
                    .and_then(|s| s.selected_indices().first().copied())
            })
            .map(|i| i.min(n - 1));
        // Anchor for the keys that compute *from* a tile (reorder, paging,
        // row-relative Home/End, type-ahead) rather than step in a direction.
        let current = cursor.unwrap_or(0);
        let col = current % cols;

        // The edge-and-page family, resolved once in `common::list_nav`.
        let nav = list_nav::nav_chord(*key, *modifiers, list_nav::ViewKind::TileGrid);

        // Select-all — Ctrl+A, ⌘A on macOS, and Ctrl+Shift+A to deselect.
        // Gated on `Multi` like `ListView` and `TreeView`: "select all" has no
        // reading for a control that holds at most one tile, and this handler
        // used to claim the chord even with no selection model at all.
        if modifiers.command() && *key == Key::A {
            if let Some(ref sel) = cfg.selection
                && sel.mode() == teksilo_data::SelectionMode::Multi
            {
                if modifiers.shift() {
                    sel.clear();
                } else {
                    sel.select_all(n);
                }
                return EventResponse::Handled;
            }
            return EventResponse::Ignored;
        }

        // Resolve the horizontal arrows, swapping under RTL.
        let logical_prev = if rtl { Key::ArrowRight } else { Key::ArrowLeft };
        let logical_next = if rtl { Key::ArrowLeft } else { Key::ArrowRight };

        // Alt+Arrow: reorder the focused tile (when reorderable).
        if modifiers.alt() && cfg.reorderable {
            let target = if *key == logical_next && current + 1 < n {
                Some(current + 1)
            } else if *key == logical_prev && current > 0 {
                Some(current - 1)
            } else if *key == Key::ArrowDown && current + cols < n {
                Some(current + cols)
            } else if *key == Key::ArrowUp && current >= cols {
                Some(current - cols)
            } else {
                None
            };
            if let Some(t) = target {
                // Express the positional move as a same-view drop the source
                // can validate + apply: dropping `current` *after* `t` when
                // moving forward, *before* `t` when moving back, yields
                // `move_item(current, t)` for an in-memory model.
                let position = if t > current {
                    DropPosition::After
                } else {
                    DropPosition::Before
                };
                let payload = (cfg.make_reorder_payload)(current);
                if (cfg.accept_drop_fn)(&payload, t, position, cfg.view_id) {
                    cfg.focused_index.set(Some(t));
                    if let Some(ref sel) = cfg.selection {
                        sel.select(t);
                    }
                    ensure_visible(&cfg, t, ctx);
                }
                return EventResponse::Handled;
            }
        }

        // macOS reads ⌘↓ as "open the focused item" in a list or an icon view
        // — Finder's meaning, and dead here otherwise. Off macOS this is
        // `None` and costs nothing.
        //
        // Read *after* the Alt+Arrow reorder above, not before: ⌥←/⌥→ resolve
        // to the subtree aliases, which a flat tile grid has no use for, and
        // claiming them first made keyboard reorder a dead chord on macOS.
        if let Some(alias) = list_nav::mac_alias(*key, *modifiers, rtl) {
            if alias == list_nav::MacAlias::Activate {
                cfg.focused_index.set(Some(current));
                if let Some(ref cb) = cfg.on_tile_activate {
                    cb(current, ctx);
                } else if let Some(ref sel) = cfg.selection {
                    sel.select(current);
                }
                return EventResponse::Handled;
            }
            return EventResponse::Ignored;
        }

        // Type-ahead: a bare printable character jumps to the next match.
        // Use `to_char()` so letters (which arrive as the dedicated
        // `Key::A`..`Key::Z` variants, NOT `Key::Character`) trigger it too —
        // matching only `Key::Character` silently broke letter type-ahead.
        //
        // A search that finds nothing returns `Ignored` rather than falling
        // through to the navigation match below, so a printable key can never
        // be read as a movement — the same shape `ListView` and `TreeView`
        // use. Nothing in the current key set is both, but the fall-through
        // made that a latent hazard rather than a decision.
        if let Some(ref label_fn) = cfg.type_ahead_label
            && !modifiers.ctrl()
            && !modifiers.alt()
            && !modifiers.super_key()
            && let Some(c) = key.to_char()
        {
            return match ta_state.search(c, current, n, cfg.type_ahead_timeout, |i| label_fn(i)) {
                Some(idx) => {
                    cfg.focused_index.set(Some(idx));
                    if let Some(ref sel) = cfg.selection {
                        sel.select(idx);
                    }
                    ensure_visible(&cfg, idx, ctx);
                    EventResponse::Handled
                }
                None => EventResponse::Ignored,
            };
        }

        // With no cursor yet, a directional key lands ON the near end tile
        // (first for forward/down, last for backward/up) instead of stepping
        // past it — otherwise the very first ArrowRight would skip tile 0.
        let new_idx: Option<usize> = if *key == logical_next {
            if cursor.is_none() {
                Some(0)
            } else if !cfg.wrap_navigation && col == cols - 1 {
                None
            } else {
                Some((current + 1).min(n - 1))
            }
        } else if *key == logical_prev {
            if cursor.is_none() {
                Some(n - 1)
            } else if !cfg.wrap_navigation && col == 0 {
                None
            } else {
                Some(current.saturating_sub(1))
            }
        } else {
            match key {
                Key::ArrowDown => {
                    if cursor.is_none() {
                        Some(0)
                    } else if current + cols < n {
                        Some(current + cols)
                    } else {
                        None
                    }
                }
                Key::ArrowUp => {
                    if cursor.is_none() {
                        Some(n - 1)
                    } else if current >= cols {
                        Some(current - cols)
                    } else {
                        None
                    }
                }
                // Home / End reach the first / last tile of the collection,
                // not the ends of the reflow row. A wrapped grid's rows change
                // with the window width, so "first tile in this row" is not a
                // target a user can form a model of — `GtkGridView` and Qt's
                // `QListView` in icon mode both resolve it absolutely for that
                // reason. The accelerator therefore adds no new destination
                // here; it only suppresses the selection, via `list_nav`.
                Key::Home | Key::End | Key::PageUp | Key::PageDown => {
                    let Some(chord) = nav else {
                        return EventResponse::Ignored;
                    };
                    Some(match chord.movement {
                        list_nav::NavMove::First | list_nav::NavMove::RowFirst => 0,
                        list_nav::NavMove::Last | list_nav::NavMove::RowLast => n - 1,
                        // Paged by real geometry rather than by
                        // `estimated_row_height`, so `VariableRowGrid` and
                        // `VirtualizedMasonry` page to the tile the user
                        // actually lands on. The shared `ensure_visible` tail
                        // then does the scrolling — this arm used to scroll
                        // too, moving the viewport twice for one keypress.
                        list_nav::NavMove::Page { down } => {
                            page_target(&cfg, current, cols, n, down)
                        }
                    })
                }
                // Ctrl+Tab / Ctrl+Shift+Tab escape the grid, so a
                // `WithinGrid` traversal is never a trap. Literal `ctrl()`,
                // macOS included: ⌘⇥ belongs to the application switcher and
                // never reaches an app. Same hatch `TableView` has.
                Key::Tab if modifiers.ctrl() => return EventResponse::Ignored,
                Key::Tab if cfg.tab_traversal == GridTabTraversal::WithinGrid => {
                    if modifiers.shift() {
                        if current == 0 {
                            None
                        } else {
                            Some(current - 1)
                        }
                    } else if current + 1 < n {
                        Some(current + 1)
                    } else {
                        None
                    }
                }
                Key::Enter => {
                    cfg.focused_index.set(Some(current));
                    if let Some(ref cb) = cfg.on_tile_activate {
                        cb(current, ctx);
                    } else if let Some(ref sel) = cfg.selection {
                        sel.select(current);
                    }
                    return EventResponse::Handled;
                }
                Key::Space if modifiers.ctrl() => {
                    // Ctrl+Space toggles the focused tile's selection — the
                    // keyboard equivalent of Ctrl+click. Pairs with
                    // Ctrl+Arrow's cursor-only move so a user can walk the
                    // cursor without disturbing the existing selection,
                    // then Ctrl+Space to add tiles one at a time.
                    //
                    // Both halves stay on literal `ctrl()`, macOS included:
                    // ⌘Space is Spotlight and never reaches an app, and ⌘↑/⌘↓
                    // already mean something else in a Finder list. This
                    // Explorer-style cursor pair has no ⌘ counterpart, so
                    // Control keeps it reachable and out of the platform's way.
                    if let Some(ref sel) = cfg.selection {
                        sel.toggle(current);
                    }
                    cfg.focused_index.set(Some(current));
                    return EventResponse::Handled;
                }
                Key::Space => {
                    // A tile carrying a checkbox reads Space as "check this" —
                    // its control is out of the Tab order, so this is the only
                    // keyboard route to it. Otherwise: toggle in `Multi`,
                    // select in `Single`, the rule `ListView` and `TreeView`
                    // follow. (This used to select unconditionally, so Space
                    // could never unpick a tile.)
                    let selection = cfg.selection.clone();
                    let fallback = std::rc::Rc::new(move || {
                        if let Some(ref sel) = selection {
                            if sel.mode() == teksilo_data::SelectionMode::Multi {
                                sel.toggle(current);
                            } else {
                                sel.select(current);
                            }
                        }
                    });
                    match cfg
                        .tile_map
                        .borrow()
                        .iter()
                        .find(|(i, _)| *i == current)
                        .map(|(_, id)| *id)
                    {
                        Some(tile_id) => ctx.row_space_activate(tile_id, fallback),
                        None => fallback(),
                    }
                    cfg.focused_index.set(Some(current));
                    return EventResponse::Handled;
                }
                Key::Escape => {
                    cfg.focused_index.set(None);
                    return EventResponse::Handled;
                }
                _ => return EventResponse::Ignored,
            }
        };

        let Some(idx) = new_idx else {
            return EventResponse::Ignored;
        };
        cfg.focused_index.set(Some(idx));
        // What the chord does to the selection. The edge-and-page keys carry
        // their own answer from `list_nav`, where the accelerator means "move
        // the cursor, leave the selection alone".
        //
        // The arrows keep reading literal `ctrl()`: ⌘Space is Spotlight and
        // ⌘↑/⌘↓ mean something else in a Finder icon view, so this pair has no
        // ⌘ counterpart to move to (see the Ctrl+Space arm above). Checked
        // against `logical_next`/`logical_prev` (already RTL-swapped) plus the
        // raw vertical keys, so the chord follows the visual arrow.
        let op = match nav {
            Some(chord) => chord.selection,
            None if modifiers.ctrl()
                && !modifiers.shift()
                && !modifiers.alt()
                && (*key == logical_next
                    || *key == logical_prev
                    || *key == Key::ArrowDown
                    || *key == Key::ArrowUp) =>
            {
                list_nav::SelectionOp::Suppress
            }
            None if modifiers.shift() => list_nav::SelectionOp::Extend,
            None => list_nav::SelectionOp::Replace,
        };
        if let Some(ref sel) = cfg.selection {
            match op {
                list_nav::SelectionOp::Replace => sel.select(idx),
                list_nav::SelectionOp::Suppress => {}
                list_nav::SelectionOp::Extend => sel.extend_to(idx),
                list_nav::SelectionOp::ExtendAdditive => sel.extend_to_additive(idx),
            }
        }
        ensure_visible(&cfg, idx, ctx);
        EventResponse::Handled
    }
}

/// The tile one viewport away from `current`, in the direction pressed.
///
/// Measured against the layout strategy's real tile rectangles rather than
/// `estimated_row_height`, because two of the three shipped strategies do not
/// have a single row height: `VariableRowGrid` sizes each row to its tallest
/// tile and `VirtualizedMasonry` has no rows at all. Paging by the estimate
/// landed short or long on both, and by an amount that changed as the
/// measurements converged.
///
/// The walk is per-row rather than a closed form for the same reason — with
/// variable rows there is no `rows × height` to divide by. It costs one
/// `tile_rect` per row of a single viewport, on a keypress.
fn page_target(cfg: &GridKeyConfig, current: usize, cols: usize, n: usize, down: bool) -> usize {
    let viewport = cfg.viewport_height.get();
    let width = cfg.viewport_width.get();
    let origin = cfg.strategy.tile_rect(current, width).y;
    let mut candidate = current;
    let mut probe = current;
    loop {
        probe = if down {
            match probe.checked_add(cols) {
                Some(next) if next < n => next,
                _ => break,
            }
        } else {
            match probe.checked_sub(cols) {
                Some(prev) => prev,
                None => break,
            }
        };
        let y = cfg.strategy.tile_rect(probe, width).y;
        if (y - origin).abs() > viewport {
            // One row past the viewport edge: stop at the last row inside it,
            // unless that would not move at all.
            candidate = if candidate == current {
                probe
            } else {
                candidate
            };
            break;
        }
        candidate = probe;
    }
    // Guarantee progress even when a single tile is taller than the viewport.
    if candidate == current {
        if down {
            (current + cols).min(n - 1)
        } else {
            current.saturating_sub(cols)
        }
    } else {
        candidate
    }
}

fn ensure_visible(cfg: &GridKeyConfig, idx: usize, ctx: &mut EventContext) {
    let delta = cfg.strategy.scroll_delta_to_reveal(
        idx,
        cfg.scroll_y.get(),
        cfg.viewport_height.get(),
        cfg.viewport_width.get(),
        ScrollAnchor::Auto,
    );
    if delta.abs() > 0.01 {
        let max = cfg.max_scroll_y.get();
        let new_y = (cfg.scroll_y.get() + delta).clamp(0.0, max);
        cfg.scroll_y.set(new_y);
    }
    // After keeping the tile in the grid's OWN viewport, chase it into any
    // enclosing scroll area. Computed analytically from the layout strategy —
    // the tile may be virtualized (not realized as a live widget) — using the
    // post-scroll offset so the rect is the tile's resting on-screen position.
    // Skip when the body pane hasn't published its origin yet (a nav before the
    // first layout), so the rect is never anchored at a stale (0, 0).
    let Some(origin) = cfg.viewport_origin.get() else {
        return;
    };
    let vp_w = cfg.viewport_width.get();
    let r = cfg.strategy.tile_rect(idx, vp_w);
    let scroll_y = cfg.scroll_y.get();
    let rect =
        teksilo_canvas::Rect::new(origin.x + r.x, origin.y + r.y - scroll_y, r.width, r.height);
    ctx.ensure_visible(rect);
}
