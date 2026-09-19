// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Make `include_dir!` react to the files it embeds.
//!
//! `cargo-teksilo` embeds three payloads with `include_dir!` / `include_str!`:
//! the probe harness, the merged skill, and the API extractor. Cargo tracks
//! `include_str!` on a single file, but an `include_dir!` over a **tree** is
//! opaque to it — nothing in the crate's source mentions the individual files,
//! so adding, editing or deleting one leaves cargo believing the crate is
//! fresh.
//!
//! The symptom is quiet and expensive: `cargo build -p cargo-teksilo` reports
//! "0 crates compiled" after a real change, the binary keeps the previous
//! payload, and `cargo teksilo probe` materialises a harness that no longer
//! matches what is on disk. It then only corrects itself when something
//! unrelated happens to dirty the crate. This was observed, not theorised —
//! a run after adding the curated examples compiled nothing.
//!
//! So walk the embedded trees and declare every file. The alternative,
//! `rerun-if-changed` on the directory alone, is not equivalent: cargo stats a
//! declared directory but does not recurse, so an edit two levels down is
//! still missed.

use std::path::Path;

fn main() {
    // Without at least one `rerun-if-changed`, cargo reruns this script on any
    // source change; with one, it reruns only for what is declared. Declaring
    // the build script itself keeps that honest.
    println!("cargo:rerun-if-changed=build.rs");

    // Everything embedded lives under one root today. If a second appears,
    // declare it here too — a payload cargo does not know about is a payload
    // that goes stale silently.
    declare_tree(Path::new("embedded"));
}

fn declare_tree(dir: &Path) {
    // The directory itself, so a file being *added* or *removed* is noticed:
    // its own mtime changes even though no declared file did.
    println!("cargo:rerun-if-changed={}", dir.display());

    let Ok(entries) = std::fs::read_dir(dir) else {
        // A missing embedded tree is a build error the compiler will report
        // far more clearly than this script could.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => declare_tree(&path),
            Ok(_) => println!("cargo:rerun-if-changed={}", path.display()),
            Err(_) => {}
        }
    }
}
