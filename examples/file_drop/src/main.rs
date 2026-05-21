//! Drag-and-drop showcase — inbound, outbound, and wrapping targets.
//!
//! **Inbound (OS → app).** Two [`DropZone`]s accept files (and text / URLs)
//! dragged in from the file manager or another application:
//!
//! - an image-only zone filtered to `png` / `jpg` / `jpeg` / `gif`
//! - a general zone that takes any file, plus dropped text and URLs
//!
//! Each zone also has a keyboard-operable **Browse…** button (the accessible
//! equivalent, since an OS drag can't be started from the keyboard).
//!
//! **Outbound (app → OS).** Two rows start a drag carrying a typed value AND a
//! MIME representation; the moment the pointer leaves the window the framework
//! escalates to a native OS drag, so they drop into Finder / Nautilus / a text
//! editor. `on_drag_ended` reports the [`DropOutcome`].
//!
//! **Wrapping target.** A [`DropTarget`] wraps an ordinary `Panel` and turns it
//! into an internal drop target without changing its look: it highlights and
//! fades in a centered hint while a row hovers, and recovers the original typed
//! `String` on drop — even after the payload was dragged out to the OS and back
//! (the framework's typed re-entry, which is what enables cross-window DnD).
//!
//! Run with: `cargo run -p file-drop`. (External OS drops are live on macOS /
//! Windows / Wayland; on X11 the Browse button is the path.)

use bastyde::core::drag_payload::{DragPayload, DropOutcome};
use bastyde::core::gesture::DragPhase;
use bastyde::core::widget_id::WidgetId;
use bastyde::prelude::*;
use bastyde::widgets::{
    DropTarget, DropTargetVariant, DropZone, Expand, HStack, Padding, Panel, TextWidget, VStack,
};

