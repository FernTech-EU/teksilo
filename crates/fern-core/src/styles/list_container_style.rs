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
//! ## Wiring status
//!
//! The trait + `style_slots.list_container` slot are in place; the
//! `ListInsertionRecipe` carries the role + thickness data consumed
//! by `ListView::paint` / `TreeView::paint`. Replacing the inline
//! paint with a composed `RectWidget` leaf (the Stage A `TabBar`
//! drop-indicator pattern) is deferred. The slot's data is
//! already read by both widgets' paint passes (no more hard-coded
//! `Color::from_rgba(0.2, 0.4, 0.9, 0.8)`).

use std::rc::Rc;

use fern_tokens::BorderRole;

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
}

impl Default for ListInsertionRecipe {
    fn default() -> Self {
        Self {
            role: BorderRole::Accent,
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
}

pub type SharedListContainerStyle = Rc<dyn ListContainerStyle>;
