// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The application-level quit: [`EventContext::request_quit`] and
//! [`EventContext::quit`], carried out by the window manager over the ticks
//! that follow.
//!
//! A quit asks every open window in turn, as if the user closed it. The order
//! is recomputed from the live windows at every step, so a window opened or
//! closed meanwhile is simply in the next list or not: modal windows first,
//! deepest first, so a dialog closes before the window it blocks; then the
//! others from the most recently used.
//!
//! A close guard that vetoes stops the quit at its window, which it then
//! *awaits*. The guard has usually opened a dialog (veto-then-reissue: the
//! dialog's answer closes the window, or does not), and nothing tells the
//! framework that the dialog was cancelled, so the quit reads the window
//! instead:
//!
//! - the window closed: the quit goes on with the next one;
//! - it is still open with a dialog up (a native modal child, a modal
//!   overlay, or a modal request still queued for presentation): it waits;
//! - it is still open with no dialog: the guard vetoed without asking, or
//!   the dialog was cancelled, and the quit is abandoned.
//!
//! Closing any other window meanwhile also abandons it; a close of the
//! awaited window or of its modal children is the dialog at work, not a new
//! request. A second guarded request while a quit is under way changes
//! nothing, since asking the awaited window again would only open a second
//! dialog; a forced one ([`EventContext::quit`]) turns the quit under way
//! into a forced one.
//!
//! [`step`] is the decision, a pure function of the quit and of what the
//! windows look like; `WindowManager::advance_quit` carries it out.
//!
//! [`EventContext::request_quit`]: teksilo_core::widget::EventContext::request_quit
//! [`EventContext::quit`]: teksilo_core::widget::EventContext::quit

use crate::window_config::TeksiloWindowId;

/// A quit under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingQuit {
    /// Close every window without asking its guard.
    pub(crate) skip_guards: bool,
    /// The window whose guard vetoed, which the quit waits on.
    pub(crate) awaiting: Option<TeksiloWindowId>,
    /// Bumped whenever the quit is replaced or made forced, so a step that
    /// ran a guard can tell whether the guard changed the quit under it.
    pub(crate) generation: u64,
}

/// What the quit sees of one open window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuitWindow {
    pub(crate) id: TeksiloWindowId,
    /// How many modal parents stand between this window and a window no
    /// modal blocks: `0` for an ordinary window, `1` for a dialog over one.
    pub(crate) modal_depth: usize,
    /// Its place in the activation order, higher for a window used later.
    pub(crate) recency: u64,
    /// A dialog is up in or over it: a native modal child, a modal overlay,
    /// or a modal request queued for presentation.
    pub(crate) dialog_open: bool,
}

/// What a quit does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitStep {
    /// No window is left: the quit is complete and the application ends.
    Complete,
    /// The awaited window is open with no dialog: give up.
    Abandon,
    /// The awaited window still has its dialog up.
    Wait,
    /// Ask this window (or close it, for a forced quit).
    Ask(TeksiloWindowId),
}

/// The next step of `quit`, given the open windows.
pub(crate) fn step(quit: &PendingQuit, windows: &[QuitWindow]) -> QuitStep {
    if windows.is_empty() {
        return QuitStep::Complete;
    }
    if let Some(awaited) = quit.awaiting
        && let Some(window) = windows.iter().find(|window| window.id == awaited)
    {
        return if window.dialog_open {
            QuitStep::Wait
        } else {
            QuitStep::Abandon
        };
    }
    let next = windows
        .iter()
        .max_by_key(|window| (window.modal_depth, window.recency, window.id.raw()))
        .expect("not empty");
    QuitStep::Ask(next.id)
}

