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
    /// row index whose label matches, searching forward from `current + 1`
    /// and wrapping around. `label(i)` yields the searchable text for row
    /// `i`, or `None` to skip a row whose data isn't resident (lazy
    /// sources). Returns `None` for control chars, a zero timeout, an empty
    /// model, or no match.
    ///
    /// Mixed input grows a prefix search as usual (`"c"` then `"r"` →
    /// `"cr"` → "Cranberry"). Repeating the SAME letter is the Qt
    /// `keyboardSearch` / macOS & Windows type-select convention instead:
    /// it does **not** grow the buffer into a (typically non-matching)
    /// `"bb"` prefix — each repeat cycles to the *next* row starting with
    /// that one letter, wrapping past the last match back to the first.
    /// The buffer itself still accumulates the repeats (so a later
    /// different letter starts a fresh multi-char term); only the term
    /// compared against row labels collapses to the single letter.
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
            let stale = st
                .last
                .map(|t| now.duration_since(t) > timeout)
                .unwrap_or(true);
            if stale {
                st.buffer.clear();
            }
            st.last = Some(now);
            st.buffer.push(c.to_ascii_lowercase());
            st.buffer.clone()
        };
        // `buffer` is non-empty (just pushed to) so `first` always exists.
        let first = buffer.chars().next().unwrap();
        let term: &str = if buffer.chars().all(|ch| ch == first) {
            &buffer[..first.len_utf8()]
        } else {
            &buffer
        };
        for offset in 1..=count {
            let i = (current + offset) % count;
            if let Some(text) = label(i) {
                if text.to_ascii_lowercase().starts_with(term) {
                    return Some(i);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &'static [&'static str]) -> impl Fn(usize) -> Option<String> {
        move |i| rows.get(i).map(|s| s.to_string())
    }

    #[test]
    fn repeating_the_same_letter_cycles_instead_of_growing_the_prefix() {
        // Three "B..." rows among others. Naive prefix growth would turn
        // the second 'b' into "bb", matching nothing; the fix cycles
        // forward through the B-rows instead, and the fourth press wraps
        // back around to the first one.
        let rows: &'static [&'static str] = &["Banana", "Apple", "Blueberry", "Cherry", "Bilberry"];
        let state = TypeAheadState::new();
        let timeout = DEFAULT_TYPE_AHEAD_TIMEOUT;

        let i1 = state
            .search('b', 0, rows.len(), timeout, labels(rows))
            .unwrap();
        assert_eq!(i1, 2, "first 'b' from row 0 finds Blueberry");

        let i2 = state
            .search('b', i1, rows.len(), timeout, labels(rows))
            .unwrap();
        assert_eq!(i2, 4, "second 'b' cycles forward to Bilberry, not \"bb\"");

        let i3 = state
            .search('b', i2, rows.len(), timeout, labels(rows))
            .unwrap();
        assert_eq!(i3, 0, "third 'b' wraps back around to Banana");
    }

    #[test]
    fn mixed_letters_still_grow_a_multi_char_prefix_search() {
        // The repeated-letter cycling must not break ordinary multi-char
        // type-ahead: distinct keystrokes keep accumulating into a longer
        // prefix, same as before this change.
        let rows: &'static [&'static str] = &["Apple", "Cherry", "Cranberry", "Date"];
        let state = TypeAheadState::new();
        let timeout = DEFAULT_TYPE_AHEAD_TIMEOUT;

        let i1 = state
            .search('c', 0, rows.len(), timeout, labels(rows))
            .unwrap();
        assert_eq!(i1, 1, "'c' finds Cherry");

        let i2 = state
            .search('r', i1, rows.len(), timeout, labels(rows))
            .unwrap();
        assert_eq!(i2, 2, "'cr' narrows to Cranberry");
    }
}
