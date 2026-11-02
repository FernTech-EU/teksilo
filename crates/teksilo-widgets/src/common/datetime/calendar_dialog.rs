// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The popup a date field shows its calendar in: a small dialog, after the
//! WAI-ARIA date-picker dialog pattern.
//!
//! ```text
//! CalendarDialog            overlay content root: no role of its own
//!   FocusScope (Cycle)      Tab and Shift+Tab wrap inside, never leave
//!     dialog node           Role::Dialog, named and described
//!       Calendar            the grid, its header, and Cancel and OK
//! ```
//!
//! The dialog role sits on an inner node, never on the overlay's content
//! root: a content root that is a dialog is a host surface to the overlay
//! stack, which the field's trigger then cannot close.
//!
//! An arrow key on Cancel or OK moves the grid's cursor, as on the header's
//! buttons: the key bubbles to the calendar. OK then picks that day.
//!
//! The page stays clickable (no scrim, no modal flag): a click outside closes
//! the popup and lands where it was clicked, and Escape and Cancel leave it.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accesskit::Role;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::TraversalScopePolicy;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;

use crate::calendar::Calendar;
use crate::focus_scope::FocusScope;
use crate::primitives::VStack;

/// The calendar popup of `DateEdit`, `DateRangeEdit` and `DateTimeEdit`.
///
/// Built once (it is the field's deferred popup) and kept between openings;
/// a rebuild keeps the subtree it built.
pub(crate) struct CalendarDialog {
    calendar: Option<Calendar>,
    name: LocalizedString,
    description: LocalizedString,
    calendar_slot: Rc<Cell<Option<WidgetId>>>,
    dialog_slot: Rc<Cell<Option<WidgetId>>>,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for CalendarDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarDialog").finish_non_exhaustive()
    }
}

impl CalendarDialog {
    pub(crate) fn new(
        calendar: Calendar,
        name: LocalizedString,
        description: LocalizedString,
    ) -> Self {
        Self {
            calendar: Some(calendar),
            name,
            description,
            calendar_slot: Rc::new(Cell::new(None)),
            dialog_slot: Rc::new(Cell::new(None)),
            root: None,
        }
    }

    /// The calendar's id once built: where focus goes when the popup opens
    /// (asking the popup for focus would land on its first header button).
    pub(crate) fn calendar_slot(&self) -> Rc<Cell<Option<WidgetId>>> {
        self.calendar_slot.clone()
    }

    /// The dialog node's id once built: what the field's `controls` names.
    pub(crate) fn dialog_slot(&self) -> Rc<Cell<Option<WidgetId>>> {
        self.dialog_slot.clone()
    }
}

impl Widget for CalendarDialog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(root) = self.root {
            return vec![root];
        }
        let Some(calendar) = self.calendar.take() else {
            return Vec::new();
        };
        let calendar_id = ctx.add(calendar);
        let dialog_id = ctx.add(
            VStack::new()
                .child(calendar_id)
                .access_role(Role::Dialog)
                .access_label(self.name.clone())
                .access_description(self.description.clone()),
        );
        let root = ctx.add(FocusScope::new(TraversalScopePolicy::Cycle).child(dialog_id));
        self.calendar_slot.set(Some(calendar_id));
        self.dialog_slot.set(Some(dialog_id));
        self.root = Some(root);
        vec![root]
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}
