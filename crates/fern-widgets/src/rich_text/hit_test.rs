//! Hit-test helpers and context-target classification.
//!
//! Pointer positions arrive in widget-local screen space. text-typeset's
//! `Typesetter::hit_test` already compensates for the current scroll
//! offset and zoom internally, so the widget forwards the pointer
//! coordinate unchanged. The `ContextTarget` enum is exposed publicly
//! so applications can build external context menus without reaching
//! into the widget's private state.

use fern_canvas::Point;
use fern_text::text_document::{MoveMode, TextDocument};
use fern_text::{HitRegion, HitTestResult, RichTextEngine};

/// What was under a click, for context-menu classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextTarget {
    /// Ordinary text, not inside a selection.
    Plain,
    /// A click inside the current selection (for "Copy/Cut" menus).
    InSelection,
    /// A hyperlink.
    Link { href: String },
    /// An inline image.
    Image { name: String },
    /// A table cell. Row and column are zero-based.
    TableCell {
        table_id: usize,
        row: usize,
        col: usize,
    },
}

/// Convenience wrapper over [`RichTextEngine::hit_test`]. The
/// engine's hit-test accepts widget-local pointer coordinates
/// directly — scroll offset and zoom are applied internally.
pub fn hit_test_at(
    engine: &RichTextEngine,
    screen: Point,
    _scroll_x: f32,
    _scroll_y: f32,
) -> Option<HitTestResult> {
    engine.hit_test(screen.x, screen.y)
}

/// Classify what the user pointed at. Used by both the widget's own
/// single-click dispatch and the application-facing
/// `context_target_at()` helper. The `selection` argument is the current
/// `(anchor, position)` of the widget's cursor in character offsets; if
/// the hit position falls within that range we report `InSelection` so
/// the context menu can show "Copy/Cut" without first collapsing the
/// selection. `document` is used to resolve the table cell row/column
/// for `TableCell` results — text-typeset's `HitTestResult::table_id`
/// gives us the table, but only the document model knows the cell's
/// row/column coordinates.
pub fn classify(
    hit: &HitTestResult,
    selection: Option<(usize, usize)>,
    document: &TextDocument,
) -> ContextTarget {
    // In-selection check first: a click inside the current selection
    // reports `InSelection` regardless of whether the hit landed on a
    // link, image, or plain text — the app's context menu typically
    // wants "Cut/Copy/Paste" in that case rather than link-specific
    // actions.
    if let Some((anchor, caret)) = selection {
        let (lo, hi) = (anchor.min(caret), anchor.max(caret));
        if lo != hi && hit.position >= lo && hit.position <= hi {
            return ContextTarget::InSelection;
        }
    }

    match &hit.region {
        HitRegion::Link { href } => ContextTarget::Link { href: href.clone() },
        HitRegion::Image { name } => ContextTarget::Image { name: name.clone() },
        _ => {
            if let Some(table_id) = hit.table_id {
                // Probe the document model for the cell's row/column.
                // A fresh cursor at `hit.position` asks text-document
                // to resolve the containing cell via
                // `current_table_cell()`, which already understands
                // row/column semantics.
                let probe = document.cursor();
                probe.set_position(hit.position, MoveMode::MoveAnchor);
                if let Some(cell_ref) = probe.current_table_cell() {
                    return ContextTarget::TableCell {
                        table_id,
                        row: cell_ref.row,
                        col: cell_ref.column,
                    };
                }
                return ContextTarget::TableCell {
                    table_id,
                    row: 0,
                    col: 0,
                };
            }
            ContextTarget::Plain
        }
    }
}
