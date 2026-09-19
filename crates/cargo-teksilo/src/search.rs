// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `cargo teksilo search` — retrieval over the version-matched corpus.
//!
//! Two rankers over the same chunks, fused:
//!
//! * **BM25** over the interned term table. Exact, fast, no model, and the
//!   only ranker in a `--no-default-features` build. It is very good when the
//!   query happens to share vocabulary with the docs and structurally blind
//!   when it does not: "make a list scrollable" contains no token `scrollarea`.
//! * **Dense vectors**, when the corpus carries them and this build has an
//!   encoder. Encodes only the *query* — the chunk vectors are precomputed and
//!   shipped — and cosines it against the corpus.
//!
//! Their outputs are fused by [`reciprocal_rank_fusion`] rather than by adding
//! scores, because a BM25 score and a cosine are not the same kind of number:
//! BM25 is unbounded and corpus-dependent, cosine is `[-1, 1]`. Rank is the
//! one quantity both rankers produce that is directly comparable.
//!
//! # Three ways the vector path is refused, and why each is a refusal
//!
//! 1. **The corpus has no vectors** (`encoder` is `None`) — nothing to compare
//!    against.
//! 2. **The corpus's encoder is not this build's encoder.** This is the one
//!    that has to be checked rather than assumed: two unrelated encoders at
//!    384 dimensions produce cosines that are perfectly well-formed and
//!    semantically meaningless. It does not throw, it does not NaN, it does
//!    not look wrong — it just ranks badly, forever. See
//!    [`vector_availability`].
//! 3. **The encoder will not load** — an offline first run, a failed model
//!    fetch, a target ONNX Runtime has no binary for.
//!
//! In all three the search still answers, from BM25, and **says which mode it
//! used**. Returning lexical results while implying they are semantic is the
//! failure worth designing against: a caller that believes the vocabulary gap
//! is covered stops compensating for it.

use std::collections::HashMap;
use std::path::Path;

use teksilo_corpus::{Chunk, Index};

use crate::guard::{self, Verdict};
use crate::resolve;
use crate::vectors::{self, ENCODER_DIM, ENCODER_ID, Encoder};

/// BM25 term-frequency saturation. Robertson's own default, and the one
/// `build_corpus.py`'s statistics were computed for.
const K1: f64 = 1.2;

/// BM25 length normalisation. Also the standard default: the corpus mixes
/// ~40-line guide sections with ~10-line Rust items, so length normalisation
/// is doing real work here and turning it off would bias towards short items.
const B: f64 = 0.75;

/// Reciprocal-rank-fusion constant. 60 is the value from Cormack, Clarke &
/// Buettcher's original paper and the de-facto default since; it is large
/// enough that the top few ranks of a list do not dominate the fusion.
const RRF_K: f64 = 60.0;

/// How many candidates each ranker contributes to the fusion.
///
/// Wider than the output: a chunk that is rank 40 lexically and rank 2
/// semantically is exactly the result hybrid retrieval exists to surface, and
/// fusing only the top 8 of each list would have thrown it away.
const FUSION_DEPTH: usize = 100;