/// Whether a close request for `closing` abandons a quit awaiting
/// `awaiting`. `parent_of` gives a window's modal parent.
pub(crate) fn close_abandons(
    awaiting: Option<TeksiloWindowId>,
    closing: TeksiloWindowId,
    parent_of: impl Fn(TeksiloWindowId) -> Option<TeksiloWindowId>,
) -> bool {
    let Some(awaited) = awaiting else {
        return false;
    };
    let mut window = Some(closing);
    // A modal chain is short and acyclic; the bound only keeps a corrupt
    // map from hanging the loop.
    for _ in 0..64 {
        match window {
            Some(id) if id == awaited => return false,
            Some(id) => window = parent_of(id),
            None => return true,
        }
    }
    true
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
            dialog_open: false,
        }
    }

    fn quit() -> PendingQuit {
        PendingQuit {
            skip_guards: false,
            awaiting: None,
            generation: 0,
        }
    }

    fn awaiting(raw: u64) -> PendingQuit {
        PendingQuit {
            awaiting: Some(id(raw)),
            ..quit()
        }
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

    #[test]
    fn the_awaited_window_with_its_dialog_up_is_waited_on() {
        let awaited = QuitWindow {
            dialog_open: true,
            ..window(1, 9)
        };
        assert_eq!(step(&awaiting(1), &[awaited, window(2, 1)]), QuitStep::Wait);
    }

    #[test]
    fn the_awaited_window_left_without_a_dialog_abandons_the_quit() {
        assert_eq!(
            step(&awaiting(1), &[window(1, 9), window(2, 1)]),
            QuitStep::Abandon
        );
    }

    #[test]
    fn the_awaited_window_closed_lets_the_quit_go_on() {
        assert_eq!(
            step(&awaiting(1), &[window(2, 1), window(3, 4)]),
            QuitStep::Ask(id(3))
        );
    }

    /// Equal recency (never true of two live windows, but the order must
    /// not depend on the slice's order either way).
    #[test]
    fn the_order_does_not_depend_on_the_input_order() {
        let a = [window(1, 4), window(2, 4)];
        let b = [window(2, 4), window(1, 4)];
        assert_eq!(step(&quit(), &a), step(&quit(), &b));
    }

    #[test]
    fn closing_another_window_abandons_but_the_dialog_at_work_does_not() {
        // 1 is awaited; 4 is a dialog over 1; 5 a dialog over 4; 2 another
        // window.
        let parent_of = |w: TeksiloWindowId| match w.raw() {
            4 => Some(id(1)),
            5 => Some(id(4)),
            _ => None,
        };
        assert!(!close_abandons(Some(id(1)), id(1), parent_of));
        assert!(!close_abandons(Some(id(1)), id(4), parent_of));
        assert!(!close_abandons(Some(id(1)), id(5), parent_of));
        assert!(close_abandons(Some(id(1)), id(2), parent_of));
        assert!(
            !close_abandons(None, id(2), parent_of),
            "a quit that awaits nothing is not disturbed"
        );
    }

    #[test]
    fn a_cyclic_parent_map_does_not_hang() {
        let parent_of = |w: TeksiloWindowId| Some(id(if w.raw() == 7 { 8 } else { 7 }));
        assert!(close_abandons(Some(id(1)), id(7), parent_of));
    }

    /// Exhaustive over small window sets: whatever the windows, a quit whose
    /// awaited window is gone never waits or abandons, a step never asks a
    /// window that is not open, and only an empty set completes.
    #[test]
    fn exhaustive_small_sets() {
        for count in 0..=4u64 {
            for mask in 0..(1u32 << (count * 2)) {
                let windows: Vec<QuitWindow> = (0..count)
                    .map(|i| QuitWindow {
                        id: id(i + 1),
                        modal_depth: ((mask >> (i * 2)) & 1) as usize,
                        recency: (i * 7 + mask as u64) % 5,
                        dialog_open: (mask >> (i * 2 + 1)) & 1 == 1,
                    })
                    .collect();
                for awaited in 0..=count + 1 {
                    let quit = PendingQuit {
                        awaiting: (awaited > 0).then(|| id(awaited)),
                        ..quit()
                    };
                    let open = |w: TeksiloWindowId| windows.iter().any(|x| x.id == w);
                    match step(&quit, &windows) {
                        QuitStep::Complete => assert!(windows.is_empty()),
                        QuitStep::Ask(w) => assert!(open(w)),
                        QuitStep::Wait | QuitStep::Abandon => {
                            let w = quit.awaiting.expect("only an awaiting quit waits");
                            assert!(open(w), "a closed awaited window never stalls the quit");
                        }
                    }
                }
            }
        }
    }
}
