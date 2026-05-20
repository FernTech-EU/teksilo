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

use bastyde::prelude::*;
use bastyde::widgets::{DropZone, Expand, HStack, Panel, TextWidget, VStack};

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

                    tree.add(
                        VStack::new()
                            .spacing(16.0)
                            .child(TextWidget::new_literal(
                                "Drag files from your file manager onto a zone — or click Browse.",
                            ))
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