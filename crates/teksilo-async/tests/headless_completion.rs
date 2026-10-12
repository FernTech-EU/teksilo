// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `spawn_local_with` in a headless tree: the completion is delivered by
//! `AsyncCompletionHandle::drain_into`, with no event loop and no poster.

use teksilo_app::{TeksiloAppBuilder, WindowConfig};
use teksilo_async::{AsyncRuntimeHandle, EventContextAsyncExt, TeksiloAppBuilderAsyncExt};
use teksilo_core::{NoopWindowOps, Signal, WidgetBuilder, WidgetId};
use teksilo_widgets::RectWidget;

/// A headless app with the runtime installed and one focusable widget, and
/// the runtime handle taken from app-state.
fn app() -> (teksilo_app::HeadlessApp, AsyncRuntimeHandle, WidgetId) {
    let mut app = TeksiloAppBuilder::new().install_async().build_headless();
    let rt = app
        .tree
        .app_context()
        .app_state::<AsyncRuntimeHandle>()
        .expect("install_async registers the runtime")
        .clone();
    let target = app.tree.add(RectWidget::new().focusable(true));
    (app, rt, target)
}

#[test]
fn completion_runs_with_its_value_and_context_effects() {
    let (mut app, rt, target) = app();
    let got = Signal::new(0_i32);

    let sink = got.clone();
    app.tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.spawn_local_with(async { 21 * 2 }, move |value, ctx| {
            sink.set(value);
            ctx.request_focus(target);
        })
        .detach();
    });

    // Nothing ran yet: the future has not been polled.
    assert_eq!(got.get(), 0);
    assert_eq!(rt.completions().pending_len(), 0);

    assert!(rt.tick(), "the spawned future is polled");
    assert_eq!(rt.completions().pending_len(), 1, "resolved, waiting");
    assert_eq!(got.get(), 0, "not delivered until drained");

    assert_eq!(rt.completions().drain_into(&mut app.tree), 1);
    assert_eq!(got.get(), 42);
    assert_eq!(
        app.tree.focused(),
        Some(target),
        "the callback's EventContext effects apply"
    );
    assert_eq!(rt.completions().pending_len(), 0);
}

#[test]
fn spawn_local_with_does_not_panic_without_a_window() {
    // This app has no initial window, so the context carries no window state.
    let (mut app, rt, _) = app();
    let done = Signal::new(false);
    let flag = done.clone();
    app.tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.spawn_local_with(async {}, move |(), _ctx| flag.set(true))
            .detach();
    });
    rt.tick();
    rt.completions().drain_into(&mut app.tree);
    assert!(done.get());
}

#[test]
fn completion_is_delivered_in_a_headless_app_with_a_window() {
    let mut app = TeksiloAppBuilder::new()
        .install_async()
        .initial_window(WindowConfig::new().root(|tree, _state| tree.add(RectWidget::new())))
        .build_headless();
    let rt = app
        .tree
        .app_context()
        .app_state::<AsyncRuntimeHandle>()
        .expect("install_async registers the runtime")
        .clone();
    let got = Signal::new(String::new());
    let sink = got.clone();
    app.tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.spawn_local_with(async { "done".to_string() }, move |v, _ctx| sink.set(v))
            .detach();
    });
    rt.tick();
    assert_eq!(rt.completions().drain_into(&mut app.tree), 1);
    assert_eq!(got.get(), "done");
}
