// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Toast Demo — end-to-end exercise of the Toast notification system.
//!
//! Run with: `cargo run -p toast-demo`
//!
//! Demonstrates:
//!
//! * `BastydeAppBuilder::install_toast_default()` — one-line install
//!   that registers a `ToastRegistry` + persistent
//!   `NotificationArchiveModel` in app-state and wraps every
//!   window's root with a `ToastHost`.
//! * `ctx.show_toast(Toast::…)` from any handler — the
//!   `EventContextToastExt` extension surface.
//! * Severity variants (`info` / `success` / `warning` / `error` /
//!   `loading`), action buttons (Link + Button), update-in-place by
//!   `id`, persistent / `archive(false)` opt-outs.
//! * `NotificationCenterButton` in the StatusBar — bell with an
//!   unread-count badge that opens a `NotificationLog` popover.
//! * `NotificationLogDialog::show(archive, ctx)` — the modal preset
//!   triggered from a button.
//! * **Background-job progress toast** — the "Start background job"
//!   button spawns a real worker thread that emits progress from
//!   *off* the UI thread through an in-process `EventSource`. A
//!   `ctx.subscribe_event_with_ctx(...)` subscription receives each
//!   event back on the UI thread *with a fresh `EventContext`* and
//!   updates the loading toast in place (percentage + a Cancel
//!   action), replacing it with a success/cancelled toast when the
//!   job ends. This is the supported bridge for driving toasts from a
//!   backend long-operation's progress events (Qleany
//!   `Origin::LongOperation(Progress | Completed | Cancelled)`).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bastyde::core::event_source::{EventSource, SubscriptionHandle};
use bastyde::prelude::*;
use bastyde::settings::AppPaths;
use bastyde::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, Spacer, StatusBar, TextWidget, Toolbar, VStack,
};

/// Update-in-place key shared by every toast the background-job demo shows, so
/// the loading → progress → success/cancel toasts are one evolving surface.
const JOB_TOAST_ID: &str = "demo.background-job";

fn build_root() -> impl Widget {
    let next_id = Rc::new(Cell::new(0_usize));
    let upload_handle: Rc<Cell<Option<ToastHandle>>> = Rc::new(Cell::new(None));

    VStack::new()
        .spacing(0.0)
        .child(toolbar(next_id.clone(), upload_handle))
        // `Expand::vertical` marks where the VStack's leftover height
        // goes: the body fills the middle and the status bar is pushed
        // to the bottom edge. Without it, a flexless VStack top-aligns
        // its children (the SwiftUI/flexbox stack rule) and the status
        // bar would sit directly under the body.
        .child(Expand::vertical().respect_intrinsic().child(body()))
        // Invisible: owns the job's context-bearing subscriptions.
        .child(JobProgressListener)
        .child(status_bar())
}

fn toolbar(next_id: Rc<Cell<usize>>, upload_handle: Rc<Cell<Option<ToastHandle>>>) -> impl Widget {
    let n0 = next_id.clone();
    let n1 = next_id.clone();
    let n2 = next_id.clone();
    let n3 = next_id.clone();
    let n4 = next_id.clone();
    let uh_start = upload_handle.clone();
    let uh_progress = upload_handle.clone();
    let uh_complete = upload_handle;

    Toolbar::new().child(
        HStack::new()
            .spacing(6.0)
            .child(Button::new(lit!("Info")).on_activate_fn(move |ctx| {
                let i = bump(&n0);
                ctx.show_toast(Toast::info(lit!(format!("Info notice #{i}"))));
            }))
            .child(Button::new(lit!("Success")).on_activate_fn(move |ctx| {
                let i = bump(&n1);
                ctx.show_toast(Toast::success(lit!(format!("Saved #{i}"))));
            }))
            .child(Button::new(lit!("Warning")).on_activate_fn(move |ctx| {
                let i = bump(&n2);
                ctx.show_toast(
                    Toast::warning(lit!(format!("Warning #{i}")))
                        .body(lit!("Take a look when you have a moment.")),
                );
            }))
            .child(Button::new(lit!("Error")).on_activate_fn(move |ctx| {
                let i = bump(&n3);
                ctx.show_toast(
                    Toast::error(lit!(format!("Build #{i} failed")))
                        .body(lit!("Three errors in src/main.rs, two warnings."))
                        .action(ToastAction::primary(lit!("Show errors"), |_| {
                            eprintln!("[demo] Show errors clicked");
                        })),
                );
            }))
            .child(
                Button::new(lit!("Persistent error")).on_activate_fn(move |ctx| {
                    let i = bump(&n4);
                    ctx.show_toast(
                        Toast::error(lit!(format!("Sticky error #{i}")))
                            .body(lit!("This one persists until you dismiss it."))
                            .persistent(),
                    );
                }),
            )
            .child(Spacer::new())
            .child(
                Button::new(lit!("Start upload"))
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(move |ctx| {
                        let h = ctx.show_toast(
                            Toast::loading(lit!("Uploading 1 of 7…")).id("demo.upload"),
                        );
                        uh_start.set(Some(h));
                    }),
            )
            .child(
                Button::new(lit!("Update upload")).on_activate_fn(move |ctx| {
                    if uh_progress.take().is_some() {
                        // Re-present with the same id. `Toast::id`
                        // is the update-in-place key: the live toast
                        // surface mutates in place AND the archive
                        // entry merges (one row in the log with the
                        // mutation recorded under `updates`).
                        let h = ctx.show_toast(
                            Toast::loading(lit!("Uploading 4 of 7…")).id("demo.upload"),
                        );
                        uh_progress.set(Some(h));
                    }
                }),
            )
            .child(
                Button::new(lit!("Complete upload"))
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(move |ctx| {
                        uh_complete.take();
                        ctx.show_toast(
                            Toast::success(lit!("Upload complete"))
                                .id("demo.upload")
                                .auto_dismiss_after(Duration::from_secs(5)),
                        );
                    }),
            )
            // Progress driven by a real background thread through
            // `subscribe_event_with_ctx` (see `JobProgressListener`).
            .child(
                Button::new(lit!("Start background job"))
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(start_background_job),
            ),
    )
}

