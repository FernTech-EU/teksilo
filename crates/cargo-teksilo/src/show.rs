// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `cargo teksilo show` — read a corpus document in full, offline.
//!
//! [`search`](crate::search) prints a snippet and a path: `docs/scroll-area.md`,
//! `examples/simple_button/src/main.rs`. Until this command existed, a caller
//! that wanted more than the snippet had three moves and all three were bad:
//!
//! 1. **Open the path.** It does not exist — the app depends on `teksilo`, and
//!    `docs/` ships in no crate while every example crate is `publish = false`.
//!    That absence is the whole reason the corpus exists.
//! 2. **Fetch it from GitHub.** `blob/main/` and the published book both track
//!    `main`, which drifts from the version the app pinned. It also needs
//!    network. This is the version-binding hole reappearing in the one tool
//!    built to close it.
//! 3. **Reason from the truncated snippet**, i.e. guess.
//!
//! The text is already here. Every chunk carries its own `text` plus the
//! 0-based inclusive line range it occupied in the original file, and the
//! generator's chunking covers every non-blank line of all 158 indexed
//! documents. So a document is *reassembled*, not re-fetched and not
//! re-embedded: lay each chunk's lines back at their recorded offsets and the
//! gaps that remain are exactly the blank separator lines the chunker dropped.
//!
//! That reconstruction is **byte-exact** for all 158 documents against a real
//! checkout, which is not an assumption — the test
//! `every_document_reconstructs_byte_exactly` reads the real files and compares
//! them, whenever the tests run inside a checkout.
//!
//! # stdout is the document, stderr is the provenance
//!
//! The version line goes to **stderr**, unlike `search`'s, which prints it on
//! stdout. `search`'s stdout is a report; this command's stdout *is* a file, and
//! a caller must be able to redirect it, diff it, or pipe it into a reader
//! without a header line corrupting the content. An agent reading combined
//! output still sees the provenance, first, which is where it wants it.
//!
//! One divergence worth naming: a reconstructed document always ends in a
//! newline, because each line is printed with [`println!`]. All 158 corpus
//! files do end in one today, so the equality holds; a future source file
//! without a trailing newline would come back with one added.

use std::collections::BTreeMap;
use std::path::Path;

use teksilo_corpus::Index;

use crate::guard::{self, Verdict};
use crate::resolve;

/// How many near-misses to offer for an unknown path.
const MAX_SUGGESTIONS: usize = 6;

#[derive(Debug, thiserror::Error)]
pub enum ShowError {
    #[error(transparent)]
    Resolve(#[from] resolve::ResolveError),

    #[error("{0}")]
    Refused(String),

    #[error("{0}")]
    Usage(String),

    /// The path names no document — carrying the help text, not just the fact.
    ///
    /// A bare "not found" is the failure mode this command exists to avoid: a
    /// model that reads one falls back on its own memory of the guide. So the
    /// message names what to try instead.
    #[error("{0}")]
    NotFound(String),

    #[error("the embedded corpus could not be read: {0}")]
    Corpus(#[from] teksilo_corpus::CorpusError),
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// What `show` was asked for, after clap and before any corpus work.
///
/// `lines` stays a raw string so its parse is [`parse_line_range`] — a pure
/// function whose 1-based/0-based boundary is unit-testable without a corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowRequest {
    pub path: Option<String>,
    pub lines: Option<String>,
    pub list: bool,
}

/// An inclusive, **1-based** span of lines, as an editor and `search`'s own
/// `(lines A-B)` footnote both count them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub first: usize,
    pub last: usize,
}

/// Parse `--lines`: `A-B` for a span, `A` for a single line.
///
/// 1-based and inclusive, deliberately — that is what `search` prints under a
/// hit and what every editor's gutter shows, and the corpus stores 0-based
/// offsets, so the conversion has to happen exactly once and in one place. It
/// happens in [`slice()`].
pub fn parse_line_range(spec: &str) -> Result<LineRange, ShowError> {
    let spec = spec.trim();
    let (first_text, last_text) = match spec.split_once('-') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (spec, spec),
    };
    let parse_one = |text: &str| -> Result<usize, ShowError> {
        match text.parse::<usize>() {
            Ok(0) => Err(ShowError::Usage(format!(
                "--lines counts from 1, not 0 (got `{spec}`). \
                 `--lines 1-1` is the first line."
            ))),
            Ok(n) => Ok(n),
            Err(_) => Err(ShowError::Usage(format!(
                "--lines wants `A-B` or `A`, got `{spec}`"
            ))),
        }
    };
    let first = parse_one(first_text)?;
    let last = parse_one(last_text)?;
    if first > last {
        return Err(ShowError::Usage(format!(
            "--lines {first}-{last} runs backwards"
        )));
    }
    Ok(LineRange { first, last })
}

