// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Test-only reader for the constant tables published in
//! `docs/kinetic-scrolling.md`.
//!
//! Every number in that document is quoted from a constant in this module tree,
//! and a documented measurement nothing reads back is a test with no assertion:
//! the page and the code drift apart the first time a value is retuned, and
//! nothing goes red. So the tables are parsed and compared, row by row, by
//! [`velocity`](super::velocity) and [`simulation`](super::simulation) — the two
//! modules that own the constants, several of which are private and therefore
//! only assertable from inside.
//!
//! The comparison fails in **both** directions on purpose: a row the page adds
//! without a reader is as much a drift as a value that moved, so an unknown row
//! key panics rather than being skipped.
//!
//! `teksilo-tokens` carries its own copy of this reader for
//! `docs/density-and-targets.md` (in `input.rs`'s test module). The duplication
//! is deliberate: sharing it would mean one of the two crates depending on the
//! other for a test helper, and a pipe-delimited table needs twenty lines of
//! `split`.

/// The rows of the first markdown table following `heading` in
/// `docs/<page>`, each row as its trimmed cells with emphasis stripped.
///
/// Panics if the page, the heading, or a table under it is missing — a silently
/// empty result would make every caller pass vacuously, which is the failure
/// mode this whole module exists to prevent.
pub(super) fn rows(page: &str, heading: &str) -> Vec<Vec<String>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(page);
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let after = doc
        .split_once(heading)
        .unwrap_or_else(|| panic!("{} carries no heading {heading:?}", path.display()))
        .1;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut in_table = false;
    for line in after.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            if in_table {
                break;
            }
            continue;
        }
        in_table = true;
        if line.contains("---") {
            // The separator; whatever preceded it was the header row.
            rows.clear();
            continue;
        }
        rows.push(
            line.trim_matches('|')
                .split('|')
                .map(|c| c.trim().trim_matches('*').trim().to_string())
                .collect(),
        );
    }
    assert!(
        !rows.is_empty(),
        "{} has no table under {heading:?}",
        path.display()
    );
    rows
}

/// Assert that `code`, printed to the precision the document chose, is what the
/// document says.
///
/// A rounding comparison rather than a tolerance, so the page can write
/// `≈ 2.3582018` for `ln(0.78)/ln(0.9)` and `9.80665 m/s²` for a gravity
/// constant without either side claiming more digits than it has — while still
/// failing on any changed digit. Units, backticks, thousands spaces and a
/// leading `≈` are stripped; a cell that carries a formula (`ln(0.78)/ln(0.9)`)
/// keeps only its final number.
pub(super) fn assert_value(page: &str, cell: &str, code: f64, what: &str) {
    let text = cell
        .trim_matches('`')
        .rsplit('≈')
        .next()
        .expect("a cell")
        .trim()
        .replace(['\u{202f}', ' '], "")
        .replace("dp/s", "")
        .replace("dp", "")
        .replace("ms", "")
        .replace("µs", "")
        .replace("samples", "")
        .replace("m/s²", "")
        .replace("Hz", "")
        .replace('`', "");
    let decimals = text.split_once('.').map_or(0, |(_, frac)| frac.len());
    assert_eq!(
        format!("{code:.decimals$}"),
        text,
        "docs/{page} says {what} is {cell:?}; the code says {code}"
    );
}