fn body() -> impl Widget {
    Padding::uniform(24.0).child(
        VStack::new()
            .spacing(12.0)
            .child(TextWidget::new(lit!("Toast Notifications Demo")).style(TextStyleRole::BodyBold))
            .child(TextWidget::new(lit!(
                "Click the buttons in the toolbar to spawn toasts. They appear at the \
                 bottom-right corner. Hover any toast to pause every timer (the auto-dismiss \
                 won't fire while your pointer is over the group). The bell icon in the \
                 status bar shows the persistent archive — every toast is logged there \
                 (unless you call `.archive(false)`), and the log survives app restarts."
            ))),
    )
}

fn status_bar() -> impl Widget {
    StatusBar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(Button::new(lit!("Open log dialog")).on_activate_fn(|ctx| {
                let archive = ctx
                    .app_state::<Rc<NotificationArchiveModel>>()
                    .cloned()
                    .expect("install_toast registers the archive");
                NotificationLogDialog::show(archive, ctx);
            }))
            .child(BellButton::default()),
    )
}

/// Tiny widget that builds the `NotificationCenterButton` against
/// the app-state archive. Wraps it because the bell needs access to
/// `BuildContext::app_state` and the outer composable closures only
/// see plain `impl Widget`.
#[derive(Debug, Default)]
struct BellButton {
    child_id: Option<WidgetId>,
}

impl Widget for BellButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let archive = ctx
            .app_state::<Rc<NotificationArchiveModel>>()
            .cloned()
            .expect("install_toast registers the archive");
        let id = ctx.add(NotificationCenterButton::new(archive));
        self.child_id = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        proposal: bastyde::canvas::SizeProposal,
        ctx: &LayoutContext,
    ) -> LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(30.0, 30.0))
            .into()
    }
}

fn bump(cell: &Rc<Cell<usize>>) -> usize {
    let n = cell.get() + 1;
    cell.set(n);
    n
}

// =====================================================================
// Background-job progress toast
//
// The showcase for `BuildContext::subscribe_event_with_ctx`: a real
// worker thread emits progress from *off* the UI thread; the framework
// bridges each event back to the UI thread and hands the subscription
// callback a fresh `EventContext`, so it can drive an evolving toast.
// This is exactly how a Bastyde app surfaces a backend long-operation's
// `Origin::LongOperation(Progress | Completed | Cancelled)` events.
// =====================================================================

/// Topics published on the in-process [`DemoBus`]. Mirrors the shape of a real
/// backend's long-operation event origins.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum DemoTopic {
    Progress,
    Done,
    Cancelled,
}

/// Payload delivered with each [`DemoTopic`] event.
#[derive(Clone, Debug)]
struct DemoJobEvent {
    percent: f32,
    message: String,
}

/// A trivial in-process [`EventSource`]: the worker thread `publish`es events
/// from *off* the UI thread, and the framework bridges them back to the UI
/// thread — where `subscribe_event_with_ctx` hands the callback a fresh
/// `EventContext`. `Clone` shares one subscriber list (the real app's
/// `EventHubSource` plays this role over Qleany's event hub).
#[derive(Clone, Default)]
struct DemoBus {
    #[allow(clippy::type_complexity)]
    subscribers: Arc<Mutex<Vec<(DemoTopic, Arc<dyn Fn(DemoJobEvent) + Send + Sync + 'static>)>>>,
}

impl DemoBus {
    fn publish(&self, topic: DemoTopic, event: DemoJobEvent) {
        for (sub_topic, callback) in self.subscribers.lock().unwrap().iter() {
            if *sub_topic == topic {
                callback(event.clone());
            }
        }
    }
}

impl EventSource for DemoBus {
    type Origin = DemoTopic;
    type Event = DemoJobEvent;

