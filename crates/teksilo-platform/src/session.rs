// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Whether anything on screen can be seen at all: on macOS, the displays
//! asleep, the screen locked, or the user's session switched out.
//!
//! AppKit reports a window not visible when another window covers it, when
//! it is minimised, and when it was never shown. A window already showing
//! stays visible to it while the displays sleep or the screen is locked, so
//! wgpu goes on acquiring its frames and the window goes on drawing them for
//! nobody. A [`SessionWatch`] hears those changes from the system instead:
//! `NSWorkspace`'s screens-did-sleep and session-did-resign-active
//! notifications and their opposites, and the distributed
//! `com.apple.screenIsLocked` and `com.apple.screenIsUnlocked`, which Apple
//! does not document but macOS has posted for many releases.
//!
//! Each of the three reasons is set and cleared by its own notifications. A
//! clearing notification that never came would leave every window drawing
//! nothing, so [`SessionWatch::recheck`] reads what the system reports and
//! clears a reason it no longer does. It never sets one: a reading that is
//! wrong can only mean frames drawn for nobody, which is what happened
//! before there was a watch. For the same reason the watch starts with no
//! reason set, even in a session already locked when it starts.
//!
//! There is no watch elsewhere. A Wayland compositor stops asking a surface
//! it does not show for frames, which already holds a window's redraws.

#[cfg(target_os = "macos")]
mod macos;

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// One reason nothing can be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Reason {
    /// The displays sleep.
    ScreensAsleep,
    /// Another user's session has the console.
    SessionInactive,
    /// The screen is locked.
    ScreenLocked,
}

impl Reason {
    fn bit(self) -> u8 {
        match self {
            Reason::ScreensAsleep => 1,
            Reason::SessionInactive => 1 << 1,
            Reason::ScreenLocked => 1 << 2,
        }
    }
}

/// What the system reports now, each `None` where it cannot say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Reading {
    screens_asleep: Option<bool>,
    on_console: Option<bool>,
    locked: Option<bool>,
}

/// The reasons set now, written from whatever thread the system posts its
/// notifications on.
#[derive(Debug, Default)]
struct Reasons(AtomicU8);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Reasons {
    /// Set or clear `reason`; whether that changed [`away`](Self::away).
    fn set(&self, reason: Reason, on: bool) -> bool {
        let bit = reason.bit();
        let before = if on {
            self.0.fetch_or(bit, Ordering::AcqRel)
        } else {
            self.0.fetch_and(!bit, Ordering::AcqRel)
        };
        let after = if on { before | bit } else { before & !bit };
        (before != 0) != (after != 0)
    }

    fn away(&self) -> bool {
        self.0.load(Ordering::Acquire) != 0
    }

    /// Clear each reason `reading` says no longer holds; whether that
    /// changed [`away`](Self::away).
    fn recheck(&self, reading: Reading) -> bool {
        let mut changed = false;
        if reading.screens_asleep == Some(false) {
            changed |= self.set(Reason::ScreensAsleep, false);
        }
        if reading.on_console == Some(true) {
            changed |= self.set(Reason::SessionInactive, false);
        }
        if reading.locked == Some(false) {
            changed |= self.set(Reason::ScreenLocked, false);
        }
        changed
    }
}

/// Watches whether anything on screen can be seen; see the
/// [module](self). Dropping it stops the watch.
pub struct SessionWatch {
    reasons: Arc<Reasons>,
    #[cfg(target_os = "macos")]
    _observer: macos::Observer,
}

impl std::fmt::Debug for SessionWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionWatch")
            .field("away", &self.away())
            .finish()
    }
}

impl SessionWatch {
    /// Start watching, on the main thread. `changed` runs each time
    /// [`away`](Self::away) changes on a notification, on whatever thread
    /// the system posts it from. `None` on every platform but macOS, and off
    /// the main thread.
    pub fn new(changed: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            let reasons = Arc::new(Reasons::default());
            let observer = macos::Observer::new(Arc::clone(&reasons), Box::new(changed))?;
            Some(Self {
                reasons,
                _observer: observer,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = changed;
            None
        }
    }

    /// Whether nothing on screen can be seen now: the displays sleep, the
    /// screen is locked, or another session has the console.
    pub fn away(&self) -> bool {
        self.reasons.away()
    }

    /// Clear each reason the system no longer reports, should the
    /// notification that clears it not have come; whether
    /// [`away`](Self::away) changed. For while it is away, on the main
    /// thread.
    pub fn recheck(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.reasons.recheck(macos::reading())
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reading, Reason, Reasons};

    /// Away while any reason holds; only the first reason set and the last
    /// one cleared change it.
    #[test]
    fn away_while_any_reason_holds() {
        let reasons = Reasons::default();
        assert!(!reasons.away());
        assert!(reasons.set(Reason::ScreensAsleep, true));
        assert!(!reasons.set(Reason::ScreenLocked, true), "already away");
        assert!(!reasons.set(Reason::ScreensAsleep, true), "set twice");
        assert!(!reasons.set(Reason::ScreensAsleep, false), "still locked");
        assert!(reasons.away());
        assert!(reasons.set(Reason::ScreenLocked, false));
        assert!(!reasons.away());
        assert!(!reasons.set(Reason::SessionInactive, false), "never set");
    }

    /// A reading clears only the reasons it says no longer hold, and sets
    /// none.
    #[test]
    fn a_reading_clears_and_never_sets() {
        let reasons = Reasons::default();
        let everything = Reading {
            screens_asleep: Some(true),
            on_console: Some(false),
            locked: Some(true),
        };
        assert!(!reasons.recheck(everything));
        assert!(!reasons.away(), "a reading sets nothing");

        reasons.set(Reason::ScreensAsleep, true);
        reasons.set(Reason::ScreenLocked, true);
        assert!(!reasons.recheck(Reading::default()), "it cannot say");
        let awake = Reading {
            screens_asleep: Some(false),
            ..Reading::default()
        };
        assert!(!reasons.recheck(awake), "the screen is still locked");
        assert!(reasons.away());
        let unlocked = Reading {
            locked: Some(false),
            ..Reading::default()
        };
        assert!(reasons.recheck(unlocked));
        assert!(!reasons.away());

        reasons.set(Reason::SessionInactive, true);
        let console = Reading {
            on_console: Some(true),
            ..Reading::default()
        };
        assert!(reasons.recheck(console));
        assert!(!reasons.away());
    }
}
