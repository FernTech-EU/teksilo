// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Incremental "type to jump" search shared by the data views.
//!
//! Matches the Qt `keyboardSearch` / macOS & Windows type-select
//! convention: typing characters builds up a search term that jumps to
//! the next row whose label starts with it; a pause longer than the
//! timeout starts a fresh term.
//!
//! The state lives in a [`TypeAheadState`] held as a **persistent field**
//! on the owning widget (not created in `build()`), so the accumulated
//! buffer survives the selection-driven rebuilds each keystroke triggers
//! — otherwise multi-character search (`"ca"` → "California") could never
//! accumulate past one character.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Default reset window between keystrokes before the search term clears.
pub(crate) const DEFAULT_TYPE_AHEAD_TIMEOUT: Duration = Duration::from_millis(500);

/// Accumulated type-ahead buffer + the time of the last keystroke.
#[derive(Default)]
pub(crate) struct TypeAheadState {
    inner: RefCell<TypeAheadInner>,
}

#[derive(Default)]
struct TypeAheadInner {
    last: Option<Instant>,
    buffer: String,
}

impl TypeAheadState {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Fold `c` into the buffer (clearing it first when more than
    /// `timeout` has elapsed since the last keystroke) and return the next
    /// row index whose label starts with the buffer, searching forward
    /// from `current + 1` and wrapping around. `label(i)` yields the
    /// searchable text for row `i`, or `None` to skip a row whose data
    /// isn't resident (lazy sources). Returns `None` for control chars, a
    /// zero timeout, an empty model, or no match.
    pub(crate) fn search(
        &self,
        c: char,
        current: usize,
        count: usize,
        timeout: Duration,
        label: impl Fn(usize) -> Option<String>,
    ) -> Option<usize> {
        if timeout.is_zero() || c.is_control() || count == 0 {
            return None;
        }
        let now = Instant::now();
        let buffer = {
            let mut st = self.inner.borrow_mut();
            let stale = st.last.map(|t| now.duration_since(t) > timeout).unwrap_or(true);
            if stale {
                st.buffer.clear();
            }
            st.last = Some(now);
            st.buffer.push(c.to_ascii_lowercase());
            st.buffer.clone()
        };
        for offset in 1..=count {
            let i = (current + offset) % count;
            if let Some(text) = label(i) {
                if text.to_ascii_lowercase().starts_with(&buffer) {
                    return Some(i);
                }
            }
        }
        None
    }
}
