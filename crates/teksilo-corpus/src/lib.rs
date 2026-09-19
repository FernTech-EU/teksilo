// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The version-matched Teksilo documentation corpus.
//!
//! This crate is **data**, not logic: one prebuilt retrieval index over the
//! hand-written guides and the worked examples — their text, chunked, with
//! BM25 statistics and a dense vector per chunk — embedded at build time and
//! parsed on first use. It carries no ML dependency; the vectors are computed
//! at release time, so a consumer only ever encodes the *query*.
//!
//! It is versioned in lockstep with the framework, which is the whole point:
//! `cargo install cargo-teksilo --version X` resolves `teksilo-corpus X`
//! through cargo's own resolver, so an answer can never come from a
//! different teksilo than the one an app pinned.
//!
//! # The corpus is one file
//!
//! `corpus/index.json` is the whole crate's data, embedded by
//! [`INDEX_JSON`]. Every [`Chunk`] carries its own `text`, and its `path`
//! names the **original** file in the teksilo repository —
//! `docs/scroll-area.md`, `examples/simple_button/src/main.rs` — so a
//! search result cites something that exists and resolves on GitHub.
//!
//! It did not always work that way. An earlier layout mirrored all 158
//! guides and example sources into `corpus/` and stored only a line range
//! into the copy. That put a second copy of every guide in the tree, one
//! fuzzy-open away from the real one, and an edit made in the copy was
//! discarded without a word by the next regeneration. Moving the text into
//! the index was size-neutral — `index.json` grew by roughly what the
//! copies weighed — and int8-quantised vectors had already bought the
//! headroom that splitting the text out was originally for.
//!
//! [`Chunk::line_start`] / [`Chunk::line_end`] survive as **provenance**:
//! an inclusive, 0-based range naming where in `path` this text was found,
//! so a reader can be sent to the right lines. `tools/build_corpus.py`
//! verifies every range against its source file before emitting the index,
//! and the line split is [`str::lines`]' — on `\n` only, with a trailing
//! `\r` stripped — so Rust and the generator agree on what a line index
//! means.
//!
//! # Term strings are interned
//!
//! [`Index::terms`] is the sorted table of every token in the corpus;
//! [`Chunk::tokens`] and [`Index::df`] are keyed by a term's index into it
//! rather than repeating the string thousands of times. Resolve one with
//! [`Index::term`], or a whole chunk's with [`Index::chunk_terms`]. In
//! JSON those maps are objects with decimal string keys (`{"17":5}`),
//! since JSON object keys must be strings; serde deserializes them
//! straight into the `u32` keys used here.
//!
//! # The index is built in two passes, in this order
//!
//! 1. `python3 tools/build_corpus.py` chunks the guides and examples and
//!    writes `index.json` with `encoder`, `encoder_dim` and every
//!    [`Chunk::embedding`] as `null`. It regenerates the file **from
//!    scratch**, so it necessarily discards any vectors already there —
//!    except the ones it can carry forward by chunk-text hash, which is
//!    every vector whose text an edit did not touch.
//! 2. `cargo teksilo build-vectors` encodes each chunk, quantises the
//!    result, and writes the vectors back into that same `index.json`,
//!    setting `encoder` and `encoder_dim`.
//!
//! Running them in the other order leaves a corpus with no vectors, which
//! degrades to lexical search rather than breaking — but it is a
//! regression, so the order is part of the release procedure, and
//! `build_corpus.py --check` is deliberately blind to the three vector
//! fields: it checks that the *chunking* is current, which is the thing
//! that goes stale when a guide is edited.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The corpus index, embedded at build time.
///
/// `include_str!` rather than a directory embedding: the corpus is exactly
/// one file, and a macro that says so cannot quietly start shipping a
/// second one. It also makes "the file is missing" and "the file is not
/// UTF-8" compile errors rather than runtime variants of [`CorpusError`].
pub static INDEX_JSON: &str = include_str!("../corpus/index.json");

