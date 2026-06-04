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

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use bastyde::prelude::*;
use bastyde::settings::AppPaths;
use bastyde::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, Spacer, StatusBar, TextWidget, Toolbar, VStack,
};

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

fn main() {
    BastydeAppBuilder::new()
        .theme(bastyde::presets::intui::light())
        .app_paths(AppPaths::new("com", "FernTech", "ToastDemo").expect("config dir"))
        .install_toast_default()
        .initial_window(
            WindowConfig::new()
                .id("main")
                .title("Toast Demo")
                .size(900, 600)
                .root(|tree, _state| tree.add(build_root())),
        )
        .run();
}
