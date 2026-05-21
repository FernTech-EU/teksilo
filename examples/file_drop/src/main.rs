//! External (OS) drag-and-drop showcase.
//!
//! Two [`DropZone`]s accept files (and text / URLs) dragged in from the file
//! manager or another application:
//!
//! - an image-only zone filtered to `png` / `jpg` / `jpeg` / `gif`
//! - a general zone that takes any file, plus dropped text and URLs
//!
//! Each accepted drop appends to the log panel below; rejected drops flash the
//! zone red and announce politely to screen readers. Every zone also has a
//! keyboard-operable **Browse…** button (the accessible equivalent, since an
//! OS drag can't be started from the keyboard).
//!
//! Run with: `cargo run -p file-drop`. Drag a file from Finder / Explorer /
//! Nautilus onto a zone. (External OS drops are live on macOS / Windows /
//! Wayland; on X11 the Browse button is the path.)

use bastyde::core::DropFeedback;
use bastyde::core::drag_payload::{DragPayload, DropOutcome};
use bastyde::core::gesture::DragPhase;
use bastyde::core::widget_id::WidgetId;
use bastyde::prelude::*;
use bastyde::widgets::{DropZone, Expand, HStack, Padding, Panel, TextWidget, VStack};

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
                        DropZone::new_literal("Drop images here")
                            .subtitle_literal("PNG · JPEG · GIF")
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
                        DropZone::new_literal("Drop anything here")
                            .subtitle_literal("files, text, or links")
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
                            .child(Padding::uniform(10.0).child(TextWidget::new_literal(
                                "⠿  Drag this file out →  (main.rs)",
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
                            .child(Padding::uniform(10.0).child(TextWidget::new_literal(
                                "⠿  Drag this text out →  (\"Hello from Bastyde\")",
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
                                .child(TextWidget::new_literal("Drag OUT (app → OS)"))
                                .add_child(file_row)
                                .add_child(text_row),
                        ),
                    );

                    // Internal drop target that reads the *typed* payload. Drop
                    // a row here directly (in-app), OR drag it out of the window
                    // and back in before dropping — either way the original
                    // typed `String` is recovered, proving the OS round-trip
                    // preserves the typed fast-path (this is what enables
                    // drag-and-drop between two windows of the same app).
                    let typed_target = {
                        let log = log.clone();
                        Panel::new()
                            .child(Padding::uniform(12.0).child(TextWidget::new_literal(
                                "Internal drop target — drop a row here (recovers the typed value)",
                            )))
                            .on_drag_hover(|payload, _pos, _ctx| {
                                if payload.has_typed::<String>() {
                                    DropFeedback::HighlightRect {
                                        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
                                        color: Color::new(0.2, 0.6, 1.0, 0.25),
                                    }
                                } else {
                                    DropFeedback::NoFeedback
                                }
                            })
                            .on_drop(move |mut payload, _pos, _ctx| {
                                match payload.take_typed::<String>() {
                                    Some(value) => {
                                        prepend(&log, &format!("✅  typed drop recovered: {value}"));
                                        true
                                    }
                                    None => false,
                                }
                            })
                    };

                    tree.add(
                        VStack::new()
                            .spacing(16.0)
                            .child(TextWidget::new_literal(
                                "Drag files from your file manager onto a zone — or drag the rows \
                                 below out into Finder / a text editor.",
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
                                    TextWidget::new_literal(String::new()).bind_text(log.clone()),
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