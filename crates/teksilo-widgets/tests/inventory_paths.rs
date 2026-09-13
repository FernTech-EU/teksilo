// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rot guard for the two file-keyed touch-migration inventories.
//!
//! `docs/density-inventory.md` and `docs/widget-pointer-inventory.md` are the
//! artifacts the density sweep (P20) and the control sweeps (P22–P31) size
//! themselves from. Both are keyed by repo-relative source path, so a rename or a
//! module split silently invalidates them. This test parses every path out of the
//! two documents and asserts it still resolves to a file on disk.
//!
//! It deliberately checks *paths only*, not line numbers: line numbers drift with
//! every edit and pinning them would make the inventories a maintenance tax rather
//! than a reference. A wrong line is a review problem; a missing file is a rot
//! problem, and that is what this catches.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Workspace root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <root>/crates/teksilo-widgets
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("teksilo-widgets lives two levels below the workspace root")
        .to_path_buf()
}

/// Pull every backtick-quoted token that looks like a repo-relative Rust source
/// path out of one markdown document.
///
/// The inventories quote paths as `` `crates/teksilo-widgets/src/button.rs` ``.
/// Anything inside backticks that starts with `crates/` and ends with `.rs` is a
/// claim about the tree and gets checked; everything else (identifiers, method
/// names, token names) is ignored.
///
/// Globs (`recipe_*_style.rs`) and brace expansions
/// (`teksilo-theme-{fluent,macos}/…`) are prose shorthand for a *set* of files, not
/// a claim about one, so they are skipped rather than resolved.
fn quoted_source_paths(markdown: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for chunk in markdown.split('`').skip(1).step_by(2) {
        let token = chunk.trim();
        let is_glob = token.contains(['*', '{', '?']);
        if token.starts_with("crates/")
            && token.ends_with(".rs")
            && !token.contains(' ')
            && !is_glob
        {
            found.insert(token.to_string());
        }
    }
    found
}

fn read_doc(name: &str) -> (PathBuf, String) {
    let path = repo_root().join("docs").join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("inventory {} is missing or unreadable: {e}", path.display()));
    (path, text)
}

/// Every path quoted in an inventory must resolve to a file that exists.
fn assert_paths_resolve(doc: &str, allow_missing: &[&str]) {
    let root = repo_root();
    let (doc_path, text) = read_doc(doc);
    let paths = quoted_source_paths(&text);

    assert!(
        paths.len() >= 20,
        "{} yielded only {} source paths — the parser or the document shape changed",
        doc_path.display(),
        paths.len()
    );

    let mut missing = Vec::new();
    for p in &paths {
        if allow_missing.contains(&p.as_str()) {
            // Explicitly documented as not-yet-created; see the note in the doc.
            assert!(
                !root.join(p).exists(),
                "{p} is listed as not-yet-created in {} but now exists — drop it from \
                 the allow-list in this test and from the doc's prose",
                doc_path.display()
            );
            continue;
        }
        if !root.join(p).is_file() {
            missing.push(p.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "{} references {} path(s) that no longer exist:\n  {}",
        doc_path.display(),
        missing.len(),
        missing.join("\n  ")
    );
}

#[test]
fn density_inventory_paths_resolve() {
    assert_paths_resolve("density-inventory.md", &[]);
}

#[test]
fn widget_pointer_inventory_paths_resolve() {
    // P23 creates this by hoisting the five duplicated EDGE / MAX_VELOCITY pairs;
    // the document says so in its closing paragraph.
    assert_paths_resolve(
        "widget-pointer-inventory.md",
        &["crates/teksilo-widgets/src/common/drag_autoscroll.rs"],
    );
}

/// Both inventories must stay reachable from the book's table of contents —
/// mdBook renders only what `SUMMARY.md` links.
#[test]
fn inventories_are_linked_from_summary() {
    let (_, summary) = read_doc("SUMMARY.md");
    for doc in [
        "density-inventory.md",
        "widget-pointer-inventory.md",
        "hover-affordance-census.md",
        "drag-operation-census.md",
    ] {
        assert!(
            summary.contains(doc),
            "docs/SUMMARY.md does not link {doc}; mdBook will not render it"
        );
        assert!(
            repo_root().join("docs").join(doc).is_file(),
            "docs/{doc} is linked from SUMMARY.md but does not exist"
        );
    }
}
