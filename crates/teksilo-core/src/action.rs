// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use std::rc::Rc;

use crate::intent::{Intent, IntentResponse};
use crate::signal::Prop;
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
    pub enabled_when: Option<Prop<bool>>,
    /// Whether the action has anything to act on now, asked by what greys a
    /// menu row and never by dispatch. See
    /// [`ActionBuilder::available_when`]. `None` means available.
    available_when: Option<Availability>,
    /// Whether the handler passes the intent on now, asked by what greys a
    /// menu row. See [`ActionBuilder::passes_on_when`]. `None` means it
    /// never does.
    passes_on_when: Option<Availability>,
}

/// The predicate behind [`ActionBuilder::available_when`] and
/// [`ActionBuilder::passes_on_when`].
pub(crate) type Availability = Rc<dyn Fn() -> bool>;

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("intent", &self.intent)
            .field("handler", &"<closure>")
            .field("enabled_when", &self.enabled_when.is_some())
            .field("available_when", &self.available_when.is_some())
            .field("passes_on_when", &self.passes_on_when.is_some())
            .finish()
    }
}

impl Action {
    /// Start building an [`Action`] bound to the given intent name.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(intent: &'static str) -> ActionBuilder {
        ActionBuilder {
            intent,
            handler: None,
            enabled_when: None,
            available_when: None,
            passes_on_when: None,
        }
    }

    /// Resolve the current enabled state. `true` when no predicate is
    /// set; otherwise reads the signal.
    pub fn is_enabled(&self) -> bool {
        self.enabled_when.as_ref().map(|s| s.get()).unwrap_or(true)
    }

    /// Whether a menu row for this action's intent shows it enabled: the
    /// action is [enabled](Self::is_enabled) and
    /// [has something to act on](ActionBuilder::available_when) now.
    pub(crate) fn is_available(&self) -> bool {
        self.is_enabled()
            && self
                .available_when
                .as_ref()
                .is_none_or(|available| available())
    }

    /// Whether a dispatch reaching this action now would go past it: it
    /// is disabled, or its handler [passes the intent on](ActionBuilder::passes_on_when).
    /// The first action on a route this answers `false` for is the one a
    /// sent intent stops at, and the one a menu row asks.
    pub(crate) fn passes_on(&self) -> bool {
        !self.is_enabled()
            || self
                .passes_on_when
                .as_ref()
                .is_some_and(|passes_on| passes_on())
    }

    /// The [availability predicate](ActionBuilder::available_when), shared.
    pub(crate) fn availability(&self) -> Option<Availability> {
        self.available_when.clone()
    }

    /// The [pass-on predicate](ActionBuilder::passes_on_when), shared.
    pub(crate) fn pass_on(&self) -> Option<Availability> {
        self.passes_on_when.clone()
    }
}

/// Fluent builder for [`Action`]. Use [`ActionBuilder::on_invoke`] for
/// the common case (handler consumes the intent) or
/// [`ActionBuilder::on_invoke_with_response`] when the handler needs
/// to decide whether to propagate.
pub struct ActionBuilder {
    intent: &'static str,
    handler: Option<ActionHandler>,
    enabled_when: Option<Prop<bool>>,
    available_when: Option<Availability>,
    passes_on_when: Option<Availability>,
}

impl ActionBuilder {
    /// Reactive enabled-predicate. When the signal holds `false` the
    /// action is skipped during dispatch.
    pub fn enabled_when(mut self, signal: impl Into<Prop<bool>>) -> Self {
        self.enabled_when = Some(signal.into());
        self
    }

    /// Whether the action has anything to act on now, asked each time a
    /// menu row for its intent decides whether to grey out
    /// ([`WidgetTree::can_handle`](crate::WidgetTree::can_handle),
    /// [`WidgetTree::handled_intents_from`](crate::WidgetTree::handled_intents_from)):
    /// a text field's Copy with nothing selected, its Paste while read-only.
    /// The row greys out while it answers `false`.
    ///
    /// Dispatch never asks it: an intent that reaches the action runs its
    /// handler whatever `available` answers, and the handler decides, as
    /// one built with [`on_invoke_with_response`](Self::on_invoke_with_response)
    /// does. So a shortcut behaves the same with or without it, unlike
    /// [`enabled_when`](Self::enabled_when), which passes the intent by.
    /// `available` runs outside every event handler, on every query (once
    /// per event-loop turn for the native menu bar), and must be cheap.
    #[doc(hidden)]
    pub fn available_when(mut self, available: impl Fn() -> bool + 'static) -> Self {
        self.available_when = Some(Rc::new(available));
        self
    }

    /// Whether the handler, built with
    /// [`on_invoke_with_response`](Self::on_invoke_with_response), would
    /// pass the intent on if it ran now (a read-only field's Cut, an
    /// editor's Undo with nothing to undo when the tree passes those on).
    /// It must answer as the handler decides: what greys a menu row asks it
    /// to find the action a dispatch would stop at, which is the first
    /// enabled action on the route this answers `false` for, and asks that
    /// action's [availability](Self::available_when). So a field with
    /// nothing selected, under a page with its own Copy, greys Copy (Copy
    /// chosen there reaches the field and stops), and a read-only field
    /// leaves Cut to whatever its Cut goes on to.
    ///
    /// Dispatch never asks it: the handler alone decides. Like `available`,
    /// it runs outside every event handler, on every query, and must be
    /// cheap. Without it, the action is taken never to pass anything on.
    #[doc(hidden)]
    pub fn passes_on_when(mut self, passes_on: impl Fn() -> bool + 'static) -> Self {
        self.passes_on_when = Some(Rc::new(passes_on));
        self
    }

