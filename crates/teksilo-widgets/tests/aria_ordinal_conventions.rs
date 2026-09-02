// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! No widget may write an ARIA ordinal past the typed builder.
//!
//! AccessKit stores `position_in_set`, `row_index`, `column_index` and `level`
//! **zero-based**, while ARIA — and every doc comment, field name and call site
//! in this repository — counts them from 1. `AccessNodeBuilder` converts at that
//! boundary, and the conversion is the only thing standing between "tab 3 of 5"
//! and "tab 4 of 5" on Windows and Linux, both of which add the 1 back
//! (`accesskit_windows-0.35.0/src/node.rs:682-687`,
//! `accesskit_atspi_common-0.20.0/src/node.rs:394`).
//!
//! `builder.inner_mut()` reaches the raw `accesskit::Node` and skips it. That is
//! a legitimate escape hatch for the many properties the builder does not wrap,
//! so it stays available — but for these four it silently reintroduces the
//! off-by-one, and it did: fourteen call sites shipped it, across every list,
//! tree, table, tab bar and stepper the framework has.
//!
//! A unit test cannot catch that, because each widget looks locally correct.
//! This one reads the source instead.

use std::path::{Path, PathBuf};

/// The raw setters that must never be reached through `inner_mut()`.
const FORBIDDEN: &[&str] = &[
    "set_position_in_set",
    "set_row_index",
    "set_column_index",
    "set_level",
];

/// This file, which names the forbidden patterns in its own prose.
const SELF_NAME: &str = "aria_ordinal_conventions.rs";

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && path.file_name().is_some_and(|n| n != SELF_NAME)
        {
            out.push(path);
        }
    }
}

/// Every bypass in `text`, by the setter it calls.
///
/// Whitespace is stripped before the search so that the method-chain form
///
/// ```ignore
/// builder
///     .inner_mut()
///     .set_position_in_set(pos);
/// ```
///
/// is caught as readily as the single-line one — rustfmt produces both
/// depending on line length, so a line-based scan would pass or fail on
/// formatting.
fn bypasses(text: &str) -> Vec<&'static str> {
    let dense: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    FORBIDDEN
        .iter()
        .copied()
        .filter(|setter| dense.contains(&format!("inner_mut().{setter}(")))
        .collect()
}

#[test]
fn no_widget_writes_an_aria_ordinal_through_inner_mut() {
    let mut sources = Vec::new();
    rust_sources(Path::new("src"), &mut sources);
    assert!(
        !sources.is_empty(),
        "found no sources under src/ — this test runs with the package root as \
         its working directory, so a change to that would silently make it pass \
         over nothing"
    );

    let mut offenders = Vec::new();
    for path in &sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for setter in bypasses(&text) {
            offenders.push(format!("{}: inner_mut().{setter}", path.display()));
        }
    }

    assert!(
        offenders.is_empty(),
        "these write an ARIA ordinal straight to the AccessKit node, skipping \
         the 1-based-to-zero-based conversion in `AccessNodeBuilder`. Call the \
         typed setter instead, which converts. Writing the raw value makes the \
         announcement one too high on Windows and Linux and has no visible \
         effect on macOS, which reads none of these:\n  {}",
        offenders.join("\n  ")
    );
}

/// The scanner has to be able to fail, or it is a green light over nothing.
#[test]
fn the_scanner_recognises_a_bypass_in_both_formattings() {
    let one_line = "builder.inner_mut().set_position_in_set(self.index + 1);";
    assert_eq!(bypasses(one_line), vec!["set_position_in_set"]);

    let chained = "builder\n    .inner_mut()\n    .set_row_index(self.row);";
    assert_eq!(bypasses(chained), vec!["set_row_index"]);

    let allowed = "builder.set_position_in_set(self.index + 1);";
    assert!(
        bypasses(allowed).is_empty(),
        "a call through the typed setter must not be reported"
    );

    let unrelated = "n.set_sort_direction(dir);";
    assert!(bypasses(unrelated).is_empty());
}
