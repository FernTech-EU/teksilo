// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Captured source location for a registered widget catalog entry.
//!
//! Populated by the `register_widget_catalog!` macro at expansion time
//! via `file!()` / `line!()`. The previewer's `--file=PATH` resolution
//! matches against the captured `file` by suffix to handle platform
//! path canonicalisation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceLoc {
    pub file: &'static str,
    pub line: u32,
}

impl SourceLoc {
    pub const fn new(file: &'static str, line: u32) -> Self {
        Self { file, line }
    }

    /// Match this source location against a path supplied on the command
    /// line. The match is a suffix match — the user typically supplies a
    /// workspace-relative path while `file!()` returns a path relative to
    /// the crate the macro expanded in. Matching by suffix accommodates
    /// both without requiring path canonicalisation.
    pub fn matches_path(&self, target: &str) -> bool {
        // Normalise separators so the comparison is consistent across OSes.
        fn norm(s: &str) -> String {
            s.replace('\\', "/")
        }
        let a = norm(self.file);
        let b = norm(target);
        a == b || a.ends_with(&b) || b.ends_with(&a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_path_handles_suffix_matches() {
        let loc = SourceLoc::new("crates/bastyde-widgets/src/button.rs", 42);
        assert!(loc.matches_path("crates/bastyde-widgets/src/button.rs"));
        assert!(loc.matches_path("button.rs"));
        assert!(loc.matches_path("src/button.rs"));
        assert!(!loc.matches_path("crates/bastyde-widgets/src/checkbox.rs"));
    }

    #[test]
    fn matches_path_normalises_separators() {
        let loc = SourceLoc::new("crates\\bastyde-widgets\\src\\button.rs", 1);
        assert!(loc.matches_path("button.rs"));
        assert!(loc.matches_path("src/button.rs"));
    }
}
