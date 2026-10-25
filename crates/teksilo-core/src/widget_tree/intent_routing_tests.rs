// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a tree leaves unclaimed, and an intent routed from another tree.
//!
//! `teksilo-app` offers an intent no action of one tree took to one other
//! tree: from a window to the application tree, or the other way. The tree
//! keeps what it left unclaimed when asked to, a shortcut's only when the
//! application tree takes it (its key falls through to the focused widget
//! otherwise), and dispatches a routed intent from its focus without
//! counting it again.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Size, SizeProposal};

use crate::event::{EventResponse, Key, Modifiers, WidgetEvent};
use crate::event_source::TreeAppContext;
use crate::shortcut::{KeyStroke, Shortcut};
use crate::telemetry::{Event, IntentSource, TelemetryContext, UsageReporter};
use crate::test_widgets::FillWidget;
use crate::window::NoopWindowOps;
use crate::{
    Action, ApplicationScope, BuildContext, Intent, LayoutContext, LayoutResponse, Signal, Widget,
    WidgetId,
};

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
fn a_shortcuts_intent_the_application_does_not_take_is_not_kept() {
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.set_application_scope(Some(ApplicationScope::new()));
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown").with_source(IntentSource::Shortcut));
    });
    assert!(tree.take_unclaimed_intents().is_empty());
}

#[test]
fn a_shortcuts_intent_the_application_takes_is_kept() {
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.set_application_scope(Some(application_taking("app.new")));
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.new").with_source(IntentSource::Shortcut));
    });
    assert_eq!(names(&tree.take_unclaimed_intents()), ["app.new"]);
}

/// The scope of an application tree whose root answers `name` with a
/// scoped action.
fn application_taking(name: &'static str) -> ApplicationScope {
    #[derive(Debug)]
    struct AppRoot(&'static str);
    impl Widget for AppRoot {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.register_action(Action::new(self.0).on_invoke(|_, _| {}));
            Vec::new()
        }
        fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            Size::new(0.0, 0.0).into()
        }
    }
    let mut app = WidgetTree::new();
    app.add(AppRoot(name));
    let scope = ApplicationScope::new();
    app.publish_application_scope(&scope);
    scope
}

/// A focused widget recording the keys it hears, under a window root that
/// binds Ctrl+N to `app.new` with a global shortcut and has no action for
/// it.
#[derive(Debug)]
struct ShortcutWindow {
    keys: Rc<RefCell<Vec<Key>>>,
    field: Rc<Cell<Option<WidgetId>>>,
}

impl Widget for ShortcutWindow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::widget_builder::WidgetBuilder;
        ctx.register_shortcut_global(
            Shortcut::new("app.new")
                .primary(KeyStroke::ctrl(Key::N))
                .build(),
        );
        let keys = self.keys.clone();
        let field = ctx.add(FillWidget::new().focusable().on_key(move |event, _| {
            if let WidgetEvent::KeyDown { key, .. } = event {
                keys.borrow_mut().push(*key);
            }
            EventResponse::Handled
        }));
        self.field.set(Some(field));
        vec![field]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(200.0, 100.0).into()
    }
}

/// A window tree keeping what it leaves, its focused field, and the keys
/// the field heard.
fn shortcut_window(scope: ApplicationScope) -> (WidgetTree, Rc<RefCell<Vec<Key>>>) {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let field = Rc::new(Cell::new(None));
    let mut tree = WidgetTree::new();
    tree.set_records_unclaimed_intents(true);
    tree.set_application_scope(Some(scope));
    tree.add(ShortcutWindow {
        keys: keys.clone(),
        field: field.clone(),
    });
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.focus(field.get().expect("built"));
    (tree, keys)
}

#[test]
fn a_shortcut_nothing_in_the_window_takes_goes_to_the_application() {
    let (mut tree, keys) = shortcut_window(application_taking("app.new"));
    tree.press_key(Key::N, Modifiers::COMMAND);
    assert_eq!(
        names(&tree.take_unclaimed_intents()),
        ["app.new"],
        "kept for the application tree"
    );
    assert!(
        keys.borrow().is_empty(),
        "and the key is the shortcut's, not the focused widget's"
    );
}

#[test]
fn a_shortcut_nobody_takes_falls_through_to_the_focused_widget() {
    // `edit.copy` is bound to Ctrl+C in every tree; nothing here, nor in
    // the application tree, handles it.
    let (mut tree, keys) = shortcut_window(application_taking("app.new"));
    tree.press_key(Key::C, Modifiers::COMMAND);
    assert_eq!(*keys.borrow(), [Key::C]);
    assert!(tree.take_unclaimed_intents().is_empty());
}

