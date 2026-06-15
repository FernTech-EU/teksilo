// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Process-unique identifier for a menu item.
//!
//! A [`MenuItemId`] is the stable token that ties a logical menu item (declared
//! in the `bastyde-widgets` `MenuModel`) to its mirror in a native platform menu
//! (`NSMenuItem` on macOS, `HMENU` item on Windows, …). It is a plain `u64`
//! newtype — `Send + Sync + Copy` — so it can ride inside an `AppEvent::External`
//! payload posted from a platform menu callback back to the UI loop, where the
//! app resolves it to the item's intent / action.
//!
//! Ids are allocated from a single process-wide monotonic counter and never
//! reused. They live in `bastyde-core` (not `bastyde-widgets`) so that
//! `bastyde-platform` — which renders the native menu and posts the click
//! payload — can name the type without depending on the widget layer.

use std::sync::atomic::{AtomicU64, Ordering};

/// A process-unique, never-reused menu item identifier.
///
/// Allocate with [`MenuItemId::next`]. The numeric value is opaque to
/// application code; it exists only to correlate a native menu item with the
/// logical item that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuItemId(u64);

static NEXT_MENU_ITEM_ID: AtomicU64 = AtomicU64::new(1);

impl MenuItemId {
    /// Allocate the next process-unique id.
    pub fn next() -> Self {
        Self(NEXT_MENU_ITEM_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw numeric id (for the platform layer's item-tag round-trip).
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Reconstruct an id from a raw value carried back across the platform
    /// boundary (e.g. an `NSMenuItem.tag`). Intended for the platform backend,
    /// not application code.
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_monotonic() {
        let a = MenuItemId::next();
        let b = MenuItemId::next();
        assert_ne!(a, b);
        assert!(b.raw() > a.raw());
    }

    #[test]
    fn raw_roundtrips() {
        let a = MenuItemId::next();
        assert_eq!(MenuItemId::from_raw(a.raw()), a);
    }
}