fn main() {
    BastydeAppBuilder::new()
        .theme(bastyde::presets::intui::light())
        .install_inspector_in_debug()
        .install_external_dnd()
        .install_file_dialog()
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — External Drag & Drop")
                .size(900, 600)
                .root(|tree, _state| {
                    // Shared log of accepted drops, newest first.
                    let log = Signal::new(String::from("Dropped items will appear here."));

                    let images_zone = {
                        let log = log.clone();
                        DropZone::new(lit!("Drop images here"))
                            .subtitle(lit!("PNG · JPEG · GIF"))
                            .accept_extensions(["png", "jpg", "jpeg", "gif"])
                            .on_files_dropped(move |paths, _ctx| {
                                for p in &paths {
                                    prepend(&log, &format!("🖼  {}", p.display()));
                                }
                            })
                    };

                    let any_zone = {
                        let files_log = log.clone();
                        let text_log = log.clone();
                        let urls_log = log.clone();
                        DropZone::new(lit!("Drop anything here"))
                            .subtitle(lit!("files, text, or links"))
                            .on_files_dropped(move |paths, _ctx| {
                                for p in &paths {
                                    prepend(&files_log, &format!("📄  {}", p.display()));
                                }
                            })
                            .on_text_dropped(move |text, _ctx| {
                                prepend(&text_log, &format!("📝  {text}"));
                            })
                            .on_urls_dropped(move |urls, _ctx| {
                                for u in &urls {
                                    prepend(&urls_log, &format!("🔗  {u}"));
                                }
                            })
                    };

                    // --- Outbound (app → OS) drag source ------------------
                    //
                    // The same `DragPayload` carries a typed value (for any
                    // future in-app target) AND a `text/uri-list` / `text/plain`
                    // MIME representation. The framework runs a normal in-app
                    // drag and, the moment the pointer leaves the window,
                    // escalates to a native OS drag using the MIME bytes — so
                    // these rows drop into Finder / Nautilus / a text editor.
                    let this_file = concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs");

                    let file_row = {
                        let id = Signal::new(WidgetId::default());
                        let id_for_drag = id.clone();
                        let log = log.clone();
                        let row = Panel::new()
                            .child(Padding::uniform(10.0).child(TextWidget::new(lit!("⠿  Drag this file out →  (main.rs)"),
                            )))
                            .on_drag(move |phase, ctx| {
                                if let DragPhase::Started { .. } = phase {
                                    let uri_list = format!("file://{this_file}\r\n");
                                    let payload = DragPayload::typed(this_file.to_string())
                                        .with_mime("text/uri-list", uri_list.into_bytes());
                                    ctx.start_drag(id_for_drag.get(), payload);
                                }
                            })
                            .on_drag_ended(move |outcome, _ctx| {
                                prepend(&log, &format!("↗  file drag ended: {}", describe(outcome)));
                            });
                        let added = tree.add(row);
                        id.set(added);
                        added
                    };

                    let text_row = {
                        let id = Signal::new(WidgetId::default());
                        let id_for_drag = id.clone();
                        let log = log.clone();
                        let row = Panel::new()
                            .child(Padding::uniform(10.0).child(TextWidget::new(lit!("⠿  Drag this text out →  (\"Hello from Bastyde\")"),
                            )))
                            .on_drag(move |phase, ctx| {
                                if let DragPhase::Started { .. } = phase {
                                    let text = "Hello from Bastyde";
                                    let payload = DragPayload::typed(text.to_string())
                                        .with_mime("text/plain", text.as_bytes().to_vec());
                                    ctx.start_drag(id_for_drag.get(), payload);
                                }
                            })
                            .on_drag_ended(move |outcome, _ctx| {
                                prepend(&log, &format!("↗  text drag ended: {}", describe(outcome)));
                            });
                        let added = tree.add(row);
                        id.set(added);
                        added
                    };

                    let drag_out = Panel::new().child(
                        Padding::uniform(12.0).child(
                            VStack::new()
                                .spacing(10.0)
                                .child(TextWidget::new(lit!("Drag OUT (app → OS)")))
                                .add_child(file_row)
                                .add_child(text_row),
                        ),
                    );

                    // Internal drop target built from the `DropTarget` wrapper.
                    // Drop a row here directly (in-app), OR drag it out of the
                    // window and back in before dropping — either way the
                    // original typed `String` is recovered, proving the OS
                    // round-trip preserves the typed fast-path (this is what
                    // enables drag-and-drop between two windows of the same app).
                    //
                    // `on_drop_typed::<String>()` implicitly accepts only
                    // payloads carrying a `String`, so the highlight + hint
                    // appear for the rows above but not for a stray OS file drag.
                    let typed_target = {
                        let log = log.clone();
                        DropTarget::new()
                            .child(Panel::new().child(Padding::uniform(12.0).child(
                                TextWidget::new(lit!("Internal drop target — drop a row here (recovers the typed value)"),
                                ),
                            )))
                            .hint(TextWidget::new(lit!("Drop to recover the typed value")))
                            .variant(DropTargetVariant::Prominent)
                            .on_drop_typed::<String>(move |value, _pos, _ctx| {
                                prepend(&log, &format!("✅  typed drop recovered: {value}"));
                                true
                            })
                    };

                    tree.add(
                        VStack::new()
                            .spacing(16.0)
                            .child(TextWidget::new(lit!("Drag files from your file manager onto a zone — or drag the rows \
                                 below out into Finder / a text editor."),
                            ))
                            .child(drag_out)
                            .child(typed_target)
                            .child(
                                Expand::new().child(
                                    HStack::new()
                                        .spacing(16.0)
                                        .child(Expand::new().child(images_zone))
                                        .child(Expand::new().child(any_zone)),
                                ),
                            )
                            .child(
                                Panel::new().child(
                                    TextWidget::new(lit!(String::new())).bind_text(log.clone()),
                                ),
                            ),
                    )
                }),
        )
        .run();
}

/// Prepend a line to the log signal (newest first).
fn prepend(log: &Signal<String>, line: &str) {
    let current = log.get();
    log.set(format!("{line}\n{current}"));
}

/// Human-readable drag outcome for the log.
fn describe(outcome: DropOutcome) -> &'static str {
    match outcome {
        DropOutcome::InApp { accepted: true } => "dropped in-app (accepted)",
        DropOutcome::InApp { accepted: false } => "dropped in-app (rejected)",
        DropOutcome::OsCopy => "copied to another app",
        DropOutcome::OsMove => "moved to another app",
        DropOutcome::Cancelled => "cancelled",
    }
}