#[test]
fn a_disabled_application_action_takes_no_shortcut() {
    #[derive(Debug)]
    struct AppRoot(Signal<bool>);
    impl Widget for AppRoot {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.register_action(
                Action::new("app.new")
                    .enabled_when(self.0.clone())
                    .on_invoke(|_, _| {}),
            );
            Vec::new()
        }
        fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            Size::new(0.0, 0.0).into()
        }
    }
    let enabled = Signal::new(false);
    let mut app = WidgetTree::new();
    app.add(AppRoot(enabled.clone()));
    let scope = ApplicationScope::new();
    app.publish_application_scope(&scope);
    let (mut tree, keys) = shortcut_window(scope);
    tree.press_key(Key::N, Modifiers::COMMAND);
    assert_eq!(*keys.borrow(), [Key::N], "disabled: the key falls through");
    enabled.set(true);
    tree.press_key(Key::N, Modifiers::COMMAND);
    assert_eq!(
        names(&tree.take_unclaimed_intents()),
        ["app.new"],
        "enabled since, with nothing published again"
    );
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

/// Answers `app.scoped` with a scoped action, as an app's root does.
#[derive(Debug)]
struct ScopedRoot {
    calls: Rc<RefCell<Vec<&'static str>>>,
}

impl Widget for ScopedRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action(Action::new("app.scoped").on_invoke(move |intent, _| {
            calls.borrow_mut().push(intent.name);
        }));
        Vec::new()
    }

    fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}

#[test]
fn with_nothing_focused_a_routed_intent_reaches_the_app_root_under_a_wrapper() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    let content = tree.add(ScopedRoot {
        calls: calls.clone(),
    });
    // A post-root hook's wrapper (the debug inspector's), the first root.
    tree.add(crate::test_widgets::StackWidget::new().child(content));
    tree.set_content_root(content);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(tree.dispatch_routed_intent(Intent::new("app.scoped"), &mut NoopWindowOps));
    assert_eq!(*calls.borrow(), ["app.scoped"]);
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

/// A tree standing for window `id`, keeping what it leaves.
fn window_tree(id: u64) -> WidgetTree {
    use crate::window::state::WindowStateInit;
    use crate::window::{TeksiloWindowId, WindowPlacement, WindowState};
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.set_window_state(WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(id),
        string_id: None,
        placement: WindowPlacement::Floating,
        title: String::new(),
        size: (100, 100),
        position: (0, 0),
        focused: true,
        resizable: true,
        always_on_top: false,
    }));
    tree
}

/// Answers `app.which` by recording the window the intent came from.
#[derive(Debug)]
struct Which(Rc<RefCell<Vec<Option<crate::window::TeksiloWindowId>>>>);

impl Widget for Which {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let seen = self.0.clone();
        ctx.register_action_global(Action::new("app.which").on_invoke(move |intent, _| {
            seen.borrow_mut().push(intent.origin_window());
        }));
        Vec::new()
    }

    fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}

#[test]
fn an_intent_names_the_window_it_was_sent_in_after_crossing() {
    use crate::window::TeksiloWindowId;
    let mut window = window_tree(7);
    window.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.which"));
    });
    let kept = window.take_unclaimed_intents();
    assert_eq!(kept[0].origin_window(), Some(TeksiloWindowId::new(7)));

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut app = WidgetTree::new();
    app.add(Which(seen.clone()));
    for intent in kept {
        assert!(app.dispatch_routed_intent(intent, &mut NoopWindowOps));
    }
    assert_eq!(*seen.borrow(), [Some(TeksiloWindowId::new(7))]);
}

#[test]
fn a_shortcuts_intent_kept_for_the_application_names_its_window() {
    use crate::window::TeksiloWindowId;
    let mut window = window_tree(3);
    window.set_application_scope(Some(application_taking("app.which")));
    window.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.which").with_source(IntentSource::Shortcut));
    });
    assert_eq!(
        window.take_unclaimed_intents()[0].origin_window(),
        Some(TeksiloWindowId::new(3))
    );
}

#[test]
fn an_intent_sent_in_a_tree_with_no_window_names_none() {
    let (mut tree, _) = tree();
    tree.set_records_unclaimed_intents(true);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.unknown"));
    });
    assert_eq!(tree.take_unclaimed_intents()[0].origin_window(), None);
}

/// Window operations whose event carried `token`, recording what a routed
/// intent holds and releases.
#[derive(Default)]
struct TokenOps {
    token: Option<String>,
    log: Vec<String>,
}

