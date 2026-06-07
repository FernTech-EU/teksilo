//! [`StepPane`] — wraps one step's content widget and emits a
//! `Role::TabPanel` named by the step title and `aria-labelledby` its indicator
//! (mirrors `TabPane` in `tab_widget`). The W3C APG step pattern: the active
//! step's content is a labelled panel controlled by its step indicator.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;

pub(crate) struct StepPane {
    title: LocalizedString,
    pending_child: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
    self_id: Option<WidgetId>,
    /// Populated by the `Switcher`'s `capture_child_ids_into`, in declaration
    /// order. Used to find this pane's index.
    panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// Populated by the `IndicatorStrip` build, in declaration order. The
    /// indicator at this pane's index is its `labelled_by` target.
    indicator_ids: Rc<RefCell<Vec<WidgetId>>>,
}

impl StepPane {
    pub(crate) fn new(
        title: LocalizedString,
        child: Box<dyn Widget>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
        indicator_ids: Rc<RefCell<Vec<WidgetId>>>,
    ) -> Self {
        Self {
            title,
            pending_child: Some(child),
            child_id: None,
            self_id: None,
            panel_ids,
            indicator_ids,
        }
    }
}

impl std::fmt::Debug for StepPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepPane").field("title", &self.title).finish()
    }
}

impl Widget for StepPane {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.self_id = Some(ctx.self_id());
        if let Some(child) = self.pending_child.take() {
            self.child_id = Some(ctx.add_boxed(child));
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child_id
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
        builder.set_role(bastyde_core::accesskit::Role::TabPanel);
        builder.set_name(self.title.resolve_now());
        // aria-labelledby → the step indicator at this pane's index. Resolve
        // the index by locating self in panel_ids (the Switcher repopulates it
        // each build, so it auto-corrects); skip the relation when the
        // indicator isn't available yet (no dangling refs).
        if let Some(self_id) = self.self_id {
            let panel_ids = self.panel_ids.borrow();
            if let Some(pos) = panel_ids.iter().position(|&id| id == self_id) {
                if let Some(&ind_id) = self.indicator_ids.borrow().get(pos) {
                    builder.push_labelled_by(bastyde_core::accessibility::widget_id_to_node_id(
                        ind_id,
                    ));
                }
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
