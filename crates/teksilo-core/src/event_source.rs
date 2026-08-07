// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Backend event subscription infrastructure (architecture §9.4).
//!
//! Widgets subscribe to external event sources (database change notifiers,
//! file watchers, message buses, network response channels) directly from
//! their `build()` method via [`crate::BuildContext::subscribe_event`]. The
//! framework bridges events from the publisher's thread to the UI thread via
//! the application's event-loop proxy and routes them to the widget's
//! UI-side callback, with automatic per-widget lifetime cleanup.
//!
//! This module defines the public [`EventSource`] trait, the opaque
//! [`SubscriptionHandle`] returned by sources, and the internal
//! [`TreeAppContext`] / [`EventSourceAdapter`] / [`AppEventPoster`] types that
//! plug a registered source into the tree.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::widget::EventContext;
use crate::window::TeksiloWindowId;

/// An external source of events that widgets can subscribe to.
///
/// Implementations include backend message buses, database change notifiers,
/// file watchers, network response channels — any source that publishes
/// events asynchronously and that widgets need to react to.
pub trait EventSource: 'static {
    /// The key by which subscribers identify which events they care about.
    /// Typically an enum (a Qleany `Origin`) or a topic string.
    type Origin: Clone + 'static;

    /// The event payload delivered to subscriber callbacks. Must be `Send`
    /// because events cross from the publisher's thread to the UI thread via
    /// the framework's proxy bridge.
    type Event: Send + 'static;

    /// Subscribe a callback to events of a given origin. The callback is
    /// invoked on whatever thread the source publishes from (typically a
    /// background thread). The returned handle, when dropped, removes the
    /// subscription from the source's internal registry.
    fn subscribe(
        &self,
        origin: Self::Origin,
        callback: Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
    ) -> SubscriptionHandle;
}

/// An opaque handle returned by [`EventSource::subscribe`].
///
/// The source defines what the handle contains; the framework treats it as a
/// token whose `Drop` impl performs the unsubscription. Returning an empty
/// handle (via [`SubscriptionHandle::empty`]) is acceptable for sources that
/// outlive the application or do not support removal.
pub struct SubscriptionHandle {
    _inner: Box<dyn Any>,
}

impl SubscriptionHandle {
    /// Wrap an arbitrary value as a subscription handle. The value is dropped
    /// when the handle is dropped — typically that drop performs removal from
    /// the source's internal subscriber registry.
    pub fn new<T: 'static>(token: T) -> Self {
        Self {
            _inner: Box::new(token),
        }
    }

    /// A handle that performs no cleanup on drop. Use this for sources that
    /// outlive the application or whose subscribers cannot be individually
    /// removed.
    pub fn empty() -> Self {
        Self::new(())
    }
}

/// A unique identifier for a subscription installed via
/// [`crate::BuildContext::subscribe_event`].
///
/// Used internally to look up the UI-side callback when a posted event
/// arrives back on the UI thread, and to key the per-widget cleanup scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub(crate) u64);

/// Posts events from background threads back to the UI thread.
///
/// teksilo-core cannot depend on winit or teksilo-app, so this trait acts as the
/// boundary: teksilo-app provides an implementation that wraps the
/// `EventLoopProxy<AppEvent>` and converts the calls into the
/// matching `AppEvent::*` user-event variants.
///
/// Two posting paths share this trait:
///
/// - `post_subscription_event` — backend events for widgets that
///   subscribed via [`BuildContext::subscribe_event`](crate::build_context::BuildContext).
/// - `post_external` — arbitrary typed payloads delivered as
///   [`AppEvent::External`](crate::app_event::AppEvent::External). Used
///   by async OS-driven integrations (file dialogs, future background
///   tasks) that resolve off the UI thread and need to deliver typed
///   results back to the main loop.
pub trait AppEventPoster: Send + Sync + 'static {
    fn post_subscription_event(&self, sub_id: SubscriptionId, event: Box<dyn Any + Send>);

    /// Post an arbitrary typed payload as `AppEvent::External(_)`.
    /// Default body is a no-op so existing implementations stay
    /// source-compatible; the real implementation in teksilo-app
    /// forwards to `EventLoopProxy::send_event`.
    fn post_external(&self, _payload: Box<dyn Any + Send>) {}
}