impl crate::window::WindowOps for TokenOps {
    fn open_window(&mut self, _: crate::window::WindowConfig) -> crate::window::TeksiloWindowId {
        unreachable!()
    }
    fn find_window(&self, _: &str) -> Option<crate::window::TeksiloWindowId> {
        None
    }
    fn window_state(
        &self,
        _: crate::window::TeksiloWindowId,
    ) -> Option<crate::window::WindowState> {
        None
    }
    fn windows(&self) -> Vec<crate::window::WindowState> {
        Vec::new()
    }
    fn focus_window(&mut self, _: crate::window::TeksiloWindowId) {}
    fn close_window_by_id(&mut self, _: crate::window::TeksiloWindowId) {}
    fn take_activation_token(&mut self) -> Option<String> {
        self.token.take()
    }
    fn hold_activation_token(&mut self, token: String) {
        self.log.push(format!("hold {token}"));
    }
    fn release_activation_token(&mut self) {
        self.log.push("release".into());
    }
}

#[test]
fn an_intent_kept_while_its_events_token_is_unused_carries_it_across() {
    let mut window = window_tree(1);
    let mut ops = TokenOps {
        token: Some("tok".into()),
        ..TokenOps::default()
    };
    window.run_with_event_context(&mut ops, |ctx| {
        ctx.send_intent(Intent::new("app.which"));
    });
    let kept = window.take_unclaimed_intents();
    assert_eq!(kept[0].activation_token(), Some("tok"));

    let mut app = WidgetTree::new();
    app.add(Which(Rc::default()));
    let mut ops = TokenOps::default();
    for intent in kept {
        app.dispatch_routed_intent(intent, &mut ops);
    }
    assert_eq!(
        ops.log,
        ["hold tok", "release"],
        "held for the actions, released after"
    );
}

/// The token is the event's, good for one activation: of two intents kept
/// in one dispatch, only the first takes it.
#[test]
fn only_the_first_intent_kept_takes_the_events_token() {
    let mut window = window_tree(1);
    let mut ops = TokenOps {
        token: Some("tok".into()),
        ..TokenOps::default()
    };
    window.run_with_event_context(&mut ops, |ctx| {
        ctx.send_intent(Intent::new("app.which"));
        ctx.send_intent(Intent::new("app.which"));
    });
    let kept = window.take_unclaimed_intents();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].activation_token(), Some("tok"));
    assert_eq!(kept[1].activation_token(), None, "taken by the first");
    assert_eq!(ops.token, None, "and no longer the event's to use");
}

#[test]
fn an_intent_with_no_token_holds_none() {
    let mut window = window_tree(1);
    window.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("app.which"));
    });
    let mut app = WidgetTree::new();
    app.add(Which(Rc::default()));
    let mut ops = TokenOps::default();
    for intent in window.take_unclaimed_intents() {
        assert_eq!(intent.activation_token(), None);
        app.dispatch_routed_intent(intent, &mut ops);
    }
    assert!(ops.log.is_empty());
}

/// Answers `on` by sending `then`, counting its calls.
#[derive(Debug)]
struct Echo {
    on: &'static str,
    then: &'static str,
    calls: Rc<Cell<u32>>,
}

impl Widget for Echo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let (then, calls) = (self.then, self.calls.clone());
        ctx.register_action_global(Action::new(self.on).on_invoke(move |_, ctx| {
            calls.set(calls.get() + 1);
            ctx.send_intent(Intent::new(then));
        }));
        Vec::new()
    }

    fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}

/// Two trees whose actions answer each other across the crossing (one
/// takes `ping` and sends `pong`, the other the reverse) stop after a
/// bounded number of crossings, however long they are offered.
#[test]
fn two_trees_answering_each_other_stop_after_a_bounded_number_of_crossings() {
    let calls = Rc::new(Cell::new(0));
    let echo = |on, then| {
        let mut tree = WidgetTree::new();
        tree.set_records_unclaimed_intents(true);
        tree.add(Echo {
            on,
            then,
            calls: calls.clone(),
        });
        tree
    };
    let mut a = echo("ping", "pong");
    let mut b = echo("pong", "ping");
    a.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent(Intent::new("pong"));
    });
    let mut rounds = 0;
    loop {
        let from_a = a.take_unclaimed_intents();
        let from_b = b.take_unclaimed_intents();
        if from_a.is_empty() && from_b.is_empty() {
            break;
        }
        for intent in from_a {
            b.dispatch_routed_intent(intent, &mut NoopWindowOps);
        }
        for intent in from_b {
            a.dispatch_routed_intent(intent, &mut NoopWindowOps);
        }
        rounds += 1;
        assert!(rounds < 1000, "the echo never stopped");
    }
    assert_eq!(
        calls.get() as usize,
        super::MAX_INTENT_CROSSINGS,
        "one call per crossing, up to the bound"
    );
}
