//! Hit-test helpers and context-target classification.
//!
//! Pointer positions arrive in widget-local screen space. text-typeset's
//! `Typesetter::hit_test` already compensates for the current scroll
//! offset and zoom internally, so the widget forwards the pointer
//! coordinate unchanged. The `ContextTarget` enum is exposed publicly
//! so applications can build external context menus without reaching
//! into the widget's private state.

use fern_canvas::Point;
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
    /// A table cell.
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
/// selection.
pub fn classify(
    hit: &HitTestResult,
    selection: Option<(usize, usize)>,
) -> ContextTarget {
    match &hit.region {
        HitRegion::Link { href } => ContextTarget::Link { href: href.clone() },
        HitRegion::Image { name } => ContextTarget::Image { name: name.clone() },
        _ => {
            if let (Some((anchor, caret)), Some(table_id)) = (selection, hit.table_id) {
                let (lo, hi) = (anchor.min(caret), anchor.max(caret));
                if hit.position >= lo && hit.position <= hi {
                    return ContextTarget::InSelection;
                }
                return ContextTarget::TableCell {
                    table_id,
                    row: 0,
                    col: 0,
                };
            }
            if let Some((anchor, caret)) = selection {
                let (lo, hi) = (anchor.min(caret), anchor.max(caret));
                if lo != hi && hit.position >= lo && hit.position <= hi {
                    return ContextTarget::InSelection;
                }
            }
            ContextTarget::Plain
        }
    }
}
