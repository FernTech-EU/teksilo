// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Close / quit confirmation — intercepting a window close to ask the
//! user before tearing the window down (and, for the last window,
//! quitting the app).
//!
//! The framework runs a per-window **close guard**
//! ([`WindowConfig::on_close_requested`]) before any *interactive* close
//! gesture takes effect — the OS close button, `Alt+F4` / `Cmd+W`, a
//! custom-chrome close button, and a handler calling
//! [`EventContext::close_window`]. The guard returns
//! [`CloseResponse::Veto`] to cancel the close, or
//! [`CloseResponse::Close`] to let it proceed.
//!
//! Because a confirmation dialog is asynchronous (it waits for a
//! click), the idiomatic shape is **veto-then-reissue**: the guard
//! vetoes *and* opens a confirmation; the dialog's "close anyway"
//! button calls [`EventContext::close_window_forced`], which closes the
//! window unconditionally without re-triggering the guard.
//!
//! This demo wires the guard two ways, one per window:
//!
//! * **Main window** — a full `on_close_requested` guard that pops a
//!   Save / Discard / Cancel [`MessageBox`] while the document is
//!   marked dirty.
//! * **Sugar window** (opened from the main window) — the reactive
//!   [`WindowConfig::can_close`] + [`WindowConfig::on_close_blocked`]
//!   shorthand: a `Signal<bool>` gates the close and a callback shows
//!   the confirmation.
//!
//! Run with `cargo run -p close-confirmation`.

use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Checkbox, EventContextMessageBoxExt, MessageBox, MessageBoxButtons,
    Padding, StandardButton, TextWidget, VStack,
};

fn main() {
    // The "document is dirty" flag is shared between the main window's
    // UI (a checkbox) and its close guard, so it is created up front and
    // cloned into both. This is the `ListModel`/`Signal` share-by-handle
    // pattern — both sides see the same value.
    let dirty = Signal::new(true);

    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(intui::light())
        .initial_window(main_window(dirty))
        .run();
}

/// Build the main window: a dirty-state checkbox, a programmatic
/// "Close window" button (which goes through the guard, exactly like the
/// OS close button), and a button that opens the `can_close`-sugar
/// window.
fn main_window(dirty: Signal<bool>) -> WindowConfig {
    let dirty_for_guard = dirty.clone();
    let dirty_for_root = dirty.clone();

    WindowConfig::new()
        .title("Close confirmation")
        .id("main")
        .size(560, 380)
        // The close guard. Consulted before the OS close button, Cmd+W,
        // or `ctx.close_window()` actually closes this window.
        .on_close_requested(move |ctx| {
            if !dirty_for_guard.get() {
                // Nothing unsaved — let the close (and, for the last
                // window, the app quit) proceed immediately.
                return CloseResponse::Close;
            }

            // Unsaved changes: veto this attempt and ask. The dialog's
            // buttons decide what happens next.
            let dirty = dirty_for_guard.clone();
            ctx.present_message_box(
                MessageBox::question(lit!("Close window?"))
                    .text(lit!("The document has unsaved changes."))
                    .informative_text(lit!(
                        "Your changes will be lost if you close without saving."
                    ))
                    .buttons(MessageBoxButtons::SaveDiscardCancel)
                    .default_button(StandardButton::Save)
                    .escape_button(StandardButton::Cancel)
                    .on_result(move |result, ctx| match result.button {
                        // "Save" — pretend to persist, clear the dirty
                        // flag, then re-issue the close (now unguarded).
                        StandardButton::Save => {
                            dirty.set(false);
                            ctx.close_window_forced();
                        }
                        // "Discard" — close without saving.
                        StandardButton::Discard => ctx.close_window_forced(),
                        // "Cancel" (or Esc) — leave the window open.
                        _ => {}
                    }),
            );
            CloseResponse::Veto
        })
        .root(move |tree, _state| {
            let dirty = dirty_for_root.clone();
            tree.add(
                Padding::symmetric(24.0, 24.0).child(
                    VStack::new()
                        .spacing(16.0)
                        .child(
                            TextWidget::new(lit!("Close-confirmation demo"))
                                .style(TextStyleRole::BodyBold),
                        )
                        .child(TextWidget::new(lit!(
                            "Try to close this window — the OS close button, Cmd+W/Alt+F4, or \
                             the button below. While the document is dirty, a Save / Discard / \
                             Cancel confirmation appears first. This is the last window, so \
                             confirming the close also quits the app."
                        )))
                        .child(
                            Checkbox::new(dirty.clone())
                                .label(lit!("Document has unsaved changes")),
                        )
                        .child(
                            Button::new(lit!("Close window (ctx.close_window)"))
                                .variant(ButtonVariant::Filled)
                                .on_activate_fn(|ctx| ctx.close_window()),
                        )
                        .child(
                            Button::new(lit!("Open can_close-sugar window…"))
                                .variant(ButtonVariant::Tinted)
                                .on_activate_fn(|ctx| {
                                    ctx.open_window(sugar_window());
                                }),
                        ),
                ),
            )
        })
}

/// A second window demonstrating the reactive sugar: instead of an
/// `on_close_requested` closure, bind a `Signal<bool>` via
/// [`WindowConfig::can_close`] and present the confirmation from
/// [`WindowConfig::on_close_blocked`].
fn sugar_window() -> WindowConfig {
    // `locked` reads `false` while the window must NOT close freely.
    // `can_close` takes the *positive* "may close?" signal, so we pass
    // its inverse.
    let locked = Signal::new(true);
    let may_close = locked.not();

    let locked_for_blocked = locked.clone();
    let locked_for_root = locked.clone();

    WindowConfig::new()
        .title("can_close sugar")
        .size(520, 320)
        .can_close(may_close)
        // Fired only when `can_close` blocked a close attempt.
        .on_close_blocked(move |ctx| {
            let locked = locked_for_blocked.clone();
            ctx.present_message_box(
                MessageBox::question(lit!("Close this window?"))
                    .text(lit!("This window is locked against accidental closing."))
                    .buttons(MessageBoxButtons::YesNo)
                    .default_button(StandardButton::No)
                    .escape_button(StandardButton::No)
                    .on_result(move |result, ctx| {
                        if matches!(result.button, StandardButton::Yes) {
                            // Unlock so `can_close` now reads true, then
                            // re-issue the close (which the now-permissive
                            // signal lets through — no force needed).
                            locked.set(false);
                            ctx.close_window();
                        }
                    }),
            );
        })
        .root(move |tree, _state| {
            let locked = locked_for_root.clone();
            tree.add(
                Padding::symmetric(24.0, 24.0).child(
                    VStack::new()
                        .spacing(16.0)
                        .child(
                            TextWidget::new(lit!("can_close sugar")).style(TextStyleRole::BodyBold),
                        )
                        .child(TextWidget::new(lit!(
                            "While 'locked' is on, every close attempt is vetoed and a \
                             confirmation appears. Answer Yes to unlock and close."
                        )))
                        .child(Checkbox::new(locked.clone()).label(lit!("Locked against closing")))
                        .child(
                            Button::new(lit!("Close window"))
                                .variant(ButtonVariant::Filled)
                                .on_activate_fn(|ctx| ctx.close_window()),
                        ),
                ),
            )
        })
}
