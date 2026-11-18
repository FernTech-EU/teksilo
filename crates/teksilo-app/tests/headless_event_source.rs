// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `build_headless` gives the event source registered on the builder to the
//! window tree and to the application tree, as `run` does, so a widget that
//! subscribes in `build()` works headless.

use std::sync::{Arc, Mutex};

use teksilo_app::TeksiloAppBuilder;
use teksilo_canvas::SizeProposal;
use teksilo_core::{
    BuildContext, EventSource, LayoutContext, LayoutResponse, SubscriptionHandle, Widget, WidgetId,
};

type Publisher = Arc<dyn Fn(u8) + Send + Sync>;

/// Keeps every subscriber's callback so a test can publish to it.
#[derive(Clone, Default)]
struct Source {
    subscribers: Arc<Mutex<Vec<Publisher>>>,
}

impl EventSource for Source {
    type Origin = u8;
    type Event = u8;

    fn subscribe(&self, _origin: u8, callback: Publisher) -> SubscriptionHandle {
        self.subscribers.lock().unwrap().push(callback);
        SubscriptionHandle::empty()
    }
}

#[derive(Debug)]
struct Subscriber;

impl Widget for Subscriber {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.subscribe_event(1_u8, |_: &u8| {});
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[test]
fn a_window_widget_subscribes_to_the_builders_event_source() {
    let source = Source::default();
    let mut app = TeksiloAppBuilder::new()
        .event_source(source.clone())
        .build_headless();
    app.tree.add(Subscriber);
    app.tree.layout(SizeProposal::exact(10.0, 10.0));

    assert_eq!(app.tree.app_context().subscription_count(), 1);
    let subscribers = source.subscribers.lock().unwrap();
    assert_eq!(subscribers.len(), 1, "the subscription reached the source");
    // Nothing delivers it headless; publishing must still be harmless.
    subscribers[0](7);
}

#[test]
fn an_application_tree_subscribes_to_the_builders_event_source() {
    let source = Source::default();
    let app = TeksiloAppBuilder::new()
        .event_source(source.clone())
        .app_root(|tree| tree.add(Subscriber))
        .build_headless();

    let app_tree = app.app_tree.as_ref().expect("an application tree");
    assert_eq!(app_tree.app_context().subscription_count(), 1);
    assert_eq!(source.subscribers.lock().unwrap().len(), 1);
}
