// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The document lock of a TSF text store (`ITextStoreACP::RequestLock`): the
//! half that needs no platform.
//!
//! The rules, from Microsoft's `RequestLock` remarks and the design document
//! (`engineering/docs/tsf-text-store.md`, 4.7):
//!
//! - With no lock held, a request is granted at once (`OnLockGranted` from
//!   inside `RequestLock`).
//! - While a lock is held (a text service asks again from inside its own
//!   grant), a synchronous request is refused (`TS_E_SYNCHRONOUS`) and an
//!   asynchronous one is queued (`TS_S_ASYNC`). Queued requests coalesce into
//!   one pending grant, read-write if any of them asked for it, which is
//!   granted as soon as the held lock is released, still inside the outer
//!   `RequestLock`.
//! - A read-only lock is never upgraded in place; the write request waits
//!   for the grant that follows.
//! - With no lock held but the document not ready to be read (changes not
//!   reported to the text service yet, or the widget busy), a synchronous
//!   request is refused and an asynchronous one **deferred**
//!   ([`LockArbiter::defer`]): `RequestLock` returns, the store reports what
//!   changed, then grants it ([`LockArbiter::take_deferred`]), as Microsoft
//!   asks ("the application can only respond to the asynchronous lock
//!   request").

#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "only the Windows store takes locks")
)]

/// `TS_LF_SYNC`.
pub(crate) const LF_SYNC: u32 = 0x1;
/// `TS_LF_READ`.
pub(crate) const LF_READ: u32 = 0x2;
/// `TS_LF_READWRITE`.
pub(crate) const LF_READWRITE: u32 = 0x6;

/// A kind of lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockKind {
    Read,
    ReadWrite,
}

impl LockKind {
    /// The kind a request's flags ask for: read-write when the write bit is
    /// set, read otherwise.
    pub(crate) fn of(flags: u32) -> Self {
        if flags & LF_READWRITE == LF_READWRITE {
            Self::ReadWrite
        } else {
            Self::Read
        }
    }

    /// The flags `OnLockGranted` is called with.
    pub(crate) fn flags(self) -> u32 {
        match self {
            Self::Read => LF_READ,
            Self::ReadWrite => LF_READWRITE,
        }
    }
}

/// What `RequestLock` does with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockDecision {
    /// Grant it now.
    Grant(LockKind),
    /// Queued behind the held lock (`TS_S_ASYNC`).
    Queued,
    /// A synchronous request while a lock is held (`TS_E_SYNCHRONOUS`).
    Refused,
}

/// The store's lock state.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LockArbiter {
    held: Option<LockKind>,
    pending: Option<LockKind>,
    deferred: Option<LockKind>,
}

impl LockArbiter {
    /// The lock held now, if any.
    pub(crate) fn held(&self) -> Option<LockKind> {
        self.held
    }

    /// Whether reads are allowed now.
    pub(crate) fn can_read(&self) -> bool {
        self.held.is_some()
    }

    /// Whether writes are allowed now.
    pub(crate) fn can_write(&self) -> bool {
        self.held == Some(LockKind::ReadWrite)
    }

    /// A text service asks for a lock with `flags`. A [`LockDecision::Grant`]
    /// holds the lock until [`release`](Self::release).
    pub(crate) fn request(&mut self, flags: u32) -> LockDecision {
        let kind = LockKind::of(flags);
        if self.held.is_none() {
            self.held = Some(kind);
            return LockDecision::Grant(kind);
        }
        if flags & LF_SYNC != 0 {
            return LockDecision::Refused;
        }
        self.pending = Some(coalesce(self.pending, kind));
        LockDecision::Queued
    }

    /// A request that cannot be granted yet (the document has changes to
    /// report, or is busy): refused when synchronous, else kept, coalesced
    /// with any other kept, for [`take_deferred`](Self::take_deferred).
    pub(crate) fn defer(&mut self, flags: u32) -> LockDecision {
        if flags & LF_SYNC != 0 {
            return LockDecision::Refused;
        }
        self.deferred = Some(coalesce(self.deferred, LockKind::of(flags)));
        LockDecision::Queued
    }

    /// The deferred request, held from now on, when no lock is.
    pub(crate) fn take_deferred(&mut self) -> Option<LockKind> {
        if self.held.is_some() {
            return None;
        }
        self.held = self.deferred.take();
        self.held
    }

    /// The text service's `OnLockGranted` returned: the lock is released,
    /// and the pending grant, if any, is taken and held now.
    pub(crate) fn release(&mut self) -> Option<LockKind> {
        self.held = self.pending.take();
        self.held
    }
}

/// A kept request joined by `kind`: read-write if either is.
fn coalesce(kept: Option<LockKind>, kind: LockKind) -> LockKind {
    match (kept, kind) {
        (Some(LockKind::ReadWrite), _) | (_, LockKind::ReadWrite) => LockKind::ReadWrite,
        _ => LockKind::Read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_store_grants_at_once() {
        let mut locks = LockArbiter::default();
        assert_eq!(
            locks.request(LF_READ | LF_SYNC),
            LockDecision::Grant(LockKind::Read)
        );
        assert!(locks.can_read());
        assert!(!locks.can_write());
        assert_eq!(locks.release(), None);
        assert!(!locks.can_read());
        assert_eq!(
            locks.request(LF_READWRITE),
            LockDecision::Grant(LockKind::ReadWrite)
        );
        assert!(locks.can_write());
    }

    #[test]
    fn a_held_lock_refuses_a_synchronous_request_and_queues_others() {
        let mut locks = LockArbiter::default();
        locks.request(LF_READ);
        assert_eq!(locks.request(LF_READWRITE | LF_SYNC), LockDecision::Refused);
        assert_eq!(locks.request(LF_READ), LockDecision::Queued);
        assert_eq!(locks.request(LF_READWRITE), LockDecision::Queued);
        assert_eq!(locks.request(LF_READ), LockDecision::Queued);
        assert_eq!(
            locks.release(),
            Some(LockKind::ReadWrite),
            "coalesced, read-write since one asked"
        );
        assert!(locks.can_write(), "held by the pending grant");
        assert_eq!(locks.release(), None);
    }

    #[test]
    fn a_request_that_cannot_be_granted_yet_is_deferred_until_taken() {
        let mut locks = LockArbiter::default();
        assert_eq!(locks.defer(LF_READ | LF_SYNC), LockDecision::Refused);
        assert_eq!(locks.take_deferred(), None, "nothing deferred");
        assert_eq!(locks.defer(LF_READ), LockDecision::Queued);
        assert_eq!(locks.defer(LF_READWRITE), LockDecision::Queued);
        assert!(!locks.can_read(), "nothing held yet");
        assert_eq!(locks.take_deferred(), Some(LockKind::ReadWrite));
        assert!(locks.can_write());
        assert_eq!(locks.take_deferred(), None, "taken once");
        assert_eq!(locks.release(), None);
    }

    #[test]
    fn kinds_and_flags() {
        assert_eq!(LockKind::of(LF_READ), LockKind::Read);
        assert_eq!(LockKind::of(LF_READWRITE | LF_SYNC), LockKind::ReadWrite);
        assert_eq!(LockKind::Read.flags(), 0x2);
        assert_eq!(LockKind::ReadWrite.flags(), 0x6);
    }
}