    fn subscribe(
        &self,
        origin: Self::Origin,
        callback: Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
    ) -> SubscriptionHandle {
        self.subscribers.lock().unwrap().push((origin, callback));
        SubscriptionHandle::empty()
    }
}

/// Shared cancel flag (app-state). The toast's Cancel action flips it; the
/// worker thread polls it. Stands in for a long operation's cancel token.
#[derive(Clone)]
struct JobCancel(Arc<AtomicBool>);

/// The loading toast used for both the initial "Starting…" and every progress
/// update, so the spinner + Cancel action stay put while only the text changes
/// (update-in-place by `JOB_TOAST_ID`).
fn job_progress_toast(percent: f32, message: &str, cancel: Arc<AtomicBool>) -> Toast {
    Toast::loading(lit!("Background job"))
        .id(JOB_TOAST_ID)
        .body(lit!(format!("{percent:.0}% · {message}")))
        .action(
            ToastAction::destructive(lit!("Cancel"), move |_ctx| {
                cancel.store(true, Ordering::Relaxed);
            })
            .closes_toast(false),
        )
}

/// Invisible widget that owns the job's context-bearing subscriptions. Its
/// `build()` wires `subscribe_event_with_ctx` for each topic; the callbacks run
/// on the UI thread with a fresh `EventContext` and drive the toast in place.
#[derive(Debug, Default)]
struct JobProgressListener;

impl Widget for JobProgressListener {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let cancel = ctx
            .app_state::<JobCancel>()
            .map(|c| c.0.clone())
            .expect("JobCancel registered in app-state");

        // Progress: mutate the loading toast in place, keeping the spinner and
        // Cancel action. This callback is the whole point of the example — it
        // runs on the UI thread, in reaction to an event published from a
        // background thread, *with* an `EventContext`.
        ctx.subscribe_event_with_ctx(DemoTopic::Progress, move |ev: &DemoJobEvent, ctx| {
            ctx.show_toast(job_progress_toast(ev.percent, &ev.message, cancel.clone()));
        });
        // Done: replace the loading toast with a self-dismissing success toast.
        ctx.subscribe_event_with_ctx(DemoTopic::Done, |ev: &DemoJobEvent, ctx| {
            ctx.show_toast(
                Toast::success(lit!("Background job complete"))
                    .id(JOB_TOAST_ID)
                    .body(lit!(ev.message.clone()))
                    .auto_dismiss_after(Duration::from_secs(5)),
            );
        });
        // Cancelled: replace with a neutral, self-dismissing notice.
        ctx.subscribe_event_with_ctx(DemoTopic::Cancelled, |ev: &DemoJobEvent, ctx| {
            ctx.show_toast(
                Toast::info(lit!("Background job cancelled"))
                    .id(JOB_TOAST_ID)
                    .body(lit!(format!("Stopped at {:.0}%", ev.percent)))
                    .auto_dismiss_after(Duration::from_secs(4)),
            );
        });
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: bastyde::canvas::SizeProposal,
        _ctx: &LayoutContext,
    ) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// Kick off the simulated background job: reset the cancel flag, show the
/// initial toast, and spawn a worker thread that emits progress off the UI
/// thread, ending in a done/cancelled event.
fn start_background_job(ctx: &mut EventContext) {
    let bus = ctx
        .app_state::<DemoBus>()
        .cloned()
        .expect("DemoBus registered in app-state");
    let cancel = ctx
        .app_state::<JobCancel>()
        .map(|c| c.0.clone())
        .expect("JobCancel registered in app-state");

    cancel.store(false, Ordering::Relaxed);
    ctx.show_toast(job_progress_toast(0.0, "Starting…", cancel.clone()));

    thread::spawn(move || {
        const STEPS: u32 = 20;
        for step in 1..=STEPS {
            if cancel.load(Ordering::Relaxed) {
                bus.publish(
                    DemoTopic::Cancelled,
                    DemoJobEvent {
                        percent: (step - 1) as f32 / STEPS as f32 * 100.0,
                        message: String::new(),
                    },
                );
                return;
            }
            thread::sleep(Duration::from_millis(160));
            bus.publish(
                DemoTopic::Progress,
                DemoJobEvent {
                    percent: step as f32 / STEPS as f32 * 100.0,
                    message: format!("Fetching item {step} of {STEPS}"),
                },
            );
        }
        bus.publish(
            DemoTopic::Done,
            DemoJobEvent {
                percent: 100.0,
                message: format!("All {STEPS} items fetched"),
            },
        );
    });
}

fn main() {
    // In-process event source + shared cancel flag for the background-job demo.
    // The bus is cloned into the builder (as the app's `EventSource`) and into
    // app-state (so button handlers / the worker thread can publish + poll).
    let bus = DemoBus::default();
    let cancel = JobCancel(Arc::new(AtomicBool::new(false)));

    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(bastyde::presets::intui::light())
        .app_paths(AppPaths::new("eu", "FernTech", "ToastDemo").expect("config dir"))
        .install_toast_default()
        .event_source(bus.clone())
        .app_state(bus)
        .app_state(cancel)
        .initial_window(
            WindowConfig::new()
                .id("main")
                .title("Toast Demo")
                .size(900, 600)
                .root(|tree, _state| tree.add(build_root())),
        )
        .run();
}