// ---------------------------------------------------------------------------
// Reassembly
// ---------------------------------------------------------------------------

/// Reassemble the document at `path` from its chunks, or `None` if the corpus
/// holds no chunk for that path.
///
/// The chunks of one file tile it: each carries its text and the 0-based
/// inclusive range it came from. Laying them back at those offsets leaves holes
/// exactly where the chunker dropped a blank separator line, so the holes come
/// back as empty lines and the result is the original.
///
/// Later chunks win on the (non-existent, and tested-for) overlap, which makes
/// the function total rather than fallible on data that cannot occur.
pub fn document_lines(index: &Index, path: &str) -> Option<Vec<String>> {
    let mut chunks: Vec<_> = index.chunks.iter().filter(|c| c.path == path).collect();
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|c| (c.line_start, c.line_end));

    let last_line = chunks.iter().map(|c| c.line_end).max()?;
    let mut lines: Vec<String> = vec![String::new(); last_line + 1];
    for chunk in chunks {
        for (offset, text) in chunk.text().split('\n').enumerate() {
            let at = chunk.line_start + offset;
            // A chunk whose text is longer than its recorded range would write
            // past the end; clamp rather than panic, since the array is sized
            // from the ranges and the text is what a reader actually wants.
            if at >= lines.len() {
                lines.resize(at + 1, String::new());
            }
            lines[at] = text.to_string();
        }
    }
    Some(lines)
}

/// The 1-based inclusive `range` of `lines`, clamped to what exists.
///
/// `Err` only when the range starts past the end — a caller that asks for
/// 500-510 of a 402-line file has the wrong file or the wrong number, and
/// silently printing nothing would read as "this document is empty". A range
/// that merely *ends* past the end is clamped, because asking for the last
/// twenty lines of a file whose length you do not know is reasonable.
pub fn slice(lines: &[String], range: LineRange) -> Result<&[String], ShowError> {
    if range.first > lines.len() {
        return Err(ShowError::Usage(format!(
            "--lines {}-{} starts past the end: this document has {} lines",
            range.first,
            range.last,
            lines.len()
        )));
    }
    let start = range.first - 1; // the one 1-based → 0-based conversion
    let end = range.last.min(lines.len());
    Ok(&lines[start..end])
}

/// Every document in the corpus, `(kind, path)`, guides before examples and
/// each group sorted.
pub fn documents(index: &Index) -> Vec<(&str, &str)> {
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for chunk in &index.chunks {
        seen.entry(chunk.path.as_str()).or_insert(&chunk.kind);
    }
    let mut out: Vec<(&str, &str)> = seen.into_iter().map(|(p, k)| (k, p)).collect();
    // Guides first: they are the material a reader orienting themselves wants,
    // and `examples/` is the longer half.
    out.sort_by_key(|(kind, path)| (*kind != "guide", *path));
    out
}

// ---------------------------------------------------------------------------
// Resolving what the caller typed
// ---------------------------------------------------------------------------

/// What a caller's path string turned out to name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// An exact corpus path.
    Exact(String),
    /// One unambiguous candidate for an abbreviated or differently-cased
    /// spelling — `scroll-area.md`, `simple_button/src/main.rs`, `Docs/...`.
    Rewritten(String),
    /// Nothing, with the help text to print.
    Unknown(String),
}

