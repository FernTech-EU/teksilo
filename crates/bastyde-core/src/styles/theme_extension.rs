//! Typed extension registry attached to a [`Theme`](crate::styles::Theme).
//!
//! Apps and downstream crates use this to attach typed values that the
//! core theme struct doesn't know about — a syntax-color palette for a
//! code editor, a data-viz palette for a dashboard, per-app default
//! variants — without modifying [`Theme`](crate::styles::Theme) or any
//! of its token sub-structs.
//!
//! Lookup is by Rust type (`TypeId`); each `T` has at most one slot.
//!
//! ```ignore
//! pub struct MySyntaxPalette { /* ... */ }
//!
//! let theme = bastyde_core::presets::intui::light()
//!     .with_extension(MySyntaxPalette { /* ... */ });
//!
//! // Later, anywhere with a &Theme:
//! if let Some(syntax) = theme.extension::<MySyntaxPalette>() {
//!     // ...
//! }
//! ```
//!
//! Extensions are skipped during serde round-trips — they re-attach at
//! runtime from app code, not from theme JSON.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ThemeExtensions {
    map: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl ThemeExtensions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|a| a.downcast_ref::<T>())
    }

    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub fn remove<T: Any + Send + Sync>(&mut self) -> bool {
        self.map.remove(&TypeId::of::<T>()).is_some()
    }

    pub fn contains<T: Any + Send + Sync>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl fmt::Debug for ThemeExtensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ThemeExtensions({} entries)", self.map.len())
    }
}

impl PartialEq for ThemeExtensions {
    /// Equality compares only which extension types are registered, not
    /// the inner values (`dyn Any` has no `PartialEq`).
    fn eq(&self, other: &Self) -> bool {
        if self.map.len() != other.map.len() {
            return false;
        }
        self.map.keys().all(|k| other.map.contains_key(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Marker(u32);

    #[derive(Debug, Clone, PartialEq)]
    struct Other(&'static str);

    #[test]
    fn insert_and_get_round_trip() {
        let mut ext = ThemeExtensions::new();
        ext.insert(Marker(7));
        assert_eq!(ext.get::<Marker>(), Some(&Marker(7)));
        assert!(ext.contains::<Marker>());
        assert_eq!(ext.len(), 1);
    }

    #[test]
    fn distinct_types_share_the_registry() {
        let mut ext = ThemeExtensions::new();
        ext.insert(Marker(1));
        ext.insert(Other("hi"));
        assert_eq!(ext.len(), 2);
        assert_eq!(ext.get::<Marker>(), Some(&Marker(1)));
        assert_eq!(ext.get::<Other>(), Some(&Other("hi")));
    }

    #[test]
    fn remove_drops_the_slot() {
        let mut ext = ThemeExtensions::new();
        ext.insert(Marker(1));
        assert!(ext.remove::<Marker>());
        assert!(!ext.contains::<Marker>());
        assert!(!ext.remove::<Marker>());
    }

    #[test]
    fn debug_format_is_short() {
        let mut ext = ThemeExtensions::new();
        ext.insert(Marker(1));
        let s = format!("{ext:?}");
        assert!(s.contains("1 entries"), "got: {s}");
    }

    #[test]
    fn equality_ignores_inner_values() {
        let mut a = ThemeExtensions::new();
        let mut b = ThemeExtensions::new();
        a.insert(Marker(1));
        b.insert(Marker(2));
        // Same TypeId, different value — still equal under our definition.
        assert_eq!(a, b);
    }
}