/// Characters of chunk text to show under a hit.
const SNIPPET_CHARS: usize = 240;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error(transparent)]
    Resolve(#[from] resolve::ResolveError),

    #[error("{0}")]
    Refused(String),

    #[error("{0}")]
    Usage(String),

    #[error("the embedded corpus could not be read: {0}")]
    Corpus(#[from] teksilo_corpus::CorpusError),
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// Which half of the corpus to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    Any,
    Guide,
    Example,
}

impl KindFilter {
    fn admits(self, chunk: &Chunk) -> bool {
        match self {
            KindFilter::Any => true,
            KindFilter::Guide => chunk.kind == "guide",
            KindFilter::Example => chunk.kind == "example",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SearchArgs {
    pub query: String,
    pub limit: usize,
    pub kind: KindFilter,
    pub lexical_only: bool,
}

const USAGE: &str = "\
USAGE: cargo teksilo search <QUERY> [--limit N] [--kind guide|example] [--lexical]

    --limit N              how many hits to print (default 8)
    --kind guide|example   restrict to the guides or to the worked examples
    --lexical              BM25 only; skip the vector path even if available";

/// Parse `search`'s arguments.
///
/// Every non-flag argument joins into the query, so an unquoted
/// `cargo teksilo search make a list scrollable` works rather than searching
/// for `make` and silently discarding the rest.
pub fn parse_args(args: &[String]) -> Result<SearchArgs, SearchError> {
    let mut words: Vec<&str> = Vec::new();
    let mut limit = 8usize;
    let mut kind = KindFilter::Any;
    let mut lexical_only = false;

    let mut it = args.iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--lexical" => lexical_only = true,
            "--limit" => {
                let value = it.next().ok_or_else(|| usage("--limit needs a number"))?;
                limit = parse_limit(value)?;
            }
            other if other.starts_with("--limit=") => {
                limit = parse_limit(&other["--limit=".len()..])?;
            }
            "--kind" => {
                let value = it.next().ok_or_else(|| usage("--kind needs a value"))?;
                kind = parse_kind(value)?;
            }
            other if other.starts_with("--kind=") => {
                kind = parse_kind(&other["--kind=".len()..])?;
            }
            "--help" | "-h" => return Err(usage("")),
            other if other.starts_with("--") => {
                return Err(usage(&format!("unknown flag `{other}`")));
            }
            other => words.push(other),
        }
    }

    let query = words.join(" ");
    if query.trim().is_empty() {
        return Err(usage("a query is required"));
    }
    Ok(SearchArgs {
        query,
        limit,
        kind,
        lexical_only,
    })
}

fn usage(problem: &str) -> SearchError {
    if problem.is_empty() {
        SearchError::Usage(USAGE.to_string())
    } else {
        SearchError::Usage(format!("{problem}\n\n{USAGE}"))
    }
}

fn parse_limit(value: &str) -> Result<usize, SearchError> {
    match value.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(usage(&format!(
            "--limit wants a positive number, got `{value}`"
        ))),
    }
}

fn parse_kind(value: &str) -> Result<KindFilter, SearchError> {
    match value {
        "guide" | "guides" => Ok(KindFilter::Guide),
        "example" | "examples" => Ok(KindFilter::Example),
        other => Err(usage(&format!(
            "--kind wants `guide` or `example`, got `{other}`"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Lexical ranking
// ---------------------------------------------------------------------------

/// Tokenize exactly as `tools/build_corpus.py` does: lowercase, split on runs
/// of anything outside `[a-z0-9_]`, drop single characters, no stemming.
///
/// It has to be exactly that, because the corpus's term table was built with
/// it: a query token that tokenizes differently here simply misses.
pub fn tokenize(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    lowered
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .filter(|t| t.len() > 1)
        .map(str::to_string)
        .collect()
}

/// BM25 over the corpus, restricted to `kind`.
///
/// Returns every chunk with a non-zero score, best first, ties broken by chunk
/// id so the ordering is total and a re-run prints the same thing.
pub fn bm25(index: &Index, query: &str, kind: KindFilter) -> Vec<(usize, f64)> {
    let total = index.chunks.len() as f64;
    let avgdl = if index.avgdl > 0.0 { index.avgdl } else { 1.0 };

    // Resolve the query to term indices once. A repeated query term counts
    // once: BM25 scores a term, not an occurrence of it in the query.
    let mut term_idf: Vec<(u32, f64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in tokenize(query) {
        let Some(term) = index.term_index(&token) else {
            continue;
        };
        if !seen.insert(term) {
            continue;
        }
        let df = f64::from(index.df.get(&term).copied().unwrap_or(0));
        // Lucene's smoothed IDF: never negative, even for a term in most of
        // the corpus, so a common word cannot penalise a chunk for having it.
        let idf = (1.0 + (total - df + 0.5) / (df + 0.5)).ln();
        term_idf.push((term, idf));
    }
    if term_idf.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f64)> = Vec::new();
    for chunk in &index.chunks {
        if !kind.admits(chunk) {
            continue;
        }
        let len_norm = K1 * (1.0 - B + B * f64::from(chunk.len) / avgdl);
        let mut score = 0.0;
        for &(term, idf) in &term_idf {
            let Some(&freq) = chunk.tokens.get(&term) else {
                continue;
            };
            let tf = f64::from(freq);
            score += idf * (tf * (K1 + 1.0)) / (tf + len_norm);
        }
        if score > 0.0 {
            scored.push((chunk.id, score));
        }
    }
    sort_by_score(&mut scored);
    scored
}

/// Cosine similarity against every vectorised chunk, restricted to `kind`.
pub fn vector_scores(index: &Index, query_vector: &[f32], kind: KindFilter) -> Vec<(usize, f64)> {
    let (quantised, scale) = vectors::quantise(query_vector);
    let mut scored: Vec<(usize, f64)> = index
        .chunks
        .iter()
        .filter(|chunk| kind.admits(chunk))
        .filter_map(|chunk| {
            let embedding = chunk.embedding.as_ref()?;
            let similarity =
                vectors::cosine_i8(&quantised, scale, &embedding.values, embedding.scale);
            Some((chunk.id, f64::from(similarity)))
        })
        .collect();
    sort_by_score(&mut scored);
    scored
}

/// Descending by score, ascending by id — a total order, so the output of two
/// identical runs is identical.
fn sort_by_score(scored: &mut [(usize, f64)]) {
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
}

// ---------------------------------------------------------------------------
// Fusion
// ---------------------------------------------------------------------------

/// Fuse ranked lists by reciprocal rank: `Σ 1 / (k + rank)`, rank 1-based.
///
/// A pure function over ranked id lists, deliberately knowing nothing about
/// what produced them. Rank rather than score because the inputs are not
/// commensurable — a BM25 score of 14.2 and a cosine of 0.71 cannot be added,
/// and normalising each list to `[0, 1]` would make the top hit of a list of
/// uniformly terrible results look as good as the top hit of a list of
/// excellent ones.
///
/// Ties break on id, so the result is a total order.
pub fn reciprocal_rank_fusion(lists: &[&[usize]], k: f64) -> Vec<(usize, f64)> {
    let mut fused: HashMap<usize, f64> = HashMap::new();
    for list in lists {
        for (position, &id) in list.iter().enumerate() {
            let rank = (position + 1) as f64;
            *fused.entry(id).or_insert(0.0) += 1.0 / (k + rank);
        }
    }
    let mut out: Vec<(usize, f64)> = fused.into_iter().collect();
    sort_by_score(&mut out);
    out
}

// ---------------------------------------------------------------------------
// The encoder-identity check
// ---------------------------------------------------------------------------

/// Whether this build may use the corpus's vectors, and why not when it may
/// not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorAvailability {
    /// The corpus's vectors were made by this build's encoder.
    Usable,
    /// The corpus has no vectors at all.
    NoVectors,
    /// The corpus's vectors came from a *different* encoder, or a different
    /// dimensionality of this one. Carries the message to print.
    Mismatch(String),
}

/// Compare the corpus's declared encoder against this build's.
///
/// Separated from everything else, and unit-tested, because it is the one
/// check here whose absence would not show up as a symptom — see the module
/// docs.
pub fn vector_availability(index: &Index) -> VectorAvailability {
    match (index.encoder.as_deref(), index.encoder_dim) {
        (None, _) => VectorAvailability::NoVectors,
        (Some(name), dim) if name == ENCODER_ID && dim == Some(ENCODER_DIM) => {
            VectorAvailability::Usable
        }
        (Some(name), dim) => VectorAvailability::Mismatch(format!(
            "note: this corpus was vectorised with {name} ({}), this build encodes \
             queries with {ENCODER_ID} ({ENCODER_DIM}). Refusing the vector path: \
             two encoders' vectors are comparable arithmetically and meaningless \
             semantically, so the failure would be silent. Falling back to lexical.",
            dim.map(|d| d.to_string())
                .unwrap_or_else(|| "no dimension declared".into()),
        )),
    }
}

// ---------------------------------------------------------------------------
// Running a search
// ---------------------------------------------------------------------------

/// Which ranker(s) actually produced the printed results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// BM25 fused with dense-vector cosine.
    Hybrid,
    /// BM25 alone.
    Lexical,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Hybrid => "hybrid (BM25 + vectors, reciprocal-rank fusion)",
            Mode::Lexical => "lexical (BM25)",
        }
    }
}

/// Run a search from the app directory `dir`.
pub fn run(dir: &Path, args: &[String]) -> Result<i32, SearchError> {
    let parsed = parse_args(args)?;
    let resolution = resolve::resolve(dir)?;

    let verdict = guard::check(&resolution.version);
    if !verdict.may_answer() {
        let Verdict::Refuse { app, tool } = &verdict else {
            unreachable!()
        };
        return Err(SearchError::Refused(guard::refusal_text(
            app,
            tool,
            "documentation search",
        )));
    }
    if let Some(note) = verdict.note() {
        eprintln!("{note}");
    }

    let index = teksilo_corpus::index()?;
    let lexical = bm25(index, &parsed.query, parsed.kind);
    let lexical_empty = lexical.is_empty();

    let (ranked, mode) = if parsed.lexical_only {
        (lexical, Mode::Lexical)
    } else {
        match semantic_ranking(index, &parsed.query, parsed.kind) {
            Some(dense) => {
                let lexical_ids: Vec<usize> = lexical
                    .iter()
                    .take(FUSION_DEPTH)
                    .map(|(id, _)| *id)
                    .collect();
                let dense_ids: Vec<usize> =
                    dense.iter().take(FUSION_DEPTH).map(|(id, _)| *id).collect();
                let fused = reciprocal_rank_fusion(&[&lexical_ids, &dense_ids], RRF_K);
                (fused, Mode::Hybrid)
            }
            None => (lexical, Mode::Lexical),
        }
    };

    // A hybrid search can never come back empty: the dense ranker cosines the
    // query against every chunk and always has a nearest one, however far
    // away. So the "no match" branch below is unreachable in hybrid mode, and
    // the honest thing is to say when the evidence is vector-only rather than
    // to invent a cosine floor and call the difference a threshold.
    print_results(index, &parsed, &ranked, mode, lexical_empty);
    Ok(0)
}

/// The dense half, or `None` with a printed reason.
///
/// Every `None` here is a degradation, never an error: the caller answers from
/// BM25 and says so.
fn semantic_ranking(index: &Index, query: &str, kind: KindFilter) -> Option<Vec<(usize, f64)>> {
    match vector_availability(index) {
        VectorAvailability::Usable => {}
        VectorAvailability::NoVectors => {
            eprintln!(
                "note: this corpus carries no vectors (run `cargo teksilo build-vectors`). \
                 Falling back to lexical."
            );
            return None;
        }
        VectorAvailability::Mismatch(message) => {
            eprintln!("{message}");
            return None;
        }
    }

    let mut encoder = match Encoder::load(false) {
        Ok(encoder) => encoder,
        Err(e) => {
            eprintln!("note: {e}\nFalling back to lexical.");
            return None;
        }
    };
    match encoder.embed_query(query) {
        Ok(vector) => Some(vector_scores(index, &vector, kind)),
        Err(e) => {
            eprintln!("note: {e}\nFalling back to lexical.");
            None
        }
    }
}

fn print_results(
    index: &Index,
    args: &SearchArgs,
    ranked: &[(usize, f64)],
    mode: Mode,
    lexical_empty: bool,
) {
    // The version is printed on every search, hit or miss: a result read out
    // of context is a claim about a particular teksilo, and an absence is
    // only ever an absence *from this corpus*.
    println!(
        "teksilo {} corpus · {}",
        index.teksilo_version,
        mode.label()
    );

    if ranked.is_empty() {
        // Deliberately not "no results": that reads as a fact about the
        // framework, and a model that reads it concludes the feature does not
        // exist. This scopes the absence to the index.
        println!(
            "no match in the {} corpus for {:?}",
            index.teksilo_version, args.query
        );
        return;
    }

    if lexical_empty && mode == Mode::Hybrid {
        println!(
            "note: no word of {:?} appears in any chunk searched, so these are \
             nearest-vector matches only — read them as leads, not as answers.",
            args.query
        );
    }

    for (rank, (id, score)) in ranked.iter().take(args.limit).enumerate() {
        let Some(chunk) = index.chunk(*id) else {
            continue;
        };
        let heading = if chunk.heading_path.is_empty() {
            "(preamble)".to_string()
        } else {
            chunk.heading_path.join(" › ")
        };
        println!();
        println!("{:>2}. [{:.4}] {}", rank + 1, score, chunk.path);
        println!(
            "    {heading}  (lines {}-{})",
            chunk.line_start + 1,
            chunk.line_end + 1
        );
        println!("    {}", snippet(chunk.text()));
    }
}

/// One line of chunk text, collapsed and cut at a character boundary.
fn snippet(text: &str) -> String {
    let collapsed: String = {
        let mut out = String::with_capacity(text.len().min(SNIPPET_CHARS * 2));
        let mut in_space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                in_space = true;
            } else {
                if in_space && !out.is_empty() {
                    out.push(' ');
                }
                in_space = false;
                out.push(c);
            }
            if out.chars().count() > SNIPPET_CHARS {
                break;
            }
        }
        out
    };
    if collapsed.chars().count() > SNIPPET_CHARS {
        let cut: String = collapsed.chars().take(SNIPPET_CHARS).collect();
        format!("{cut}…")
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use teksilo_corpus::ChunkEmbedding;

    fn arg(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_bare_word_joins_into_the_query() {
        // An unquoted query is the common agent mistake; silently searching
        // for its first word would be a wrong answer rather than an error.
        let parsed = parse_args(&arg(&["make", "a", "list", "scrollable"])).unwrap();
        assert_eq!(parsed.query, "make a list scrollable");
        assert_eq!(parsed.limit, 8);
        assert_eq!(parsed.kind, KindFilter::Any);
        assert!(!parsed.lexical_only);
    }

    #[test]
    fn flags_parse_in_both_spellings_and_anywhere() {
        let parsed = parse_args(&arg(&[
            "--kind=example",
            "drag",
            "--limit",
            "3",
            "--lexical",
        ]))
        .unwrap();
        assert_eq!(parsed.query, "drag");
        assert_eq!(parsed.limit, 3);
        assert_eq!(parsed.kind, KindFilter::Example);
        assert!(parsed.lexical_only);

        assert_eq!(parse_args(&arg(&["x", "--limit=2"])).unwrap().limit, 2);
        assert_eq!(
            parse_args(&arg(&["x", "--kind", "guide"])).unwrap().kind,
            KindFilter::Guide
        );
    }

    #[test]
    fn bad_arguments_are_rejected_with_the_usage() {
        for bad in [
            vec!["--limit", "0", "q"],
            vec!["--limit", "many", "q"],
            vec!["--kind", "widgets", "q"],
            vec!["--nope", "q"],
            vec![],
            vec!["   "],
        ] {
            let err = parse_args(&arg(&bad)).unwrap_err();
            assert!(matches!(err, SearchError::Usage(_)), "{bad:?} -> {err}");
        }
    }

    #[test]
    fn tokenize_matches_the_generators_rules() {
        // Same rules as tools/build_corpus.py's TOKEN_RE over text.lower():
        // lowercase, [a-z0-9_]+ runs, drop length-1 tokens.
        assert_eq!(tokenize("ScrollArea::new()"), vec!["scrollarea", "new"]);
        assert_eq!(tokenize("list_model"), vec!["list_model"]);
        assert_eq!(
            tokenize("a b cd"),
            vec!["cd"],
            "single characters are dropped"
        );
        assert_eq!(tokenize("Signal<T>"), vec!["signal"]);
        assert!(tokenize("— ·").is_empty());
    }

    /// A two-chunk corpus with a controlled vocabulary.
    fn toy_index(encoder: Option<&str>, dim: Option<u32>) -> Index {
        let terms: Vec<String> = ["list", "scroll", "scrollarea", "widget"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let chunk =
            |id: usize, kind: &str, tokens: &[(u32, u32)], embedding: Option<Vec<i8>>| Chunk {
                id,
                kind: kind.to_string(),
                path: format!("docs/{id}.md"),
                heading_path: vec![format!("H{id}")],
                text: format!("# H{id}"),
                line_start: 0,
                line_end: 0,
                crate_name: None,
                tokens: tokens.iter().copied().collect::<BTreeMap<u32, u32>>(),
                len: tokens.iter().map(|(_, n)| n).sum(),
                embedding: embedding.map(|values| ChunkEmbedding {
                    scale: 0.01,
                    values,
                }),
            };
        let mut df = BTreeMap::new();
        df.insert(0, 2);
        df.insert(1, 1);
        df.insert(2, 1);
        df.insert(3, 2);
        Index {
            schema: 1,
            teksilo_version: "0.12.1".into(),
            encoder: encoder.map(str::to_string),
            encoder_dim: dim,
            chunk_count: 2,
            avgdl: 4.0,
            terms,
            df,
            chunks: vec![
                chunk(0, "guide", &[(0, 3), (3, 1)], Some(vec![127, 0, 0])),
                chunk(
                    1,
                    "example",
                    &[(0, 1), (1, 2), (2, 1), (3, 1)],
                    Some(vec![0, 127, 0]),
                ),
            ],
        }
    }

    #[test]
    fn bm25_ranks_by_term_overlap_and_ignores_unknown_terms() {
        let index = toy_index(None, None);
        let ranked = bm25(&index, "scrollarea", KindFilter::Any);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, 1, "only chunk 1 has the term");
        assert!(ranked[0].1 > 0.0);

        // The vocabulary gap, stated as a test: this query shares no token
        // with the chunk that answers it, so BM25 alone cannot find it.
        assert!(bm25(&index, "make a thing slide", KindFilter::Any).is_empty());
    }

    #[test]
    fn bm25_respects_the_kind_filter() {
        let index = toy_index(None, None);
        assert_eq!(bm25(&index, "list", KindFilter::Guide).len(), 1);
        assert_eq!(bm25(&index, "list", KindFilter::Guide)[0].0, 0);
        assert_eq!(bm25(&index, "list", KindFilter::Example)[0].0, 1);
        assert_eq!(bm25(&index, "list", KindFilter::Any).len(), 2);
    }

    #[test]
    fn a_repeated_query_term_is_not_counted_twice() {
        let index = toy_index(None, None);
        let once = bm25(&index, "scroll", KindFilter::Any);
        let thrice = bm25(&index, "scroll scroll scroll", KindFilter::Any);
        assert_eq!(once, thrice);
    }

    #[test]
    fn vector_scores_rank_by_cosine_and_respect_the_filter() {
        let index = toy_index(Some(ENCODER_ID), Some(ENCODER_DIM));
        // Pointing at chunk 1's axis must rank chunk 1 first.
        let ranked = vector_scores(&index, &[0.0, 1.0, 0.0], KindFilter::Any);
        assert_eq!(ranked[0].0, 1);
        assert!(ranked[0].1 > 0.99 && ranked[1].1.abs() < 1e-6);
        assert_eq!(
            vector_scores(&index, &[0.0, 1.0, 0.0], KindFilter::Guide).len(),
            1
        );
    }

    #[test]
    fn a_matching_encoder_is_usable() {
        assert_eq!(
            vector_availability(&toy_index(Some(ENCODER_ID), Some(ENCODER_DIM))),
            VectorAvailability::Usable
        );
    }

    #[test]
    fn a_corpus_without_vectors_reports_that_rather_than_mismatching() {
        assert_eq!(
            vector_availability(&toy_index(None, None)),
            VectorAvailability::NoVectors
        );
    }

    #[test]
    fn a_different_encoder_at_the_same_dimension_is_refused() {
        // THE check. `all-MiniLM-L6-v2` is also 384-dimensional, so nothing
        // downstream would fail — the cosines would be well-formed and mean
        // nothing. Only the recorded identity can catch it.
        let index = toy_index(Some("sentence-transformers/all-MiniLM-L6-v2"), Some(384));
        let VectorAvailability::Mismatch(message) = vector_availability(&index) else {
            panic!("a foreign encoder at the same dimension must be refused");
        };
        assert!(message.contains("all-MiniLM-L6-v2"), "{message}");
        assert!(message.contains(ENCODER_ID), "{message}");
        assert!(
            message.contains("lexical"),
            "the message must name the fallback: {message}"
        );
    }

    #[test]
    fn the_right_encoder_at_the_wrong_dimension_is_refused() {
        assert!(matches!(
            vector_availability(&toy_index(Some(ENCODER_ID), Some(768))),
            VectorAvailability::Mismatch(_)
        ));
        assert!(matches!(
            vector_availability(&toy_index(Some(ENCODER_ID), None)),
            VectorAvailability::Mismatch(_)
        ));
    }

    #[test]
    fn fusion_rewards_agreement_between_the_two_rankers() {
        // 7 is second in both lists and beats 1, which is first in one and
        // absent from the other — the whole point of fusing.
        let lexical = [1usize, 7, 3];
        let dense = [9usize, 7, 4];
        let fused = reciprocal_rank_fusion(&[&lexical, &dense], RRF_K);
        assert_eq!(fused[0].0, 7);
        let score_of = |id: usize| fused.iter().find(|(i, _)| *i == id).unwrap().1;
        assert!((score_of(7) - (2.0 / 62.0)).abs() < 1e-12);
        assert!((score_of(1) - (1.0 / 61.0)).abs() < 1e-12);
    }

    #[test]
    fn fusion_surfaces_what_only_one_ranker_found() {
        // A chunk the lexical ranker never saw still places, which is the
        // vocabulary gap being closed.
        let lexical = [1usize, 2, 3];
        let dense = [42usize, 1, 2];
        let fused = reciprocal_rank_fusion(&[&lexical, &dense], RRF_K);
        assert!(fused.iter().any(|(id, _)| *id == 42));
        assert_eq!(
            fused[0].0, 1,
            "agreement still wins over a single first place"
        );
    }

    #[test]
    fn fusion_is_order_independent_and_total() {
        let a = [5usize, 6];
        let b = [6usize, 5];
        let one = reciprocal_rank_fusion(&[&a, &b], RRF_K);
        let other = reciprocal_rank_fusion(&[&b, &a], RRF_K);
        assert_eq!(
            one, other,
            "fusing the same lists in either order must agree"
        );
        // Equal scores, so the tie breaks on id and the order is total.
        assert_eq!(
            one.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![5, 6]
        );

        assert!(reciprocal_rank_fusion(&[], RRF_K).is_empty());
        assert!(reciprocal_rank_fusion(&[&[]], RRF_K).is_empty());
    }

    #[test]
    fn a_smaller_k_sharpens_the_fusion() {
        let lexical = [1usize, 2];
        let dense = [2usize, 1];
        let sharp = reciprocal_rank_fusion(&[&lexical, &dense], 0.0);
        // With k = 0 the reciprocals are 1 and 1/2 in both lists, so both
        // chunks still tie — the constant shifts weight, it does not reorder
        // agreement. Worth pinning so a future k change is a deliberate one.
        assert!((sharp[0].1 - sharp[1].1).abs() < 1e-12);
    }

    #[test]
    fn the_embedded_corpus_answers_a_lexical_query() {
        // An end-to-end check against the real shipped index, not a toy.
        let index = teksilo_corpus::index().unwrap();
        let ranked = bm25(index, "scrollarea", KindFilter::Any);
        assert!(
            !ranked.is_empty(),
            "the corpus must know what a ScrollArea is"
        );
        let top = index.chunk(ranked[0].0).unwrap();
        assert!(!top.text().is_empty());
    }

    #[test]
    fn a_query_of_pure_nonsense_returns_nothing_rather_than_everything() {
        let index = teksilo_corpus::index().unwrap();
        assert!(bm25(index, "zzqx_not_a_teksilo_term", KindFilter::Any).is_empty());
    }

    #[test]
    fn a_snippet_is_one_collapsed_line_cut_on_a_character_boundary() {
        assert_eq!(snippet("a\n  b\tc "), "a b c");
        let long = "é".repeat(SNIPPET_CHARS * 2);
        let cut = snippet(&long);
        assert!(cut.ends_with('…'));
        assert_eq!(cut.chars().count(), SNIPPET_CHARS + 1);
    }
}