/// The teksilo version this corpus was generated from.
pub const CORPUS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A chunk's dense vector: int8 values plus the scale that decodes them.
///
/// # Why int8, and why a per-vector scale
///
/// The encoder emits 384 `f32`s per chunk. Stored as JSON numbers that is
/// ~4 MB for this corpus, which alone would push the crate past the 10 MB
/// crates.io limit. Stored as int8 it is 384 bytes, base64-encodes to 512
/// characters, and — because the encoder L2-normalises its output — loses
/// almost nothing: a vector's cosine against its own quantisation is
/// > 0.999 (see `cargo-teksilo`'s `vectors` tests, which assert it).
///
/// The scale is per-vector rather than corpus-wide because it is derived
/// from *that* vector's largest magnitude, so every vector uses the full
/// −127..=127 range regardless of how peaked it happens to be.
///
/// # JSON shape
///
/// ```json
/// "embedding": {"scale": 0.00512, "q": "ABn/…"}
/// ```
///
/// `q` is standard base64 (RFC 4648, padded) of the int8 values reinterpreted
/// as bytes, two's complement, in dimension order. Base64 rather than a JSON
/// array of decimal numbers: the array form is ~1600 bytes per chunk against
/// base64's 512, which is the difference between shipping and not shipping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkEmbedding {
    /// Multiplier that turns a stored int8 back into the encoder's `f32`.
    /// Always > 0; a zero vector is stored with `scale = 1.0` rather than
    /// with a zero scale, so decoding never divides by it.
    pub scale: f32,
    /// The quantised values, base64 in JSON under the key `q`.
    #[serde(rename = "q", with = "base64_i8")]
    pub values: Vec<i8>,
}

impl ChunkEmbedding {
    /// The approximate `f32` vector this was quantised from.
    ///
    /// Retrieval does not need this — a cosine can be taken on the int8
    /// values directly, and `cargo-teksilo` does exactly that — but a
    /// consumer doing its own vector maths wants the floats back.
    pub fn dequantised(&self) -> Vec<f32> {
        self.values
            .iter()
            .map(|&q| f32::from(q) * self.scale)
            .collect()
    }

    /// Number of dimensions, for checking against [`Index::encoder_dim`].
    pub fn dim(&self) -> usize {
        self.values.len()
    }
}