/// Type-erased wrapper around a registered [`EventSource`].
///
/// The generic source `S` is consumed when the adapter is constructed via
/// [`EventSourceAdapter::new`]; only erased closures and `TypeId`s remain.
/// This lets `WidgetTree` / `BuildContext` reach the source without becoming
/// generic over `S`.
pub struct EventSourceAdapter {
    pub(crate) origin_type: TypeId,
    pub(crate) origin_type_name: &'static str,
    pub(crate) event_type: TypeId,
    pub(crate) event_type_name: &'static str,
    #[allow(clippy::type_complexity)]
    pub(crate) subscribe_fn: Box<
        dyn Fn(
            Box<dyn Any>,
            Arc<dyn Fn(Box<dyn Any + Send>) + Send + Sync + 'static>,
        ) -> SubscriptionHandle,
    >,
}

impl EventSourceAdapter {
    /// Build an adapter from a concrete event source. Called by
    /// `TeksiloAppBuilder::event_source`.
    pub fn new<S: EventSource>(source: S) -> Self {
        let source = Arc::new(source);
        let origin_type = TypeId::of::<S::Origin>();
        let origin_type_name = std::any::type_name::<S::Origin>();
        let event_type = TypeId::of::<S::Event>();
        let event_type_name = std::any::type_name::<S::Event>();

        let subscribe_fn: Box<
            dyn Fn(
                Box<dyn Any>,
                Arc<dyn Fn(Box<dyn Any + Send>) + Send + Sync + 'static>,
            ) -> SubscriptionHandle,
        > = Box::new(move |erased_origin, framework_wrapper| {
            let origin: Box<S::Origin> = erased_origin
                .downcast::<S::Origin>()
                .expect("origin type mismatch — framework bug");

            // Wrap the framework's `Fn(Box<dyn Any + Send>)` into the
            // `Fn(S::Event)` shape the source expects. The wrapper boxes
            // the typed event and forwards to the framework's poster.
            let typed_callback: Arc<dyn Fn(S::Event) + Send + Sync + 'static> =
                Arc::new(move |event: S::Event| {
                    let erased: Box<dyn Any + Send> = Box::new(event);
                    framework_wrapper(erased);
                });

            source.subscribe(*origin, typed_callback)
        });

        Self {
            origin_type,
            origin_type_name,
            event_type,
            event_type_name,
            subscribe_fn,
        }
    }
}

/// UI-side callback for a *context-bearing* subscription
/// ([`BuildContext::subscribe_event_with_ctx`](crate::build_context::BuildContext::subscribe_event_with_ctx)):
/// it receives the downcast event **and** a fresh [`EventContext`], so it can
/// imperatively drive toasts, modals, intents and navigation in reaction to a
/// backend event — the things a plain, context-free `subscribe_event` callback
/// cannot. teksilo-app invokes it from inside
/// [`WidgetTree::run_with_event_context`](crate::WidgetTree::run_with_event_context),
/// keyed by the originating window so the context binds to the right tree.
///
/// Stored behind `Rc` (not `Box`) so dispatch can **clone the handle and release
/// the map borrow before invoking** — the callback runs arbitrary UI code (it
/// may `open_window`, which synchronously builds a new tree whose widgets can
/// register *their own* context-bearing subscriptions, i.e. re-enter this very
/// map). Holding the borrow across the call would `BorrowMutError`-panic there.
type CtxSubscriptionCallback = Rc<dyn Fn(&dyn Any, &mut EventContext)>;

