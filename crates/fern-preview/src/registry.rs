//! Registry helpers — thin wrappers over `inventory::iter` for the
//! previewer's typical lookup paths.

use crate::catalog::CatalogEntry;

/// Iterate over every catalog entry registered into the current binary's
/// link graph. Order is stable across runs but not lexicographic — sort
/// callers if a particular ordering is required.
pub fn iter_entries() -> impl Iterator<Item = &'static dyn CatalogEntry> {
    inventory::iter::<&'static dyn CatalogEntry>().copied()
}

/// Find an entry by its widget id (`Button::id()`).
pub fn find_by_id(id: &str) -> Option<&'static dyn CatalogEntry> {
    iter_entries().find(|e| e.id() == id)
}

/// Find an entry whose `source().file` matches `path` (suffix match —
/// see [`crate::source_loc::SourceLoc::matches_path`]).
pub fn find_by_file(path: &str) -> Option<&'static dyn CatalogEntry> {
    iter_entries().find(|e| e.source().matches_path(path))
}

/// Group entries by `group()` and return them sorted within each
/// group. The outer `Vec` is sorted by group name. The inner `Vec`s
/// are sorted by display name.
pub fn entries_by_group() -> Vec<(&'static str, Vec<&'static dyn CatalogEntry>)> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<&'static str, Vec<&'static dyn CatalogEntry>> = BTreeMap::new();
    for e in iter_entries() {
        map.entry(e.group()).or_default().push(e);
    }
    for entries in map.values_mut() {
        entries.sort_by_key(|e| e.display_name());
    }
    map.into_iter().collect()
}