/// Base64 for the int8 payload, as a serde `with` module.
///
/// Hand-rolled rather than pulled in as a dependency: this crate is *data*
/// and its whole value is that a consumer can depend on it without
/// inheriting a tree. Standard alphabet, padded, so the field is readable
/// by any other tool without a private convention to look up.
mod base64_i8 {
    use serde::{Deserialize, Deserializer, Serializer};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// `ALPHABET` inverted: byte value -> 6-bit symbol, or 0xFF for "not a
    /// base64 character". Built at compile time so decoding is a table
    /// lookup rather than a search.
    const INVERSE: [u8; 256] = {
        let mut table = [0xFFu8; 256];
        let mut i = 0usize;
        while i < 64 {
            table[ALPHABET[i] as usize] = i as u8;
            i += 1;
        }
        table
    };

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for group in bytes.chunks(3) {
            let b0 = group[0] as u32;
            let b1 = *group.get(1).unwrap_or(&0) as u32;
            let b2 = *group.get(2).unwrap_or(&0) as u32;
            let packed = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(packed >> 18) as usize & 0x3F] as char);
            out.push(ALPHABET[(packed >> 12) as usize & 0x3F] as char);
            out.push(if group.len() > 1 {
                ALPHABET[(packed >> 6) as usize & 0x3F] as char
            } else {
                '='
            });
            out.push(if group.len() > 2 {
                ALPHABET[packed as usize & 0x3F] as char
            } else {
                '='
            });
        }
        out
    }

    pub fn decode(text: &str) -> Result<Vec<u8>, &'static str> {
        let body = text.trim_end_matches('=');
        if !text.len().is_multiple_of(4) || text.len() - body.len() > 2 {
            return Err("not a padded base64 string");
        }
        let mut out = Vec::with_capacity(body.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0u32;
        for &byte in body.as_bytes() {
            let symbol = INVERSE[byte as usize];
            if symbol == 0xFF {
                return Err("invalid base64 character");
            }
            acc = (acc << 6) | u32::from(symbol);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        Ok(out)
    }

    pub fn serialize<S: Serializer>(values: &[i8], serializer: S) -> Result<S::Ok, S::Error> {
        let bytes: Vec<u8> = values.iter().map(|&v| v as u8).collect();
        serializer.serialize_str(&encode(&bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<i8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = decode(&text).map_err(serde::de::Error::custom)?;
        Ok(bytes.into_iter().map(|b| b as i8).collect())
    }
}

/// One retrieval chunk of the corpus, as written by `tools/build_corpus.py`.
///
/// `crate_name` is `#[serde(rename = "crate")]` because `crate` is a Rust
/// keyword. `embedding` is written by a **second** pass —
/// `cargo teksilo build-vectors` — which runs after `build_corpus.py` and
/// fills it in for every chunk at once; see the crate-level docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    /// Position of this chunk in `Index::chunks`; stable within one
    /// generated corpus (i.e. one `teksilo_version`).
    pub id: usize,
    /// `"guide"` or `"example"`.
    pub kind: String,
    /// Path to the source file this chunk came from, **relative to the
    /// teksilo repository root** (e.g. `"docs/layout-primitives.md"` or
    /// `"examples/simple_button/src/main.rs"`). It is a real path in the
    /// repository, so it is safe to print as a citation — the crate itself
    /// does not ship the file, only this chunk's [`text`](Chunk::text).
    pub path: String,
    /// For a guide chunk, the open ancestor headings plus this chunk's own
    /// heading. For an example chunk, a one-element label (an item
    /// signature, or a `"lines A-B"` window descriptor for the windowed
    /// fallback).
    pub heading_path: Vec<String>,
    /// This chunk's text, verbatim — exactly what was tokenized into
    /// [`tokens`](Chunk::tokens) and encoded into
    /// [`embedding`](Chunk::embedding). [`Chunk::text`] is the borrowing
    /// accessor; the field is `pub` like every other one so a consumer can
    /// still build a `Chunk` of its own.
    pub text: String,
    /// Index of this chunk's first line in `path`, 0-based and inclusive.
    ///
    /// Provenance into the original file, not storage: the text is right
    /// there in [`Chunk::text`]. `tools/build_corpus.py` proves the range
    /// names that text before emitting the index.
    pub line_start: usize,
    /// Index of this chunk's last line in `path`, 0-based and **inclusive**.
    /// Equal to `line_start` for a one-line chunk.
    pub line_end: usize,
    /// The originating example crate's name, for an example chunk; `None`
    /// for a guide chunk.
    #[serde(rename = "crate")]
    pub crate_name: Option<String>,
    /// Per-chunk BM25 term frequencies, keyed by a term's index into
    /// [`Index::terms`] rather than by the term itself. Terms are
    /// lowercased, `[a-z0-9_]+`-tokenized, of length > 1, unstemmed.
    pub tokens: BTreeMap<u32, u32>,
    /// Sum of `tokens`' values — this chunk's length in tokens.
    pub len: u32,
    /// This chunk's dense vector, quantised — `None` in a corpus whose
    /// [`Index::encoder`] is `None`, `Some` in every chunk of one whose
    /// `encoder` is set. See [`ChunkEmbedding`].
    pub embedding: Option<ChunkEmbedding>,
}

impl Chunk {
    /// This chunk's text, borrowed out of the index.
    ///
    /// Infallible and free. It used to be an `Option<String>` joined out of
    /// a copy of the source file embedded beside the index; the text is now
    /// stored in the chunk itself, so there is nothing left to fail at and
    /// nothing to allocate.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The full retrieval index, embedded as [`INDEX_JSON`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    /// Format version of this index. Currently `2`.
    ///
    /// Bumped from `1` when the corpus stopped shipping copies of the guides
    /// and examples: a chunk gained its own `text`, and `path` changed from a
    /// corpus-internal name (`guides/foo.md`) to the real repository path
    /// (`docs/foo.md`). A version 1 index will not deserialize here — it has no
    /// `text` — which is the point of the field moving with the format.
    ///
    /// The crate and the corpus it embeds ship together, so a mismatch cannot
    /// arise from a normal install; the field is for anyone reading a stray
    /// `index.json` and needing to know what shape it is.
    pub schema: u32,
    /// The teksilo workspace version this corpus was generated from.
    /// Matches [`CORPUS_VERSION`] for a corpus built from this crate's own
    /// release, but is carried in the data too so a copy of `index.json`
    /// examined outside the crate is still self-describing.
    pub teksilo_version: String,
    /// Name of the dense-vector encoder that produced every
    /// [`Chunk::embedding`], e.g. `"BAAI/bge-small-en-v1.5"`; `None` in a
    /// corpus that has not had `cargo teksilo build-vectors` run over it.
    ///
    /// **A consumer must compare this against the encoder it is about to
    /// encode a query with, and refuse the vector path when they differ.**
    /// Two unrelated encoders at the same dimension yield perfectly
    /// well-formed cosines that mean nothing, so the failure is silent
    /// rather than loud — there is no runtime symptom to notice.
    pub encoder: Option<String>,
    /// Dimensionality of that encoding, and therefore of every
    /// [`ChunkEmbedding::values`]. `None` when `encoder` is.
    pub encoder_dim: Option<u32>,
    /// `chunks.len()`, kept as its own field so a consumer can sanity-check
    /// the index without materializing `chunks` first.
    pub chunk_count: usize,
    /// Mean chunk length in tokens across the whole corpus — BM25's
    /// `avgdl`.
    pub avgdl: f64,
    /// Every term in the corpus, **sorted**, interning the keys of
    /// [`Index::df`] and every [`Chunk::tokens`]. Sorted, so a query term
    /// can be binary-searched straight to its index — see
    /// [`Index::term_index`].
    pub terms: Vec<String>,
    /// Corpus-wide document frequency: for each term *index*, the number of
    /// chunks that term appears in at least once.
    pub df: BTreeMap<u32, u32>,
    /// Every chunk in the corpus, in a stable, deterministic order.
    pub chunks: Vec<Chunk>,
}

impl Index {
    /// The chunk with the given `id`, if one exists.
    ///
    /// Looks up by the `id` field rather than by vector position, so this
    /// stays correct even if a future corpus version doesn't lay `chunks`
    /// out in id order.
    pub fn chunk(&self, id: usize) -> Option<&Chunk> {
        self.chunks.iter().find(|chunk| chunk.id == id)
    }

    /// The text of the chunk with the given `id`, if one exists.
    ///
    /// Sugar for [`Index::chunk`] plus [`Chunk::text`]. `None` means no
    /// chunk carries that `id` — the text itself is never missing.
    pub fn chunk_text(&self, id: usize) -> Option<&str> {
        Some(self.chunk(id)?.text())
    }

    /// The term string an index in [`Chunk::tokens`] or [`Index::df`]
    /// refers to.
    ///
    /// Takes a `u32` to match those maps' key type, so iterating a chunk's
    /// tokens needs no cast.
    pub fn term(&self, idx: u32) -> Option<&str> {
        self.terms.get(idx as usize).map(String::as_str)
    }

    /// The index of `term` in [`Index::terms`], if the corpus contains it.
    ///
    /// A binary search — `terms` is sorted — so this is the cheap way to
    /// turn a tokenized query into the keys `df` and `tokens` speak.
    pub fn term_index(&self, term: &str) -> Option<u32> {
        self.terms
            .binary_search_by(|candidate| candidate.as_str().cmp(term))
            .ok()
            .map(|idx| idx as u32)
    }

    /// Corpus-wide document frequency of `term`, by string. `0` if the
    /// corpus never saw it.
    pub fn document_frequency(&self, term: &str) -> u32 {
        self.term_index(term)
            .and_then(|idx| self.df.get(&idx))
            .copied()
            .unwrap_or(0)
    }

    /// A chunk's term frequencies with the terms resolved back to strings,
    /// in ascending term order (which is ascending term-index order, since
    /// [`Index::terms`] is sorted).
    ///
    /// Any token index that does not name a term in this index is skipped;
    /// within one generated corpus there are none.
    pub fn chunk_terms<'a>(&'a self, chunk: &Chunk) -> Vec<(&'a str, u32)> {
        chunk
            .tokens
            .iter()
            .filter_map(|(&idx, &count)| Some((self.term(idx)?, count)))
            .collect()
    }
}