/// Per-tree app-level subscription state.
///
/// Held by `WidgetTree` as `Rc<TreeAppContext>` so that `BuildContext` can
/// reach the event source adapter, allocate subscription ids, and post the
/// UI-side callback into the lookup map. Fields are `RefCell`/`Cell`
/// because the tree borrows itself mutably during `build()` and we need
/// shared interior access.
pub struct TreeAppContext {
    pub(crate) poster: Option<Arc<dyn AppEventPoster>>,
    pub(crate) event_source: Option<EventSourceAdapter>,
    #[allow(clippy::type_complexity)]
    pub(crate) subscription_callbacks: RefCell<HashMap<SubscriptionId, Box<dyn Fn(&dyn Any)>>>,
    /// Context-bearing subscription callbacks + the window they target.
    /// Populated by [`BuildContext::subscribe_event_with_ctx`](crate::build_context::BuildContext::subscribe_event_with_ctx);
    /// dispatched by teksilo-app with a freshly-minted [`EventContext`]. A given
    /// `SubscriptionId` lives in exactly one of the two callback maps.
    ///
    /// The window is `Option` because a subscription registered from a windowless
    /// tree (headless / tests) has no tree to mint an `EventContext` from — the
    /// app-side router then can't deliver it (real app widgets always have a
    /// window). Direct [`dispatch_subscription_event_with_ctx`](Self::dispatch_subscription_event_with_ctx)
    /// is window-agnostic, which is what unit tests drive.
    #[allow(clippy::type_complexity)]
    pub(crate) subscription_ctx_callbacks:
        RefCell<HashMap<SubscriptionId, (Option<TeksiloWindowId>, CtxSubscriptionCallback)>>,
    pub(crate) next_subscription_id: Cell<u64>,
    /// Application-scoped values keyed by `TypeId`.
    /// Populated at builder time, read-only after the tree starts running.
    pub(crate) app_state: HashMap<TypeId, Box<dyn Any>>,
}

impl TreeAppContext {
    /// Empty context — no event source, no proxy poster. Used by tests
    /// and by `WidgetTree::new()`.
    pub fn empty() -> Self {
        Self {
            poster: None,
            event_source: None,
            subscription_callbacks: RefCell::new(HashMap::new()),
            subscription_ctx_callbacks: RefCell::new(HashMap::new()),
            next_subscription_id: Cell::new(1),
            app_state: HashMap::new(),
        }
    }

    /// Build a context with both a registered event source and a proxy
    /// poster. Called by `teksilo-app` when constructing a window for an
    /// application that registered an event source on the builder.
    pub fn with_source_and_poster(
        event_source: EventSourceAdapter,
        poster: Arc<dyn AppEventPoster>,
    ) -> Self {
        Self {
            poster: Some(poster),
            event_source: Some(event_source),
            subscription_callbacks: RefCell::new(HashMap::new()),
            subscription_ctx_callbacks: RefCell::new(HashMap::new()),
            next_subscription_id: Cell::new(1),
            app_state: HashMap::new(),
        }
    }

    /// Install an app-state registry. Consumes `self`
    /// and returns a new context with the registry attached; the builder
    /// calls this after constructing the context and before wrapping it
    /// in `Rc`.
    pub fn with_app_state(mut self, registry: HashMap<TypeId, Box<dyn Any>>) -> Self {
        self.app_state = registry;
        self
    }

    /// Install an [`AppEventPoster`] so background work (file dialogs,
    /// future async-result features) can post typed payloads back to
    /// the UI loop via `AppEvent::External`. The builder calls this
    /// unconditionally during `TeksiloAppBuilder::run` — the poster is
    /// cheap (a thin wrapper around the event-loop proxy) and being
    /// reachable means widgets do not have to depend on the event-source
    /// feature for unrelated async-result delivery.
    pub fn with_poster(mut self, poster: Arc<dyn AppEventPoster>) -> Self {
        self.poster = Some(poster);
        self
    }