    /// Register a handler that consumes the intent. The handler's
    /// return value is ignored; the framework treats every invocation
    /// as [`IntentResponse::Handled`].
    pub fn on_invoke(mut self, mut f: impl FnMut(&Intent, &mut EventContext) + 'static) -> Action {
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
            available_when: self.available_when,
            passes_on_when: self.passes_on_when,
        }
    }
}

/// The intents the application tree's actions take, as a window's tree sees
/// them.
///
/// A shortcut whose intent nothing in its window takes is offered to the
/// application tree when that tree takes it, and its key falls through to the
/// focused widget otherwise. The window decides while it dispatches the key,
/// when the application tree cannot be borrowed, so `teksilo-app` shares one
/// of these between the application tree, which fills it whenever it has run
/// ([`WidgetTree::publish_application_scope`](crate::WidgetTree::publish_application_scope)),
/// and every window's tree, which reads it
/// ([`WidgetTree::set_application_scope`](crate::WidgetTree::set_application_scope)).
///
/// It keeps each action's [enabled predicate](ActionBuilder::enabled_when),
/// not its value, so an action enabled or disabled since the last publish is
/// answered for as it is now, and its
/// [availability](ActionBuilder::available_when) and
/// [pass-on predicate](ActionBuilder::passes_on_when) the same way, for what
/// greys a menu row. An action that may decline when it runs
/// ([`on_invoke_with_response`](ActionBuilder::on_invoke_with_response))
/// without saying when counts as taking the intent.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct ApplicationScope {
    actions: Rc<std::cell::RefCell<Vec<ScopedAction>>>,
}

/// One action of an [`ApplicationScope`]: its intent name, its enabled
/// predicate, its availability and its pass-on predicate.
pub(crate) type ScopedAction = (
    &'static str,
    Option<Prop<bool>>,
    Option<Availability>,
    Option<Availability>,
);

impl ApplicationScope {
    /// An empty scope: the application takes nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an enabled application action takes intents named `name`:
    /// what decides where a shortcut's intent goes. Availability does not
    /// count, as it does not in a dispatch.
    pub fn takes(&self, name: &str) -> bool {
        self.actions
            .borrow()
            .iter()
            .any(|(intent, enabled, _, _)| *intent == name && Self::enabled(enabled))
    }

    /// Whether a menu row for intents named `name` shows enabled for what
    /// the application takes: the first enabled application action for it
    /// that does not [pass it on](ActionBuilder::passes_on_when), the one an
    /// intent sent there stops at,
    /// [has something to act on](ActionBuilder::available_when).
    pub(crate) fn offers(&self, name: &str) -> bool {
        self.actions
            .borrow()
            .iter()
            .find(|action| action.0 == name && Self::stops(action))
            .is_some_and(Self::available)
    }

    /// Every intent name the application [offers](Self::offers), in the
    /// order they were published, each once: what greys the menu rows.
    pub fn taken(&self) -> Vec<&'static str> {
        let actions = self.actions.borrow();
        let mut decided: Vec<(&'static str, bool)> = Vec::new();
        for action in actions.iter().filter(|action| Self::stops(action)) {
            if decided.iter().all(|(name, _)| *name != action.0) {
                decided.push((action.0, Self::available(action)));
            }
        }
        decided
            .into_iter()
            .filter_map(|(name, available)| available.then_some(name))
            .collect()
    }

    fn enabled(enabled: &Option<Prop<bool>>) -> bool {
        enabled.as_ref().is_none_or(|enabled| enabled.get())
    }

    /// Whether an intent reaching `action` stops there: it is enabled and
    /// does not pass the intent on now.
    fn stops((_, enabled, _, passes_on): &ScopedAction) -> bool {
        Self::enabled(enabled) && passes_on.as_ref().is_none_or(|passes_on| !passes_on())
    }

    fn available((_, _, available, _): &ScopedAction) -> bool {
        available.as_ref().is_none_or(|available| available())
    }

    /// Replace what the application takes with `actions`, intent names
    /// with their enabled predicates, availability and pass-on predicates.
    pub(crate) fn replace(&self, actions: Vec<ScopedAction>) {
        *self.actions.borrow_mut() = actions;
    }
}

impl std::fmt::Debug for ApplicationScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let actions = self.actions.borrow();
        f.debug_list()
            .entries(actions.iter().map(|(intent, _, _, _)| intent))
            .finish()
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
        let enabled = crate::signal::Signal::new(false);
        let action = Action::new("edit.delete")
            .enabled_when(enabled.clone())
            .on_invoke(|_, _| {});
        assert!(!action.is_enabled());
        enabled.set(true);
        assert!(action.is_enabled());
    }
}