/// Trim the spellings that are the same path: a leading `./`, backslashes from
/// a Windows shell, a leading or trailing `/`.
fn normalize(raw: &str) -> String {
    let mut s = raw.trim().replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    s.trim_matches('/').to_string()
}

/// Turn what the caller typed into a corpus path, or into help.
///
/// Resolves the three abbreviations that are unambiguous — a wrong case, a bare
/// basename, a trailing path fragment — and *suggests* for everything else. The
/// line between the two is ambiguity, not similarity: rewriting `scroll-area.md`
/// to `docs/scroll-area.md` cannot surprise anyone, while rewriting a typo to
/// its nearest neighbour could serve a different document than the one asked
/// for, which is the failure this whole tool is built against.
pub fn resolve_path(index: &Index, raw: &str) -> Resolved {
    let wanted = normalize(raw);
    let paths: Vec<&str> = documents(index).into_iter().map(|(_, p)| p).collect();

    if paths.contains(&wanted.as_str()) {
        return Resolved::Exact(wanted);
    }

    let lower = wanted.to_lowercase();
    let unambiguous: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|candidate| {
            let c = candidate.to_lowercase();
            c == lower || basename(&c) == lower || c.ends_with(&format!("/{lower}"))
        })
        .collect();
    match unambiguous.as_slice() {
        [one] => return Resolved::Rewritten((*one).to_string()),
        [] => {}
        many => {
            let mut out = format!(
                "`{raw}` matches {} documents in the teksilo {} corpus. Name one:\n\n",
                many.len(),
                index.teksilo_version,
            );
            for p in many.iter().take(MAX_SUGGESTIONS) {
                out.push_str(&format!("  {p}\n"));
            }
            if many.len() > MAX_SUGGESTIONS {
                out.push_str(&format!(
                    "  … and {} more (`cargo teksilo show --list`)\n",
                    many.len() - MAX_SUGGESTIONS
                ));
            }
            return Resolved::Unknown(out);
        }
    }

    Resolved::Unknown(not_found_text(index, raw, &wanted, &paths))
}

/// The help printed for a path that names nothing.
///
/// Three tiers, cheapest first: what lives *under* the path if it reads like a
/// directory, then near spellings, then the two commands that always work.
fn not_found_text(index: &Index, raw: &str, wanted: &str, paths: &[&str]) -> String {
    let mut out = format!(
        "no document at `{raw}` in the teksilo {} corpus.\n",
        index.teksilo_version
    );

    let prefix = format!("{wanted}/");
    let under: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|p| p.starts_with(&prefix))
        .collect();
    if !under.is_empty() {
        out.push_str(&format!(
            "\n`{wanted}` is a directory. It holds {} documents:\n",
            under.len()
        ));
        for p in under.iter().take(MAX_SUGGESTIONS) {
            out.push_str(&format!("  {p}\n"));
        }
        if under.len() > MAX_SUGGESTIONS {
            out.push_str(&format!(
                "  … and {} more (`cargo teksilo show --list`)\n",
                under.len() - MAX_SUGGESTIONS
            ));
        }
        return out;
    }

    let near = suggestions(paths, wanted);
    if near.is_empty() {
        out.push_str("\nNo path looks close to it.\n");
    } else {
        out.push_str("\nDid you mean:\n");
        for p in &near {
            out.push_str(&format!("  {p}\n"));
        }
    }
    out.push_str(
        "\n`cargo teksilo search \"<question>\"` finds paths by content; \
         `cargo teksilo show --list` prints every one.\n",
    );
    out
}