    /// Borrow the registered [`AppEventPoster`] if one was installed.
    /// Used by integrations that need to post typed payloads back to
    /// the UI loop from an external thread (e.g. file-dialog backends).
    pub fn poster(&self) -> Option<&Arc<dyn AppEventPoster>> {
        self.poster.as_ref()
    }

    /// Look up an app-state value of type `T` previously registered via
    /// `TeksiloAppBuilder::app_state`.
    pub fn app_state<T: 'static>(&self) -> Option<&T> {
        self.app_state
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    pub(crate) fn allocate_subscription_id(&self) -> SubscriptionId {
        let id = self.next_subscription_id.get();
        self.next_subscription_id.set(id + 1);
        SubscriptionId(id)
    }

    /// Look up and invoke the UI-side callback for a posted subscription
    /// event. Returns `true` if a callback was found and invoked.
    pub fn dispatch_subscription_event(&self, sub_id: SubscriptionId, event: &dyn Any) -> bool {
        let callbacks = self.subscription_callbacks.borrow();
        if let Some(callback) = callbacks.get(&sub_id) {
            callback(event);
            true
        } else {
            false
        }
    }

    /// The window a *context-bearing* subscription targets, if `sub_id` names one
    /// **and** it was registered from a window (always true in a real app).
    /// teksilo-app peeks this to know which window's tree to mint the
    /// [`EventContext`] from before dispatching.
    pub fn ctx_subscription_window(&self, sub_id: SubscriptionId) -> Option<TeksiloWindowId> {
        self.subscription_ctx_callbacks
            .borrow()
            .get(&sub_id)
            .and_then(|(window_id, _)| *window_id)
    }

    /// Invoke the *context-bearing* UI-side callback for a posted subscription
    /// event, passing the freshly-minted [`EventContext`]. Returns `true` if a
    /// callback was found and invoked. Called by teksilo-app from inside
    /// [`WidgetTree::run_with_event_context`](crate::WidgetTree::run_with_event_context).
    ///
    /// The `Rc` handle is **cloned and the map borrow released before the call**,
    /// so the callback may freely re-enter this map — e.g. `ctx.open_window(...)`
    /// synchronously builds a new tree whose widgets register their own
    /// context-bearing subscriptions. (Holding the borrow across the call would
    /// panic there; hence `Rc`, not `Box`.)
    pub fn dispatch_subscription_event_with_ctx(
        &self,
        sub_id: SubscriptionId,
        event: &dyn Any,
        ctx: &mut EventContext,
    ) -> bool {
        let callback = self
            .subscription_ctx_callbacks
            .borrow()
            .get(&sub_id)
            .map(|(_window_id, callback)| Rc::clone(callback));
        match callback {
            Some(callback) => {
                callback(event, ctx);
                true
            }
            None => false,
        }
    }

    /// Number of context-bearing subscription callbacks currently installed.
    /// Companion to [`subscription_count`](Self::subscription_count); used by
    /// lifecycle tests to assert the ctx-map teardown ran.
    pub fn ctx_subscription_count(&self) -> usize {
        self.subscription_ctx_callbacks.borrow().len()
    }

    /// Drop every context-bearing subscription targeting `window_id`. Called by
    /// teksilo-app when a window closes, so the shared, longer-lived
    /// `TreeAppContext` map doesn't retain inert callbacks for a torn-down tree
    /// (a window's tree is dropped without a per-widget `destroy_subtree` pass).
    /// Mirrors [`AsyncCompletionHandle::purge_window`](crate::AsyncCompletionHandle::purge_window).
    pub fn purge_ctx_subscriptions_for_window(&self, window_id: TeksiloWindowId) {
        self.subscription_ctx_callbacks
            .borrow_mut()
            .retain(|_, (win, _)| *win != Some(window_id));
    }

