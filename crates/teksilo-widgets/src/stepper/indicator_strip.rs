// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`IndicatorStrip`] — the row (horizontal) or column (vertical) of step
//! markers. Emits `Role::TabList` (non-linear) or `Role::List` (linear) with
//! the matching `aria-orientation`, and records each marker's `WidgetId` into
//! the shared `indicator_ids` buffer for the panes' `labelled_by`.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;

use super::StepperOrientation;
use super::controller::StepperController;
use super::indicator::StepIndicator;
use crate::primitives::{HStack, Spacer, VStack};

#[derive(Clone)]
pub(crate) struct StepMeta {
    pub title: LocalizedString,
    pub supporting_text: Option<LocalizedString>,
}

pub(crate) struct IndicatorStrip {
    steps: Vec<StepMeta>,
    controller: StepperController,
    orientation: StepperOrientation,
    non_linear: bool,
    circle_size: f32,
    indicator_ids: Rc<RefCell<Vec<WidgetId>>>,
    panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    root_child_id: Option<WidgetId>,
}

impl IndicatorStrip {
    pub(crate) fn new(
        steps: Vec<StepMeta>,
        controller: StepperController,
        orientation: StepperOrientation,
        non_linear: bool,
        circle_size: f32,
        indicator_ids: Rc<RefCell<Vec<WidgetId>>>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    ) -> Self {
        Self {
            steps,
            controller,
            orientation,
            non_linear,
            circle_size,
            indicator_ids,
            panel_ids,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for IndicatorStrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndicatorStrip")
            .field("steps", &self.steps.len())
            .field("non_linear", &self.non_linear)
            .finish()
    }
}

impl Widget for IndicatorStrip {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let total = self.steps.len();
        let current = self.controller.current_step_signal();
        let version = self.controller.version_signal();
        let horizontal = matches!(self.orientation, StepperOrientation::Horizontal);

        // Repopulate from scratch so a rebuild doesn't append stale ids.
        self.indicator_ids.borrow_mut().clear();

        let mut indicator_ids = Vec::with_capacity(total);
        for (i, meta) in self.steps.iter().enumerate() {
            // Per-step status follows the controller: recomputed when the
            // version (status change) or current step flips.
            let c = self.controller.clone();
            let status_sig = version.zip(&current).map(move |_| c.status(i));

            let on_activate = if self.non_linear {
                let c2 = self.controller.clone();
                Some(Rc::new(move |_ctx: &mut EventContext| c2.go_to(i))
                    as Rc<dyn Fn(&mut EventContext)>)
            } else {
                None
            };

            let id = ctx.add(StepIndicator::new(
                i,
                total,
                meta.title.clone(),
                meta.supporting_text.clone(),
                status_sig,
                current.clone(),
                self.orientation,
                self.non_linear,
                self.circle_size,
                on_activate,
                self.panel_ids.clone(),
            ));
            // A step gated out by `Step::visible_when` drops its marker too —
            // otherwise the strip advertises a step navigation refuses to
            // reach. Dormant, so it also leaves the AT tree.
            let c2 = self.controller.clone();
            ctx.visible_when(id, version.map(move |_| c2.is_visible(i)));
            indicator_ids.push(id);
        }
        *self.indicator_ids.borrow_mut() = indicator_ids.clone();

        let root = if horizontal {
            // Evenly spread markers across the bar with flexible spacers.
            let mut row = HStack::new().spacing(8.0);
            for (i, &id) in indicator_ids.iter().enumerate() {
                if i > 0 {
                    // The separating spacer follows its marker's visibility,
                    // so hiding a step doesn't leave a double gap.
                    let spacer_id = ctx.add(Spacer::new());
                    let c = self.controller.clone();
                    ctx.visible_when(spacer_id, version.map(move |_| c.is_visible(i)));
                    row = row.add_child(spacer_id);
                }
                row = row.add_child(id);
            }
            ctx.add(row)
        } else {
            let mut col = VStack::new().spacing(16.0);
            for &id in &indicator_ids {
                col = col.add_child(id);
            }
            ctx.add(col)
        };
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.non_linear {
            builder.set_role(teksilo_core::accesskit::Role::TabList);
            builder.inner_mut().set_orientation(match self.orientation {
                StepperOrientation::Horizontal => teksilo_core::accesskit::Orientation::Horizontal,
                StepperOrientation::Vertical => teksilo_core::accesskit::Orientation::Vertical,
            });
        } else {
            builder.set_role(teksilo_core::accesskit::Role::List);
        }
        builder
            .set_name(teksilo_i18n::tr_widget!(a11y_stepper_indicator_strip_name()).resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
