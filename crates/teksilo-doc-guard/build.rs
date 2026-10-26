// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Gather the fences `docs/` marks `<!-- compile-check -->` into one Markdown
//! file, which `src/lib.rs` hands to rustdoc as `no_run` doctests.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The line that opts the fence directly below it into compilation.
const MARKER: &str = "<!-- compile-check -->";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Read at run time, not with `env!`: a compiler cache (sccache) can serve
    // this script compiled in another checkout, whose path `env!` would have
    // baked in, and that checkout may be gone.
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is set by cargo for a build script");
    let docs = Path::new(&manifest_dir).join("../../docs");
    let mut files = Vec::new();
    collect_markdown(&docs, &mut files);
    files.sort();

    let mut out = String::new();
    let mut fences = 0;
    for file in &files {
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let shown = file.strip_prefix(&docs).unwrap_or(file).display();
        for (line, code) in marked_fences(&text, &shown.to_string()) {
            fences += 1;
            let _ = writeln!(
                out,
                "`docs/{shown}`, line {line}:\n\n```rust,no_run\n{code}```\n"
            );
        }
    }

    // No fence at all means the guides were not found, not that none is
    // marked: the gate would pass having compiled nothing.
    assert!(
        fences > 0,
        "no `{MARKER}` fence found under {}: the guides are missing, or none is marked",
        docs.display()
    );

    let dest = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"))
        .join("compile_checked.md");
    std::fs::write(&dest, out).unwrap_or_else(|e| panic!("cannot write {}: {e}", dest.display()));
}

/// Every `.md` under `dir`, each declared to cargo. Cargo stats a declared
/// directory but does not recurse into it, so the directories are declared for
/// a file being added or removed, and every file for an edit inside it.
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_markdown(&path, out),
            Ok(_) if path.extension().is_some_and(|e| e == "md") => {
                println!("cargo:rerun-if-changed={}", path.display());
                out.push(path);
            }
            _ => {}
        }
    }
}

/// `(line of the fence, its code)` for each marked fence in `text`.
///
/// A marker that is not directly above a `rust` fence fails the build: a
/// marker that silently checked nothing is how a broken program would go
/// unnoticed again.
fn marked_fences(text: &str, file: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() != MARKER {
            i += 1;
            continue;
        }
        let fence = i + 1;
        if lines.get(fence).map(|l| l.trim()) != Some("```rust") {
            panic!(
                "docs/{file}:{}: `{MARKER}` must sit directly above a ```rust fence",
                i + 1
            );
        }
        let mut code = String::new();
        let mut end = fence + 1;
        while end < lines.len() && lines[end].trim() != "```" {
            code.push_str(lines[end]);
            code.push('\n');
            end += 1;
        }
        if end == lines.len() {
            panic!(
                "docs/{file}:{}: the marked fence is never closed",
                fence + 1
            );
        }
        found.push((fence + 1, code));
        i = end + 1;
    }
    found
}
