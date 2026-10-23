// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a tree leaves unclaimed, and an intent routed from another tree.
//!
//! `teksilo-app` offers an intent no action of one tree took to one other
//! tree: from a window to the application tree, or the other way. The tree
//! keeps what it left unclaimed when asked to, never a shortcut's, and
//! dispatches a routed intent from its focus without counting it again.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Size, SizeProposal};

use crate::event_source::TreeAppContext;
use crate::telemetry::{Event, IntentSource, TelemetryContext, UsageReporter};
use crate::window::NoopWindowOps;
use crate::{Action, BuildContext, Intent, LayoutContext, LayoutResponse, Widget, WidgetId};

use super::WidgetTree;

/// Registers a global action for `app.known`, which records its calls and,
/// for one of them, sends `app.followup`, which nothing takes.
#[derive(Debug)]
struct Root {
    calls: Rc<RefCell<Vec<&'static str>>>,
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action_global(Action::new("app.known").on_invoke(move |intent, ctx| {
            calls.borrow_mut().push(intent.name);
            ctx.send_intent(Intent::new("app.followup"));
        }));
        Vec::new()
    }

    fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}

fn tree() -> (WidgetTree, Rc<RefCell<Vec<&'static str>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    tree.add(Root {
        calls: calls.clone(),
    });
    (tree, calls)
}

fn names(intents: &[Intent]) -> Vec<&'static str> {
    intents.iter().map(|intent| intent.name).collect()
}

#[test]
fn nothing_is_kept_unless_asked() {
    let (mut tree, _) = tree();
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown"));
    });
    assert!(tree.take_unclaimed_intents().is_empty());
}

#[test]
fn an_intent_no_action_takes_is_kept_once() {
    let (mut tree, calls) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown"));
        ctx.send_intent(Intent::new("app.known"));
    });
    assert_eq!(*calls.borrow(), ["app.known"]);
    assert!(tree.has_unclaimed_intents());
    assert_eq!(
        names(&tree.take_unclaimed_intents()),
        ["app.unknown", "app.followup"],
        "the taken one is not kept; what its handler sent and nothing took is"
    );
    assert!(tree.take_unclaimed_intents().is_empty(), "taken once");
    assert!(!tree.has_unclaimed_intents());
}

#[test]
fn a_shortcuts_intent_is_never_kept() {
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown").with_source(IntentSource::Shortcut));
    });
    assert!(tree.take_unclaimed_intents().is_empty());
}

#[test]
fn turning_recording_off_drops_what_was_kept() {
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown"));
    });
    tree.set_records_unclaimed_intents(false);
    assert!(tree.take_unclaimed_intents().is_empty());
}

#[test]
fn a_routed_intent_reaches_this_trees_actions() {
    let (mut tree, calls) = tree();
    tree.set_records_unclaimed_intents(true);
    assert!(tree.dispatch_routed_intent(Intent::new("app.known"), &mut NoopWindowOps));
    assert_eq!(*calls.borrow(), ["app.known"]);
    assert_eq!(
        names(&tree.take_unclaimed_intents()),
        ["app.followup"],
        "what its handler sent is this tree's own, kept when nothing took it"
    );
}

#[test]
fn a_routed_intent_nothing_takes_is_not_kept_again() {
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    assert!(!tree.dispatch_routed_intent(Intent::new("app.unknown"), &mut NoopWindowOps));
    assert!(
        tree.take_unclaimed_intents().is_empty(),
        "an intent is offered to one other tree at most"
    );
}

#[test]
fn an_empty_tree_takes_no_routed_intent() {
    let mut tree = WidgetTree::new();
    assert!(!tree.dispatch_routed_intent(Intent::new("app.known"), &mut NoopWindowOps));
}

/// Counts `intent.dispatched` events.
struct Counting(Rc<Cell<u32>>);

impl UsageReporter for Counting {
    fn record(&self, event: &Event<'_>) {
        if event.name == "intent.dispatched" {
            self.0.set(self.0.get() + 1);
        }
    }

    fn adapter_name(&self) -> &'static str {
        "counting"
    }

    fn endpoint(&self) -> &str {
        ""
    }
}

/// Telemetry counts an intent where it was sent: a routed one was counted
/// in the tree that sent it, so its dispatch here is not.
#[test]
fn a_routed_intent_is_not_counted_again() {
    let (mut tree, _) = tree();
    let counted = Rc::new(Cell::new(0));
    let mut app_state: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
    app_state.insert(
        TypeId::of::<TelemetryContext>(),
        Box::new(TelemetryContext {
            reporter: Rc::new(Counting(counted.clone())),
            session_id: String::new(),
            schema_version: 1,
        }),
    );
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(app_state)));
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown"));
    });
    assert_eq!(counted.get(), 1, "counted where it was sent");
    tree.dispatch_routed_intent(Intent::new("app.known"), &mut NoopWindowOps);
    assert_eq!(
        counted.get(),
        2,
        "the routed intent is not counted; the followup its handler sent is"
    );
}
