// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The application-level quit: [`EventContext::quit`] and
//! [`EventContext::quit_forced`], carried out by the window manager over the
//! ticks that follow.
//!
//! A quit asks the application first, through the guard declared with
//! [`TeksiloAppBuilder::on_quit_requested`](crate::TeksiloAppBuilder::on_quit_requested),
//! then every open window in turn, as if the user closed it. The order
//! is recomputed from the live windows at every step, so a window opened or
//! closed meanwhile is simply in the next list or not: modal windows first,
//! deepest first, so a dialog closes before the window it blocks; then the
//! others from the most recently used.
//!
//! The application is asked before any window closes, as Qt's
//! `QEvent::Quit` reaches the application before its windows are closed and
//! Electron's `before-quit` precedes the windows' `close`: a quit closes the
//! windows as it goes, so an application asked last could only refuse with
//! its windows already gone. (AppKit asks its delegate's
//! `applicationShouldTerminate:` after the document controller has reviewed
//! the documents, but that review saves documents and closes no window.)
//!
//! Each guard, the application's and the windows', answers as a window's
//! does for a close
//! ([`CloseResponse`](teksilo_core::CloseResponse)):
//!
//! - `Close`: the window closes and the quit goes on with the next one;
//! - `Veto`: the quit is abandoned at once;
//! - `Defer`: the quit *awaits* the window until its
//!   [`CloseDecision`](teksilo_core::CloseDecision) is answered: `proceed`
//!   closes it and the quit goes on, `cancel` (or dropping the decision)
//!   abandons the quit. The window is brought to the front meanwhile.
//!
//! A window whose own close is already deferred (its guard is waiting on the
//! user's answer to a close button) is not asked again: the quit adopts that
//! answer and awaits it first, so the user sees one question. The window
//! being awaited closed by another route (its dialog closing it by id) lets
//! the quit go on.
//!
//! A quit runs in one of three [`QuitMode`]s: guarded (the above), forced
//! ([`EventContext::quit_forced`]: no guard is asked, nothing is awaited),
//! or overruling, for a source that leaves no time to ask (a session
//! ending now, a termination signal): every guard is asked, so it can
//! save, but whatever it answers its window closes. A request while a quit
//! is under way joins it, and one in a stronger mode makes the quit under
//! way that mode, on behalf of its own source. Upgraded so, an answer
//! already given stands and a pending one is asked again: a window whose
//! guard already answered (a stored `proceed()`, Don't Save) closes without
//! being asked again, and an application guard that already agreed is not
//! asked again, while a guard whose answer is still pending (its box up) is
//! asked again with the new source, so it can save, and its box answers
//! Cancel as its window closes. A fresh overruling quit asks every guard
//! once. Once a quit has completed
//! the application is ending, and a request starts nothing: a service
//! manager that sends `SIGTERM` and `SIGHUP` together gets its guards asked
//! once.
//!
//! [`step`] is the decision, a pure function of the quit and of what the
//! windows look like; `WindowManager::advance_quit` carries it out.
//!
//! # Where a quit comes from
//!
//! Every source asks through one entry,
//! `WindowManager::request_quit_from(source, forced, reply)`: the
//! application (`ctx.quit()`, `ctx.quit_forced()`, the App menu's Quit) and
//! the platform's own sources ([`QuitSource`], also read by guards through
//! [`EventContext::quit_source`](teksilo_core::widget::EventContext::quit_source)). A source that must answer
//! the operating system once the quit is decided (AppKit's
//! `NSTerminateLater` then `replyToApplicationShouldTerminate:`) passes a
//! [`QuitReply`], told how the quit ends: [`QuitOutcome::Completed`] once
//! every window has closed, [`QuitOutcome::Cancelled`] when a guard refuses
//! or the quit is cancelled. A source with no time to ask (a session ending
//! now, Windows' `WM_QUERYENDSESSION`, a termination signal) calls
//! `TeksiloAppHandler::quit_now`, which runs an overruling quit to its end
//! before it returns: every guard not already answered is asked and reads
//! the source (once, unless a pending answer is asked again, as above), so a
//! document's guard saves without asking; a refusal or a deferred answer is
//! overruled, and every dialog still up, or asked and not shown yet,
//! answers through its own Cancel path as its window closes.
//!
//! [`EventContext::quit`]: teksilo_core::widget::EventContext::quit
//! [`EventContext::quit_forced`]: teksilo_core::widget::EventContext::quit_forced