/// Paths close to `wanted`, best first.
///
/// Scored on whichever is closer, the whole path or the basename — a caller who
/// guessed the wrong directory and one who mistyped the filename have made
/// different mistakes, and only one of the two distances sees each. The
/// tolerance scales with length so a short name cannot match everything.
pub fn suggestions(paths: &[&str], wanted: &str) -> Vec<String> {
    let wanted = wanted.to_lowercase();
    let wanted_base = basename(&wanted);
    let budget = |len: usize| (len / 3).clamp(2, 6);

    let mut scored: Vec<(usize, &str)> = paths
        .iter()
        .filter_map(|candidate| {
            let lower = candidate.to_lowercase();
            let base = basename(&lower);
            let whole = levenshtein(&wanted, &lower);
            let by_base = levenshtein(wanted_base, base);
            let best = whole.min(by_base);
            let tolerance = budget(wanted_base.len().max(2));
            // A shared substring is its own kind of evidence: `docs/scroll` is
            // far from `docs/scroll-area.md` by edit distance and obviously
            // means it.
            let contains = base.contains(wanted_base) || wanted_base.contains(base);
            (best <= tolerance || contains)
                .then_some((if contains { best.min(1) } else { best }, *candidate))
        })
        .collect();

    scored.sort_by_key(|(score, path)| (*score, *path));
    scored
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, p)| p.to_string())
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Classic full-matrix Levenshtein, two rows.
///
/// Over 158 short strings, once, on a path that was already wrong.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/// Run `show` from the app directory `dir`.
pub fn run(dir: &Path, request: &ShowRequest) -> Result<i32, ShowError> {
    let resolution = resolve::resolve(dir)?;

    let verdict = guard::check(&resolution.version);
    if !verdict.may_answer() {
        let Verdict::Refuse { app, tool } = &verdict else {
            unreachable!()
        };
        return Err(ShowError::Refused(guard::refusal_text(
            app,
            tool,
            "corpus documents",
            guard::readable_sources(&resolution).as_deref(),
        )));
    }
    if let Some(note) = verdict.note() {
        eprintln!("{note}");
    }

    let index = teksilo_corpus::index()?;

    if request.list {
        return print_list(index);
    }

    let Some(raw) = request.path.as_deref() else {
        return Err(ShowError::Usage(
            "cargo teksilo show <PATH>          the whole document\n\
             cargo teksilo show <PATH> --lines 166-172\n\
             cargo teksilo show --list          every path in the corpus"
                .to_string(),
        ));
    };

    let path = match resolve_path(index, raw) {
        Resolved::Exact(p) => p,
        Resolved::Rewritten(p) => {
            eprintln!("note: `{raw}` → {p}");
            p
        }
        Resolved::Unknown(help) => return Err(ShowError::NotFound(help)),
    };

    let lines = document_lines(index, &path).expect("resolve_path only returns corpus paths");
    let total = lines.len();

    let shown = match request.lines.as_deref() {
        None => {
            eprintln!(
                "teksilo {} corpus · {path} ({total} lines)",
                index.teksilo_version
            );
            &lines[..]
        }
        Some(spec) => {
            let range = parse_line_range(spec)?;
            let shown = slice(&lines, range)?;
            eprintln!(
                "teksilo {} corpus · {path} lines {}-{} of {total}",
                index.teksilo_version,
                range.first,
                range.first + shown.len().saturating_sub(1),
            );
            shown
        }
    };

    for line in shown {
        println!("{line}");
    }
    Ok(0)
}

