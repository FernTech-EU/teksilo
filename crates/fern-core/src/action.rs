//! Widget-owned units of behavior bound to named intents.
//!
//! An [`Action`] lives on a widget alongside its event handlers. When
//! an [`Intent`] is dispatched, the framework walks the focus /
//! source-widget chain and, at each level, invokes any [`Action`]
//! whose `intent` name matches the intent's name. The handler's
//! [`IntentResponse`] controls whether the intent is consumed or
//! continues up the chain.
//!
//! A widget may declare multiple actions for different intent names;
//! at a single level, if two actions match the same name, the first
//! (by declaration order) wins.

use crate::intent::{Intent, IntentResponse};
use crate::signal::Signal;
use crate::widget::EventContext;

/// Closure signature for an action handler.
pub type ActionHandler = Box<dyn FnMut(&Intent, &mut EventContext) -> IntentResponse + 'static>;

/// A widget-owned handler bound to a named intent.
pub struct Action {
    /// The intent name this action responds to. Matches against
    /// [`Intent::name`] during dispatch.
    pub intent: &'static str,
    /// Invoked when a matching intent is dispatched to this widget.
    pub handler: ActionHandler,
    /// Reactive "is this action currently applicable?" predicate.
    /// `None` means always enabled.
    ///
    /// Disabled semantics at dispatch time: the intent propagates past
    /// this action as if no match existed here, **unless** the firing
    /// [`Shortcut`](crate::shortcut::Shortcut) has
    /// `propagate_when_disabled == false`, in which case the intent
    /// is consumed (dormant) at this level.
    pub enabled_when: Option<Signal<bool>>,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("intent", &self.intent)
            .field("handler", &"<closure>")
            .field("enabled_when", &self.enabled_when.is_some())
            .finish()
    }
}

impl Action {
    /// Start building an [`Action`] bound to the given intent name.
    pub fn new(intent: &'static str) -> ActionBuilder {
        ActionBuilder {
            intent,
            handler: None,
            enabled_when: None,
        }
    }

    /// Resolve the current enabled state. `true` when no predicate is
    /// set; otherwise reads the signal.
    pub fn is_enabled(&self) -> bool {
        self.enabled_when.as_ref().map(|s| s.get()).unwrap_or(true)
    }
}

/// Fluent builder for [`Action`]. Use [`ActionBuilder::on_invoke`] for
/// the common case (handler consumes the intent) or
/// [`ActionBuilder::on_invoke_with_response`] when the handler needs
/// to decide whether to propagate.
pub struct ActionBuilder {
    intent: &'static str,
    handler: Option<ActionHandler>,
    enabled_when: Option<Signal<bool>>,
}

impl ActionBuilder {
    /// Reactive enabled-predicate. When the signal holds `false` the
    /// action is skipped during dispatch.
    pub fn enabled_when(mut self, signal: Signal<bool>) -> Self {
        self.enabled_when = Some(signal);
        self
    }

    /// Register a handler that consumes the intent. The handler's
    /// return value is ignored; the framework treats every invocation
    /// as [`IntentResponse::Handled`].
    pub fn on_invoke(
        mut self,
        mut f: impl FnMut(&Intent, &mut EventContext) + 'static,
    ) -> Action {
        self.handler = Some(Box::new(move |intent, ctx| {
            f(intent, ctx);
            IntentResponse::Handled
        }));
        self.finish()
    }

    /// Register a handler whose return value decides whether the
    /// intent propagates to ancestor widgets. Use when a widget wants
    /// to observe an intent (e.g., update a draft status) while still
    /// letting an ancestor perform the primary action.
    pub fn on_invoke_with_response(
        mut self,
        f: impl FnMut(&Intent, &mut EventContext) -> IntentResponse + 'static,
    ) -> Action {
        self.handler = Some(Box::new(f));
        self.finish()
    }

    fn finish(self) -> Action {
        Action {
            intent: self.intent,
            handler: self
                .handler
                .expect("ActionBuilder requires on_invoke or on_invoke_with_response"),
            enabled_when: self.enabled_when,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn action_builder_on_invoke_defaults_to_handled() {
        let fired = Rc::new(Cell::new(false));
        let fired_flag = fired.clone();
        let mut action = Action::new("app.save").on_invoke(move |_intent, _ctx| {
            fired_flag.set(true);
        });

        let mut ctx = EventContext::new();
        let intent = Intent::new("app.save");
        let response = (action.handler)(&intent, &mut ctx);
        assert_eq!(response, IntentResponse::Handled);
        assert!(fired.get());
        assert_eq!(action.intent, "app.save");
    }

    #[test]
    fn action_handler_receives_typed_payload() {
        let seen = Rc::new(Cell::new(0_i64));
        let seen_flag = seen.clone();
        let mut action = Action::new("tab.switch").on_invoke(move |intent, _ctx| {
            if let Some(&n) = intent.payload::<i64>() {
                seen_flag.set(n);
            }
        });

        let mut ctx = EventContext::new();
        let intent = Intent::with_payload("tab.switch", 5_i64);
        (action.handler)(&intent, &mut ctx);
        assert_eq!(seen.get(), 5);
    }

    #[test]
    fn action_with_response_can_propagate() {
        let mut action = Action::new("log.observe")
            .on_invoke_with_response(|_intent, _ctx| IntentResponse::Propagated);
        let mut ctx = EventContext::new();
        let intent = Intent::new("log.observe");
        let response = (action.handler)(&intent, &mut ctx);
        assert_eq!(response, IntentResponse::Propagated);
    }

    #[test]
    fn enabled_when_defaults_true() {
        let action = Action::new("edit.delete").on_invoke(|_, _| {});
        assert!(action.is_enabled());
    }

    #[test]
    fn enabled_when_follows_signal() {
        let enabled = Signal::new(false);
        let action = Action::new("edit.delete")
            .enabled_when(enabled.clone())
            .on_invoke(|_, _| {});
        assert!(!action.is_enabled());
        enabled.set(true);
        assert!(action.is_enabled());
    }
}