use crate::window_config::TeksiloWindowId;

pub(crate) use teksilo_core::QuitSource;

/// How a quit treats the guards it asks. Ordered from the weakest: a
/// request in a stronger mode makes the quit under way that mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum QuitMode {
    /// Every guard is asked, and can refuse the quit or defer its answer.
    Guarded,
    /// Every guard is asked, so it can save, and cannot refuse: a veto or
    /// a deferred answer is overruled and the window closes. For a source
    /// that leaves no time to ask. A quit upgraded to it keeps the answers
    /// already given and asks the pending ones again (see the module
    /// documentation).
    Overruling,
    /// No guard is asked.
    Forced,
}

/// How a quit ended, told to its [`QuitReply`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitOutcome {
    /// Every window closed: the application is ending.
    Completed,
    /// A guard refused, a deferred answer cancelled, or the quit was
    /// cancelled: the application goes on.
    Cancelled,
}

/// Told how the quit it was passed with ends, once, on the event loop's
/// thread. A request made while a quit is under way adds its reply to that
/// quit's.
pub(crate) type QuitReply = Box<dyn FnOnce(QuitOutcome)>;

/// Whose deferred answer a quit waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Awaited {
    /// The application's guard.
    Application,
    /// A window's guard.
    Window(TeksiloWindowId),
}

/// A quit under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingQuit {
    /// How the guards are treated.
    pub(crate) mode: QuitMode,
    /// The application agreed to the quit (or has no guard to ask).
    pub(crate) application_agreed: bool,
    /// Whose deferred answer the quit waits on.
    pub(crate) awaiting: Option<Awaited>,
    /// Who asked for it first, or the request that made it overruling or
    /// forced: what a guard reads.
    pub(crate) source: QuitSource,
    /// Bumped whenever the quit is replaced or made forced, so a step that
    /// ran a guard can tell whether the guard changed the quit under it.
    pub(crate) generation: u64,
}

/// What the quit sees of one open window. A window already closing this
/// turn is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuitWindow {
    pub(crate) id: TeksiloWindowId,
    /// How many modal parents stand between this window and a window no
    /// modal blocks: `0` for an ordinary window, `1` for a dialog over one.
    pub(crate) modal_depth: usize,
    /// Its place in the activation order, higher for a window used later.
    pub(crate) recency: u64,
    /// Its guard deferred a close whose answer has not come yet.
    pub(crate) deferred: bool,
}

/// What a quit does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitStep {
    /// No window is left: the quit is complete and the application ends.
    Complete,
    /// The awaited guard has not answered yet.
    Wait,
    /// Ask the application's guard.
    AskApplication,
    /// This window's guard already deferred a close: await that answer
    /// instead of asking again. A guarded quit only.
    Adopt(TeksiloWindowId),
    /// Ask this window (or close it, for a forced quit; ask it and close
    /// it whatever it answers, for an overruling one).
    Ask(TeksiloWindowId),
}