    /// Number of UI-side subscription callbacks currently installed.
    /// Used by lifecycle tests to assert cleanup ran correctly.
    pub fn subscription_count(&self) -> usize {
        self.subscription_callbacks.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::Signal;
    use crate::widget::{LayoutContext, Widget};
    use crate::widget_id::WidgetId;
    use crate::widget_tree::WidgetTree;
    use std::sync::Mutex;
    use teksilo_canvas::SizeProposal;

    // --- Test event source ---

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    enum TestOrigin {
        Created,
        Updated,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TestEvent {
        id: u64,
        message: String,
    }

    /// A trivial in-process event source. Holds a list of (origin, callback)
    /// entries; `publish` walks them and invokes matching callbacks
    /// synchronously on the calling thread.
    #[derive(Default)]
    struct MockEventSource {
        #[allow(clippy::type_complexity)]
        subscribers: Mutex<Vec<(TestOrigin, Arc<dyn Fn(TestEvent) + Send + Sync + 'static>)>>,
    }

    impl EventSource for MockEventSource {
        type Origin = TestOrigin;
        type Event = TestEvent;

        fn subscribe(
            &self,
            origin: Self::Origin,
            callback: Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
        ) -> SubscriptionHandle {
            self.subscribers.lock().unwrap().push((origin, callback));
            SubscriptionHandle::empty()
        }
    }

    impl MockEventSource {
        fn publish(&self, origin: TestOrigin, event: TestEvent) {
            let subs = self.subscribers.lock().unwrap();
            for (sub_origin, cb) in subs.iter() {
                if *sub_origin == origin {
                    cb(event.clone());
                }
            }
        }

        fn subscriber_count(&self) -> usize {
            self.subscribers.lock().unwrap().len()
        }
    }

    /// A poster that buffers posted events into a thread-safe queue. Tests
    /// drain it after `publish` and dispatch them through the tree's
    /// app_context, mirroring the real proxy → user_event flow.
    #[derive(Default)]
    struct TestPoster {
        #[allow(clippy::type_complexity)]
        queue: Mutex<Vec<(SubscriptionId, Box<dyn Any + Send>)>>,
    }

    impl AppEventPoster for TestPoster {
        fn post_subscription_event(&self, sub_id: SubscriptionId, event: Box<dyn Any + Send>) {
            self.queue.lock().unwrap().push((sub_id, event));
        }
    }

    impl TestPoster {
        fn drain(&self) -> Vec<(SubscriptionId, Box<dyn Any + Send>)> {
            std::mem::take(&mut *self.queue.lock().unwrap())
        }
    }

    // --- Test widget that subscribes in build() ---

    #[derive(Debug)]
    struct SubscribingWidget {
        origin: TestOrigin,
        last_message: Signal<String>,
    }

    impl Widget for SubscribingWidget {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let last_message = self.last_message.clone();
            ctx.subscribe_event(self.origin.clone(), move |event: &TestEvent| {
                last_message.set(event.message.clone());
            });
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    /// Subscribes via the *context-bearing* API in `build()` — headless, so the
    /// registration records `None` for the window but still lands in the ctx map
    /// and is torn down on destroy.
    #[derive(Debug)]
    struct CtxSubscribingWidget {
        origin: TestOrigin,
    }

    impl Widget for CtxSubscribingWidget {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            ctx.subscribe_event_with_ctx(
                self.origin.clone(),
                |_event: &TestEvent, _ctx: &mut crate::widget::EventContext| {},
            );
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    // --- Helpers ---

    fn install_source(
        tree: &mut WidgetTree,
        source: MockEventSource,
    ) -> (Arc<MockEventSource>, Arc<TestPoster>) {
        let source = Arc::new(source);
        // We need to share the source between the test and the adapter,
        // so wrap a thin proxy that delegates to the Arc.
        struct SharedSource {
            inner: Arc<MockEventSource>,
        }
        impl EventSource for SharedSource {
            type Origin = TestOrigin;
            type Event = TestEvent;
            fn subscribe(
                &self,
                origin: Self::Origin,
                callback: Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
            ) -> SubscriptionHandle {
                self.inner.subscribe(origin, callback)
            }
        }

        let adapter = EventSourceAdapter::new(SharedSource {
            inner: source.clone(),
        });
        let poster: Arc<TestPoster> = Arc::new(TestPoster::default());
        let poster_dyn: Arc<dyn AppEventPoster> = poster.clone();
        let app_context =
            std::rc::Rc::new(TreeAppContext::with_source_and_poster(adapter, poster_dyn));
        tree.set_app_context(app_context);
        (source, poster)
    }

    fn drain_and_dispatch(tree: &WidgetTree, poster: &TestPoster) {
        let events = poster.drain();
        for (sub_id, event) in events {
            tree.app_context()
                .dispatch_subscription_event(sub_id, &*event);
        }
    }

    // --- Tests ---

    #[test]
    fn subscribe_event_delivers_to_widget_signal() {
        let mut tree = WidgetTree::new();
        let (source, poster) = install_source(&mut tree, MockEventSource::default());

        let signal = Signal::new(String::new());
        let _id = tree.add(SubscribingWidget {
            origin: TestOrigin::Created,
            last_message: signal.clone(),
        });

        assert_eq!(source.subscriber_count(), 1);
        assert_eq!(tree.app_context().subscription_count(), 1);

        source.publish(
            TestOrigin::Created,
            TestEvent {
                id: 1,
                message: "hello".to_string(),
            },
        );
        drain_and_dispatch(&tree, &poster);

        assert_eq!(signal.get(), "hello");
    }

    #[test]
    fn subscribe_event_with_ctx_dispatches_inside_fresh_context() {
        use crate::window::{NoopWindowOps, TeksiloWindowId};

        let mut tree = WidgetTree::new();
        // Register a context-bearing callback the way
        // `BuildContext::subscribe_event_with_ctx` does — but directly, so the
        // test needs no real window (that routing is covered end-to-end by the
        // `toast_demo` example and the Skribisto importer).
        let app_ctx = tree.app_context().clone();
        let sub_id = app_ctx.allocate_subscription_id();
        let win = TeksiloWindowId::new(1);
        let seen = Signal::new(String::new());
        let seen_cb = seen.clone();
        let stored: std::rc::Rc<dyn Fn(&dyn Any, &mut crate::widget::EventContext)> =
            std::rc::Rc::new(move |event_any, _ctx: &mut crate::widget::EventContext| {
                let ev = event_any
                    .downcast_ref::<TestEvent>()
                    .expect("subscription event downcast failed");
                seen_cb.set(ev.message.clone());
            });
        app_ctx
            .subscription_ctx_callbacks
            .borrow_mut()
            .insert(sub_id, (Some(win), stored));

        // The target window is peekable (teksilo-app reads it to pick the tree
        // whose `EventContext` it mints).
        assert_eq!(app_ctx.ctx_subscription_window(sub_id), Some(win));
        assert_eq!(app_ctx.ctx_subscription_window(SubscriptionId(9999)), None);

        // Dispatch inside a fresh `EventContext`, exactly like teksilo-app's
        // `try_dispatch_subscription_with_ctx`.
        let event = TestEvent {
            id: 9,
            message: "progress-42".to_string(),
        };
        let handled = std::cell::Cell::new(false);
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            handled.set(app_ctx.dispatch_subscription_event_with_ctx(sub_id, &event, ctx));
        });
        assert!(
            handled.get(),
            "context-bearing dispatch must find the callback"
        );
        assert_eq!(seen.get(), "progress-42");

        // An unknown sub_id is not consumed (so the caller falls back to the
        // plain, context-free path).
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            assert!(!app_ctx.dispatch_subscription_event_with_ctx(
                SubscriptionId(9999),
                &event,
                ctx
            ));
        });
    }

    /// Regression for the re-entrancy panic: dispatch must clone the `Rc` and
    /// **release the map borrow before invoking** the callback, so a callback
    /// that re-enters the same map — as `ctx.open_window(...)` does via a nested
    /// `build()` calling `subscribe_event_with_ctx` — does not `BorrowMutError`.
    #[test]
    fn ctx_dispatch_releases_borrow_before_invoking_callback() {
        use crate::window::{NoopWindowOps, TeksiloWindowId};

        let mut tree = WidgetTree::new();
        let app_ctx = tree.app_context().clone();
        let sub_id = app_ctx.allocate_subscription_id();

        let reenter_ctx = app_ctx.clone();
        let reentered = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = reentered.clone();
        let cb: std::rc::Rc<dyn Fn(&dyn Any, &mut crate::widget::EventContext)> =
            std::rc::Rc::new(move |_ev, _ctx| {
                // Simulate open_window → build() → subscribe_event_with_ctx: a
                // fresh registration into the SAME map while this callback runs.
                reenter_ctx.subscription_ctx_callbacks.borrow_mut().insert(
                    SubscriptionId(4242),
                    (
                        Some(TeksiloWindowId::new(2)),
                        std::rc::Rc::new(|_e: &dyn Any, _c: &mut crate::widget::EventContext| {}),
                    ),
                );
                flag.set(true);
            });
        app_ctx
            .subscription_ctx_callbacks
            .borrow_mut()
            .insert(sub_id, (Some(TeksiloWindowId::new(1)), cb));

        let event = TestEvent {
            id: 1,
            message: String::new(),
        };
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            assert!(app_ctx.dispatch_subscription_event_with_ctx(sub_id, &event, ctx));
        });