/// `--list`: every path, one per line on stdout, counts on stderr.
///
/// Split that way on purpose. `search` ranks by relevance, so it is the wrong
/// instrument for "what is in here at all" — an agent orienting itself, or
/// recovering from a mistyped path, needs the roster, and a roster is only
/// useful if it greps. So stdout stays bare paths and the counts go where the
/// document's provenance goes.
fn print_list(index: &Index) -> Result<i32, ShowError> {
    let docs = documents(index);
    let guides = docs.iter().filter(|(kind, _)| *kind == "guide").count();
    eprintln!(
        "teksilo {} corpus · {} documents ({guides} guides, then {} example sources)",
        index.teksilo_version,
        docs.len(),
        docs.len() - guides,
    );
    for (_, path) in docs {
        println!("{path}");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use teksilo_corpus::Chunk;

    fn chunk(id: usize, kind: &str, path: &str, start: usize, text: &str) -> Chunk {
        let line_count = text.split('\n').count();
        Chunk {
            id,
            kind: kind.to_string(),
            path: path.to_string(),
            heading_path: vec![],
            text: text.to_string(),
            line_start: start,
            line_end: start + line_count - 1,
            crate_name: None,
            tokens: BTreeMap::new(),
            len: 0,
            embedding: None,
        }
    }

    fn toy(chunks: Vec<Chunk>) -> Index {
        Index {
            schema: 2,
            teksilo_version: "0.0.0".to_string(),
            encoder: None,
            encoder_dim: None,
            chunk_count: chunks.len(),
            avgdl: 1.0,
            terms: vec![],
            df: BTreeMap::new(),
            chunks,
        }
    }

    /// The teksilo checkout, when the tests are running inside one.
    ///
    /// `None` from a registry copy, where the sources this corpus was built
    /// from are simply not there to compare against.
    fn repo_root() -> Option<std::path::PathBuf> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?
            .to_path_buf();
        (root.join("docs").is_dir() && root.join("examples").is_dir()).then_some(root)
    }

    // -- the 1-based boundary ------------------------------------------------

    #[test]
    fn a_line_range_is_one_based_and_inclusive() {
        assert_eq!(
            parse_line_range("166-172").unwrap(),
            LineRange {
                first: 166,
                last: 172
            }
        );
        assert_eq!(
            parse_line_range("7").unwrap(),
            LineRange { first: 7, last: 7 }
        );
        assert_eq!(
            parse_line_range(" 1 - 1 ").unwrap(),
            LineRange { first: 1, last: 1 }
        );
    }

    #[test]
    fn line_zero_is_rejected_rather_than_read_as_the_first_line() {
        // The corpus stores 0-based offsets and `search` prints 1-based ones,
        // so accepting 0 here would quietly serve an off-by-one to whichever
        // caller was counting the other way.
        let err = parse_line_range("0-4").unwrap_err();
        assert!(matches!(err, ShowError::Usage(_)));
        assert!(err.to_string().contains("counts from 1"));
    }

    #[test]
    fn a_backwards_or_unparseable_range_is_rejected() {
        for bad in ["9-2", "a-b", "", "3-", "-3"] {
            assert!(
                matches!(parse_line_range(bad), Err(ShowError::Usage(_))),
                "{bad:?} should not parse"
            );
        }
    }

    #[test]
    fn lines_one_to_one_is_the_first_line() {
        let doc: Vec<String> = ["first", "second", "third"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            slice(&doc, parse_line_range("1-1").unwrap()).unwrap(),
            ["first".to_string()]
        );
        assert_eq!(
            slice(&doc, parse_line_range("2-3").unwrap()).unwrap(),
            ["second".to_string(), "third".to_string()]
        );
        // The last line, by its 1-based number.
        assert_eq!(
            slice(&doc, parse_line_range("3").unwrap()).unwrap(),
            ["third".to_string()]
        );
    }

    #[test]
    fn a_range_past_the_end_clamps_but_a_range_starting_past_it_is_an_error() {
        let doc: Vec<String> = (0..3).map(|i| i.to_string()).collect();
        assert_eq!(
            slice(
                &doc,
                LineRange {
                    first: 2,
                    last: 900
                }
            )
            .unwrap()
            .len(),
            2
        );
        let err = slice(&doc, LineRange { first: 9, last: 9 }).unwrap_err();
        assert!(err.to_string().contains("3 lines"), "{err}");
    }

    // -- reassembly ----------------------------------------------------------

    #[test]
    fn a_gap_between_chunks_comes_back_as_a_blank_line() {
        // This is the whole mechanism: the chunker drops the blank separator
        // between two sections, and the recorded offsets put it back.
        let index = toy(vec![
            chunk(0, "guide", "docs/a.md", 0, "# Title\nintro"),
            chunk(1, "guide", "docs/a.md", 3, "## Next\nbody"),
        ]);
        assert_eq!(
            document_lines(&index, "docs/a.md").unwrap(),
            ["# Title", "intro", "", "## Next", "body"]
        );
    }

    #[test]
    fn an_unknown_path_reassembles_to_nothing() {
        let index = toy(vec![chunk(0, "guide", "docs/a.md", 0, "x")]);
        assert!(document_lines(&index, "docs/b.md").is_none());
    }

    #[test]
    fn chunks_of_other_documents_do_not_bleed_in() {
        let index = toy(vec![
            chunk(0, "guide", "docs/a.md", 0, "a0"),
            chunk(1, "guide", "docs/b.md", 1, "b1"),
        ]);
        assert_eq!(document_lines(&index, "docs/a.md").unwrap(), ["a0"]);
        assert_eq!(document_lines(&index, "docs/b.md").unwrap(), ["", "b1"]);
    }

    // -- against the embedded corpus ----------------------------------------

    #[test]
    fn every_corpus_path_reassembles() {
        let index = teksilo_corpus::index().unwrap();
        let docs = documents(index);
        assert!(
            docs.len() > 100,
            "corpus shrank unexpectedly: {}",
            docs.len()
        );
        for (_, path) in &docs {
            let lines = document_lines(index, path)
                .unwrap_or_else(|| panic!("{path} is listed but does not reassemble"));
            assert!(!lines.is_empty(), "{path} reassembled empty");
        }
    }

    #[test]
    fn no_two_chunks_disagree_about_a_line() {
        // `document_lines` lets a later chunk win, which is only safe because
        // the generator never emits a conflicting overlap. If it ever does,
        // this is the test that says so rather than a reader noticing a
        // duplicated paragraph.
        let index = teksilo_corpus::index().unwrap();
        let mut seen: BTreeMap<(&str, usize), &str> = BTreeMap::new();
        for c in &index.chunks {
            for (offset, text) in c.text().split('\n').enumerate() {
                let key = (c.path.as_str(), c.line_start + offset);
                if let Some(prev) = seen.insert(key, text) {
                    assert_eq!(prev, text, "chunks disagree about {}:{}", key.0, key.1 + 1);
                }
            }
        }
    }

    #[test]
    fn a_chunks_text_matches_the_line_range_it_claims() {
        let index = teksilo_corpus::index().unwrap();
        for c in &index.chunks {
            assert_eq!(
                c.text().split('\n').count(),
                c.line_end - c.line_start + 1,
                "chunk {} of {} claims lines {}-{}",
                c.id,
                c.path,
                c.line_start,
                c.line_end
            );
        }
    }

    #[test]
    fn every_document_reconstructs_byte_exactly() {
        // The load-bearing claim: `show` is exposure of data already held, not
        // an approximation of it. Only checkable inside a checkout, since the
        // corpus deliberately does not ship the sources.
        let Some(root) = repo_root() else {
            return;
        };
        let index = teksilo_corpus::index().unwrap();
        for (_, path) in documents(index) {
            let original =
                std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
            let rebuilt = document_lines(index, path).unwrap().join("\n") + "\n";
            assert_eq!(rebuilt, original, "{path} did not reconstruct byte-exactly");
        }
    }

    #[test]
    fn search_line_numbers_address_the_same_lines_show_prints() {
        // `search` prints `line_start + 1`; `--lines` subtracts one. The two
        // conversions have to be inverses or every citation is off by one.
        let index = teksilo_corpus::index().unwrap();
        let chunk = index
            .chunks
            .iter()
            .find(|c| c.line_start > 5 && c.line_end > c.line_start)
            .expect("some chunk starts past line 5");
        let lines = document_lines(index, &chunk.path).unwrap();
        let printed = LineRange {
            first: chunk.line_start + 1,
            last: chunk.line_end + 1,
        };
        let shown = slice(&lines, printed).unwrap().join("\n");
        assert_eq!(shown, *chunk.text());
    }

    // -- resolving a path ----------------------------------------------------

    #[test]
    fn an_exact_path_resolves_to_itself() {
        let index = teksilo_corpus::index().unwrap();
        assert_eq!(
            resolve_path(index, "docs/scroll-area.md"),
            Resolved::Exact("docs/scroll-area.md".to_string())
        );
    }

    #[test]
    fn the_spellings_that_are_the_same_path_are_accepted() {
        let index = teksilo_corpus::index().unwrap();
        for spelling in [
            "./docs/scroll-area.md",
            "docs\\scroll-area.md",
            "/docs/scroll-area.md",
            "  docs/scroll-area.md  ",
        ] {
            assert_eq!(
                resolve_path(index, spelling),
                Resolved::Exact("docs/scroll-area.md".to_string()),
                "{spelling}"
            );
        }
    }

    #[test]
    fn an_unambiguous_abbreviation_is_rewritten_not_refused() {
        let index = teksilo_corpus::index().unwrap();
        assert_eq!(
            resolve_path(index, "scroll-area.md"),
            Resolved::Rewritten("docs/scroll-area.md".to_string())
        );
        assert_eq!(
            resolve_path(index, "DOCS/Scroll-Area.MD"),
            Resolved::Rewritten("docs/scroll-area.md".to_string())
        );
        assert_eq!(
            resolve_path(index, "simple_button/src/main.rs"),
            Resolved::Rewritten("examples/simple_button/src/main.rs".to_string())
        );
    }

    #[test]
    fn an_ambiguous_basename_lists_the_candidates_instead_of_picking_one() {
        // `main.rs` is every example's entry point. Guessing here would serve
        // a different document than the one asked for.
        let index = teksilo_corpus::index().unwrap();
        let Resolved::Unknown(help) = resolve_path(index, "main.rs") else {
            panic!("`main.rs` must not resolve to one document");
        };
        assert!(help.contains("matches"), "{help}");
        assert!(help.contains("examples/"), "{help}");
    }

    #[test]
    fn a_typo_suggests_the_real_path_rather_than_saying_not_found() {
        let index = teksilo_corpus::index().unwrap();
        let Resolved::Unknown(help) = resolve_path(index, "docs/scroll_area.md") else {
            panic!("a typo must not silently resolve");
        };
        assert!(
            help.contains("docs/scroll-area.md"),
            "no suggestion in:\n{help}"
        );
        assert!(help.contains("cargo teksilo search"), "{help}");
    }

    #[test]
    fn a_directory_lists_what_is_under_it() {
        let index = teksilo_corpus::index().unwrap();
        let Resolved::Unknown(help) = resolve_path(index, "examples/simple_button") else {
            panic!("a directory is not a document");
        };
        assert!(help.contains("is a directory"), "{help}");
        assert!(
            help.contains("examples/simple_button/src/main.rs"),
            "{help}"
        );
    }

    #[test]
    fn a_path_that_resembles_nothing_still_points_somewhere() {
        let index = teksilo_corpus::index().unwrap();
        let Resolved::Unknown(help) = resolve_path(index, "docs/qzxwvblargh.md") else {
            panic!("nonsense must not resolve");
        };
        assert!(help.contains("cargo teksilo show --list"), "{help}");
    }

    #[test]
    fn suggestions_are_bounded_and_best_first() {
        let paths = [
            "docs/scroll-area.md",
            "docs/scene-ink.md",
            "docs/settings.md",
            "docs/shortcut-intent-action.md",
            "docs/styling-system.md",
            "docs/table-view.md",
            "docs/telemetry.md",
        ];
        let got = suggestions(&paths, "docs/scroll-are.md");
        assert_eq!(got.first().map(String::as_str), Some("docs/scroll-area.md"));
        assert!(got.len() <= MAX_SUGGESTIONS);
    }

    #[test]
    fn levenshtein_is_a_metric_on_the_cases_that_matter() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("scroll-area", "scroll_area"), 1);
    }

    // -- the roster ----------------------------------------------------------

    #[test]
    fn the_listing_puts_guides_first_and_names_every_document_once() {
        let index = teksilo_corpus::index().unwrap();
        let docs = documents(index);
        let mut paths: Vec<&str> = docs.iter().map(|(_, p)| *p).collect();
        let before = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(before, paths.len(), "a path was listed twice");

        let first_example = docs.iter().position(|(kind, _)| *kind == "example");
        let last_guide = docs.iter().rposition(|(kind, _)| *kind == "guide");
        assert!(
            matches!((first_example, last_guide), (Some(e), Some(g)) if g < e),
            "guides and examples are interleaved"
        );
    }
}
