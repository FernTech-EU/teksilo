// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The clean-room vocabulary guard.
//!
//! The KDE crates are written from Teksilo's specification
//! (`engineering/docs/kde-breeze-spec.md`), never from KDE's or Qt's code.
//! An identifier that only exists upstream (a KDE function or class name, a
//! Qt type) appearing in the source is a sign that the code was shaped
//! after upstream rather than after the specification, so this test fails
//! on any of them, naming the file and line.
//!
//! The words are matched whole and case-sensitively. A comment line that
//! cites `doc.qt.io`, Qt's public documentation, may name what it cites.
//! Test fixtures (verbatim upstream files) are not scanned, and neither is
//! the word list below.
//!
//! To guard another crate, add its directories to [`SCANNED`]. The paths are
//! relative to the repository root, and a missing directory fails the test
//! rather than passing it unscanned, as does a directory with no file in it
//! or a scan that misses `src/math.rs` or this file. Every file found is
//! read, a non-UTF-8 one through a lossy decoding. This file reads the
//! repository tree, so it is left out of the published package.

use std::fs;
use std::path::{Path, PathBuf};

/// The directories scanned, recursively, relative to the repository root.
const SCANNED: &[&str] = &[
    "crates/teksilo-kde-scheme/src",
    "crates/teksilo-kde-scheme/tests",
    "crates/teksilo-theme-kde/src",
    "crates/teksilo-theme-kde/tests",
];

/// Directories inside [`SCANNED`] that hold verbatim upstream files.
const SKIPPED: &[&str] = &["crates/teksilo-kde-scheme/tests/fixtures"];

const LIST_BEGIN: &str = "// clean-room word list: begin";
const LIST_END: &str = "// clean-room word list: end";

// clean-room word list: begin
const FORBIDDEN: &[&str] = &[
    "KHCY",
    "lumag",
    "igamma",
    "mixQreal",
    "mix_qreal",
    "tintHelper",
    "tint_helper",
    "contrastRatioForLuma",
    "SerializedColors",
    "StateEffects",
    "applyScheme",
    "accentForeground",
    "accentBackground",
    "QColor",
    "KColorScheme",
    "qreal",
];
// clean-room word list: end

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The forbidden words that appear whole in `line`.
fn whole_words_in(line: &str) -> Vec<&'static str> {
    let bytes = line.as_bytes();
    FORBIDDEN
        .iter()
        .copied()
        .filter(|word| {
            line.match_indices(word).any(|(start, _)| {
                let end = start + word.len();
                let before = start == 0 || !is_word_byte(bytes[start - 1]);
                let after = end == bytes.len() || !is_word_byte(bytes[end]);
                before && after
            })
        })
        .collect()
}

fn cites_qt_documentation(line: &str) -> bool {
    let trimmed = line.trim_start();
    (trimmed.starts_with("//") || trimmed.starts_with('#')) && line.contains("doc.qt.io")
}

fn collect_files(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if SKIPPED.iter().any(|skip| relative == Path::new(skip)) {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, root, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn no_upstream_identifier_appears_in_the_kde_crates() {
    let root = repository_root();
    let this_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/clean_room_vocabulary.rs");
    let mut files = Vec::new();
    for dir in SCANNED {
        let path = root.join(dir);
        assert!(
            path.is_dir(),
            "{dir} is listed in SCANNED but does not exist"
        );
        let before = files.len();
        collect_files(&path, &root, &mut files);
        assert!(
            files.len() > before,
            "{dir} is listed in SCANNED but holds no file to scan"
        );
    }
    let canonical: Vec<PathBuf> = files
        .iter()
        .map(|file| {
            fs::canonicalize(file)
                .unwrap_or_else(|e| panic!("cannot resolve {}: {e}", file.display()))
        })
        .collect();
    let this_file = fs::canonicalize(&this_file).expect("this test file resolves");
    let math = fs::canonicalize(root.join("crates/teksilo-kde-scheme/src/math.rs"))
        .expect("the colour arithmetic module resolves");
    assert!(
        canonical.contains(&math),
        "the scan did not reach crates/teksilo-kde-scheme/src/math.rs"
    );
    assert!(
        canonical.contains(&this_file),
        "the scan did not reach this test file"
    );
    let mut findings = Vec::new();
    let mut lines_read = 0usize;
    for (file, resolved) in files.iter().zip(&canonical) {
        // Every file is read: one that is not UTF-8 is scanned through a
        // lossy decoding, so an identifier in it is still found.
        let bytes =
            fs::read(file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let text = String::from_utf8_lossy(&bytes);
        let is_this_file = *resolved == this_file;
        let mut in_list = false;
        for (index, line) in text.lines().enumerate() {
            lines_read += 1;
            if is_this_file && line.trim() == LIST_BEGIN {
                in_list = true;
            }
            if is_this_file && line.trim() == LIST_END {
                in_list = false;
                continue;
            }
            if in_list || cites_qt_documentation(line) {
                continue;
            }
            for word in whole_words_in(line) {
                let shown = file.strip_prefix(&root).unwrap_or(file);
                findings.push(format!("{}:{}: {word}", shown.display(), index + 1));
            }
        }
    }
    assert!(
        lines_read > files.len(),
        "the scan read {lines_read} lines from {} files",
        files.len()
    );
    assert!(
        findings.is_empty(),
        "upstream identifiers in clean-room code:\n{}",
        findings.join("\n")
    );
}

#[test]
fn words_match_whole_and_case_sensitively() {
    let word = FORBIDDEN[0];
    assert_eq!(whole_words_in(&format!("let x = {word};")), vec![word]);
    assert!(whole_words_in(&format!("{word}x")).is_empty());
    assert!(whole_words_in(&format!("x{word}")).is_empty());
    assert!(whole_words_in(&format!("_{word}")).is_empty());
    assert_ne!(word, word.to_lowercase());
    assert!(whole_words_in(&word.to_lowercase()).is_empty());
    assert!(cites_qt_documentation(&format!(
        "    // {word}: see https://doc.qt.io/qt-6/"
    )));
    assert!(!cites_qt_documentation(&format!(
        "let {word} = 1; // https://doc.qt.io/"
    )));
}