/// An error parsing the embedded corpus index.
///
/// One variant, and it names the only failure `include_str!` leaves
/// reachable: a missing file and non-UTF-8 bytes are both compile errors
/// now, so the variants that used to report them at runtime are gone.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum CorpusError {
    /// [`INDEX_JSON`]'s content is not valid per the [`Index`] schema.
    #[error("failed to parse index.json: {0}")]
    InvalidJson(String),
}

/// Parse and cache the embedded corpus index.
///
/// The first call parses [`INDEX_JSON`]; every later call returns the
/// cached result. Parsing is infallible in practice — an [`Err`] here
/// means the embedded index itself is malformed, which is a build-time
/// packaging bug in `teksilo-corpus` rather than a condition a caller can
/// recover from — but the [`Result`] is still surfaced rather than
/// panicking, since a corpus consumer may want to degrade gracefully
/// (e.g. an editor plugin falling back to no documentation search) instead
/// of crashing the host process.
pub fn index() -> Result<&'static Index, CorpusError> {
    static CACHE: OnceLock<Result<Index, CorpusError>> = OnceLock::new();
    let cached = CACHE.get_or_init(|| {
        serde_json::from_str::<Index>(INDEX_JSON)
            .map_err(|e| CorpusError::InvalidJson(e.to_string()))
    });
    match cached {
        Ok(index) => Ok(index),
        Err(e) => Err(e.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The teksilo checkout this crate lives in, when there is one.
    ///
    /// `Some` in the repository, `None` for a `cargo package` verification
    /// build or anyone testing the published crate from a registry copy —
    /// the corpus no longer carries the sources, so a test that wants to
    /// open one has to say what it does without them. Identified by the two
    /// directories the corpus is generated from rather than by
    /// `.git`/`Cargo.toml`, which a vendored copy could also have.
    fn repo_root() -> Option<std::path::PathBuf> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?
            .to_path_buf();
        (root.join("docs").is_dir() && root.join("examples").is_dir()).then_some(root)
    }

    #[test]
    fn index_parses() {
        index().expect("embedded index.json should parse");
    }

    #[test]
    fn chunk_count_matches_chunks_len() {
        let idx = index().unwrap();
        assert_eq!(idx.chunk_count, idx.chunks.len());
    }

    #[test]
    fn every_chunk_path_is_a_repo_path_and_really_exists_in_a_checkout() {
        // `path` is printed as a citation, so the claim to test is that it
        // is a path someone can open. Two tiers, because the corpus no
        // longer ships the file: the shape check runs everywhere (including
        // from a registry copy), and the existence check — the one that
        // would actually catch a generator writing `guides/…` again — runs
        // whenever the checkout is there to check against.
        let idx = index().unwrap();
        let root = repo_root();
        for chunk in &idx.chunks {
            let expected_prefix = match chunk.kind.as_str() {
                "guide" => "docs/",
                "example" => "examples/",
                other => panic!("chunk {} has an unknown kind {other:?}", chunk.id),
            };
            assert!(
                chunk.path.starts_with(expected_prefix),
                "chunk {} is a {} but its path is {:?}",
                chunk.id,
                chunk.kind,
                chunk.path
            );
            assert!(
                !chunk.path.contains("..") && !chunk.path.starts_with('/'),
                "chunk {} path must be repo-relative, got {:?}",
                chunk.id,
                chunk.path
            );
            if let Some(root) = &root {
                assert!(
                    root.join(&chunk.path).is_file(),
                    "chunk {} cites {}, which does not exist in this checkout",
                    chunk.id,
                    chunk.path
                );
            }
        }
    }

    #[test]
    fn df_is_non_empty() {
        let idx = index().unwrap();
        assert!(!idx.df.is_empty());
    }

    #[test]
    fn no_chunk_has_empty_text() {
        let idx = index().unwrap();
        for chunk in &idx.chunks {
            assert!(
                !chunk.text().trim().is_empty(),
                "chunk {} has empty text",
                chunk.id
            );
        }
    }

    #[test]
    fn chunk_text_looks_up_by_id() {
        let idx = index().unwrap();
        let first = &idx.chunks[0];
        assert_eq!(idx.chunk_text(first.id), Some(first.text()));
        assert_eq!(idx.chunk_text(usize::MAX), None);
        assert!(idx.chunk(usize::MAX).is_none());
    }

    #[test]
    fn every_chunk_line_range_reproduces_its_own_text_in_a_checkout() {
        // The provenance claim, checked the way a reader would check it:
        // open `path`, take `line_start..=line_end`, expect this chunk. The
        // range is no longer how the text is *stored*, which makes it
        // exactly the kind of field that can rot unnoticed — so it gets a
        // test rather than an assumption. The ordering half runs even
        // without a checkout.
        let idx = index().unwrap();
        for chunk in &idx.chunks {
            assert!(
                chunk.line_start <= chunk.line_end,
                "chunk {} has an inverted range {}..={}",
                chunk.id,
                chunk.line_start,
                chunk.line_end
            );
        }

        let Some(root) = repo_root() else {
            return;
        };
        let mut sources: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
        for chunk in &idx.chunks {
            let contents = sources
                .entry(chunk.path.as_str())
                .or_insert_with(|| std::fs::read_to_string(root.join(&chunk.path)).unwrap());
            let count = chunk.line_end - chunk.line_start + 1;
            let sliced: Vec<&str> = contents
                .lines()
                .skip(chunk.line_start)
                .take(count)
                .collect();
            assert_eq!(
                sliced.len(),
                count,
                "chunk {} ({}) claims lines {}..={} the file does not have",
                chunk.id,
                chunk.path,
                chunk.line_start,
                chunk.line_end
            );
            assert_eq!(
                sliced.join("\n"),
                chunk.text(),
                "chunk {} ({}) lines {}..={} do not reproduce its stored text",
                chunk.id,
                chunk.path,
                chunk.line_start,
                chunk.line_end
            );
        }
    }

    #[test]
    fn chunk_text_round_trips_a_guide_headings_own_line() {
        // A guide chunk's range must start exactly at the ATX heading that
        // named it. That pins the line range to the right place in the
        // file, which re-slicing with the same code could not.
        let idx = index().unwrap();
        let mut checked = 0usize;
        for chunk in &idx.chunks {
            let Some(title) = chunk.heading_path.last() else {
                continue;
            };
            if chunk.kind != "guide" {
                continue;
            }
            let first = chunk.text().lines().next().expect("non-empty chunk");
            assert!(
                first.starts_with('#'),
                "chunk {} ({}) should start at its heading, got {first:?}",
                chunk.id,
                chunk.path
            );
            assert!(
                first.contains(title.as_str()),
                "chunk {} ({}) heading line {first:?} does not contain {title:?}",
                chunk.id,
                chunk.path
            );
            checked += 1;
        }
        assert!(
            checked > 100,
            "expected many guide heading chunks, saw {checked}"
        );
    }

    #[test]
    fn terms_is_sorted_and_non_empty() {
        let idx = index().unwrap();
        assert!(!idx.terms.is_empty());
        assert!(
            idx.terms.windows(2).all(|w| w[0] < w[1]),
            "terms must be sorted and duplicate-free"
        );
        // …and therefore binary-searchable.
        let first = idx.terms.first().unwrap().clone();
        assert_eq!(idx.term_index(&first), Some(0));
        assert_eq!(idx.term_index("\u{1}not a corpus term"), None);
    }

    #[test]
    fn every_token_key_indexes_a_real_term() {
        let idx = index().unwrap();
        for &term_idx in idx.df.keys() {
            assert!(
                idx.term(term_idx).is_some(),
                "df key {term_idx} does not index a term (terms.len() = {})",
                idx.terms.len()
            );
        }
        for chunk in &idx.chunks {
            for &term_idx in chunk.tokens.keys() {
                assert!(
                    idx.term(term_idx).is_some(),
                    "chunk {} token key {term_idx} does not index a term",
                    chunk.id
                );
            }
            assert_eq!(
                idx.chunk_terms(chunk).len(),
                chunk.tokens.len(),
                "chunk {} lost terms resolving them to strings",
                chunk.id
            );
        }
    }

    #[test]
    fn base64_round_trips_every_byte_value() {
        // 256 distinct bytes exercises all four residues of len % 3 as the
        // slices below get truncated, and every symbol in the alphabet.
        let all: Vec<u8> = (0..=255u8).collect();
        for cut in [0usize, 1, 2, 3, 255, 256] {
            let slice = &all[..cut];
            let encoded = base64_i8::encode(slice);
            assert_eq!(
                encoded.len() % 4,
                0,
                "base64 must be padded to a multiple of 4"
            );
            assert_eq!(
                base64_i8::decode(&encoded).unwrap(),
                slice,
                "round trip at len {cut}"
            );
        }
    }

    #[test]
    fn base64_rejects_what_is_not_base64() {
        assert!(base64_i8::decode("AAA").is_err(), "unpadded length");
        assert!(base64_i8::decode("A!==").is_err(), "invalid character");
        assert!(base64_i8::decode("A===").is_err(), "over-padded");
    }

    #[test]
    fn an_embedding_round_trips_through_json_by_value() {
        let original = ChunkEmbedding {
            scale: 0.007_812_5,
            values: vec![-128, -1, 0, 1, 127],
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.contains("\"q\":\""),
            "values must serialize under `q`, got {json}"
        );
        assert!(
            !json.contains('['),
            "values must not serialize as an array, got {json}"
        );
        let back: ChunkEmbedding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
        assert_eq!(back.dim(), 5);
        assert_eq!(back.dequantised()[4], 127.0 * 0.007_812_5);
    }

    #[test]
    fn vectors_are_all_present_or_all_absent_and_agree_with_the_header() {
        // Half a corpus of vectors would silently rank the vectorless half
        // last forever, so the invariant is all-or-nothing.
        let idx = index().unwrap();
        let with_vectors = idx.chunks.iter().filter(|c| c.embedding.is_some()).count();
        match (&idx.encoder, idx.encoder_dim) {
            (None, None) => assert_eq!(with_vectors, 0, "vectors present with no encoder named"),
            (Some(name), Some(dim)) => {
                assert!(!name.is_empty());
                assert_eq!(
                    with_vectors,
                    idx.chunks.len(),
                    "every chunk must carry a vector"
                );
                for chunk in &idx.chunks {
                    let e = chunk.embedding.as_ref().unwrap();
                    assert_eq!(
                        e.dim(),
                        dim as usize,
                        "chunk {} has the wrong dimension",
                        chunk.id
                    );
                    assert!(e.scale > 0.0, "chunk {} has a non-positive scale", chunk.id);
                }
            }
            other => panic!("encoder and encoder_dim must be set together, got {other:?}"),
        }
    }

    #[test]
    fn document_frequency_resolves_by_string() {
        let idx = index().unwrap();
        // Pick a term the corpus certainly has, via the table itself, so
        // this does not depend on any particular doc surviving an edit.
        let sample = idx.terms.iter().max_by_key(|t| t.len()).unwrap();
        let sample_idx = idx.term_index(sample).expect("term is in terms");
        assert_eq!(idx.document_frequency(sample), idx.df[&sample_idx]);
        assert!(idx.document_frequency(sample) > 0);
        assert_eq!(idx.document_frequency("\u{1}not a corpus term"), 0);
    }
}