/// The next step of `quit`, given the open windows.
pub(crate) fn step(quit: &PendingQuit, windows: &[QuitWindow]) -> QuitStep {
    match quit.awaiting {
        Some(Awaited::Application) => return QuitStep::Wait,
        Some(Awaited::Window(awaited)) if windows.iter().any(|window| window.id == awaited) => {
            return QuitStep::Wait;
        }
        _ => {}
    }
    if quit.mode != QuitMode::Forced && !quit.application_agreed {
        return QuitStep::AskApplication;
    }
    if windows.is_empty() {
        return QuitStep::Complete;
    }
    if quit.mode == QuitMode::Guarded
        && let Some(deferred) = windows
            .iter()
            .filter(|window| window.deferred)
            .max_by_key(|window| (window.recency, window.id.raw()))
    {
        return QuitStep::Adopt(deferred.id);
    }
    let next = windows
        .iter()
        .max_by_key(|window| (window.modal_depth, window.recency, window.id.raw()))
        .expect("not empty");
    QuitStep::Ask(next.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: u64) -> TeksiloWindowId {
        TeksiloWindowId::new(raw)
    }

    fn window(raw: u64, recency: u64) -> QuitWindow {
        QuitWindow {
            id: id(raw),
            modal_depth: 0,
            recency,
            deferred: false,
        }
    }

    fn deferred(raw: u64, recency: u64) -> QuitWindow {
        QuitWindow {
            deferred: true,
            ..window(raw, recency)
        }
    }

    /// A guarded quit the application has agreed to.
    fn quit() -> PendingQuit {
        PendingQuit {
            mode: QuitMode::Guarded,
            application_agreed: true,
            awaiting: None,
            source: QuitSource::Application,
            generation: 0,
        }
    }

    fn awaiting(raw: u64) -> PendingQuit {
        PendingQuit {
            awaiting: Some(Awaited::Window(id(raw))),
            ..quit()
        }
    }

    fn forced() -> PendingQuit {
        PendingQuit {
            mode: QuitMode::Forced,
            ..quit()
        }
    }

    fn overruling() -> PendingQuit {
        PendingQuit {
            mode: QuitMode::Overruling,
            ..quit()
        }
    }

    /// The application is asked first, before any window, and with no
    /// window open too.
    #[test]
    fn the_application_is_asked_before_any_window() {
        let fresh = PendingQuit {
            application_agreed: false,
            ..quit()
        };
        assert_eq!(step(&fresh, &[window(1, 5)]), QuitStep::AskApplication);
        assert_eq!(step(&fresh, &[deferred(1, 5)]), QuitStep::AskApplication);
        assert_eq!(step(&fresh, &[]), QuitStep::AskApplication);
    }

    #[test]
    fn a_quit_waits_on_the_application_whatever_the_windows() {
        let waiting = PendingQuit {
            application_agreed: false,
            awaiting: Some(Awaited::Application),
            ..quit()
        };
        assert_eq!(step(&waiting, &[window(1, 5)]), QuitStep::Wait);
        assert_eq!(step(&waiting, &[]), QuitStep::Wait);
    }

    /// A forced quit asks nobody, the application included.
    #[test]
    fn a_forced_quit_does_not_ask_the_application() {
        let forced = PendingQuit {
            application_agreed: false,
            ..forced()
        };
        assert_eq!(step(&forced, &[window(1, 5)]), QuitStep::Ask(id(1)));
        assert_eq!(step(&forced, &[]), QuitStep::Complete);
    }

    /// An overruling quit asks the application, so it can save, and
    /// every window, but awaits no answer: a window whose own close waits
    /// on the user is asked again rather than adopted, dialogs first.
    #[test]
    fn an_overruling_quit_asks_everyone_and_adopts_nothing() {
        let fresh = PendingQuit {
            application_agreed: false,
            ..overruling()
        };
        assert_eq!(step(&fresh, &[window(1, 5)]), QuitStep::AskApplication);
        assert_eq!(step(&fresh, &[]), QuitStep::AskApplication);
        let its_box = QuitWindow {
            modal_depth: 1,
            ..window(4, 1)
        };
        assert_eq!(
            step(&overruling(), &[deferred(2, 9), its_box]),
            QuitStep::Ask(id(4))
        );
        assert_eq!(
            step(&overruling(), &[deferred(2, 9), window(3, 1)]),
            QuitStep::Ask(id(2))
        );
        assert_eq!(step(&overruling(), &[]), QuitStep::Complete);
    }

    #[test]
    fn the_modes_are_ordered_from_the_weakest() {
        assert!(QuitMode::Guarded < QuitMode::Overruling);
        assert!(QuitMode::Overruling < QuitMode::Forced);
    }

    #[test]
    fn no_window_left_completes_the_quit() {
        assert_eq!(step(&quit(), &[]), QuitStep::Complete);
        assert_eq!(step(&awaiting(3), &[]), QuitStep::Complete);
    }

    #[test]
    fn the_most_recently_used_window_is_asked_first() {
        let windows = [window(1, 5), window(2, 9), window(3, 7)];
        assert_eq!(step(&quit(), &windows), QuitStep::Ask(id(2)));
    }

    #[test]
    fn a_dialog_is_asked_before_the_window_it_blocks_whatever_the_recency() {
        let dialog = QuitWindow {
            modal_depth: 1,
            ..window(4, 1)
        };
        let nested = QuitWindow {
            modal_depth: 2,
            ..window(5, 0)
        };
        assert_eq!(step(&quit(), &[window(1, 9), dialog]), QuitStep::Ask(id(4)));
        assert_eq!(
            step(&quit(), &[window(1, 9), dialog, nested]),
            QuitStep::Ask(id(5)),
            "the deepest dialog first"
        );
    }

    /// The awaited window waits for its answer, whatever else is open: an
    /// unrelated dialog or palette has no say in it.
    #[test]
    fn the_awaited_window_open_is_waited_on() {
        assert_eq!(
            step(&awaiting(1), &[deferred(1, 9), window(2, 1)]),
            QuitStep::Wait
        );
    }

    #[test]
    fn the_awaited_window_closed_lets_the_quit_go_on() {
        assert_eq!(
            step(&awaiting(1), &[window(2, 1), window(3, 4)]),
            QuitStep::Ask(id(3))
        );
    }

    /// A window whose own close is waiting on the user is not asked a
    /// second question, even when a dialog of it would be asked first: the
    /// dialog is the question.
    #[test]
    fn a_window_already_deferred_is_adopted_before_anything_is_asked() {
        let its_box = QuitWindow {
            modal_depth: 1,
            ..window(4, 10)
        };
        assert_eq!(
            step(&quit(), &[window(1, 9), deferred(2, 3), its_box]),
            QuitStep::Adopt(id(2))
        );
        assert_eq!(
            step(&quit(), &[deferred(1, 3), deferred(2, 5)]),
            QuitStep::Adopt(id(2)),
            "the one used last first"
        );
    }

    /// A forced quit asks nobody, so it adopts nothing either: it closes.
    #[test]
    fn a_forced_quit_closes_a_deferred_window_rather_than_waiting() {
        assert_eq!(
            step(&forced(), &[deferred(1, 3), window(2, 1)]),
            QuitStep::Ask(id(1))
        );
    }

    /// Equal recency (never true of two live windows, but the order must
    /// not depend on the slice's order either way).
    #[test]
    fn the_order_does_not_depend_on_the_input_order() {
        let a = [window(1, 4), window(2, 4)];
        let b = [window(2, 4), window(1, 4)];
        assert_eq!(step(&quit(), &a), step(&quit(), &b));
        let a = [deferred(1, 4), deferred(2, 4)];
        let b = [deferred(2, 4), deferred(1, 4)];
        assert_eq!(step(&quit(), &a), step(&quit(), &b));
    }

    /// Exhaustive over small window sets: a step never names a window that
    /// is not open, only an empty set completes, a quit waits only on an
    /// open awaited window, only a guarded quit waits or adopts, a forced
    /// quit never asks the application, and only a quit the application has
    /// not agreed to asks it.
    #[test]
    fn exhaustive_small_sets() {
        for count in 0..=4u64 {
            for mask in 0..(1u32 << (count * 2)) {
                let windows: Vec<QuitWindow> = (0..count)
                    .map(|i| QuitWindow {
                        id: id(i + 1),
                        modal_depth: ((mask >> (i * 2)) & 1) as usize,
                        recency: (i * 7 + mask as u64) % 5,
                        deferred: (mask >> (i * 2 + 1)) & 1 == 1,
                    })
                    .collect();
                let open = |w: TeksiloWindowId| windows.iter().any(|x| x.id == w);
                for awaited in 0..=count + 1 {
                    for (mode, application_agreed) in
                        [QuitMode::Guarded, QuitMode::Overruling, QuitMode::Forced]
                            .into_iter()
                            .flat_map(|mode| [(mode, false), (mode, true)])
                    {
                        let guarded = mode == QuitMode::Guarded;
                        let quit = PendingQuit {
                            mode,
                            application_agreed,
                            awaiting: (awaited > 0 && guarded)
                                .then(|| Awaited::Window(id(awaited))),
                            ..quit()
                        };
                        match step(&quit, &windows) {
                            QuitStep::Complete => assert!(windows.is_empty()),
                            QuitStep::AskApplication => {
                                assert!(mode != QuitMode::Forced && !application_agreed);
                            }
                            QuitStep::Ask(w) => assert!(open(w)),
                            QuitStep::Adopt(w) => {
                                assert!(guarded, "only a guarded quit adopts");
                                assert!(open(w));
                            }
                            QuitStep::Wait => {
                                assert!(guarded, "only a guarded quit waits");
                                let Some(Awaited::Window(w)) = quit.awaiting else {
                                    panic!("only an awaiting quit waits");
                                };
                                assert!(open(w), "a closed awaited window never stalls the quit");
                            }
                        }
                    }
                }
            }
        }
    }
}