        assert!(
            reentered.get(),
            "callback ran and its re-entrant map insert did not panic"
        );
        assert_eq!(
            app_ctx.ctx_subscription_count(),
            2,
            "original + the re-entrant insert both present"
        );
    }

    /// Exercises the real `BuildContext::subscribe_event_with_ctx` (headless →
    /// window `None`) end-to-end: registration lands in the ctx map, and
    /// destroying the widget tears it back down (covers the widget-destroy path's
    /// removal from the ctx map).
    #[test]
    fn subscribe_event_with_ctx_registers_and_tears_down() {
        let mut tree = WidgetTree::new();
        let (_source, _poster) = install_source(&mut tree, MockEventSource::default());

        let id = tree.add(CtxSubscribingWidget {
            origin: TestOrigin::Created,
        });
        // The ctx path uses the OTHER map — the plain count stays 0.
        assert_eq!(tree.app_context().ctx_subscription_count(), 1);
        assert_eq!(tree.app_context().subscription_count(), 0);

        tree.destroy_subtree(id);
        assert_eq!(
            tree.app_context().ctx_subscription_count(),
            0,
            "destroying the widget must remove its context-bearing subscription"
        );
    }

    #[test]
    fn unrelated_origin_does_not_fire_callback() {
        let mut tree = WidgetTree::new();
        let (source, poster) = install_source(&mut tree, MockEventSource::default());

        let signal = Signal::new(String::new());
        let _id = tree.add(SubscribingWidget {
            origin: TestOrigin::Created,
            last_message: signal.clone(),
        });

        source.publish(
            TestOrigin::Updated,
            TestEvent {
                id: 1,
                message: "ignored".to_string(),
            },
        );
        drain_and_dispatch(&tree, &poster);

        assert_eq!(signal.get(), "");
    }

    #[test]
    fn destroying_widget_removes_ui_callback() {
        let mut tree = WidgetTree::new();
        let (_source, _poster) = install_source(&mut tree, MockEventSource::default());

        let signal = Signal::new(String::new());
        let id = tree.add(SubscribingWidget {
            origin: TestOrigin::Created,
            last_message: signal.clone(),
        });

        assert_eq!(tree.app_context().subscription_count(), 1);
        tree.destroy_subtree(id);
        assert_eq!(tree.app_context().subscription_count(), 0);
    }

    #[test]
    fn in_flight_event_after_destroy_is_dropped_not_delivered() {
        // An event that was buffered in the proxy queue before the widget
        // was destroyed is silently dropped once cleanup completes. The
        // destroy path removes the UI-side callback synchronously, so by
        // the time the drain happens the callback lookup misses. This
        // preserves the invariant that a destroyed widget never sees
        // another event.
        let mut tree = WidgetTree::new();
        let (source, poster) = install_source(&mut tree, MockEventSource::default());

        let signal = Signal::new(String::new());
        let id = tree.add(SubscribingWidget {
            origin: TestOrigin::Created,
            last_message: signal.clone(),
        });

        // Publish — the wrapper fires and enqueues into the test poster.
        source.publish(
            TestOrigin::Created,
            TestEvent {
                id: 7,
                message: "buffered".to_string(),
            },
        );

        tree.destroy_subtree(id);
        drain_and_dispatch(&tree, &poster);

        assert_eq!(signal.get(), "");
        assert_eq!(tree.app_context().subscription_count(), 0);
    }

    #[test]
    #[should_panic(expected = "no event source was registered")]
    fn subscribe_without_event_source_panics() {
        let mut tree = WidgetTree::new();
        let signal = Signal::new(String::new());
        // No install_source — tree has the empty default app context.
        tree.add(SubscribingWidget {
            origin: TestOrigin::Created,
            last_message: signal,
        });
    }

    // --- app_state tests (architecture §9.5) ---

    use std::rc::Rc;

    struct TestGlobals {
        greeting: Signal<String>,
    }

    /// Widget that reads `Rc<TestGlobals>` from app_state in `build()` and
    /// records what it observed into an out-of-band signal.
    #[derive(Debug)]
    struct AppStateReader {
        observed: Signal<String>,
        saw_none: Signal<bool>,
    }

    impl Widget for AppStateReader {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            match ctx.app_state::<Rc<TestGlobals>>() {
                Some(globals) => self.observed.set(globals.greeting.get()),
                None => self.saw_none.set(true),
            }
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    #[test]
    fn app_state_roundtrip_in_build_context() {
        let globals = Rc::new(TestGlobals {
            greeting: Signal::new("hello from registry".to_string()),
        });

        let mut registry: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        registry.insert(TypeId::of::<Rc<TestGlobals>>(), Box::new(globals.clone()));

        let mut tree = WidgetTree::new();
        tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(registry)));

        let observed = Signal::new(String::new());
        let saw_none = Signal::new(false);
        tree.add(AppStateReader {
            observed: observed.clone(),
            saw_none: saw_none.clone(),
        });

        assert_eq!(observed.get(), "hello from registry");
        assert!(!saw_none.get());
    }

    #[test]
    fn app_state_missing_returns_none() {
        let mut tree = WidgetTree::new();
        // No app_state installed — tree has the empty default app context.

        let observed = Signal::new(String::new());
        let saw_none = Signal::new(false);
        tree.add(AppStateReader {
            observed: observed.clone(),
            saw_none: saw_none.clone(),
        });

        assert_eq!(observed.get(), "");
        assert!(saw_none.get());
    }

    #[test]
    fn app_state_distinct_types_coexist() {
        struct Alpha(u32);
        struct Beta(String);

        let mut registry: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        registry.insert(TypeId::of::<Rc<Alpha>>(), Box::new(Rc::new(Alpha(42))));
        registry.insert(
            TypeId::of::<Rc<Beta>>(),
            Box::new(Rc::new(Beta("beta!".to_string()))),
        );

        let ctx = TreeAppContext::empty().with_app_state(registry);
        assert_eq!(ctx.app_state::<Rc<Alpha>>().unwrap().0, 42);
        assert_eq!(ctx.app_state::<Rc<Beta>>().unwrap().0, "beta!");
        assert!(ctx.app_state::<Rc<u64>>().is_none());
    }
}
