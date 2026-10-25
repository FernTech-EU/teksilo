// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Close / quit confirmation: intercepting a window close to ask the
//! user before tearing the window down (and, for the last window,
//! quitting the app).
//!
//! The framework runs a per-window **close guard**
//! ([`WindowConfig::on_close_requested`]) before any *interactive* close
//! gesture takes effect: the OS close button, `Alt+F4` / `Cmd+W`, a
//! custom-chrome close button, and a handler calling
//! [`EventContext::close_window`]. The guard returns
//! [`CloseResponse::Close`] to let the close proceed,
//! [`CloseResponse::Veto`] to refuse it, or [`CloseResponse::Defer`] to
//! answer later.
//!
//! A confirmation dialog is asynchronous (it waits for a click), so the
//! guard **defers**: it hands the framework a [`CloseDecision`], shows the
//! dialog, and the dialog's answer calls [`CloseDecision::proceed`] (the
//! window closes, without its guard being asked again) or
//! [`CloseDecision::cancel`] (it stays open). The decision knows which
//! window it is for, so it does not matter that on macOS and Windows the
//! dialog is a native window of its own whose answer runs there.
//!
//! [`EventContext::quit`] asks every window's guard in turn, the way a
//! Quit menu item does; a guard reads [`EventContext::quit_in_progress`]
//! to word its question for the whole application, and the same decision
//! answers for the quit: proceed and the quit goes on, cancel and it stops.
//! For a session ending or a termination signal
//! ([`EventContext::quit_source`]), the guard saves without asking.
//!
//! This demo wires the guard two ways, one per window:
//!
//! * **Main window**: a full `on_close_requested` guard that defers and
//!   pops a Save / Discard / Cancel [`MessageBox`] while the document is
//!   marked dirty, and a Quit button.
//! * **Sugar window** (opened from the main window): the reactive
//!   [`WindowConfig::can_close`] + [`WindowConfig::on_close_blocked`]
//!   shorthand: a `Signal<bool>` gates the close and a callback shows
//!   a confirmation and answers the close's `CloseDecision`: Yes proceeds,
//!   and a quit waiting on the window goes on; No cancels it.
//!
//! Run with `cargo run -p close-confirmation`.

use teksilo::prelude::*;
use teksilo::widgets::{
    Button, ButtonVariant, Checkbox, EventContextMessageBoxExt, MessageBox, MessageBoxButtons,
    Padding, StandardButton, TextWidget, VStack,
};

fn main() {
    // The "document is dirty" flag is shared between the main window's
    // UI (a checkbox) and its close guard, so it is created up front and
    // cloned into both. This is the `ListModel`/`Signal` share-by-handle
    // pattern — both sides see the same value.
    let dirty = Signal::new(true);

    TeksiloAppBuilder::new()
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

            // The session is ending or a termination signal arrived:
            // nobody may be there to answer, and on Windows and for a
            // signal the window closes whatever the guard says. Save
            // directly instead of asking.
            if matches!(
                ctx.quit_source(),
                Some(QuitSource::SessionEnd | QuitSource::Signal)
            ) {
                // "Save": pretend to persist, and say so.
                println!("close-confirmation: saved for {:?}", ctx.quit_source());
                dirty_for_guard.set(false);
                return CloseResponse::Close;
            }

            // Unsaved changes: ask, and answer later. The dialog's buttons
            // settle the close through the decision. Asked by a quit, the
            // question is about the whole application, and the answer
            // settles the quit too.
            let dirty = dirty_for_guard.clone();
            let question = if ctx.quit_in_progress() {
                lit!("Quit with unsaved changes?")
            } else {
                lit!("Close window?")
            };
            let decision = CloseDecision::new();
            let answer = decision.clone();
            ctx.present_message_box(
                MessageBox::question(question)
                    .text(lit!("The document has unsaved changes."))
                    .informative_text(lit!(
                        "Your changes will be lost if you close without saving."
                    ))
                    .buttons(MessageBoxButtons::SaveDiscardCancel)
                    .default_button(StandardButton::Save)
                    .escape_button(StandardButton::Cancel)
                    .on_result(move |result, _ctx| match result.button {
                        // "Save": pretend to persist, clear the dirty flag,
                        // then let the close go ahead.
                        StandardButton::Save => {
                            dirty.set(false);
                            answer.proceed();
                        }
                        // "Discard": close without saving.
                        StandardButton::Discard => answer.proceed(),
                        // "Cancel" (or Esc): the window stays open, and a
                        // quit that asked stops here.
                        _ => answer.cancel(),
                    }),
            );
            CloseResponse::Defer(decision)
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
                            Button::new(lit!("Quit (ctx.quit)")).on_activate_fn(|ctx| ctx.quit()),
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
        .on_close_blocked(move |ctx, decision| {
            let locked = locked_for_blocked.clone();
            ctx.present_message_box(
                MessageBox::question(lit!("Close this window?"))
                    .text(lit!("This window is locked against accidental closing."))
                    .buttons(MessageBoxButtons::YesNo)
                    .default_button(StandardButton::No)
                    .escape_button(StandardButton::No)
                    .on_result(move |result, _ctx| {
                        if matches!(result.button, StandardButton::Yes) {
                            // Unlock, then let the close go on: the answer
                            // may run in the dialog's own window, and a
                            // quit waiting on this window continues too.
                            locked.set(false);
                            decision.proceed();
                        } else {
                            decision.cancel();
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
