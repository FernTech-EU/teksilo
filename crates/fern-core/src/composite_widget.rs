use crate::accessibility::AccessNodeBuilder;
use crate::event::{EventResponse, WidgetEvent};
use crate::focus::FocusPolicy;
use crate::widget::EventContext;
use crate::widget_id::WidgetId;

// Re-export BuildContext from the shared module for backward compatibility.
pub use crate::build_context::BuildContext;

/// Trait for Level 1 (composition) widgets.
/// A composite widget describes what it is made of by implementing build().
#[deprecated(note = "V2: implement Widget trait directly with build() instead")]
pub trait CompositeWidget: std::fmt::Debug {
    /// Construct the initial subtree. Pure — no side effects.
    fn build(&self, ctx: &mut BuildContext) -> WidgetId;

    /// Handle events for the composite as a unit.
    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    /// Focus behavior for the composite.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Default
    }

    /// Whether the composite as a whole can receive keyboard focus.
    fn is_focusable(&self) -> bool {
        false
    }

    /// Accessibility identity for the composite root node.
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    /// Take a deferred `visible_when` binding stored by the builder pattern.
    /// Called before build() to register with the tree after insertion.
    fn take_visible_when(&mut self) -> Option<crate::state::Reactive<bool>> {
        None
    }

    /// Take a deferred `enabled_when` binding stored by the builder pattern.
    /// Called before build() to register with the tree after insertion.
    fn take_enabled_when(&mut self) -> Option<crate::state::Reactive<bool>> {
        None
    }
}
