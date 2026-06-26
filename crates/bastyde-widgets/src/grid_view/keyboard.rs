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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use bastyde_core::drag_payload::DragPayload;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::EventContext;
use bastyde_data::{DropPosition, SelectionModel};

use super::layout::{GridLayoutStrategy, ScrollAnchor};
use crate::data_views::RowDrag;

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
    pub(crate) strategy: Rc<dyn GridLayoutStrategy>,
    pub(crate) wrap_navigation: bool,
    pub(crate) tab_traversal: GridTabTraversal,
    /// Activation (Enter / double-click) — index only; the app looks up the
    /// item from its own model handle.
    #[allow(clippy::type_complexity)]
    pub(crate) on_tile_activate: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    pub(crate) reorderable: bool,
    /// Source-owned reorder commit (erased from the backing `ListDataSource`).
    /// Alt+Arrow synthesizes a same-view [`RowDrag`] and routes it through the
    /// exact same path a pointer drop takes. `(payload, target, position,
    /// view_id) -> applied`.
    #[allow(clippy::type_complexity)]
    pub(crate) accept_drop_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, usize) -> bool>,
    /// This grid's id, stamped into the synthetic `RowDrag` so the source
    /// recognizes the move as same-view.
    pub(crate) view_id: usize,
    pub(crate) type_ahead_timeout: Duration,
    #[allow(clippy::type_complexity)]
    pub(crate) type_ahead_label: Option<Rc<dyn Fn(usize) -> String>>,
}

/// Build the `on_key` closure for a `GridView`.
pub(crate) fn build_grid_key_handler(
    cfg: GridKeyConfig,
) -> impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static {
    // Type-ahead accumulator: (last keystroke time, buffer).
    let ta_state = Rc::new(RefCell::new((Instant::now(), String::new())));
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
        let current = cfg.focused_index.get().unwrap_or(0).min(n - 1);
        let col = current % cols;

        // Select-all.
        if modifiers.ctrl() && *key == Key::A {
            if let Some(ref sel) = cfg.selection {
                sel.select_all(n);
            }
            return EventResponse::Handled;
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
                let payload = DragPayload::typed(RowDrag {
                    source_index: current,
                    source_view_id: cfg.view_id,
                });
                if (cfg.accept_drop_fn)(&payload, t, position, cfg.view_id) {
                    cfg.focused_index.set(Some(t));
                    if let Some(ref sel) = cfg.selection {
                        sel.select(t);
                    }
                    ensure_visible(&cfg, t);
                }
                return EventResponse::Handled;
            }
        }

        // Type-ahead: a bare printable character jumps to the next match.
        // Use `to_char()` so letters (which arrive as the dedicated
        // `Key::A`..`Key::Z` variants, NOT `Key::Character`) trigger it too —
        // matching only `Key::Character` silently broke letter type-ahead.
        if !modifiers.ctrl()
            && !modifiers.alt()
            && !modifiers.super_key()
            && let Some(c) = key.to_char()
        {
            if let Some(idx) = type_ahead(&cfg, &ta_state, c, current, n) {
                cfg.focused_index.set(Some(idx));
                if let Some(ref sel) = cfg.selection {
                    sel.select(idx);
                }
                ensure_visible(&cfg, idx);
                return EventResponse::Handled;
            }
        }

        let new_idx: Option<usize> = if *key == logical_next {
            if !cfg.wrap_navigation && col == cols - 1 {
                None
            } else {
                Some((current + 1).min(n - 1))
            }
        } else if *key == logical_prev {
            if !cfg.wrap_navigation && col == 0 {
                None
            } else {
                Some(current.saturating_sub(1))
            }
        } else {
            match key {
                Key::ArrowDown => {
                    if current + cols < n {
                        Some(current + cols)
                    } else {
                        None
                    }
                }
                Key::ArrowUp => {
                    if current >= cols {
                        Some(current - cols)
                    } else {
                        None
                    }
                }
                Key::Home if modifiers.ctrl() => Some(0),
                Key::End if modifiers.ctrl() => Some(n - 1),
                Key::Home => Some(current - col), // first item in this row
                Key::End => Some((current - col + cols - 1).min(n - 1)),
                Key::PageDown => {
                    let rows = rows_per_page(&cfg);
                    page_scroll(&cfg, rows as f32);
                    Some((current + rows * cols).min(n - 1))
                }
                Key::PageUp => {
                    let rows = rows_per_page(&cfg);
                    page_scroll(&cfg, -(rows as f32));
                    Some(current.saturating_sub(rows * cols))
                }
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
                Key::Space => {
                    if let Some(ref sel) = cfg.selection {
                        sel.select(current);
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
        if let Some(ref sel) = cfg.selection {
            if modifiers.shift() {
                sel.extend_to(idx);
            } else {
                sel.select(idx);
            }
        }
        ensure_visible(&cfg, idx);
        EventResponse::Handled
    }
}

/// Append `c` to the type-ahead buffer (resetting it after the timeout) and
/// return the next item whose label starts with the buffer, searching from
/// just after the current focus and wrapping. Returns `None` when no label
/// function is configured or nothing matches.
fn type_ahead(
    cfg: &GridKeyConfig,
    state: &Rc<RefCell<(Instant, String)>>,
    c: char,
    current: usize,
    n: usize,
) -> Option<usize> {
    let label_fn = cfg.type_ahead_label.as_ref()?;
    if cfg.type_ahead_timeout.is_zero() || c.is_control() {
        return None;
    }
    let now = Instant::now();
    let mut st = state.borrow_mut();
    if now.duration_since(st.0) > cfg.type_ahead_timeout {
        st.1.clear();
    }
    st.0 = now;
    st.1.push(c.to_ascii_lowercase());
    let buffer = st.1.clone();
    drop(st);

    // Search from current+1, wrapping around to current.
    for offset in 1..=n {
        let i = (current + offset) % n;
        let label = label_fn(i).to_ascii_lowercase();
        if label.starts_with(&buffer) {
            return Some(i);
        }
    }
    None
}

fn rows_per_page(cfg: &GridKeyConfig) -> usize {
    let vp = cfg.viewport_height.get();
    let step = cfg.strategy.estimated_row_height().max(1.0);
    ((vp / step).floor() as usize).max(1)
}

/// Scroll by `rows` rows (signed), clamped. Used by PageUp/PageDown so the
/// viewport tracks the focus jump.
fn page_scroll(cfg: &GridKeyConfig, rows: f32) {
    let step = cfg.strategy.estimated_row_height().max(1.0);
    let max = cfg.max_scroll_y.get();
    let new_y = (cfg.scroll_y.get() + rows * step).clamp(0.0, max);
    cfg.scroll_y.set(new_y);
}

fn ensure_visible(cfg: &GridKeyConfig, idx: usize) {
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
}
