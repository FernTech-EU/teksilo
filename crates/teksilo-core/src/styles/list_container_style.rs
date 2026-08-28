// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `ListView` and `TreeView` container
//! chrome (shared trait — both widgets paint the same kind of insertion
//! line). See `docs/styling-system.md`.
//!
//! Both widgets' rows are already themed via `StandardItemStyle`. The
//! only container-level chrome is the drag-insertion line and the
//! optional selection-band painted behind rows (today the selection
//! band is also a paint pass). This trait gives a place to install a
//! custom insertion indicator — the default uses the accent role.
//!
//! ## Two drop affordances, deliberately unalike
//!
//! A tree answers two different questions during a drag — "between which
//! two siblings?" ([`ListInsertionRecipe`], a line) and "inside which
//! container?" ([`ListDropIntoRecipe`], a box round the target row). They
//! must not be mistakable for one another, which is why the box is
//! **inset**: painted flush to the row it would share its top edge with a
//! `Before` line and its bottom edge with an `After` line, and the drag
//! ghost hides the vertical sides that would have told them apart — so the
//! only thing a writer reads is "an accent bar at a row boundary",
//! identical in all three cases. The inset is the whole point of the
//! recipe; a theme lowering it to zero re-creates the ambiguity.
//!
//! ## Wiring status
//!
//! The trait + `style_slots.list_container` slot are in place; the
//! `ListInsertionRecipe` carries the role + thickness data consumed
//! by `ListView::paint` / `TreeView::paint`. Replacing the inline
//! paint with a composed `RectWidget` leaf (analogous to the `TabBar`
//! drop-indicator pattern) is deferred. The slot's data is already
//! read by both widgets' paint passes (no more hard-coded
//! `Color::from_rgba(0.2, 0.4, 0.9, 0.8)`).

use std::rc::Rc;

use teksilo_tokens::BorderRole;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

/// Configuration for `make_insertion_indicator`.
pub struct ListInsertionConfig {
    /// `y` for vertical lists, in container-local coordinates.
    pub axis_offset: f32,
    /// Width (horizontal extent) of the indicator stroke.
    pub width: f32,
}

/// Recipe — non-widget data describing the inline paint of the
/// insertion line. `ListView::paint` / `TreeView::paint` resolve the
/// `role` against the active theme each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListInsertionRecipe {
    /// Border role for the insertion line. Defaults to `Accent`.
    pub role: BorderRole,
    /// Stroke thickness of the line, in logical pixels.
    pub thickness: f32,
    /// Horizontal offset applied per tree level, so the line starts where
    /// the dropped row's own indent will start — the Finder / Scrivener
    /// convention that tells "after this scene, still inside the chapter"
    /// from "after the chapter". Matches `StandardTreeItem`'s indent step;
    /// `0.0` disables the indent. Read by `TreeView` only — `ListView`'s rows
    /// have no depth, and `TreeTableView` uses its own `indent_per_level`,
    /// the value its indent gutter actually renders with.
    pub indent_step: f32,
}

impl Default for ListInsertionRecipe {
    fn default() -> Self {
        Self {
            role: BorderRole::Accent,
            thickness: 2.0,
            indent_step: 16.0,
        }
    }
}

/// Recipe — non-widget data describing the inline paint of the
/// "drop **into** this container" highlight, the reparent counterpart of
/// [`ListInsertionRecipe`]. Resolved against the active theme each frame by
/// `TreeView::paint` / `TreeTableView::paint`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListDropIntoRecipe {
    /// Border role for the box's outline and (at `fill_alpha`) its wash.
    /// Defaults to `Accent`.
    pub role: BorderRole,
    /// Alpha applied to `role`'s colour for the interior wash. High enough
    /// to read on a light surface, low enough to leave the row's own label
    /// legible.
    pub fill_alpha: f32,
    /// Corner radius of the box, in logical pixels.
    pub corner_radius: f32,
    /// How far the box is pulled in from the target row's bounds, in
    /// logical pixels. **Load-bearing** — see the module docs: at `0.0` the
    /// box's horizontal edges land exactly on the pixels a `Before` /
    /// `After` line would occupy, and the two affordances stop being
    /// distinguishable.
    pub inset: f32,
    /// Outline thickness, in logical pixels. Defaults to the insertion line's
    /// own weight, so the two drop affordances read as one family — and so
    /// neither is mistaken for `StandardItem`'s thinner selection edge.
    pub thickness: f32,
}

impl Default for ListDropIntoRecipe {
    fn default() -> Self {
        Self {
            role: BorderRole::Accent,
            fill_alpha: 0.28,
            corner_radius: 6.0,
            inset: 3.0,
            thickness: 2.0,
        }
    }
}

pub trait ListContainerStyle: 'static {
    fn make_insertion_indicator(
        &self,
        cfg: &ListInsertionConfig,
        ctx: &mut BuildContext,
    ) -> WidgetId;
    /// Recipe data for the inline paint pass.
    fn insertion(&self) -> ListInsertionRecipe;

    /// Recipe data for the "drop into this container" highlight. Defaulted,
    /// so an existing style needs no change.
    fn drop_into(&self) -> ListDropIntoRecipe {
        ListDropIntoRecipe::default()
    }
}

pub type SharedListContainerStyle = Rc<dyn ListContainerStyle>;
