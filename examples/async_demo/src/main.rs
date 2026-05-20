//! `bastyde-async` demo — imperative async inside UI handlers.
//!
//! Run with: `cargo run -p async-demo`
//!
//! Two buttons:
//! - **Load data** uses [`spawn_local`] + [`spawn_blocking`]: a worker thread
//!   does the slow work, and the result flows back into `Signal`s on resume
//!   (the owned-handles model).
//! - **Fetch + open window** uses `spawn_local_with`: the result is delivered
//!   to a callback with a *fresh* `EventContext`, which opens a new window.
//!
//! The app sleeps at zero idle CPU between wakes — the executor only runs when
//! a task is woken (e.g. when a `spawn_blocking` worker finishes).

use std::time::Duration;

use bastyde::prelude::*;
use bastyde::widgets::{Button, ButtonVariant, TextWidget, VStack};

struct Root {
    status: Signal<String>,
    busy: Signal<bool>,
    root_child: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            status: Signal::new("Idle — click a button.".to_string()),
            busy: Signal::new(false),
            root_child: None,
        }
    }
}

impl std::fmt::Debug for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Root").finish()
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Button 1 — spawn_local + spawn_blocking. The result is written back
        // into Signals on resume; the view re-renders reactively.
        let load_btn = {
            let busy = self.busy.clone();
            let status = self.status.clone();
            Button::new_literal("Load data (spawn_blocking)")
                .variant(ButtonVariant::Filled)
                .on_activate_fn(move |ctx| {
                    if busy.get() {
                        return;
                    }
                    busy.set(true);
                    status.set("Loading… (1.2 s on a worker thread)".to_string());
                    let busy = busy.clone();
                    let status = status.clone();
                    ctx.spawn_local(async move {
                        match spawn_blocking(|| {
                            std::thread::sleep(Duration::from_millis(1200));
                            40 + 2
                        })
                        .await
                        {
                            Ok(value) => status.set(format!("Done — worker returned {value}.")),
                            Err(err) => status.set(format!("Worker failed: {err}")),
                        }
                        busy.set(false);
                    })
                    .detach();
                })
        };

        // Button 2 — spawn_local_with. The result is delivered with a fresh
        // EventContext, used here to open a new window.
        let open_btn = {
            let status = self.status.clone();
            Button::new_literal("Fetch + open result window (spawn_local_with)")
                .variant(ButtonVariant::Tinted)
                .on_activate_fn(move |ctx| {
                    status.set("Fetching for a new window…".to_string());
                    let status = status.clone();
                    ctx.spawn_local_with(
                        async {
                            spawn_blocking(|| {
                                std::thread::sleep(Duration::from_millis(800));
                                7 * 6
                            })
                            .await
                            .unwrap_or(-1)
                        },
                        move |value, ctx| {
                            status.set(format!("Opened a window for value {value}."));
                            ctx.open_window(
                                WindowConfig::new()
                                    .title("Async result")
                                    .size(360, 160)
                                    .root(move |tree, _state| {
                                        tree.add(VStack::new().spacing(8.0).child(
                                            TextWidget::new_literal(format!(
                                                "Delivered with a fresh EventContext — value {value}."
                                            )),
                                        ))
                                    }),
                            );
                        },
                    )
                    .detach();
                })
        };

        let root = ctx.add(
            VStack::new()
                .spacing(14.0)
                .child(TextWidget::new_literal("bastyde-async demo").style(TextStyleRole::BodyBold))
                .child(TextWidget::new_literal("").bind_text(self.status.clone()))
                .child(load_btn)
                .child(open_btn),
        );
        self.root_child = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

fn main() {
    BastydeAppBuilder::new()
        .theme(bastyde::presets::intui::light())
        .install_async()
        .install_inspector_in_debug()
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Async Demo")
                .size(560, 360)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
